//! Processor-status manipulation: the individual flag set/clear opcodes,
//! REP/SEP, XCE, and NOP.

use super::{Cpu, CpuFlags};
use crate::bus::{BusResult, MemoryBus};

impl Cpu {
    /// NOP - No Operation (2 cycles)
    pub(super) fn op_nop(&mut self) -> BusResult<u8> {
        Ok(2)
    }

    /// CLC - Clear Carry Flag (2 cycles)
    pub(super) fn op_clc(&mut self) -> BusResult<u8> {
        self.p.remove(CpuFlags::CARRY);
        Ok(2)
    }

    /// SEC - Set Carry Flag (2 cycles)
    pub(super) fn op_sec(&mut self) -> BusResult<u8> {
        self.p.insert(CpuFlags::CARRY);
        Ok(2)
    }

    /// CLD - Clear Decimal Flag (2 cycles)
    pub(super) fn op_cld(&mut self) -> BusResult<u8> {
        self.p.remove(CpuFlags::DECIMAL);
        Ok(2)
    }

    /// SED - Set Decimal Flag (2 cycles)
    pub(super) fn op_sed(&mut self) -> BusResult<u8> {
        self.p.insert(CpuFlags::DECIMAL);
        Ok(2)
    }

    /// CLI - Clear Interrupt Disable Flag (2 cycles)
    pub(super) fn op_cli(&mut self) -> BusResult<u8> {
        self.p.remove(CpuFlags::IRQ_DISABLE);
        Ok(2)
    }

    /// SEI - Set Interrupt Disable Flag (2 cycles)
    pub(super) fn op_sei(&mut self) -> BusResult<u8> {
        self.p.insert(CpuFlags::IRQ_DISABLE);
        Ok(2)
    }

    /// CLV - Clear Overflow Flag (2 cycles)
    pub(super) fn op_clv(&mut self) -> BusResult<u8> {
        self.p.remove(CpuFlags::OVERFLOW);
        Ok(2)
    }

    // ==================== Load Instructions ====================

    /// REP (0xC2) - Reset Processor Status Bits. Note that in emulation
    /// mode this cannot actually widen M/X to 16-bit even if the operand
    /// asks for it -- see `enforce_emulation_mode_register_widths`.
    pub(super) fn op_rep(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let mask = self.fetch_u8(bus)?;
        self.p.remove(CpuFlags::from_bits_truncate(mask));
        self.enforce_emulation_mode_register_widths();
        Ok(3)
    }

    /// SEP (0xE2) - Set Processor Status Bits
    ///
    /// Setting the X flag (8-bit index mode) forces the high bytes of X
    /// and Y to zero immediately -- a real 65816 hardware quirk, unlike
    /// the accumulator's high byte (see `set_a`), which correctly persists
    /// across M-width changes. Without this, code that sets a 16-bit X/Y
    /// value, narrows to 8-bit for a while, then widens back to 16-bit
    /// later sees the stale pre-narrowing high byte reappear instead of
    /// the zero real hardware would show, silently producing a wrong
    /// 16-bit index value.
    pub(super) fn op_sep(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let mask = self.fetch_u8(bus)?;
        self.p.insert(CpuFlags::from_bits_truncate(mask));
        if mask & CpuFlags::INDEX_8BIT.bits() != 0 {
            self.x &= 0x00FF;
            self.y &= 0x00FF;
        }
        Ok(3)
    }

    /// XCE (0xFB) - Exchange Carry and Emulation Flag. Only exchanges E
    /// and Carry -- unlike a full RESET, XCE does not touch the Direct
    /// Page register. Entering emulation mode (E becomes true) also
    /// forces 8-bit A/X/Y widths on real hardware regardless of the
    /// current M/X bits in P, with X/Y's high bytes truncated to zero
    /// immediately (the same quirk `op_sep` documents for SEP $10).
    pub(super) fn op_xce(&mut self) -> BusResult<u8> {
        self.e = self.p.contains(CpuFlags::CARRY);
        if self.e {
            self.p.remove(CpuFlags::CARRY);
            self.p.insert(CpuFlags::IRQ_DISABLE);
            self.p.insert(CpuFlags::MEMORY_8BIT);
            self.p.insert(CpuFlags::INDEX_8BIT);
            self.x &= 0x00FF;
            self.y &= 0x00FF;
            self.sp = (self.sp & 0x00FF) | 0x0100; // Set high byte to 0x01
        } else {
            self.p.insert(CpuFlags::CARRY);
        }
        Ok(2)
    }
}
