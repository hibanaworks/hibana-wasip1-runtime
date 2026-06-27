use std::{
    collections::VecDeque,
    io,
    io::{BufRead, Write},
};

use hibana_wasip1_runtime::{
    choreofs::{ChoreoFs, ChoreoFsObject, ChoreoFsObjectSet, ChoreoFsWrite, FdSpec, ObjectId},
    exchange::FdBindingTable,
    protocol::{
        self, FdBinding, FdReadDone, FdReadRow, FdReaddirRow, FdRequest, FdStat, FdWriteDone,
        FdWriteRow, MemRights,
    },
};

use crate::wasi_shell_demo_lib::error::DemoResult;

const ROOT_FD: u8 = 3;
const LED_GREEN_FD: u8 = 4;
const LED_GREEN_ID: ObjectId = ObjectId(1);
const FD_WRITE_RIGHT: u64 = 1 << 6;

const ERRNO_ACCES: u16 = 2;
const ERRNO_BADF: u16 = 8;

static CHOREOFS_OBJECTS: ChoreoFsObjectSet<1> = ChoreoFsObjectSet::new([ChoreoFsObject::writable(
    b"outputs/led/green",
    LED_GREEN_ID,
    FdSpec::new(LED_GREEN_FD as u32, FD_WRITE_RIGHT, 1),
    FdBinding::write(FdWriteRow::Object),
)]);

pub struct ShellEnv {
    choreofs: ChoreoFs<'static>,
    entries: Vec<FdEntry>,
    input: VecDeque<u8>,
    led_green: bool,
}

impl ShellEnv {
    pub fn new() -> Self {
        Self {
            choreofs: choreofs(),
            entries: vec![
                FdEntry::input(),
                FdEntry::output(),
                FdEntry::error_output(),
                FdEntry::preopen(),
            ],
            input: VecDeque::new(),
            led_green: false,
        }
    }

    pub fn led_green(&self) -> bool {
        self.led_green
    }

    pub fn flush_output(&mut self) {
        let _ = io::stdout().flush();
        let _ = io::stderr().flush();
    }

    pub fn write_fd(&mut self, write: protocol::FdWrite) -> protocol::FdWriteDoneRet {
        match self.entries.iter().find(|entry| entry.fd == write.fd()) {
            Some(FdEntry {
                kind: FdKind::Output,
                ..
            }) => {
                let _ = io::stdout().write_all(write.as_bytes());
                let _ = io::stdout().flush();
                protocol::FdWriteDoneRet(FdWriteDone::new(write.fd(), write.len() as u8))
            }
            Some(FdEntry {
                kind: FdKind::ErrorOutput,
                ..
            }) => {
                let _ = io::stderr().write_all(write.as_bytes());
                let _ = io::stderr().flush();
                protocol::FdWriteDoneRet(FdWriteDone::new(write.fd(), write.len() as u8))
            }
            Some(FdEntry {
                kind: FdKind::OutputLedGreen,
                ..
            }) => {
                let write = self.choreofs.fd_write(write);
                self.apply_led_green_write(write)
            }
            Some(_) => self.choreofs.fd_write(write).written(),
            None => {
                protocol::FdWriteDoneRet(FdWriteDone::new_with_errno(write.fd(), 0, ERRNO_BADF))
            }
        }
    }

    pub fn read_fd(&mut self, read: protocol::FdRead) -> DemoResult<protocol::FdReadDoneRet> {
        if !matches!(
            self.entries.iter().find(|entry| entry.fd == read.fd()),
            Some(FdEntry {
                kind: FdKind::Input,
                ..
            })
        ) {
            return Ok(protocol::FdReadDoneRet(FdReadDone::new_with_errno(
                read.fd(),
                b"",
                ERRNO_BADF,
            )?));
        }
        if self.input.is_empty() {
            let mut line = String::new();
            io::stdin().lock().read_line(&mut line)?;
            self.input.extend(line.into_bytes());
        }
        let len = self.input.len().min(read.max_len() as usize);
        let mut out = Vec::with_capacity(len);
        for _ in 0..len {
            if let Some(byte) = self.input.pop_front() {
                out.push(byte);
            }
        }
        Ok(protocol::FdReadDoneRet(FdReadDone::new(read.fd(), &out)?))
    }

