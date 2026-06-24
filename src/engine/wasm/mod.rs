//! Private Wasm/WASI P1 engine facade.
//!
//! The engine boundary has one handle: [`Guest`]. The parser, interpreter,
//! import lowering, memory writeback, and pending slot live in `machine`.

mod machine;

use crate::protocol::{BudgetExpired, BudgetRun, ProcExitStatus};

pub use machine::{
    DEFAULT_GUEST_MEMORY_BYTES, FdStat, FileStat, GUEST_MEMORY_PAGE_SIZE, GuestMemory,
    ImportPlanDiagnostics, PathBytes,
};

pub type Error = machine::WasmError;

pub struct Guest<'a> {
    engine: machine::Vm<'a>,
}

impl<'a> Guest<'a> {
    /// # Safety
    ///
    /// `dst` must be valid for writes, properly aligned for `Guest<'a>`, and
    /// must not be read until this function returns `Ok(())`.
    pub unsafe fn init_in_place(
        dst: *mut Self,
        module: &'a [u8],
        memory: GuestMemory<'a>,
    ) -> Result<(), Error> {
        unsafe {
            machine::Vm::init_in_place(core::ptr::addr_of_mut!((*dst).engine), module, memory)?;
        }
        Ok(())
    }

    pub fn resume(&mut self, budget: BudgetRun) -> Result<Event, Error> {
        match self.engine.resume(budget) {
            Ok(machine::VmEvent::FdWrite(call)) => Ok(Event::Call(Call::FdWrite(FdWrite { call }))),
            Ok(machine::VmEvent::FdRead(call)) => Ok(Event::Call(Call::FdRead(FdRead { call }))),
            Ok(machine::VmEvent::FdFdstatGet(call)) => {
                Ok(Event::Call(Call::FdFdstatGet(FdFdstatGet { call })))
            }
            Ok(machine::VmEvent::FdPrestatGet(call)) => {
                Ok(Event::Call(Call::FdPrestatGet(FdPrestatGet { call })))
            }
            Ok(machine::VmEvent::FdPrestatDirName(call)) => {
                Ok(Event::Call(Call::FdPrestatDirName(FdPrestatDirName {
                    call,
                })))
            }
            Ok(machine::VmEvent::FdFilestatGet(call)) => {
                Ok(Event::Call(Call::FdFilestatGet(FdFilestatGet { call })))
            }
            Ok(machine::VmEvent::FdClose(call)) => Ok(Event::Call(Call::FdClose(FdClose { call }))),
            Ok(machine::VmEvent::ClockResGet(call)) => {
                Ok(Event::Call(Call::ClockResGet(ClockResGet { call })))
            }
            Ok(machine::VmEvent::ClockTimeGet(call)) => {
                Ok(Event::Call(Call::ClockTimeGet(ClockTimeGet { call })))
            }
            Ok(machine::VmEvent::PollOneoff(call)) => {
                Ok(Event::Call(Call::PollOneoff(PollOneoff { call })))
            }
            Ok(machine::VmEvent::RandomGet(call)) => {
                Ok(Event::Call(Call::RandomGet(RandomGet { call })))
            }
            Ok(machine::VmEvent::FdReaddir(call)) => {
                Ok(Event::Call(Call::FdReaddir(FdReaddir { call })))
            }
            Ok(machine::VmEvent::PathOpen(call)) => {
                Ok(Event::Call(Call::PathOpen(PathOpen { call })))
            }
            Ok(machine::VmEvent::PathFilestatGet(call)) => {
                Ok(Event::Call(Call::PathFilestatGet(PathFilestatGet { call })))
            }
            Ok(machine::VmEvent::ArgsSizesGet(call)) => {
                Ok(Event::Call(Call::ArgsSizesGet(ArgsSizesGet { call })))
            }
            Ok(machine::VmEvent::ArgsGet(call)) => Ok(Event::Call(Call::ArgsGet(ArgsGet { call }))),
            Ok(machine::VmEvent::EnvironSizesGet(call)) => {
                Ok(Event::Call(Call::EnvironSizesGet(EnvironSizesGet { call })))
            }
            Ok(machine::VmEvent::EnvironGet(call)) => {
                Ok(Event::Call(Call::EnvironGet(EnvironGet { call })))
            }
            Ok(machine::VmEvent::MemoryGrow(event)) => {
                Ok(Event::MemoryGrowPending(MemoryGrowPending { event }))
            }
            Ok(machine::VmEvent::BudgetExpired(expired)) => Ok(Event::BudgetExpired(expired)),
            Ok(machine::VmEvent::ProcExit(status)) => Ok(Event::Exit(Exit::new(status))),
            Ok(machine::VmEvent::Done) => Ok(Event::Exit(Exit::returned())),
            Err(error) => Err(error),
        }
    }

