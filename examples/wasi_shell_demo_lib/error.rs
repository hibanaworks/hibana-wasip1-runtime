use core::fmt;

use hibana::runtime::{AttachError, wire::CodecError};
use hibana_wasip1_runtime::{WasmError, exchange::ExchangeError};

pub type DemoResult<T> = Result<T, DemoError>;

#[derive(Debug)]
pub enum DemoError {
    Message(String),
    Io(std::io::Error),
    Attach(AttachError),
    Endpoint(hibana::EndpointError),
    Codec(CodecError),
    Wasm(WasmError),
    Exchange(ExchangeError),
}

impl DemoError {
    pub fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for DemoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Message(message) => formatter.write_str(message),
            Self::Io(error) => write!(formatter, "{error}"),
            Self::Attach(error) => write!(formatter, "{error:?}"),
            Self::Endpoint(error) => write!(formatter, "{error:?}"),
            Self::Codec(error) => write!(formatter, "{error:?}"),
            Self::Wasm(error) => write!(formatter, "{error:?}"),
            Self::Exchange(error) => write!(formatter, "{error:?}"),
        }
    }
}

impl From<std::io::Error> for DemoError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<AttachError> for DemoError {
    fn from(error: AttachError) -> Self {
        Self::Attach(error)
    }
}

impl From<hibana::EndpointError> for DemoError {
    fn from(error: hibana::EndpointError) -> Self {
        Self::Endpoint(error)
    }
}

impl From<CodecError> for DemoError {
    fn from(error: CodecError) -> Self {
        Self::Codec(error)
    }
}

impl From<WasmError> for DemoError {
    fn from(error: WasmError) -> Self {
        Self::Wasm(error)
    }
}

impl From<ExchangeError> for DemoError {
    fn from(error: ExchangeError) -> Self {
        Self::Exchange(error)
    }
}
