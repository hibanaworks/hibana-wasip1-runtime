#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

#[cfg(test)]
extern crate std;

pub mod choreofs;
pub mod engine;
pub mod protocol;

mod wasip1;
