//! Addition and subtraction: ADC/SBC across the addressing modes, over both
//! the binary and the decimal-mode (BCD) cores.

use super::{Cpu, CpuFlags};
use crate::bus::{BusResult, MemoryBus};

impl Cpu {
    /// ADC Immediate (0x69) - Add with Carry
    pub(super) fn op_adc_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let operand = self.addr_immediate(bus, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 3 } else { 2 })
    }

    /// SBC Immediate (0xE9) - Subtract with Carry (Borrow)
    pub(super) fn op_sbc_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let operand = self.addr_immediate(bus, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 3 } else { 2 })
    }

    /// ADC helper shared by every addressing-mode variant of the ADC
    /// opcode (immediate, absolute, direct-page, stack-relative, etc.).
    /// Despite the name, this is the single dispatch point for both
    /// binary and BCD (Decimal-flag) arithmetic -- putting the check here
    /// rather than in each `op_adc_*` means every addressing mode gets
    /// correct decimal-mode behavior for free.
    pub(super) fn adc_binary(&mut self, operand: u16, is_16bit: bool) {
        if self.p.contains(CpuFlags::DECIMAL) {
            self.adc_decimal(operand, is_16bit);
            return;
        }
        let a = self.a;
        let c = if self.p.contains(CpuFlags::CARRY) { 1 } else { 0 };

        if is_16bit {
            let result = (a as u32) + (operand as u32) + c;
            self.a = (result & 0xFFFF) as u16;

            if result > 0xFFFF { self.p.insert(CpuFlags::CARRY); } else { self.p.remove(CpuFlags::CARRY); }

            let overflow = (!(a ^ operand) & (a ^ self.a) & 0x8000) != 0;
            if overflow { self.p.insert(CpuFlags::OVERFLOW); } else { self.p.remove(CpuFlags::OVERFLOW); }

            self.update_nz_flags_16(self.a);
        } else {
            let a_8 = (a & 0xFF) as u16;
            let op_8 = (operand & 0xFF) as u16;
            let result = a_8 + op_8 + (c as u16);

            self.a = (self.a & 0xFF00) | (result & 0xFF);

            if result > 0xFF { self.p.insert(CpuFlags::CARRY); } else { self.p.remove(CpuFlags::CARRY); }

            let overflow = (!(a_8 ^ op_8) & (a_8 ^ (self.a & 0xFF)) & 0x80) != 0;
            if overflow { self.p.insert(CpuFlags::OVERFLOW); } else { self.p.remove(CpuFlags::OVERFLOW); }

            self.update_nz_flags_8((self.a & 0xFF) as u8);
        }
    }

    /// SBC helper shared by every addressing-mode variant of the SBC
    /// opcode, mirroring `adc_binary`'s dispatch role. Binary-mode SBC is
    /// implemented as ADC of the bitwise-NOT'd operand (the standard
    /// two's-complement trick); decimal mode cannot reuse that trick (BCD
    /// subtraction needs genuine per-digit borrow propagation), so it
    /// dispatches to `sbc_decimal` instead.
    pub(super) fn sbc_binary(&mut self, operand: u16, is_16bit: bool) {
        if self.p.contains(CpuFlags::DECIMAL) {
            self.sbc_decimal(operand, is_16bit);
            return;
        }
        // SBC is simply ADC with the bitwise NOT of the operand
        if is_16bit {
            self.adc_binary(!operand, true);
        } else {
            self.adc_binary((!operand) & 0xFF, false);
        }
    }

    /// ADC in BCD (Decimal-flag) mode. Treats the accumulator and operand
    /// as packed BCD digits (one per nibble) and adds them digit-by-digit
    /// with carry propagation: whenever a digit's raw binary sum exceeds
    /// 9, it's corrected by adding 6 and a carry is generated into the
    /// next digit -- the standard decimal-adjust used by 6502-family
    /// CPUs. Carry reflects the final digit's carry-out; N/Z are derived
    /// from the BCD-corrected result byte/word (the 65C816, unlike the
    /// NMOS 6502, sets N/Z correctly in decimal mode rather than from
    /// binary-looking garbage). Overflow is computed with the same
    /// sign-based formula used in binary mode, applied to the corrected
    /// result -- decimal-mode V is rarely relied upon by real software
    /// and the 65816 doesn't guarantee a specific semantic for it, so
    /// this is a reasonable best-effort rather than a hardware-verified
    /// exact match.
    pub(super) fn adc_decimal(&mut self, operand: u16, is_16bit: bool) {
        let carry_in: u32 = if self.p.contains(CpuFlags::CARRY) { 1 } else { 0 };
        if is_16bit {
            let a = self.a;
            let op = operand;
            let mut result: u32 = 0;
            let mut carry = carry_in;
            for nibble in 0..4u32 {
                let shift = nibble * 4;
                let an = ((a as u32) >> shift) & 0xF;
                let bn = ((op as u32) >> shift) & 0xF;
                let mut sum = an + bn + carry;
                if sum > 9 {
                    sum += 6;
                    carry = 1;
                } else {
                    carry = 0;
                }
                result |= (sum & 0xF) << shift;
            }
            let result = result as u16;
            if carry != 0 { self.p.insert(CpuFlags::CARRY); } else { self.p.remove(CpuFlags::CARRY); }
            let overflow = (!(a ^ op) & (a ^ result) & 0x8000) != 0;
            if overflow { self.p.insert(CpuFlags::OVERFLOW); } else { self.p.remove(CpuFlags::OVERFLOW); }
            self.a = result;
            self.update_nz_flags_16(self.a);
        } else {
            let a = (self.a & 0xFF) as u32;
            let op = (operand & 0xFF) as u32;
            let mut lo = (a & 0xF) + (op & 0xF) + carry_in;
            let carry_lo = if lo > 9 { lo += 6; 1 } else { 0 };
            let mut hi = ((a >> 4) & 0xF) + ((op >> 4) & 0xF) + carry_lo;
            let carry_hi = if hi > 9 { hi += 6; 1 } else { 0 };
            let result = (((hi & 0xF) << 4) | (lo & 0xF)) as u8;
            if carry_hi != 0 { self.p.insert(CpuFlags::CARRY); } else { self.p.remove(CpuFlags::CARRY); }
            let overflow = (!((a as u8) ^ (op as u8)) & ((a as u8) ^ result) & 0x80) != 0;
            if overflow { self.p.insert(CpuFlags::OVERFLOW); } else { self.p.remove(CpuFlags::OVERFLOW); }
            self.a = (self.a & 0xFF00) | (result as u16);
            self.update_nz_flags_8(result);
        }
    }

    /// SBC in BCD (Decimal-flag) mode. Subtracts packed BCD digits
    /// digit-by-digit with borrow propagation: whenever a digit's raw
    /// binary difference goes negative, it's corrected by adding 10 (the
    /// decimal-subtract mirror of ADC's "add 6") and a borrow propagates
    /// into the next digit. Carry is set when no final borrow occurred
    /// (matching standard 6502-family SBC carry semantics: Carry=1 means
    /// "no borrow"); N/Z are derived from the BCD-corrected result. See
    /// `adc_decimal`'s doc comment for the same caveat on Overflow.
    pub(super) fn sbc_decimal(&mut self, operand: u16, is_16bit: bool) {
        let borrow_in: i32 = if self.p.contains(CpuFlags::CARRY) { 0 } else { 1 };
        if is_16bit {
            let a = self.a;
            let op = operand;
            let mut result: u16 = 0;
            let mut borrow = borrow_in;
            for nibble in 0..4u32 {
                let shift = nibble * 4;
                let an = ((a >> shift) & 0xF) as i32;
                let bn = ((op >> shift) & 0xF) as i32;
                let mut diff = an - bn - borrow;
                if diff < 0 {
                    diff += 10;
                    borrow = 1;
                } else {
                    borrow = 0;
                }
                result |= ((diff as u16) & 0xF) << shift;
            }
            if borrow == 0 { self.p.insert(CpuFlags::CARRY); } else { self.p.remove(CpuFlags::CARRY); }
            let overflow = ((a ^ op) & (a ^ result) & 0x8000) != 0;
            if overflow { self.p.insert(CpuFlags::OVERFLOW); } else { self.p.remove(CpuFlags::OVERFLOW); }
            self.a = result;
            self.update_nz_flags_16(self.a);
        } else {
            let a = (self.a & 0xFF) as i32;
            let op = (operand & 0xFF) as i32;
            let mut lo = (a & 0xF) - (op & 0xF) - borrow_in;
            let borrow_lo = if lo < 0 { lo += 10; 1 } else { 0 };
            let mut hi = ((a >> 4) & 0xF) - ((op >> 4) & 0xF) - borrow_lo;
            let borrow_hi = if hi < 0 { hi += 10; 1 } else { 0 };
            let result = (((hi & 0xF) << 4) | (lo & 0xF)) as u8;
            if borrow_hi == 0 { self.p.insert(CpuFlags::CARRY); } else { self.p.remove(CpuFlags::CARRY); }
            let overflow = (((a as u8) ^ (op as u8)) & ((a as u8) ^ result) & 0x80) != 0;
            if overflow { self.p.insert(CpuFlags::OVERFLOW); } else { self.p.remove(CpuFlags::OVERFLOW); }
            self.a = (self.a & 0xFF00) | (result as u16);
            self.update_nz_flags_8(result);
        }
    }

    // ==================== Logical Operations ====================

    pub(super) fn op_adc_sr(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative(bus)? as u32;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    pub(super) fn op_adc_sr_indirect_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative_indirect_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 8 } else { 7 })
    }

    pub(super) fn op_sbc_sr(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative(bus)? as u32;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    pub(super) fn op_sbc_sr_indirect_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative_indirect_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 8 } else { 7 })
    }

    /// ADC [$dp] (0x67)
    pub(super) fn op_adc_indirect_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// SBC [$dp] (0xE7)
    pub(super) fn op_sbc_indirect_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    pub(super) fn op_adc_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    pub(super) fn op_adc_abs_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    pub(super) fn op_sbc_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    pub(super) fn op_sbc_abs_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// ADC $addr (long) (0x6F)
    pub(super) fn op_adc_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// SBC $addr (long) (0xEF)
    pub(super) fn op_sbc_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// ADC $addr,X (long) (0x7F)
    pub(super) fn op_adc_long_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// SBC $addr,X (long) (0xFF)
    pub(super) fn op_sbc_long_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    // ==================== ALU family: [$dp],Y (indirect long indexed) ====================

    /// ADC [$dp],Y (0x77)
    pub(super) fn op_adc_indirect_long_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// SBC [$dp],Y (0xF7)
    pub(super) fn op_sbc_indirect_long_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    // ==================== LDX/LDY, remaining indexed Direct Page forms ====================

    // ADC/SBC, remaining addressing modes (dp, abs, dp+X, (dp,X), (dp),Y) --
    // only the immediate form existed before. Opcode values verified
    // against wiki.superfamicom.org/65816-reference.
    pub(super) fn op_adc_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 4 + extra } else { 3 + extra })
    }

    pub(super) fn op_adc_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    pub(super) fn op_adc_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    pub(super) fn op_adc_indirect_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    pub(super) fn op_adc_indirect_dp_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    pub(super) fn op_adc_indirect_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    pub(super) fn op_sbc_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 4 + extra } else { 3 + extra })
    }

    pub(super) fn op_sbc_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    pub(super) fn op_sbc_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    pub(super) fn op_sbc_indirect_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    pub(super) fn op_sbc_indirect_dp_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    pub(super) fn op_sbc_indirect_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }
}
