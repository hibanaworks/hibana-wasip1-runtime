use super::*;

pub type MemoryGrowReqMsg = Msg<LABEL_ENGINE_MEMORY_GROW, MemoryGrowReq>;
pub type MemoryGrowRetMsg = Msg<LABEL_ENGINE_MEMORY_GROW_RET, MemoryGrowRet>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemRights {
    Read,
    Write,
}

impl MemRights {
    pub const fn tag(self) -> u8 {
        match self {
            Self::Read => 1,
            Self::Write => 2,
        }
    }

    fn decode(tag: u8) -> Result<Self, CodecError> {
        match tag {
            1 => Ok(Self::Read),
            2 => Ok(Self::Write),
            _ => Err(CodecError::Malformed),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetRun {
    run_id: u16,
    generation: u16,
    fuel: u32,
}

impl BudgetRun {
    pub const fn new(run_id: u16, generation: u16, fuel: u32) -> Self {
        Self {
            run_id,
            generation,
            fuel,
        }
    }

    pub const fn run_id(&self) -> u16 {
        self.run_id
    }

    pub const fn generation(&self) -> u16 {
        self.generation
    }

    pub const fn fuel(&self) -> u32 {
        self.fuel
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 8 {
            return Err(CodecError::Malformed);
        }
        Ok(Self::new(
            u16::from_be_bytes([bytes[0], bytes[1]]),
            u16::from_be_bytes([bytes[2], bytes[3]]),
            u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        ))
    }
}

impl WireEncode for BudgetRun {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 8 {
            return Err(CodecError::Truncated);
        }
        out[0..2].copy_from_slice(&self.run_id.to_be_bytes());
        out[2..4].copy_from_slice(&self.generation.to_be_bytes());
        out[4..8].copy_from_slice(&self.fuel.to_be_bytes());
        Ok(8)
    }
}

impl WirePayload for BudgetRun {
    type Decoded<'a> = Self;

    wire_payload_via_decode!();

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        Self::decode(input.as_bytes())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetExpired {
    run_id: u16,
    generation: u16,
}

impl BudgetExpired {
    pub const fn new(run_id: u16, generation: u16) -> Self {
        Self { run_id, generation }
    }

    pub const fn run_id(&self) -> u16 {
        self.run_id
    }

    pub const fn generation(&self) -> u16 {
        self.generation
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 4 {
            return Err(CodecError::Malformed);
        }
        Ok(Self::new(
            u16::from_be_bytes([bytes[0], bytes[1]]),
            u16::from_be_bytes([bytes[2], bytes[3]]),
        ))
    }
}

impl WireEncode for BudgetExpired {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 4 {
            return Err(CodecError::Truncated);
        }
        out[0..2].copy_from_slice(&self.run_id.to_be_bytes());
        out[2..4].copy_from_slice(&self.generation.to_be_bytes());
        Ok(4)
    }
}

impl WirePayload for BudgetExpired {
    type Decoded<'a> = Self;

    wire_payload_via_decode!();

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        Self::decode(input.as_bytes())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryGrow {
    previous_pages: u32,
    requested_pages: u32,
    max_pages: u32,
}

impl MemoryGrow {
    pub const fn new(previous_pages: u32, requested_pages: u32, max_pages: u32) -> Self {
        Self {
            previous_pages,
            requested_pages,
            max_pages,
        }
    }

    pub const fn previous_pages(&self) -> u32 {
        self.previous_pages
    }

    pub const fn requested_pages(&self) -> u32 {
        self.requested_pages
    }

    pub const fn max_pages(&self) -> u32 {
        self.max_pages
    }

    pub const fn would_fit(&self) -> bool {
        match self.previous_pages.checked_add(self.requested_pages) {
            Some(new_pages) => new_pages <= self.max_pages,
            None => false,
        }
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 12 {
            return Err(CodecError::Malformed);
        }
        Ok(Self::new(
            u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            u32::from_be_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
        ))
    }
}

impl WireEncode for MemoryGrow {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 12 {
            return Err(CodecError::Truncated);
        }
        out[0..4].copy_from_slice(&self.previous_pages.to_be_bytes());
        out[4..8].copy_from_slice(&self.requested_pages.to_be_bytes());
        out[8..12].copy_from_slice(&self.max_pages.to_be_bytes());
        Ok(12)
    }
}

impl WirePayload for MemoryGrow {
    type Decoded<'a> = Self;

    wire_payload_via_decode!();

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        Self::decode(input.as_bytes())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryGrowDecision {
    grant: bool,
}

impl MemoryGrowDecision {
    pub const fn grant() -> Self {
        Self { grant: true }
    }

    pub const fn reject() -> Self {
        Self { grant: false }
    }

    pub const fn granted(&self) -> bool {
        self.grant
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 1 {
            return Err(CodecError::Malformed);
        }
        match bytes[0] {
            0 => Ok(Self::reject()),
            1 => Ok(Self::grant()),
            _ => Err(CodecError::Malformed),
        }
    }
}

impl WireEncode for MemoryGrowDecision {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.is_empty() {
            return Err(CodecError::Truncated);
        }
        out[0] = u8::from(self.grant);
        Ok(1)
    }
}

impl WirePayload for MemoryGrowDecision {
    type Decoded<'a> = Self;

    wire_payload_via_decode!();

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        Self::decode(input.as_bytes())
    }
}

trait TypedWasiPayload: Sized {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError>;
    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError>;
}

macro_rules! engine_req_payload {
    ($wrapper:ident, $payload:ty) => {
        impl WireEncode for $wrapper {
            fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
                self.0.encode_payload(out)
            }
        }

        impl WirePayload for $wrapper {
            type Decoded<'a> = Self;

            wire_payload_via_decode!();

            fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
                <$payload as TypedWasiPayload>::decode_payload_bytes(input.as_bytes()).map(Self)
            }
        }
    };
}

