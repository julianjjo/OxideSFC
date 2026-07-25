//! Branches and jumps: the conditional branches, BRA/BRL, JMP/JML in their
//! addressing modes, and the subroutine calls and returns.

use super::{Cpu, CpuFlags};
use crate::bus::{BusResult, MemoryBus};

impl Cpu {
    pub(super) fn branch_if(&mut self, condition: bool, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let offset = self.fetch_u8(bus)? as i8 as i16;
        if condition {
            let old_pc = self.pc;
            self.pc = self.pc.wrapping_add(offset as u16);
            
            // Branch taken costs +1 cycle. If emulation mode and page bound crossed, another +1.
            let mut cycles = 3;
            if self.e && (old_pc & 0xFF00) != (self.pc & 0xFF00) {
                cycles += 1;
            }
            Ok(cycles)
        } else {
            Ok(2)
        }
    }

    /// BCC - Branch if Carry Clear
    pub(super) fn op_bcc(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.branch_if(!self.p.contains(CpuFlags::CARRY), bus)
    }

    /// BCS - Branch if Carry Set
    pub(super) fn op_bcs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.branch_if(self.p.contains(CpuFlags::CARRY), bus)
    }

    /// BNE - Branch if Not Equal (Zero Clear)
    pub(super) fn op_bne(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.branch_if(!self.p.contains(CpuFlags::ZERO), bus)
    }

    /// BEQ - Branch if Equal (Zero Set)
    pub(super) fn op_beq(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.branch_if(self.p.contains(CpuFlags::ZERO), bus)
    }

    /// BPL - Branch if Plus (Negative Clear)
    pub(super) fn op_bpl(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.branch_if(!self.p.contains(CpuFlags::NEGATIVE), bus)
    }

    /// BMI - Branch if Minus (Negative Set)
    pub(super) fn op_bmi(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.branch_if(self.p.contains(CpuFlags::NEGATIVE), bus)
    }

    /// BVC - Branch if Overflow Clear
    pub(super) fn op_bvc(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.branch_if(!self.p.contains(CpuFlags::OVERFLOW), bus)
    }

    /// BVS - Branch if Overflow Set
    pub(super) fn op_bvs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.branch_if(self.p.contains(CpuFlags::OVERFLOW), bus)
    }

    /// BRA - Branch Always
    pub(super) fn op_bra(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.branch_if(true, bus)
    }

    /// JMP Absolute (0x4C) - Jump to new absolute address
    pub(super) fn op_jmp_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.pc = self.fetch_u16(bus)?;
        Ok(3)
    }

    // ==================== Arithmetic ====================

    /// JSR Absolute (0x20) - Jump to Subroutine
    pub(super) fn op_jsr_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let addr = self.fetch_u16(bus)?;
        // Push return address - 1 (PC will be incremented after fetch).
        // The return address is always a full 16-bit PC regardless of
        // mode/flags.
        let return_addr = self.pc.wrapping_sub(1);
        self.push_stack(bus, return_addr, true)?;

        self.pc = addr;
        Ok(6)
    }

    /// JSR ($addr,X) (0xFC) - Jump to Subroutine Absolute Indexed Indirect.
    /// The 16-bit pointer (operand + X) is fetched from the current
    /// Program Bank (PB), not DB -- the same PB-relative rule as
    /// `op_jmp_ix`, since both are same-bank computed jumps introduced
    /// with the 65816. Pushes the same 16-bit return address JSR does.
    pub(super) fn op_jsr_ix(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let operand = self.fetch_u16(bus)?;
        let return_addr = self.pc.wrapping_sub(1);
        self.push_stack(bus, return_addr, true)?;

        let ptr = operand.wrapping_add(self.x);
        let addr = ((self.pb as u32) << 16) | (ptr as u32);
        let target_lo = bus.read_u8(addr)? as u16;
        let target_hi = bus.read_u8(addr.wrapping_add(1))? as u16;
        self.pc = (target_hi << 8) | target_lo;
        Ok(8)
    }

    /// JSL $addr (0x22) - Jump Subroutine Long: calls a 24-bit address in
    /// any bank, pushing PB then PC-1 (so RTL can restore both).
    pub(super) fn op_jsl(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let lo = self.fetch_u8(bus)? as u16;
        let mid = self.fetch_u8(bus)? as u16;
        let target_bank = self.fetch_u8(bus)?;
        let target_offset = (mid << 8) | lo;

        let return_addr = self.pc.wrapping_sub(1);
        self.push_stack(bus, self.pb as u16, false)?;
        self.push_stack(bus, return_addr, true)?;

        self.pb = target_bank;
        self.pc = target_offset;
        Ok(8)
    }

    /// RTL (0x6B) - Return from Subroutine Long: pulls PC then PB (the
    /// reverse order JSL pushed them), then advances PC past the call.
    pub(super) fn op_rtl(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let addr = self.pull_stack(bus, true)?;
        let pb = self.pull_stack(bus, false)? as u8;
        self.pb = pb;
        self.pc = addr.wrapping_add(1);
        Ok(6)
    }

    /// RTS (0x60) - Return from Subroutine
    pub(super) fn op_rts(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let addr = self.pull_stack(bus, true)?;
        self.pc = addr.wrapping_add(1);
        Ok(6)
    }

    /// JMP Indirect (0x6C) - Jump to address pointed by operand. Real 65816
    /// hardware always fetches this pointer from bank 0, regardless of DB
    /// (a 6502-inherited quirk), same as `op_jml_indirect` below.
    pub(super) fn op_jmp_ind(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let ptr = self.fetch_u16(bus)?;
        let addr = ptr as u32;
        let target_lo = bus.read_u8(addr)? as u16;
        let target_hi = bus.read_u8(addr.wrapping_add(1))? as u16;
        self.pc = (target_hi << 8) | target_lo;
        Ok(5)
    }

    /// JML [$addr] (0xDC) - Jump absolute indirect long: the 2-byte
    /// operand is a bank-0 pointer to a 3-byte (24-bit) target address.
    pub(super) fn op_jml_indirect(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let ptr = self.fetch_u16(bus)? as u32;
        let target_lo = bus.read_u8(ptr)? as u16;
        let target_mid = bus.read_u8(ptr.wrapping_add(1))? as u16;
        let target_hi = bus.read_u8(ptr.wrapping_add(2))?;
        self.pc = (target_mid << 8) | target_lo;
        self.pb = target_hi;
        Ok(6)
    }

    /// JMP Indirect,X (0x7C) - Jump to address pointed by operand + X. This
    /// pointer is fetched from the current Program Bank (PB), not DB, since
    /// it's a same-bank computed jump introduced with the 65816.
    pub(super) fn op_jmp_ix(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let ptr = self.fetch_u16(bus)?;
        let ptr = ptr.wrapping_add(self.x);
        let addr = ((self.pb as u32) << 16) | (ptr as u32);
        let target_lo = bus.read_u8(addr)? as u16;
        let target_hi = bus.read_u8(addr.wrapping_add(1))? as u16;
        self.pc = (target_hi << 8) | target_lo;
        Ok(6)
    }

    /// BRL (0x82) - Branch Always Long
    pub(super) fn op_brl(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let offset = self.fetch_u16(bus)?;
        // Sign-extend offset to i32
        let offset = if offset & 0x8000 != 0 {
            (offset as i32) | -65536i32  // 0xFFFF0000 as i32
        } else {
            offset as i32
        };
        self.pc = self.pc.wrapping_add(offset as u16);
        Ok(4)
    }

    /// JML $addr (0x5C) - Jump (long) to a 24-bit absolute address.
    pub(super) fn op_jml(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let lo = self.fetch_u8(bus)? as u16;
        let mid = self.fetch_u8(bus)? as u16;
        let bank = self.fetch_u8(bus)?;
        self.pc = (mid << 8) | lo;
        self.pb = bank;
        Ok(4)
    }

    /// PER label (0x62) - Push Effective Relative: pushes (PC after this
    /// instruction + signed 16-bit offset).
    pub(super) fn op_per(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let offset = self.fetch_u16(bus)?;
        let value = self.pc.wrapping_add(offset);
        self.push_stack(bus, value, true)?;
        Ok(6)
    }
}
