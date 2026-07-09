#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmulationError {
    UnimplementedOpcode(u8),
    InvalidAddress(u32),
    OpenBus,
    /// A save-state buffer was truncated, had a bad magic/version, or
    /// didn't match the loaded cartridge.
    InvalidSaveState(&'static str),
}