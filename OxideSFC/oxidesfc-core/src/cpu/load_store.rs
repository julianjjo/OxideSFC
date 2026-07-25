//! Loads and stores (LDA/LDX/LDY, STA/STX/STY/STZ) across the addressing
//! modes.

use super::{Cpu, CpuFlags};
use crate::bus::{BusResult, MemoryBus};

impl Cpu {
    /// LDA Immediate (0xA9) - Load Accumulator with immediate value
    pub(super) fn op_lda_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let value = self.addr_immediate(bus, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(value, is_16bit);
        Ok(if is_16bit { 3 } else { 2 })
    }

    /// LDA Absolute (0xAD) - Load Accumulator from absolute address
    pub(super) fn op_lda_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(value, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    /// LDA Direct Page (0xA5) - Load Accumulator from Direct Page
    pub(super) fn op_lda_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(value, is_16bit);
        // +1 cycle if D low byte != 0
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 4 + extra } else { 3 + extra })
    }

    /// LDX Immediate (0xA2) - Load X Register with immediate value
    pub(super) fn op_ldx_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let value = self.addr_immediate(bus, is_16bit)?;
        self.x = value;
        self.update_nz_flags_mem(value, is_16bit);
        Ok(if is_16bit { 3 } else { 2 })
    }

    /// LDX Absolute (0xAE) - Load X Register from absolute address
    pub(super) fn op_ldx_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_absolute(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.x = value;
        self.update_nz_flags_mem(value, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    /// LDX Direct Page (0xA6) - Load X Register from Direct Page
    pub(super) fn op_ldx_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.x = value;
        self.update_nz_flags_mem(value, is_16bit);
        // +1 cycle if D low byte != 0
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 4 + extra } else { 3 + extra })
    }

    /// LDY Immediate (0xA0) - Load Y Register with immediate value
    pub(super) fn op_ldy_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let value = self.addr_immediate(bus, is_16bit)?;
        self.y = value;
        self.update_nz_flags_mem(value, is_16bit);
        Ok(if is_16bit { 3 } else { 2 })
    }

    /// LDY Absolute (0xAC) - Load Y Register from absolute address
    pub(super) fn op_ldy_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_absolute(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.y = value;
        self.update_nz_flags_mem(value, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    /// LDY Direct Page (0xA4) - Load Y Register from Direct Page
    pub(super) fn op_ldy_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.y = value;
        self.update_nz_flags_mem(value, is_16bit);
        // +1 cycle if D low byte != 0
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 4 + extra } else { 3 + extra })
    }

    // ==================== Store Instructions ====================

    /// STA Absolute (0x8D) - Store Accumulator to absolute address
    pub(super) fn op_sta_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 5 } else { 4 })
    }

    /// STA Direct Page (0x85) - Store Accumulator to Direct Page
    pub(super) fn op_sta_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        // +1 cycle if D low byte != 0
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 4 + extra } else { 3 + extra })
    }

    /// STZ Direct Page (0x64) - Store Zero to Direct Page
    pub(super) fn op_stz_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        self.write_memory(bus, addr, 0, is_16bit)?;
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 4 + extra } else { 3 + extra })
    }

    /// STZ Direct Page,X (0x74)
    pub(super) fn op_stz_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        self.write_memory(bus, addr, 0, is_16bit)?;
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 5 + extra } else { 4 + extra })
    }

    /// STA Direct Page,X (0x95)
    pub(super) fn op_sta_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 5 + extra } else { 4 + extra })
    }

    /// LDA Direct Page,X (0xB5)
    pub(super) fn op_lda_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 5 + extra } else { 4 + extra })
    }

    /// STZ Absolute (0x9C) - Store Zero to absolute address
    pub(super) fn op_stz_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        self.write_memory(bus, addr, 0, is_16bit)?;
        Ok(if is_16bit { 5 } else { 4 })
    }

    /// STZ Absolute,X (0x9E)
    pub(super) fn op_stz_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        self.write_memory(bus, addr, 0, is_16bit)?;
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// STX Absolute (0x8E) - Store X Register to absolute address
    pub(super) fn op_stx_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_absolute(bus)?;
        self.write_memory(bus, addr, self.x, is_16bit)?;
        Ok(if is_16bit { 5 } else { 4 })
    }

    /// STX Direct Page (0x86) - Store X Register to Direct Page
    pub(super) fn op_stx_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_direct_page(bus)?;
        self.write_memory(bus, addr, self.x, is_16bit)?;
        // +1 cycle if D low byte != 0
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 4 + extra } else { 3 + extra })
    }

    /// STY Absolute (0x8C) - Store Y Register to absolute address
    pub(super) fn op_sty_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_absolute(bus)?;
        self.write_memory(bus, addr, self.y, is_16bit)?;
        Ok(if is_16bit { 5 } else { 4 })
    }

    /// STY Direct Page (0x84) - Store Y Register to Direct Page
    pub(super) fn op_sty_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_direct_page(bus)?;
        self.write_memory(bus, addr, self.y, is_16bit)?;
        // +1 cycle if D low byte != 0
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 4 + extra } else { 3 + extra })
    }

    // ==================== Control Flow & Branching ====================

    /// STA Absolute Long (0x8F) - Store Accumulator to a 24-bit address
    pub(super) fn op_sta_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// LDA Absolute Long (0xAF) - Load Accumulator from a 24-bit address
    pub(super) fn op_lda_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// STA Absolute Long Indexed,X (0x9F)
    pub(super) fn op_sta_long_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long_x(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// LDA (dp) (0xB2)
    pub(super) fn op_lda_indirect_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// STA (dp) (0x92)
    pub(super) fn op_sta_indirect_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// LDA (dp),Y (0xB1)
    pub(super) fn op_lda_indirect_dp_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_y(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// STA (dp),Y (0x91)
    pub(super) fn op_sta_indirect_dp_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_y(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// LDA (dp,X) (0xA1)
    pub(super) fn op_lda_indirect_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// STA (dp,X) (0x81)
    pub(super) fn op_sta_indirect_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_x(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 7 } else { 6 })
    }

    pub(super) fn op_lda_sr(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative(bus)? as u32;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    pub(super) fn op_lda_sr_indirect_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative_indirect_y(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(if is_16bit { 8 } else { 7 })
    }

    pub(super) fn op_sta_sr(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative(bus)? as u32;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 5 } else { 4 })
    }

    pub(super) fn op_sta_sr_indirect_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative_indirect_y(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 8 } else { 7 })
    }

    /// LDA [$dp] (0xA7)
    pub(super) fn op_lda_indirect_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// STA [$dp] (0x87)
    pub(super) fn op_sta_indirect_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// LDA $addr,X (long) (0xBF)
    pub(super) fn op_lda_long_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// LDY Direct Page,X (0xB4)
    pub(super) fn op_ldy_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.y = value;
        self.update_nz_flags_mem(value, is_16bit);
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 5 + extra } else { 4 + extra })
    }

    /// LDX Direct Page,Y (0xB6)
    pub(super) fn op_ldx_dp_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_direct_page_y(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.x = value;
        self.update_nz_flags_mem(value, is_16bit);
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 5 + extra } else { 4 + extra })
    }

    // ==================== Read-Modify-Write: Direct Page,X / Absolute,X ====================
    // ASL/LSR/ROL/ROR/INC/DEC only had Direct Page and Absolute forms;
    // these are the indexed variants, sharing the same compute logic as
    // the accumulator/dp/abs forms above via small pure helpers.

    /// STA Absolute,Y (0x99)
    pub(super) fn op_sta_abs_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_y(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// STX Direct Page,Y (0x96)
    pub(super) fn op_stx_dp_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_direct_page_y(bus)?;
        self.write_memory(bus, addr, self.x, is_16bit)?;
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 5 + extra } else { 4 + extra })
    }

    /// STY Direct Page,X (0x94)
    pub(super) fn op_sty_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        self.write_memory(bus, addr, self.y, is_16bit)?;
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 5 + extra } else { 4 + extra })
    }

    /// LDA [$dp],Y (0xB7)
    pub(super) fn op_lda_indirect_long_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long_y(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// STA [$dp],Y (0x97)
    pub(super) fn op_sta_indirect_long_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long_y(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// LDA Absolute,Y (0xB9) -- NOT to be confused with "LDA (dp),Y" (the
    /// real opcode for that is 0xB1); an earlier version of this code
    /// wrongly assumed 0xB9 meant "(dp),Y" by mistaken symmetry with 0x91
    /// (STA (dp),Y), which silently consumed the wrong number of operand
    /// bytes (1 instead of 2) for every real "LDA addr,Y" in the ROM,
    /// desyncing instruction-boundary decoding from that point on -- the
    /// root cause of a stack-corruption crash traced through ~560,000
    /// instructions of real SMW execution.
    pub(super) fn op_lda_abs_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_y(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// STA Absolute,X (0x9D)
    pub(super) fn op_sta_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// LDA Absolute,X (0xBD)
    pub(super) fn op_lda_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    /// LDX Absolute,Y (0xBE) - Load X Register from absolute address + Y.
    /// Unlike A (which keeps a hidden high byte across M-mode changes, see
    /// `set_a`), X/Y architecturally zero their high byte in 8-bit index
    /// mode, so a direct assignment from `read_memory`'s already-truncated
    /// value (matching `op_ldx_abs`/`op_ldx_dp`) is correct here.
    pub(super) fn op_ldx_abs_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_absolute_y(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.x = value;
        self.update_nz_flags_mem(value, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    /// LDY Absolute,X (0xBC) - Load Y Register from absolute address + X.
    pub(super) fn op_ldy_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.y = value;
        self.update_nz_flags_mem(value, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    // ==================== Memory Access Helpers ====================
}
