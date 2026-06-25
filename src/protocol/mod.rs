use hibana::{
    g::Msg,
    runtime::wire::{CodecError, Payload, WireEncode, WirePayload},
};

macro_rules! wire_payload_via_decode {
    () => {
        fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
            Self::decode_payload(input).map(|_| ())
        }

        fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a> {
            Self::decode_payload(input).expect("validated payload")
        }
    };
}

pub const LABEL_MEM_BORROW_READ: u8 = 28;
pub const LABEL_MEM_BORROW_WRITE: u8 = 29;
pub const LABEL_MEM_GRANT: u8 = 30;
pub const LABEL_MEM_RELEASE: u8 = 31;
pub const LABEL_MEM_COMMIT: u8 = 32;
pub const LABEL_ENGINE_MEMORY_GROW: u8 = 54;
pub const LABEL_ENGINE_MEMORY_GROW_RET: u8 = 55;
pub const LABEL_WASI_CLOCK_RES_GET: u8 = 63;
pub const LABEL_WASI_CLOCK_RES_GET_RET: u8 = 64;
pub const LABEL_WASI_FD_WRITE: u8 = 85;
pub const LABEL_WASI_FD_WRITE_RET: u8 = 86;
pub const LABEL_WASI_FD_WRITE_REFINED: u8 = 151;
pub const LABEL_WASI_FD_WRITE_REFINED_RET: u8 = 152;
pub const LABEL_WASI_FD_READ: u8 = 87;
pub const LABEL_WASI_FD_READ_RET: u8 = 88;
pub const LABEL_WASI_FD_FDSTAT_GET: u8 = 89;
pub const LABEL_WASI_FD_FDSTAT_GET_RET: u8 = 90;
pub const LABEL_WASI_FD_CLOSE: u8 = 91;
pub const LABEL_WASI_FD_CLOSE_RET: u8 = 92;
pub const LABEL_WASI_CLOCK_TIME_GET: u8 = 93;
pub const LABEL_WASI_CLOCK_TIME_GET_RET: u8 = 94;
pub const LABEL_WASI_POLL_ONEOFF: u8 = 95;
pub const LABEL_WASI_POLL_ONEOFF_RET: u8 = 96;
pub const LABEL_WASI_RANDOM_GET: u8 = 97;
pub const LABEL_WASI_RANDOM_GET_RET: u8 = 98;
pub const LABEL_WASI_ARGS_GET: u8 = 100;
pub const LABEL_WASI_ARGS_GET_RET: u8 = 101;
pub const LABEL_WASI_ENVIRON_GET: u8 = 102;
pub const LABEL_WASI_ENVIRON_GET_RET: u8 = 103;
pub const LABEL_WASI_ARGS_SIZES_GET: u8 = 123;
pub const LABEL_WASI_ARGS_SIZES_GET_RET: u8 = 124;
pub const LABEL_WASI_ENVIRON_SIZES_GET: u8 = 125;
pub const LABEL_WASI_ENVIRON_SIZES_GET_RET: u8 = 126;
pub const LABEL_WASI_PATH_OPEN: u8 = 127;
pub const LABEL_WASI_PATH_OPEN_RET: u8 = 128;
pub const LABEL_WASI_FD_READDIR: u8 = 149;
pub const LABEL_WASI_FD_READDIR_RET: u8 = 150;
pub const LABEL_WASI_FD_PRESTAT_GET: u8 = 153;
pub const LABEL_WASI_FD_PRESTAT_GET_RET: u8 = 154;
pub const LABEL_WASI_FD_PRESTAT_DIR_NAME: u8 = 155;
pub const LABEL_WASI_FD_PRESTAT_DIR_NAME_RET: u8 = 156;
pub const LABEL_WASI_FD_FILESTAT_GET: u8 = 157;
pub const LABEL_WASI_FD_FILESTAT_GET_RET: u8 = 158;
pub const LABEL_WASI_PATH_FILESTAT_GET: u8 = 159;
pub const LABEL_WASI_PATH_FILESTAT_GET_RET: u8 = 160;

const TAG_REQ_WASI_FD_WRITE: u8 = 11;
const TAG_REQ_WASI_FD_READ: u8 = 12;
const TAG_REQ_WASI_FD_FDSTAT_GET: u8 = 13;
const TAG_REQ_WASI_FD_CLOSE: u8 = 14;
const TAG_REQ_WASI_CLOCK_TIME_GET: u8 = 15;
const TAG_REQ_WASI_POLL_ONEOFF: u8 = 16;
const TAG_REQ_WASI_RANDOM_GET: u8 = 17;
const TAG_REQ_WASI_ARGS_GET: u8 = 19;
const TAG_REQ_WASI_ENVIRON_GET: u8 = 20;
const TAG_REQ_WASI_CLOCK_RES_GET: u8 = 21;
const TAG_REQ_WASI_ARGS_SIZES_GET: u8 = 22;
const TAG_REQ_WASI_ENVIRON_SIZES_GET: u8 = 23;
const TAG_REQ_WASI_PATH_OPEN: u8 = 24;
const TAG_REQ_WASI_FD_READDIR: u8 = 25;
const TAG_REQ_WASI_FD_PRESTAT_GET: u8 = 26;
const TAG_REQ_WASI_FD_PRESTAT_DIR_NAME: u8 = 27;
const TAG_REQ_WASI_FD_FILESTAT_GET: u8 = 28;
const TAG_REQ_WASI_PATH_FILESTAT_GET: u8 = 29;

const TAG_RET_WASI_FD_WRITE_DONE: u8 = 10;
const TAG_RET_WASI_FD_READ_DONE: u8 = 11;
const TAG_RET_WASI_FD_FDSTAT: u8 = 12;
const TAG_RET_WASI_FD_CLOSED: u8 = 13;
const TAG_RET_WASI_CLOCK_TIME: u8 = 14;
const TAG_RET_WASI_POLL_READY: u8 = 15;
const TAG_RET_WASI_RANDOM_DONE: u8 = 16;
const TAG_RET_WASI_ARGS_DONE: u8 = 17;
const TAG_RET_WASI_ENVIRON_DONE: u8 = 18;
const TAG_RET_WASI_CLOCK_RESOLUTION: u8 = 19;
const TAG_RET_WASI_ARGS_SIZES: u8 = 20;
const TAG_RET_WASI_ENVIRON_SIZES: u8 = 21;
const TAG_RET_WASI_PATH_OPENED: u8 = 22;
const TAG_RET_WASI_FD_READDIR_DONE: u8 = 23;
const TAG_RET_WASI_FD_PRESTAT: u8 = 24;
const TAG_RET_WASI_FD_PRESTAT_DIR_NAME: u8 = 25;
const TAG_RET_WASI_FD_FILESTAT: u8 = 26;
const TAG_RET_WASI_PATH_FILESTAT: u8 = 27;

pub const WASIP1_IO_CHUNK_CAPACITY: usize = 64;
pub const WASIP1_PATH_CHUNK_CAPACITY: usize = 40;

mod wasi;
pub use wasi::*;