    pub const fn import_plan_diagnostics(&self) -> ImportPlanDiagnostics {
        self.engine.import_plan_diagnostics()
    }
}

pub enum Event {
    Call(Call),
    MemoryGrowPending(MemoryGrowPending),
    BudgetExpired(BudgetExpired),
    Exit(Exit),
}

#[doc(hidden)]
pub enum Call {
    FdWrite(FdWrite),
    FdRead(FdRead),
    FdFdstatGet(FdFdstatGet),
    FdPrestatGet(FdPrestatGet),
    FdPrestatDirName(FdPrestatDirName),
    FdFilestatGet(FdFilestatGet),
    FdClose(FdClose),
    ClockResGet(ClockResGet),
    ClockTimeGet(ClockTimeGet),
    PollOneoff(PollOneoff),
    RandomGet(RandomGet),
    FdReaddir(FdReaddir),
    PathOpen(PathOpen),
    PathFilestatGet(PathFilestatGet),
    ArgsSizesGet(ArgsSizesGet),
    ArgsGet(ArgsGet),
    EnvironSizesGet(EnvironSizesGet),
    EnvironGet(EnvironGet),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Exit {
    status: u32,
}

impl Exit {
    const fn new(status: u32) -> Self {
        Self { status }
    }

    const fn returned() -> Self {
        Self { status: 0 }
    }

    pub const fn status(self) -> u32 {
        self.status
    }

    pub const fn as_protocol_status(self) -> Option<ProcExitStatus> {
        if self.status <= u8::MAX as u32 {
            Some(ProcExitStatus::new(self.status as u8))
        } else {
            None
        }
    }
}

pub struct Payload {
    raw: machine::InlinePayload,
}

impl Payload {
    pub fn as_bytes(&self) -> &[u8] {
        self.raw.as_bytes()
    }
}

pub struct FdWrite {
    call: machine::FdWriteCall,
}

impl FdWrite {
    pub const fn fd(&self) -> u8 {
        self.call.fd()
    }

    pub fn payload(&self, guest: &Guest<'_>) -> Result<Payload, Error> {
        Ok(Payload {
            raw: guest.engine.fd_write_payload(self.call)?,
        })
    }

    pub fn complete(self, guest: &mut Guest<'_>, errno: u32) -> Result<(), Error> {
        guest.engine.finish_fd_write(self.call, errno)
    }
}

pub struct FdRead {
    call: machine::FdReadCall,
}

impl FdRead {
    pub const fn fd(&self) -> u8 {
        self.call.fd()
    }

    pub fn max_len(&self, guest: &Guest<'_>) -> Result<usize, Error> {
        let (_, max_len) = guest.engine.fd_read_iovec(self.call)?;
        Ok(max_len as usize)
    }

    pub fn complete(self, guest: &mut Guest<'_>, bytes: &[u8], errno: u32) -> Result<(), Error> {
        guest.engine.finish_fd_read(self.call, bytes, errno)
    }
}

pub struct FdFdstatGet {
    call: machine::FdRequestCall,
}

impl FdFdstatGet {
    pub const fn fd(&self) -> u8 {
        self.call.fd()
    }

    pub fn complete(self, guest: &mut Guest<'_>, stat: FdStat, errno: u32) -> Result<(), Error> {
        guest.engine.finish_fd_fdstat_get(self.call, stat, errno)
    }
}

pub struct FdPrestatGet {
    call: machine::FdRequestCall,
}

impl FdPrestatGet {
    pub const fn fd(&self) -> u8 {
        self.call.fd()
    }

