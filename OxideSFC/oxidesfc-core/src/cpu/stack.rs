//! Stack instructions: the push/pull pairs for every register, plus PEA/PEI.

use super::{Cpu, CpuFlags};
use crate::bus::{BusResult, MemoryBus};

impl Cpu {
    /// PHA - Push Accumulator (3 cycles)
    pub(super) fn op_pha(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        self.push_stack(bus, self.a, is_16bit)?;
        Ok(3)
    }

    /// PLA - Pull Accumulator (4 cycles)
    pub(super) fn op_pla(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let value = self.pull_stack(bus, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(4)
    }

    /// PHX - Push X Register (3 cycles)
    pub(super) fn op_phx(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        self.push_stack(bus, self.x, is_16bit)?;
        Ok(3)
    }

    /// PLX - Pull X Register (4 cycles)
    pub(super) fn op_plx(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        self.x = self.pull_stack(bus, is_16bit)?;
        self.update_nz_flags_mem(self.x, is_16bit);
        Ok(4)
    }

    /// PHY - Push Y Register (3 cycles)
    pub(super) fn op_phy(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        self.push_stack(bus, self.y, is_16bit)?;
        Ok(3)
    }

    /// PLY - Pull Y Register (4 cycles)
    pub(super) fn op_ply(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        self.y = self.pull_stack(bus, is_16bit)?;
        self.update_nz_flags_mem(self.y, is_16bit);
        Ok(4)
    }

    /// PHP - Push Processor Status (3 cycles)
    ///
    /// Pushes P exactly as it currently is. Forcing bits 4-5 (X and M) to 1
    /// is a 6502/NMOS quirk for the synthesized "B" flag that does not
    /// apply to the 65816 in native mode -- there, bits 4 and 5 are the
    /// real, meaningful index/accumulator width flags, and PHP must
    /// preserve them exactly so a later PLP restores the correct width.
    /// Forcing them corrupted the M/X flags through any PHP/PLP pair --
    /// found via the real ROM, where the NMI handler's own
    /// `PHP ... REP #$30 ... SEP #$30 ... PLP` prologue/epilogue silently
    /// flipped the interrupted code's accumulator width on return,
    /// desyncing instruction decoding from that point on.
    pub(super) fn op_php(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.push_stack(bus, self.p.bits() as u16, false)?;
        Ok(3)
    }

    /// PLP - Pull Processor Status (4 cycles)
    pub(super) fn op_plp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let value = self.pull_stack(bus, false)?;
        self.p = CpuFlags::from_bits_truncate(value as u8);
        self.enforce_emulation_mode_register_widths();
        Ok(4)
    }

    /// PHB - Push Data Bank Register (3 cycles)
    pub(super) fn op_phb(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.push_stack(bus, self.db as u16, false)?;
        Ok(3)
    }

    /// PLB - Pull Data Bank Register (4 cycles)
    pub(super) fn op_plb(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let value = self.pull_stack(bus, false)?;
        self.db = value as u8;
        self.update_nz_flags_8(self.db);
        Ok(4)
    }

    /// PHD - Push Direct Page Register (4 cycles). D is always pushed as a
    /// full 16-bit value regardless of the M flag.
    pub(super) fn op_phd(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.push_stack(bus, self.d, true)?;
        Ok(4)
    }

    /// PLD - Pull Direct Page Register (5 cycles)
    pub(super) fn op_pld(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.d = self.pull_stack(bus, true)?;
        self.update_nz_flags_16(self.d);
        Ok(5)
    }

    /// PHK - Push Program Bank Register (3 cycles)
    pub(super) fn op_phk(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.push_stack(bus, self.pb as u16, false)?;
        Ok(3)
    }

    /// PEA $addr (0xF4) - Push Effective Absolute: pushes a 16-bit
    /// immediate operand, always as 2 bytes regardless of the M flag.
    pub(super) fn op_pea(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let value = self.fetch_u16(bus)?;
        self.push_stack(bus, value, true)?;
        Ok(5)
    }

    /// PEI (dp) (0xD4) - Push Effective Indirect: pushes the 16-bit
    /// pointer stored at the direct-page address (bank 0, not DB-relative).
    pub(super) fn op_pei(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let dp_addr = self.addr_direct_page(bus)?;
        let lo = bus.read_u8(dp_addr)? as u16;
        let hi = bus.read_u8(dp_addr.wrapping_add(1))? as u16;
        let value = (hi << 8) | lo;
        self.push_stack(bus, value, true)?;
        Ok(6)
    }
}
