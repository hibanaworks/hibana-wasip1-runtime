//! Hibana-native stepper and typed lowering for WASI P1 import rows.
//!
//! Local roles keep completion code at the Hibana level: receive the typed row,
//! answer admitted labels with typed payloads, and let missing rows fail through
//! normal endpoint progress. The choreography remains the progress authority.

use core::mem::MaybeUninit;

use crate::{
    Exit, WasmError,
    engine::wasm::{
        self, Call, Event, FdStat as WasmFdStat, FileStat as WasmFileStat, Guest, GuestMemory,
        ImportPlanDiagnostics, MemoryGrowPending,
    },
    protocol::{
        self, ArgsGet, BudgetExpired, BudgetRun, ClockResGet, ClockTimeGet, EnvironGet, FdBinding,
        FdReadRow, FdReaddirRow, FdRequest, FdWriteRow, MemRights, PollOneoff, RandomGet,
        WASIP1_IO_CHUNK_CAPACITY, WASIP1_PATH_CHUNK_CAPACITY,
    },
};
use hibana::runtime::wire::CodecError;

const FD_READ_RIGHT: u64 = 1 << 1;
const FD_WRITE_RIGHT: u64 = 1 << 6;
const FD_READDIR_RIGHT: u64 = 1 << 14;
const MAX_ARG_REFS: usize = WASIP1_IO_CHUNK_CAPACITY;
pub const FD_BINDING_CAPACITY: usize = 16;
const UNSUPPORTED_WASIP1_INLINE_REPLY_TOO_LARGE: u16 = 0x5101;
const UNSUPPORTED_WASIP1_PATH_REPLY_TOO_LARGE: u16 = 0x5102;
const UNSUPPORTED_WASIP1_CLOCK_ID_TOO_LARGE: u16 = 0x5103;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdBindingTable {
    entries: [Option<FdBinding>; FD_BINDING_CAPACITY],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdBindingCapacityError {
    fd: u8,
}

impl FdBindingCapacityError {
    pub const fn new(fd: u8) -> Self {
        Self { fd }
    }

    pub const fn fd(self) -> u8 {
        self.fd
    }
}

impl FdBindingTable {
    pub const fn empty() -> Self {
        Self {
            entries: [None; FD_BINDING_CAPACITY],
        }
    }

    pub fn bind_fd(&mut self, fd: u8, binding: FdBinding) -> Result<(), FdBindingCapacityError> {
        let Some(slot) = self.entries.get_mut(fd as usize) else {
            return Err(FdBindingCapacityError::new(fd));
        };
        *slot = Some(binding);
        Ok(())
    }

    pub fn remove_fd(&mut self, fd: u8) {
        if let Some(slot) = self.entries.get_mut(fd as usize) {
            *slot = None;
        }
    }

    pub fn binding(&self, fd: u8) -> Option<FdBinding> {
        self.entries.get(fd as usize).and_then(|binding| *binding)
    }

    pub fn bound_write_row(&self, fd: u8) -> Option<FdWriteRow> {
        self.binding(fd).and_then(|binding| binding.write)
    }

    pub fn bound_read_row(&self, fd: u8) -> Option<FdReadRow> {
        self.binding(fd).and_then(|binding| binding.read)
    }

    pub fn bound_readdir_row(&self, fd: u8) -> Option<FdReaddirRow> {
        self.binding(fd).and_then(|binding| binding.readdir)
    }
}

#[derive(Debug)]
pub enum ExchangeError {
    Codec(CodecError),
    Wasm(WasmError),
    FdBindingCapacity(FdBindingCapacityError),
    UnboundFd(u8),
    CompletionMismatch {
        pending: WasiImport,
        completion: WasiImport,
    },
    ReturnFdMismatch {
        import: WasiImport,
        expected_fd: u8,
        actual_fd: u8,
    },
    GuestStorageAlreadyInitialized,
}

impl From<CodecError> for ExchangeError {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

impl From<WasmError> for ExchangeError {
    fn from(error: WasmError) -> Self {
        Self::Wasm(error)
    }
}

impl From<FdBindingCapacityError> for ExchangeError {
    fn from(error: FdBindingCapacityError) -> Self {
        Self::FdBindingCapacity(error)
    }
}

pub struct HibanaWasiGuest<'a> {
    guest: Guest<'a>,
    bindings: FdBindingTable,
}

pub struct HibanaWasiGuestStorage<'a> {
    slot: MaybeUninit<HibanaWasiGuest<'a>>,
    initialized: bool,
}

