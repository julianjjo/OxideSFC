//! The comparison instructions CMP/CPX/CPY, which set flags from a
//! subtraction without storing the result.

use super::{Cpu, CpuFlags};
use crate::bus::{BusResult, MemoryBus};

impl Cpu {
    /// CMP Immediate (0xC9)
    pub(super) fn op_cmp_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let operand = self.addr_immediate(bus, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 3 } else { 2 })
    }

    /// CMP Absolute (0xCD)
    pub(super) fn op_cmp_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 4 } else { 3 })
    }

    /// CMP Direct Page (0xC5)
    pub(super) fn op_cmp_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 4 } else { 3 })
    }

    /// CPX Immediate (0xE0)
    pub(super) fn op_cpx_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let operand = self.addr_immediate(bus, is_16bit)?;
        self.compare(self.x, operand, is_16bit);
        Ok(if is_16bit { 3 } else { 2 })
    }

    /// CPX Absolute (0xEC)
    pub(super) fn op_cpx_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_absolute(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.x, operand, is_16bit);
        Ok(if is_16bit { 4 } else { 3 })
    }

    /// CPX Direct Page (0xE4)
    pub(super) fn op_cpx_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.x, operand, is_16bit);
        Ok(if is_16bit { 4 } else { 3 })
    }

    /// CPY Immediate (0xC0)
    pub(super) fn op_cpy_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let operand = self.addr_immediate(bus, is_16bit)?;
        self.compare(self.y, operand, is_16bit);
        Ok(if is_16bit { 3 } else { 2 })
    }

    /// CPY Absolute (0xCC)
    pub(super) fn op_cpy_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_absolute(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.y, operand, is_16bit);
        Ok(if is_16bit { 4 } else { 3 })
    }

    /// CPY Direct Page (0xC4)
    pub(super) fn op_cpy_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.y, operand, is_16bit);
        Ok(if is_16bit { 4 } else { 3 })
    }

    pub(super) fn compare(&mut self, reg: u16, operand: u16, is_16bit: bool) {
        let result = reg.wrapping_sub(operand);
        if is_16bit {
            if result == 0 {
                self.p.insert(CpuFlags::ZERO);
            } else {
                self.p.remove(CpuFlags::ZERO);
            }
            if (result & 0x8000) != 0 {
                self.p.insert(CpuFlags::NEGATIVE);
            } else {
                self.p.remove(CpuFlags::NEGATIVE);
            }
            // Carry is set if reg >= operand
            if reg >= operand {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
        } else {
            let reg_8 = (reg & 0xFF) as u8;
            let op_8 = (operand & 0xFF) as u8;
            let result_8 = reg_8.wrapping_sub(op_8);
            if result_8 == 0 {
                self.p.insert(CpuFlags::ZERO);
            } else {
                self.p.remove(CpuFlags::ZERO);
            }
            if (result_8 & 0x80) != 0 {
                self.p.insert(CpuFlags::NEGATIVE);
            } else {
                self.p.remove(CpuFlags::NEGATIVE);
            }
            if reg_8 >= op_8 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
        }
    }

    // ==================== Increment/Decrement ====================

    pub(super) fn op_cmp_sr(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative(bus)? as u32;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    pub(super) fn op_cmp_sr_indirect_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative_indirect_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 8 } else { 7 })
    }

    /// CMP [$dp] (0xC7)
    pub(super) fn op_cmp_indirect_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    pub(super) fn op_cmp_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    pub(super) fn op_cmp_abs_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    // ==================== ALU family: Absolute Long and Absolute Long,X ====================
    // 24-bit-address forms -- opcode values verified against
    // wiki.superfamicom.org/65816-reference. Common in real code for
    // accessing data living in a bank other than the current Data Bank.

    /// CMP $addr (long) (0xCF)
    pub(super) fn op_cmp_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// CMP $addr,X (long) (0xDF)
    pub(super) fn op_cmp_long_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// CMP [$dp],Y (0xD7)
    pub(super) fn op_cmp_indirect_long_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    pub(super) fn op_cmp_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    pub(super) fn op_cmp_indirect_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    pub(super) fn op_cmp_indirect_dp_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    pub(super) fn op_cmp_indirect_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }
}
