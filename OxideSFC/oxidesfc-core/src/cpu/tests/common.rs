//! Shared CPU test harness.

use crate::bus::{BusResult, MemoryBus};
use crate::error::EmulationError;
use crate::wram::Wram;

/// Test-only double covering the FULL bank-0 address space (unlike
/// `Wram`, which only mirrors bank 0's low 8KB Direct Page range and
/// rejects everything else in $2000-$FFFF). Needed for tests that
/// exercise hardware interrupt vector fetches ($00FFE4 etc.), which on
/// real hardware live in cartridge ROM, not WRAM -- `Wram` alone can't
/// serve those addresses. Bank $7E/$7F (real WRAM) and bank 0's
/// existing Direct Page mirror still delegate to a real `Wram`, so
/// stack/DP behavior stays identical to every other CPU test; only
/// the otherwise-unmapped $00:2000-$00:FFFF range gets a backing
/// array here, purely so vector bytes can be placed there.
pub(super) struct VectorTestBus {
    wram: Wram,
    bank0_high: Box<[u8; 0x10000]>,
}

impl VectorTestBus {
    pub(super) fn new() -> Self {
        Self {
            wram: Wram::new(),
            bank0_high: vec![0u8; 0x10000].into_boxed_slice().try_into().unwrap(),
        }
    }
}

impl MemoryBus for VectorTestBus {
    fn read_u8(&mut self, addr: u32) -> BusResult<u8> {
        if addr < 0x2000 || (0x7E0000..0x800000).contains(&addr) {
            self.wram.read_u8(addr)
        } else if addr < 0x10000 {
            Ok(self.bank0_high[addr as usize])
        } else {
            Err(EmulationError::InvalidAddress(addr))
        }
    }

    fn write_u8(&mut self, addr: u32, value: u8) -> BusResult<()> {
        if addr < 0x2000 || (0x7E0000..0x800000).contains(&addr) {
            self.wram.write_u8(addr, value)
        } else if addr < 0x10000 {
            self.bank0_high[addr as usize] = value;
            Ok(())
        } else {
            Err(EmulationError::InvalidAddress(addr))
        }
    }
}

