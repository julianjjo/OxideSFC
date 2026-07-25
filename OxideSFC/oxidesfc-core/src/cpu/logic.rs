//! Bitwise logic: AND/ORA/EOR across the addressing modes.

use super::{Cpu, CpuFlags};
use crate::bus::{BusResult, MemoryBus};

impl Cpu {
    /// AND Immediate (0x29) - Logical AND with accumulator
    pub(super) fn op_and_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let operand = self.addr_immediate(bus, is_16bit)?;
        
        if is_16bit {
            self.a = self.a & operand;
            self.update_nz_flags_16(self.a);
        } else {
            self.a = (self.a & 0xFF00) | ((self.a as u8) & (operand as u8)) as u16;
            self.update_nz_flags_8(self.a as u8);
        }
        
        Ok(if is_16bit { 3 } else { 2 })
    }

    /// AND Absolute (0x2D)
    pub(super) fn op_and_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        
        if is_16bit {
            self.a = self.a & operand;
            self.update_nz_flags_16(self.a);
        } else {
            self.a = (self.a & 0xFF00) | ((self.a as u8) & (operand as u8)) as u16;
            self.update_nz_flags_8(self.a as u8);
        }
        
        Ok(if is_16bit { 4 } else { 3 })
    }

    /// AND Direct Page (0x25)
    pub(super) fn op_and_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        
        if is_16bit {
            self.a = self.a & operand;
            self.update_nz_flags_16(self.a);
        } else {
            self.a = (self.a & 0xFF00) | ((self.a as u8) & (operand as u8)) as u16;
            self.update_nz_flags_8(self.a as u8);
        }
        
        Ok(if is_16bit { 4 } else { 3 })
    }

    /// ORA Immediate (0x09) - Logical OR with accumulator
    pub(super) fn op_ora_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let operand = self.addr_immediate(bus, is_16bit)?;
        
        if is_16bit {
            self.a = self.a | operand;
            self.update_nz_flags_16(self.a);
        } else {
            self.a = (self.a & 0xFF00) | (((self.a as u8) | (operand as u8)) as u16);
            self.update_nz_flags_8(self.a as u8);
        }
        
        Ok(if is_16bit { 3 } else { 2 })
    }

    /// ORA Absolute (0x0D)
    pub(super) fn op_ora_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        
        if is_16bit {
            self.a = self.a | operand;
            self.update_nz_flags_16(self.a);
        } else {
            self.a = (self.a & 0xFF00) | (((self.a as u8) | (operand as u8)) as u16);
            self.update_nz_flags_8(self.a as u8);
        }
        
        Ok(if is_16bit { 4 } else { 3 })
    }

    /// ORA Direct Page (0x05)
    pub(super) fn op_ora_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        
        if is_16bit {
            self.a = self.a | operand;
            self.update_nz_flags_16(self.a);
        } else {
            self.a = (self.a & 0xFF00) | (((self.a as u8) | (operand as u8)) as u16);
            self.update_nz_flags_8(self.a as u8);
        }
        
        Ok(if is_16bit { 4 } else { 3 })
    }

    /// EOR Immediate (0x49) - Logical XOR with accumulator
    pub(super) fn op_eor_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let operand = self.addr_immediate(bus, is_16bit)?;
        
        if is_16bit {
            self.a = self.a ^ operand;
            self.update_nz_flags_16(self.a);
        } else {
            self.a = (self.a & 0xFF00) | (((self.a as u8) ^ (operand as u8)) as u16);
            self.update_nz_flags_8(self.a as u8);
        }
        
        Ok(if is_16bit { 3 } else { 2 })
    }

    /// EOR Absolute (0x4D)
    pub(super) fn op_eor_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        
        if is_16bit {
            self.a = self.a ^ operand;
            self.update_nz_flags_16(self.a);
        } else {
            self.a = (self.a & 0xFF00) | (((self.a as u8) ^ (operand as u8)) as u16);
            self.update_nz_flags_8(self.a as u8);
        }
        
        Ok(if is_16bit { 4 } else { 3 })
    }

    /// EOR Direct Page (0x45)
    pub(super) fn op_eor_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        
        if is_16bit {
            self.a = self.a ^ operand;
            self.update_nz_flags_16(self.a);
        } else {
            self.a = (self.a & 0xFF00) | (((self.a as u8) ^ (operand as u8)) as u16);
            self.update_nz_flags_8(self.a as u8);
        }
        
        Ok(if is_16bit { 4 } else { 3 })
    }

    /// ORA/AND/EOR/ADC/CMP/SBC/LDA sr,S and (sr,S),Y -- stack-relative
    /// addressing, rare enough to have been deprioritized initially but
    /// confirmed needed once real SMW execution reached bank $A1 (verified
    /// against wiki.superfamicom.org/65816-reference, same as every other
    /// addressing-mode family above).
    pub(super) fn op_ora_sr(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative(bus)? as u32;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.ora_into_a(operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    pub(super) fn op_ora_sr_indirect_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative_indirect_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.ora_into_a(operand, is_16bit);
        Ok(if is_16bit { 8 } else { 7 })
    }

    pub(super) fn op_and_sr(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative(bus)? as u32;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.and_into_a(operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    pub(super) fn op_and_sr_indirect_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative_indirect_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.and_into_a(operand, is_16bit);
        Ok(if is_16bit { 8 } else { 7 })
    }

    pub(super) fn op_eor_sr(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative(bus)? as u32;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.eor_into_a(operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    pub(super) fn op_eor_sr_indirect_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative_indirect_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.eor_into_a(operand, is_16bit);
        Ok(if is_16bit { 8 } else { 7 })
    }

    /// ORA [$dp] (0x07)
    pub(super) fn op_ora_indirect_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit {
            self.a |= operand;
            self.update_nz_flags_16(self.a);
        } else {
            self.a = (self.a & 0xFF00) | (((self.a as u8) | (operand as u8)) as u16);
            self.update_nz_flags_8(self.a as u8);
        }
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// AND [$dp] (0x27)
    pub(super) fn op_and_indirect_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit {
            self.a &= operand;
            self.update_nz_flags_16(self.a);
        } else {
            self.a = (self.a & 0xFF00) | ((self.a as u8) & (operand as u8)) as u16;
            self.update_nz_flags_8(self.a as u8);
        }
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// EOR [$dp] (0x47)
    pub(super) fn op_eor_indirect_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit {
            self.a ^= operand;
            self.update_nz_flags_16(self.a);
        } else {
            self.a = (self.a & 0xFF00) | (((self.a as u8) ^ (operand as u8)) as u16);
            self.update_nz_flags_8(self.a as u8);
        }
        Ok(if is_16bit { 7 } else { 6 })
    }

    // ALU family, Absolute,X and Absolute,Y addressing -- opcode values
    // verified against wiki.superfamicom.org/65816-reference.
    pub(super) fn op_ora_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a |= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) | (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 6 } else { 5 })
    }

    pub(super) fn op_ora_abs_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a |= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) | (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 6 } else { 5 })
    }

    pub(super) fn op_and_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a &= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | ((self.a as u8) & (operand as u8)) as u16; self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 6 } else { 5 })
    }

    pub(super) fn op_and_abs_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a &= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | ((self.a as u8) & (operand as u8)) as u16; self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 6 } else { 5 })
    }

    pub(super) fn op_eor_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a ^= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) ^ (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 6 } else { 5 })
    }

    pub(super) fn op_eor_abs_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a ^= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) ^ (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 6 } else { 5 })
    }

    pub(super) fn ora_into_a(&mut self, operand: u16, is_16bit: bool) {
        if is_16bit { self.a |= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) | (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
    }

    pub(super) fn and_into_a(&mut self, operand: u16, is_16bit: bool) {
        if is_16bit { self.a &= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | ((self.a as u8) & (operand as u8)) as u16; self.update_nz_flags_8(self.a as u8); }
    }

    pub(super) fn eor_into_a(&mut self, operand: u16, is_16bit: bool) {
        if is_16bit { self.a ^= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) ^ (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
    }

    /// ORA $addr (long) (0x0F)
    pub(super) fn op_ora_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.ora_into_a(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// AND $addr (long) (0x2F)
    pub(super) fn op_and_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.and_into_a(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// EOR $addr (long) (0x4F)
    pub(super) fn op_eor_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.eor_into_a(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// ORA $addr,X (long) (0x1F)
    pub(super) fn op_ora_long_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.ora_into_a(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// AND $addr,X (long) (0x3F)
    pub(super) fn op_and_long_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.and_into_a(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// EOR $addr,X (long) (0x5F)
    pub(super) fn op_eor_long_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.eor_into_a(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// ORA [$dp],Y (0x17)
    pub(super) fn op_ora_indirect_long_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.ora_into_a(operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// AND [$dp],Y (0x37)
    pub(super) fn op_and_indirect_long_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.and_into_a(operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// EOR [$dp],Y (0x57)
    pub(super) fn op_eor_indirect_long_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.eor_into_a(operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    // ORA/AND/EOR/CMP, remaining addressing modes (dp+X, (dp,X), (dp),Y,
    // (dp)) -- column pattern cross-checked against the already-verified
    // LDA/STA/ADC/SBC instances at the same column offsets (x1=(dp,X),
    // x5=dp+X, x11=(dp),Y, x2=(dp)).
    pub(super) fn op_ora_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a |= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) | (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 5 } else { 4 })
    }

    pub(super) fn op_ora_indirect_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a |= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) | (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 7 } else { 6 })
    }

    pub(super) fn op_ora_indirect_dp_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a |= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) | (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 7 } else { 6 })
    }

    pub(super) fn op_ora_indirect_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a |= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) | (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 6 } else { 5 })
    }

    pub(super) fn op_and_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a &= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | ((self.a as u8) & (operand as u8)) as u16; self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 5 } else { 4 })
    }

    pub(super) fn op_and_indirect_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a &= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | ((self.a as u8) & (operand as u8)) as u16; self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 7 } else { 6 })
    }

    pub(super) fn op_and_indirect_dp_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a &= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | ((self.a as u8) & (operand as u8)) as u16; self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 7 } else { 6 })
    }

    pub(super) fn op_and_indirect_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a &= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | ((self.a as u8) & (operand as u8)) as u16; self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 6 } else { 5 })
    }

    pub(super) fn op_eor_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a ^= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) ^ (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 5 } else { 4 })
    }

    pub(super) fn op_eor_indirect_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a ^= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) ^ (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 7 } else { 6 })
    }

    pub(super) fn op_eor_indirect_dp_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a ^= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) ^ (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 7 } else { 6 })
    }

    pub(super) fn op_eor_indirect_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a ^= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) ^ (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 6 } else { 5 })
    }
}
