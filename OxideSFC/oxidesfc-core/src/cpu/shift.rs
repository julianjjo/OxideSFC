//! Shifts and rotates (ASL/LSR/ROL/ROR) on the accumulator and in memory,
//! at both 8- and 16-bit widths.

use super::{Cpu, CpuFlags};
use crate::bus::{BusResult, MemoryBus};

impl Cpu {
    /// Helper for 8-bit ROL
    pub(super) fn rol_8(value: u8, carry: bool) -> (u8, bool) {
        let old_bit7 = (value & 0x80) != 0;
        let result = (value << 1) | (if carry { 1 } else { 0 });
        (result, old_bit7)
    }

    /// Helper for 8-bit ROR
    pub(super) fn ror_8(value: u8, carry: bool) -> (u8, bool) {
        let old_bit0 = (value & 0x01) != 0;
        let result = (value >> 1) | (if carry { 0x80 } else { 0 });
        (result, old_bit0)
    }

    pub(super) fn op_asl_acc(&mut self) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        if is_16bit {
            if (self.a & 0x8000) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            self.a = self.a << 1;
            self.update_nz_flags_16(self.a);
        } else {
            if (self.a & 0x80) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            self.a = (self.a & 0xFF00) | (((self.a as u8) << 1) as u16);
            self.update_nz_flags_8(self.a as u8);
        }
        Ok(2)
    }

    /// LSR Accumulator (0x4A)
    pub(super) fn op_lsr_acc(&mut self) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        if is_16bit {
            if (self.a & 0x0001) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            self.a = self.a >> 1;
            self.update_nz_flags_16(self.a);
        } else {
            if (self.a & 0x01) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            self.a = (self.a & 0xFF00) | (((self.a as u8) >> 1) as u16);
            self.update_nz_flags_8(self.a as u8);
        }
        Ok(2)
    }

    /// ROL Accumulator (0x2A)
    pub(super) fn op_rol_acc(&mut self) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let carry = self.p.contains(CpuFlags::CARRY);
        if is_16bit {
            if (self.a & 0x8000) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            self.a = (self.a << 1) | (if carry { 1 } else { 0 });
            self.update_nz_flags_16(self.a);
        } else {
            let a8 = self.a as u8;
            if (a8 & 0x80) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            let (result, _) = Self::rol_8(a8, carry);
            self.a = (self.a & 0xFF00) | (result as u16);
            self.update_nz_flags_8(result);
        }
        Ok(2)
    }

    /// ROR Accumulator (0x6A)
    pub(super) fn op_ror_acc(&mut self) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let carry = self.p.contains(CpuFlags::CARRY);
        if is_16bit {
            if (self.a & 0x0001) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            self.a = (self.a >> 1) | (if carry { 0x8000 } else { 0 });
            self.update_nz_flags_16(self.a);
        } else {
            let a8 = self.a as u8;
            if (a8 & 0x01) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            let (result, _) = Self::ror_8(a8, carry);
            self.a = (self.a & 0xFF00) | (result as u16);
            self.update_nz_flags_8(result);
        }
        Ok(2)
    }

    /// ASL Direct Page (0x06)
    pub(super) fn op_asl_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let result = if is_16bit {
            if (value & 0x8000) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            value << 1
        } else {
            let v = value as u8;
            if (v & 0x80) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            (v << 1) as u16
        };
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 7 } else { 5 })
    }

    /// ASL Absolute (0x0E)
    pub(super) fn op_asl_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let result = if is_16bit {
            if (value & 0x8000) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            value << 1
        } else {
            let v = value as u8;
            if (v & 0x80) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            (v << 1) as u16
        };
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 7 } else { 5 })
    }

    /// LSR Direct Page (0x46)
    pub(super) fn op_lsr_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let result = if is_16bit {
            if (value & 0x0001) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            value >> 1
        } else {
            let v = value as u8;
            if (v & 0x01) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            (v >> 1) as u16
        };
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 7 } else { 5 })
    }

    /// LSR Absolute (0x4E)
    pub(super) fn op_lsr_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let result = if is_16bit {
            if (value & 0x0001) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            value >> 1
        } else {
            let v = value as u8;
            if (v & 0x01) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            (v >> 1) as u16
        };
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 7 } else { 5 })
    }

    /// ROL Direct Page (0x26)
    pub(super) fn op_rol_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let carry = self.p.contains(CpuFlags::CARRY);
        let result = if is_16bit {
            if (value & 0x8000) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            (value << 1) | (if carry { 1 } else { 0 })
        } else {
            let v = value as u8;
            if (v & 0x80) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            let (result, _) = Self::rol_8(v, carry);
            result as u16
        };
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 7 } else { 5 })
    }

    /// ROL Absolute (0x2E)
    pub(super) fn op_rol_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let carry = self.p.contains(CpuFlags::CARRY);
        let result = if is_16bit {
            if (value & 0x8000) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            (value << 1) | (if carry { 1 } else { 0 })
        } else {
            let v = value as u8;
            if (v & 0x80) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            let (result, _) = Self::rol_8(v, carry);
            result as u16
        };
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 7 } else { 5 })
    }

    /// ROR Direct Page (0x66)
    pub(super) fn op_ror_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let carry = self.p.contains(CpuFlags::CARRY);
        let result = if is_16bit {
            if (value & 0x0001) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            (value >> 1) | (if carry { 0x8000 } else { 0 })
        } else {
            let v = value as u8;
            if (v & 0x01) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            let (result, _) = Self::ror_8(v, carry);
            result as u16
        };
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 7 } else { 5 })
    }

    /// ROR Absolute (0x6E)
    pub(super) fn op_ror_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let old_carry = if self.p.contains(CpuFlags::CARRY) { 0x80 } else { 0 };
        let result = if is_16bit {
            if (value & 0x0001) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            (value >> 1) | ((old_carry as u16) << 8)
        } else {
            let v = value as u8;
            if (v & 0x01) != 0 {
                self.p.insert(CpuFlags::CARRY);
            } else {
                self.p.remove(CpuFlags::CARRY);
            }
            ((v >> 1) | old_carry) as u16
        };
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 7 } else { 5 })
    }

    // ==================== Stack Operations ====================

    pub(super) fn asl_compute(value: u16, is_16bit: bool) -> (u16, bool) {
        if is_16bit {
            (value << 1, (value & 0x8000) != 0)
        } else {
            let v = value as u8;
            (((v << 1) as u16), (v & 0x80) != 0)
        }
    }

    pub(super) fn lsr_compute(value: u16, is_16bit: bool) -> (u16, bool) {
        if is_16bit {
            (value >> 1, (value & 0x0001) != 0)
        } else {
            let v = value as u8;
            ((v >> 1) as u16, (v & 0x01) != 0)
        }
    }

    pub(super) fn rol_compute(value: u16, is_16bit: bool, carry_in: bool) -> (u16, bool) {
        if is_16bit {
            let carry_out = (value & 0x8000) != 0;
            ((value << 1) | (if carry_in { 1 } else { 0 }), carry_out)
        } else {
            let (result, carry_out) = Self::rol_8(value as u8, carry_in);
            (result as u16, carry_out)
        }
    }

    pub(super) fn ror_compute(value: u16, is_16bit: bool, carry_in: bool) -> (u16, bool) {
        if is_16bit {
            let carry_out = (value & 0x0001) != 0;
            ((value >> 1) | (if carry_in { 0x8000 } else { 0 }), carry_out)
        } else {
            let (result, carry_out) = Self::ror_8(value as u8, carry_in);
            (result as u16, carry_out)
        }
    }

    /// ASL Direct Page,X (0x16)
    pub(super) fn op_asl_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let (result, carry) = Self::asl_compute(value, is_16bit);
        if carry { self.p.insert(CpuFlags::CARRY); } else { self.p.remove(CpuFlags::CARRY); }
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 8 } else { 6 })
    }

    /// ASL Absolute,X (0x1E)
    pub(super) fn op_asl_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let (result, carry) = Self::asl_compute(value, is_16bit);
        if carry { self.p.insert(CpuFlags::CARRY); } else { self.p.remove(CpuFlags::CARRY); }
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 9 } else { 7 })
    }

    /// LSR Direct Page,X (0x56)
    pub(super) fn op_lsr_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let (result, carry) = Self::lsr_compute(value, is_16bit);
        if carry { self.p.insert(CpuFlags::CARRY); } else { self.p.remove(CpuFlags::CARRY); }
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 8 } else { 6 })
    }

    /// LSR Absolute,X (0x5E)
    pub(super) fn op_lsr_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let (result, carry) = Self::lsr_compute(value, is_16bit);
        if carry { self.p.insert(CpuFlags::CARRY); } else { self.p.remove(CpuFlags::CARRY); }
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 9 } else { 7 })
    }

    /// ROL Direct Page,X (0x36)
    pub(super) fn op_rol_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let carry_in = self.p.contains(CpuFlags::CARRY);
        let (result, carry_out) = Self::rol_compute(value, is_16bit, carry_in);
        if carry_out { self.p.insert(CpuFlags::CARRY); } else { self.p.remove(CpuFlags::CARRY); }
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 8 } else { 6 })
    }

    /// ROL Absolute,X (0x3E)
    pub(super) fn op_rol_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let carry_in = self.p.contains(CpuFlags::CARRY);
        let (result, carry_out) = Self::rol_compute(value, is_16bit, carry_in);
        if carry_out { self.p.insert(CpuFlags::CARRY); } else { self.p.remove(CpuFlags::CARRY); }
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 9 } else { 7 })
    }

    /// ROR Direct Page,X (0x76)
    pub(super) fn op_ror_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let carry_in = self.p.contains(CpuFlags::CARRY);
        let (result, carry_out) = Self::ror_compute(value, is_16bit, carry_in);
        if carry_out { self.p.insert(CpuFlags::CARRY); } else { self.p.remove(CpuFlags::CARRY); }
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 8 } else { 6 })
    }

    /// ROR Absolute,X (0x7E)
    pub(super) fn op_ror_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let carry_in = self.p.contains(CpuFlags::CARRY);
        let (result, carry_out) = Self::ror_compute(value, is_16bit, carry_in);
        if carry_out { self.p.insert(CpuFlags::CARRY); } else { self.p.remove(CpuFlags::CARRY); }
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 9 } else { 7 })
    }
}