impl<'a> HibanaWasiGuestStorage<'a> {
    pub const fn uninit() -> Self {
        Self {
            slot: MaybeUninit::uninit(),
            initialized: false,
        }
    }

    pub fn init(
        &mut self,
        module: &'a [u8],
        memory: GuestMemory<'a>,
        bindings: FdBindingTable,
    ) -> Result<&mut HibanaWasiGuest<'a>, ExchangeError> {
        if self.initialized {
            return Err(ExchangeError::GuestStorageAlreadyInitialized);
        }
        unsafe {
            HibanaWasiGuest::init_in_place(self.slot.as_mut_ptr(), module, memory, bindings)?;
        }
        self.initialized = true;
        Ok(unsafe { &mut *self.slot.as_mut_ptr() })
    }
}

impl Drop for HibanaWasiGuestStorage<'_> {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                self.slot.assume_init_drop();
            }
        }
    }
}

impl<'a> HibanaWasiGuest<'a> {
    /// # Safety
    ///
    /// `dst` must be valid for writes, properly aligned for
    /// `HibanaWasiGuest<'a>`, and must not be read until this function returns
    /// `Ok(())`.
    pub unsafe fn init_in_place(
        dst: *mut Self,
        module: &'a [u8],
        memory: GuestMemory<'a>,
        bindings: FdBindingTable,
    ) -> Result<(), ExchangeError> {
        unsafe {
            Guest::init_in_place(core::ptr::addr_of_mut!((*dst).guest), module, memory)?;
            core::ptr::addr_of_mut!((*dst).bindings).write(bindings);
        }
        Ok(())
    }

    pub fn resume_wasi_boundary(
        &mut self,
        budget: BudgetRun,
    ) -> Result<WasiBoundaryStep, ExchangeError> {
        let event = self.guest.resume(budget);
        match event? {
            Event::Call(call) => {
                let pending = lower_call(&self.guest, call, &self.bindings);
                Ok(WasiBoundaryStep::ImportPending(pending?))
            }
            Event::MemoryGrowPending(pending) => {
                let request = protocol::MemoryGrowReq(protocol::MemoryGrow::new(
                    pending.previous_pages(),
                    pending.requested_pages(),
                    pending.max_pages(),
                ));
                Ok(WasiBoundaryStep::MemoryGrowPending(WasiMemoryGrowPending {
                    request,
                    pending,
                }))
            }
            Event::BudgetExpired(expired) => Ok(WasiBoundaryStep::BudgetExpired(expired)),
            Event::Exit(exit) => Ok(WasiBoundaryStep::Exit(exit)),
        }
    }

    pub const fn import_plan_diagnostics(&self) -> ImportPlanDiagnostics {
        self.guest.import_plan_diagnostics()
    }
}

pub enum WasiBoundaryStep {
    ImportPending(WasiImportPending),
    MemoryGrowPending(WasiMemoryGrowPending),
    BudgetExpired(BudgetExpired),
    Exit(Exit),
}

pub struct WasiImportPending {
    request: WasiImportRequest,
    pending: PendingCall,
}

impl WasiImportPending {
    pub const fn request(&self) -> WasiImportRequest {
        self.request
    }

    pub const fn import(&self) -> WasiImport {
        self.request.import()
    }

    pub fn complete(
        self,
        guest: &mut HibanaWasiGuest<'_>,
        completion: WasiImportCompletion,
    ) -> Result<(), ExchangeError> {
        self.pending
            .complete_with(&mut guest.guest, completion, &mut guest.bindings)
    }
}

pub struct WasiMemoryGrowPending {
    request: protocol::MemoryGrowReq,
    pending: MemoryGrowPending,
}

impl WasiMemoryGrowPending {
    pub const fn request(&self) -> protocol::MemoryGrowReq {
        self.request
    }

    pub const fn previous_pages(&self) -> u32 {
        self.pending.previous_pages()
    }

    pub const fn requested_pages(&self) -> u32 {
        self.pending.requested_pages()
    }

    pub const fn max_pages(&self) -> u32 {
        self.pending.max_pages()
    }