    pub fn complete(self, guest: &mut Guest<'_>, name_len: u32, errno: u32) -> Result<(), Error> {
        guest
            .engine
            .finish_fd_prestat_get(self.call, name_len, errno)
    }
}

pub struct FdPrestatDirName {
    call: machine::FdPrestatDirNameCall,
}

impl FdPrestatDirName {
    pub const fn fd(&self) -> u8 {
        self.call.fd()
    }

    pub const fn max_len(&self) -> usize {
        self.call.max_len()
    }

    pub fn complete(self, guest: &mut Guest<'_>, bytes: &[u8], errno: u32) -> Result<(), Error> {
        guest
            .engine
            .finish_fd_prestat_dir_name(self.call, bytes, errno)
    }
}

pub struct FdFilestatGet {
    call: machine::FdRequestCall,
}

impl FdFilestatGet {
    pub const fn fd(&self) -> u8 {
        self.call.fd()
    }

    pub fn complete(self, guest: &mut Guest<'_>, stat: FileStat, errno: u32) -> Result<(), Error> {
        guest.engine.finish_fd_filestat_get(self.call, stat, errno)
    }
}

pub struct FdClose {
    call: machine::FdRequestCall,
}

impl FdClose {
    pub const fn fd(&self) -> u8 {
        self.call.fd()
    }

    pub fn complete(self, guest: &mut Guest<'_>, errno: u32) -> Result<(), Error> {
        guest.engine.finish_fd_close(self.call, errno)
    }
}

pub struct ClockResGet {
    call: machine::ClockResGetCall,
}

impl ClockResGet {
    pub const fn clock_id(&self) -> u32 {
        self.call.clock_id()
    }

    pub fn complete(
        self,
        guest: &mut Guest<'_>,
        resolution_nanos: u64,
        errno: u32,
    ) -> Result<(), Error> {
        guest
            .engine
            .finish_clock_res_get(self.call, resolution_nanos, errno)
    }
}

pub struct ClockTimeGet {
    call: machine::ClockTimeGetCall,
}

impl ClockTimeGet {
    pub const fn clock_id(&self) -> u32 {
        self.call.clock_id()
    }

    pub const fn precision(&self) -> u64 {
        self.call.precision()
    }

    pub fn complete(self, guest: &mut Guest<'_>, nanos: u64, errno: u32) -> Result<(), Error> {
        guest.engine.finish_clock_time_get(self.call, nanos, errno)
    }
}

pub struct PollOneoff {
    call: machine::PollOneoffCall,
}

impl PollOneoff {
    pub fn delay_ticks(&self, guest: &Guest<'_>) -> Result<u64, Error> {
        guest.engine.poll_oneoff_delay_ticks(self.call)
    }

    pub fn complete(self, guest: &mut Guest<'_>, ready: u32, errno: u32) -> Result<(), Error> {
        guest.engine.finish_poll_oneoff(self.call, ready, errno)
    }
}

pub struct RandomGet {
    call: machine::RandomGetCall,
}

impl RandomGet {
    pub const fn buf_len(&self) -> u32 {
        self.call.buf_len()
    }

    pub fn complete(self, guest: &mut Guest<'_>, bytes: &[u8], errno: u32) -> Result<(), Error> {
        guest.engine.finish_random_get(self.call, bytes, errno)
    }
}

pub struct FdReaddir {
    call: machine::FdReaddirCall,
}

impl FdReaddir {
    pub const fn fd(&self) -> u8 {
        self.call.fd()
    }

    pub const fn cookie(&self) -> u64 {
        self.call.cookie()
    }

    pub const fn max_len(&self) -> usize {
        self.call.max_len()
    }

    pub fn complete(self, guest: &mut Guest<'_>, bytes: &[u8], errno: u32) -> Result<(), Error> {
        guest.engine.finish_fd_readdir(self.call, bytes, errno)
    }
}

pub struct PathOpen {
    call: machine::PathOpenCall,
}

impl PathOpen {
    pub const fn fd(&self) -> u8 {
        self.call.fd()
    }

    pub const fn rights_base(&self) -> u64 {
        self.call.rights_base()
    }

    pub fn path_bytes(&self, guest: &Guest<'_>) -> Result<PathBytes, Error> {
        guest.engine.path_bytes(self.call)
    }

