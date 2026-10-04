//! Hibana-native stepper and typed lowering for WASI P1 import rows.
//!
//! Local roles keep completion code at the Hibana level: receive the typed row,
//! answer admitted labels with typed payloads, and let missing rows fail through
//! normal endpoint progress. The choreography remains the progress authority.

use core::mem::MaybeUninit;

use crate::{
    Exit, WasmError,
    engine::wasm::{
        Call, Event, FdStat as WasmFdStat, FileStat as WasmFileStat, Guest, GuestMemory,
        ImportPlanDiagnostics, MemoryGrowPending,
    },
    protocol::{
        self, ArgsGet, BudgetExpired, BudgetRun, ClockResGet, ClockTimeGet, EnvironGet, FdBinding,
        FdReadRow, FdReaddirRow, FdRequest, FdWriteRow, MemRights, RandomGet,
        WASIP1_IO_CHUNK_CAPACITY, WASIP1_PATH_CHUNK_CAPACITY,
    },
};
use hibana::runtime::wire::CodecError;

const FD_READ_RIGHT: u64 = 1 << 1;
const FD_WRITE_RIGHT: u64 = 1 << 6;
const FD_READDIR_RIGHT: u64 = 1 << 14;
const MAX_ARG_REFS: usize = WASIP1_IO_CHUNK_CAPACITY;
const MAX_ENV_REFS: usize = WASIP1_IO_CHUNK_CAPACITY / 3;
pub const FD_BINDING_CAPACITY: usize = 16;
const UNSUPPORTED_WASIP1_INLINE_REPLY_TOO_LARGE: u16 = 0x5101;
const UNSUPPORTED_WASIP1_PATH_REPLY_TOO_LARGE: u16 = 0x5102;
const UNSUPPORTED_WASIP1_CLOCK_ID_TOO_LARGE: u16 = 0x5103;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FdBindingEntry {
    fd: u8,
    binding: FdBinding,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdBindingTable {
    entries: [Option<FdBindingEntry>; FD_BINDING_CAPACITY],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdBindingCapacityError {
    fd: u8,
}

impl FdBindingCapacityError {
    const fn new(fd: u8) -> Self {
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
        if let Some(entry) = self
            .entries
            .iter_mut()
            .flatten()
            .find(|entry| entry.fd == fd)
        {
            entry.binding = binding;
            return Ok(());
        }
        if let Some(slot) = self.entries.iter_mut().find(|entry| entry.is_none()) {
            *slot = Some(FdBindingEntry { fd, binding });
            return Ok(());
        }
        Err(FdBindingCapacityError::new(fd))
    }

    pub fn remove_fd(&mut self, fd: u8) {
        if let Some(slot) = self
            .entries
            .iter_mut()
            .find(|entry| entry.is_some_and(|entry| entry.fd == fd))
        {
            *slot = None;
        }
    }

    pub fn binding(&self, fd: u8) -> Option<FdBinding> {
        self.entries
            .iter()
            .flatten()
            .find(|entry| entry.fd == fd)
            .map(|entry| entry.binding)
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
    GuestStorageConsumed,
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

const _: () = assert!(!core::mem::needs_drop::<HibanaWasiGuest<'static>>());

#[derive(Clone, Copy, PartialEq, Eq)]
enum GuestStorageState {
    Vacant,
    Failed,
    Initialized,
}

pub struct HibanaWasiGuestStorage<'a> {
    slot: MaybeUninit<HibanaWasiGuest<'a>>,
    state: GuestStorageState,
}

impl<'a> HibanaWasiGuestStorage<'a> {
    pub const fn uninit() -> Self {
        Self {
            slot: MaybeUninit::uninit(),
            state: GuestStorageState::Vacant,
        }
    }

    pub fn init(
        &mut self,
        module: &'a [u8],
        memory: GuestMemory<'a>,
        bindings: FdBindingTable,
    ) -> Result<&mut HibanaWasiGuest<'a>, ExchangeError> {
        if self.state != GuestStorageState::Vacant {
            return Err(ExchangeError::GuestStorageConsumed);
        }
        self.state = GuestStorageState::Failed;
        // SAFETY: `slot` is aligned writable storage, remains exclusively
        // borrowed through `self`, and this one-shot storage is never read,
        // dropped as initialized, or retried unless initialization succeeds.
        unsafe {
            HibanaWasiGuest::init_in_place(self.slot.as_mut_ptr(), module, memory, bindings)?;
        }
        self.state = GuestStorageState::Initialized;
        // SAFETY: the successful initializer wrote every field and the state
        // transition above records that fact before the reference is exposed.
        Ok(unsafe { &mut *self.slot.as_mut_ptr() })
    }
}

impl Drop for HibanaWasiGuestStorage<'_> {
    fn drop(&mut self) {
        if self.state == GuestStorageState::Initialized {
            // SAFETY: `Initialized` is set only after complete initialization
            // and the storage cannot be initialized a second time.
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
    unsafe fn init_in_place(
        dst: *mut Self,
        module: &'a [u8],
        memory: GuestMemory<'a>,
        bindings: FdBindingTable,
    ) -> Result<(), ExchangeError> {
        // SAFETY: the caller provides exclusive aligned storage. `Guest`
        // initializes its complete field before `bindings` is written, and
        // neither field is read through `dst` until both writes succeed.
        unsafe {
            Guest::init_in_place(core::ptr::addr_of_mut!((*dst).guest), module, memory)?;
            core::ptr::addr_of_mut!((*dst).bindings).write(bindings);
        }
        Ok(())
    }

    pub fn resume_wasi_boundary(
        &mut self,
        budget: BudgetRun,
    ) -> Result<WasiBoundaryStep<'_, 'a>, ExchangeError> {
        let event = self.guest.resume(budget);
        match event? {
            Event::Call(call) => {
                let pending = WasiImportPending { call, guest: self };
                pending.request()?;
                Ok(WasiBoundaryStep::ImportPending(pending))
            }
            Event::MemoryGrowPending(pending) => {
                Ok(WasiBoundaryStep::MemoryGrowPending(WasiMemoryGrowPending {
                    guest: self,
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

pub enum WasiBoundaryStep<'guest, 'module> {
    ImportPending(WasiImportPending<'guest, 'module>),
    MemoryGrowPending(WasiMemoryGrowPending<'guest, 'module>),
    BudgetExpired(BudgetExpired),
    Exit(Exit),
}

/// An affine import boundary retaining exclusive ownership of its original guest.
/// Requests borrow this token; completion consumes it before execution can resume.
///
/// ```compile_fail,E0499
/// use hibana_wasip1_runtime::{HibanaWasiGuest, WasiBoundaryStep, WasiImportCompletion};
/// use hibana_wasip1_runtime::{exchange::ExchangeError, protocol::BudgetRun};
/// fn resume_while_pending(guest: &mut HibanaWasiGuest<'_>, budget: BudgetRun,
///     completion: WasiImportCompletion<'_>) -> Result<(), ExchangeError> {
///     if let WasiBoundaryStep::ImportPending(pending) = guest.resume_wasi_boundary(budget)? {
///         guest.resume_wasi_boundary(budget)?;
///         pending.complete(completion)?;
///     }
///     Ok(())
/// }
/// ```
pub struct WasiImportPending<'guest, 'module> {
    guest: &'guest mut HibanaWasiGuest<'module>,
    call: Call,
}

impl WasiImportPending<'_, '_> {
    pub fn request(&self) -> Result<WasiImportRequest<'_>, ExchangeError> {
        lower_request(&self.guest.guest, &self.call, &self.guest.bindings)
    }

    pub fn complete(self, completion: WasiImportCompletion<'_>) -> Result<(), ExchangeError> {
        complete_call(
            self.call,
            &mut self.guest.guest,
            completion,
            &mut self.guest.bindings,
        )
    }
}

pub struct WasiMemoryGrowPending<'guest, 'module> {
    guest: &'guest mut HibanaWasiGuest<'module>,
    pending: MemoryGrowPending,
}

impl WasiMemoryGrowPending<'_, '_> {
    pub const fn request(&self) -> protocol::MemoryGrowReq {
        protocol::MemoryGrowReq(protocol::MemoryGrow::new(
            self.pending.previous_pages(),
            self.pending.requested_pages(),
            self.pending.max_pages(),
        ))
    }

    pub fn complete(self, decision: protocol::MemoryGrowRet) -> Result<(), ExchangeError> {
        self.pending
            .complete(&mut self.guest.guest, decision.0.granted())?;
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
pub enum WasiImportRequest<'a> {
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
    PollOneoff(protocol::PollOneoffReq<'a>),
    RandomGet(protocol::RandomGetReq),
}

impl WasiImportRequest<'_> {
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
pub enum WasiImportCompletion<'a> {
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
    PollOneoff(protocol::PollReadyRet<'a>),
    RandomGet(protocol::RandomDoneRet),
}

impl WasiImportCompletion<'_> {
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

fn call_import(call: &Call, bindings: &FdBindingTable) -> Result<WasiImport, ExchangeError> {
    Ok(match call {
        Call::FdWrite(call) => match bindings
            .bound_write_row(call.fd())
            .ok_or(ExchangeError::UnboundFd(call.fd()))?
        {
            FdWriteRow::Base => WasiImport::FdWrite,
            FdWriteRow::Object => WasiImport::FdWriteObject,
        },
        Call::FdRead(_) => WasiImport::FdRead,
        Call::FdReaddir(_) => WasiImport::FdReaddir,
        Call::PathOpen(_) => WasiImport::PathOpen,
        Call::FdPrestatGet(_) => WasiImport::FdPrestatGet,
        Call::FdPrestatDirName(_) => WasiImport::FdPrestatDirName,
        Call::FdFilestatGet(_) => WasiImport::FdFilestatGet,
        Call::ArgsSizesGet(_) => WasiImport::ArgsSizesGet,
        Call::ArgsGet(_) => WasiImport::ArgsGet,
        Call::EnvironSizesGet(_) => WasiImport::EnvironSizesGet,
        Call::EnvironGet(_) => WasiImport::EnvironGet,
        Call::FdFdstatGet(_) => WasiImport::FdFdstatGet,
        Call::PathFilestatGet(_) => WasiImport::PathFilestatGet,
        Call::FdClose(_) => WasiImport::FdClose,
        Call::ClockResGet(_) => WasiImport::ClockResGet,
        Call::ClockTimeGet(_) => WasiImport::ClockTimeGet,
        Call::PollOneoff(_) => WasiImport::PollOneoff,
        Call::RandomGet(_) => WasiImport::RandomGet,
    })
}

fn complete_call(
    call: Call,
    guest: &mut Guest<'_>,
    completion: WasiImportCompletion<'_>,
    bindings: &mut FdBindingTable,
) -> Result<(), ExchangeError> {
    let pending = call_import(&call, bindings)?;
    let completed = completion.import();
    if pending != completed {
        return Err(ExchangeError::CompletionMismatch {
            pending,
            completion: completed,
        });
    }

    match (call, completion) {
        (
            Call::FdWrite(call),
            WasiImportCompletion::FdWrite(done) | WasiImportCompletion::FdWriteObject(done),
        ) => {
            expect_fd(pending, call.fd(), done.0.fd())?;
            call.complete(guest, done.0.written() as u32, done.0.errno() as u32)?;
        }
        (Call::FdRead(call), WasiImportCompletion::FdRead(done)) => {
            expect_fd(WasiImport::FdRead, call.fd(), done.0.fd())?;
            call.complete(guest, done.0.as_bytes(), done.0.errno() as u32)?;
        }
        (Call::FdReaddir(call), WasiImportCompletion::FdReaddir(done)) => {
            expect_fd(WasiImport::FdReaddir, call.fd(), done.0.fd())?;
            call.complete(guest, done.0.as_bytes(), done.0.errno() as u32)?;
        }
        (Call::PathOpen(call), WasiImportCompletion::PathOpen(opened)) => {
            let prepared_bindings = prepare_path_open_bindings(*bindings, opened.0)?;
            call.complete(guest, opened.0.fd() as u32, opened.0.errno() as u32)?;
            *bindings = prepared_bindings;
        }
        (Call::FdPrestatGet(call), WasiImportCompletion::FdPrestatGet(prestat)) => {
            expect_fd(WasiImport::FdPrestatGet, call.fd(), prestat.0.fd())?;
            call.complete(guest, prestat.0.name_len() as u32, prestat.0.errno() as u32)?;
        }
        (Call::FdPrestatDirName(call), WasiImportCompletion::FdPrestatDirName(name)) => {
            expect_fd(WasiImport::FdPrestatDirName, call.fd(), name.0.fd())?;
            call.complete(guest, name.0.as_bytes(), name.0.errno() as u32)?;
        }
        (Call::FdFilestatGet(call), WasiImportCompletion::FdFilestatGet(stat)) => {
            call.complete(guest, wasm_file_stat(stat.0), stat.0.errno() as u32)?;
        }
        (Call::ArgsSizesGet(call), WasiImportCompletion::ArgsSizesGet(sizes)) => {
            call.complete(guest, sizes.0.count() as u32, sizes.0.buf_size() as u32, 0)?;
        }
        (Call::ArgsGet(call), WasiImportCompletion::ArgsGet(done)) => {
            let mut args = [&[][..]; MAX_ARG_REFS];
            let count = split_args(done.0.as_bytes(), done.0.count(), &mut args)?;
            call.complete(guest, &args[..count], 0)?;
        }
        (Call::EnvironSizesGet(call), WasiImportCompletion::EnvironSizesGet(sizes)) => {
            call.complete(guest, sizes.0.count() as u32, sizes.0.buf_size() as u32, 0)?;
        }
        (Call::EnvironGet(call), WasiImportCompletion::EnvironGet(done)) => {
            let mut environ = [(&[][..], &[][..]); MAX_ENV_REFS];
            let count = split_environ(done.0.as_bytes(), done.0.count(), &mut environ)?;
            call.complete(guest, &environ[..count], 0)?;
        }
        (Call::FdFdstatGet(call), WasiImportCompletion::FdFdstatGet(stat)) => {
            expect_fd(WasiImport::FdFdstatGet, call.fd(), stat.0.fd())?;
            call.complete(guest, wasm_fd_stat(stat.0), stat.0.errno() as u32)?;
        }
        (Call::PathFilestatGet(call), WasiImportCompletion::PathFilestatGet(stat)) => {
            call.complete(guest, wasm_file_stat(stat.0), stat.0.errno() as u32)?;
        }
        (Call::FdClose(call), WasiImportCompletion::FdClose(closed)) => {
            expect_fd(WasiImport::FdClose, call.fd(), closed.0.fd())?;
            let prepared_bindings =
                prepare_fd_close_bindings(*bindings, call.fd(), closed.0.errno());
            call.complete(guest, closed.0.errno() as u32)?;
            *bindings = prepared_bindings;
        }
        (Call::ClockResGet(call), WasiImportCompletion::ClockResGet(resolution)) => {
            call.complete(guest, resolution.0.nanos(), 0)?;
        }
        (Call::ClockTimeGet(call), WasiImportCompletion::ClockTimeGet(time)) => {
            call.complete(guest, time.0.nanos(), 0)?;
        }
        (Call::PollOneoff(call), WasiImportCompletion::PollOneoff(ready)) => {
            call.complete(guest, ready.0)?;
        }
        (Call::RandomGet(call), WasiImportCompletion::RandomGet(done)) => {
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

fn lower_request<'a>(
    guest: &'a Guest<'_>,
    call: &Call,
    bindings: &FdBindingTable,
) -> Result<WasiImportRequest<'a>, ExchangeError> {
    Ok(match call {
        Call::FdWrite(call) => {
            let request = protocol::FdWriteReq(protocol::FdWrite::new(
                call.fd(),
                call.payload(guest)?.as_bytes(),
            )?);
            match bindings
                .bound_write_row(call.fd())
                .ok_or(ExchangeError::UnboundFd(call.fd()))?
            {
                FdWriteRow::Base => WasiImportRequest::FdWrite(request),
                FdWriteRow::Object => WasiImportRequest::FdWriteObject(request),
            }
        }
        Call::FdRead(call) => {
            bindings
                .bound_read_row(call.fd())
                .ok_or(ExchangeError::UnboundFd(call.fd()))?;
            WasiImportRequest::FdRead(protocol::FdReadReq(protocol::FdRead::new(
                call.fd(),
                inline_io_request_len(call.max_len(guest)?),
            )?))
        }
        Call::FdReaddir(call) => {
            bindings
                .bound_readdir_row(call.fd())
                .ok_or(ExchangeError::UnboundFd(call.fd()))?;
            WasiImportRequest::FdReaddir(protocol::FdReaddirReq(protocol::FdReaddir::new(
                call.fd(),
                call.cookie(),
                inline_io_request_len(call.max_len()),
            )?))
        }
        Call::PathOpen(call) => {
            WasiImportRequest::PathOpen(protocol::PathOpenReq(protocol::PathOpen::new(
                call.fd(),
                call.rights_base(),
                call.path_bytes(guest)?.as_bytes(),
            )?))
        }
        Call::FdPrestatGet(call) => {
            WasiImportRequest::FdPrestatGet(protocol::FdPrestatGetReq(FdRequest::new(call.fd())))
        }
        Call::FdPrestatDirName(call) => {
            WasiImportRequest::FdPrestatDirName(protocol::FdPrestatDirNameReq(
                protocol::FdPrestatDirName::new(call.fd(), exact_path_reply_len(call.max_len())?)?,
            ))
        }
        Call::FdFilestatGet(call) => {
            WasiImportRequest::FdFilestatGet(protocol::FdFilestatGetReq(FdRequest::new(call.fd())))
        }
        Call::ArgsSizesGet(_) => {
            WasiImportRequest::ArgsSizesGet(protocol::ArgsSizesGetReq(protocol::ArgsSizesGet))
        }
        Call::ArgsGet(_) => WasiImportRequest::ArgsGet(protocol::ArgsGetReq(ArgsGet::new(
            WASIP1_IO_CHUNK_CAPACITY as u8,
        )?)),
        Call::EnvironSizesGet(_) => WasiImportRequest::EnvironSizesGet(
            protocol::EnvironSizesGetReq(protocol::EnvironSizesGet),
        ),
        Call::EnvironGet(_) => WasiImportRequest::EnvironGet(protocol::EnvironGetReq(
            EnvironGet::new(WASIP1_IO_CHUNK_CAPACITY as u8)?,
        )),
        Call::FdFdstatGet(call) => {
            WasiImportRequest::FdFdstatGet(protocol::FdFdstatGetReq(FdRequest::new(call.fd())))
        }
        Call::PathFilestatGet(call) => WasiImportRequest::PathFilestatGet(
            protocol::PathFilestatGetReq(protocol::PathFilestatGet::new(
                call.fd(),
                call.flags(),
                call.path_bytes(guest)?.as_bytes(),
            )?),
        ),
        Call::FdClose(call) => {
            WasiImportRequest::FdClose(protocol::FdCloseReq(FdRequest::new(call.fd())))
        }
        Call::ClockResGet(call) => WasiImportRequest::ClockResGet(protocol::ClockResGetReq(
            ClockResGet::new(clock_id_u8(call.clock_id())?),
        )),
        Call::ClockTimeGet(call) => WasiImportRequest::ClockTimeGet(protocol::ClockTimeGetReq(
            ClockTimeGet::new(clock_id_u8(call.clock_id())?, call.precision()),
        )),
        Call::PollOneoff(call) => {
            WasiImportRequest::PollOneoff(protocol::PollOneoffReq(call.request(guest)?))
        }
        Call::RandomGet(call) => WasiImportRequest::RandomGet(protocol::RandomGetReq(
            RandomGet::new(exact_io_reply_len(call.buf_len())?)?,
        )),
    })
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

fn prepare_path_open_bindings(
    mut bindings: FdBindingTable,
    opened: protocol::PathOpened,
) -> Result<FdBindingTable, FdBindingCapacityError> {
    if opened.errno() == 0 && !opened.binding().is_empty() {
        bindings.bind_fd(opened.fd(), opened.binding())?;
    }
    Ok(bindings)
}

fn prepare_fd_close_bindings(mut bindings: FdBindingTable, fd: u8, errno: u16) -> FdBindingTable {
    if errno == 0 {
        bindings.remove_fd(fd);
    }
    bindings
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

fn split_args<'a>(
    mut bytes: &'a [u8],
    count: usize,
    out: &mut [&'a [u8]; MAX_ARG_REFS],
) -> Result<usize, CodecError> {
    let slots = out.get_mut(..count).ok_or(CodecError::Malformed)?;
    for slot in slots {
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .ok_or(CodecError::Malformed)?;
        *slot = &bytes[..end];
        bytes = &bytes[end + 1..];
    }
    if !bytes.is_empty() {
        return Err(CodecError::Malformed);
    }
    Ok(count)
}

fn split_environ<'a>(
    mut bytes: &'a [u8],
    count: usize,
    out: &mut [(&'a [u8], &'a [u8]); MAX_ENV_REFS],
) -> Result<usize, CodecError> {
    let slots = out.get_mut(..count).ok_or(CodecError::Malformed)?;
    for slot in slots {
        let end = bytes
            .iter()
            .position(|byte| *byte == 0)
            .ok_or(CodecError::Malformed)?;
        let entry = &bytes[..end];
        let separator = entry
            .iter()
            .position(|byte| *byte == b'=')
            .filter(|separator| *separator != 0)
            .ok_or(CodecError::Malformed)?;
        *slot = (&entry[..separator], &entry[separator + 1..]);
        bytes = &bytes[end + 1..];
    }
    if !bytes.is_empty() {
        return Err(CodecError::Malformed);
    }
    Ok(count)
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
mod tests;
