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
        FdRequest, FdWriteRow, MemRights, PollOneoff, RandomGet, WASIP1_IO_CHUNK_CAPACITY,
    },
};
use hibana::{Endpoint, EndpointError, runtime::wire::CodecError};

const FD_READ_RIGHT: u64 = 1 << 1;
const FD_WRITE_RIGHT: u64 = 1 << 6;
const FD_READDIR_RIGHT: u64 = 1 << 14;
const MAX_ARG_REFS: usize = WASIP1_IO_CHUNK_CAPACITY;
pub const FD_BINDING_CAPACITY: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowId {
    FdWrite,
    FdWriteRefined,
    FdRead,
    FdReaddir,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdBindingTable {
    entries: [Option<FdBinding>; FD_BINDING_CAPACITY],
}

impl FdBindingTable {
    pub const fn empty() -> Self {
        Self {
            entries: [None; FD_BINDING_CAPACITY],
        }
    }

    pub fn bind_fd(&mut self, fd: u8, binding: FdBinding) -> bool {
        let Some(slot) = self.entries.get_mut(fd as usize) else {
            return false;
        };
        *slot = Some(binding);
        true
    }

    pub fn remove_fd(&mut self, fd: u8) {
        if let Some(slot) = self.entries.get_mut(fd as usize) {
            *slot = None;
        }
    }

    pub fn binding(&self, fd: u8) -> Option<FdBinding> {
        self.entries.get(fd as usize).and_then(|binding| *binding)
    }

    pub fn bound_write_row(&self, fd: u8) -> Option<RowId> {
        self.binding(fd)
            .and_then(|binding| binding.write)
            .map(fd_write_row_id)
    }

    pub fn bound_read_row(&self, fd: u8) -> Option<RowId> {
        self.binding(fd).and_then(|binding| {
            if binding.read.is_some() {
                Some(RowId::FdRead)
            } else {
                None
            }
        })
    }

    pub fn bound_readdir_row(&self, fd: u8) -> Option<RowId> {
        self.binding(fd).and_then(|binding| {
            if binding.readdir.is_some() {
                Some(RowId::FdReaddir)
            } else {
                None
            }
        })
    }
}

const fn fd_write_row_id(row: FdWriteRow) -> RowId {
    match row {
        FdWriteRow::Base => RowId::FdWrite,
        FdWriteRow::Refined => RowId::FdWriteRefined,
    }
}

#[derive(Debug)]
pub enum ExchangeError {
    Endpoint(EndpointError),
    Codec(CodecError),
    Wasm(WasmError),
    FdBindingCapacity,
    UnboundFd(u8),
    GuestStorageAlreadyInitialized,
}

impl From<EndpointError> for ExchangeError {
    fn from(error: EndpointError) -> Self {
        Self::Endpoint(error)
    }
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

    pub async fn resume_hibana<const ROLE: u8>(
        &mut self,
        endpoint: &mut Endpoint<'_, ROLE>,
        budget: BudgetRun,
    ) -> Result<HibanaStep, ExchangeError> {
        match self.guest.resume(budget)? {
            Event::Call(call) => {
                let pending = send_call(&self.guest, call, endpoint, &self.bindings).await?;
                Ok(HibanaStep::ImportPending(HibanaImportPending { pending }))
            }
            Event::MemoryGrowPending(pending) => {
                let request = protocol::MemoryGrowReq(protocol::MemoryGrow::new(
                    pending.previous_pages(),
                    pending.requested_pages(),
                    pending.max_pages(),
                ));
                endpoint
                    .send::<protocol::MemoryGrowReqMsg>(&request)
                    .await?;
                Ok(HibanaStep::MemoryGrowPending(HibanaMemoryGrowPending {
                    pending,
                }))
            }
            Event::BudgetExpired(expired) => Ok(HibanaStep::BudgetExpired(expired)),
            Event::Exit(exit) => Ok(HibanaStep::Exit(exit)),
        }
    }

    pub const fn import_plan_diagnostics(&self) -> ImportPlanDiagnostics {
        self.guest.import_plan_diagnostics()
    }
}

pub enum HibanaStep {
    ImportPending(HibanaImportPending),
    MemoryGrowPending(HibanaMemoryGrowPending),
    BudgetExpired(BudgetExpired),
    Exit(Exit),
}

pub struct HibanaImportPending {
    pending: PendingCall,
}

impl HibanaImportPending {
    pub async fn complete<const ROLE: u8>(
        self,
        guest: &mut HibanaWasiGuest<'_>,
        endpoint: &mut Endpoint<'_, ROLE>,
    ) -> Result<(), ExchangeError> {
        self.pending
            .complete(&mut guest.guest, endpoint, &mut guest.bindings)
            .await
    }
}

pub struct HibanaMemoryGrowPending {
    pending: MemoryGrowPending,
}

impl HibanaMemoryGrowPending {
    pub const fn previous_pages(&self) -> u32 {
        self.pending.previous_pages()
    }

    pub const fn requested_pages(&self) -> u32 {
        self.pending.requested_pages()
    }

    pub const fn max_pages(&self) -> u32 {
        self.pending.max_pages()
    }

    pub async fn complete<const ROLE: u8>(
        self,
        guest: &mut HibanaWasiGuest<'_>,
        endpoint: &mut Endpoint<'_, ROLE>,
    ) -> Result<(), ExchangeError> {
        let decision = endpoint.recv::<protocol::MemoryGrowRetMsg>().await?;
        self.pending
            .complete(&mut guest.guest, decision.0.granted())?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasiImport {
    FdWrite,
    FdWriteRefined,
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
            protocol::LABEL_WASI_FD_WRITE_REFINED => Some(Self::FdWriteRefined),
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

enum PendingCall {
    FdWrite(wasm::FdWrite),
    FdWriteRefined(wasm::FdWrite),
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
    pub async fn complete<const ROLE: u8>(
        self,
        guest: &mut Guest<'_>,
        endpoint: &mut Endpoint<'_, ROLE>,
        bindings: &mut FdBindingTable,
    ) -> Result<(), ExchangeError> {
        match self {
            Self::FdWrite(call) => {
                let done = endpoint.recv::<protocol::FdWriteRetMsg>().await?;
                call.complete(guest, done.0.errno() as u32)?;
            }
            Self::FdWriteRefined(call) => {
                let done = endpoint.recv::<protocol::FdWriteRefinedRetMsg>().await?;
                call.complete(guest, done.0.errno() as u32)?;
            }
            Self::FdRead(call) => {
                let done = endpoint.recv::<protocol::FdReadRetMsg>().await?;
                call.complete(guest, done.0.as_bytes(), 0)?;
            }
            Self::FdReaddir(call) => {
                let done = endpoint.recv::<protocol::FdReaddirRetMsg>().await?;
                call.complete(guest, done.0.as_bytes(), done.0.errno() as u32)?;
            }
            Self::PathOpen(call) => {
                let opened = endpoint.recv::<protocol::PathOpenRetMsg>().await?;
                call.complete(guest, opened.0.fd() as u32, opened.0.errno() as u32)?;
                if opened.0.errno() == 0
                    && !opened.0.binding().is_empty()
                    && !bindings.bind_fd(opened.0.fd(), opened.0.binding())
                {
                    return Err(ExchangeError::FdBindingCapacity);
                }
            }
            Self::FdPrestatGet(call) => {
                let prestat = endpoint.recv::<protocol::FdPrestatGetRetMsg>().await?;
                call.complete(guest, prestat.0.name_len() as u32, prestat.0.errno() as u32)?;
            }
            Self::FdPrestatDirName(call) => {
                let name = endpoint.recv::<protocol::FdPrestatDirNameRetMsg>().await?;
                call.complete(guest, name.0.as_bytes(), name.0.errno() as u32)?;
            }
            Self::FdFilestatGet(call) => {
                let stat = endpoint.recv::<protocol::FdFilestatGetRetMsg>().await?;
                call.complete(guest, wasm_file_stat(stat.0), stat.0.errno() as u32)?;
            }
            Self::ArgsSizesGet(call) => {
                let sizes = endpoint.recv::<protocol::ArgsSizesGetRetMsg>().await?;
                call.complete(guest, sizes.0.count() as u32, sizes.0.buf_size() as u32, 0)?;
            }
            Self::ArgsGet(call) => {
                let done = endpoint.recv::<protocol::ArgsGetRetMsg>().await?;
                let mut args = [&[][..]; MAX_ARG_REFS];
                let count = split_args(done.0.as_bytes(), &mut args);
                call.complete(guest, &args[..count], 0)?;
            }
            Self::EnvironSizesGet(call) => {
                let sizes = endpoint.recv::<protocol::EnvironSizesGetRetMsg>().await?;
                call.complete(guest, sizes.0.count() as u32, sizes.0.buf_size() as u32, 0)?;
            }
            Self::EnvironGet(call) => {
                let _done = endpoint.recv::<protocol::EnvironGetRetMsg>().await?;
                call.complete(guest, &[], 0)?;
            }
            Self::FdFdstatGet(call) => {
                let stat = endpoint.recv::<protocol::FdFdstatGetRetMsg>().await?;
                call.complete(guest, wasm_fd_stat(stat.0), 0)?;
            }
            Self::PathFilestatGet(call) => {
                let stat = endpoint.recv::<protocol::PathFilestatGetRetMsg>().await?;
                call.complete(guest, wasm_file_stat(stat.0), stat.0.errno() as u32)?;
            }
            Self::FdClose(call) => {
                let _closed = endpoint.recv::<protocol::FdCloseRetMsg>().await?;
                bindings.remove_fd(call.fd());
                call.complete(guest, 0)?;
            }
            Self::ClockResGet(call) => {
                let resolution = endpoint.recv::<protocol::ClockResGetRetMsg>().await?;
                call.complete(guest, resolution.0.nanos(), 0)?;
            }
            Self::ClockTimeGet(call) => {
                let time = endpoint.recv::<protocol::ClockTimeGetRetMsg>().await?;
                call.complete(guest, time.0.nanos(), 0)?;
            }
            Self::PollOneoff(call) => {
                let ready = endpoint.recv::<protocol::PollOneoffRetMsg>().await?;
                call.complete(guest, ready.0.ready() as u32, 0)?;
            }
            Self::RandomGet(call) => {
                let done = endpoint.recv::<protocol::RandomGetRetMsg>().await?;
                call.complete(guest, done.0.as_bytes(), 0)?;
            }
        }
        Ok(())
    }
}

async fn send_call<const ROLE: u8>(
    guest: &Guest<'_>,
    call: Call,
    endpoint: &mut Endpoint<'_, ROLE>,
    bindings: &FdBindingTable,
) -> Result<PendingCall, ExchangeError> {
    match call {
        Call::FdWrite(call) => {
            let payload = call.payload(guest)?;
            let request =
                protocol::FdWriteReq(protocol::FdWrite::new(call.fd(), payload.as_bytes())?);
            let row = bindings
                .bound_write_row(call.fd())
                .ok_or(ExchangeError::UnboundFd(call.fd()))?;
            if row == RowId::FdWriteRefined {
                endpoint
                    .send::<protocol::FdWriteRefinedReqMsg>(&request)
                    .await?;
                Ok(PendingCall::FdWriteRefined(call))
            } else {
                endpoint.send::<protocol::FdWriteReqMsg>(&request).await?;
                Ok(PendingCall::FdWrite(call))
            }
        }
        Call::FdRead(call) => {
            bindings
                .bound_read_row(call.fd())
                .ok_or(ExchangeError::UnboundFd(call.fd()))?;
            let max_len = bounded_u8(call.max_len(guest)?);
            let request = protocol::FdReadReq(protocol::FdRead::new(call.fd(), max_len)?);
            endpoint.send::<protocol::FdReadReqMsg>(&request).await?;
            Ok(PendingCall::FdRead(call))
        }
        Call::FdReaddir(call) => {
            bindings
                .bound_readdir_row(call.fd())
                .ok_or(ExchangeError::UnboundFd(call.fd()))?;
            let request = protocol::FdReaddirReq(protocol::FdReaddir::new(
                call.fd(),
                call.cookie(),
                bounded_u8(call.max_len()),
            )?);
            endpoint.send::<protocol::FdReaddirReqMsg>(&request).await?;
            Ok(PendingCall::FdReaddir(call))
        }
        Call::PathOpen(call) => {
            let path = call.path_bytes(guest)?;
            let request = protocol::PathOpenReq(protocol::PathOpen::new(
                call.fd(),
                call.rights_base(),
                path.as_bytes(),
            )?);
            endpoint.send::<protocol::PathOpenReqMsg>(&request).await?;
            Ok(PendingCall::PathOpen(call))
        }
        Call::FdPrestatGet(call) => {
            let request = protocol::FdPrestatGetReq(FdRequest::new(call.fd()));
            endpoint
                .send::<protocol::FdPrestatGetReqMsg>(&request)
                .await?;
            Ok(PendingCall::FdPrestatGet(call))
        }
        Call::FdPrestatDirName(call) => {
            let request = protocol::FdPrestatDirNameReq(protocol::FdPrestatDirName::new(
                call.fd(),
                bounded_u8(call.max_len()),
            )?);
            endpoint
                .send::<protocol::FdPrestatDirNameReqMsg>(&request)
                .await?;
            Ok(PendingCall::FdPrestatDirName(call))
        }
        Call::FdFilestatGet(call) => {
            let request = protocol::FdFilestatGetReq(FdRequest::new(call.fd()));
            endpoint
                .send::<protocol::FdFilestatGetReqMsg>(&request)
                .await?;
            Ok(PendingCall::FdFilestatGet(call))
        }
        Call::ArgsSizesGet(call) => {
            let request = protocol::ArgsSizesGetReq(protocol::ArgsSizesGet);
            endpoint
                .send::<protocol::ArgsSizesGetReqMsg>(&request)
                .await?;
            Ok(PendingCall::ArgsSizesGet(call))
        }
        Call::ArgsGet(call) => {
            let request = protocol::ArgsGetReq(ArgsGet::new(WASIP1_IO_CHUNK_CAPACITY as u8)?);
            endpoint.send::<protocol::ArgsGetReqMsg>(&request).await?;
            Ok(PendingCall::ArgsGet(call))
        }
        Call::EnvironSizesGet(call) => {
            let request = protocol::EnvironSizesGetReq(protocol::EnvironSizesGet);
            endpoint
                .send::<protocol::EnvironSizesGetReqMsg>(&request)
                .await?;
            Ok(PendingCall::EnvironSizesGet(call))
        }
        Call::EnvironGet(call) => {
            let request = protocol::EnvironGetReq(EnvironGet::new(WASIP1_IO_CHUNK_CAPACITY as u8)?);
            endpoint
                .send::<protocol::EnvironGetReqMsg>(&request)
                .await?;
            Ok(PendingCall::EnvironGet(call))
        }
        Call::FdFdstatGet(call) => {
            let request = protocol::FdFdstatGetReq(FdRequest::new(call.fd()));
            endpoint
                .send::<protocol::FdFdstatGetReqMsg>(&request)
                .await?;
            Ok(PendingCall::FdFdstatGet(call))
        }
        Call::PathFilestatGet(call) => {
            let path = call.path_bytes(guest)?;
            let request = protocol::PathFilestatGetReq(protocol::PathFilestatGet::new(
                call.fd(),
                call.flags(),
                path.as_bytes(),
            )?);
            endpoint
                .send::<protocol::PathFilestatGetReqMsg>(&request)
                .await?;
            Ok(PendingCall::PathFilestatGet(call))
        }
        Call::FdClose(call) => {
            let request = protocol::FdCloseReq(FdRequest::new(call.fd()));
            endpoint.send::<protocol::FdCloseReqMsg>(&request).await?;
            Ok(PendingCall::FdClose(call))
        }
        Call::ClockResGet(call) => {
            let request = protocol::ClockResGetReq(ClockResGet::new(bounded_u8(call.clock_id())));
            endpoint
                .send::<protocol::ClockResGetReqMsg>(&request)
                .await?;
            Ok(PendingCall::ClockResGet(call))
        }
        Call::ClockTimeGet(call) => {
            let request = protocol::ClockTimeGetReq(ClockTimeGet::new(
                bounded_u8(call.clock_id()),
                call.precision(),
            ));
            endpoint
                .send::<protocol::ClockTimeGetReqMsg>(&request)
                .await?;
            Ok(PendingCall::ClockTimeGet(call))
        }
        Call::PollOneoff(call) => {
            let request = protocol::PollOneoffReq(PollOneoff::new(call.delay_ticks(guest)?));
            endpoint
                .send::<protocol::PollOneoffReqMsg>(&request)
                .await?;
            Ok(PendingCall::PollOneoff(call))
        }
        Call::RandomGet(call) => {
            let request = protocol::RandomGetReq(RandomGet::new(bounded_u8(call.buf_len()))?);
            endpoint.send::<protocol::RandomGetReqMsg>(&request).await?;
            Ok(PendingCall::RandomGet(call))
        }
    }
}

fn bounded_u8(value: impl TryInto<usize>) -> u8 {
    let value = match value.try_into() {
        Ok(value) => value,
        Err(_) => WASIP1_IO_CHUNK_CAPACITY,
    };
    value.min(WASIP1_IO_CHUNK_CAPACITY) as u8
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
    use super::{FdBindingTable, HibanaImportPending, PendingCall};
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
            size_of::<HibanaImportPending>() <= 64,
            "HibanaImportPending uses {} bytes",
            size_of::<HibanaImportPending>()
        );
    }
}
