use super::*;

use core::num::NonZeroU8;

pub type BudgetRunMsg = Msg<LABEL_ENGINE_RUN, BudgetRun>;
pub type BudgetExpiredMsg = Msg<LABEL_ENGINE_BUDGET_EXPIRED, BudgetExpired>;
pub type BudgetSuspendMsg = Msg<LABEL_ENGINE_SUSPEND, BudgetSuspend>;
pub type BudgetRestartMsg = Msg<LABEL_ENGINE_RESTART, BudgetRestart>;

pub type MemReadGrantControl = Msg<LABEL_MEM_GRANT_READ_CONTROL, ()>;
pub type MemWriteGrantControl = Msg<LABEL_MEM_GRANT_WRITE_CONTROL, ()>;

const WIRE_LEASE_INLINE: u8 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LeaseId(NonZeroU8);

impl LeaseId {
    pub const fn new(id: NonZeroU8) -> Self {
        Self(id)
    }

    pub fn from_raw(raw: u8) -> Result<Self, CodecError> {
        let Some(id) = NonZeroU8::new(raw) else {
            return Err(CodecError::Malformed);
        };
        Ok(Self(id))
    }

    pub const fn raw(self) -> u8 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemLeaseLen(NonZeroU8);

impl MemLeaseLen {
    pub const fn new(len: NonZeroU8) -> Self {
        Self(len)
    }

    pub fn from_raw(raw: u8) -> Result<Self, CodecError> {
        let Some(len) = NonZeroU8::new(raw) else {
            return Err(CodecError::Malformed);
        };
        Ok(Self(len))
    }

    pub const fn raw(self) -> u8 {
        self.0.get()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaseRef {
    Inline,
    Lease(LeaseId),
}

impl LeaseRef {
    pub const fn inline() -> Self {
        Self::Inline
    }

    pub const fn lease(id: LeaseId) -> Self {
        Self::Lease(id)
    }

    fn from_raw(raw: u8) -> Result<Self, CodecError> {
        if raw == WIRE_LEASE_INLINE {
            Ok(Self::Inline)
        } else {
            LeaseId::from_raw(raw).map(Self::Lease)
        }
    }

    pub const fn raw(self) -> u8 {
        match self {
            Self::Inline => WIRE_LEASE_INLINE,
            Self::Lease(id) => id.raw(),
        }
    }
}

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
pub struct MemBorrow {
    ptr: u32,
    len: MemLeaseLen,
    epoch: u32,
}

impl MemBorrow {
    pub const fn new(ptr: u32, len: MemLeaseLen, epoch: u32) -> Self {
        Self { ptr, len, epoch }
    }

    pub const fn ptr(&self) -> u32 {
        self.ptr
    }

    pub const fn byte_len(&self) -> u8 {
        self.len.raw()
    }

    pub const fn epoch(&self) -> u32 {
        self.epoch
    }
}

impl WireEncode for MemBorrow {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 9 {
            return Err(CodecError::Truncated);
        }
        out[..4].copy_from_slice(&self.ptr.to_be_bytes());
        out[4] = self.len.raw();
        out[5..9].copy_from_slice(&self.epoch.to_be_bytes());
        Ok(9)
    }
}

impl WirePayload for MemBorrow {
    type Decoded<'a> = Self;