    pub fn stat_fd(&self, request: FdRequest) -> protocol::FdStatRet {
        let Some(entry) = self.entries.iter().find(|entry| entry.fd == request.fd()) else {
            return protocol::FdStatRet(FdStat::new_with_errno(
                request.fd(),
                MemRights::Read,
                ERRNO_BADF,
            ));
        };
        let rights = match entry {
            FdEntry {
                kind: FdKind::Output | FdKind::ErrorOutput | FdKind::OutputLedGreen,
                ..
            } => MemRights::Write,
            _ => MemRights::Read,
        };
        protocol::FdStatRet(FdStat::new(request.fd(), rights))
    }

    pub fn open_path(&mut self, open: protocol::PathOpen) -> DemoResult<protocol::PathOpenedRet> {
        let normalized = normalize_choreofs_path(open.path());
        let open = protocol::PathOpen::new(open.preopen_fd(), open.rights_base(), normalized)?;
        let operation = self.choreofs.path_open(open);
        if let (Some(fd), Some(object)) = (operation.fd(), operation.object()) {
            self.entries.retain(|entry| entry.fd != fd);
            if object == LED_GREEN_ID {
                self.entries.push(FdEntry::output_led_green(fd));
            }
        }
        Ok(operation.opened_ret())
    }

    fn apply_led_green_write(&mut self, write: ChoreoFsWrite) -> protocol::FdWriteDoneRet {
        match write.bytes() {
            b"1" => {
                self.led_green = true;
                write.written()
            }
            b"\n" => write.written(),
            _ => protocol::FdWriteDoneRet(FdWriteDone::new_with_errno(
                write.request().fd(),
                0,
                ERRNO_ACCES,
            )),
        }
    }
}

pub fn initial_bindings() -> FdBindingTable {
    let mut bindings = FdBindingTable::empty();
    bindings
        .bind_fd(0, FdBinding::read(FdReadRow::Base))
        .expect("stdin fd fits binding table");
    bindings
        .bind_fd(1, FdBinding::write(FdWriteRow::Base))
        .expect("stdout fd fits binding table");
    bindings
        .bind_fd(2, FdBinding::write(FdWriteRow::Base))
        .expect("stderr fd fits binding table");
    bindings
        .bind_fd(ROOT_FD, FdBinding::readdir(FdReaddirRow::Base))
        .expect("root fd fits binding table");
    bindings
}

struct FdEntry {
    fd: u8,
    kind: FdKind,
}

impl FdEntry {
    const fn input() -> Self {
        Self {
            fd: 0,
            kind: FdKind::Input,
        }
    }

    const fn output() -> Self {
        Self {
            fd: 1,
            kind: FdKind::Output,
        }
    }

    const fn error_output() -> Self {
        Self {
            fd: 2,
            kind: FdKind::ErrorOutput,
        }
    }

    const fn preopen() -> Self {
        Self {
            fd: ROOT_FD,
            kind: FdKind::Preopen,
        }
    }

    const fn output_led_green(fd: u8) -> Self {
        Self {
            fd,
            kind: FdKind::OutputLedGreen,
        }
    }
}

enum FdKind {
    Input,
    Output,
    ErrorOutput,
    Preopen,
    OutputLedGreen,
}

fn choreofs() -> ChoreoFs<'static> {
    CHOREOFS_OBJECTS.choreofs()
}

fn normalize_choreofs_path(path: &[u8]) -> &[u8] {
    match path.first() {
        Some(b'/') => &path[1..],
        _ => path,
    }
}
