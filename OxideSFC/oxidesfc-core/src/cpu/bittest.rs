//! Bit tests that set flags without keeping a result: BIT, and the
//! test-and-set/test-and-reset pair TSB/TRB.

use super::{Cpu, CpuFlags};
use crate::bus::{BusResult, MemoryBus};

impl Cpu {
    /// BIT Direct Page (0x24) - Test bits
    pub(super) fn op_bit_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        
        self.bit_test(operand, is_16bit);
        Ok(if is_16bit { 4 } else { 3 })
    }

    /// BIT Absolute (0x2C)
    pub(super) fn op_bit_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        
        self.bit_test(operand, is_16bit);
        Ok(if is_16bit { 4 } else { 3 })
    }

    pub(super) fn bit_test(&mut self, operand: u16, is_16bit: bool) {
        if is_16bit {
            let result = self.a & operand;
            if result == 0 {
                self.p.insert(CpuFlags::ZERO);
            } else {
                self.p.remove(CpuFlags::ZERO);
            }
            // In 16-bit mode, N and V come from operand bits 15 and 6
            if (operand & 0x8000) != 0 {
                self.p.insert(CpuFlags::NEGATIVE);
            } else {
                self.p.remove(CpuFlags::NEGATIVE);
            }
            if (operand & 0x4000) != 0 {
                self.p.insert(CpuFlags::OVERFLOW);
            } else {
                self.p.remove(CpuFlags::OVERFLOW);
            }
        } else {
            let result = (self.a as u8) & (operand as u8);
            if result == 0 {
                self.p.insert(CpuFlags::ZERO);
            } else {
                self.p.remove(CpuFlags::ZERO);
            }
            // In 8-bit mode, N and V come from operand bits 7 and 6
            if (operand & 0x80) != 0 {
                self.p.insert(CpuFlags::NEGATIVE);
            } else {
                self.p.remove(CpuFlags::NEGATIVE);
            }
            if (operand & 0x40) != 0 {
                self.p.insert(CpuFlags::OVERFLOW);
            } else {
                self.p.remove(CpuFlags::OVERFLOW);
            }
        }
    }

    // ==================== Comparison Operations ====================

    /// TSB Direct Page (0x04) - Z reflects (mem & A); mem is then OR'd with A.
    pub(super) fn op_tsb_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let test = if is_16bit { value & self.a } else { (value & 0xFF) & (self.a & 0xFF) };
        if test == 0 { self.p.insert(CpuFlags::ZERO); } else { self.p.remove(CpuFlags::ZERO); }
        let result = if is_16bit { value | self.a } else { (value & 0xFF00) | (((value as u8) | (self.a as u8)) as u16) };
        self.write_memory(bus, addr, result, is_16bit)?;
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 7 + extra } else { 5 + extra })
    }

    /// TSB Absolute (0x0C)
    pub(super) fn op_tsb_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let test = if is_16bit { value & self.a } else { (value & 0xFF) & (self.a & 0xFF) };
        if test == 0 { self.p.insert(CpuFlags::ZERO); } else { self.p.remove(CpuFlags::ZERO); }
        let result = if is_16bit { value | self.a } else { (value & 0xFF00) | (((value as u8) | (self.a as u8)) as u16) };
        self.write_memory(bus, addr, result, is_16bit)?;
        Ok(if is_16bit { 8 } else { 6 })
    }

    /// TRB Direct Page (0x14) - Z reflects (mem & A); mem is then AND'd with !A.
    pub(super) fn op_trb_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let test = if is_16bit { value & self.a } else { (value & 0xFF) & (self.a & 0xFF) };
        if test == 0 { self.p.insert(CpuFlags::ZERO); } else { self.p.remove(CpuFlags::ZERO); }
        let result = if is_16bit { value & !self.a } else { (value & 0xFF00) | (((value as u8) & !(self.a as u8)) as u16) };
        self.write_memory(bus, addr, result, is_16bit)?;
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 7 + extra } else { 5 + extra })
    }

    /// TRB Absolute (0x1C)
    pub(super) fn op_trb_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let test = if is_16bit { value & self.a } else { (value & 0xFF) & (self.a & 0xFF) };
        if test == 0 { self.p.insert(CpuFlags::ZERO); } else { self.p.remove(CpuFlags::ZERO); }
        let result = if is_16bit { value & !self.a } else { (value & 0xFF00) | (((value as u8) & !(self.a as u8)) as u16) };
        self.write_memory(bus, addr, result, is_16bit)?;
        Ok(if is_16bit { 8 } else { 6 })
    }

    /// BIT Immediate (0x89) - immediate addressing only ever affects Z;
    /// N/V require a real memory operand per the 65816 spec (see
    /// `bit_test`, used by the dp/abs/dp,X/abs,X forms instead).
    pub(super) fn op_bit_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let operand = self.addr_immediate(bus, is_16bit)?;
        let test = if is_16bit { self.a & operand } else { (self.a & 0xFF) & (operand & 0xFF) };
        if test == 0 { self.p.insert(CpuFlags::ZERO); } else { self.p.remove(CpuFlags::ZERO); }
        Ok(if is_16bit { 3 } else { 2 })
    }

    /// BIT Direct Page,X (0x34)
    pub(super) fn op_bit_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.bit_test(operand, is_16bit);
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 5 + extra } else { 4 + extra })
    }

    /// BIT Absolute,X (0x3C)
    pub(super) fn op_bit_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.bit_test(operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    // ==================== Block Move ====================
}