    pub fn complete(self, guest: &mut Guest<'_>, opened_fd: u32, errno: u32) -> Result<(), Error> {
        guest.engine.finish_path_open(self.call, opened_fd, errno)
    }
}

pub struct PathFilestatGet {
    call: machine::PathFilestatGetCall,
}

impl PathFilestatGet {
    pub const fn fd(&self) -> u8 {
        self.call.fd()
    }

    pub const fn flags(&self) -> u32 {
        self.call.flags()
    }

    pub fn path_bytes(&self, guest: &Guest<'_>) -> Result<PathBytes, Error> {
        guest.engine.path_filestat_bytes(self.call)
    }

    pub fn complete(self, guest: &mut Guest<'_>, stat: FileStat, errno: u32) -> Result<(), Error> {
        guest
            .engine
            .finish_path_filestat_get(self.call, stat, errno)
    }
}

pub struct ArgsSizesGet {
    call: machine::ArgsSizesGetCall,
}

impl ArgsSizesGet {
    pub fn complete(
        self,
        guest: &mut Guest<'_>,
        argc: u32,
        argv_buf_size: u32,
        errno: u32,
    ) -> Result<(), Error> {
        guest
            .engine
            .finish_args_sizes_get(self.call, argc, argv_buf_size, errno)
    }
}

pub struct ArgsGet {
    call: machine::ArgsGetCall,
}

impl ArgsGet {
    pub fn complete(self, guest: &mut Guest<'_>, args: &[&[u8]], errno: u32) -> Result<(), Error> {
        guest.engine.finish_args_get(self.call, args, errno)
    }
}

pub struct EnvironSizesGet {
    call: machine::EnvironSizesGetCall,
}

impl EnvironSizesGet {
    pub fn complete(
        self,
        guest: &mut Guest<'_>,
        count: u32,
        buf_size: u32,
        errno: u32,
    ) -> Result<(), Error> {
        guest
            .engine
            .finish_environ_sizes_get(self.call, count, buf_size, errno)
    }
}

pub struct EnvironGet {
    call: machine::EnvironGetCall,
}

impl EnvironGet {
    pub fn complete(
        self,
        guest: &mut Guest<'_>,
        environ: &[(&[u8], &[u8])],
        errno: u32,
    ) -> Result<(), Error> {
        guest.engine.finish_environ_get(self.call, environ, errno)
    }
}

pub struct MemoryGrowPending {
    event: machine::MemoryGrowEvent,
}

impl MemoryGrowPending {
    pub const fn previous_pages(&self) -> u32 {
        self.event.previous_pages
    }

    pub const fn requested_pages(&self) -> u32 {
        self.event.requested_pages
    }

    pub const fn max_pages(&self) -> u32 {
        self.event.max_pages
    }

    pub fn complete(self, guest: &mut Guest<'_>, grant: bool) -> Result<(), Error> {
        guest.engine.finish_memory_grow_event(self.event, grant)
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_GUEST_MEMORY_BYTES, Event, Guest, GuestMemory};
    use crate::protocol::BudgetRun;
    use core::mem::MaybeUninit;
    use std::boxed::Box;

    const START_RETURNS: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x04, 0x01, 0x60, 0x00, 0x00, 0x03,
        0x02, 0x01, 0x00, 0x07, 0x0a, 0x01, 0x06, b'_', b's', b't', b'a', b'r', b't', 0x00, 0x00,
        0x0a, 0x04, 0x01, 0x02, 0x00, 0x0b,
    ];

    #[test]
    fn facade_returns_explicit_exit_when_start_returns() {
        let mut storage = Box::new(MaybeUninit::<Guest<'_>>::uninit());
        let guest = unsafe {
            let ptr = storage.as_mut_ptr();
            let memory = Box::leak(Box::new([0u8; DEFAULT_GUEST_MEMORY_BYTES]));
            Guest::init_in_place(ptr, START_RETURNS, GuestMemory::new(&mut memory[..]))
                .expect("guest init");
            &mut *ptr
        };

        let event = guest
            .resume(BudgetRun::new(1, 1, 16))
            .expect("guest returns");

        match event {
            Event::Exit(exit) => assert_eq!(exit.status(), 0),
            Event::Call(_) | Event::MemoryGrowPending(_) | Event::BudgetExpired(_) => {
                panic!("expected explicit exit event")
            }
        }
    }
}
