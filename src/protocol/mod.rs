use hibana::{
    g::Msg,
    runtime::wire::{CodecError, Payload, WireEncode, WirePayload},
};

mod schema {
    const WASIP1: u32 = 0x5750_0000;

    macro_rules! schema_ids {
        ($($name:ident = $id:literal),+ $(,)?) => {
            $(pub(super) const $name: u32 = WASIP1 | $id;)+

            #[cfg(test)]
            pub(super) const ALL: &[u32] = &[$($name),+];
        };
    }

    schema_ids! {
        MEMORY_GROW = 1,
        MEMORY_GROW_DECISION = 2,
        FD_WRITE = 3,
        FD_READ = 4,
        FD_READDIR = 5,
        FD_REQUEST = 6,
        FD_PRESTAT_DIR_NAME = 7,
        PATH_OPEN = 8,
        PATH_FILESTAT_GET = 9,
        CLOCK_RES_GET = 10,
        CLOCK_TIME_GET = 11,
        POLL_ONEOFF = 12,
        RANDOM_GET = 13,
        ARGS_SIZES_GET = 14,
        ARGS_GET = 15,
        ENVIRON_SIZES_GET = 16,
        ENVIRON_GET = 17,
        FD_WRITE_DONE = 18,
        FD_READ_DONE = 19,
        FD_READDIR_DONE = 20,
        FD_STAT = 21,
        FD_PRESTAT = 22,
        FD_PRESTAT_DIR_NAME_DONE = 23,
        FILE_STAT = 24,
        FD_CLOSED = 25,
        CLOCK_RESOLUTION = 26,
        CLOCK_TIME = 27,
        POLL_READY = 28,
        RANDOM_DONE = 29,
        ARGS_SIZES = 30,
        ARGS_DONE = 31,
        ENVIRON_SIZES = 32,
        ENVIRON_DONE = 33,
        PATH_OPENED = 34,
    }
}

macro_rules! wire_payload {
    ($wrapper:ident, $payload:ty) => {
        impl WireEncode for $wrapper {
            fn encode_into(&self, out: &mut [u8]) -> Result<usize, CodecError> {
                self.0.encode_payload(out)
            }
        }

        impl WirePayload for $wrapper {
            const SCHEMA_ID: u32 = <$payload as TypedWasiPayload>::SCHEMA_ID;

            type Decoded<'a> = Self;

            fn validate_payload(input: Payload<'_>) -> Result<(), CodecError> {
                <$payload as TypedWasiPayload>::decode_payload_bytes(input.as_bytes()).map(drop)
            }

            fn decode_validated_payload<'a>(input: Payload<'a>) -> Self::Decoded<'a> {
                Self(
                    <$payload as TypedWasiPayload>::decode_payload_bytes(input.as_bytes())
                        .expect("payload was validated"),
                )
            }
        }
    };
}

pub const LABEL_ENGINE_MEMORY_GROW: u8 = 54;
pub const LABEL_ENGINE_MEMORY_GROW_RET: u8 = 55;
pub const LABEL_WASI_CLOCK_RES_GET: u8 = 63;
pub const LABEL_WASI_CLOCK_RES_GET_RET: u8 = 64;
pub const LABEL_WASI_FD_WRITE: u8 = 85;
pub const LABEL_WASI_FD_WRITE_RET: u8 = 86;
pub const LABEL_WASI_FD_WRITE_OBJECT: u8 = 151;
pub const LABEL_WASI_FD_WRITE_OBJECT_RET: u8 = 152;
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

pub const WASIP1_IO_CHUNK_CAPACITY: usize = 96;
pub const WASIP1_PATH_CHUNK_CAPACITY: usize = 40;

mod wasi;
pub use wasi::*;