macro_rules! engine_ret_payload {
    ($wrapper:ident, $payload:ty) => {
        impl WireEncode for $wrapper {
            fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
                self.0.encode_payload(out)
            }
        }

        impl WirePayload for $wrapper {
            type Decoded<'a> = Self;

            wire_payload_via_decode!();

            fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
                <$payload as TypedWasiPayload>::decode_payload_bytes(input.as_bytes()).map(Self)
            }
        }
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdWriteReq(pub FdWrite);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdReadReq(pub FdRead);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdReaddirReq(pub FdReaddir);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdFdstatGetReq(pub FdRequest);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdPrestatGetReq(pub FdRequest);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdPrestatDirNameReq(pub FdPrestatDirName);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdFilestatGetReq(pub FdRequest);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdCloseReq(pub FdRequest);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockResGetReq(pub ClockResGet);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockTimeGetReq(pub ClockTimeGet);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PollOneoffReq(pub PollOneoff);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RandomGetReq(pub RandomGet);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArgsSizesGetReq(pub ArgsSizesGet);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArgsGetReq(pub ArgsGet);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvironSizesGetReq(pub EnvironSizesGet);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvironGetReq(pub EnvironGet);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathOpenReq(pub PathOpen);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathFilestatGetReq(pub PathFilestatGet);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryGrowReq(pub MemoryGrow);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdWriteDoneRet(pub FdWriteDone);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdReadDoneRet(pub FdReadDone);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdReaddirDoneRet(pub FdReaddirDone);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdStatRet(pub FdStat);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdPrestatRet(pub FdPrestat);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdPrestatDirNameRet(pub FdPrestatDirNameDone);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdFilestatRet(pub FileStat);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathFilestatRet(pub FileStat);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdClosedRet(pub FdClosed);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockResolutionRet(pub ClockResolution);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockTimeRet(pub ClockTime);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PollReadyRet(pub PollReady);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RandomDoneRet(pub RandomDone);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArgsSizesRet(pub ArgsSizes);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArgsDoneRet(pub ArgsDone);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvironSizesRet(pub EnvironSizes);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvironDoneRet(pub EnvironDone);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathOpenedRet(pub PathOpened);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryGrowRet(pub MemoryGrowDecision);

engine_req_payload!(FdWriteReq, FdWrite);
engine_req_payload!(FdReadReq, FdRead);
engine_req_payload!(FdReaddirReq, FdReaddir);
engine_req_payload!(FdFdstatGetReq, FdRequest);
engine_req_payload!(FdPrestatGetReq, FdRequest);
engine_req_payload!(FdPrestatDirNameReq, FdPrestatDirName);
engine_req_payload!(FdFilestatGetReq, FdRequest);
engine_req_payload!(FdCloseReq, FdRequest);
engine_req_payload!(ClockResGetReq, ClockResGet);
engine_req_payload!(ClockTimeGetReq, ClockTimeGet);
engine_req_payload!(PollOneoffReq, PollOneoff);
engine_req_payload!(RandomGetReq, RandomGet);
engine_req_payload!(ArgsSizesGetReq, ArgsSizesGet);
engine_req_payload!(ArgsGetReq, ArgsGet);
engine_req_payload!(EnvironSizesGetReq, EnvironSizesGet);
engine_req_payload!(EnvironGetReq, EnvironGet);
engine_req_payload!(PathOpenReq, PathOpen);
engine_req_payload!(PathFilestatGetReq, PathFilestatGet);

impl WireEncode for MemoryGrowReq {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        self.0.encode_into(out)
    }
}

impl WirePayload for MemoryGrowReq {
    type Decoded<'a> = Self;

    wire_payload_via_decode!();

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        MemoryGrow::decode_payload(input).map(Self)
    }
}

engine_ret_payload!(FdWriteDoneRet, FdWriteDone);
engine_ret_payload!(FdReadDoneRet, FdReadDone);
engine_ret_payload!(FdReaddirDoneRet, FdReaddirDone);
engine_ret_payload!(FdStatRet, FdStat);
engine_ret_payload!(FdPrestatRet, FdPrestat);
engine_ret_payload!(FdPrestatDirNameRet, FdPrestatDirNameDone);
engine_ret_payload!(FdFilestatRet, FileStat);
engine_ret_payload!(PathFilestatRet, FileStat);
engine_ret_payload!(FdClosedRet, FdClosed);
engine_ret_payload!(ClockResolutionRet, ClockResolution);
engine_ret_payload!(ClockTimeRet, ClockTime);
engine_ret_payload!(PollReadyRet, PollReady);
engine_ret_payload!(RandomDoneRet, RandomDone);
engine_ret_payload!(ArgsSizesRet, ArgsSizes);
engine_ret_payload!(ArgsDoneRet, ArgsDone);
engine_ret_payload!(EnvironSizesRet, EnvironSizes);
engine_ret_payload!(EnvironDoneRet, EnvironDone);
engine_ret_payload!(PathOpenedRet, PathOpened);

impl WireEncode for MemoryGrowRet {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        self.0.encode_into(out)
    }
}

impl WirePayload for MemoryGrowRet {
    type Decoded<'a> = Self;

    wire_payload_via_decode!();

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        MemoryGrowDecision::decode_payload(input).map(Self)
    }
}

