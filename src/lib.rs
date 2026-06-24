#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(test)]
extern crate std;

pub mod choreofs;
pub mod exchange;
pub mod protocol;

mod engine;
mod wasip1;

pub use engine::wasm::{
    DEFAULT_GUEST_MEMORY_BYTES, Error as WasmError, Exit, GUEST_MEMORY_PAGE_SIZE, GuestMemory,
    ImportPlanDiagnostics,
};
pub use exchange::{
    FdBindingTable, HibanaImportPending, HibanaMemoryGrowPending, HibanaStep, HibanaWasiGuest,
    HibanaWasiGuestStorage, WasiImport,
};
