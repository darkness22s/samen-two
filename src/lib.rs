#![no_std]

// These types and algorithms are shared by the bare-metal kernel and host tests.
pub mod fs;

#[cfg(test)]
extern crate std;
