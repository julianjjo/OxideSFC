//! Increments and decrements, on registers and in memory.

use super::{Cpu, CpuFlags};
use crate::bus::{BusResult, MemoryBus};

impl Cpu {
    /// INX - Increment X Register (2 cycles)
    ///
    /// Unlike A (which keeps a "hidden" high byte across 8-bit operations,
    /// restorable via XBA), X and Y architecturally zero their high byte
    /// on any 8-bit-mode write -- this previously preserved it instead
    /// (`self.x & 0xFF00 | ...`), inconsistent with `LDX`'s already-correct
    /// zero-extending behavior. A real, separate bug from the LDA one;
    /// found by tracing a stack-corruption crash back to a DEX/BPL loop
    /// whose exit condition depended on X's actual 8-bit value.
    pub(super) fn op_inx(&mut self) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        if is_16bit {
            self.x = self.x.wrapping_add(1);
            self.update_nz_flags_16(self.x);
        } else {
            self.x = (self.x as u8).wrapping_add(1) as u16;
            self.update_nz_flags_8(self.x as u8);
        }
        Ok(2)
    }

    /// DEX - Decrement X Register (2 cycles)
    pub(super) fn op_dex(&mut self) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        if is_16bit {
            self.x = self.x.wrapping_sub(1);
            self.update_nz_flags_16(self.x);
        } else {
            self.x = (self.x as u8).wrapping_sub(1) as u16;
            self.update_nz_flags_8(self.x as u8);
        }
        Ok(2)
    }

    /// INY - Increment Y Register (2 cycles)
    pub(super) fn op_iny(&mut self) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        if is_16bit {
            self.y = self.y.wrapping_add(1);
            self.update_nz_flags_16(self.y);
        } else {
            self.y = (self.y as u8).wrapping_add(1) as u16;
            self.update_nz_flags_8(self.y as u8);
        }
        Ok(2)
    }

    /// DEY - Decrement Y Register (2 cycles)
    pub(super) fn op_dey(&mut self) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        if is_16bit {
            self.y = self.y.wrapping_sub(1);
            self.update_nz_flags_16(self.y);
        } else {
            self.y = (self.y as u8).wrapping_sub(1) as u16;
            self.update_nz_flags_8(self.y as u8);
        }
        Ok(2)
    }

    /// INC Direct Page (0xE6)
    pub(super) fn op_inc_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let result = value.wrapping_add(1);
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 7 } else { 5 })
    }

    /// INC Absolute (0xEE)
    pub(super) fn op_inc_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let result = value.wrapping_add(1);
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 7 } else { 5 })
    }

    /// DEC Direct Page (0xC6)
    pub(super) fn op_dec_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let result = value.wrapping_sub(1);
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 7 } else { 5 })
    }

    /// DEC Absolute (0xCE)
    pub(super) fn op_dec_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let result = value.wrapping_sub(1);
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 7 } else { 5 })
    }

    // ==================== Shift/Rotate ====================

    /// ASL Accumulator (0x0A)
    /// INC A (0x1A) - Increment Accumulator
    pub(super) fn op_inc_acc(&mut self) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        if is_16bit {
            self.a = self.a.wrapping_add(1);
            self.update_nz_flags_16(self.a);
        } else {
            let result = (self.a as u8).wrapping_add(1);
            self.a = (self.a & 0xFF00) | (result as u16);
            self.update_nz_flags_8(result);
        }
        Ok(2)
    }

    /// DEC A (0x3A) - Decrement Accumulator
    pub(super) fn op_dec_acc(&mut self) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        if is_16bit {
            self.a = self.a.wrapping_sub(1);
            self.update_nz_flags_16(self.a);
        } else {
            let result = (self.a as u8).wrapping_sub(1);
            self.a = (self.a & 0xFF00) | (result as u16);
            self.update_nz_flags_8(result);
        }
        Ok(2)
    }

    /// DEC Direct Page,X (0xD6)
    pub(super) fn op_dec_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let result = value.wrapping_sub(1);
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 8 } else { 6 })
    }

    /// DEC Absolute,X (0xDE)
    pub(super) fn op_dec_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let result = value.wrapping_sub(1);
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 9 } else { 7 })
    }

    /// INC Direct Page,X (0xF6)
    pub(super) fn op_inc_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let result = value.wrapping_add(1);
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 8 } else { 6 })
    }

    /// INC Absolute,X (0xFE)
    pub(super) fn op_inc_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let result = value.wrapping_add(1);
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 9 } else { 7 })
    }

    // ==================== TSB/TRB and remaining BIT forms ====================
}
