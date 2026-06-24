use std::{
    collections::VecDeque,
    io,
    io::{BufRead, Write},
};

use hibana_wasip1_runtime::{
    choreofs::{ChoreoFs, ChoreoFsObject, ChoreoFsObjectSet, ChoreoFsWrite, FdSpec, ObjectId},
    exchange::FdBindingTable,
    protocol::{
        self, FdBinding, FdClosed, FdPrestat, FdPrestatDirNameDone, FdReadDone, FdReadRow,
        FdReaddirDone, FdReaddirRow, FdRequest, FdStat, FdWriteDone, FdWriteRow, FileStat,
        MemRights, WASIP1_FILETYPE_DIRECTORY, WASIP1_FILETYPE_REGULAR_FILE,
        WASIP1_IO_CHUNK_CAPACITY,
    },
};

use crate::wasi_shell_demo_lib::error::DemoResult;

const ROOT_FD: u8 = 3;
const OBJECTS_DIR_FD: u8 = 4;
const OBJECT_LOG_FD: u8 = 5;
const LED_GREEN_FD: u8 = 6;

const OBJECTS_DIR_ID: ObjectId = ObjectId(1);
const OBJECT_LOG_ID: ObjectId = ObjectId(2);
const LED_GREEN_ID: ObjectId = ObjectId(3);

const FD_READ_RIGHT: u64 = 1 << 1;
const FD_WRITE_RIGHT: u64 = 1 << 6;
const FD_READDIR_RIGHT: u64 = 1 << 14;

const ERRNO_SUCCESS: u16 = 0;
const ERRNO_ACCES: u16 = 2;
const ERRNO_BADF: u16 = 8;
const ERRNO_NOENT: u16 = 44;

const ROOT_PREOPEN_NAME: &[u8] = b"/";
const OBJECT_LOG_BYTES: &[u8] = b"session=attached\n";
const OBJECTS_DIRENT_LOG: &[u8] = &[
    1, 0, 0, 0, 0, 0, 0, 0, // d_next
    1, 0, 0, 0, 0, 0, 0, 0, // d_ino
    3, 0, 0, 0, // d_namlen
    4, // d_type: regular file
    0, 0, 0, // padding
    b'l', b'o', b'g',
];