pub type FdWriteReqMsg = Msg<LABEL_WASI_FD_WRITE, FdWriteReq>;
pub type FdWriteRetMsg = Msg<LABEL_WASI_FD_WRITE_RET, FdWriteDoneRet>;
pub type FdWriteObjectReqMsg = Msg<LABEL_WASI_FD_WRITE_OBJECT, FdWriteReq>;
pub type FdWriteObjectRetMsg = Msg<LABEL_WASI_FD_WRITE_OBJECT_RET, FdWriteDoneRet>;
pub type FdReadReqMsg = Msg<LABEL_WASI_FD_READ, FdReadReq>;
pub type FdReadRetMsg = Msg<LABEL_WASI_FD_READ_RET, FdReadDoneRet>;
pub type FdReaddirReqMsg = Msg<LABEL_WASI_FD_READDIR, FdReaddirReq>;
pub type FdReaddirRetMsg = Msg<LABEL_WASI_FD_READDIR_RET, FdReaddirDoneRet>;
pub type FdFdstatGetReqMsg = Msg<LABEL_WASI_FD_FDSTAT_GET, FdFdstatGetReq>;
pub type FdFdstatGetRetMsg = Msg<LABEL_WASI_FD_FDSTAT_GET_RET, FdStatRet>;
pub type FdPrestatGetReqMsg = Msg<LABEL_WASI_FD_PRESTAT_GET, FdPrestatGetReq>;
pub type FdPrestatGetRetMsg = Msg<LABEL_WASI_FD_PRESTAT_GET_RET, FdPrestatRet>;
pub type FdPrestatDirNameReqMsg = Msg<LABEL_WASI_FD_PRESTAT_DIR_NAME, FdPrestatDirNameReq>;
pub type FdPrestatDirNameRetMsg = Msg<LABEL_WASI_FD_PRESTAT_DIR_NAME_RET, FdPrestatDirNameRet>;
pub type FdFilestatGetReqMsg = Msg<LABEL_WASI_FD_FILESTAT_GET, FdFilestatGetReq>;
pub type FdFilestatGetRetMsg = Msg<LABEL_WASI_FD_FILESTAT_GET_RET, FdFilestatRet>;
pub type FdCloseReqMsg = Msg<LABEL_WASI_FD_CLOSE, FdCloseReq>;
pub type FdCloseRetMsg = Msg<LABEL_WASI_FD_CLOSE_RET, FdClosedRet>;
pub type ClockResGetReqMsg = Msg<LABEL_WASI_CLOCK_RES_GET, ClockResGetReq>;
pub type ClockResGetRetMsg = Msg<LABEL_WASI_CLOCK_RES_GET_RET, ClockResolutionRet>;
pub type ClockTimeGetReqMsg = Msg<LABEL_WASI_CLOCK_TIME_GET, ClockTimeGetReq>;
pub type ClockTimeGetRetMsg = Msg<LABEL_WASI_CLOCK_TIME_GET_RET, ClockTimeRet>;
pub type PollOneoffReqMsg = Msg<LABEL_WASI_POLL_ONEOFF, PollOneoffReq>;
pub type PollOneoffRetMsg = Msg<LABEL_WASI_POLL_ONEOFF_RET, PollReadyRet>;
pub type RandomGetReqMsg = Msg<LABEL_WASI_RANDOM_GET, RandomGetReq>;
pub type RandomGetRetMsg = Msg<LABEL_WASI_RANDOM_GET_RET, RandomDoneRet>;
pub type ArgsSizesGetReqMsg = Msg<LABEL_WASI_ARGS_SIZES_GET, ArgsSizesGetReq>;
pub type ArgsSizesGetRetMsg = Msg<LABEL_WASI_ARGS_SIZES_GET_RET, ArgsSizesRet>;
pub type ArgsGetReqMsg = Msg<LABEL_WASI_ARGS_GET, ArgsGetReq>;
pub type ArgsGetRetMsg = Msg<LABEL_WASI_ARGS_GET_RET, ArgsDoneRet>;
pub type EnvironSizesGetReqMsg = Msg<LABEL_WASI_ENVIRON_SIZES_GET, EnvironSizesGetReq>;
pub type EnvironSizesGetRetMsg = Msg<LABEL_WASI_ENVIRON_SIZES_GET_RET, EnvironSizesRet>;
pub type EnvironGetReqMsg = Msg<LABEL_WASI_ENVIRON_GET, EnvironGetReq>;
pub type EnvironGetRetMsg = Msg<LABEL_WASI_ENVIRON_GET_RET, EnvironDoneRet>;
pub type PathOpenReqMsg = Msg<LABEL_WASI_PATH_OPEN, PathOpenReq>;
pub type PathOpenRetMsg = Msg<LABEL_WASI_PATH_OPEN_RET, PathOpenedRet>;
pub type PathFilestatGetReqMsg = Msg<LABEL_WASI_PATH_FILESTAT_GET, PathFilestatGetReq>;
pub type PathFilestatGetRetMsg = Msg<LABEL_WASI_PATH_FILESTAT_GET_RET, PathFilestatRet>;