    pub fn complete(
        self,
        guest: &mut HibanaWasiGuest<'_>,
        decision: protocol::MemoryGrowRet,
    ) -> Result<(), ExchangeError> {
        self.pending
            .complete(&mut guest.guest, decision.0.granted())?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasiImport {
    FdWrite,
    FdWriteObject,
    FdRead,
    FdReaddir,
    PathOpen,
    FdPrestatGet,
    FdPrestatDirName,
    FdFilestatGet,
    ArgsSizesGet,
    ArgsGet,
    EnvironSizesGet,
    EnvironGet,
    FdFdstatGet,
    PathFilestatGet,
    FdClose,
    ClockResGet,
    ClockTimeGet,
    PollOneoff,
    RandomGet,
}

impl WasiImport {
    pub const fn from_label(label: u8) -> Option<Self> {
        match label {
            protocol::LABEL_WASI_FD_WRITE => Some(Self::FdWrite),
            protocol::LABEL_WASI_FD_WRITE_OBJECT => Some(Self::FdWriteObject),
            protocol::LABEL_WASI_FD_READ => Some(Self::FdRead),
            protocol::LABEL_WASI_FD_READDIR => Some(Self::FdReaddir),
            protocol::LABEL_WASI_PATH_OPEN => Some(Self::PathOpen),
            protocol::LABEL_WASI_FD_PRESTAT_GET => Some(Self::FdPrestatGet),
            protocol::LABEL_WASI_FD_PRESTAT_DIR_NAME => Some(Self::FdPrestatDirName),
            protocol::LABEL_WASI_FD_FILESTAT_GET => Some(Self::FdFilestatGet),
            protocol::LABEL_WASI_ARGS_SIZES_GET => Some(Self::ArgsSizesGet),
            protocol::LABEL_WASI_ARGS_GET => Some(Self::ArgsGet),
            protocol::LABEL_WASI_ENVIRON_SIZES_GET => Some(Self::EnvironSizesGet),
            protocol::LABEL_WASI_ENVIRON_GET => Some(Self::EnvironGet),
            protocol::LABEL_WASI_FD_FDSTAT_GET => Some(Self::FdFdstatGet),
            protocol::LABEL_WASI_PATH_FILESTAT_GET => Some(Self::PathFilestatGet),
            protocol::LABEL_WASI_FD_CLOSE => Some(Self::FdClose),
            protocol::LABEL_WASI_CLOCK_RES_GET => Some(Self::ClockResGet),
            protocol::LABEL_WASI_CLOCK_TIME_GET => Some(Self::ClockTimeGet),
            protocol::LABEL_WASI_POLL_ONEOFF => Some(Self::PollOneoff),
            protocol::LABEL_WASI_RANDOM_GET => Some(Self::RandomGet),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasiImportRequest {
    FdWrite(protocol::FdWriteReq),
    FdWriteObject(protocol::FdWriteReq),
    FdRead(protocol::FdReadReq),
    FdReaddir(protocol::FdReaddirReq),
    PathOpen(protocol::PathOpenReq),
    FdPrestatGet(protocol::FdPrestatGetReq),
    FdPrestatDirName(protocol::FdPrestatDirNameReq),
    FdFilestatGet(protocol::FdFilestatGetReq),
    ArgsSizesGet(protocol::ArgsSizesGetReq),
    ArgsGet(protocol::ArgsGetReq),
    EnvironSizesGet(protocol::EnvironSizesGetReq),
    EnvironGet(protocol::EnvironGetReq),
    FdFdstatGet(protocol::FdFdstatGetReq),
    PathFilestatGet(protocol::PathFilestatGetReq),
    FdClose(protocol::FdCloseReq),
    ClockResGet(protocol::ClockResGetReq),
    ClockTimeGet(protocol::ClockTimeGetReq),
    PollOneoff(protocol::PollOneoffReq),
    RandomGet(protocol::RandomGetReq),
}

impl WasiImportRequest {
    pub const fn import(self) -> WasiImport {
        match self {
            Self::FdWrite(_) => WasiImport::FdWrite,
            Self::FdWriteObject(_) => WasiImport::FdWriteObject,
            Self::FdRead(_) => WasiImport::FdRead,
            Self::FdReaddir(_) => WasiImport::FdReaddir,
            Self::PathOpen(_) => WasiImport::PathOpen,
            Self::FdPrestatGet(_) => WasiImport::FdPrestatGet,
            Self::FdPrestatDirName(_) => WasiImport::FdPrestatDirName,
            Self::FdFilestatGet(_) => WasiImport::FdFilestatGet,
            Self::ArgsSizesGet(_) => WasiImport::ArgsSizesGet,
            Self::ArgsGet(_) => WasiImport::ArgsGet,
            Self::EnvironSizesGet(_) => WasiImport::EnvironSizesGet,
            Self::EnvironGet(_) => WasiImport::EnvironGet,
            Self::FdFdstatGet(_) => WasiImport::FdFdstatGet,
            Self::PathFilestatGet(_) => WasiImport::PathFilestatGet,
            Self::FdClose(_) => WasiImport::FdClose,
            Self::ClockResGet(_) => WasiImport::ClockResGet,
            Self::ClockTimeGet(_) => WasiImport::ClockTimeGet,
            Self::PollOneoff(_) => WasiImport::PollOneoff,
            Self::RandomGet(_) => WasiImport::RandomGet,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasiImportCompletion {
    FdWrite(protocol::FdWriteDoneRet),
    FdWriteObject(protocol::FdWriteDoneRet),
    FdRead(protocol::FdReadDoneRet),
    FdReaddir(protocol::FdReaddirDoneRet),
    PathOpen(protocol::PathOpenedRet),
    FdPrestatGet(protocol::FdPrestatRet),
    FdPrestatDirName(protocol::FdPrestatDirNameRet),
    FdFilestatGet(protocol::FdFilestatRet),
    ArgsSizesGet(protocol::ArgsSizesRet),
    ArgsGet(protocol::ArgsDoneRet),
    EnvironSizesGet(protocol::EnvironSizesRet),
    EnvironGet(protocol::EnvironDoneRet),
    FdFdstatGet(protocol::FdStatRet),
    PathFilestatGet(protocol::PathFilestatRet),
    FdClose(protocol::FdClosedRet),
    ClockResGet(protocol::ClockResolutionRet),
    ClockTimeGet(protocol::ClockTimeRet),
    PollOneoff(protocol::PollReadyRet),
    RandomGet(protocol::RandomDoneRet),
}

impl WasiImportCompletion {
    pub const fn import(self) -> WasiImport {
        match self {
            Self::FdWrite(_) => WasiImport::FdWrite,
            Self::FdWriteObject(_) => WasiImport::FdWriteObject,
            Self::FdRead(_) => WasiImport::FdRead,
            Self::FdReaddir(_) => WasiImport::FdReaddir,
            Self::PathOpen(_) => WasiImport::PathOpen,
            Self::FdPrestatGet(_) => WasiImport::FdPrestatGet,
            Self::FdPrestatDirName(_) => WasiImport::FdPrestatDirName,
            Self::FdFilestatGet(_) => WasiImport::FdFilestatGet,
            Self::ArgsSizesGet(_) => WasiImport::ArgsSizesGet,
            Self::ArgsGet(_) => WasiImport::ArgsGet,
            Self::EnvironSizesGet(_) => WasiImport::EnvironSizesGet,
            Self::EnvironGet(_) => WasiImport::EnvironGet,
            Self::FdFdstatGet(_) => WasiImport::FdFdstatGet,
            Self::PathFilestatGet(_) => WasiImport::PathFilestatGet,
            Self::FdClose(_) => WasiImport::FdClose,
            Self::ClockResGet(_) => WasiImport::ClockResGet,
            Self::ClockTimeGet(_) => WasiImport::ClockTimeGet,
            Self::PollOneoff(_) => WasiImport::PollOneoff,
            Self::RandomGet(_) => WasiImport::RandomGet,
        }
    }
}

enum PendingCall {
    FdWrite(wasm::FdWrite),
    FdWriteObject(wasm::FdWrite),
    FdRead(wasm::FdRead),
    FdReaddir(wasm::FdReaddir),
    PathOpen(wasm::PathOpen),
    FdPrestatGet(wasm::FdPrestatGet),
    FdPrestatDirName(wasm::FdPrestatDirName),
    FdFilestatGet(wasm::FdFilestatGet),
    ArgsSizesGet(wasm::ArgsSizesGet),
    ArgsGet(wasm::ArgsGet),
    EnvironSizesGet(wasm::EnvironSizesGet),
    EnvironGet(wasm::EnvironGet),
    FdFdstatGet(wasm::FdFdstatGet),
    PathFilestatGet(wasm::PathFilestatGet),
    FdClose(wasm::FdClose),
    ClockResGet(wasm::ClockResGet),
    ClockTimeGet(wasm::ClockTimeGet),
    PollOneoff(wasm::PollOneoff),
    RandomGet(wasm::RandomGet),
}

impl PendingCall {
    fn import(&self) -> WasiImport {
        match self {
            Self::FdWrite(_) => WasiImport::FdWrite,
            Self::FdWriteObject(_) => WasiImport::FdWriteObject,
            Self::FdRead(_) => WasiImport::FdRead,
            Self::FdReaddir(_) => WasiImport::FdReaddir,
            Self::PathOpen(_) => WasiImport::PathOpen,
            Self::FdPrestatGet(_) => WasiImport::FdPrestatGet,
            Self::FdPrestatDirName(_) => WasiImport::FdPrestatDirName,
            Self::FdFilestatGet(_) => WasiImport::FdFilestatGet,
            Self::ArgsSizesGet(_) => WasiImport::ArgsSizesGet,
            Self::ArgsGet(_) => WasiImport::ArgsGet,
            Self::EnvironSizesGet(_) => WasiImport::EnvironSizesGet,
            Self::EnvironGet(_) => WasiImport::EnvironGet,
            Self::FdFdstatGet(_) => WasiImport::FdFdstatGet,
            Self::PathFilestatGet(_) => WasiImport::PathFilestatGet,
            Self::FdClose(_) => WasiImport::FdClose,
            Self::ClockResGet(_) => WasiImport::ClockResGet,
            Self::ClockTimeGet(_) => WasiImport::ClockTimeGet,
            Self::PollOneoff(_) => WasiImport::PollOneoff,
            Self::RandomGet(_) => WasiImport::RandomGet,
        }
    }

    fn complete_with(
        self,
        guest: &mut Guest<'_>,
        completion: WasiImportCompletion,
        bindings: &mut FdBindingTable,
    ) -> Result<(), ExchangeError> {
        let pending = self.import();
        let completed = completion.import();
        if pending != completed {
            return Err(ExchangeError::CompletionMismatch {
                pending,
                completion: completed,
            });
        }

        match (self, completion) {
            (Self::FdWrite(call), WasiImportCompletion::FdWrite(done)) => {
                expect_fd(WasiImport::FdWrite, call.fd(), done.0.fd())?;
                call.complete(guest, done.0.errno() as u32)?;
            }
            (Self::FdWriteObject(call), WasiImportCompletion::FdWriteObject(done)) => {
                expect_fd(WasiImport::FdWriteObject, call.fd(), done.0.fd())?;
                call.complete(guest, done.0.errno() as u32)?;
            }
            (Self::FdRead(call), WasiImportCompletion::FdRead(done)) => {
                expect_fd(WasiImport::FdRead, call.fd(), done.0.fd())?;
                call.complete(guest, done.0.as_bytes(), done.0.errno() as u32)?;
            }
            (Self::FdReaddir(call), WasiImportCompletion::FdReaddir(done)) => {
                expect_fd(WasiImport::FdReaddir, call.fd(), done.0.fd())?;
                call.complete(guest, done.0.as_bytes(), done.0.errno() as u32)?;
            }
            (Self::PathOpen(call), WasiImportCompletion::PathOpen(opened)) => {
                call.complete(guest, opened.0.fd() as u32, opened.0.errno() as u32)?;
                if opened.0.errno() == 0 && !opened.0.binding().is_empty() {
                    bindings.bind_fd(opened.0.fd(), opened.0.binding())?;
                }
            }
            (Self::FdPrestatGet(call), WasiImportCompletion::FdPrestatGet(prestat)) => {
                expect_fd(WasiImport::FdPrestatGet, call.fd(), prestat.0.fd())?;
                call.complete(guest, prestat.0.name_len() as u32, prestat.0.errno() as u32)?;
            }
            (Self::FdPrestatDirName(call), WasiImportCompletion::FdPrestatDirName(name)) => {
                expect_fd(WasiImport::FdPrestatDirName, call.fd(), name.0.fd())?;
                call.complete(guest, name.0.as_bytes(), name.0.errno() as u32)?;
            }
            (Self::FdFilestatGet(call), WasiImportCompletion::FdFilestatGet(stat)) => {
                call.complete(guest, wasm_file_stat(stat.0), stat.0.errno() as u32)?;
            }
            (Self::ArgsSizesGet(call), WasiImportCompletion::ArgsSizesGet(sizes)) => {
                call.complete(guest, sizes.0.count() as u32, sizes.0.buf_size() as u32, 0)?;
            }
            (Self::ArgsGet(call), WasiImportCompletion::ArgsGet(done)) => {
                let mut args = [&[][..]; MAX_ARG_REFS];
                let count = split_args(done.0.as_bytes(), &mut args);
                call.complete(guest, &args[..count], 0)?;
            }
            (Self::EnvironSizesGet(call), WasiImportCompletion::EnvironSizesGet(sizes)) => {
                call.complete(guest, sizes.0.count() as u32, sizes.0.buf_size() as u32, 0)?;
            }
            (Self::EnvironGet(call), WasiImportCompletion::EnvironGet(_done)) => {
                call.complete(guest, &[], 0)?;
            }
            (Self::FdFdstatGet(call), WasiImportCompletion::FdFdstatGet(stat)) => {
                expect_fd(WasiImport::FdFdstatGet, call.fd(), stat.0.fd())?;
                call.complete(guest, wasm_fd_stat(stat.0), stat.0.errno() as u32)?;
            }
            (Self::PathFilestatGet(call), WasiImportCompletion::PathFilestatGet(stat)) => {
                call.complete(guest, wasm_file_stat(stat.0), stat.0.errno() as u32)?;
            }
            (Self::FdClose(call), WasiImportCompletion::FdClose(closed)) => {
                expect_fd(WasiImport::FdClose, call.fd(), closed.0.fd())?;
                if closed.0.errno() == 0 {
                    bindings.remove_fd(call.fd());
                }
                call.complete(guest, closed.0.errno() as u32)?;
            }
            (Self::ClockResGet(call), WasiImportCompletion::ClockResGet(resolution)) => {
                call.complete(guest, resolution.0.nanos(), 0)?;
            }
            (Self::ClockTimeGet(call), WasiImportCompletion::ClockTimeGet(time)) => {
                call.complete(guest, time.0.nanos(), 0)?;
            }
            (Self::PollOneoff(call), WasiImportCompletion::PollOneoff(ready)) => {
                call.complete(guest, ready.0.ready() as u32, 0)?;
            }
            (Self::RandomGet(call), WasiImportCompletion::RandomGet(done)) => {
                call.complete(guest, done.0.as_bytes(), 0)?;
            }
            _ => {
                return Err(ExchangeError::CompletionMismatch {
                    pending,
                    completion: completed,
                });
            }
        }
        Ok(())
    }
}

fn lower_call(
    guest: &Guest<'_>,
    call: Call,
    bindings: &FdBindingTable,
) -> Result<WasiImportPending, ExchangeError> {
    match call {
        Call::FdWrite(call) => {
            let payload = call.payload(guest)?;
            let request =
                protocol::FdWriteReq(protocol::FdWrite::new(call.fd(), payload.as_bytes())?);
            let row = bindings
                .bound_write_row(call.fd())
                .ok_or(ExchangeError::UnboundFd(call.fd()))?;
            match row {
                FdWriteRow::Base => Ok(WasiImportPending {
                    request: WasiImportRequest::FdWrite(request),
                    pending: PendingCall::FdWrite(call),
                }),
                FdWriteRow::Object => Ok(WasiImportPending {
                    request: WasiImportRequest::FdWriteObject(request),
                    pending: PendingCall::FdWriteObject(call),
                }),
            }
        }
        Call::FdRead(call) => {
            bindings
                .bound_read_row(call.fd())
                .ok_or(ExchangeError::UnboundFd(call.fd()))?;
            let max_len = inline_io_request_len(call.max_len(guest)?);
            let request = protocol::FdReadReq(protocol::FdRead::new(call.fd(), max_len)?);
            Ok(WasiImportPending {
                request: WasiImportRequest::FdRead(request),
                pending: PendingCall::FdRead(call),
            })
        }
        Call::FdReaddir(call) => {
            bindings
                .bound_readdir_row(call.fd())
                .ok_or(ExchangeError::UnboundFd(call.fd()))?;
            let request = protocol::FdReaddirReq(protocol::FdReaddir::new(
                call.fd(),
                call.cookie(),
                inline_io_request_len(call.max_len()),
            )?);
            Ok(WasiImportPending {
                request: WasiImportRequest::FdReaddir(request),
                pending: PendingCall::FdReaddir(call),
            })
        }
        Call::PathOpen(call) => {
            let path = call.path_bytes(guest)?;
            let request = protocol::PathOpenReq(protocol::PathOpen::new(
                call.fd(),
                call.rights_base(),
                path.as_bytes(),
            )?);
            Ok(WasiImportPending {
                request: WasiImportRequest::PathOpen(request),
                pending: PendingCall::PathOpen(call),
            })
        }
        Call::FdPrestatGet(call) => {
            let request = protocol::FdPrestatGetReq(FdRequest::new(call.fd()));
            Ok(WasiImportPending {
                request: WasiImportRequest::FdPrestatGet(request),
                pending: PendingCall::FdPrestatGet(call),
            })
        }
        Call::FdPrestatDirName(call) => {
            let request = protocol::FdPrestatDirNameReq(protocol::FdPrestatDirName::new(
                call.fd(),
                exact_path_reply_len(call.max_len())?,
            )?);
            Ok(WasiImportPending {
                request: WasiImportRequest::FdPrestatDirName(request),
                pending: PendingCall::FdPrestatDirName(call),
            })
        }
        Call::FdFilestatGet(call) => {
            let request = protocol::FdFilestatGetReq(FdRequest::new(call.fd()));
            Ok(WasiImportPending {
                request: WasiImportRequest::FdFilestatGet(request),
                pending: PendingCall::FdFilestatGet(call),
            })
        }
        Call::ArgsSizesGet(call) => {
            let request = protocol::ArgsSizesGetReq(protocol::ArgsSizesGet);
            Ok(WasiImportPending {
                request: WasiImportRequest::ArgsSizesGet(request),
                pending: PendingCall::ArgsSizesGet(call),
            })
        }
        Call::ArgsGet(call) => {
            let request = protocol::ArgsGetReq(ArgsGet::new(WASIP1_IO_CHUNK_CAPACITY as u8)?);
            Ok(WasiImportPending {
                request: WasiImportRequest::ArgsGet(request),
                pending: PendingCall::ArgsGet(call),
            })
        }
        Call::EnvironSizesGet(call) => {
            let request = protocol::EnvironSizesGetReq(protocol::EnvironSizesGet);
            Ok(WasiImportPending {
                request: WasiImportRequest::EnvironSizesGet(request),
                pending: PendingCall::EnvironSizesGet(call),
            })
        }
        Call::EnvironGet(call) => {
            let request = protocol::EnvironGetReq(EnvironGet::new(WASIP1_IO_CHUNK_CAPACITY as u8)?);
            Ok(WasiImportPending {
                request: WasiImportRequest::EnvironGet(request),
                pending: PendingCall::EnvironGet(call),
            })
        }
        Call::FdFdstatGet(call) => {
            let request = protocol::FdFdstatGetReq(FdRequest::new(call.fd()));
            Ok(WasiImportPending {
                request: WasiImportRequest::FdFdstatGet(request),
                pending: PendingCall::FdFdstatGet(call),
            })
        }
        Call::PathFilestatGet(call) => {
            let path = call.path_bytes(guest)?;
            let request = protocol::PathFilestatGetReq(protocol::PathFilestatGet::new(
                call.fd(),
                call.flags(),
                path.as_bytes(),
            )?);
            Ok(WasiImportPending {
                request: WasiImportRequest::PathFilestatGet(request),
                pending: PendingCall::PathFilestatGet(call),
            })
        }
        Call::FdClose(call) => {
            let request = protocol::FdCloseReq(FdRequest::new(call.fd()));
            Ok(WasiImportPending {
                request: WasiImportRequest::FdClose(request),
                pending: PendingCall::FdClose(call),
            })
        }
        Call::ClockResGet(call) => {
            let request = protocol::ClockResGetReq(ClockResGet::new(clock_id_u8(call.clock_id())?));
            Ok(WasiImportPending {
                request: WasiImportRequest::ClockResGet(request),
                pending: PendingCall::ClockResGet(call),
            })
        }
        Call::ClockTimeGet(call) => {
            let request = protocol::ClockTimeGetReq(ClockTimeGet::new(
                clock_id_u8(call.clock_id())?,
                call.precision(),
            ));
            Ok(WasiImportPending {
                request: WasiImportRequest::ClockTimeGet(request),
                pending: PendingCall::ClockTimeGet(call),
            })
        }
        Call::PollOneoff(call) => {
            let request = protocol::PollOneoffReq(PollOneoff::new(call.delay_ticks(guest)?));
            Ok(WasiImportPending {
                request: WasiImportRequest::PollOneoff(request),
                pending: PendingCall::PollOneoff(call),
            })
        }
        Call::RandomGet(call) => {
            let request =
                protocol::RandomGetReq(RandomGet::new(exact_io_reply_len(call.buf_len())?)?);
            Ok(WasiImportPending {
                request: WasiImportRequest::RandomGet(request),
                pending: PendingCall::RandomGet(call),
            })
        }
    }
}

fn expect_fd(import: WasiImport, expected_fd: u8, actual_fd: u8) -> Result<(), ExchangeError> {
    if expected_fd == actual_fd {
        Ok(())
    } else {
        Err(ExchangeError::ReturnFdMismatch {
            import,
            expected_fd,
            actual_fd,
        })
    }
}

fn inline_io_request_len(value: usize) -> u8 {
    value.min(WASIP1_IO_CHUNK_CAPACITY) as u8
}

fn exact_io_reply_len(value: u32) -> Result<u8, ExchangeError> {
    let len = value as usize;
    if len <= WASIP1_IO_CHUNK_CAPACITY {
        Ok(len as u8)
    } else {
        Err(unsupported(UNSUPPORTED_WASIP1_INLINE_REPLY_TOO_LARGE))
    }
}

fn exact_path_reply_len(value: usize) -> Result<u8, ExchangeError> {
    if value <= WASIP1_PATH_CHUNK_CAPACITY {
        Ok(value as u8)
    } else {
        Err(unsupported(UNSUPPORTED_WASIP1_PATH_REPLY_TOO_LARGE))
    }
}

fn clock_id_u8(value: u32) -> Result<u8, ExchangeError> {
    u8::try_from(value).map_err(|_| unsupported(UNSUPPORTED_WASIP1_CLOCK_ID_TOO_LARGE))
}

fn unsupported(code: u16) -> ExchangeError {
    ExchangeError::Wasm(WasmError::Unsupported(code))
}

fn split_args<'a>(bytes: &'a [u8], out: &mut [&'a [u8]; MAX_ARG_REFS]) -> usize {
    let mut count = 0usize;
    for arg in bytes.split(|byte| *byte == 0).filter(|arg| !arg.is_empty()) {
        if count == out.len() {
            break;
        }
        out[count] = arg;
        count += 1;
    }
    count
}

fn wasm_fd_stat(stat: protocol::FdStat) -> WasmFdStat {
    let rights_base = match stat.rights() {
        MemRights::Read => FD_READ_RIGHT | FD_READDIR_RIGHT,
        MemRights::Write => FD_WRITE_RIGHT,
    };
    WasmFdStat::new(0, 0, rights_base, rights_base)
}

fn wasm_file_stat(stat: protocol::FileStat) -> WasmFileStat {
    WasmFileStat::new(stat.filetype(), stat.size())
}

#[cfg(test)]
mod tests {
    use super::{
        ExchangeError, FdBindingTable, PendingCall, UNSUPPORTED_WASIP1_CLOCK_ID_TOO_LARGE,
        UNSUPPORTED_WASIP1_INLINE_REPLY_TOO_LARGE, UNSUPPORTED_WASIP1_PATH_REPLY_TOO_LARGE,
        WasiBoundaryStep, WasiImportPending, WasiImportRequest, clock_id_u8, exact_io_reply_len,
        exact_path_reply_len, inline_io_request_len,
    };
    use crate::WasmError;
    use core::mem::size_of;

    #[test]
    fn binding_table_and_pending_token_stay_small() {
        assert!(
            size_of::<FdBindingTable>() <= 128,
            "FdBindingTable uses {} bytes",
            size_of::<FdBindingTable>()
        );
        assert!(
            size_of::<PendingCall>() <= 64,
            "PendingCall uses {} bytes",
            size_of::<PendingCall>()
        );
        assert!(
            size_of::<WasiImportRequest>() <= 80,
            "WasiImportRequest uses {} bytes",
            size_of::<WasiImportRequest>()
        );
        assert!(
            size_of::<WasiImportPending>() <= 128,
            "WasiImportPending uses {} bytes",
            size_of::<WasiImportPending>()
        );
        assert!(
            size_of::<WasiBoundaryStep>() <= 136,
            "WasiBoundaryStep uses {} bytes",
            size_of::<WasiBoundaryStep>()
        );
    }

    #[test]
    fn inline_io_len_only_clamps_partial_transfer_imports() {
        assert_eq!(inline_io_request_len(0), 0);
        assert_eq!(inline_io_request_len(64), 64);
        assert_eq!(inline_io_request_len(65), 64);

        assert!(matches!(exact_io_reply_len(64), Ok(64)));
        assert!(matches!(
            exact_io_reply_len(65),
            Err(ExchangeError::Wasm(WasmError::Unsupported(code)))
                if code == UNSUPPORTED_WASIP1_INLINE_REPLY_TOO_LARGE
        ));
    }

    #[test]
    fn exact_path_and_clock_values_fail_fast() {
        assert!(matches!(exact_path_reply_len(40), Ok(40)));
        assert!(matches!(
            exact_path_reply_len(41),
            Err(ExchangeError::Wasm(WasmError::Unsupported(code)))
                if code == UNSUPPORTED_WASIP1_PATH_REPLY_TOO_LARGE
        ));

        assert!(matches!(clock_id_u8(255), Ok(255)));
        assert!(matches!(
            clock_id_u8(256),
            Err(ExchangeError::Wasm(WasmError::Unsupported(code)))
                if code == UNSUPPORTED_WASIP1_CLOCK_ID_TOO_LARGE
        ));
    }
}