    wire_payload_via_decode!();

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        let bytes = input.as_bytes();
        if bytes.len() != 9 {
            return Err(CodecError::Malformed);
        }
        let mut ptr = [0u8; 4];
        let mut epoch = [0u8; 4];
        ptr.copy_from_slice(&bytes[..4]);
        epoch.copy_from_slice(&bytes[5..9]);
        Ok(Self::new(
            u32::from_be_bytes(ptr),
            MemLeaseLen::from_raw(bytes[4])?,
            u32::from_be_bytes(epoch),
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemGrant {
    lease_id: LeaseId,
    ptr: u32,
    len: MemLeaseLen,
    epoch: u32,
    rights: MemRights,
}

impl MemGrant {
    pub const fn new(
        lease_id: LeaseId,
        ptr: u32,
        len: MemLeaseLen,
        epoch: u32,
        rights: MemRights,
    ) -> Self {
        Self {
            lease_id,
            ptr,
            len,
            epoch,
            rights,
        }
    }

    pub const fn lease_id(&self) -> LeaseId {
        self.lease_id
    }

    pub const fn ptr(&self) -> u32 {
        self.ptr
    }

    pub const fn byte_len(&self) -> u8 {
        self.len.raw()
    }

    pub const fn epoch(&self) -> u32 {
        self.epoch
    }

    pub const fn rights(&self) -> MemRights {
        self.rights
    }
}

impl WireEncode for MemGrant {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 11 {
            return Err(CodecError::Truncated);
        }
        out[0] = self.lease_id.raw();
        out[1..5].copy_from_slice(&self.ptr.to_be_bytes());
        out[5] = self.len.raw();
        out[6..10].copy_from_slice(&self.epoch.to_be_bytes());
        out[10] = self.rights.tag();
        Ok(11)
    }
}

impl WirePayload for MemGrant {
    type Decoded<'a> = Self;

    wire_payload_via_decode!();

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        let bytes = input.as_bytes();
        if bytes.len() != 11 {
            return Err(CodecError::Malformed);
        }
        let mut ptr = [0u8; 4];
        let mut epoch = [0u8; 4];
        ptr.copy_from_slice(&bytes[1..5]);
        epoch.copy_from_slice(&bytes[6..10]);
        Ok(Self::new(
            LeaseId::from_raw(bytes[0])?,
            u32::from_be_bytes(ptr),
            MemLeaseLen::from_raw(bytes[5])?,
            u32::from_be_bytes(epoch),
            MemRights::decode(bytes[10])?,
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemRelease {
    lease_id: LeaseId,
}

impl MemRelease {
    pub const fn new(lease_id: LeaseId) -> Self {
        Self { lease_id }
    }

    pub const fn lease_id(&self) -> LeaseId {
        self.lease_id
    }
}

impl WireEncode for MemRelease {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        let Some(first) = out.first_mut() else {
            return Err(CodecError::Truncated);
        };
        *first = self.lease_id.raw();
        Ok(1)
    }
}

impl WirePayload for MemRelease {
    type Decoded<'a> = Self;

    wire_payload_via_decode!();

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        let bytes = input.as_bytes();
        if bytes.len() != 1 {
            return Err(CodecError::Malformed);
        }
        Ok(Self::new(LeaseId::from_raw(bytes[0])?))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemCommit {
    lease_id: LeaseId,
    written: u8,
}

impl MemCommit {
    pub const fn new(lease_id: LeaseId, written: u8) -> Self {
        Self { lease_id, written }
    }

    pub const fn lease_id(&self) -> LeaseId {
        self.lease_id
    }

    pub const fn written(&self) -> u8 {
        self.written
    }
}

impl WireEncode for MemCommit {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 2 {
            return Err(CodecError::Truncated);
        }
        out[0] = self.lease_id.raw();
        out[1] = self.written;
        Ok(2)
    }
}

impl WirePayload for MemCommit {
    type Decoded<'a> = Self;

    wire_payload_via_decode!();

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        let bytes = input.as_bytes();
        if bytes.len() != 2 {
            return Err(CodecError::Malformed);
        }
        Ok(Self::new(LeaseId::from_raw(bytes[0])?, bytes[1]))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemFenceReason {
    MemoryGrow,
    Trap,
    Suspend,
    Kill,
    HotSwap,
}

impl MemFenceReason {
    pub const fn tag(self) -> u8 {
        match self {
            Self::MemoryGrow => 1,
            Self::Trap => 2,
            Self::Suspend => 3,
            Self::Kill => 4,
            Self::HotSwap => 5,
        }
    }

    fn decode(tag: u8) -> Result<Self, CodecError> {
        match tag {
            1 => Ok(Self::MemoryGrow),
            2 => Ok(Self::Trap),
            3 => Ok(Self::Suspend),
            4 => Ok(Self::Kill),
            5 => Ok(Self::HotSwap),
            _ => Err(CodecError::Malformed),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemFence {
    reason: MemFenceReason,
    new_epoch: u32,
}

impl MemFence {
    pub const fn new(reason: MemFenceReason, new_epoch: u32) -> Self {
        Self { reason, new_epoch }
    }

    pub const fn reason(&self) -> MemFenceReason {
        self.reason
    }

    pub const fn new_epoch(&self) -> u32 {
        self.new_epoch
    }
}

impl WireEncode for MemFence {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 5 {
            return Err(CodecError::Truncated);
        }
        out[0] = self.reason.tag();
        out[1..5].copy_from_slice(&self.new_epoch.to_be_bytes());
        Ok(5)
    }
}

impl WirePayload for MemFence {
    type Decoded<'a> = Self;

    wire_payload_via_decode!();

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        let bytes = input.as_bytes();
        if bytes.len() != 5 {
            return Err(CodecError::Malformed);
        }
        let mut new_epoch = [0u8; 4];
        new_epoch.copy_from_slice(&bytes[1..5]);
        Ok(Self::new(
            MemFenceReason::decode(bytes[0])?,
            u32::from_be_bytes(new_epoch),
        ))
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
pub struct BudgetSuspend {
    run_id: u16,
    generation: u16,
}

impl BudgetSuspend {
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

impl WireEncode for BudgetSuspend {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 4 {
            return Err(CodecError::Truncated);
        }
        out[0..2].copy_from_slice(&self.run_id.to_be_bytes());
        out[2..4].copy_from_slice(&self.generation.to_be_bytes());
        Ok(4)
    }
}

impl WirePayload for BudgetSuspend {
    type Decoded<'a> = Self;

    wire_payload_via_decode!();

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        Self::decode(input.as_bytes())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetRestart {
    run_id: u16,
    generation: u16,
    fuel: u32,
}

impl BudgetRestart {
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

impl WireEncode for BudgetRestart {
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

impl WirePayload for BudgetRestart {
    type Decoded<'a> = Self;

    wire_payload_via_decode!();

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        Self::decode(input.as_bytes())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineReq {
    FdWrite(FdWrite),
    FdRead(FdRead),
    FdReaddir(FdReaddir),
    FdFdstatGet(FdRequest),
    FdClose(FdRequest),
    ClockResGet(ClockResGet),
    ClockTimeGet(ClockTimeGet),
    PollOneoff(PollOneoff),
    RandomGet(RandomGet),
    ProcExit(ProcExitStatus),
    ArgsSizesGet(ArgsSizesGet),
    ArgsGet(ArgsGet),
    EnvironSizesGet(EnvironSizesGet),
    EnvironGet(EnvironGet),
    PathOpen(PathOpen),
}

impl WireEncode for EngineReq {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        match *self {
            Self::FdWrite(write) => {
                let len = write.len();
                if out.len() < 4 + len {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_REQ_WASI_FD_WRITE;
                out[1] = write.fd();
                out[2] = write.lease().raw();
                out[3] = len as u8;
                out[4..4 + len].copy_from_slice(write.as_bytes());
                Ok(4 + len)
            }
            Self::FdRead(read) => {
                if out.len() < 4 {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_REQ_WASI_FD_READ;
                out[1] = read.fd();
                out[2] = read.lease().raw();
                out[3] = read.max_len();
                Ok(4)
            }
            Self::FdReaddir(read) => {
                if out.len() < 12 {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_REQ_WASI_FD_READDIR;
                out[1] = read.fd();
                out[2] = read.lease().raw();
                out[3..11].copy_from_slice(&read.cookie().to_be_bytes());
                out[11] = read.max_len();
                Ok(12)
            }
            Self::FdFdstatGet(request) => {
                if out.len() < 2 {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_REQ_WASI_FD_FDSTAT_GET;
                out[1] = request.fd();
                Ok(2)
            }
            Self::FdClose(request) => {
                if out.len() < 2 {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_REQ_WASI_FD_CLOSE;
                out[1] = request.fd();
                Ok(2)
            }
            Self::ClockResGet(request) => {
                if out.len() < 2 {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_REQ_WASI_CLOCK_RES_GET;
                out[1] = request.clock_id();
                Ok(2)
            }
            Self::ClockTimeGet(request) => {
                if out.len() < 10 {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_REQ_WASI_CLOCK_TIME_GET;
                out[1] = request.clock_id();
                out[2..10].copy_from_slice(&request.precision().to_be_bytes());
                Ok(10)
            }
            Self::PollOneoff(request) => {
                if out.len() < 9 {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_REQ_WASI_POLL_ONEOFF;
                out[1..9].copy_from_slice(&request.timeout_tick().to_be_bytes());
                Ok(9)
            }
            Self::RandomGet(request) => {
                if out.len() < 3 {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_REQ_WASI_RANDOM_GET;
                out[1] = request.lease().raw();
                out[2] = request.max_len();
                Ok(3)
            }
            Self::ProcExit(status) => {
                if out.len() < 2 {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_REQ_WASI_PROC_EXIT;
                out[1] = status.code();
                Ok(2)
            }
            Self::ArgsSizesGet(_) => {
                if out.is_empty() {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_REQ_WASI_ARGS_SIZES_GET;
                Ok(1)
            }
            Self::ArgsGet(request) => {
                if out.len() < 3 {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_REQ_WASI_ARGS_GET;
                out[1] = request.lease().raw();
                out[2] = request.max_len();
                Ok(3)
            }
            Self::EnvironSizesGet(_) => {
                if out.is_empty() {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_REQ_WASI_ENVIRON_SIZES_GET;
                Ok(1)
            }
            Self::EnvironGet(request) => {
                if out.len() < 3 {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_REQ_WASI_ENVIRON_GET;
                out[1] = request.lease().raw();
                out[2] = request.max_len();
                Ok(3)
            }
            Self::PathOpen(open) => {
                let len = open.len();
                if out.len() < 12 + len {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_REQ_WASI_PATH_OPEN;
                out[1] = open.preopen_fd();
                out[2] = open.lease().raw();
                out[3..11].copy_from_slice(&open.rights_base().to_be_bytes());
                out[11] = len as u8;
                out[12..12 + len].copy_from_slice(open.path());
                Ok(12 + len)
            }
        }
    }
}

impl WirePayload for EngineReq {
    type Decoded<'a> = Self;

    wire_payload_via_decode!();

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        let bytes = input.as_bytes();
        let Some((&tag, rest)) = bytes.split_first() else {
            return Err(CodecError::Truncated);
        };
        match tag {
            TAG_REQ_WASI_FD_WRITE => Ok(Self::FdWrite(FdWrite::decode(rest)?)),
            TAG_REQ_WASI_FD_READ => Ok(Self::FdRead(FdRead::decode(rest)?)),
            TAG_REQ_WASI_FD_READDIR => Ok(Self::FdReaddir(FdReaddir::decode(rest)?)),
            TAG_REQ_WASI_FD_FDSTAT_GET => Ok(Self::FdFdstatGet(FdRequest::decode(rest)?)),
            TAG_REQ_WASI_FD_CLOSE => Ok(Self::FdClose(FdRequest::decode(rest)?)),
            TAG_REQ_WASI_CLOCK_RES_GET => Ok(Self::ClockResGet(ClockResGet::decode(rest)?)),
            TAG_REQ_WASI_CLOCK_TIME_GET => Ok(Self::ClockTimeGet(ClockTimeGet::decode(rest)?)),
            TAG_REQ_WASI_POLL_ONEOFF => Ok(Self::PollOneoff(PollOneoff::decode(rest)?)),
            TAG_REQ_WASI_RANDOM_GET => Ok(Self::RandomGet(RandomGet::decode(rest)?)),
            TAG_REQ_WASI_PROC_EXIT => Ok(Self::ProcExit(ProcExitStatus::decode(rest)?)),
            TAG_REQ_WASI_ARGS_SIZES_GET => Ok(Self::ArgsSizesGet(ArgsSizesGet::decode(rest)?)),
            TAG_REQ_WASI_ARGS_GET => Ok(Self::ArgsGet(ArgsGet::decode(rest)?)),
            TAG_REQ_WASI_ENVIRON_SIZES_GET => {
                Ok(Self::EnvironSizesGet(EnvironSizesGet::decode(rest)?))
            }
            TAG_REQ_WASI_ENVIRON_GET => Ok(Self::EnvironGet(EnvironGet::decode(rest)?)),
            TAG_REQ_WASI_PATH_OPEN => Ok(Self::PathOpen(PathOpen::decode(rest)?)),
            _ => Err(CodecError::Malformed),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineRet {
    FdWriteDone(FdWriteDone),
    FdReadDone(FdReadDone),
    FdReaddirDone(FdReaddirDone),
    FdStat(FdStat),
    FdClosed(FdClosed),
    ClockResolution(ClockResolution),
    ClockTime(ClockTime),
    PollReady(PollReady),
    RandomDone(RandomDone),
    ArgsSizes(ArgsSizes),
    ArgsDone(ArgsDone),
    EnvironSizes(EnvironSizes),
    EnvironDone(EnvironDone),
    PathOpened(PathOpened),
}

impl WireEncode for EngineRet {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        match *self {
            Self::FdWriteDone(done) => {
                if out.len() < 5 {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_RET_WASI_FD_WRITE_DONE;
                out[1] = done.fd();
                out[2] = done.written();
                out[3..5].copy_from_slice(&done.errno().to_be_bytes());
                Ok(5)
            }
            Self::FdReadDone(done) => {
                let len = done.len();
                if out.len() < 4 + len {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_RET_WASI_FD_READ_DONE;
                out[1] = done.fd();
                out[2] = done.lease().raw();
                out[3] = len as u8;
                out[4..4 + len].copy_from_slice(done.as_bytes());
                Ok(4 + len)
            }
            Self::FdReaddirDone(done) => {
                let len = done.len();
                if out.len() < 6 + len {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_RET_WASI_FD_READDIR_DONE;
                out[1] = done.fd();
                out[2] = done.lease().raw();
                out[3..5].copy_from_slice(&done.errno().to_be_bytes());
                out[5] = len as u8;
                out[6..6 + len].copy_from_slice(done.as_bytes());
                Ok(6 + len)
            }
            Self::FdStat(stat) => {
                if out.len() < 3 {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_RET_WASI_FD_FDSTAT;
                out[1] = stat.fd();
                out[2] = stat.rights().tag();
                Ok(3)
            }
            Self::FdClosed(closed) => {
                if out.len() < 2 {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_RET_WASI_FD_CLOSED;
                out[1] = closed.fd();
                Ok(2)
            }
            Self::ClockResolution(resolution) => {
                if out.len() < 9 {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_RET_WASI_CLOCK_RESOLUTION;
                out[1..9].copy_from_slice(&resolution.nanos().to_be_bytes());
                Ok(9)
            }
            Self::ClockTime(now) => {
                if out.len() < 9 {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_RET_WASI_CLOCK_TIME;
                out[1..9].copy_from_slice(&now.nanos().to_be_bytes());
                Ok(9)
            }
            Self::PollReady(ready) => {
                if out.len() < 2 {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_RET_WASI_POLL_READY;
                out[1] = ready.ready();
                Ok(2)
            }
            Self::RandomDone(done) => {
                let len = done.len();
                if out.len() < 3 + len {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_RET_WASI_RANDOM_DONE;
                out[1] = done.lease().raw();
                out[2] = len as u8;
                out[3..3 + len].copy_from_slice(done.as_bytes());
                Ok(3 + len)
            }
            Self::ArgsSizes(sizes) => {
                if out.len() < 3 {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_RET_WASI_ARGS_SIZES;
                out[1] = sizes.count();
                out[2] = sizes.buf_size();
                Ok(3)
            }
            Self::ArgsDone(done) => {
                let len = done.len();
                if out.len() < 3 + len {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_RET_WASI_ARGS_DONE;
                out[1] = done.lease().raw();
                out[2] = len as u8;
                out[3..3 + len].copy_from_slice(done.as_bytes());
                Ok(3 + len)
            }
            Self::EnvironSizes(sizes) => {
                if out.len() < 3 {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_RET_WASI_ENVIRON_SIZES;
                out[1] = sizes.count();
                out[2] = sizes.buf_size();
                Ok(3)
            }
            Self::EnvironDone(done) => {
                let len = done.len();
                if out.len() < 3 + len {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_RET_WASI_ENVIRON_DONE;
                out[1] = done.lease().raw();
                out[2] = len as u8;
                out[3..3 + len].copy_from_slice(done.as_bytes());
                Ok(3 + len)
            }
            Self::PathOpened(opened) => {
                if out.len() < 4 {
                    return Err(CodecError::Truncated);
                }
                out[0] = TAG_RET_WASI_PATH_OPENED;
                out[1] = opened.fd();
                out[2..4].copy_from_slice(&opened.errno().to_be_bytes());
                Ok(4)
            }
        }
    }
}

impl WirePayload for EngineRet {
    type Decoded<'a> = Self;

    wire_payload_via_decode!();

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        let bytes = input.as_bytes();
        let Some((&tag, rest)) = bytes.split_first() else {
            return Err(CodecError::Truncated);
        };
        match tag {
            TAG_RET_WASI_FD_WRITE_DONE => Ok(Self::FdWriteDone(FdWriteDone::decode(rest)?)),
            TAG_RET_WASI_FD_READ_DONE => Ok(Self::FdReadDone(FdReadDone::decode(rest)?)),
            TAG_RET_WASI_FD_READDIR_DONE => Ok(Self::FdReaddirDone(FdReaddirDone::decode(rest)?)),
            TAG_RET_WASI_FD_FDSTAT => Ok(Self::FdStat(FdStat::decode(rest)?)),
            TAG_RET_WASI_FD_CLOSED => Ok(Self::FdClosed(FdClosed::decode(rest)?)),
            TAG_RET_WASI_CLOCK_RESOLUTION => {
                Ok(Self::ClockResolution(ClockResolution::decode(rest)?))
            }
            TAG_RET_WASI_CLOCK_TIME => Ok(Self::ClockTime(ClockTime::decode(rest)?)),
            TAG_RET_WASI_POLL_READY => Ok(Self::PollReady(PollReady::decode(rest)?)),
            TAG_RET_WASI_RANDOM_DONE => Ok(Self::RandomDone(RandomDone::decode(rest)?)),
            TAG_RET_WASI_ARGS_SIZES => Ok(Self::ArgsSizes(ArgsSizes::decode(rest)?)),
            TAG_RET_WASI_ARGS_DONE => Ok(Self::ArgsDone(ArgsDone::decode(rest)?)),
            TAG_RET_WASI_ENVIRON_SIZES => Ok(Self::EnvironSizes(EnvironSizes::decode(rest)?)),
            TAG_RET_WASI_ENVIRON_DONE => Ok(Self::EnvironDone(EnvironDone::decode(rest)?)),
            TAG_RET_WASI_PATH_OPENED => Ok(Self::PathOpened(PathOpened::decode(rest)?)),
            _ => Err(CodecError::Malformed),
        }
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
    lease: LeaseRef,
    len: u8,
    bytes: [u8; WASIP1_IO_CHUNK_CAPACITY],
}

impl WasiP1IoChunk {
    pub fn new(bytes: &[u8]) -> Result<Self, CodecError> {
        Self::new_with_lease_ref(LeaseRef::Inline, bytes)
    }

    pub fn new_with_lease(lease_id: LeaseId, bytes: &[u8]) -> Result<Self, CodecError> {
        Self::new_with_lease_ref(LeaseRef::Lease(lease_id), bytes)
    }

    fn new_with_lease_ref(lease: LeaseRef, bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() > WASIP1_IO_CHUNK_CAPACITY {
            return Err(CodecError::Malformed);
        }
        let mut out = [0u8; WASIP1_IO_CHUNK_CAPACITY];
        out[..bytes.len()].copy_from_slice(bytes);
        Ok(Self {
            lease,
            len: bytes.len() as u8,
            bytes: out,
        })
    }

    pub fn with_lease(&self, lease_id: LeaseId) -> Self {
        Self {
            lease: LeaseRef::Lease(lease_id),
            len: self.len,
            bytes: self.bytes,
        }
    }

    pub const fn lease(&self) -> LeaseRef {
        self.lease
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
        if bytes.len() < 2 {
            return Err(CodecError::Truncated);
        };
        let lease = LeaseRef::from_raw(bytes[0])?;
        let len = bytes[1] as usize;
        let payload = &bytes[2..];
        if len > WASIP1_IO_CHUNK_CAPACITY {
            return Err(CodecError::Malformed);
        }
        if payload.len() != len {
            return Err(CodecError::Malformed);
        }
        Self::new_with_lease_ref(lease, payload)
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

    pub fn new_with_lease(fd: u8, lease_id: LeaseId, bytes: &[u8]) -> Result<Self, CodecError> {
        Ok(Self {
            fd,
            chunk: WasiP1IoChunk::new_with_lease(lease_id, bytes)?,
        })
    }

    fn new_with_lease_ref(fd: u8, lease: LeaseRef, bytes: &[u8]) -> Result<Self, CodecError> {
        Ok(Self {
            fd,
            chunk: WasiP1IoChunk::new_with_lease_ref(lease, bytes)?,
        })
    }

    pub const fn fd(&self) -> u8 {
        self.fd
    }

    pub const fn lease(&self) -> LeaseRef {
        self.chunk.lease()
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
        if bytes.len() < 3 {
            return Err(CodecError::Truncated);
        }
        let fd = bytes[0];
        Self::new_with_lease_ref(fd, LeaseRef::from_raw(bytes[1])?, &bytes[3..]).and_then(|write| {
            if write.len() != bytes[2] as usize {
                return Err(CodecError::Malformed);
            }
            Ok(write)
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdRead {
    fd: u8,
    lease: LeaseRef,
    max_len: u8,
}

impl FdRead {
    pub fn new(fd: u8, max_len: u8) -> Result<Self, CodecError> {
        Self::new_with_lease_ref(fd, LeaseRef::Inline, max_len)
    }

    pub fn new_with_lease(fd: u8, lease_id: LeaseId, max_len: u8) -> Result<Self, CodecError> {
        Self::new_with_lease_ref(fd, LeaseRef::Lease(lease_id), max_len)
    }

    fn new_with_lease_ref(fd: u8, lease: LeaseRef, max_len: u8) -> Result<Self, CodecError> {
        if max_len as usize > WASIP1_IO_CHUNK_CAPACITY {
            return Err(CodecError::Malformed);
        }
        Ok(Self { fd, lease, max_len })
    }

    pub const fn fd(&self) -> u8 {
        self.fd
    }

    pub const fn lease(&self) -> LeaseRef {
        self.lease
    }

    pub const fn max_len(&self) -> u8 {
        self.max_len
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 3 {
            return Err(CodecError::Malformed);
        }
        Self::new_with_lease_ref(bytes[0], LeaseRef::from_raw(bytes[1])?, bytes[2])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdReaddir {
    fd: u8,
    lease: LeaseRef,
    cookie: u64,
    max_len: u8,
}

impl FdReaddir {
    pub fn new(fd: u8, cookie: u64, max_len: u8) -> Result<Self, CodecError> {
        Self::new_with_lease_ref(fd, LeaseRef::Inline, cookie, max_len)
    }

    pub fn new_with_lease(
        fd: u8,
        lease_id: LeaseId,
        cookie: u64,
        max_len: u8,
    ) -> Result<Self, CodecError> {
        Self::new_with_lease_ref(fd, LeaseRef::Lease(lease_id), cookie, max_len)
    }

    fn new_with_lease_ref(
        fd: u8,
        lease: LeaseRef,
        cookie: u64,
        max_len: u8,
    ) -> Result<Self, CodecError> {
        if max_len as usize > WASIP1_IO_CHUNK_CAPACITY {
            return Err(CodecError::Malformed);
        }
        Ok(Self {
            fd,
            lease,
            cookie,
            max_len,
        })
    }

    pub const fn fd(&self) -> u8 {
        self.fd
    }

    pub const fn lease(&self) -> LeaseRef {
        self.lease
    }

    pub const fn cookie(&self) -> u64 {
        self.cookie
    }

    pub const fn max_len(&self) -> u8 {
        self.max_len
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 11 {
            return Err(CodecError::Malformed);
        }
        let mut cookie = [0u8; 8];
        cookie.copy_from_slice(&bytes[2..10]);
        Self::new_with_lease_ref(
            bytes[0],
            LeaseRef::from_raw(bytes[1])?,
            u64::from_be_bytes(cookie),
            bytes[10],
        )
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
pub struct FdError {
    fd: u8,
    errno: u16,
}

impl FdError {
    pub const fn new(fd: u8, errno: u16) -> Self {
        Self { fd, errno }
    }

    pub const fn fd(&self) -> u8 {
        self.fd
    }

    pub const fn errno(&self) -> u16 {
        self.errno
    }
}

impl WireEncode for FdError {
    fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
        if out.len() < 3 {
            return Err(CodecError::Truncated);
        }
        out[0] = self.fd;
        out[1..3].copy_from_slice(&self.errno.to_be_bytes());
        Ok(3)
    }
}

impl WirePayload for FdError {
    type Decoded<'a> = Self;

    wire_payload_via_decode!();

    fn decode_payload<'a>(input: Payload<'a>) -> Result<Self::Decoded<'a>, CodecError> {
        let bytes = input.as_bytes();
        if bytes.len() != 3 {
            return Err(CodecError::Malformed);
        }
        Ok(Self::new(
            bytes[0],
            u16::from_be_bytes([bytes[1], bytes[2]]),
        ))
    }
}

pub type FdErrorMsg = Msg<LABEL_WASI_FD_ERROR, FdError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct PathOpen {
    rights_base: u64,
    path: [u8; WASIP1_PATH_CHUNK_CAPACITY],
    preopen_fd: u8,
    lease: LeaseRef,
    len: u8,
}

impl PathOpen {
    pub fn new(preopen_fd: u8, rights_base: u64, path: &[u8]) -> Result<Self, CodecError> {
        Self::new_with_lease_ref(preopen_fd, LeaseRef::Inline, rights_base, path)
    }

    pub fn new_with_lease(
        preopen_fd: u8,
        lease_id: LeaseId,
        rights_base: u64,
        path: &[u8],
    ) -> Result<Self, CodecError> {
        Self::new_with_lease_ref(preopen_fd, LeaseRef::Lease(lease_id), rights_base, path)
    }

    fn new_with_lease_ref(
        preopen_fd: u8,
        lease: LeaseRef,
        rights_base: u64,
        path: &[u8],
    ) -> Result<Self, CodecError> {
        if path.len() > WASIP1_PATH_CHUNK_CAPACITY {
            return Err(CodecError::Malformed);
        }
        let mut out = [0u8; WASIP1_PATH_CHUNK_CAPACITY];
        out[..path.len()].copy_from_slice(path);
        Ok(Self {
            rights_base,
            path: out,
            preopen_fd,
            lease,
            len: path.len() as u8,
        })
    }

    pub const fn preopen_fd(&self) -> u8 {
        self.preopen_fd
    }

    pub const fn lease(&self) -> LeaseRef {
        self.lease
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
        if bytes.len() < 11 {
            return Err(CodecError::Truncated);
        }
        let len = bytes[10] as usize;
        if bytes.len() != 11 + len {
            return Err(CodecError::Malformed);
        }
        let mut rights = [0u8; 8];
        rights.copy_from_slice(&bytes[2..10]);
        Self::new_with_lease_ref(
            bytes[0],
            LeaseRef::from_raw(bytes[1])?,
            u64::from_be_bytes(rights),
            &bytes[11..],
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathOpened {
    fd: u8,
    errno: u16,
}

impl PathOpened {
    pub const fn new(fd: u8, errno: u16) -> Self {
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
        Ok(Self::new(
            bytes[0],
            u16::from_be_bytes([bytes[1], bytes[2]]),
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
    lease: LeaseRef,
    max_len: u8,
}

impl RandomGet {
    pub fn new(max_len: u8) -> Result<Self, CodecError> {
        Self::new_with_lease_ref(LeaseRef::Inline, max_len)
    }

    pub fn new_with_lease(lease_id: LeaseId, max_len: u8) -> Result<Self, CodecError> {
        Self::new_with_lease_ref(LeaseRef::Lease(lease_id), max_len)
    }

    fn new_with_lease_ref(lease: LeaseRef, max_len: u8) -> Result<Self, CodecError> {
        if max_len as usize > WASIP1_IO_CHUNK_CAPACITY {
            return Err(CodecError::Malformed);
        }
        Ok(Self { lease, max_len })
    }

    pub const fn lease(&self) -> LeaseRef {
        self.lease
    }

    pub const fn max_len(&self) -> u8 {
        self.max_len
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 2 {
            return Err(CodecError::Malformed);
        }
        Self::new_with_lease_ref(LeaseRef::from_raw(bytes[0])?, bytes[1])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcExitStatus {
    code: u8,
}

impl ProcExitStatus {
    pub const fn new(code: u8) -> Self {
        Self { code }
    }

    pub const fn code(&self) -> u8 {
        self.code
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 1 {
            return Err(CodecError::Malformed);
        }
        Ok(Self::new(bytes[0]))
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
    lease: LeaseRef,
    max_len: u8,
}

impl ArgsGet {
    pub fn new(max_len: u8) -> Result<Self, CodecError> {
        Self::new_with_lease_ref(LeaseRef::Inline, max_len)
    }

    pub fn new_with_lease(lease_id: LeaseId, max_len: u8) -> Result<Self, CodecError> {
        Self::new_with_lease_ref(LeaseRef::Lease(lease_id), max_len)
    }

    fn new_with_lease_ref(lease: LeaseRef, max_len: u8) -> Result<Self, CodecError> {
        if max_len as usize > WASIP1_IO_CHUNK_CAPACITY {
            return Err(CodecError::Malformed);
        }
        Ok(Self { lease, max_len })
    }

    pub const fn lease(&self) -> LeaseRef {
        self.lease
    }

    pub const fn max_len(&self) -> u8 {
        self.max_len
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 2 {
            return Err(CodecError::Malformed);
        }
        Self::new_with_lease_ref(LeaseRef::from_raw(bytes[0])?, bytes[1])
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
    lease: LeaseRef,
    max_len: u8,
}

impl EnvironGet {
    pub fn new(max_len: u8) -> Result<Self, CodecError> {
        Self::new_with_lease_ref(LeaseRef::Inline, max_len)
    }

    pub fn new_with_lease(lease_id: LeaseId, max_len: u8) -> Result<Self, CodecError> {
        Self::new_with_lease_ref(LeaseRef::Lease(lease_id), max_len)
    }

    fn new_with_lease_ref(lease: LeaseRef, max_len: u8) -> Result<Self, CodecError> {
        if max_len as usize > WASIP1_IO_CHUNK_CAPACITY {
            return Err(CodecError::Malformed);
        }
        Ok(Self { lease, max_len })
    }

    pub const fn lease(&self) -> LeaseRef {
        self.lease
    }

    pub const fn max_len(&self) -> u8 {
        self.max_len
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 2 {
            return Err(CodecError::Malformed);
        }
        Self::new_with_lease_ref(LeaseRef::from_raw(bytes[0])?, bytes[1])
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
}

impl FdReadDone {
    pub fn new(fd: u8, bytes: &[u8]) -> Result<Self, CodecError> {
        Ok(Self {
            fd,
            chunk: WasiP1IoChunk::new(bytes)?,
        })
    }

    pub fn new_with_lease(fd: u8, lease_id: LeaseId, bytes: &[u8]) -> Result<Self, CodecError> {
        Ok(Self {
            fd,
            chunk: WasiP1IoChunk::new_with_lease(lease_id, bytes)?,
        })
    }

    fn new_with_lease_ref(fd: u8, lease: LeaseRef, bytes: &[u8]) -> Result<Self, CodecError> {
        Ok(Self {
            fd,
            chunk: WasiP1IoChunk::new_with_lease_ref(lease, bytes)?,
        })
    }

    pub const fn fd(&self) -> u8 {
        self.fd
    }

    pub const fn lease(&self) -> LeaseRef {
        self.chunk.lease()
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
        if bytes.len() < 3 {
            return Err(CodecError::Truncated);
        }
        let fd = bytes[0];
        Self::new_with_lease_ref(fd, LeaseRef::from_raw(bytes[1])?, &bytes[3..]).and_then(|read| {
            if read.len() != bytes[2] as usize {
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

    pub fn new_with_lease(
        fd: u8,
        lease_id: LeaseId,
        bytes: &[u8],
        errno: u16,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            fd,
            chunk: WasiP1IoChunk::new_with_lease(lease_id, bytes)?,
            errno,
        })
    }

    fn new_with_lease_ref(
        fd: u8,
        lease: LeaseRef,
        bytes: &[u8],
        errno: u16,
    ) -> Result<Self, CodecError> {
        Ok(Self {
            fd,
            chunk: WasiP1IoChunk::new_with_lease_ref(lease, bytes)?,
            errno,
        })
    }

    pub const fn fd(&self) -> u8 {
        self.fd
    }

    pub const fn lease(&self) -> LeaseRef {
        self.chunk.lease()
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
        if bytes.len() < 5 {
            return Err(CodecError::Truncated);
        }
        let fd = bytes[0];
        let lease = LeaseRef::from_raw(bytes[1])?;
        let errno = u16::from_be_bytes([bytes[2], bytes[3]]);
        let len = bytes[4] as usize;
        let payload = &bytes[5..];
        if payload.len() != len {
            return Err(CodecError::Malformed);
        }
        Self::new_with_lease_ref(fd, lease, payload, errno)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdStat {
    fd: u8,
    rights: MemRights,
}

impl FdStat {
    pub const fn new(fd: u8, rights: MemRights) -> Self {
        Self { fd, rights }
    }

    pub const fn fd(&self) -> u8 {
        self.fd
    }

    pub const fn rights(&self) -> MemRights {
        self.rights
    }

    fn decode(bytes: &[u8]) -> Result<Self, CodecError> {
        if bytes.len() != 2 {
            return Err(CodecError::Malformed);
        }
        Ok(Self::new(bytes[0], MemRights::decode(bytes[1])?))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FdClosed {
    fd: u8,
}

impl FdClosed {
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

    pub fn new_with_lease(lease_id: LeaseId, bytes: &[u8]) -> Result<Self, CodecError> {
        Ok(Self {
            chunk: WasiP1IoChunk::new_with_lease(lease_id, bytes)?,
        })
    }

    pub const fn lease(&self) -> LeaseRef {
        self.chunk.lease()
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

    pub fn new_with_lease(lease_id: LeaseId, bytes: &[u8]) -> Result<Self, CodecError> {
        Ok(Self {
            chunk: WasiP1IoChunk::new_with_lease(lease_id, bytes)?,
        })
    }

    pub const fn lease(&self) -> LeaseRef {
        self.chunk.lease()
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

    pub fn new_with_lease(lease_id: LeaseId, bytes: &[u8]) -> Result<Self, CodecError> {
        Ok(Self {
            chunk: WasiP1IoChunk::new_with_lease(lease_id, bytes)?,
        })
    }

    pub const fn lease(&self) -> LeaseRef {
        self.chunk.lease()
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
    fn budget_suspend_and_restart_are_distinct_wire_states() {
        let suspend = BudgetSuspend::new(0x0102, 0x0304);
        let restart = BudgetRestart::new(0x0102, 0x0305, 0x0607_0809);
        let mut suspend_bytes = [0u8; 8];
        let mut restart_bytes = [0u8; 8];

        assert_eq!(suspend.encode_into(&mut suspend_bytes), Ok(4));
        assert_eq!(&suspend_bytes[..4], &[0x01, 0x02, 0x03, 0x04]);
        assert_eq!(BudgetSuspend::decode(&suspend_bytes[..4]), Ok(suspend));

        assert_eq!(restart.encode_into(&mut restart_bytes), Ok(8));
        assert_eq!(
            restart_bytes,
            [0x01, 0x02, 0x03, 0x05, 0x06, 0x07, 0x08, 0x09]
        );
        assert_eq!(BudgetRestart::decode(&restart_bytes), Ok(restart));
    }

    #[test]
    fn zero_lease_is_inline_only() {
        let lease = LeaseId::from_raw(7).expect("non-zero lease id");
        let mut release = [0u8; 1];
        let mut commit = [0u8; 2];

        assert_eq!(LeaseRef::from_raw(0), Ok(LeaseRef::Inline));
        assert!(matches!(LeaseId::from_raw(0), Err(CodecError::Malformed)));

        assert_eq!(MemRelease::new(lease).encode_into(&mut release), Ok(1));
        assert_eq!(release, [7]);
        assert_eq!(MemCommit::new(lease, 3).encode_into(&mut commit), Ok(2));
        assert_eq!(commit, [7, 3]);

        assert!(matches!(
            <MemRelease as WirePayload>::decode_payload(Payload::new(&[0])),
            Err(CodecError::Malformed)
        ));
        assert!(matches!(
            <MemCommit as WirePayload>::decode_payload(Payload::new(&[0, 3])),
            Err(CodecError::Malformed)
        ));
        assert!(matches!(
            <MemGrant as WirePayload>::decode_payload(Payload::new(&[
                0, 0, 0, 0, 1, 1, 0, 0, 0, 1, 1
            ])),
            Err(CodecError::Malformed)
        ));
    }

    #[test]
    fn memory_lease_borrow_and_grant_require_non_zero_length() {
        let lease = LeaseId::from_raw(7).expect("non-zero lease id");
        let len = MemLeaseLen::from_raw(5).expect("non-zero memory lease length");
        let mut borrow = [0u8; 9];
        let mut grant = [0u8; 11];

        assert!(matches!(
            MemLeaseLen::from_raw(0),
            Err(CodecError::Malformed)
        ));

        assert_eq!(MemBorrow::new(0x0102_0304, len, 9).byte_len(), 5);
        assert_eq!(
            MemBorrow::new(0x0102_0304, len, 9).encode_into(&mut borrow),
            Ok(9)
        );
        assert_eq!(borrow[4], 5);

        assert_eq!(
            MemGrant::new(lease, 0x0102_0304, len, 9, MemRights::Read).encode_into(&mut grant),
            Ok(11)
        );
        assert_eq!(grant[0], 7);
        assert_eq!(grant[5], 5);

        assert!(matches!(
            <MemBorrow as WirePayload>::decode_payload(Payload::new(&[0, 0, 0, 1, 0, 0, 0, 0, 1])),
            Err(CodecError::Malformed)
        ));
        assert!(matches!(
            <MemGrant as WirePayload>::decode_payload(Payload::new(&[
                7, 0, 0, 0, 1, 0, 0, 0, 0, 1, 1
            ])),
            Err(CodecError::Malformed)
        ));
    }

    #[test]
    fn io_chunk_lease_ref_preserves_inline_and_lease_wire_states() {
        let lease = LeaseId::from_raw(9).expect("non-zero lease id");

        let inline = WasiP1IoChunk::decode(&[0, 2, b'o', b'k']).expect("inline chunk");
        assert_eq!(inline.lease(), LeaseRef::Inline);
        assert_eq!(inline.as_bytes(), b"ok");

        let leased = WasiP1IoChunk::decode(&[9, 1, b'x']).expect("leased chunk");
        assert_eq!(leased.lease(), LeaseRef::Lease(lease));
        assert_eq!(leased.as_bytes(), b"x");
    }
}