const FD_BINDING_READ_BASE: u8 = 1 << 0;
const FD_BINDING_WRITE_BASE: u8 = 1 << 1;
const FD_BINDING_WRITE_OBJECT: u8 = 1 << 2;
const FD_BINDING_READDIR_BASE: u8 = 1 << 3;
const FD_BINDING_KNOWN_BITS: u8 = FD_BINDING_READ_BASE
    | FD_BINDING_WRITE_BASE
    | FD_BINDING_WRITE_OBJECT
    | FD_BINDING_READDIR_BASE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FdReadRow {
    Base,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FdWriteRow {
    Base,
    Object,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FdReaddirRow {
    Base,
}

pub const WASIP1_FILETYPE_DIRECTORY: u8 = 3;
pub const WASIP1_FILETYPE_REGULAR_FILE: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdBinding {
    pub read: Option<FdReadRow>,
    pub write: Option<FdWriteRow>,
    pub readdir: Option<FdReaddirRow>,
}

impl FdBinding {
    pub const fn none() -> Self {
        Self {
            read: None,
            write: None,
            readdir: None,
        }
    }

    pub const fn read(row: FdReadRow) -> Self {
        Self {
            read: Some(row),
            write: None,
            readdir: None,
        }
    }

    pub const fn write(row: FdWriteRow) -> Self {
        Self {
            read: None,
            write: Some(row),
            readdir: None,
        }
    }

    pub const fn readdir(row: FdReaddirRow) -> Self {
        Self {
            read: None,
            write: None,
            readdir: Some(row),
        }
    }

    pub const fn is_empty(self) -> bool {
        self.read.is_none() && self.write.is_none() && self.readdir.is_none()
    }

    pub const fn bits(self) -> u8 {
        let mut bits = 0;
        if self.read.is_some() {
            bits |= FD_BINDING_READ_BASE;
        }
        match self.write {
            Some(FdWriteRow::Base) => bits |= FD_BINDING_WRITE_BASE,
            Some(FdWriteRow::Object) => bits |= FD_BINDING_WRITE_OBJECT,
            None => {}
        }
        if self.readdir.is_some() {
            bits |= FD_BINDING_READDIR_BASE;
        }
        bits
    }

    fn from_bits(bits: u8) -> Result<Self, CodecError> {
        if bits & !FD_BINDING_KNOWN_BITS != 0 {
            return Err(CodecError::Malformed);
        }
        if bits & FD_BINDING_WRITE_BASE != 0 && bits & FD_BINDING_WRITE_OBJECT != 0 {
            return Err(CodecError::Malformed);
        }
        let read = if bits & FD_BINDING_READ_BASE != 0 {
            Some(FdReadRow::Base)
        } else {
            None
        };
        let write = if bits & FD_BINDING_WRITE_BASE != 0 {
            Some(FdWriteRow::Base)
        } else if bits & FD_BINDING_WRITE_OBJECT != 0 {
            Some(FdWriteRow::Object)
        } else {
            None
        };
        let readdir = if bits & FD_BINDING_READDIR_BASE != 0 {
            Some(FdReaddirRow::Base)
        } else {
            None
        };
        Ok(Self {
            read,
            write,
            readdir,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct PathFilestatGet {
    flags: u32,
    path: [u8; WASIP1_PATH_CHUNK_CAPACITY],
    preopen_fd: u8,
    len: u8,
}

impl PathFilestatGet {
    pub fn new(preopen_fd: u8, flags: u32, path: &[u8]) -> Result<Self, CodecError> {
        if path.len() > WASIP1_PATH_CHUNK_CAPACITY {
            return Err(CodecError::Malformed);
        }
        let mut out = [0u8; WASIP1_PATH_CHUNK_CAPACITY];
        out[..path.len()].copy_from_slice(path);
        Ok(Self {
            flags,
            path: out,
            preopen_fd,
            len: path.len() as u8,
        })
    }

    pub const fn preopen_fd(&self) -> u8 {
        self.preopen_fd
    }

    pub const fn flags(&self) -> u32 {
        self.flags
    }

    pub const fn len(&self) -> usize {
        self.len as usize
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn path(&self) -> &[u8] {
        self.path.split_at(self.len()).0
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() < 6 {
            return Err(CodecError::Truncated);
        }
        let len = bytes[5] as usize;
        if bytes.len() != 6 + len {
            return Err(CodecError::Malformed);
        }
        let mut flags = [0u8; 4];
        flags.copy_from_slice(&bytes[1..5]);
        Self::new(bytes[0], u32::from_be_bytes(flags), &bytes[6..])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockTime {
    nanos: u64,
}

impl ClockTime {
    pub const fn new(nanos: u64) -> Self {
        Self { nanos }
    }

    pub const fn nanos(&self) -> u64 {
        self.nanos
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 8 {
            return Err(CodecError::Malformed);
        }
        let mut buf = [0u8; 8];
        buf.copy_from_slice(bytes);
        Ok(Self::new(u64::from_be_bytes(buf)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WasiP1IoChunk {
    len: u8,
    bytes: [u8; WASIP1_IO_CHUNK_CAPACITY],
}

impl WasiP1IoChunk {
    pub fn new(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() > WASIP1_IO_CHUNK_CAPACITY {
            return Err(CodecError::Malformed);
        }
        let mut out = [0u8; WASIP1_IO_CHUNK_CAPACITY];
        out[..bytes.len()].copy_from_slice(bytes);
        Ok(Self {
            len: bytes.len() as u8,
            bytes: out,
        })
    }

    pub const fn len(&self) -> usize {
        self.len as usize
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len()]
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.is_empty() {
            return Err(CodecError::Truncated);
        };
        let len = bytes[0] as usize;
        let payload = &bytes[1..];
        if len > WASIP1_IO_CHUNK_CAPACITY {
            return Err(CodecError::Malformed);
        }
        if payload.len() != len {
            return Err(CodecError::Malformed);
        }
        Self::new(payload)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdWrite {
    fd: u8,
    chunk: WasiP1IoChunk,
}

impl FdWrite {
    pub fn new(fd: u8, bytes: &[u8]) -> Result<Self, CodecError> {
        Ok(Self {
            fd,
            chunk: WasiP1IoChunk::new(bytes)?,
        })
    }

    pub const fn fd(&self) -> u8 {
        self.fd
    }

    pub const fn len(&self) -> usize {
        self.chunk.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.chunk.is_empty()
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.chunk.as_bytes()
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() < 2 {
            return Err(CodecError::Truncated);
        }
        let fd = bytes[0];
        Self::new(fd, &bytes[2..]).and_then(|write| {
            if write.len() != bytes[1] as usize {
                return Err(CodecError::Malformed);
            }
            Ok(write)
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdRead {
    fd: u8,
    max_len: u8,
}

impl FdRead {
    pub fn new(fd: u8, max_len: u8) -> Result<Self, CodecError> {
        if max_len as usize > WASIP1_IO_CHUNK_CAPACITY {
            return Err(CodecError::Malformed);
        }
        Ok(Self { fd, max_len })
    }

    pub const fn fd(&self) -> u8 {
        self.fd
    }

    pub const fn max_len(&self) -> u8 {
        self.max_len
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 2 {
            return Err(CodecError::Malformed);
        }
        Self::new(bytes[0], bytes[1])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdReaddir {
    fd: u8,
    cookie: u64,
    max_len: u8,
}

impl FdReaddir {
    pub fn new(fd: u8, cookie: u64, max_len: u8) -> Result<Self, CodecError> {
        if max_len as usize > WASIP1_IO_CHUNK_CAPACITY {
            return Err(CodecError::Malformed);
        }
        Ok(Self {
            fd,
            cookie,
            max_len,
        })
    }

    pub const fn fd(&self) -> u8 {
        self.fd
    }

    pub const fn cookie(&self) -> u64 {
        self.cookie
    }

    pub const fn max_len(&self) -> u8 {
        self.max_len
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 10 {
            return Err(CodecError::Malformed);
        }
        let mut cookie = [0u8; 8];
        cookie.copy_from_slice(&bytes[1..9]);
        Self::new(bytes[0], u64::from_be_bytes(cookie), bytes[9])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdRequest {
    fd: u8,
}

impl FdRequest {
    pub const fn new(fd: u8) -> Self {
        Self { fd }
    }

    pub const fn fd(&self) -> u8 {
        self.fd
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 1 {
            return Err(CodecError::Malformed);
        }
        Ok(Self::new(bytes[0]))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdPrestatDirName {
    fd: u8,
    max_len: u8,
}

impl FdPrestatDirName {
    pub fn new(fd: u8, max_len: u8) -> Result<Self, CodecError> {
        if max_len as usize > WASIP1_PATH_CHUNK_CAPACITY {
            return Err(CodecError::Malformed);
        }
        Ok(Self { fd, max_len })
    }

    pub const fn fd(&self) -> u8 {
        self.fd
    }

    pub const fn max_len(&self) -> u8 {
        self.max_len
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 2 {
            return Err(CodecError::Malformed);
        }
        Self::new(bytes[0], bytes[1])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct PathOpen {
    rights_base: u64,
    path: [u8; WASIP1_PATH_CHUNK_CAPACITY],
    preopen_fd: u8,
    len: u8,
}

impl PathOpen {
    pub fn new(preopen_fd: u8, rights_base: u64, path: &[u8]) -> Result<Self, CodecError> {
        if path.len() > WASIP1_PATH_CHUNK_CAPACITY {
            return Err(CodecError::Malformed);
        }
        let mut out = [0u8; WASIP1_PATH_CHUNK_CAPACITY];
        out[..path.len()].copy_from_slice(path);
        Ok(Self {
            rights_base,
            path: out,
            preopen_fd,
            len: path.len() as u8,
        })
    }

    pub const fn preopen_fd(&self) -> u8 {
        self.preopen_fd
    }

    pub const fn rights_base(&self) -> u64 {
        self.rights_base
    }

    pub const fn len(&self) -> usize {
        self.len as usize
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn path(&self) -> &[u8] {
        self.path.split_at(self.len()).0
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() < 10 {
            return Err(CodecError::Truncated);
        }
        let len = bytes[9] as usize;
        if bytes.len() != 10 + len {
            return Err(CodecError::Malformed);
        }
        let mut rights = [0u8; 8];
        rights.copy_from_slice(&bytes[1..9]);
        Self::new(bytes[0], u64::from_be_bytes(rights), &bytes[10..])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathOpened {
    fd: u8,
    errno: u16,
    binding: FdBinding,
}

impl PathOpened {
    pub const fn new(fd: u8, errno: u16) -> Self {
        Self {
            fd,
            errno,
            binding: FdBinding::none(),
        }
    }

    pub const fn new_with_binding(fd: u8, errno: u16, binding: FdBinding) -> Self {
        Self { fd, errno, binding }
    }

    pub const fn fd(&self) -> u8 {
        self.fd
    }

    pub const fn errno(&self) -> u16 {
        self.errno
    }

    pub const fn binding(&self) -> FdBinding {
        self.binding
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 4 {
            return Err(CodecError::Malformed);
        }
        Ok(Self::new_with_binding(
            bytes[0],
            u16::from_be_bytes([bytes[1], bytes[2]]),
            FdBinding::from_bits(bytes[3])?,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockResGet {
    clock_id: u8,
}

impl ClockResGet {
    pub const fn new(clock_id: u8) -> Self {
        Self { clock_id }
    }

    pub const fn clock_id(&self) -> u8 {
        self.clock_id
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 1 {
            return Err(CodecError::Malformed);
        }
        Ok(Self::new(bytes[0]))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockTimeGet {
    clock_id: u8,
    precision: u64,
}

impl ClockTimeGet {
    pub const fn new(clock_id: u8, precision: u64) -> Self {
        Self {
            clock_id,
            precision,
        }
    }

    pub const fn clock_id(&self) -> u8 {
        self.clock_id
    }

    pub const fn precision(&self) -> u64 {
        self.precision
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 9 {
            return Err(CodecError::Malformed);
        }
        let mut precision = [0u8; 8];
        precision.copy_from_slice(&bytes[1..9]);
        Ok(Self::new(bytes[0], u64::from_be_bytes(precision)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClockResolution {
    nanos: u64,
}

impl ClockResolution {
    pub const fn new(nanos: u64) -> Self {
        Self { nanos }
    }

    pub const fn nanos(&self) -> u64 {
        self.nanos
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 8 {
            return Err(CodecError::Malformed);
        }
        let mut buf = [0u8; 8];
        buf.copy_from_slice(bytes);
        Ok(Self::new(u64::from_be_bytes(buf)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PollOneoff {
    timeout_tick: u64,
}

impl PollOneoff {
    pub const fn new(timeout_tick: u64) -> Self {
        Self { timeout_tick }
    }

    pub const fn timeout_tick(&self) -> u64 {
        self.timeout_tick
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 8 {
            return Err(CodecError::Malformed);
        }
        let mut timeout = [0u8; 8];
        timeout.copy_from_slice(bytes);
        Ok(Self::new(u64::from_be_bytes(timeout)))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RandomGet {
    max_len: u8,
}

impl RandomGet {
    pub fn new(max_len: u8) -> Result<Self, CodecError> {
        if max_len as usize > WASIP1_IO_CHUNK_CAPACITY {
            return Err(CodecError::Malformed);
        }
        Ok(Self { max_len })
    }

    pub const fn max_len(&self) -> u8 {
        self.max_len
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 1 {
            return Err(CodecError::Malformed);
        }
        Self::new(bytes[0])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArgsSizesGet;

impl ArgsSizesGet {
    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if !bytes.is_empty() {
            return Err(CodecError::Malformed);
        }
        Ok(Self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArgsGet {
    max_len: u8,
}

impl ArgsGet {
    pub fn new(max_len: u8) -> Result<Self, CodecError> {
        if max_len as usize > WASIP1_IO_CHUNK_CAPACITY {
            return Err(CodecError::Malformed);
        }
        Ok(Self { max_len })
    }

    pub const fn max_len(&self) -> u8 {
        self.max_len
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 1 {
            return Err(CodecError::Malformed);
        }
        Self::new(bytes[0])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvironSizesGet;

impl EnvironSizesGet {
    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if !bytes.is_empty() {
            return Err(CodecError::Malformed);
        }
        Ok(Self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvironGet {
    max_len: u8,
}

impl EnvironGet {
    pub fn new(max_len: u8) -> Result<Self, CodecError> {
        if max_len as usize > WASIP1_IO_CHUNK_CAPACITY {
            return Err(CodecError::Malformed);
        }
        Ok(Self { max_len })
    }

    pub const fn max_len(&self) -> u8 {
        self.max_len
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 1 {
            return Err(CodecError::Malformed);
        }
        Self::new(bytes[0])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdWriteDone {
    fd: u8,
    written: u8,
    errno: u16,
}

impl FdWriteDone {
    pub const fn new(fd: u8, written: u8) -> Self {
        Self {
            fd,
            written,
            errno: 0,
        }
    }

    pub const fn new_with_errno(fd: u8, written: u8, errno: u16) -> Self {
        Self { fd, written, errno }
    }

    pub const fn fd(&self) -> u8 {
        self.fd
    }

    pub const fn written(&self) -> u8 {
        self.written
    }

    pub const fn errno(&self) -> u16 {
        self.errno
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 4 {
            return Err(CodecError::Malformed);
        }
        Ok(Self::new_with_errno(
            bytes[0],
            bytes[1],
            u16::from_be_bytes([bytes[2], bytes[3]]),
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdReadDone {
    fd: u8,
    chunk: WasiP1IoChunk,
    errno: u16,
}

impl FdReadDone {
    pub fn new(fd: u8, bytes: &[u8]) -> Result<Self, CodecError> {
        Self::new_with_errno(fd, bytes, 0)
    }

    pub fn new_with_errno(fd: u8, bytes: &[u8], errno: u16) -> Result<Self, CodecError> {
        if errno != 0 && !bytes.is_empty() {
            return Err(CodecError::Malformed);
        }
        Ok(Self {
            fd,
            chunk: WasiP1IoChunk::new(bytes)?,
            errno,
        })
    }

    pub const fn fd(&self) -> u8 {
        self.fd
    }

    pub const fn len(&self) -> usize {
        self.chunk.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.chunk.is_empty()
    }

    pub const fn errno(&self) -> u16 {
        self.errno
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.chunk.as_bytes()
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() < 4 {
            return Err(CodecError::Truncated);
        }
        let fd = bytes[0];
        let errno = u16::from_be_bytes([bytes[1], bytes[2]]);
        Self::new_with_errno(fd, &bytes[4..], errno).and_then(|read| {
            if read.len() != bytes[3] as usize {
                return Err(CodecError::Malformed);
            }
            Ok(read)
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdReaddirDone {
    fd: u8,
    chunk: WasiP1IoChunk,
    errno: u16,
}

impl FdReaddirDone {
    pub fn new(fd: u8, bytes: &[u8], errno: u16) -> Result<Self, CodecError> {
        Ok(Self {
            fd,
            chunk: WasiP1IoChunk::new(bytes)?,
            errno,
        })
    }

    pub const fn fd(&self) -> u8 {
        self.fd
    }

    pub const fn len(&self) -> usize {
        self.chunk.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.chunk.is_empty()
    }

    pub const fn errno(&self) -> u16 {
        self.errno
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.chunk.as_bytes()
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() < 4 {
            return Err(CodecError::Truncated);
        }
        let fd = bytes[0];
        let errno = u16::from_be_bytes([bytes[1], bytes[2]]);
        let len = bytes[3] as usize;
        let payload = &bytes[4..];
        if payload.len() != len {
            return Err(CodecError::Malformed);
        }
        Self::new(fd, payload, errno)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdStat {
    fd: u8,
    rights: MemRights,
    errno: u16,
}

impl FdStat {
    pub const fn new(fd: u8, rights: MemRights) -> Self {
        Self::new_with_errno(fd, rights, 0)
    }

    pub const fn new_with_errno(fd: u8, rights: MemRights, errno: u16) -> Self {
        Self { fd, rights, errno }
    }

    pub const fn fd(&self) -> u8 {
        self.fd
    }

    pub const fn rights(&self) -> MemRights {
        self.rights
    }

    pub const fn errno(&self) -> u16 {
        self.errno
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 4 {
            return Err(CodecError::Malformed);
        }
        Ok(Self::new_with_errno(
            bytes[0],
            MemRights::decode(bytes[1])?,
            u16::from_be_bytes([bytes[2], bytes[3]]),
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdPrestat {
    fd: u8,
    name_len: u8,
    errno: u16,
}

impl FdPrestat {
    pub const fn new(fd: u8, name_len: u8) -> Self {
        Self {
            fd,
            name_len,
            errno: 0,
        }
    }

    pub const fn new_with_errno(fd: u8, name_len: u8, errno: u16) -> Self {
        Self {
            fd,
            name_len,
            errno,
        }
    }

    pub const fn fd(&self) -> u8 {
        self.fd
    }

    pub const fn name_len(&self) -> u8 {
        self.name_len
    }

    pub const fn errno(&self) -> u16 {
        self.errno
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 4 {
            return Err(CodecError::Malformed);
        }
        Ok(Self::new_with_errno(
            bytes[0],
            bytes[1],
            u16::from_be_bytes([bytes[2], bytes[3]]),
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdPrestatDirNameDone {
    fd: u8,
    chunk: WasiP1IoChunk,
    errno: u16,
}

impl FdPrestatDirNameDone {
    pub fn new(fd: u8, bytes: &[u8], errno: u16) -> Result<Self, CodecError> {
        Ok(Self {
            fd,
            chunk: WasiP1IoChunk::new(bytes)?,
            errno,
        })
    }

    pub const fn fd(&self) -> u8 {
        self.fd
    }

    pub const fn len(&self) -> usize {
        self.chunk.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.chunk.is_empty()
    }

    pub const fn errno(&self) -> u16 {
        self.errno
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.chunk.as_bytes()
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() < 4 {
            return Err(CodecError::Truncated);
        }
        let fd = bytes[0];
        let errno = u16::from_be_bytes([bytes[1], bytes[2]]);
        let len = bytes[3] as usize;
        let payload = &bytes[4..];
        if payload.len() != len {
            return Err(CodecError::Malformed);
        }
        Self::new(fd, payload, errno)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStat {
    filetype: u8,
    size: u64,
    errno: u16,
}

impl FileStat {
    pub const fn new(filetype: u8, size: u64) -> Self {
        Self {
            filetype,
            size,
            errno: 0,
        }
    }

    pub const fn new_with_errno(filetype: u8, size: u64, errno: u16) -> Self {
        Self {
            filetype,
            size,
            errno,
        }
    }

    pub const fn filetype(&self) -> u8 {
        self.filetype
    }

    pub const fn size(&self) -> u64 {
        self.size
    }

    pub const fn errno(&self) -> u16 {
        self.errno
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 11 {
            return Err(CodecError::Malformed);
        }
        let mut size = [0u8; 8];
        size.copy_from_slice(&bytes[3..11]);
        Ok(Self::new_with_errno(
            bytes[0],
            u64::from_be_bytes(size),
            u16::from_be_bytes([bytes[1], bytes[2]]),
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdClosed {
    fd: u8,
    errno: u16,
}

impl FdClosed {
    pub const fn new(fd: u8) -> Self {
        Self::new_with_errno(fd, 0)
    }

    pub const fn new_with_errno(fd: u8, errno: u16) -> Self {
        Self { fd, errno }
    }

    pub const fn fd(&self) -> u8 {
        self.fd
    }

    pub const fn errno(&self) -> u16 {
        self.errno
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 3 {
            return Err(CodecError::Malformed);
        }
        Ok(Self::new_with_errno(
            bytes[0],
            u16::from_be_bytes([bytes[1], bytes[2]]),
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PollReady {
    ready: u8,
}

impl PollReady {
    pub const fn new(ready: u8) -> Self {
        Self { ready }
    }

    pub const fn ready(&self) -> u8 {
        self.ready
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 1 {
            return Err(CodecError::Malformed);
        }
        Ok(Self::new(bytes[0]))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RandomDone {
    chunk: WasiP1IoChunk,
}

impl RandomDone {
    pub fn new(bytes: &[u8]) -> Result<Self, CodecError> {
        Ok(Self {
            chunk: WasiP1IoChunk::new(bytes)?,
        })
    }

    pub const fn len(&self) -> usize {
        self.chunk.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.chunk.is_empty()
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.chunk.as_bytes()
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        WasiP1IoChunk::decode(bytes).map(|chunk| Self { chunk })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArgsSizes {
    count: u8,
    buf_size: u8,
}

impl ArgsSizes {
    pub const fn new(count: u8, buf_size: u8) -> Self {
        Self { count, buf_size }
    }

    pub const fn count(&self) -> u8 {
        self.count
    }

    pub const fn buf_size(&self) -> u8 {
        self.buf_size
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 2 {
            return Err(CodecError::Malformed);
        }
        Ok(Self::new(bytes[0], bytes[1]))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArgsDone {
    chunk: WasiP1IoChunk,
}

impl ArgsDone {
    pub fn new(bytes: &[u8]) -> Result<Self, CodecError> {
        Ok(Self {
            chunk: WasiP1IoChunk::new(bytes)?,
        })
    }

    pub const fn len(&self) -> usize {
        self.chunk.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.chunk.is_empty()
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.chunk.as_bytes()
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        WasiP1IoChunk::decode(bytes).map(|chunk| Self { chunk })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvironSizes {
    count: u8,
    buf_size: u8,
}

impl EnvironSizes {
    pub const fn new(count: u8, buf_size: u8) -> Self {
        Self { count, buf_size }
    }

    pub const fn count(&self) -> u8 {
        self.count
    }

    pub const fn buf_size(&self) -> u8 {
        self.buf_size
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 2 {
            return Err(CodecError::Malformed);
        }
        Ok(Self::new(bytes[0], bytes[1]))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvironDone {
    chunk: WasiP1IoChunk,
}

impl EnvironDone {
    pub fn new(bytes: &[u8]) -> Result<Self, CodecError> {
        Ok(Self {
            chunk: WasiP1IoChunk::new(bytes)?,
        })
    }

    pub const fn len(&self) -> usize {
        self.chunk.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.chunk.is_empty()
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.chunk.as_bytes()
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        WasiP1IoChunk::decode(bytes).map(|chunk| Self { chunk })
    }
}

impl TypedWasiPayload for WasiP1IoChunk {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let len = self.len();
        if out.len() < 1 + len {
            return Err(CodecError::Truncated);
        }
        out[0] = len as u8;
        out[1..1 + len].copy_from_slice(self.as_bytes());
        Ok(1 + len)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for FdWrite {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let len = self.len();
        if out.len() < 2 + len {
            return Err(CodecError::Truncated);
        }
        out[0] = self.fd();
        out[1] = len as u8;
        out[2..2 + len].copy_from_slice(self.as_bytes());
        Ok(2 + len)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for FdRead {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 2 {
            return Err(CodecError::Truncated);
        }
        out[0] = self.fd();
        out[1] = self.max_len();
        Ok(2)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for FdReaddir {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 10 {
            return Err(CodecError::Truncated);
        }
        out[0] = self.fd();
        out[1..9].copy_from_slice(&self.cookie().to_be_bytes());
        out[9] = self.max_len();
        Ok(10)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for FdRequest {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let Some(first) = out.first_mut() else {
            return Err(CodecError::Truncated);
        };
        *first = self.fd();
        Ok(1)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for FdPrestatDirName {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 2 {
            return Err(CodecError::Truncated);
        }
        out[0] = self.fd();
        out[1] = self.max_len();
        Ok(2)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for PathOpen {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let len = self.len();
        if out.len() < 10 + len {
            return Err(CodecError::Truncated);
        }
        out[0] = self.preopen_fd();
        out[1..9].copy_from_slice(&self.rights_base().to_be_bytes());
        out[9] = len as u8;
        out[10..10 + len].copy_from_slice(self.path());
        Ok(10 + len)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for PathFilestatGet {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let len = self.len();
        if out.len() < 6 + len {
            return Err(CodecError::Truncated);
        }
        out[0] = self.preopen_fd();
        out[1..5].copy_from_slice(&self.flags().to_be_bytes());
        out[5] = len as u8;
        out[6..6 + len].copy_from_slice(self.path());
        Ok(6 + len)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for ClockResGet {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let Some(first) = out.first_mut() else {
            return Err(CodecError::Truncated);
        };
        *first = self.clock_id();
        Ok(1)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for ClockTimeGet {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 9 {
            return Err(CodecError::Truncated);
        }
        out[0] = self.clock_id();
        out[1..9].copy_from_slice(&self.precision().to_be_bytes());
        Ok(9)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for PollOneoff {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 8 {
            return Err(CodecError::Truncated);
        }
        out[..8].copy_from_slice(&self.timeout_tick().to_be_bytes());
        Ok(8)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for RandomGet {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let Some(first) = out.first_mut() else {
            return Err(CodecError::Truncated);
        };
        *first = self.max_len();
        Ok(1)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for ArgsSizesGet {
    fn encode_payload(&self, _out: &mut [u8]) -> Result<usize, CodecError> {
        Ok(0)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for ArgsGet {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let Some(first) = out.first_mut() else {
            return Err(CodecError::Truncated);
        };
        *first = self.max_len();
        Ok(1)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for EnvironSizesGet {
    fn encode_payload(&self, _out: &mut [u8]) -> Result<usize, CodecError> {
        Ok(0)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for EnvironGet {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let Some(first) = out.first_mut() else {
            return Err(CodecError::Truncated);
        };
        *first = self.max_len();
        Ok(1)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for FdWriteDone {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 4 {
            return Err(CodecError::Truncated);
        }
        out[0] = self.fd();
        out[1] = self.written();
        out[2..4].copy_from_slice(&self.errno().to_be_bytes());
        Ok(4)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for FdReadDone {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let len = self.len();
        if out.len() < 4 + len {
            return Err(CodecError::Truncated);
        }
        out[0] = self.fd();
        out[1..3].copy_from_slice(&self.errno().to_be_bytes());
        out[3] = len as u8;
        out[4..4 + len].copy_from_slice(self.as_bytes());
        Ok(4 + len)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for FdReaddirDone {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let len = self.len();
        if out.len() < 4 + len {
            return Err(CodecError::Truncated);
        }
        out[0] = self.fd();
        out[1..3].copy_from_slice(&self.errno().to_be_bytes());
        out[3] = len as u8;
        out[4..4 + len].copy_from_slice(self.as_bytes());
        Ok(4 + len)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for FdStat {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 4 {
            return Err(CodecError::Truncated);
        }
        out[0] = self.fd();
        out[1] = self.rights().tag();
        out[2..4].copy_from_slice(&self.errno().to_be_bytes());
        Ok(4)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for FdPrestat {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 4 {
            return Err(CodecError::Truncated);
        }
        out[0] = self.fd();
        out[1] = self.name_len();
        out[2..4].copy_from_slice(&self.errno().to_be_bytes());
        Ok(4)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for FdPrestatDirNameDone {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let len = self.len();
        if out.len() < 4 + len {
            return Err(CodecError::Truncated);
        }
        out[0] = self.fd();
        out[1..3].copy_from_slice(&self.errno().to_be_bytes());
        out[3] = len as u8;
        out[4..4 + len].copy_from_slice(self.as_bytes());
        Ok(4 + len)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for FileStat {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 11 {
            return Err(CodecError::Truncated);
        }
        out[0] = self.filetype();
        out[1..3].copy_from_slice(&self.errno().to_be_bytes());
        out[3..11].copy_from_slice(&self.size().to_be_bytes());
        Ok(11)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for FdClosed {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 3 {
            return Err(CodecError::Truncated);
        }
        out[0] = self.fd();
        out[1..3].copy_from_slice(&self.errno().to_be_bytes());
        Ok(3)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for ClockResolution {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 8 {
            return Err(CodecError::Truncated);
        }
        out[..8].copy_from_slice(&self.nanos().to_be_bytes());
        Ok(8)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for ClockTime {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 8 {
            return Err(CodecError::Truncated);
        }
        out[..8].copy_from_slice(&self.nanos().to_be_bytes());
        Ok(8)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for PollReady {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let Some(first) = out.first_mut() else {
            return Err(CodecError::Truncated);
        };
        *first = self.ready();
        Ok(1)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for RandomDone {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        self.chunk.encode_payload(out)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for ArgsSizes {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 2 {
            return Err(CodecError::Truncated);
        }
        out[0] = self.count();
        out[1] = self.buf_size();
        Ok(2)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for ArgsDone {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        self.chunk.encode_payload(out)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for EnvironSizes {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 2 {
            return Err(CodecError::Truncated);
        }
        out[0] = self.count();
        out[1] = self.buf_size();
        Ok(2)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for EnvironDone {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        self.chunk.encode_payload(out)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

impl TypedWasiPayload for PathOpened {
    fn encode_payload(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 4 {
            return Err(CodecError::Truncated);
        }
        out[0] = self.fd();
        out[1..3].copy_from_slice(&self.errno().to_be_bytes());
        out[3] = self.binding().bits();
        Ok(4)
    }

    fn decode_payload_bytes(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::decode(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_run_wire_is_fuel_only() {
        let run = BudgetRun::new(0x1234, 0x5678, 0x9abc_def0);
        let mut out = [0u8; 16];

        assert_eq!(run.encode_into(&mut out), Ok(8));
        assert_eq!(&out[..8], &[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]);
        assert_eq!(
            BudgetRun::decode(&out[..8]),
            Ok(BudgetRun::new(0x1234, 0x5678, 0x9abc_def0))
        );
        assert!(matches!(
            run.encode_into(&mut [0u8; 7]),
            Err(CodecError::Truncated)
        ));
        assert!(matches!(
            BudgetRun::decode(&out[..9]),
            Err(CodecError::Malformed)
        ));
    }

    #[test]
    fn memory_grow_request_and_decision_are_typed_wire_states() {
        let request = MemoryGrow::new(1, 2, 4);
        let mut request_bytes = [0u8; 12];
        let mut decision_bytes = [0u8; 1];

        assert_eq!(request.encode_into(&mut request_bytes), Ok(12));
        assert_eq!(request_bytes, [0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 4]);
        assert_eq!(
            <MemoryGrow as WirePayload>::decode_payload(Payload::new(&request_bytes)),
            Ok(request)
        );
        assert!(request.would_fit());
        assert!(!MemoryGrow::new(3, 2, 4).would_fit());

        assert_eq!(
            MemoryGrowDecision::grant().encode_into(&mut decision_bytes),
            Ok(1)
        );
        assert_eq!(decision_bytes, [1]);
        assert_eq!(
            <MemoryGrowDecision as WirePayload>::decode_payload(Payload::new(&decision_bytes)),
            Ok(MemoryGrowDecision::grant())
        );
        assert_eq!(
            <MemoryGrowDecision as WirePayload>::decode_payload(Payload::new(&[0])),
            Ok(MemoryGrowDecision::reject())
        );
        assert!(matches!(
            <MemoryGrowDecision as WirePayload>::decode_payload(Payload::new(&[2])),
            Err(CodecError::Malformed)
        ));
    }

    #[test]
    fn typed_wasi_rows_do_not_carry_private_union_tags() {
        let mut write = [0u8; 8];
        let request = FdWriteReq(FdWrite::new(4, b"G").expect("fd_write request"));
        assert_eq!(request.encode_into(&mut write), Ok(3));
        assert_eq!(&write[..3], &[4, 1, b'G']);
        assert_eq!(
            <FdWriteReq as WirePayload>::decode_payload(Payload::new(&write[..3])),
            Ok(request)
        );

        let mut poll = [0u8; 8];
        let request = PollOneoffReq(PollOneoff::new(20));
        assert_eq!(request.encode_into(&mut poll), Ok(8));
        assert_eq!(poll, 20u64.to_be_bytes());
        assert_eq!(
            <PollOneoffReq as WirePayload>::decode_payload(Payload::new(&poll)),
            Ok(request)
        );

        let mut written = [0u8; 4];
        let done = FdWriteDoneRet(FdWriteDone::new(4, 1));
        assert_eq!(done.encode_into(&mut written), Ok(4));
        assert_eq!(&written, &[4, 1, 0, 0]);
        assert_eq!(
            <FdWriteDoneRet as WirePayload>::decode_payload(Payload::new(&written)),
            Ok(done)
        );

        let mut read_done = [0u8; 4];
        let done = FdReadDoneRet(FdReadDone::new_with_errno(4, b"", 8).expect("fd_read done"));
        assert_eq!(done.encode_into(&mut read_done), Ok(4));
        assert_eq!(read_done, [4, 0, 8, 0]);
        assert_eq!(
            <FdReadDoneRet as WirePayload>::decode_payload(Payload::new(&read_done)),
            Ok(done)
        );

        let mut stat = [0u8; 4];
        let done = FdStatRet(FdStat::new_with_errno(4, MemRights::Read, 8));
        assert_eq!(done.encode_into(&mut stat), Ok(4));
        assert_eq!(stat, [4, MemRights::Read.tag(), 0, 8]);
        assert_eq!(
            <FdStatRet as WirePayload>::decode_payload(Payload::new(&stat)),
            Ok(done)
        );

        let mut closed = [0u8; 3];
        let done = FdClosedRet(FdClosed::new_with_errno(4, 8));
        assert_eq!(done.encode_into(&mut closed), Ok(3));
        assert_eq!(closed, [4, 0, 8]);
        assert_eq!(
            <FdClosedRet as WirePayload>::decode_payload(Payload::new(&closed)),
            Ok(done)
        );

        let mut ready = [0u8; 1];
        let done = PollReadyRet(PollReady::new(1));
        assert_eq!(done.encode_into(&mut ready), Ok(1));
        assert_eq!(ready, [1]);
        assert_eq!(
            <PollReadyRet as WirePayload>::decode_payload(Payload::new(&ready)),
            Ok(done)
        );

        let unit = ArgsSizesGetReq(ArgsSizesGet);
        let mut out = [0xffu8; 1];
        assert_eq!(unit.encode_into(&mut out), Ok(0));
        assert_eq!(
            <ArgsSizesGetReq as WirePayload>::decode_payload(Payload::new(&[])),
            Ok(unit)
        );
        assert!(matches!(
            <ArgsSizesGetReq as WirePayload>::decode_payload(Payload::new(&[0])),
            Err(CodecError::Malformed)
        ));
    }

    #[test]
    fn io_chunk_wire_is_inline_bytes_only() {
        let inline = WasiP1IoChunk::decode(&[2, b'o', b'k']).expect("inline chunk");
        assert_eq!(inline.as_bytes(), b"ok");
    }
}
