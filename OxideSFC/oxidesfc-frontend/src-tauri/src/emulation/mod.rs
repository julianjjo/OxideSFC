//! Emulation: the composed machine, the controller that runs it, and the
//! video frame type handed to the frontend.

mod controller;
mod snes;
mod video;

#[cfg(test)]
mod real_rom_tests;
#[cfg(test)]
mod region_tests;

pub use controller::{EmulationController, GameInfo, InputState};
pub use video::VideoFrame;
