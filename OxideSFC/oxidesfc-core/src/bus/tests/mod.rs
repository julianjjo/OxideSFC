//! Bus tests, grouped to mirror the production modules: the memory map,
//! register ports, transfers, timing/interrupts, input, the math units and
//! save states. Shared fixtures live in `common`.

mod common;
mod joypad;
mod math;
mod memory;
mod ports;
mod state;
mod timing;
mod transfer;