static CHOREOFS_OBJECTS: ChoreoFsObjectSet<3> = ChoreoFsObjectSet::new([
    ChoreoFsObject::readdir(
        b"objects",
        OBJECTS_DIR_ID,
        FdSpec::new(OBJECTS_DIR_FD as u32, FD_READDIR_RIGHT, 1),
        OBJECTS_DIRENT_LOG,
        FdBinding::readdir(FdReaddirRow::Base),
    ),
    ChoreoFsObject::readable(
        b"objects/log",
        OBJECT_LOG_ID,
        FdSpec::new(OBJECT_LOG_FD as u32, FD_READ_RIGHT, 1),
        OBJECT_LOG_BYTES,
        FdBinding::read(FdReadRow::Base),
    ),
    ChoreoFsObject::writable(
        b"outputs/led/green",
        LED_GREEN_ID,
        FdSpec::new(LED_GREEN_FD as u32, FD_WRITE_RIGHT, 1),
        FdBinding::write(FdWriteRow::Refined),
    ),
]);

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
            entries: initial_entries(),
            input: VecDeque::new(),
            led_green: false,
        }
    }

    pub fn led_green(&self) -> bool {
        self.led_green
    }

    pub fn write_fd(&mut self, write: protocol::FdWrite) -> protocol::FdWriteDoneRet {
        match self.entries.iter().find(|entry| entry.fd == write.fd()) {
            Some(FdEntry {
                kind: FdKind::Output,
                ..
            }) => {
                let _ = io::stdout().write_all(write.as_bytes());
                let _ = io::stdout().flush();
                protocol::FdWriteDoneRet(FdWriteDone::new(write.fd(), bounded_u8(write.len())))
            }
            Some(FdEntry {
                kind: FdKind::ErrorOutput,
                ..
            }) => {
                let _ = io::stderr().write_all(write.as_bytes());
                let _ = io::stderr().flush();
                protocol::FdWriteDoneRet(FdWriteDone::new(write.fd(), bounded_u8(write.len())))
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

    pub fn open_path(&mut self, open: protocol::PathOpen) -> DemoResult<protocol::PathOpenedRet> {
        let normalized = normalize_choreofs_path(open.path());
        let open = protocol::PathOpen::new(open.preopen_fd(), open.rights_base(), normalized)?;
        let operation = self.choreofs.path_open(open);
        if let (Some(fd), Some(object)) = (operation.fd(), operation.object()) {
            self.materialize_choreofs_fd(fd, object);
        }
        Ok(operation.opened_ret())
    }

    pub fn prestat_fd(&self, request: FdRequest) -> protocol::FdPrestatRet {
        if request.fd() == ROOT_FD {
            protocol::FdPrestatRet(FdPrestat::new(
                request.fd(),
                bounded_u8(ROOT_PREOPEN_NAME.len()),
            ))
        } else {
            protocol::FdPrestatRet(FdPrestat::new_with_errno(request.fd(), 0, ERRNO_BADF))
        }
    }

    pub fn prestat_dir_name(
        &self,
        request: protocol::FdPrestatDirName,
    ) -> DemoResult<protocol::FdPrestatDirNameRet> {
        if request.fd() != ROOT_FD {
            return Ok(protocol::FdPrestatDirNameRet(FdPrestatDirNameDone::new(
                request.fd(),
                b"",
                ERRNO_BADF,
            )?));
        }
        Ok(protocol::FdPrestatDirNameRet(FdPrestatDirNameDone::new(
            request.fd(),
            ROOT_PREOPEN_NAME,
            ERRNO_SUCCESS,
        )?))
    }

    pub fn file_stat_fd(&self, request: FdRequest) -> protocol::FdFilestatRet {
        let Some(entry) = self.entries.iter().find(|entry| entry.fd == request.fd()) else {
            return protocol::FdFilestatRet(FileStat::new_with_errno(0, 0, ERRNO_BADF));
        };
        protocol::FdFilestatRet(entry.file_stat())
    }

    pub fn file_stat_path(&self, request: protocol::PathFilestatGet) -> protocol::PathFilestatRet {
        match self
            .choreofs
            .facts()
            .resolve(normalize_choreofs_path(request.path()))
        {
            Some(OBJECTS_DIR_ID) => {
                protocol::PathFilestatRet(FileStat::new(WASIP1_FILETYPE_DIRECTORY, 0))
            }
            Some(OBJECT_LOG_ID) => protocol::PathFilestatRet(FileStat::new(
                WASIP1_FILETYPE_REGULAR_FILE,
                OBJECT_LOG_BYTES.len() as u64,
            )),
            Some(LED_GREEN_ID) => {
                protocol::PathFilestatRet(FileStat::new(WASIP1_FILETYPE_REGULAR_FILE, 1))
            }
            _ => protocol::PathFilestatRet(FileStat::new_with_errno(0, 0, ERRNO_NOENT)),
        }
    }

    pub fn read_dir_fd(
        &mut self,
        read: protocol::FdReaddir,
    ) -> DemoResult<protocol::FdReaddirDoneRet> {
        let Some(entry) = self.entries.iter().find(|entry| entry.fd == read.fd()) else {
            return Ok(protocol::FdReaddirDoneRet(FdReaddirDone::new(
                read.fd(),
                b"",
                ERRNO_BADF,
            )?));
        };
        match &entry.kind {
            FdKind::Preopen => Ok(protocol::FdReaddirDoneRet(FdReaddirDone::new(
                read.fd(),
                b"",
                ERRNO_SUCCESS,
            )?)),
            FdKind::Dir => Ok(self.choreofs.fd_readdir(read).read_dir()?),
            _ => Ok(protocol::FdReaddirDoneRet(FdReaddirDone::new(
                read.fd(),
                b"",
                ERRNO_BADF,
            )?)),
        }
    }

    pub fn read_fd(&mut self, read: protocol::FdRead) -> DemoResult<protocol::FdReadDoneRet> {
        let Some(index) = self.entries.iter().position(|entry| entry.fd == read.fd()) else {
            return Ok(protocol::FdReadDoneRet(FdReadDone::new(read.fd(), b"")?));
        };
        if matches!(self.entries[index].kind, FdKind::Input) {
            return self.read_terminal_input(read);
        }
        match &mut self.entries[index].kind {
            FdKind::Object { cursor } => {
                let (response, next_cursor) = self.choreofs.fd_read(read).read_from(*cursor)?;
                *cursor = next_cursor;
                Ok(response)
            }
            _ => Ok(protocol::FdReadDoneRet(FdReadDone::new(read.fd(), b"")?)),
        }
    }

    pub fn stat_fd(&self, request: FdRequest) -> protocol::FdStatRet {
        let fd = request.fd();
        let rights = match self.entries.iter().find(|entry| entry.fd == fd) {
            Some(FdEntry {
                kind: FdKind::Output | FdKind::ErrorOutput | FdKind::OutputLedGreen,
                ..
            }) => MemRights::Write,
            _ => MemRights::Read,
        };
        protocol::FdStatRet(FdStat::new(fd, rights))
    }

    pub fn close_fd(&mut self, request: FdRequest) -> protocol::FdClosedRet {
        let fd = request.fd();
        self.entries.retain(|entry| entry.fd != fd);
        protocol::FdClosedRet(FdClosed::new(fd))
    }

    pub fn flush_output(&mut self) {
        let _ = io::stdout().flush();
        let _ = io::stderr().flush();
    }

    fn apply_led_green_write(&mut self, write: ChoreoFsWrite) -> protocol::FdWriteDoneRet {
        match write.bytes() {
            b"1" => {
                self.led_green = true;
                write.written()
            }
            b"0" => {
                self.led_green = false;
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

    fn read_terminal_input(
        &mut self,
        read: protocol::FdRead,
    ) -> DemoResult<protocol::FdReadDoneRet> {
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

    fn materialize_choreofs_fd(&mut self, fd: u8, object: ObjectId) {
        self.entries.retain(|entry| entry.fd != fd);
        match object {
            OBJECTS_DIR_ID => self.entries.push(FdEntry::dir(fd)),
            OBJECT_LOG_ID => self.entries.push(FdEntry::object(fd)),
            LED_GREEN_ID => self.entries.push(FdEntry::output_led_green(fd)),
            _ => {}
        }
    }
}

pub fn initial_bindings() -> FdBindingTable {
    let mut bindings = FdBindingTable::empty();
    let _ = bindings.bind_fd(0, FdBinding::read(FdReadRow::Base));
    let _ = bindings.bind_fd(1, FdBinding::write(FdWriteRow::Base));
    let _ = bindings.bind_fd(2, FdBinding::write(FdWriteRow::Base));
    let _ = bindings.bind_fd(ROOT_FD, FdBinding::readdir(FdReaddirRow::Base));
    bindings
}

fn choreofs() -> ChoreoFs<'static> {
    CHOREOFS_OBJECTS.choreofs()
}

fn bounded_u8(value: impl TryInto<usize>) -> u8 {
    let value = match value.try_into() {
        Ok(value) => value,
        Err(_) => WASIP1_IO_CHUNK_CAPACITY,
    };
    value.min(WASIP1_IO_CHUNK_CAPACITY) as u8
}

fn normalize_choreofs_path(path: &[u8]) -> &[u8] {
    match path.first() {
        Some(b'/') => &path[1..],
        _ => path,
    }
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

    const fn dir(fd: u8) -> Self {
        Self {
            fd,
            kind: FdKind::Dir,
        }
    }

    const fn object(fd: u8) -> Self {
        Self {
            fd,
            kind: FdKind::Object { cursor: 0 },
        }
    }

    const fn output_led_green(fd: u8) -> Self {
        Self {
            fd,
            kind: FdKind::OutputLedGreen,
        }
    }

    fn file_stat(&self) -> FileStat {
        match &self.kind {
            FdKind::Preopen => FileStat::new(WASIP1_FILETYPE_DIRECTORY, 0),
            FdKind::Dir => FileStat::new(WASIP1_FILETYPE_DIRECTORY, 0),
            FdKind::Object { .. } => {
                FileStat::new(WASIP1_FILETYPE_REGULAR_FILE, OBJECT_LOG_BYTES.len() as u64)
            }
            FdKind::OutputLedGreen => FileStat::new(WASIP1_FILETYPE_REGULAR_FILE, 1),
            FdKind::Input | FdKind::Output | FdKind::ErrorOutput => {
                FileStat::new(WASIP1_FILETYPE_REGULAR_FILE, 0)
            }
        }
    }
}

enum FdKind {
    Input,
    Output,
    ErrorOutput,
    Preopen,
    Dir,
    Object { cursor: usize },
    OutputLedGreen,
}

fn initial_entries() -> Vec<FdEntry> {
    vec![
        FdEntry::input(),
        FdEntry::output(),
        FdEntry::error_output(),
        FdEntry::preopen(),
    ]
}
