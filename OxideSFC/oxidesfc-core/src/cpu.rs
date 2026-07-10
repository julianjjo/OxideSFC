use crate::bus::{BusResult, MemoryBus};
#[cfg(test)]
use crate::error::EmulationError;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CpuFlags: u8 {
        const CARRY             = 0b0000_0001; // C
        const ZERO              = 0b0000_0010; // Z
        const IRQ_DISABLE       = 0b0000_0100; // I
        const DECIMAL           = 0b0000_1000; // D
        const INDEX_8BIT        = 0b0001_0000; // X (1=8-bit indices)
        const MEMORY_8BIT       = 0b0010_0000; // M (1=8-bit mem/acc)
        const OVERFLOW          = 0b0100_0000; // V
        const NEGATIVE          = 0b1000_0000; // N
    }
}

pub struct Cpu {
    pub a: u16,
    pub x: u16,
    pub y: u16,
    pub pc: u16,
    pub pb: u8,
    pub sp: u16,
    pub db: u8,
    pub d: u16,
    pub p: CpuFlags,
    pub e: bool,
    pub cycles: u64,
    /// Set by WAI (0xCB) or STP (0xDB): suspends instruction fetch until
    /// `nmi()` wakes the CPU back up (real STP technically only wakes on a
    /// full reset, but treating it the same as WAI is a harmless
    /// simplification -- neither opcode is expected in normal gameplay
    /// code, just defensive coverage so hitting one doesn't read as an
    /// unimplemented-opcode halt).
    pub waiting_for_interrupt: bool,
    /// Side channel used by `op_mvn`/`op_mvp` to report their true cycle
    /// cost. Those two opcodes move up to 65536 bytes at 7 cycles/byte
    /// (up to 458,752 cycles) in a single `step()` call, which doesn't fit
    /// in the `u8` every other opcode handler returns; they stash the
    /// overflow here and `execute()` folds it into the widened `u32`
    /// total immediately after dispatch, so it never survives past a
    /// single instruction.
    pending_cycle_adjustment: u32,
    #[cfg(feature = "stack_shadow_debug")]
    pub shadow_stack: Vec<(u32, u8)>,
    #[cfg(feature = "stack_shadow_debug")]
    pub stack_mismatch: Option<String>,
    #[cfg(feature = "stack_shadow_debug")]
    pub instruction_trace: std::collections::VecDeque<(u32, u8, u32)>,
}

impl Cpu {
    pub fn new() -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            pc: 0,
            pb: 0,
            sp: 0x01FF,
            db: 0,
            d: 0,
            p: CpuFlags::IRQ_DISABLE | CpuFlags::MEMORY_8BIT | CpuFlags::INDEX_8BIT,
            e: true,
            cycles: 0,
            waiting_for_interrupt: false,
            pending_cycle_adjustment: 0,
            #[cfg(feature = "stack_shadow_debug")]
            shadow_stack: Vec::new(),
            #[cfg(feature = "stack_shadow_debug")]
            stack_mismatch: None,
            #[cfg(feature = "stack_shadow_debug")]
            instruction_trace: std::collections::VecDeque::new(),
        }
    }

    /// Serializes the complete architectural CPU state (registers, flags,
    /// emulation-mode bit, WAI latch) for save states. The
    /// `stack_shadow_debug` diagnostic fields are intentionally excluded.
    pub fn save_state(&self, out: &mut Vec<u8>) {
        crate::state::put_u16(out, self.a);
        crate::state::put_u16(out, self.x);
        crate::state::put_u16(out, self.y);
        crate::state::put_u16(out, self.pc);
        crate::state::put_u8(out, self.pb);
        crate::state::put_u16(out, self.sp);
        crate::state::put_u8(out, self.db);
        crate::state::put_u16(out, self.d);
        crate::state::put_u8(out, self.p.bits());
        crate::state::put_bool(out, self.e);
        crate::state::put_u64(out, self.cycles);
        crate::state::put_bool(out, self.waiting_for_interrupt);
    }

    /// Restores state produced by `save_state`.
    pub(crate) fn load_state(&mut self, r: &mut crate::state::StateReader) -> Result<(), crate::error::EmulationError> {
        self.a = r.u16()?;
        self.x = r.u16()?;
        self.y = r.u16()?;
        self.pc = r.u16()?;
        self.pb = r.u8()?;
        self.sp = r.u16()?;
        self.db = r.u8()?;
        self.d = r.u16()?;
        self.p = CpuFlags::from_bits_truncate(r.u8()?);
        self.e = r.bool()?;
        self.cycles = r.u64()?;
        self.waiting_for_interrupt = r.bool()?;
        self.pending_cycle_adjustment = 0;
        Ok(())
    }

    pub fn reset(&mut self, bus: &mut impl MemoryBus) -> BusResult<()> {
        // Load reset vector from $FFFC-$FFFD
        let pc_lo = bus.read_u8(0xFFFC)?;
        let pc_hi = bus.read_u8(0xFFFD)?;
        self.pc = ((pc_hi as u16) << 8) | (pc_lo as u16);

        // Reset state
        self.pb = 0;
        self.db = 0;
        self.d = 0;
        self.sp = (self.sp & 0x00FF) | 0x0100; // Preserve low byte, set high to 0x01
        self.p = CpuFlags::IRQ_DISABLE | CpuFlags::MEMORY_8BIT | CpuFlags::INDEX_8BIT;
        self.e = true;
        self.a = 0;
        self.x = 0;
        self.y = 0;
        self.waiting_for_interrupt = false;
        self.pending_cycle_adjustment = 0;

        Ok(())
    }

    /// Services a non-maskable interrupt: pushes the return context onto
    /// the stack and jumps to the NMI vector. Mirrors `op_rti`'s pull
    /// order in reverse so the two stay symmetric -- emulation mode pushes
    /// only PC then P (no bank, matching the 6502-style 3-byte frame
    /// `op_rti` pulls), native mode additionally pushes PB first (4-byte
    /// frame). The interrupt vector is $FFEA/$FFEB in native mode and
    /// $FFFA/$FFFB in emulation mode. Real hardware doesn't check NMI mid
    /// instruction -- callers should only invoke this between `step()`
    /// calls.
    pub fn nmi(&mut self, bus: &mut impl MemoryBus) -> BusResult<()> {
        self.waiting_for_interrupt = false;
        if !self.e {
            self.push_stack(bus, self.pb as u16, false)?;
        }
        self.push_stack(bus, self.pc, true)?;
        self.push_stack(bus, self.p.bits() as u16, false)?;

        self.p.remove(CpuFlags::DECIMAL);
        self.p.insert(CpuFlags::IRQ_DISABLE);

        let vector = if self.e { 0xFFFA_u32 } else { 0xFFEA_u32 };
        let lo = bus.read_u8(vector)? as u16;
        let hi = bus.read_u8(vector.wrapping_add(1))? as u16;
        self.pc = (hi << 8) | lo;
        self.pb = 0;

        Ok(())
    }

    /// Services a maskable IRQ (e.g. the PPU H/V-timer interrupt SMW uses
    /// for its in-level status-bar raster split). Same push/vector shape
    /// as `nmi()` but through the IRQ vectors ($FFEE native, $FFFE
    /// emulation). The CALLER must check `CpuFlags::IRQ_DISABLE` first --
    /// the 65816 ignores the (level-triggered) IRQ line while I is set,
    /// and the line stays asserted in the bus until software acknowledges
    /// it (reading $4211), so dispatching while I is set would re-enter
    /// forever.
    pub fn irq(&mut self, bus: &mut impl MemoryBus) -> BusResult<()> {
        self.waiting_for_interrupt = false;
        if !self.e {
            self.push_stack(bus, self.pb as u16, false)?;
        }
        self.push_stack(bus, self.pc, true)?;
        self.push_stack(bus, self.p.bits() as u16, false)?;

        self.p.remove(CpuFlags::DECIMAL);
        self.p.insert(CpuFlags::IRQ_DISABLE);

        let vector = if self.e { 0xFFFE_u32 } else { 0xFFEE_u32 };
        let lo = bus.read_u8(vector)? as u16;
        let hi = bus.read_u8(vector.wrapping_add(1))? as u16;
        self.pc = (hi << 8) | lo;
        self.pb = 0;

        Ok(())
    }

    /// Wakes the CPU out of a WAI/STP-induced halt when `pending` is
    /// true, without dispatching an interrupt handler. Real 65816
    /// hardware resumes normal instruction fetch on ANY asserted
    /// interrupt line (NMI or IRQ), even while the I flag masks IRQ
    /// dispatch -- it just won't jump to a handler in that case. `nmi()`
    /// already clears `waiting_for_interrupt` whenever it actually runs
    /// (and NMI dispatch is never gated on I), so this method exists for
    /// the IRQ side: callers should invoke it with the bus's live
    /// interrupt-line state (e.g. `bus.irq_pending() || nmi_pending`)
    /// BEFORE the IRQ_DISABLE-gated call to `irq()`, so a WAI right
    /// before/around SEI doesn't hang forever waiting for a handler that
    /// will never be allowed to run.
    pub fn wake_if_interrupt_pending(&mut self, pending: bool) {
        if pending {
            self.waiting_for_interrupt = false;
        }
    }

    /// Lee un byte de la dirección actual (PB:PC) y avanza PC
    pub fn fetch_u8(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let addr = ((self.pb as u32) << 16) | (self.pc as u32);
        let byte = bus.read_u8(addr)?;
        self.pc = self.pc.wrapping_add(1);
        Ok(byte)
    }

    /// Lee un word de la dirección actual (little-endian) y avanza PC
    pub fn fetch_u16(&mut self, bus: &mut impl MemoryBus) -> BusResult<u16> {
        let lo = self.fetch_u8(bus)? as u16;
        let hi = self.fetch_u8(bus)? as u16;
        Ok((hi << 8) | lo)
    }

    /// Ejecuta un ciclo de instrucción
    ///
    /// Returns the number of cycles the executed instruction cost. This is
    /// `u32` rather than `u8` solely to accommodate `MVN`/`MVP` (0x54/0x44),
    /// which can move up to 65536 bytes at 7 cycles/byte -- up to 458,752
    /// cycles -- in a single call; every other opcode's cost still fits
    /// comfortably in a handful of bits.
    pub fn step(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        if self.waiting_for_interrupt {
            // WAI/STP suspended fetch -- only `nmi()`, or `irq()`/
            // `wake_if_interrupt_pending()` on an asserted IRQ line, can
            // resume it.
            return Ok(1);
        }
        #[cfg(feature = "stack_shadow_debug")]
        let pc_before_full = ((self.pb as u32) << 16) | (self.pc as u32);
        let opcode = self.fetch_u8(bus)?;
        let result = self.execute(opcode, bus);
        #[cfg(feature = "stack_shadow_debug")]
        {
            let pc_after_full = ((self.pb as u32) << 16) | (self.pc as u32);
            self.instruction_trace.push_back((pc_before_full, opcode, pc_after_full));
            if self.instruction_trace.len() > 300 {
                self.instruction_trace.pop_front();
            }
        }
        result
    }

    /// Dispatches a single opcode. Every handler except `op_mvn`/`op_mvp`
    /// returns its cycle cost directly as `BusResult<u8>`; those two
    /// instead stash their (potentially much larger) true cost in
    /// `self.pending_cycle_adjustment` and return `Ok(0)`, which gets
    /// folded into the widened `u32` result below. This keeps every other
    /// opcode handler's `BusResult<u8>` signature untouched.
    fn execute(&mut self, opcode: u8, bus: &mut impl MemoryBus) -> BusResult<u32> {
        self.pending_cycle_adjustment = 0;
        let base_cycles: BusResult<u8> = match opcode {
            // NOP
            0xEA => self.op_nop(),

            // Transfer instructions
            0xAA => self.op_tax(),           // TAX
            0x8A => self.op_txa(),           // TXA
            0xA8 => self.op_tay(),           // TAY
            0x98 => self.op_tya(),           // TYA
            0xBA => self.op_tsx(),           // TSX
            0x9A => self.op_txs(),           // TXS

            // Flag instructions
            0x18 => self.op_clc(),           // CLC
            0x38 => self.op_sec(),           // SEC
            0xD8 => self.op_cld(),           // CLD
            0xF8 => self.op_sed(),           // SED
            0x58 => self.op_cli(),           // CLI
            0x78 => self.op_sei(),           // SEI
            0xB8 => self.op_clv(),           // CLV

            // LDA - Load Accumulator
            0xA9 => self.op_lda_imm(bus),    // LDA #const (Immediate)
            0xAD => self.op_lda_abs(bus),    // LDA $addr (Absolute)
            0xA5 => self.op_lda_dp(bus),     // LDA $dp (Direct Page)

            // LDX - Load X Register
            0xA2 => self.op_ldx_imm(bus),    // LDX #const (Immediate)
            0xAE => self.op_ldx_abs(bus),    // LDX $addr (Absolute)
            0xA6 => self.op_ldx_dp(bus),     // LDX $dp (Direct Page)
            0xBE => self.op_ldx_abs_y(bus),  // LDX $addr,Y (Absolute,Y)

            // LDY - Load Y Register
            0xA0 => self.op_ldy_imm(bus),    // LDY #const (Immediate)
            0xAC => self.op_ldy_abs(bus),    // LDY $addr (Absolute)
            0xA4 => self.op_ldy_dp(bus),     // LDY $dp (Direct Page)
            0xBC => self.op_ldy_abs_x(bus),  // LDY $addr,X (Absolute,X)

            // STA - Store Accumulator
            0x8D => self.op_sta_abs(bus),    // STA $addr (Absolute)
            0x85 => self.op_sta_dp(bus),     // STA $dp (Direct Page)

            // STX - Store X Register
            0x8E => self.op_stx_abs(bus),    // STX $addr (Absolute)
            0x86 => self.op_stx_dp(bus),      // STX $dp (Direct Page)

            // STY - Store Y Register
            0x8C => self.op_sty_abs(bus),    // STY $addr (Absolute)
            0x84 => self.op_sty_dp(bus),      // STY $dp (Direct Page)

            // STZ - Store Zero
            0x9C => self.op_stz_abs(bus),    // STZ $addr (Absolute)
            0x64 => self.op_stz_dp(bus),     // STZ $dp (Direct Page)
            0x74 => self.op_stz_dp_x(bus),   // STZ $dp,X (Direct Page,X)
            0x9E => self.op_stz_abs_x(bus),  // STZ $addr,X (Absolute,X)
            0x22 => self.op_jsl(bus),        // JSL $addr (24-bit, Jump Subroutine Long)
            0x6B => self.op_rtl(bus),        // RTL (Return from Subroutine Long)
            0xDC => self.op_jml_indirect(bus), // JML [$addr] (Indirect Long)
            0xB1 => self.op_lda_indirect_dp_y(bus), // LDA (dp),Y
            0xB9 => self.op_lda_abs_y(bus),         // LDA $addr,Y (Absolute,Y)
            0x91 => self.op_sta_indirect_dp_y(bus), // STA (dp),Y
            0x87 => self.op_sta_indirect_long(bus), // STA [$dp]
            0x07 => self.op_ora_indirect_long(bus), // ORA [$dp]
            0x27 => self.op_and_indirect_long(bus), // AND [$dp]
            0x47 => self.op_eor_indirect_long(bus), // EOR [$dp]
            0xC7 => self.op_cmp_indirect_long(bus), // CMP [$dp]
            0x67 => self.op_adc_indirect_long(bus), // ADC [$dp]
            0xE7 => self.op_sbc_indirect_long(bus), // SBC [$dp]
            0x1D => self.op_ora_abs_x(bus),  // ORA $addr,X
            0x19 => self.op_ora_abs_y(bus),  // ORA $addr,Y
            0x3D => self.op_and_abs_x(bus),  // AND $addr,X
            0x39 => self.op_and_abs_y(bus),  // AND $addr,Y
            0x5D => self.op_eor_abs_x(bus),  // EOR $addr,X
            0x59 => self.op_eor_abs_y(bus),  // EOR $addr,Y
            0x7D => self.op_adc_abs_x(bus),  // ADC $addr,X
            0x79 => self.op_adc_abs_y(bus),  // ADC $addr,Y
            0xFD => self.op_sbc_abs_x(bus),  // SBC $addr,X
            0xF9 => self.op_sbc_abs_y(bus),  // SBC $addr,Y
            0xDD => self.op_cmp_abs_x(bus),  // CMP $addr,X
            0xD9 => self.op_cmp_abs_y(bus),  // CMP $addr,Y
            0x99 => self.op_sta_abs_y(bus),  // STA $addr,Y
            0x96 => self.op_stx_dp_y(bus),   // STX $dp,Y
            0x94 => self.op_sty_dp_x(bus),   // STY $dp,X
            0xB2 => self.op_lda_indirect_dp(bus), // LDA (dp)
            0x92 => self.op_sta_indirect_dp(bus), // STA (dp)
            0x65 => self.op_adc_dp(bus),
            0x6D => self.op_adc_abs(bus),
            0x75 => self.op_adc_dp_x(bus),
            0x61 => self.op_adc_indirect_dp_x(bus),
            0x71 => self.op_adc_indirect_dp_y(bus),
            0x72 => self.op_adc_indirect_dp(bus),
            0xE5 => self.op_sbc_dp(bus),
            0xED => self.op_sbc_abs(bus),
            0xF5 => self.op_sbc_dp_x(bus),
            0xE1 => self.op_sbc_indirect_dp_x(bus),
            0xF1 => self.op_sbc_indirect_dp_y(bus),
            0xF2 => self.op_sbc_indirect_dp(bus),
            0x15 => self.op_ora_dp_x(bus),
            0x01 => self.op_ora_indirect_dp_x(bus),
            0x11 => self.op_ora_indirect_dp_y(bus),
            0x12 => self.op_ora_indirect_dp(bus),
            0x35 => self.op_and_dp_x(bus),
            0x21 => self.op_and_indirect_dp_x(bus),
            0x31 => self.op_and_indirect_dp_y(bus),
            0x32 => self.op_and_indirect_dp(bus),
            0x55 => self.op_eor_dp_x(bus),
            0x41 => self.op_eor_indirect_dp_x(bus),
            0x51 => self.op_eor_indirect_dp_y(bus),
            0x52 => self.op_eor_indirect_dp(bus),
            0xD5 => self.op_cmp_dp_x(bus),
            0xC1 => self.op_cmp_indirect_dp_x(bus),
            0xD1 => self.op_cmp_indirect_dp_y(bus),
            0xD2 => self.op_cmp_indirect_dp(bus),
            0xA1 => self.op_lda_indirect_dp_x(bus), // LDA (dp,X)
            0x81 => self.op_sta_indirect_dp_x(bus), // STA (dp,X)
            0x9B => self.op_txy(),           // TXY
            0xBB => self.op_tyx(),           // TYX
            0x95 => self.op_sta_dp_x(bus),   // STA $dp,X (Direct Page,X)
            0xB5 => self.op_lda_dp_x(bus),   // LDA $dp,X (Direct Page,X)

            // Absolute Long (24-bit address, explicit bank)
            0x8F => self.op_sta_long(bus),   // STA $addr (Absolute Long)
            0xAF => self.op_lda_long(bus),   // LDA $addr (Absolute Long)
            0x9F => self.op_sta_long_x(bus), // STA $addr,X (Absolute Long Indexed,X)

            // Absolute Indexed,X
            0x9D => self.op_sta_abs_x(bus),  // STA $addr,X (Absolute,X)
            0xBD => self.op_lda_abs_x(bus),  // LDA $addr,X (Absolute,X)

            // Direct Page Indirect Long
            0xA7 => self.op_lda_indirect_long(bus),    // LDA [$dp]
            0xB7 => self.op_lda_indirect_long_y(bus),  // LDA [$dp],Y
            0x97 => self.op_sta_indirect_long_y(bus),  // STA [$dp],Y

            // Branch instructions
            0x90 => self.op_bcc(bus),        // BCC
            0xB0 => self.op_bcs(bus),        // BCS
            0xD0 => self.op_bne(bus),        // BNE
            0xF0 => self.op_beq(bus),        // BEQ
            0x10 => self.op_bpl(bus),        // BPL
            0x30 => self.op_bmi(bus),        // BMI
            0x50 => self.op_bvc(bus),        // BVC
            0x70 => self.op_bvs(bus),        // BVS
            0x80 => self.op_bra(bus),        // BRA

            // Jumps
            0x4C => self.op_jmp_abs(bus),    // JMP $addr

            // Arithmetic
            0x69 => self.op_adc_imm(bus),    // ADC #const
            0xE9 => self.op_sbc_imm(bus),    // SBC #const

            // Logical operations
            0x29 => self.op_and_imm(bus),     // AND #const
            0x2D => self.op_and_abs(bus),    // AND $addr
            0x25 => self.op_and_dp(bus),     // AND $dp
            0x09 => self.op_ora_imm(bus),     // ORA #const
            0x0D => self.op_ora_abs(bus),    // ORA $addr
            0x05 => self.op_ora_dp(bus),     // ORA $dp
            0x49 => self.op_eor_imm(bus),     // EOR #const
            0x4D => self.op_eor_abs(bus),    // EOR $addr
            0x45 => self.op_eor_dp(bus),     // EOR $dp
            0x24 => self.op_bit_dp(bus),     // BIT $dp
            0x2C => self.op_bit_abs(bus),    // BIT $addr

            // Comparison operations
            0xC9 => self.op_cmp_imm(bus),     // CMP #const
            0xCD => self.op_cmp_abs(bus),    // CMP $addr
            0xC5 => self.op_cmp_dp(bus),     // CMP $dp
            0xE0 => self.op_cpx_imm(bus),     // CPX #const
            0xEC => self.op_cpx_abs(bus),    // CPX $addr
            0xE4 => self.op_cpx_dp(bus),     // CPX $dp
            0xC0 => self.op_cpy_imm(bus),     // CPY #const
            0xCC => self.op_cpy_abs(bus),    // CPY $addr
            0xC4 => self.op_cpy_dp(bus),     // CPY $dp

            // Increment/Decrement
            0xE8 => self.op_inx(),            // INX
            0xCA => self.op_dex(),            // DEX
            0xC8 => self.op_iny(),            // INY
            0x88 => self.op_dey(),            // DEY
            0xE6 => self.op_inc_dp(bus),     // INC $dp
            0xEE => self.op_inc_abs(bus),    // INC $addr
            0xC6 => self.op_dec_dp(bus),     // DEC $dp
            0xCE => self.op_dec_abs(bus),    // DEC $addr

            // Increment/Decrement Accumulator
            0x1A => self.op_inc_acc(),        // INC A
            0x3A => self.op_dec_acc(),        // DEC A

            // Shift/Rotate
            0x0A => self.op_asl_acc(),        // ASL A
            0x4A => self.op_lsr_acc(),        // LSR A
            0x2A => self.op_rol_acc(),        // ROL A
            0x6A => self.op_ror_acc(),        // ROR A
            0x06 => self.op_asl_dp(bus),     // ASL $dp
            0x0E => self.op_asl_abs(bus),    // ASL $addr
            0x46 => self.op_lsr_dp(bus),     // LSR $dp
            0x4E => self.op_lsr_abs(bus),    // LSR $addr
            0x26 => self.op_rol_dp(bus),     // ROL $dp
            0x2E => self.op_rol_abs(bus),    // ROL $addr
            0x66 => self.op_ror_dp(bus),     // ROR $dp
            0x6E => self.op_ror_abs(bus),    // ROR $addr

            // Stack operations
            0x48 => self.op_pha(bus),         // PHA
            0x68 => self.op_pla(bus),         // PLA
            0xDA => self.op_phx(bus),         // PHX
            0xFA => self.op_plx(bus),         // PLX
            0x5A => self.op_phy(bus),         // PHY
            0x7A => self.op_ply(bus),         // PLY
            0x08 => self.op_php(bus),         // PHP
            0x28 => self.op_plp(bus),         // PLP

            // Jump/Call
            0x20 => self.op_jsr_abs(bus),    // JSR $addr
            0xFC => self.op_jsr_ix(bus),     // JSR ($addr,X)
            0x60 => self.op_rts(bus),         // RTS
            0x40 => self.op_rti(bus),         // RTI
            0x6C => self.op_jmp_ind(bus),    // JMP ($addr)
            0x7C => self.op_jmp_ix(bus),      // JMP ($addr,X)

            // Branch (additional)
            0x82 => self.op_brl(bus),         // BRL

            // REP/SEP (processor flags)
            0xC2 => self.op_rep(bus),         // REP #const
            0xE2 => self.op_sep(bus),         // SEP #const

            // XCE (exchange carry and emulation flag)
            0xFB => self.op_xce(),            // XCE
            0xEB => self.op_xba(),            // XBA (exchange accumulator bytes)

            // Direct Page / Stack-pointer transfers (always full 16-bit,
            // since D and S are always 16-bit registers regardless of the
            // M/X width flags)
            0x5B => self.op_tcd(),            // TCD
            0x7B => self.op_tdc(),            // TDC
            0x1B => self.op_tcs(),            // TCS
            0x3B => self.op_tsc(),            // TSC

            // Bank/Direct-Page register stack operations
            0x8B => self.op_phb(bus),         // PHB
            0xAB => self.op_plb(bus),         // PLB
            0x0B => self.op_phd(bus),         // PHD
            0x2B => self.op_pld(bus),         // PLD
            0x4B => self.op_phk(bus),         // PHK

            // ALU long / long,X / [dp],Y
            0x0F => self.op_ora_long(bus),
            0x2F => self.op_and_long(bus),
            0x4F => self.op_eor_long(bus),
            0x6F => self.op_adc_long(bus),
            0xCF => self.op_cmp_long(bus),
            0xEF => self.op_sbc_long(bus),
            0xBF => self.op_lda_long_x(bus),
            0x1F => self.op_ora_long_x(bus),
            0x3F => self.op_and_long_x(bus),
            0x5F => self.op_eor_long_x(bus),
            0x7F => self.op_adc_long_x(bus),
            0xDF => self.op_cmp_long_x(bus),
            0xFF => self.op_sbc_long_x(bus),
            0x17 => self.op_ora_indirect_long_y(bus),
            0x37 => self.op_and_indirect_long_y(bus),
            0x57 => self.op_eor_indirect_long_y(bus),
            0x77 => self.op_adc_indirect_long_y(bus),
            0xD7 => self.op_cmp_indirect_long_y(bus),
            0xF7 => self.op_sbc_indirect_long_y(bus),

            // LDX/LDY remaining indexed Direct Page forms
            0xB4 => self.op_ldy_dp_x(bus),
            0xB6 => self.op_ldx_dp_y(bus),

            // RMW Direct Page,X / Absolute,X
            0x16 => self.op_asl_dp_x(bus),
            0x1E => self.op_asl_abs_x(bus),
            0x56 => self.op_lsr_dp_x(bus),
            0x5E => self.op_lsr_abs_x(bus),
            0x36 => self.op_rol_dp_x(bus),
            0x3E => self.op_rol_abs_x(bus),
            0x76 => self.op_ror_dp_x(bus),
            0x7E => self.op_ror_abs_x(bus),
            0xD6 => self.op_dec_dp_x(bus),
            0xDE => self.op_dec_abs_x(bus),
            0xF6 => self.op_inc_dp_x(bus),
            0xFE => self.op_inc_abs_x(bus),

            // TSB/TRB and remaining BIT forms
            0x04 => self.op_tsb_dp(bus),
            0x0C => self.op_tsb_abs(bus),
            0x14 => self.op_trb_dp(bus),
            0x1C => self.op_trb_abs(bus),
            0x89 => self.op_bit_imm(bus),
            0x34 => self.op_bit_dp_x(bus),
            0x3C => self.op_bit_abs_x(bus),

            // Block move
            0x54 => self.op_mvn(bus),
            0x44 => self.op_mvp(bus),

            // Misc control
            0x5C => self.op_jml(bus),         // JML $addr
            0xCB => self.op_wai(),            // WAI
            0xDB => self.op_wai(),            // STP (treated as WAI, see field doc)
            0x00 => self.op_brk(bus),         // BRK
            0x02 => self.op_cop(bus),         // COP
            0x42 => self.op_wdm(bus),         // WDM (reserved)
            0xF4 => self.op_pea(bus),         // PEA $addr
            0xD4 => self.op_pei(bus),         // PEI (dp)
            0x62 => self.op_per(bus),         // PER label

            // Stack Relative / Stack Relative Indirect Indexed,Y
            0x03 => self.op_ora_sr(bus),
            0x13 => self.op_ora_sr_indirect_y(bus),
            0x23 => self.op_and_sr(bus),
            0x33 => self.op_and_sr_indirect_y(bus),
            0x43 => self.op_eor_sr(bus),
            0x53 => self.op_eor_sr_indirect_y(bus),
            0x63 => self.op_adc_sr(bus),
            0x73 => self.op_adc_sr_indirect_y(bus),
            0x83 => self.op_sta_sr(bus),
            0x93 => self.op_sta_sr_indirect_y(bus),
            0xA3 => self.op_lda_sr(bus),
            0xB3 => self.op_lda_sr_indirect_y(bus),
            0xC3 => self.op_cmp_sr(bus),
            0xD3 => self.op_cmp_sr_indirect_y(bus),
            0xE3 => self.op_sbc_sr(bus),
            0xF3 => self.op_sbc_sr_indirect_y(bus),

            // No wildcard arm: all 256 opcodes are implemented, and the
            // compiler's exhaustiveness check now guards against any
            // dispatch entry being accidentally removed.
        };
        let cycles = base_cycles? as u32 + self.pending_cycle_adjustment;
        Ok(cycles)
    }

    // ==================== Instruction Implementations ====================

    /// NOP - No Operation (2 cycles)
    fn op_nop(&mut self) -> BusResult<u8> {
        Ok(2)
    }

    /// TAX - Transfer Accumulator to Index X (2 cycles)
    fn op_tax(&mut self) -> BusResult<u8> {
        if self.p.contains(CpuFlags::INDEX_8BIT) {
            // 8-bit index mode: only transfer low byte
            self.x = (self.a & 0xFF) as u16;
            self.update_nz_flags_8(self.x as u8);
        } else {
            // 16-bit index mode: transfer full accumulator
            self.x = self.a;
            self.update_nz_flags_16(self.x);
        }
        Ok(2)
    }

    /// TXA - Transfer Index X to Accumulator (2 cycles)
    fn op_txa(&mut self) -> BusResult<u8> {
        if self.p.contains(CpuFlags::MEMORY_8BIT) {
            // 8-bit memory mode: only transfer low byte, preserving A's high byte
            self.set_a(self.x, false);
            self.update_nz_flags_8(self.a as u8);
        } else {
            // 16-bit memory mode: transfer full X
            self.a = self.x;
            self.update_nz_flags_16(self.a);
        }
        Ok(2)
    }

    /// TAY - Transfer Accumulator to Index Y (2 cycles)
    fn op_tay(&mut self) -> BusResult<u8> {
        if self.p.contains(CpuFlags::INDEX_8BIT) {
            // 8-bit index mode: only transfer low byte
            self.y = (self.a & 0xFF) as u16;
            self.update_nz_flags_8(self.y as u8);
        } else {
            // 16-bit index mode: transfer full accumulator
            self.y = self.a;
            self.update_nz_flags_16(self.y);
        }
        Ok(2)
    }

    /// TYA - Transfer Index Y to Accumulator (2 cycles)
    fn op_tya(&mut self) -> BusResult<u8> {
        if self.p.contains(CpuFlags::MEMORY_8BIT) {
            // 8-bit memory mode: only transfer low byte, preserving A's high byte
            self.set_a(self.y, false);
            self.update_nz_flags_8(self.a as u8);
        } else {
            // 16-bit memory mode: transfer full Y
            self.a = self.y;
            self.update_nz_flags_16(self.a);
        }
        Ok(2)
    }

    /// TSX - Transfer Stack Pointer to Index X (2 cycles)
    fn op_tsx(&mut self) -> BusResult<u8> {
        if self.p.contains(CpuFlags::INDEX_8BIT) {
            // 8-bit index mode: only transfer low byte of SP
            self.x = (self.sp & 0xFF) as u16;
            self.update_nz_flags_8(self.x as u8);
        } else {
            // 16-bit index mode: transfer full SP
            self.x = self.sp;
            self.update_nz_flags_16(self.x);
        }
        Ok(2)
    }

    /// TXY - Transfer X to Y (0x9B, 2 cycles)
    fn op_txy(&mut self) -> BusResult<u8> {
        if self.p.contains(CpuFlags::INDEX_8BIT) {
            self.y = self.x & 0xFF;
            self.update_nz_flags_8(self.y as u8);
        } else {
            self.y = self.x;
            self.update_nz_flags_16(self.y);
        }
        Ok(2)
    }

    /// TYX - Transfer Y to X (0xBB, 2 cycles)
    fn op_tyx(&mut self) -> BusResult<u8> {
        if self.p.contains(CpuFlags::INDEX_8BIT) {
            self.x = self.y & 0xFF;
            self.update_nz_flags_8(self.x as u8);
        } else {
            self.x = self.y;
            self.update_nz_flags_16(self.x);
        }
        Ok(2)
    }

    /// TXS - Transfer Index X to Stack Pointer (2 cycles)
    /// Note: TXS does NOT affect N/Z flags
    fn op_txs(&mut self) -> BusResult<u8> {
        if self.p.contains(CpuFlags::INDEX_8BIT) {
            // 8-bit index mode: only low byte matters
            // In emulation mode, SP high byte stays at 0x01
            if self.e {
                self.sp = 0x0100 | (self.x & 0xFF);
            } else {
                self.sp = self.x & 0xFF;
            }
        } else {
            // 16-bit index mode: transfer full X
            // In emulation mode, still restricted to page 1
            if self.e {
                self.sp = 0x0100 | (self.x & 0xFF);
            } else {
                self.sp = self.x;
            }
        }
        Ok(2)
    }

    /// CLC - Clear Carry Flag (2 cycles)
    fn op_clc(&mut self) -> BusResult<u8> {
        self.p.remove(CpuFlags::CARRY);
        Ok(2)
    }

    /// SEC - Set Carry Flag (2 cycles)
    fn op_sec(&mut self) -> BusResult<u8> {
        self.p.insert(CpuFlags::CARRY);
        Ok(2)
    }

    /// CLD - Clear Decimal Flag (2 cycles)
    fn op_cld(&mut self) -> BusResult<u8> {
        self.p.remove(CpuFlags::DECIMAL);
        Ok(2)
    }

    /// SED - Set Decimal Flag (2 cycles)
    fn op_sed(&mut self) -> BusResult<u8> {
        self.p.insert(CpuFlags::DECIMAL);
        Ok(2)
    }

    /// CLI - Clear Interrupt Disable Flag (2 cycles)
    fn op_cli(&mut self) -> BusResult<u8> {
        self.p.remove(CpuFlags::IRQ_DISABLE);
        Ok(2)
    }

    /// SEI - Set Interrupt Disable Flag (2 cycles)
    fn op_sei(&mut self) -> BusResult<u8> {
        self.p.insert(CpuFlags::IRQ_DISABLE);
        Ok(2)
    }

    /// CLV - Clear Overflow Flag (2 cycles)
    fn op_clv(&mut self) -> BusResult<u8> {
        self.p.remove(CpuFlags::OVERFLOW);
        Ok(2)
    }

    // ==================== Load Instructions ====================

    /// LDA Immediate (0xA9) - Load Accumulator with immediate value
    fn op_lda_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let value = self.addr_immediate(bus, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(value, is_16bit);
        Ok(if is_16bit { 3 } else { 2 })
    }

    /// LDA Absolute (0xAD) - Load Accumulator from absolute address
    fn op_lda_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(value, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    /// LDA Direct Page (0xA5) - Load Accumulator from Direct Page
    fn op_lda_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_ldx_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let value = self.addr_immediate(bus, is_16bit)?;
        self.x = value;
        self.update_nz_flags_mem(value, is_16bit);
        Ok(if is_16bit { 3 } else { 2 })
    }

    /// LDX Absolute (0xAE) - Load X Register from absolute address
    fn op_ldx_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_absolute(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.x = value;
        self.update_nz_flags_mem(value, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    /// LDX Direct Page (0xA6) - Load X Register from Direct Page
    fn op_ldx_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_ldy_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let value = self.addr_immediate(bus, is_16bit)?;
        self.y = value;
        self.update_nz_flags_mem(value, is_16bit);
        Ok(if is_16bit { 3 } else { 2 })
    }

    /// LDY Absolute (0xAC) - Load Y Register from absolute address
    fn op_ldy_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_absolute(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.y = value;
        self.update_nz_flags_mem(value, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    /// LDY Direct Page (0xA4) - Load Y Register from Direct Page
    fn op_ldy_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_sta_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 5 } else { 4 })
    }

    /// STA Direct Page (0x85) - Store Accumulator to Direct Page
    fn op_sta_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        // +1 cycle if D low byte != 0
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 4 + extra } else { 3 + extra })
    }

    /// STZ Direct Page (0x64) - Store Zero to Direct Page
    fn op_stz_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        self.write_memory(bus, addr, 0, is_16bit)?;
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 4 + extra } else { 3 + extra })
    }

    /// Direct Page + index effective-address computation shared by the
    /// dp,X / dp,Y / (dp,X) addressing modes. Reproduces the documented
    /// 65816 emulation-mode quirk (from "Programming the 65816" by Eyes &
    /// Lichty, inherited for 6502 compatibility): when the CPU is in
    /// emulation mode (E=1) AND the low byte of D is zero, the low byte of
    /// (offset + index) wraps within a single 256-byte page instead of
    /// carrying into D's high byte. In every other case (native mode, or
    /// emulation mode with DL != 0) this is a plain 16-bit wrapping add.
    fn dp_indexed_address(&self, offset: u16, index: u16) -> u16 {
        if self.e && (self.d & 0xFF) == 0 {
            self.d | (offset.wrapping_add(index) & 0xFF)
        } else {
            self.d.wrapping_add(offset).wrapping_add(index)
        }
    }

    /// Direct Page Indexed,X: like Direct Page, plus X (bank 0, wraps within 16 bits)
    fn addr_direct_page_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let offset = self.fetch_u8(bus)? as u16;
        let addr = self.dp_indexed_address(offset, self.x);
        Ok(addr as u32)
    }

    /// STZ Direct Page,X (0x74)
    fn op_stz_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        self.write_memory(bus, addr, 0, is_16bit)?;
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 5 + extra } else { 4 + extra })
    }

    /// STA Direct Page,X (0x95)
    fn op_sta_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 5 + extra } else { 4 + extra })
    }

    /// LDA Direct Page,X (0xB5)
    fn op_lda_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 5 + extra } else { 4 + extra })
    }

    /// STZ Absolute (0x9C) - Store Zero to absolute address
    fn op_stz_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        self.write_memory(bus, addr, 0, is_16bit)?;
        Ok(if is_16bit { 5 } else { 4 })
    }

    /// STZ Absolute,X (0x9E)
    fn op_stz_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        self.write_memory(bus, addr, 0, is_16bit)?;
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// STX Absolute (0x8E) - Store X Register to absolute address
    fn op_stx_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_absolute(bus)?;
        self.write_memory(bus, addr, self.x, is_16bit)?;
        Ok(if is_16bit { 5 } else { 4 })
    }

    /// STX Direct Page (0x86) - Store X Register to Direct Page
    fn op_stx_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_direct_page(bus)?;
        self.write_memory(bus, addr, self.x, is_16bit)?;
        // +1 cycle if D low byte != 0
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 4 + extra } else { 3 + extra })
    }

    /// STY Absolute (0x8C) - Store Y Register to absolute address
    fn op_sty_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_absolute(bus)?;
        self.write_memory(bus, addr, self.y, is_16bit)?;
        Ok(if is_16bit { 5 } else { 4 })
    }

    /// STY Direct Page (0x84) - Store Y Register to Direct Page
    fn op_sty_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_direct_page(bus)?;
        self.write_memory(bus, addr, self.y, is_16bit)?;
        // +1 cycle if D low byte != 0
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 4 + extra } else { 3 + extra })
    }

    // ==================== Control Flow & Branching ====================

    fn branch_if(&mut self, condition: bool, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_bcc(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.branch_if(!self.p.contains(CpuFlags::CARRY), bus)
    }

    /// BCS - Branch if Carry Set
    fn op_bcs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.branch_if(self.p.contains(CpuFlags::CARRY), bus)
    }

    /// BNE - Branch if Not Equal (Zero Clear)
    fn op_bne(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.branch_if(!self.p.contains(CpuFlags::ZERO), bus)
    }

    /// BEQ - Branch if Equal (Zero Set)
    fn op_beq(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.branch_if(self.p.contains(CpuFlags::ZERO), bus)
    }

    /// BPL - Branch if Plus (Negative Clear)
    fn op_bpl(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.branch_if(!self.p.contains(CpuFlags::NEGATIVE), bus)
    }

    /// BMI - Branch if Minus (Negative Set)
    fn op_bmi(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.branch_if(self.p.contains(CpuFlags::NEGATIVE), bus)
    }

    /// BVC - Branch if Overflow Clear
    fn op_bvc(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.branch_if(!self.p.contains(CpuFlags::OVERFLOW), bus)
    }

    /// BVS - Branch if Overflow Set
    fn op_bvs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.branch_if(self.p.contains(CpuFlags::OVERFLOW), bus)
    }

    /// BRA - Branch Always
    fn op_bra(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.branch_if(true, bus)
    }

    /// JMP Absolute (0x4C) - Jump to new absolute address
    fn op_jmp_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.pc = self.fetch_u16(bus)?;
        Ok(3)
    }

    // ==================== Arithmetic ====================

    /// ADC Immediate (0x69) - Add with Carry
    fn op_adc_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let operand = self.addr_immediate(bus, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 3 } else { 2 })
    }

    /// SBC Immediate (0xE9) - Subtract with Carry (Borrow)
    fn op_sbc_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn adc_binary(&mut self, operand: u16, is_16bit: bool) {
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
    fn sbc_binary(&mut self, operand: u16, is_16bit: bool) {
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
    fn adc_decimal(&mut self, operand: u16, is_16bit: bool) {
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
    fn sbc_decimal(&mut self, operand: u16, is_16bit: bool) {
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

    /// AND Immediate (0x29) - Logical AND with accumulator
    fn op_and_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_and_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_and_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_ora_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_ora_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_ora_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_eor_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_eor_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_eor_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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

    /// BIT Direct Page (0x24) - Test bits
    fn op_bit_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        
        self.bit_test(operand, is_16bit);
        Ok(if is_16bit { 4 } else { 3 })
    }

    /// BIT Absolute (0x2C)
    fn op_bit_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        
        self.bit_test(operand, is_16bit);
        Ok(if is_16bit { 4 } else { 3 })
    }

    fn bit_test(&mut self, operand: u16, is_16bit: bool) {
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

    /// CMP Immediate (0xC9)
    fn op_cmp_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let operand = self.addr_immediate(bus, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 3 } else { 2 })
    }

    /// CMP Absolute (0xCD)
    fn op_cmp_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 4 } else { 3 })
    }

    /// CMP Direct Page (0xC5)
    fn op_cmp_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 4 } else { 3 })
    }

    /// CPX Immediate (0xE0)
    fn op_cpx_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let operand = self.addr_immediate(bus, is_16bit)?;
        self.compare(self.x, operand, is_16bit);
        Ok(if is_16bit { 3 } else { 2 })
    }

    /// CPX Absolute (0xEC)
    fn op_cpx_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_absolute(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.x, operand, is_16bit);
        Ok(if is_16bit { 4 } else { 3 })
    }

    /// CPX Direct Page (0xE4)
    fn op_cpx_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.x, operand, is_16bit);
        Ok(if is_16bit { 4 } else { 3 })
    }

    /// CPY Immediate (0xC0)
    fn op_cpy_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let operand = self.addr_immediate(bus, is_16bit)?;
        self.compare(self.y, operand, is_16bit);
        Ok(if is_16bit { 3 } else { 2 })
    }

    /// CPY Absolute (0xCC)
    fn op_cpy_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_absolute(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.y, operand, is_16bit);
        Ok(if is_16bit { 4 } else { 3 })
    }

    /// CPY Direct Page (0xC4)
    fn op_cpy_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.y, operand, is_16bit);
        Ok(if is_16bit { 4 } else { 3 })
    }

    fn compare(&mut self, reg: u16, operand: u16, is_16bit: bool) {
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

    /// INX - Increment X Register (2 cycles)
    ///
    /// Unlike A (which keeps a "hidden" high byte across 8-bit operations,
    /// restorable via XBA), X and Y architecturally zero their high byte
    /// on any 8-bit-mode write -- this previously preserved it instead
    /// (`self.x & 0xFF00 | ...`), inconsistent with `LDX`'s already-correct
    /// zero-extending behavior. A real, separate bug from the LDA one;
    /// found by tracing a stack-corruption crash back to a DEX/BPL loop
    /// whose exit condition depended on X's actual 8-bit value.
    fn op_inx(&mut self) -> BusResult<u8> {
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
    fn op_dex(&mut self) -> BusResult<u8> {
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
    fn op_iny(&mut self) -> BusResult<u8> {
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
    fn op_dey(&mut self) -> BusResult<u8> {
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
    fn op_inc_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let result = value.wrapping_add(1);
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 7 } else { 5 })
    }

    /// INC Absolute (0xEE)
    fn op_inc_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let result = value.wrapping_add(1);
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 7 } else { 5 })
    }

    /// DEC Direct Page (0xC6)
    fn op_dec_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let result = value.wrapping_sub(1);
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 7 } else { 5 })
    }

    /// DEC Absolute (0xCE)
    fn op_dec_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let result = value.wrapping_sub(1);
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 7 } else { 5 })
    }

    // ==================== Shift/Rotate ====================

    /// Helper for 8-bit ROL
    fn rol_8(value: u8, carry: bool) -> (u8, bool) {
        let old_bit7 = (value & 0x80) != 0;
        let result = (value << 1) | (if carry { 1 } else { 0 });
        (result, old_bit7)
    }

    /// Helper for 8-bit ROR
    fn ror_8(value: u8, carry: bool) -> (u8, bool) {
        let old_bit0 = (value & 0x01) != 0;
        let result = (value >> 1) | (if carry { 0x80 } else { 0 });
        (result, old_bit0)
    }

    /// ASL Accumulator (0x0A)
    /// INC A (0x1A) - Increment Accumulator
    fn op_inc_acc(&mut self) -> BusResult<u8> {
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
    fn op_dec_acc(&mut self) -> BusResult<u8> {
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

    fn op_asl_acc(&mut self) -> BusResult<u8> {
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
    fn op_lsr_acc(&mut self) -> BusResult<u8> {
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
    fn op_rol_acc(&mut self) -> BusResult<u8> {
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
    fn op_ror_acc(&mut self) -> BusResult<u8> {
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
    fn op_asl_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_asl_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_lsr_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_lsr_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_rol_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_rol_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_ror_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_ror_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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

    /// PHA - Push Accumulator (3 cycles)
    fn op_pha(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        self.push_stack(bus, self.a, is_16bit)?;
        Ok(3)
    }

    /// PLA - Pull Accumulator (4 cycles)
    fn op_pla(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let value = self.pull_stack(bus, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(4)
    }

    /// PHX - Push X Register (3 cycles)
    fn op_phx(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        self.push_stack(bus, self.x, is_16bit)?;
        Ok(3)
    }

    /// PLX - Pull X Register (4 cycles)
    fn op_plx(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        self.x = self.pull_stack(bus, is_16bit)?;
        self.update_nz_flags_mem(self.x, is_16bit);
        Ok(4)
    }

    /// PHY - Push Y Register (3 cycles)
    fn op_phy(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        self.push_stack(bus, self.y, is_16bit)?;
        Ok(3)
    }

    /// PLY - Pull Y Register (4 cycles)
    fn op_ply(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_php(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.push_stack(bus, self.p.bits() as u16, false)?;
        Ok(3)
    }

    /// PLP - Pull Processor Status (4 cycles)
    fn op_plp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let value = self.pull_stack(bus, false)?;
        self.p = CpuFlags::from_bits_truncate(value as u8);
        self.enforce_emulation_mode_register_widths();
        Ok(4)
    }

    /// Real 65816 hardware cannot represent 16-bit A/X/Y while the
    /// emulation-mode flag (E) is set -- E forces M and X to 1 (8-bit)
    /// unconditionally. `op_xce` already enforces this when E transitions
    /// to true, but any opcode that can otherwise rewrite P from an
    /// arbitrary value (REP clearing M/X to request 16-bit, or PLP/RTI
    /// restoring whatever flags were sitting on the stack) must re-apply
    /// the same constraint afterward, or the CPU ends up in a
    /// hardware-impossible state: emulation mode with 16-bit registers.
    /// Mirrors `op_xce`'s enforcement exactly, including truncating X/Y's
    /// high bytes to zero the moment INDEX_8BIT becomes forced on.
    fn enforce_emulation_mode_register_widths(&mut self) {
        if self.e {
            self.p.insert(CpuFlags::MEMORY_8BIT);
            self.p.insert(CpuFlags::INDEX_8BIT);
            self.x &= 0x00FF;
            self.y &= 0x00FF;
        }
    }

    /// TCD - Transfer Accumulator (C) to Direct Page register (2 cycles)
    /// D is always a full 16-bit register regardless of the M flag.
    fn op_tcd(&mut self) -> BusResult<u8> {
        self.d = self.a;
        self.update_nz_flags_16(self.d);
        Ok(2)
    }

    /// TDC - Transfer Direct Page register to Accumulator (C) (2 cycles)
    fn op_tdc(&mut self) -> BusResult<u8> {
        self.a = self.d;
        self.update_nz_flags_16(self.a);
        Ok(2)
    }

    /// TCS - Transfer Accumulator (C) to Stack Pointer (2 cycles)
    /// Does not affect N/Z flags. In emulation mode SP's high byte stays 0x01.
    fn op_tcs(&mut self) -> BusResult<u8> {
        if self.e {
            self.sp = 0x0100 | (self.a & 0xFF);
        } else {
            self.sp = self.a;
        }
        Ok(2)
    }

    /// TSC (0x3B) - Transfer Stack Pointer to Accumulator (C) (2 cycles).
    /// Always transfers the full 16-bit S into the full 16-bit C
    /// regardless of the M flag, and sets N/Z from the 16-bit result
    /// (unlike its mirror `op_tcs`, which touches no flags).
    fn op_tsc(&mut self) -> BusResult<u8> {
        self.a = self.sp;
        self.update_nz_flags_16(self.a);
        Ok(2)
    }

    /// PHB - Push Data Bank Register (3 cycles)
    fn op_phb(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.push_stack(bus, self.db as u16, false)?;
        Ok(3)
    }

    /// PLB - Pull Data Bank Register (4 cycles)
    fn op_plb(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let value = self.pull_stack(bus, false)?;
        self.db = value as u8;
        self.update_nz_flags_8(self.db);
        Ok(4)
    }

    /// PHD - Push Direct Page Register (4 cycles). D is always pushed as a
    /// full 16-bit value regardless of the M flag.
    fn op_phd(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.push_stack(bus, self.d, true)?;
        Ok(4)
    }

    /// PLD - Pull Direct Page Register (5 cycles)
    fn op_pld(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.d = self.pull_stack(bus, true)?;
        self.update_nz_flags_16(self.d);
        Ok(5)
    }

    /// PHK - Push Program Bank Register (3 cycles)
    fn op_phk(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.push_stack(bus, self.pb as u16, false)?;
        Ok(3)
    }

    /// Effective bus address of the current stack pointer location.
    ///
    /// In emulation mode, the 65816 hardware forces the stack into page 1
    /// (the SP high byte is fixed at 0x01) regardless of what's actually in
    /// `self.sp`'s high byte. In native mode, SP is a full 16-bit register
    /// pointing anywhere in bank 0 -- forcing page 1 unconditionally here
    /// (as this code used to) silently corrupted any native-mode stack
    /// usage outside page 1, e.g. SMW's boot code sets SP=$1FFF via TCS.
    fn stack_addr(&self) -> u32 {
        if self.e {
            0x0100 | (self.sp & 0xFF) as u32
        } else {
            self.sp as u32
        }
    }

    /// Pushes `value` onto the stack. The number of bytes pushed is decided
    /// solely by `is_16bit` -- callers already compute this correctly from
    /// the relevant M/X flag (or pass a hardcoded width for registers like
    /// D/PC that are always a fixed size). The previous version also forced
    /// a 2-byte push whenever `self.e` was true, which meant PHA/PHX/PHY/PHP
    /// (genuinely 8-bit operations in emulation mode, the SNES's default
    /// boot state) silently pushed an extra phantom byte and corrupted SP.
    fn push_stack(&mut self, bus: &mut impl MemoryBus, value: u16, is_16bit: bool) -> BusResult<()> {
        #[cfg(feature = "stack_shadow_debug")]
        {
            let full_pc = ((self.pb as u32) << 16) | (self.pc as u32);
            self.shadow_stack.push((full_pc, if is_16bit { 2 } else { 1 }));
        }
        if is_16bit {
            bus.write_u8(self.stack_addr(), (value >> 8) as u8)?;
            self.sp = self.sp.wrapping_sub(1);
        }
        bus.write_u8(self.stack_addr(), (value & 0xFF) as u8)?;
        self.sp = self.sp.wrapping_sub(1);
        Ok(())
    }

    fn pull_stack(&mut self, bus: &mut impl MemoryBus, is_16bit: bool) -> BusResult<u16> {
        #[cfg(feature = "stack_shadow_debug")]
        {
            let full_pc = ((self.pb as u32) << 16) | (self.pc as u32);
            let expected = if is_16bit { 2 } else { 1 };
            match self.shadow_stack.pop() {
                Some((push_pc, push_size)) if push_size != expected => {
                    if self.stack_mismatch.is_none() {
                        self.stack_mismatch = Some(format!(
                            "push at PC={:06X} pushed {} byte(s), but pull at PC={:06X} expects {} byte(s)",
                            push_pc, push_size, full_pc, expected
                        ));
                    }
                }
                None => {
                    if self.stack_mismatch.is_none() {
                        self.stack_mismatch = Some(format!("pull at PC={:06X} ({} bytes) with empty shadow stack", full_pc, expected));
                    }
                }
                _ => {}
            }
        }
        self.sp = self.sp.wrapping_add(1);
        let low = bus.read_u8(self.stack_addr())? as u16;

        if is_16bit {
            self.sp = self.sp.wrapping_add(1);
            let high = bus.read_u8(self.stack_addr())? as u16;
            Ok((high << 8) | low)
        } else {
            Ok(low)
        }
    }

    // ==================== Jump/Call ====================

    /// JSR Absolute (0x20) - Jump to Subroutine
    fn op_jsr_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_jsr_ix(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_jsl(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_rtl(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let addr = self.pull_stack(bus, true)?;
        let pb = self.pull_stack(bus, false)? as u8;
        self.pb = pb;
        self.pc = addr.wrapping_add(1);
        Ok(6)
    }

    /// RTS (0x60) - Return from Subroutine
    fn op_rts(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let addr = self.pull_stack(bus, true)?;
        self.pc = addr.wrapping_add(1);
        Ok(6)
    }

    /// RTI (0x40) - Return from Interrupt
    ///
    /// Emulation mode mirrors the 6502/65C02: the interrupt sequence pushed
    /// only P then PC (3 bytes total, no bank), so RTI pulls just those two.
    /// Native mode's interrupt sequence additionally pushes PB (4 bytes
    /// total: PB, PCH, PCL, P), so RTI must also pull PB back -- skipping
    /// this in native mode left PB stuck at whatever it was after the
    /// pull, silently corrupting the active bank on return from any
    /// interrupt taken while running in native mode (e.g. SMW's NMI
    /// handler, since the boot code switches to native mode via CLC/XCE
    /// before enabling NMI).
    fn op_rti(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        // Pull status register (always a single byte)
        let status = self.pull_stack(bus, false)?;
        self.p = CpuFlags::from_bits_truncate(status as u8);
        self.enforce_emulation_mode_register_widths();

        // Pull PC (always a full 16-bit value)
        self.pc = self.pull_stack(bus, true)?;

        if !self.e {
            self.pb = self.pull_stack(bus, false)? as u8;
            Ok(7)
        } else {
            Ok(6)
        }
    }

    /// JMP Indirect (0x6C) - Jump to address pointed by operand. Real 65816
    /// hardware always fetches this pointer from bank 0, regardless of DB
    /// (a 6502-inherited quirk), same as `op_jml_indirect` below.
    fn op_jmp_ind(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let ptr = self.fetch_u16(bus)?;
        let addr = ptr as u32;
        let target_lo = bus.read_u8(addr)? as u16;
        let target_hi = bus.read_u8(addr.wrapping_add(1))? as u16;
        self.pc = (target_hi << 8) | target_lo;
        Ok(5)
    }

    /// JML [$addr] (0xDC) - Jump absolute indirect long: the 2-byte
    /// operand is a bank-0 pointer to a 3-byte (24-bit) target address.
    fn op_jml_indirect(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_jmp_ix(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let ptr = self.fetch_u16(bus)?;
        let ptr = ptr.wrapping_add(self.x);
        let addr = ((self.pb as u32) << 16) | (ptr as u32);
        let target_lo = bus.read_u8(addr)? as u16;
        let target_hi = bus.read_u8(addr.wrapping_add(1))? as u16;
        self.pc = (target_hi << 8) | target_lo;
        Ok(6)
    }

    /// BRL (0x82) - Branch Always Long
    fn op_brl(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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

    /// REP (0xC2) - Reset Processor Status Bits. Note that in emulation
    /// mode this cannot actually widen M/X to 16-bit even if the operand
    /// asks for it -- see `enforce_emulation_mode_register_widths`.
    fn op_rep(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_sep(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_xce(&mut self) -> BusResult<u8> {
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

    /// XBA - Exchange the two bytes of the Accumulator (3 cycles).
    /// Always operates on the full 16-bit C register regardless of the M
    /// flag; N/Z are set from the new low byte only.
    fn op_xba(&mut self) -> BusResult<u8> {
        self.a = self.a.rotate_left(8);
        self.update_nz_flags_8((self.a & 0xFF) as u8);
        Ok(3)
    }

    // ==================== Addressing Modes ====================

    /// Immediate: lee operando del stream de instrucciones
    fn addr_immediate(&mut self, bus: &mut impl MemoryBus, is_16bit: bool) -> BusResult<u16> {
        if is_16bit {
            self.fetch_u16(bus)
        } else {
            self.fetch_u8(bus).map(|b| b as u16)
        }
    }

    /// Absolute: lee dirección de 16 bits del stream y combina con DB
    fn addr_absolute(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let addr = self.fetch_u16(bus)?;
        Ok(((self.db as u32) << 16) | (addr as u32))
    }

    /// Direct Page: suma D al operando de 8 bits (siempre en banco 0)
    fn addr_direct_page(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let offset = self.fetch_u8(bus)? as u16;
        let addr = self.d.wrapping_add(offset);
        Ok(addr as u32)
    }

    /// Absolute Long: 3-byte little-endian operand, explicit bank (ignores DB)
    fn addr_absolute_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let lo = self.fetch_u8(bus)? as u32;
        let mid = self.fetch_u8(bus)? as u32;
        let hi = self.fetch_u8(bus)? as u32;
        Ok((hi << 16) | (mid << 8) | lo)
    }

    /// STA Absolute Long (0x8F) - Store Accumulator to a 24-bit address
    fn op_sta_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// LDA Absolute Long (0xAF) - Load Accumulator from a 24-bit address
    fn op_lda_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// Absolute Long Indexed,X: 24-bit base + X, wrapping within 24 bits
    /// (unlike plain Absolute,X, the carry is allowed to cross banks).
    fn addr_absolute_long_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let base = self.addr_absolute_long(bus)?;
        Ok(base.wrapping_add(self.x as u32) & 0xFF_FFFF)
    }

    /// STA Absolute Long Indexed,X (0x9F)
    fn op_sta_long_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long_x(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// Direct Page Indirect Long: reads a 24-bit pointer (low, high, bank)
    /// stored at the direct-page address, used as-is.
    fn addr_indirect_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let dp_addr = self.addr_direct_page(bus)?;
        let lo = bus.read_u8(dp_addr)? as u32;
        let mid = bus.read_u8(dp_addr.wrapping_add(1))? as u32;
        let hi = bus.read_u8(dp_addr.wrapping_add(2))? as u32;
        Ok((hi << 16) | (mid << 8) | lo)
    }

    /// Direct Page Indirect Indexed,Y -- "(dp),Y": a 16-bit pointer stored
    /// at the direct-page address, combined with the Data Bank register
    /// (NOT an explicit bank byte, unlike the "[dp],Y" long form), then
    /// indexed by Y. The Y addition wraps within 16 bits without carrying
    /// into the next bank (same convention as plain Absolute,X/Y).
    fn addr_indirect_dp_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let dp_addr = self.addr_direct_page(bus)?;
        let lo = bus.read_u8(dp_addr)? as u16;
        let hi = bus.read_u8(dp_addr.wrapping_add(1))? as u16;
        let pointer = ((hi << 8) | lo).wrapping_add(self.y);
        Ok(((self.db as u32) << 16) | (pointer as u32))
    }

    /// Direct Page Indirect -- "(dp)": a 16-bit pointer stored at the
    /// direct-page address, combined with the Data Bank register, with no
    /// index applied.
    fn addr_indirect_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let dp_addr = self.addr_direct_page(bus)?;
        let lo = bus.read_u8(dp_addr)? as u16;
        let hi = bus.read_u8(dp_addr.wrapping_add(1))? as u16;
        let pointer = (hi << 8) | lo;
        Ok(((self.db as u32) << 16) | (pointer as u32))
    }

    /// LDA (dp) (0xB2)
    fn op_lda_indirect_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// STA (dp) (0x92)
    fn op_sta_indirect_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// LDA (dp),Y (0xB1)
    fn op_lda_indirect_dp_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_y(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// STA (dp),Y (0x91)
    fn op_sta_indirect_dp_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_y(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// Direct Page Indexed Indirect,X -- "(dp,X)": add X to the direct
    /// page address *before* dereferencing the 16-bit pointer, then
    /// combine with the Data Bank register.
    fn addr_indirect_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let offset = self.fetch_u8(bus)? as u16;
        let dp_addr = self.dp_indexed_address(offset, self.x) as u32;
        let lo = bus.read_u8(dp_addr)? as u16;
        let hi = bus.read_u8(dp_addr.wrapping_add(1))? as u16;
        let pointer = (hi << 8) | lo;
        Ok(((self.db as u32) << 16) | (pointer as u32))
    }

    /// LDA (dp,X) (0xA1)
    fn op_lda_indirect_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// STA (dp,X) (0x81)
    fn op_sta_indirect_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_x(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// Direct Page Indirect Long Indexed,Y: same 24-bit pointer, plus Y,
    /// wrapping within 24 bits (the carry may cross banks, like Absolute
    /// Long Indexed,X).
    fn addr_indirect_long_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let base = self.addr_indirect_long(bus)?;
        Ok(base.wrapping_add(self.y as u32) & 0xFF_FFFF)
    }

    /// Stack Relative: an 8-bit offset added to SP, always within bank 0
    /// (the stack never leaves bank 0 on the 65816).
    fn addr_stack_relative(&mut self, bus: &mut impl MemoryBus) -> BusResult<u16> {
        let offset = self.fetch_u8(bus)? as u16;
        Ok(self.sp.wrapping_add(offset))
    }

    /// Stack Relative Indirect Indexed,Y -- "(sr,S),Y": a 16-bit pointer
    /// stored at the stack-relative address (bank 0), combined with the
    /// Data Bank register and indexed by Y (wraps within 16 bits, no
    /// carry into DB, same convention as plain Absolute,X/Y).
    fn addr_stack_relative_indirect_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let sr_addr = self.addr_stack_relative(bus)?;
        let lo = bus.read_u8(sr_addr as u32)? as u16;
        let hi = bus.read_u8(sr_addr.wrapping_add(1) as u32)? as u16;
        let pointer = ((hi << 8) | lo).wrapping_add(self.y);
        Ok(((self.db as u32) << 16) | (pointer as u32))
    }

    /// ORA/AND/EOR/ADC/CMP/SBC/LDA sr,S and (sr,S),Y -- stack-relative
    /// addressing, rare enough to have been deprioritized initially but
    /// confirmed needed once real SMW execution reached bank $A1 (verified
    /// against wiki.superfamicom.org/65816-reference, same as every other
    /// addressing-mode family above).
    fn op_ora_sr(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative(bus)? as u32;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.ora_into_a(operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }
    fn op_ora_sr_indirect_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative_indirect_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.ora_into_a(operand, is_16bit);
        Ok(if is_16bit { 8 } else { 7 })
    }
    fn op_and_sr(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative(bus)? as u32;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.and_into_a(operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }
    fn op_and_sr_indirect_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative_indirect_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.and_into_a(operand, is_16bit);
        Ok(if is_16bit { 8 } else { 7 })
    }
    fn op_eor_sr(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative(bus)? as u32;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.eor_into_a(operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }
    fn op_eor_sr_indirect_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative_indirect_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.eor_into_a(operand, is_16bit);
        Ok(if is_16bit { 8 } else { 7 })
    }
    fn op_adc_sr(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative(bus)? as u32;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }
    fn op_adc_sr_indirect_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative_indirect_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 8 } else { 7 })
    }
    fn op_sbc_sr(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative(bus)? as u32;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }
    fn op_sbc_sr_indirect_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative_indirect_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 8 } else { 7 })
    }
    fn op_cmp_sr(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative(bus)? as u32;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }
    fn op_cmp_sr_indirect_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative_indirect_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 8 } else { 7 })
    }
    fn op_lda_sr(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative(bus)? as u32;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }
    fn op_lda_sr_indirect_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative_indirect_y(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(if is_16bit { 8 } else { 7 })
    }
    fn op_sta_sr(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative(bus)? as u32;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 5 } else { 4 })
    }
    fn op_sta_sr_indirect_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_stack_relative_indirect_y(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 8 } else { 7 })
    }

    /// LDA [$dp] (0xA7)
    fn op_lda_indirect_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// STA [$dp] (0x87)
    fn op_sta_indirect_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// ORA [$dp] (0x07)
    fn op_ora_indirect_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_and_indirect_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_eor_indirect_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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

    /// CMP [$dp] (0xC7)
    fn op_cmp_indirect_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// ADC [$dp] (0x67)
    fn op_adc_indirect_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// SBC [$dp] (0xE7)
    fn op_sbc_indirect_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    // ALU family, Absolute,X and Absolute,Y addressing -- opcode values
    // verified against wiki.superfamicom.org/65816-reference.
    fn op_ora_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a |= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) | (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 6 } else { 5 })
    }
    fn op_ora_abs_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a |= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) | (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 6 } else { 5 })
    }
    fn op_and_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a &= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | ((self.a as u8) & (operand as u8)) as u16; self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 6 } else { 5 })
    }
    fn op_and_abs_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a &= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | ((self.a as u8) & (operand as u8)) as u16; self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 6 } else { 5 })
    }
    fn op_eor_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a ^= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) ^ (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 6 } else { 5 })
    }
    fn op_eor_abs_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a ^= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) ^ (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 6 } else { 5 })
    }
    fn op_adc_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }
    fn op_adc_abs_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }
    fn op_sbc_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }
    fn op_sbc_abs_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }
    fn op_cmp_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }
    fn op_cmp_abs_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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

    fn ora_into_a(&mut self, operand: u16, is_16bit: bool) {
        if is_16bit { self.a |= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) | (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
    }
    fn and_into_a(&mut self, operand: u16, is_16bit: bool) {
        if is_16bit { self.a &= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | ((self.a as u8) & (operand as u8)) as u16; self.update_nz_flags_8(self.a as u8); }
    }
    fn eor_into_a(&mut self, operand: u16, is_16bit: bool) {
        if is_16bit { self.a ^= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) ^ (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
    }

    /// ORA $addr (long) (0x0F)
    fn op_ora_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.ora_into_a(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }
    /// AND $addr (long) (0x2F)
    fn op_and_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.and_into_a(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }
    /// EOR $addr (long) (0x4F)
    fn op_eor_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.eor_into_a(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }
    /// ADC $addr (long) (0x6F)
    fn op_adc_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }
    /// CMP $addr (long) (0xCF)
    fn op_cmp_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }
    /// SBC $addr (long) (0xEF)
    fn op_sbc_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// LDA $addr,X (long) (0xBF)
    fn op_lda_long_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }
    /// ORA $addr,X (long) (0x1F)
    fn op_ora_long_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.ora_into_a(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }
    /// AND $addr,X (long) (0x3F)
    fn op_and_long_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.and_into_a(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }
    /// EOR $addr,X (long) (0x5F)
    fn op_eor_long_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.eor_into_a(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }
    /// ADC $addr,X (long) (0x7F)
    fn op_adc_long_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }
    /// CMP $addr,X (long) (0xDF)
    fn op_cmp_long_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }
    /// SBC $addr,X (long) (0xFF)
    fn op_sbc_long_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_long_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    // ==================== ALU family: [$dp],Y (indirect long indexed) ====================

    /// ORA [$dp],Y (0x17)
    fn op_ora_indirect_long_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.ora_into_a(operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }
    /// AND [$dp],Y (0x37)
    fn op_and_indirect_long_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.and_into_a(operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }
    /// EOR [$dp],Y (0x57)
    fn op_eor_indirect_long_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.eor_into_a(operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }
    /// ADC [$dp],Y (0x77)
    fn op_adc_indirect_long_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }
    /// CMP [$dp],Y (0xD7)
    fn op_cmp_indirect_long_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }
    /// SBC [$dp],Y (0xF7)
    fn op_sbc_indirect_long_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    // ==================== LDX/LDY, remaining indexed Direct Page forms ====================

    /// LDY Direct Page,X (0xB4)
    fn op_ldy_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.y = value;
        self.update_nz_flags_mem(value, is_16bit);
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 5 + extra } else { 4 + extra })
    }
    /// LDX Direct Page,Y (0xB6)
    fn op_ldx_dp_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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

    fn asl_compute(value: u16, is_16bit: bool) -> (u16, bool) {
        if is_16bit {
            (value << 1, (value & 0x8000) != 0)
        } else {
            let v = value as u8;
            (((v << 1) as u16), (v & 0x80) != 0)
        }
    }
    fn lsr_compute(value: u16, is_16bit: bool) -> (u16, bool) {
        if is_16bit {
            (value >> 1, (value & 0x0001) != 0)
        } else {
            let v = value as u8;
            ((v >> 1) as u16, (v & 0x01) != 0)
        }
    }
    fn rol_compute(value: u16, is_16bit: bool, carry_in: bool) -> (u16, bool) {
        if is_16bit {
            let carry_out = (value & 0x8000) != 0;
            ((value << 1) | (if carry_in { 1 } else { 0 }), carry_out)
        } else {
            let (result, carry_out) = Self::rol_8(value as u8, carry_in);
            (result as u16, carry_out)
        }
    }
    fn ror_compute(value: u16, is_16bit: bool, carry_in: bool) -> (u16, bool) {
        if is_16bit {
            let carry_out = (value & 0x0001) != 0;
            ((value >> 1) | (if carry_in { 0x8000 } else { 0 }), carry_out)
        } else {
            let (result, carry_out) = Self::ror_8(value as u8, carry_in);
            (result as u16, carry_out)
        }
    }

    /// ASL Direct Page,X (0x16)
    fn op_asl_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_asl_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_lsr_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_lsr_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_rol_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_rol_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_ror_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_ror_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    /// DEC Direct Page,X (0xD6)
    fn op_dec_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let result = value.wrapping_sub(1);
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 8 } else { 6 })
    }
    /// DEC Absolute,X (0xDE)
    fn op_dec_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let result = value.wrapping_sub(1);
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 9 } else { 7 })
    }
    /// INC Direct Page,X (0xF6)
    fn op_inc_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let result = value.wrapping_add(1);
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 8 } else { 6 })
    }
    /// INC Absolute,X (0xFE)
    fn op_inc_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        let result = value.wrapping_add(1);
        self.write_memory(bus, addr, result, is_16bit)?;
        self.update_nz_flags_mem(result, is_16bit);
        Ok(if is_16bit { 9 } else { 7 })
    }

    // ==================== TSB/TRB and remaining BIT forms ====================

    /// TSB Direct Page (0x04) - Z reflects (mem & A); mem is then OR'd with A.
    fn op_tsb_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_tsb_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_trb_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_trb_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_bit_imm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let operand = self.addr_immediate(bus, is_16bit)?;
        let test = if is_16bit { self.a & operand } else { (self.a & 0xFF) & (operand & 0xFF) };
        if test == 0 { self.p.insert(CpuFlags::ZERO); } else { self.p.remove(CpuFlags::ZERO); }
        Ok(if is_16bit { 3 } else { 2 })
    }
    /// BIT Direct Page,X (0x34)
    fn op_bit_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.bit_test(operand, is_16bit);
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 5 + extra } else { 4 + extra })
    }
    /// BIT Absolute,X (0x3C)
    fn op_bit_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.bit_test(operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    // ==================== Block Move ====================

    /// MVN $src,$dest (0x54) - Move Memory Negative (incrementing
    /// addresses). Per spec, MVN/MVP always operate on the full 16-bit
    /// A/X/Y regardless of the M/X flags. Real hardware re-executes this
    /// opcode one byte at a time (so a large transfer can be interrupted
    /// mid-copy by NMI/IRQ); this performs the whole transfer in one step
    /// instead, which leaves A/X/Y/DB in exactly the architecturally
    /// specified end state -- the only difference being the move is atomic
    /// here rather than interruptible mid-transfer.
    ///
    /// Real hardware spends 7 cycles per byte moved, which for a large
    /// transfer (up to 65536 bytes) can total up to 458,752 cycles -- far
    /// more than fits in the `u8` this function still returns for its
    /// direct-call-site compatibility with every other opcode handler. The
    /// true total is instead stashed in `self.pending_cycle_adjustment`,
    /// which `execute()` folds into its widened `u32` result immediately
    /// after this call returns.
    fn op_mvn(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        // Operand order per the 65816 spec (Eyes & Lichty, "Block Move
        // Instructions"): the byte after the opcode is the DESTINATION
        // bank, the next one is the SOURCE bank -- the reverse of the
        // assembler mnemonic's `MVN src,dst` operand order. These used to
        // be read swapped, which made every cross-bank block move copy
        // from the wrong bank into the wrong bank (e.g. SMW's overworld
        // loader `MVN $7E,$0C` -- ROM tile data into WRAM -- instead read
        // WRAM garbage and wrote it into read-only ROM, leaving the
        // overworld's Map16 buffer holding the previous level's tiles).
        let dest_bank = self.fetch_u8(bus)?;
        let src_bank = self.fetch_u8(bus)?;
        let initial_count = (self.a as u32).wrapping_add(1);
        let mut count = initial_count;
        while count > 0 {
            let byte = bus.read_u8(((src_bank as u32) << 16) | (self.x as u32))?;
            bus.write_u8(((dest_bank as u32) << 16) | (self.y as u32), byte)?;
            self.x = self.x.wrapping_add(1);
            self.y = self.y.wrapping_add(1);
            count -= 1;
        }
        self.a = 0xFFFF;
        self.db = dest_bank;
        self.pending_cycle_adjustment = initial_count * 7;
        Ok(0)
    }
    /// MVP $src,$dest (0x44) - Move Memory Positive (decrementing
    /// addresses), used for overlapping copies where the destination is
    /// above the source. See `op_mvn` for the atomic-transfer rationale
    /// and cycle-accounting note.
    fn op_mvp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        // Destination bank first, then source bank -- see `op_mvn`.
        let dest_bank = self.fetch_u8(bus)?;
        let src_bank = self.fetch_u8(bus)?;
        let initial_count = (self.a as u32).wrapping_add(1);
        let mut count = initial_count;
        while count > 0 {
            let byte = bus.read_u8(((src_bank as u32) << 16) | (self.x as u32))?;
            bus.write_u8(((dest_bank as u32) << 16) | (self.y as u32), byte)?;
            self.x = self.x.wrapping_sub(1);
            self.y = self.y.wrapping_sub(1);
            count -= 1;
        }
        self.a = 0xFFFF;
        self.db = dest_bank;
        self.pending_cycle_adjustment = initial_count * 7;
        Ok(0)
    }

    // ==================== Misc control ====================

    /// JML $addr (0x5C) - Jump (long) to a 24-bit absolute address.
    fn op_jml(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let lo = self.fetch_u8(bus)? as u16;
        let mid = self.fetch_u8(bus)? as u16;
        let bank = self.fetch_u8(bus)?;
        self.pc = (mid << 8) | lo;
        self.pb = bank;
        Ok(4)
    }

    /// WAI (0xCB) - Wait for Interrupt: suspends fetch until `nmi()` wakes
    /// the CPU. STP (0xDB) is treated identically -- see the field comment
    /// on `waiting_for_interrupt`.
    fn op_wai(&mut self) -> BusResult<u8> {
        self.waiting_for_interrupt = true;
        Ok(3)
    }

    /// BRK (0x00) - Software interrupt. Pushes the same return-context
    /// frame as `nmi()` (see its comment for the native/emulation
    /// push-count distinction) and jumps to the BRK/IRQ vector. The byte
    /// immediately after the opcode is a signature byte real hardware
    /// fetches but ignores -- consumed here only so PC lands correctly.
    fn op_brk(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.fetch_u8(bus)?;
        if !self.e {
            self.push_stack(bus, self.pb as u16, false)?;
        }
        self.push_stack(bus, self.pc, true)?;
        self.push_stack(bus, self.p.bits() as u16, false)?;
        self.p.remove(CpuFlags::DECIMAL);
        self.p.insert(CpuFlags::IRQ_DISABLE);
        let vector = if self.e { 0xFFFE_u32 } else { 0xFFE6_u32 };
        let lo = bus.read_u8(vector)? as u16;
        let hi = bus.read_u8(vector.wrapping_add(1))? as u16;
        self.pc = (hi << 8) | lo;
        self.pb = 0;
        Ok(7)
    }

    /// COP (0x02) - Coprocessor software interrupt. Same push frame and
    /// flag updates as `op_brk` (see its comment for the native/emulation
    /// push-count distinction) -- the only difference is COP dispatches
    /// through its own vector pair instead of BRK/IRQ's, since real
    /// hardware gives COP a distinct entry point so a coprocessor trap
    /// handler doesn't collide with the BRK/IRQ handler. Vectors are
    /// $00FFE4 (native) / $00FFF4 (emulation), one step below the
    /// NMI ($FFEA/$FFFA) and IRQ/BRK ($FFEE/$FFFE) pairs already used by
    /// `nmi()`/`irq()` in this file. Like BRK, the byte immediately after
    /// the opcode is a signature byte real hardware fetches but ignores --
    /// consumed here only so PC lands correctly.
    fn op_cop(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.fetch_u8(bus)?;
        if !self.e {
            self.push_stack(bus, self.pb as u16, false)?;
        }
        self.push_stack(bus, self.pc, true)?;
        self.push_stack(bus, self.p.bits() as u16, false)?;
        self.p.remove(CpuFlags::DECIMAL);
        self.p.insert(CpuFlags::IRQ_DISABLE);
        let vector = if self.e { 0xFFF4_u32 } else { 0xFFE4_u32 };
        let lo = bus.read_u8(vector)? as u16;
        let hi = bus.read_u8(vector.wrapping_add(1))? as u16;
        self.pc = (hi << 8) | lo;
        self.pb = 0;
        Ok(7)
    }

    /// WDM (0x42) - Reserved/undefined opcode. Real silicon fetches and
    /// discards one operand byte and otherwise behaves as a 2-cycle NOP.
    fn op_wdm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.fetch_u8(bus)?;
        Ok(2)
    }

    /// PEA $addr (0xF4) - Push Effective Absolute: pushes a 16-bit
    /// immediate operand, always as 2 bytes regardless of the M flag.
    fn op_pea(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let value = self.fetch_u16(bus)?;
        self.push_stack(bus, value, true)?;
        Ok(5)
    }
    /// PEI (dp) (0xD4) - Push Effective Indirect: pushes the 16-bit
    /// pointer stored at the direct-page address (bank 0, not DB-relative).
    fn op_pei(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let dp_addr = self.addr_direct_page(bus)?;
        let lo = bus.read_u8(dp_addr)? as u16;
        let hi = bus.read_u8(dp_addr.wrapping_add(1))? as u16;
        let value = (hi << 8) | lo;
        self.push_stack(bus, value, true)?;
        Ok(6)
    }
    /// PER label (0x62) - Push Effective Relative: pushes (PC after this
    /// instruction + signed 16-bit offset).
    fn op_per(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let offset = self.fetch_u16(bus)?;
        let value = self.pc.wrapping_add(offset);
        self.push_stack(bus, value, true)?;
        Ok(6)
    }

    // ADC/SBC, remaining addressing modes (dp, abs, dp+X, (dp,X), (dp),Y) --
    // only the immediate form existed before. Opcode values verified
    // against wiki.superfamicom.org/65816-reference.
    fn op_adc_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 4 + extra } else { 3 + extra })
    }
    fn op_adc_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }
    fn op_adc_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }
    fn op_adc_indirect_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }
    fn op_adc_indirect_dp_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }
    fn op_adc_indirect_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.adc_binary(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    fn op_sbc_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 4 + extra } else { 3 + extra })
    }
    fn op_sbc_abs(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }
    fn op_sbc_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }
    fn op_sbc_indirect_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }
    fn op_sbc_indirect_dp_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }
    fn op_sbc_indirect_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.sbc_binary(operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    // ORA/AND/EOR/CMP, remaining addressing modes (dp+X, (dp,X), (dp),Y,
    // (dp)) -- column pattern cross-checked against the already-verified
    // LDA/STA/ADC/SBC instances at the same column offsets (x1=(dp,X),
    // x5=dp+X, x11=(dp),Y, x2=(dp)).
    fn op_ora_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a |= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) | (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 5 } else { 4 })
    }
    fn op_ora_indirect_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a |= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) | (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 7 } else { 6 })
    }
    fn op_ora_indirect_dp_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a |= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) | (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 7 } else { 6 })
    }
    fn op_ora_indirect_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a |= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) | (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 6 } else { 5 })
    }

    fn op_and_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a &= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | ((self.a as u8) & (operand as u8)) as u16; self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 5 } else { 4 })
    }
    fn op_and_indirect_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a &= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | ((self.a as u8) & (operand as u8)) as u16; self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 7 } else { 6 })
    }
    fn op_and_indirect_dp_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a &= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | ((self.a as u8) & (operand as u8)) as u16; self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 7 } else { 6 })
    }
    fn op_and_indirect_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a &= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | ((self.a as u8) & (operand as u8)) as u16; self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 6 } else { 5 })
    }

    fn op_eor_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a ^= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) ^ (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 5 } else { 4 })
    }
    fn op_eor_indirect_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a ^= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) ^ (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 7 } else { 6 })
    }
    fn op_eor_indirect_dp_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a ^= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) ^ (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 7 } else { 6 })
    }
    fn op_eor_indirect_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        if is_16bit { self.a ^= operand; self.update_nz_flags_16(self.a); }
        else { self.a = (self.a & 0xFF00) | (((self.a as u8) ^ (operand as u8)) as u16); self.update_nz_flags_8(self.a as u8); }
        Ok(if is_16bit { 6 } else { 5 })
    }

    fn op_cmp_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }
    fn op_cmp_indirect_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_x(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }
    fn op_cmp_indirect_dp_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp_y(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }
    fn op_cmp_indirect_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_dp(bus)?;
        let operand = self.read_memory(bus, addr, is_16bit)?;
        self.compare(self.a, operand, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// STA Absolute,Y (0x99)
    fn op_sta_abs_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_y(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// Direct Page Indexed,Y: like Direct Page, plus Y (used by STX/LDX dp,Y)
    fn addr_direct_page_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let offset = self.fetch_u8(bus)? as u16;
        let addr = self.dp_indexed_address(offset, self.y);
        Ok(addr as u32)
    }

    /// STX Direct Page,Y (0x96)
    fn op_stx_dp_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_direct_page_y(bus)?;
        self.write_memory(bus, addr, self.x, is_16bit)?;
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 5 + extra } else { 4 + extra })
    }

    /// STY Direct Page,X (0x94)
    fn op_sty_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_direct_page_x(bus)?;
        self.write_memory(bus, addr, self.y, is_16bit)?;
        let extra = if (self.d & 0xFF) != 0 { 1 } else { 0 };
        Ok(if is_16bit { 5 + extra } else { 4 + extra })
    }

    /// LDA [$dp],Y (0xB7)
    fn op_lda_indirect_long_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long_y(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// STA [$dp],Y (0x97)
    fn op_sta_indirect_long_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_indirect_long_y(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 7 } else { 6 })
    }

    /// Absolute Indexed,X: DBR:addr + X computed as a full 24-bit addition --
    /// a carry out of the 16-bit offset propagates into the bank byte
    /// (DBR effectively becomes DBR+1 for that access, wrapping $FF to $00).
    fn addr_absolute_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let addr = self.fetch_u16(bus)?;
        let base = ((self.db as u32) << 16) | (addr as u32);
        Ok(base.wrapping_add(self.x as u32) & 0xFF_FFFF)
    }

    /// Absolute Indexed,Y: DBR:addr + Y computed as a full 24-bit addition --
    /// a carry out of the 16-bit offset propagates into the bank byte
    /// (DBR effectively becomes DBR+1 for that access, wrapping $FF to $00).
    fn addr_absolute_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let addr = self.fetch_u16(bus)?;
        let base = ((self.db as u32) << 16) | (addr as u32);
        Ok(base.wrapping_add(self.y as u32) & 0xFF_FFFF)
    }

    /// LDA Absolute,Y (0xB9) -- NOT to be confused with "LDA (dp),Y" (the
    /// real opcode for that is 0xB1); an earlier version of this code
    /// wrongly assumed 0xB9 meant "(dp),Y" by mistaken symmetry with 0x91
    /// (STA (dp),Y), which silently consumed the wrong number of operand
    /// bytes (1 instead of 2) for every real "LDA addr,Y" in the ROM,
    /// desyncing instruction-boundary decoding from that point on -- the
    /// root cause of a stack-corruption crash traced through ~560,000
    /// instructions of real SMW execution.
    fn op_lda_abs_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_y(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.set_a(value, is_16bit);
        self.update_nz_flags_mem(self.a, is_16bit);
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// STA Absolute,X (0x9D)
    fn op_sta_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::MEMORY_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        self.write_memory(bus, addr, self.a, is_16bit)?;
        Ok(if is_16bit { 6 } else { 5 })
    }

    /// LDA Absolute,X (0xBD)
    fn op_lda_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
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
    fn op_ldx_abs_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_absolute_y(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.x = value;
        self.update_nz_flags_mem(value, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    /// LDY Absolute,X (0xBC) - Load Y Register from absolute address + X.
    fn op_ldy_abs_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let is_16bit = !self.p.contains(CpuFlags::INDEX_8BIT);
        let addr = self.addr_absolute_x(bus)?;
        let value = self.read_memory(bus, addr, is_16bit)?;
        self.y = value;
        self.update_nz_flags_mem(value, is_16bit);
        Ok(if is_16bit { 5 } else { 4 })
    }

    // ==================== Memory Access Helpers ====================

    /// Lee memoria según tamaño (8 o 16 bits)
    fn read_memory(&mut self, bus: &mut impl MemoryBus, addr: u32, is_16bit: bool) -> BusResult<u16> {
        if is_16bit {
            let lo = bus.read_u8(addr)? as u16;
            let hi = bus.read_u8(addr.wrapping_add(1))? as u16;
            Ok((hi << 8) | lo)
        } else {
            Ok(bus.read_u8(addr)? as u16)
        }
    }

    /// Escribe memoria según tamaño (8 o 16 bits)
    fn write_memory(&mut self, bus: &mut impl MemoryBus, addr: u32, value: u16, is_16bit: bool) -> BusResult<()> {
        bus.write_u8(addr, (value & 0xFF) as u8)?;
        if is_16bit {
            bus.write_u8(addr.wrapping_add(1), (value >> 8) as u8)?;
        }
        Ok(())
    }

    /// Sets the accumulator from a loaded `value`. In 16-bit mode this
    /// replaces all of A. In 8-bit mode, only the low byte is the
    /// architectural accumulator -- the high byte is a "hidden" register
    /// (exposed via XBA) that 8-bit loads must NOT clobber. Several load
    /// opcodes (LDA in every addressing mode, TXA, TYA, PLA) used to do
    /// `self.a = value` unconditionally, zeroing the high byte even in
    /// 8-bit mode; real code that stages a byte in the high half via XBA
    /// before an 8-bit LDA/PLA/TXA/TYA (a common and legitimate pattern,
    /// e.g. Super Mario World's own SPC700 upload routine) would have that
    /// byte silently destroyed.
    fn set_a(&mut self, value: u16, is_16bit: bool) {
        if is_16bit {
            self.a = value;
        } else {
            self.a = (self.a & 0xFF00) | (value & 0xFF);
        }
    }

    /// Update N/Z flags based on memory size (uses value for flags, not register)
    fn update_nz_flags_mem(&mut self, value: u16, is_16bit: bool) {
        if is_16bit {
            self.update_nz_flags_16(value);
        } else {
            self.update_nz_flags_8(value as u8);
        }
    }

    // ==================== Flag Helpers ====================

    /// Update N/Z flags based on a 16-bit value (for 16-bit register operations)
    pub fn update_nz_flags_16(&mut self, value: u16) {
        if value == 0 {
            self.p.insert(CpuFlags::ZERO);
        } else {
            self.p.remove(CpuFlags::ZERO);
        }
        if (value & 0x8000) != 0 {
            self.p.insert(CpuFlags::NEGATIVE);
        } else {
            self.p.remove(CpuFlags::NEGATIVE);
        }
    }

    /// Update N/Z flags based on an 8-bit value (for 8-bit operations)
    pub fn update_nz_flags_8(&mut self, value: u8) {
        if value == 0 {
            self.p.insert(CpuFlags::ZERO);
        } else {
            self.p.remove(CpuFlags::ZERO);
        }
        if (value & 0x80) != 0 {
            self.p.insert(CpuFlags::NEGATIVE);
        } else {
            self.p.remove(CpuFlags::NEGATIVE);
        }
    }
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wram::Wram;

    /// Test-only double covering the FULL bank-0 address space (unlike
    /// `Wram`, which only mirrors bank 0's low 8KB Direct Page range and
    /// rejects everything else in $2000-$FFFF). Needed for tests that
    /// exercise hardware interrupt vector fetches ($00FFE4 etc.), which on
    /// real hardware live in cartridge ROM, not WRAM -- `Wram` alone can't
    /// serve those addresses. Bank $7E/$7F (real WRAM) and bank 0's
    /// existing Direct Page mirror still delegate to a real `Wram`, so
    /// stack/DP behavior stays identical to every other CPU test; only
    /// the otherwise-unmapped $00:2000-$00:FFFF range gets a backing
    /// array here, purely so vector bytes can be placed there.
    struct VectorTestBus {
        wram: Wram,
        bank0_high: Box<[u8; 0x10000]>,
    }

    impl VectorTestBus {
        fn new() -> Self {
            Self {
                wram: Wram::new(),
                bank0_high: vec![0u8; 0x10000].into_boxed_slice().try_into().unwrap(),
            }
        }
    }

    impl MemoryBus for VectorTestBus {
        fn read_u8(&mut self, addr: u32) -> BusResult<u8> {
            if addr < 0x2000 || (0x7E0000..0x800000).contains(&addr) {
                self.wram.read_u8(addr)
            } else if addr < 0x10000 {
                Ok(self.bank0_high[addr as usize])
            } else {
                Err(EmulationError::InvalidAddress(addr))
            }
        }

        fn write_u8(&mut self, addr: u32, value: u8) -> BusResult<()> {
            if addr < 0x2000 || (0x7E0000..0x800000).contains(&addr) {
                self.wram.write_u8(addr, value)
            } else if addr < 0x10000 {
                self.bank0_high[addr as usize] = value;
                Ok(())
            } else {
                Err(EmulationError::InvalidAddress(addr))
            }
        }
    }

    #[test]
    fn cpu_initial_state_emulation_mode() {
        let cpu = Cpu::new();
        assert!(cpu.e, "CPU debe arrancar en modo emulación");
        assert!(cpu.p.contains(CpuFlags::IRQ_DISABLE));
        assert_eq!(cpu.sp & 0xFF00, 0x0100, "SP high byte debe ser 0x01 en emulación");
    }

    #[test]
    fn flags_bitmask_correct() {
        assert_eq!(CpuFlags::CARRY.bits(), 0x01);
        assert_eq!(CpuFlags::NEGATIVE.bits(), 0x80);
    }

    #[test]
    fn cpu_nop_cycles() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        // NOP en dirección 0
        wram.write_u8(0x7E0000, 0xEA).unwrap();
        cpu.pc = 0x0000;
        cpu.pb = 0x7E;

        let cycles = cpu.step(&mut wram).unwrap();
        assert_eq!(cycles, 2);
        assert_eq!(cpu.pc, 0x0001);
    }

    #[test]
    fn cpu_tax_8bit_mode() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.a = 0x1234;
        cpu.p.insert(CpuFlags::INDEX_8BIT); // 8-bit index mode

        wram.write_u8(0x7E0000, 0xAA).unwrap(); // TAX
        cpu.pc = 0x0000;
        cpu.pb = 0x7E;

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.x, 0x34); // Solo byte bajo en modo 8-bit
    }

    #[test]
    fn php_pushes_p_exactly_without_forcing_index_or_memory_width_bits() {
        // Regression guard for a real bug found via the actual SMW ROM:
        // op_php used to unconditionally OR in bits 4-5 (X and M width)
        // before pushing -- a 6502/NMOS quirk for the synthesized "B"
        // flag that does not apply to the 65816 in native mode, where
        // those bits are the real, meaningful index/accumulator widths.
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.e = false; // native mode
        cpu.p.remove(CpuFlags::INDEX_8BIT); // X/Y = 16-bit
        cpu.p.remove(CpuFlags::MEMORY_8BIT); // A = 16-bit
        cpu.p.insert(CpuFlags::CARRY);
        cpu.sp = 0x1FFF;

        wram.write_u8(0x7E0000, 0x08).unwrap(); // PHP
        cpu.pc = 0x0000;
        cpu.pb = 0x7E;
        cpu.step(&mut wram).unwrap();

        let pushed = wram.read_u8(0x7E1FFF).unwrap();
        assert_eq!(
            pushed & 0x30,
            0,
            "PHP must push the real (16-bit/16-bit) width bits as zero, not force them to 1: got {:#04X}",
            pushed
        );
        assert_eq!(pushed & 0x01, 1, "the real CARRY bit must still be preserved");
    }

    #[test]
    fn php_then_plp_round_trip_preserves_accumulator_width_across_an_nmi_style_prologue() {
        // Reproduces the exact real-world scenario that exposed this bug:
        // SMW's NMI handler does `PHP; REP #$30; ...; SEP #$30; ...; PLP`
        // to save/restore the interrupted code's register widths while
        // working in a fixed-width mode itself. If PHP forces bits 4-5,
        // PLP restores the wrong width, desyncing instruction decoding
        // for the code that resumes after the interrupt -- this was the
        // root cause of a real, 65816-desync bug that only manifested
        // after ~1.7M cycles into real SMW execution.
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.e = false;
        cpu.p.remove(CpuFlags::MEMORY_8BIT); // A = 16-bit, matching the interrupted code
        cpu.p.remove(CpuFlags::INDEX_8BIT); // X/Y = 16-bit
        cpu.sp = 0x1FFF;

        // PHP; SEP #$30 (switch to 8-bit, like the handler's own body); PLP
        wram.write_u8(0x7E0000, 0x08).unwrap(); // PHP
        wram.write_u8(0x7E0001, 0xE2).unwrap(); // SEP
        wram.write_u8(0x7E0002, 0x30).unwrap(); //   #$30
        wram.write_u8(0x7E0003, 0x28).unwrap(); // PLP
        cpu.pc = 0x0000;
        cpu.pb = 0x7E;

        cpu.step(&mut wram).unwrap(); // PHP
        cpu.step(&mut wram).unwrap(); // SEP #$30
        assert!(cpu.p.contains(CpuFlags::MEMORY_8BIT), "SEP #$30 must switch to 8-bit for the handler body");
        assert!(cpu.p.contains(CpuFlags::INDEX_8BIT));

        cpu.step(&mut wram).unwrap(); // PLP
        assert!(
            !cpu.p.contains(CpuFlags::MEMORY_8BIT),
            "PLP must restore the original 16-bit accumulator width, not leave the handler's forced 8-bit"
        );
        assert!(!cpu.p.contains(CpuFlags::INDEX_8BIT), "PLP must restore the original 16-bit index width");
    }

    #[test]
    fn cpu_tax_16bit_mode() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.a = 0x1234;
        cpu.p.remove(CpuFlags::INDEX_8BIT); // 16-bit index mode

        wram.write_u8(0x7E0000, 0xAA).unwrap(); // TAX
        cpu.pc = 0x0000;
        cpu.pb = 0x7E;

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.x, 0x1234); // Full word en modo 16-bit
    }

    #[test]
    fn cpu_txa_8bit_mode() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.x = 0x1234;
        cpu.p.insert(CpuFlags::MEMORY_8BIT); // 8-bit memory mode

        wram.write_u8(0x7E0000, 0x8A).unwrap(); // TXA
        cpu.pc = 0x0000;
        cpu.pb = 0x7E;

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x34); // Solo byte bajo en modo 8-bit
    }

    #[test]
    fn cpu_txa_16bit_mode() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.x = 0x1234;
        cpu.p.remove(CpuFlags::MEMORY_8BIT); // 16-bit memory mode

        wram.write_u8(0x7E0000, 0x8A).unwrap(); // TXA
        cpu.pc = 0x0000;
        cpu.pb = 0x7E;

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x1234); // Full word en modo 16-bit
    }

    #[test]
    fn cpu_flag_operations() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();

        // Setup: point PB to WRAM
        cpu.pb = 0x7E;

        // CLC
        cpu.p.insert(CpuFlags::CARRY);
        wram.write_u8(0x7E0000, 0x18).unwrap();
        cpu.pc = 0x0000;
        cpu.step(&mut wram).unwrap();
        assert!(!cpu.p.contains(CpuFlags::CARRY));

        // SEC
        wram.write_u8(0x7E0001, 0x38).unwrap();
        cpu.step(&mut wram).unwrap();
        assert!(cpu.p.contains(CpuFlags::CARRY));

        // CLD
        wram.write_u8(0x7E0002, 0xD8).unwrap();
        cpu.step(&mut wram).unwrap();
        assert!(!cpu.p.contains(CpuFlags::DECIMAL));

        // SED
        wram.write_u8(0x7E0003, 0xF8).unwrap();
        cpu.step(&mut wram).unwrap();
        assert!(cpu.p.contains(CpuFlags::DECIMAL));
    }

    #[test]
    fn cpu_nz_flags_16bit() {
        let mut cpu = Cpu::new();

        // Test zero
        cpu.update_nz_flags_16(0);
        assert!(cpu.p.contains(CpuFlags::ZERO));
        assert!(!cpu.p.contains(CpuFlags::NEGATIVE));

        // Test negative (bit 15 set)
        cpu.update_nz_flags_16(0x8000);
        assert!(!cpu.p.contains(CpuFlags::ZERO));
        assert!(cpu.p.contains(CpuFlags::NEGATIVE));

        // Test positive non-zero
        cpu.update_nz_flags_16(0x7FFF);
        assert!(!cpu.p.contains(CpuFlags::ZERO));
        assert!(!cpu.p.contains(CpuFlags::NEGATIVE));
    }

    #[test]
    fn cpu_nz_flags_8bit() {
        let mut cpu = Cpu::new();

        // Test zero
        cpu.update_nz_flags_8(0);
        assert!(cpu.p.contains(CpuFlags::ZERO));
        assert!(!cpu.p.contains(CpuFlags::NEGATIVE));

        // Test negative (bit 7 set)
        cpu.update_nz_flags_8(0x80);
        assert!(!cpu.p.contains(CpuFlags::ZERO));
        assert!(cpu.p.contains(CpuFlags::NEGATIVE));

        // Test positive non-zero
        cpu.update_nz_flags_8(0x7F);
        assert!(!cpu.p.contains(CpuFlags::ZERO));
        assert!(!cpu.p.contains(CpuFlags::NEGATIVE));
    }

    #[test]
    fn cpu_tsx_emulation_mode() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.sp = 0x01FF;
        cpu.p.insert(CpuFlags::INDEX_8BIT);

        wram.write_u8(0x7E0000, 0xBA).unwrap(); // TSX
        cpu.pc = 0x0000;
        cpu.pb = 0x7E;

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.x, 0xFF); // Low byte of SP
        assert!(cpu.p.contains(CpuFlags::NEGATIVE)); // 0xFF has bit 7 set
    }

    #[test]
    fn cpu_txs_emulation_mode() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.x = 0x0055;
        cpu.p.insert(CpuFlags::INDEX_8BIT);
        cpu.e = true; // Emulation mode

        wram.write_u8(0x7E0000, 0x9A).unwrap(); // TXS
        cpu.pc = 0x0000;
        cpu.pb = 0x7E;

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.sp, 0x0155); // High byte stays at 0x01 in emulation
    }

    // ==================== Load/Store Tests ====================

    #[test]
    fn lda_immediate_8bit() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.p.insert(CpuFlags::MEMORY_8BIT); // 8-bit mode
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // LDA #$42
        wram.write_u8(0x7E0000, 0xA9).unwrap();
        wram.write_u8(0x7E0001, 0x42).unwrap();

        let cycles = cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x42);
        assert_eq!(cycles, 2);
        assert!(!cpu.p.contains(CpuFlags::ZERO));
        assert!(!cpu.p.contains(CpuFlags::NEGATIVE));
    }

    #[test]
    fn lda_immediate_16bit() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.p.remove(CpuFlags::MEMORY_8BIT); // 16-bit mode
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // LDA #$1234
        wram.write_u8(0x7E0000, 0xA9).unwrap();
        wram.write_u8(0x7E0001, 0x34).unwrap();
        wram.write_u8(0x7E0002, 0x12).unwrap();

        let cycles = cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x1234);
        assert_eq!(cycles, 3);
    }

    #[test]
    fn lda_immediate_zero_flag() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // LDA #$00
        wram.write_u8(0x7E0000, 0xA9).unwrap();
        wram.write_u8(0x7E0001, 0x00).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x00);
        assert!(cpu.p.contains(CpuFlags::ZERO));
    }

    #[test]
    fn lda_immediate_negative_flag() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // LDA #$80
        wram.write_u8(0x7E0000, 0xA9).unwrap();
        wram.write_u8(0x7E0001, 0x80).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x80);
        assert!(cpu.p.contains(CpuFlags::NEGATIVE));
    }

    #[test]
    fn lda_absolute_8bit() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.db = 0x7E;
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // Pre-populate memory
        wram.write_u8(0x7E1234, 0xAB).unwrap();

        // LDA $1234
        wram.write_u8(0x7E0000, 0xAD).unwrap();
        wram.write_u8(0x7E0001, 0x34).unwrap();
        wram.write_u8(0x7E0002, 0x12).unwrap();

        let cycles = cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0xAB);
        assert_eq!(cycles, 4);
    }

    #[test]
    fn lda_absolute_16bit() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.p.remove(CpuFlags::MEMORY_8BIT); // 16-bit mode
        cpu.db = 0x7E;
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // Pre-populate memory (little-endian)
        wram.write_u8(0x7E1234, 0xCD).unwrap();
        wram.write_u8(0x7E1235, 0xAB).unwrap();

        // LDA $1234
        wram.write_u8(0x7E0000, 0xAD).unwrap();
        wram.write_u8(0x7E0001, 0x34).unwrap();
        wram.write_u8(0x7E0002, 0x12).unwrap();

        let cycles = cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0xABCD);
        assert_eq!(cycles, 5);
    }

    #[test]
    fn lda_direct_page() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.d = 0x1000;
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // Value at DP+10 = 0x1010
        wram.write_u8(0x001010, 0x55).unwrap();

        // LDA $10 (Direct Page)
        wram.write_u8(0x7E0000, 0xA5).unwrap();
        wram.write_u8(0x7E0001, 0x10).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x55);
    }

    #[test]
    fn lda_direct_page_with_offset() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.d = 0x10F0; // D low byte != 0
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.p.insert(CpuFlags::MEMORY_8BIT);

        // Value at DP+$10 = 0x10F0 + 0x10 = 0x1100 (wrapping)
        wram.write_u8(0x001100, 0x77).unwrap();

        // LDA $10
        wram.write_u8(0x7E0000, 0xA5).unwrap();
        wram.write_u8(0x7E0001, 0x10).unwrap();

        let cycles = cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x77);
        // Extra cycle when D low byte != 0
        assert_eq!(cycles, 4);
    }

    #[test]
    fn sta_absolute_8bit() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.a = 0xAB;
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.db = 0x7E;
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // STA $1234
        wram.write_u8(0x7E0000, 0x8D).unwrap();
        wram.write_u8(0x7E0001, 0x34).unwrap();
        wram.write_u8(0x7E0002, 0x12).unwrap();

        let cycles = cpu.step(&mut wram).unwrap();
        assert_eq!(wram.read_u8(0x7E1234).unwrap(), 0xAB);
        assert_eq!(cycles, 4);
    }

    #[test]
    fn sta_absolute_16bit() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.a = 0xABCD;
        cpu.p.remove(CpuFlags::MEMORY_8BIT); // 16-bit mode
        cpu.db = 0x7E;
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // STA $1234
        wram.write_u8(0x7E0000, 0x8D).unwrap();
        wram.write_u8(0x7E0001, 0x34).unwrap();
        wram.write_u8(0x7E0002, 0x12).unwrap();

        let cycles = cpu.step(&mut wram).unwrap();
        assert_eq!(wram.read_u8(0x7E1234).unwrap(), 0xCD); // Low byte
        assert_eq!(wram.read_u8(0x7E1235).unwrap(), 0xAB); // High byte
        assert_eq!(cycles, 5);
    }

    #[test]
    fn sta_direct_page() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.a = 0x99;
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        // Direct Page addressing always targets bank 0; keep D + operand
        // within the real hardware's 8KB WRAM mirror ($0000-$1FFF) --
        // `Wram` itself now correctly rejects the rest of bank 0
        // ($2000-$7FFF is I/O, $8000-$FFFF is ROM), matching real
        // hardware rather than treating all of bank 0 as WRAM.
        cpu.d = 0x1000;
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // STA $20 (Direct Page)
        wram.write_u8(0x7E0000, 0x85).unwrap();
        wram.write_u8(0x7E0001, 0x20).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(wram.read_u8(0x001020).unwrap(), 0x99);
    }

    #[test]
    fn ldx_immediate_8bit() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.p.insert(CpuFlags::INDEX_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // LDX #$77
        wram.write_u8(0x7E0000, 0xA2).unwrap();
        wram.write_u8(0x7E0001, 0x77).unwrap();

        let cycles = cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.x, 0x77);
        assert_eq!(cycles, 2);
    }

    #[test]
    fn ldx_immediate_16bit() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.p.remove(CpuFlags::INDEX_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // LDX #$BEEF
        wram.write_u8(0x7E0000, 0xA2).unwrap();
        wram.write_u8(0x7E0001, 0xEF).unwrap();
        wram.write_u8(0x7E0002, 0xBE).unwrap();

        let cycles = cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.x, 0xBEEF);
        assert_eq!(cycles, 3);
    }

    #[test]
    fn ldx_absolute() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.p.insert(CpuFlags::INDEX_8BIT);
        cpu.db = 0x7E;
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        wram.write_u8(0x7E5678, 0x33).unwrap();

        // LDX $5678
        wram.write_u8(0x7E0000, 0xAE).unwrap();
        wram.write_u8(0x7E0001, 0x78).unwrap();
        wram.write_u8(0x7E0002, 0x56).unwrap();

        let cycles = cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.x, 0x33);
        assert_eq!(cycles, 4);
    }

    #[test]
    fn ldy_immediate_8bit() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.p.insert(CpuFlags::INDEX_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // LDY #$88
        wram.write_u8(0x7E0000, 0xA0).unwrap();
        wram.write_u8(0x7E0001, 0x88).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.y, 0x88);
        assert!(cpu.p.contains(CpuFlags::NEGATIVE));
    }

    #[test]
    fn stx_absolute() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.x = 0xDE;
        cpu.p.insert(CpuFlags::INDEX_8BIT);
        cpu.db = 0x7E;
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // STX $ABCD
        wram.write_u8(0x7E0000, 0x8E).unwrap();
        wram.write_u8(0x7E0001, 0xCD).unwrap();
        wram.write_u8(0x7E0002, 0xAB).unwrap();

        let cycles = cpu.step(&mut wram).unwrap();
        assert_eq!(wram.read_u8(0x7EABCD).unwrap(), 0xDE);
        assert_eq!(cycles, 4);
    }

    #[test]
    fn sty_absolute() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.y = 0x55;
        cpu.p.insert(CpuFlags::INDEX_8BIT);
        cpu.db = 0x7E;
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // STY $3000
        wram.write_u8(0x7E0000, 0x8C).unwrap();
        wram.write_u8(0x7E0001, 0x00).unwrap();
        wram.write_u8(0x7E0002, 0x30).unwrap();

        let cycles = cpu.step(&mut wram).unwrap();
        assert_eq!(wram.read_u8(0x7E3000).unwrap(), 0x55);
        assert_eq!(cycles, 4);
    }

    #[test]
    fn stx_direct_page() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.x = 0x42;
        cpu.p.insert(CpuFlags::INDEX_8BIT);
        cpu.d = 0x0000;
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // STX $05
        wram.write_u8(0x7E0000, 0x86).unwrap();
        wram.write_u8(0x7E0001, 0x05).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(wram.read_u8(0x0005).unwrap(), 0x42);
    }

    #[test]
    fn sty_direct_page() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.y = 0x33;
        cpu.p.insert(CpuFlags::INDEX_8BIT);
        cpu.d = 0x1000;
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // STY $80
        wram.write_u8(0x7E0000, 0x84).unwrap();
        wram.write_u8(0x7E0001, 0x80).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(wram.read_u8(0x1080).unwrap(), 0x33);
    }

    // ==================== Direct Page indexed wrap quirk ====================
    // Regression coverage for the documented 65816 emulation-mode quirk
    // (Eyes & Lichty, "Programming the 65816", inherited for 6502
    // compatibility): when E=1 and D's low byte is 0, dp,X / dp,Y / (dp,X)
    // must wrap (offset + index) within a single 256-byte page instead of
    // carrying into D's high byte.

    // Test code lives at $7E3000, not $7E0000 -- addresses below $2000 in
    // any bank alias the same underlying WRAM bytes as bank 0's Direct
    // Page mirror ($7E0000-$7E1FFF == $000000-$001FFF, see `Wram`), so
    // placing the opcode/operand there would collide with the small
    // effective addresses ($0001, $0002, ...) these quirk cases target.

    #[test]
    fn lda_dp_x_wraps_within_page_in_emulation_mode_with_dl_zero() {
        let mut cpu = Cpu::new(); // e = true, d = 0 by default
        let mut wram = Wram::new();
        cpu.x = 0x02;
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x3000;

        // 0xFF + 0x02 = 0x101 -> must wrap the low byte to $0001, not $0101
        wram.write_u8(0x000001, 0x42).unwrap();
        wram.write_u8(0x000101, 0x99).unwrap(); // decoy: what the old (wrong) code would read

        // LDA $FF,X
        wram.write_u8(0x7E3000, 0xB5).unwrap();
        wram.write_u8(0x7E3001, 0xFF).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x42, "must honor the page-wrap quirk, not a plain 16-bit add");
    }

    #[test]
    fn lda_dp_x_uses_full_16bit_add_in_native_mode() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.e = false; // native mode: quirk never applies, regardless of D
        cpu.d = 0x1000;
        cpu.x = 0x02;
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x3000;

        // 0x1000 + 0xFF + 0x02 = 0x1101, full carry into D's high byte
        wram.write_u8(0x001101, 0x77).unwrap();

        // LDA $FF,X
        wram.write_u8(0x7E3000, 0xB5).unwrap();
        wram.write_u8(0x7E3001, 0xFF).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x77);
    }

    #[test]
    fn lda_dp_x_uses_full_16bit_add_in_emulation_mode_with_dl_nonzero() {
        let mut cpu = Cpu::new(); // e = true
        let mut wram = Wram::new();
        cpu.d = 0x0010; // DL != 0 disables the quirk even in emulation mode
        cpu.x = 0x02;
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x3000;

        // 0x0010 + 0xFF + 0x02 = 0x0111
        wram.write_u8(0x000111, 0x88).unwrap();

        // LDA $FF,X
        wram.write_u8(0x7E3000, 0xB5).unwrap();
        wram.write_u8(0x7E3001, 0xFF).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x88);
    }

    #[test]
    fn ldx_dp_y_wraps_within_page_in_emulation_mode_with_dl_zero() {
        let mut cpu = Cpu::new(); // e = true, d = 0 by default
        let mut wram = Wram::new();
        cpu.y = 0x02;
        cpu.p.insert(CpuFlags::INDEX_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x3000;

        wram.write_u8(0x000001, 0x11).unwrap();
        wram.write_u8(0x000101, 0xEE).unwrap(); // decoy

        // LDX $FF,Y
        wram.write_u8(0x7E3000, 0xB6).unwrap();
        wram.write_u8(0x7E3001, 0xFF).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.x, 0x11, "must honor the page-wrap quirk, not a plain 16-bit add");
    }

    #[test]
    fn ldx_dp_y_uses_full_16bit_add_in_native_mode() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.e = false;
        cpu.d = 0x1000;
        cpu.y = 0x02;
        cpu.p.insert(CpuFlags::INDEX_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x3000;

        wram.write_u8(0x001101, 0x66).unwrap();

        // LDX $FF,Y
        wram.write_u8(0x7E3000, 0xB6).unwrap();
        wram.write_u8(0x7E3001, 0xFF).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.x, 0x66);
    }

    #[test]
    fn ldx_dp_y_uses_full_16bit_add_in_emulation_mode_with_dl_nonzero() {
        let mut cpu = Cpu::new(); // e = true
        let mut wram = Wram::new();
        cpu.d = 0x0010; // DL != 0 disables the quirk even in emulation mode
        cpu.y = 0x02;
        cpu.p.insert(CpuFlags::INDEX_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x3000;

        // 0x0010 + 0xFF + 0x02 = 0x0111
        wram.write_u8(0x000111, 0x44).unwrap();

        // LDX $FF,Y
        wram.write_u8(0x7E3000, 0xB6).unwrap();
        wram.write_u8(0x7E3001, 0xFF).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.x, 0x44);
    }

    #[test]
    fn lda_indirect_dp_x_wraps_pointer_lookup_within_page_in_emulation_mode_with_dl_zero() {
        let mut cpu = Cpu::new(); // e = true, d = 0 by default
        let mut wram = Wram::new();
        cpu.x = 0x02;
        cpu.db = 0x7E;
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x3000;

        // Pointer must be read from wrapped dp address $0001, not $0101
        wram.write_u8(0x000001, 0x00).unwrap(); // pointer lo
        wram.write_u8(0x000002, 0x02).unwrap(); // pointer hi -> pointer = $0200
        wram.write_u8(0x7E0200, 0x5A).unwrap(); // target value

        // Decoy pointer at the unwrapped ($0101) address, pointing elsewhere
        wram.write_u8(0x000101, 0xAD).unwrap();
        wram.write_u8(0x000102, 0xDE).unwrap();

        // LDA ($FF,X)
        wram.write_u8(0x7E3000, 0xA1).unwrap();
        wram.write_u8(0x7E3001, 0xFF).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x5A, "must dereference the page-wrapped dp pointer, not a plain 16-bit add");
    }

    #[test]
    fn lda_indirect_dp_x_uses_full_16bit_add_in_native_mode() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.e = false;
        cpu.d = 0x1000;
        cpu.x = 0x02;
        cpu.db = 0x7E;
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x3000;

        // dp_addr = 0x1000 + 0xFF + 0x02 = 0x1101
        wram.write_u8(0x001101, 0x00).unwrap(); // pointer lo
        wram.write_u8(0x001102, 0x03).unwrap(); // pointer hi -> pointer = $0300
        wram.write_u8(0x7E0300, 0x9C).unwrap();

        // LDA ($FF,X)
        wram.write_u8(0x7E3000, 0xA1).unwrap();
        wram.write_u8(0x7E3001, 0xFF).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x9C);
    }

    #[test]
    fn lda_indirect_dp_x_uses_full_16bit_add_in_emulation_mode_with_dl_nonzero() {
        let mut cpu = Cpu::new(); // e = true
        let mut wram = Wram::new();
        cpu.d = 0x0010; // DL != 0 disables the quirk even in emulation mode
        cpu.x = 0x02;
        cpu.db = 0x7E;
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x3000;

        // dp_addr = 0x0010 + 0xFF + 0x02 = 0x0111
        wram.write_u8(0x000111, 0x00).unwrap(); // pointer lo
        wram.write_u8(0x000112, 0x04).unwrap(); // pointer hi -> pointer = $0400
        wram.write_u8(0x7E0400, 0x13).unwrap();

        // LDA ($FF,X)
        wram.write_u8(0x7E3000, 0xA1).unwrap();
        wram.write_u8(0x7E3001, 0xFF).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x13);
    }

    // ==================== New opcode tests ====================

    #[test]
    fn cpu_and_immediate() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.a = 0xFF;
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // AND #$0F
        wram.write_u8(0x7E0000, 0x29).unwrap();
        wram.write_u8(0x7E0001, 0x0F).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x0F);
    }

    #[test]
    fn cpu_ora_immediate() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.a = 0x0F;
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // ORA #$F0
        wram.write_u8(0x7E0000, 0x09).unwrap();
        wram.write_u8(0x7E0001, 0xF0).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0xFF);
    }

    #[test]
    fn cpu_eor_immediate() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.a = 0xFF;
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // EOR #$FF
        wram.write_u8(0x7E0000, 0x49).unwrap();
        wram.write_u8(0x7E0001, 0xFF).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x00);
        assert!(cpu.p.contains(CpuFlags::ZERO));
    }

    #[test]
    fn cpu_cmp_equal() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.a = 0x42;
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // CMP #$42
        wram.write_u8(0x7E0000, 0xC9).unwrap();
        wram.write_u8(0x7E0001, 0x42).unwrap();

        cpu.step(&mut wram).unwrap();
        assert!(cpu.p.contains(CpuFlags::ZERO));
        assert!(cpu.p.contains(CpuFlags::CARRY)); // A >= operand
    }

    #[test]
    fn cpu_cmp_greater() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.a = 0x50;
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // CMP #$40
        wram.write_u8(0x7E0000, 0xC9).unwrap();
        wram.write_u8(0x7E0001, 0x40).unwrap();

        cpu.step(&mut wram).unwrap();
        assert!(!cpu.p.contains(CpuFlags::ZERO));
        assert!(cpu.p.contains(CpuFlags::CARRY));
    }

    #[test]
    fn cpu_cmp_less() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.a = 0x30;
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // CMP #$40
        wram.write_u8(0x7E0000, 0xC9).unwrap();
        wram.write_u8(0x7E0001, 0x40).unwrap();

        cpu.step(&mut wram).unwrap();
        assert!(!cpu.p.contains(CpuFlags::ZERO));
        assert!(!cpu.p.contains(CpuFlags::CARRY));
    }

    #[test]
    fn cpu_inx() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.x = 0x05;
        cpu.p.insert(CpuFlags::INDEX_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // INX
        wram.write_u8(0x7E0000, 0xE8).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.x, 0x06);
    }

    #[test]
    fn cpu_dex() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.x = 0x05;
        cpu.p.insert(CpuFlags::INDEX_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // DEX
        wram.write_u8(0x7E0000, 0xCA).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.x, 0x04);
    }

    #[test]
    fn cpu_asl_accumulator() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.a = 0x40; // 0100 0000
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // ASL A
        wram.write_u8(0x7E0000, 0x0A).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x80); // 1000 0000
        assert!(!cpu.p.contains(CpuFlags::CARRY)); // bit 7 was 0
    }

    #[test]
    fn cpu_lsr_accumulator() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.a = 0x81; // 1000 0001
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // LSR A
        wram.write_u8(0x7E0000, 0x4A).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x40); // 0100 0000
        assert!(cpu.p.contains(CpuFlags::CARRY)); // bit 0 was 1
    }

    #[test]
    fn cpu_rol_accumulator() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.a = 0x80; // 1000 0000, carry = 0 initially
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // ROL A (with carry = 0)
        wram.write_u8(0x7E0000, 0x2A).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x00); // 0000 0000 (rotate through carry)
        assert!(cpu.p.contains(CpuFlags::CARRY)); // old bit 7 becomes carry
        assert!(cpu.p.contains(CpuFlags::ZERO));
    }

    #[test]
    fn cpu_ror_accumulator() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.a = 0x01; // 0000 0001
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // ROR A (with carry = 0)
        wram.write_u8(0x7E0000, 0x6A).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x00); // 0000 0000
        assert!(cpu.p.contains(CpuFlags::CARRY)); // old bit 0 becomes carry
        assert!(cpu.p.contains(CpuFlags::ZERO));
    }

    #[test]
    fn cpu_jsr_rts() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.pb = 0x7E;
        cpu.sp = 0x01FF;
        cpu.pc = 0x0000;

        // JSR $0200
        wram.write_u8(0x7E0000, 0x20).unwrap();
        wram.write_u8(0x7E0001, 0x00).unwrap();
        wram.write_u8(0x7E0002, 0x02).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.pc, 0x0200);
        // In emulation mode, JSR pushes 2 bytes (PC-1)
    }

    #[test]
    fn cpu_rep_clear_carry() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        // Ensure CARRY is set initially
        cpu.p = CpuFlags::CARRY;
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // REP #$01 (clear C)
        wram.write_u8(0x7E0000, 0xC2).unwrap();
        wram.write_u8(0x7E0001, 0x01).unwrap();

        cpu.step(&mut wram).unwrap();
        assert!(!cpu.p.contains(CpuFlags::CARRY));
    }

    #[test]
    fn cpu_sep_set_carry() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        // Ensure CARRY is clear initially
        cpu.p = CpuFlags::empty();
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        // SEP #$01 (set C)
        wram.write_u8(0x7E0000, 0xE2).unwrap();
        wram.write_u8(0x7E0001, 0x01).unwrap();

        cpu.step(&mut wram).unwrap();
        assert!(cpu.p.contains(CpuFlags::CARRY));
    }

    #[test]
    fn cpu_xba_swaps_accumulator_bytes() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.a = 0x12CC;

        wram.write_u8(0x7E0000, 0xEB).unwrap(); // XBA
        let cycles = cpu.step(&mut wram).unwrap();
        assert_eq!(cycles, 3);
        assert_eq!(cpu.a, 0xCC12, "high and low bytes must swap");
    }

    #[test]
    fn cpu_inc_dec_acc_8bit_wraps_within_low_byte() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.a = 0x12FF;

        wram.write_u8(0x7E0000, 0x1A).unwrap(); // INC A
        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x1200, "8-bit INC must wrap within the low byte, leaving the high byte untouched");

        cpu.pc = 0x0001;
        wram.write_u8(0x7E0001, 0x3A).unwrap(); // DEC A
        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x12FF);
    }

    #[test]
    fn cpu_inx_dex_iny_dey_zero_high_byte_in_8bit_index_mode() {
        // Unlike A (which keeps a "hidden" high byte across 8-bit ops,
        // exposed via XBA), X and Y architecturally zero their high byte
        // on any 8-bit-mode write -- LDX/LDY already did this correctly,
        // but INX/DEX/INY/DEY previously preserved the stale high byte
        // instead (`self.x & 0xFF00 | ...`), a real, separate bug from the
        // LDA one. Found by tracing a stack-corruption crash deep into
        // real SMW execution back to a DEX/BPL loop.
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.p.insert(CpuFlags::INDEX_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.x = 0x1200;
        cpu.y = 0x3400;

        wram.write_u8(0x7E0000, 0xE8).unwrap(); // INX
        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.x, 0x01, "8-bit INX must zero the high byte, not preserve it");

        cpu.x = 0x1200;
        cpu.pc = 0x0001;
        wram.write_u8(0x7E0001, 0xCA).unwrap(); // DEX
        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.x, 0xFF, "8-bit DEX must zero the high byte, not preserve it");

        cpu.pc = 0x0002;
        wram.write_u8(0x7E0002, 0xC8).unwrap(); // INY
        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.y, 0x01, "8-bit INY must zero the high byte, not preserve it");

        cpu.y = 0x3400;
        cpu.pc = 0x0003;
        wram.write_u8(0x7E0003, 0x88).unwrap(); // DEY
        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.y, 0xFF, "8-bit DEY must zero the high byte, not preserve it");
    }

    // The opcodes below (STZ, TCD/TDC/TCS, PHB/PLB/PHD/PLD/PHK, absolute-long
    // and absolute-indexed addressing) were added while tracing real
    // execution of Super Mario World's actual boot code byte-for-byte; each
    // test pins the exact behavior observed against the genuine ROM bytes,
    // not just a textbook description of the opcode.

    #[test]
    fn cpu_stz_abs_writes_zero_8bit() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.db = 0x7E;
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        wram.write_u8(0x7E1234, 0xFF).unwrap(); // pre-fill with garbage
        wram.write_u8(0x7E0000, 0x9C).unwrap(); // STZ $1234
        wram.write_u8(0x7E0001, 0x34).unwrap();
        wram.write_u8(0x7E0002, 0x12).unwrap();

        let cycles = cpu.step(&mut wram).unwrap();
        assert_eq!(cycles, 4);
        assert_eq!(wram.read_u8(0x7E1234).unwrap(), 0x00);
    }

    #[test]
    fn cpu_stz_dp_writes_zero_16bit() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.p.remove(CpuFlags::MEMORY_8BIT); // 16-bit accumulator/memory
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.d = 0x0000;

        wram.write_u8(0x7E0050, 0xAA).unwrap();
        wram.write_u8(0x7E0051, 0xBB).unwrap();
        wram.write_u8(0x7E0000, 0x64).unwrap(); // STZ $50
        wram.write_u8(0x7E0001, 0x50).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(wram.read_u8(0x7E0050).unwrap(), 0x00);
        assert_eq!(wram.read_u8(0x7E0051).unwrap(), 0x00);
    }

    #[test]
    fn cpu_tcd_and_tdc_roundtrip() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.a = 0x1234;

        wram.write_u8(0x7E0000, 0x5B).unwrap(); // TCD
        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.d, 0x1234, "TCD must move the full 16-bit accumulator into D");

        cpu.a = 0x0000;
        cpu.pb = 0x7E;
        cpu.pc = 0x0001;
        wram.write_u8(0x7E0001, 0x7B).unwrap(); // TDC
        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x1234, "TDC must move all 16 bits of D back into the accumulator");
    }

    #[test]
    fn cpu_tcs_sets_stack_pointer_in_native_mode() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.e = false; // native mode
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.a = 0x1FFF;

        wram.write_u8(0x7E0000, 0x1B).unwrap(); // TCS
        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.sp, 0x1FFF);
    }

    #[test]
    fn cpu_tsc_transfers_stack_pointer_to_full_accumulator() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.e = false; // native mode
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.sp = 0x8FF0;
        cpu.a = 0x0000;
        cpu.p.insert(CpuFlags::MEMORY_8BIT); // must be ignored: TSC is always 16-bit

        wram.write_u8(0x7E0000, 0x3B).unwrap(); // TSC
        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x8FF0, "TSC must move all 16 bits of S into the accumulator even with M set");
        assert!(cpu.p.contains(CpuFlags::NEGATIVE), "N must reflect bit 15 of the 16-bit result");
        assert!(!cpu.p.contains(CpuFlags::ZERO));
    }

    #[test]
    fn cpu_jsr_indexed_indirect_jumps_through_pb_relative_pointer_and_pushes_return() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.e = false;
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.sp = 0x1FFF;
        cpu.x = 0x0004;

        // JSR ($0010,X) -> pointer at $7E:0014 -> target $8042.
        wram.write_u8(0x7E0000, 0xFC).unwrap();
        wram.write_u8(0x7E0001, 0x10).unwrap();
        wram.write_u8(0x7E0002, 0x00).unwrap();
        wram.write_u8(0x7E0014, 0x42).unwrap();
        wram.write_u8(0x7E0015, 0x80).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.pc, 0x8042, "must jump through the X-indexed pointer in the program bank");
        assert_eq!(cpu.pb, 0x7E, "JSR (addr,X) never changes the program bank");
        assert_eq!(cpu.sp, 0x1FFD, "must push a 16-bit return address");
        // Return address = last byte of the 3-byte instruction ($0002),
        // so RTS (which adds 1) resumes at $0003.
        assert_eq!(wram.read_u8(0x7E1FFE).unwrap(), 0x02);
        assert_eq!(wram.read_u8(0x7E1FFF).unwrap(), 0x00);
    }

    #[test]
    fn cpu_phb_plb_roundtrip() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.e = false;
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.db = 0x80;
        cpu.sp = 0x1FFF;

        wram.write_u8(0x7E0000, 0x8B).unwrap(); // PHB
        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.sp, 0x1FFE, "PHB must push exactly one byte");

        cpu.db = 0x00;
        cpu.pc = 0x0001;
        wram.write_u8(0x7E0001, 0xAB).unwrap(); // PLB
        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.db, 0x80, "PLB must restore the pushed Data Bank value");
        assert_eq!(cpu.sp, 0x1FFF);
    }

    #[test]
    fn cpu_phd_pld_roundtrip() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.e = false;
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.d = 0xABCD;
        cpu.sp = 0x1FFF;

        wram.write_u8(0x7E0000, 0x0B).unwrap(); // PHD
        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.sp, 0x1FFD, "PHD must push a full 16-bit value");

        cpu.d = 0x0000;
        cpu.pc = 0x0001;
        wram.write_u8(0x7E0001, 0x2B).unwrap(); // PLD
        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.d, 0xABCD);
        assert_eq!(cpu.sp, 0x1FFF);
    }

    #[test]
    fn cpu_phk_pushes_program_bank() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.e = false;
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.sp = 0x1FFF;

        wram.write_u8(0x7E0000, 0x4B).unwrap(); // PHK
        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.sp, 0x1FFE);
        assert_eq!(wram.read_u8(0x7E1FFF).unwrap(), 0x7E, "PHK must push the current program bank byte");
    }

    #[test]
    fn cpu_sta_long_uses_explicit_bank_ignoring_db() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.db = 0x00; // deliberately different from the long address's bank
        cpu.a = 0x42;

        // STA $7E2000 (Absolute Long): bytes are little-endian addr, then bank
        wram.write_u8(0x7E0000, 0x8F).unwrap();
        wram.write_u8(0x7E0001, 0x00).unwrap();
        wram.write_u8(0x7E0002, 0x20).unwrap();
        wram.write_u8(0x7E0003, 0x7E).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(wram.read_u8(0x7E2000).unwrap(), 0x42, "STA long must use its own embedded bank, not DB");
    }

    #[test]
    fn cpu_lda_long_reads_explicit_bank() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.db = 0x00;

        wram.write_u8(0x7E3000, 0x99).unwrap();
        wram.write_u8(0x7E0000, 0xAF).unwrap(); // LDA $7E3000 (Absolute Long)
        wram.write_u8(0x7E0001, 0x00).unwrap();
        wram.write_u8(0x7E0002, 0x30).unwrap();
        wram.write_u8(0x7E0003, 0x7E).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x99);
    }

    #[test]
    fn cpu_sta_long_x_wraps_carry_into_bank() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.p.insert(CpuFlags::INDEX_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.a = 0x55;
        cpu.x = 0x10;

        // STA $7EFFF8,X with X=0x10 -> effective address 0x7F0008 (carries into next bank)
        wram.write_u8(0x7E0000, 0x9F).unwrap();
        wram.write_u8(0x7E0001, 0xF8).unwrap();
        wram.write_u8(0x7E0002, 0xFF).unwrap();
        wram.write_u8(0x7E0003, 0x7E).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(wram.read_u8(0x7F0008).unwrap(), 0x55, "the carry from base+X must cross into the next bank");
    }

    #[test]
    fn cpu_sta_abs_x_carries_into_next_bank() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.p.insert(CpuFlags::INDEX_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.db = 0x7E;
        cpu.a = 0x77;
        cpu.x = 0x10;

        // STA $FFF8,X with X=0x10: DB:$FFF8 + X overflows the 16-bit offset,
        // so the carry propagates into the bank byte -- effective address is
        // $7F0008, not $7E0008 (real 65816 hardware behavior).
        wram.write_u8(0x7E0000, 0x9D).unwrap();
        wram.write_u8(0x7E0001, 0xF8).unwrap();
        wram.write_u8(0x7E0002, 0xFF).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(wram.read_u8(0x7F0008).unwrap(), 0x77, "plain Absolute,X must carry into the next bank on overflow");
    }

    #[test]
    fn cpu_sta_abs_y_carries_into_next_bank() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.p.insert(CpuFlags::INDEX_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.db = 0x7E;
        cpu.a = 0x77;
        cpu.y = 0x0A;

        // STA $FFFE,Y with Y=0x0A: DB:$FFFE + Y overflows the 16-bit offset,
        // so the carry propagates into the bank byte -- effective address is
        // $7F0008, not $7E0008.
        wram.write_u8(0x7E0000, 0x99).unwrap();
        wram.write_u8(0x7E0001, 0xFE).unwrap();
        wram.write_u8(0x7E0002, 0xFF).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(wram.read_u8(0x7F0008).unwrap(), 0x77, "plain Absolute,Y must carry into the next bank on overflow");
    }

    #[test]
    fn cpu_lda_abs_x_carries_into_next_bank_wrapping_ff_to_00() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.p.insert(CpuFlags::INDEX_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.db = 0xFF;
        cpu.x = 0x0A;

        // LDA $FFFE,X with DB=$FF, X=0x0A: the bank carry must wrap from
        // $FF to $00 (the canonical Eyes & Lichty example), landing at $000008.
        wram.write_u8(0x7E0008, 0x42).unwrap();
        wram.write_u8(0x7E0000, 0xBD).unwrap();
        wram.write_u8(0x7E0001, 0xFE).unwrap();
        wram.write_u8(0x7E0002, 0xFF).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x42, "bank carry must wrap from $FF to $00");
    }

    #[test]
    fn cpu_lda_abs_y_carries_into_next_bank_wrapping_ff_to_00() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.p.insert(CpuFlags::INDEX_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.db = 0xFF;
        cpu.y = 0x0A;

        // LDA $FFFE,Y with DB=$FF, Y=0x0A: same wraparound as the ,X case.
        wram.write_u8(0x7E0008, 0x42).unwrap();
        wram.write_u8(0x7E0000, 0xB9).unwrap();
        wram.write_u8(0x7E0001, 0xFE).unwrap();
        wram.write_u8(0x7E0002, 0xFF).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x42, "bank carry must wrap from $FF to $00");
    }

    #[test]
    fn cpu_lda_abs_x_reads_from_data_bank() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.p.insert(CpuFlags::MEMORY_8BIT);
        cpu.p.insert(CpuFlags::INDEX_8BIT);
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.db = 0x7E;
        cpu.x = 0x05;

        wram.write_u8(0x7E2005, 0x66).unwrap();
        wram.write_u8(0x7E0000, 0xBD).unwrap(); // LDA $2000,X
        wram.write_u8(0x7E0001, 0x00).unwrap();
        wram.write_u8(0x7E0002, 0x20).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a, 0x66);
    }

    // ==================== Bug-fix regression tests ====================

    #[test]
    fn cpu_adc_decimal_09_plus_01_equals_10_no_carry() {
        let mut cpu = Cpu::new(); // emulation mode: 8-bit A by default
        let mut wram = Wram::new();
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.a = 0x09;
        cpu.p.insert(CpuFlags::DECIMAL);
        cpu.p.remove(CpuFlags::CARRY);

        wram.write_u8(0x7E0000, 0x69).unwrap(); // ADC #$01
        wram.write_u8(0x7E0001, 0x01).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a & 0xFF, 0x10, "BCD 09 + 01 must produce 10, not the binary 0A");
        assert!(!cpu.p.contains(CpuFlags::CARRY));
    }

    #[test]
    fn cpu_adc_decimal_99_plus_01_equals_00_with_carry() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.a = 0x99;
        cpu.p.insert(CpuFlags::DECIMAL);
        cpu.p.remove(CpuFlags::CARRY);

        wram.write_u8(0x7E0000, 0x69).unwrap(); // ADC #$01
        wram.write_u8(0x7E0001, 0x01).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a & 0xFF, 0x00, "BCD 99 + 01 must wrap to 00");
        assert!(cpu.p.contains(CpuFlags::CARRY), "BCD 99 + 01 must set Carry");
    }

    #[test]
    fn cpu_sbc_decimal_10_minus_01_equals_09_no_borrow() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.a = 0x10;
        cpu.p.insert(CpuFlags::DECIMAL);
        cpu.p.insert(CpuFlags::CARRY); // Carry set = no incoming borrow

        wram.write_u8(0x7E0000, 0xE9).unwrap(); // SBC #$01
        wram.write_u8(0x7E0001, 0x01).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a & 0xFF, 0x09, "BCD 10 - 01 must produce 09, not the binary 0F");
        assert!(cpu.p.contains(CpuFlags::CARRY), "no borrow occurred, so Carry must remain set");
    }

    #[test]
    fn cpu_sbc_decimal_00_minus_01_borrows_to_99() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.a = 0x00;
        cpu.p.insert(CpuFlags::DECIMAL);
        cpu.p.insert(CpuFlags::CARRY);

        wram.write_u8(0x7E0000, 0xE9).unwrap(); // SBC #$01
        wram.write_u8(0x7E0001, 0x01).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.a & 0xFF, 0x99, "BCD 00 - 01 must borrow down to 99");
        assert!(!cpu.p.contains(CpuFlags::CARRY), "a borrow occurred, so Carry must be cleared");
    }

    #[test]
    fn cpu_xce_does_not_reset_direct_page_when_entering_native_mode() {
        // Regression test: XCE previously zeroed D whenever it switched
        // from emulation to native mode, which is not real 65816 behavior
        // -- only a full RESET clears the Direct Page register.
        let mut cpu = Cpu::new(); // starts in emulation mode (e = true)
        let mut wram = Wram::new();
        cpu.d = 0xABCD;
        cpu.p.remove(CpuFlags::CARRY); // old Carry = 0 -> new E = false (native)
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        wram.write_u8(0x7E0000, 0xFB).unwrap(); // XCE
        cpu.step(&mut wram).unwrap();

        assert!(!cpu.e, "Carry was clear, so XCE must switch to native mode");
        assert_eq!(
            cpu.d, 0xABCD,
            "XCE must not touch the Direct Page register -- only RESET clears D"
        );
    }

    #[test]
    fn cpu_xce_entering_emulation_forces_8bit_widths_and_truncates_index_high_bytes() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.e = false; // start in native mode
        cpu.p.remove(CpuFlags::MEMORY_8BIT);
        cpu.p.remove(CpuFlags::INDEX_8BIT);
        cpu.p.insert(CpuFlags::CARRY); // old Carry = 1 -> new E = true (emulation)
        cpu.x = 0x1234;
        cpu.y = 0x5678;
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        wram.write_u8(0x7E0000, 0xFB).unwrap(); // XCE
        cpu.step(&mut wram).unwrap();

        assert!(cpu.e, "Carry was set, so XCE must switch to emulation mode");
        assert!(
            cpu.p.contains(CpuFlags::MEMORY_8BIT),
            "entering emulation mode must force 8-bit accumulator width"
        );
        assert!(
            cpu.p.contains(CpuFlags::INDEX_8BIT),
            "entering emulation mode must force 8-bit index width"
        );
        assert_eq!(cpu.x, 0x0034, "entering emulation mode must truncate X's high byte");
        assert_eq!(cpu.y, 0x0078, "entering emulation mode must truncate Y's high byte");
    }

    #[test]
    fn rep_forces_8bit_registers_when_emulation_mode_is_active() {
        // Regression test: real 65816 hardware cannot have 16-bit M/X
        // while E is set -- REP must not be able to widen registers out
        // of that hardware-enforced state, even though its mask asks for
        // it.
        let mut cpu = Cpu::new(); // starts in emulation mode (e = true)
        let mut wram = Wram::new();
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.x = 0x1234;
        cpu.y = 0x5678;
        assert!(cpu.e, "test assumes the CPU starts in emulation mode");

        wram.write_u8(0x7E0000, 0xC2).unwrap(); // REP #$30
        wram.write_u8(0x7E0001, 0x30).unwrap(); // clear M and X bits
        cpu.step(&mut wram).unwrap();

        assert!(
            cpu.p.contains(CpuFlags::MEMORY_8BIT),
            "emulation mode must force 8-bit accumulator width even after REP clears M"
        );
        assert!(
            cpu.p.contains(CpuFlags::INDEX_8BIT),
            "emulation mode must force 8-bit index width even after REP clears X"
        );
        assert_eq!(cpu.x, 0x0034, "forcing 8-bit index width must truncate X's high byte");
        assert_eq!(cpu.y, 0x0078, "forcing 8-bit index width must truncate Y's high byte");
    }

    #[test]
    fn plp_forces_8bit_registers_when_emulation_mode_is_active() {
        // Regression test: PLP restores P from whatever was pushed onto
        // the stack, which could be a 16-bit-widths byte from code that
        // ran while the CPU was briefly native. Pulling that back while
        // E is set must not leave the CPU in a hardware-impossible
        // 16-bit-registers-in-emulation-mode state.
        let mut cpu = Cpu::new(); // starts in emulation mode (e = true)
        let mut wram = Wram::new();
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.x = 0x1234;
        cpu.y = 0x5678;
        cpu.sp = 0x01FF;
        assert!(cpu.e, "test assumes the CPU starts in emulation mode");

        // Push a status byte with M and X both clear (16-bit request).
        wram.write_u8(0x7E01FF, 0x00).unwrap();
        cpu.sp = 0x01FE;

        wram.write_u8(0x7E0000, 0x28).unwrap(); // PLP
        cpu.step(&mut wram).unwrap();

        assert!(
            cpu.p.contains(CpuFlags::MEMORY_8BIT),
            "emulation mode must force 8-bit accumulator width even after PLP pulls M=0"
        );
        assert!(
            cpu.p.contains(CpuFlags::INDEX_8BIT),
            "emulation mode must force 8-bit index width even after PLP pulls X=0"
        );
        assert_eq!(cpu.x, 0x0034, "forcing 8-bit index width must truncate X's high byte");
        assert_eq!(cpu.y, 0x0078, "forcing 8-bit index width must truncate Y's high byte");
    }

    #[test]
    fn rti_forces_8bit_registers_when_emulation_mode_is_active() {
        // Regression test: RTI restores P from the interrupt stack frame.
        // If that frame's status byte has M/X clear (e.g. corrupted, or
        // from a mismatched native-mode push), returning while E is set
        // must not leave 16-bit registers active in emulation mode.
        let mut cpu = Cpu::new(); // starts in emulation mode (e = true)
        let mut wram = Wram::new();
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.x = 0x1234;
        cpu.y = 0x5678;

        // Emulation-mode interrupt frame is 3 bytes: P, PCL, PCH (pulled
        // low address first per stack_addr/pull_stack convention below).
        cpu.sp = 0x01FC;
        wram.write_u8(0x7E01FD, 0x00).unwrap(); // P with M=0, X=0
        wram.write_u8(0x7E01FE, 0x34).unwrap(); // PCL
        wram.write_u8(0x7E01FF, 0x12).unwrap(); // PCH -> return PC = 0x1234

        wram.write_u8(0x7E0000, 0x40).unwrap(); // RTI
        cpu.step(&mut wram).unwrap();

        assert!(
            cpu.p.contains(CpuFlags::MEMORY_8BIT),
            "emulation mode must force 8-bit accumulator width even after RTI pulls M=0"
        );
        assert!(
            cpu.p.contains(CpuFlags::INDEX_8BIT),
            "emulation mode must force 8-bit index width even after RTI pulls X=0"
        );
        assert_eq!(cpu.x, 0x0034, "forcing 8-bit index width must truncate X's high byte");
        assert_eq!(cpu.y, 0x0078, "forcing 8-bit index width must truncate Y's high byte");
        assert_eq!(cpu.pc, 0x1234, "RTI must still restore PC correctly");
    }

    #[test]
    fn cop_dispatches_through_its_own_vector_not_brks() {
        // Regression test: COP (0x02) previously had no implementation and
        // fell through to the unimplemented-opcode error path. It must
        // push the same return-context frame as BRK, then jump through
        // its OWN vector ($00FFE4 native / $00FFF4 emulation) rather than
        // BRK/IRQ's ($00FFEE native / $00FFFE emulation).
        let mut cpu = Cpu::new();
        let mut bus = VectorTestBus::new();
        cpu.e = false; // native mode, so PB is also pushed
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.sp = 0x1FFF;
        cpu.p.insert(CpuFlags::DECIMAL);
        cpu.p.remove(CpuFlags::IRQ_DISABLE);

        // COP's native vector at $00FFE4/$00FFE5 -> jump to $9ABC in bank 0.
        bus.write_u8(0x00FFE4, 0xBC).unwrap();
        bus.write_u8(0x00FFE5, 0x9A).unwrap();
        // BRK/IRQ's native vector at $00FFEE/$00FFEF -> a decoy target
        // that must NOT be used by COP.
        bus.write_u8(0x00FFEE, 0xFF).unwrap();
        bus.write_u8(0x00FFEF, 0xFF).unwrap();

        bus.write_u8(0x7E0000, 0x02).unwrap(); // COP
        bus.write_u8(0x7E0001, 0x00).unwrap(); // signature byte (ignored)
        let cycles = cpu.step(&mut bus).unwrap();

        assert_eq!(cycles, 7, "COP costs 7 cycles, same as BRK");
        assert_eq!(cpu.pc, 0x9ABC, "COP must dispatch through its own vector, not BRK/IRQ's");
        assert_eq!(cpu.pb, 0x00, "COP must clear PB to bank 0 like BRK");
        assert!(!cpu.p.contains(CpuFlags::DECIMAL), "COP must clear the Decimal flag like BRK");
        assert!(cpu.p.contains(CpuFlags::IRQ_DISABLE), "COP must set IRQ_DISABLE like BRK");

        // Verify the full native-mode push frame (PB, PCH, PCL, P) landed
        // correctly, matching BRK's push shape.
        assert_eq!(bus.read_u8(0x7E1FFF).unwrap(), 0x7E, "pushed PB");
        assert_eq!(bus.read_u8(0x7E1FFE).unwrap(), 0x00, "pushed PCH (PC was 0x0002 after 2 fetches)");
        assert_eq!(bus.read_u8(0x7E1FFD).unwrap(), 0x02, "pushed PCL");
    }

    #[test]
    fn cop_uses_emulation_mode_vector_distinct_from_native() {
        // Regression test: emulation-mode COP must read $00FFF4/$00FFF5,
        // not the native-mode $00FFE4/$00FFE5 pair, and must not push PB.
        let mut cpu = Cpu::new(); // starts in emulation mode (e = true)
        let mut bus = VectorTestBus::new();
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.sp = 0x01FF;

        bus.write_u8(0x00FFF4, 0x00).unwrap();
        bus.write_u8(0x00FFF5, 0x40).unwrap(); // -> PC = 0x4000
        // Decoy at the native vector that must not be used.
        bus.write_u8(0x00FFE4, 0xFF).unwrap();
        bus.write_u8(0x00FFE5, 0xFF).unwrap();

        bus.write_u8(0x7E0000, 0x02).unwrap(); // COP
        bus.write_u8(0x7E0001, 0x00).unwrap();
        cpu.step(&mut bus).unwrap();

        assert_eq!(cpu.pc, 0x4000, "emulation-mode COP must use the $00FFF4 vector");
    }

    #[test]
    fn cpu_mvn_reports_true_cycle_cost_for_the_whole_transfer() {
        // Regression test: op_mvn/op_mvp used to always return a flat
        // Ok(7) no matter how many bytes were moved. Real hardware spends
        // 7 cycles per byte.
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.a = 9; // count = A + 1 = 10 bytes
        cpu.x = 0x2000;
        cpu.y = 0x3000;

        wram.write_u8(0x7E0000, 0x54).unwrap(); // MVN srcbank,destbank
        wram.write_u8(0x7E0001, 0x7E).unwrap();
        wram.write_u8(0x7E0002, 0x7E).unwrap();
        for i in 0..10u32 {
            wram.write_u8(0x7E2000 + i, 0xAA).unwrap();
        }

        let cycles = cpu.step(&mut wram).unwrap();
        assert_eq!(cycles, 70, "moving 10 bytes must cost 7 cycles/byte = 70, not a flat 7");
        assert_eq!(cpu.x, 0x200A);
        assert_eq!(cpu.y, 0x300A);
    }

    #[test]
    fn cpu_mvn_large_transfer_cycle_cost_exceeds_u8_range() {
        // A transfer of more than 36 bytes already costs more than 255
        // cycles, which is the concrete case the old flat-Ok(7) bug (and
        // the u8 return type it was wedged into) could never represent.
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.a = 99; // count = 100 bytes -> 700 cycles
        cpu.x = 0x2000;
        cpu.y = 0x3000;

        wram.write_u8(0x7E0000, 0x54).unwrap(); // MVN srcbank,destbank
        wram.write_u8(0x7E0001, 0x7E).unwrap();
        wram.write_u8(0x7E0002, 0x7E).unwrap();
        for i in 0..100u32 {
            wram.write_u8(0x7E2000 + i, 0x00).unwrap();
        }

        let cycles = cpu.step(&mut wram).unwrap();
        assert_eq!(cycles, 700);
    }

    #[test]
    fn cpu_mvn_operand_bytes_are_destination_bank_then_source_bank() {
        // Pins the machine-code operand ORDER with raw hand-written bytes
        // (not an assembler helper): per the 65816 spec the byte after the
        // MVN/MVP opcode is the DESTINATION bank and the following byte is
        // the SOURCE bank -- the reverse of the `MVN src,dst` mnemonic.
        // These were read swapped, which silently broke every cross-bank
        // block move (same-bank moves, like the two tests above, could
        // never catch it).
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.a = 3; // move 4 bytes
        cpu.x = 0x2000; // source offset
        cpu.y = 0x3000; // destination offset

        // MVN with dest bank $7F, source bank $7E: raw bytes 54 7F 7E.
        wram.write_u8(0x7E0000, 0x54).unwrap();
        wram.write_u8(0x7E0001, 0x7F).unwrap(); // destination bank
        wram.write_u8(0x7E0002, 0x7E).unwrap(); // source bank
        for i in 0..4u32 {
            wram.write_u8(0x7E2000 + i, 0xA0 + i as u8).unwrap(); // real source
            wram.write_u8(0x7F2000 + i, 0x11).unwrap(); // decoy at swapped source
        }

        cpu.step(&mut wram).unwrap();

        for i in 0..4u32 {
            assert_eq!(
                wram.read_u8(0x7F3000 + i).unwrap(),
                0xA0 + i as u8,
                "byte {} must be copied FROM $7E:2000+ TO $7F:3000+ -- a swapped read \
                 would have copied the $11 decoys from $7F:2000+ instead",
                i
            );
        }
        assert_eq!(cpu.db, 0x7F, "DB must be left holding the destination bank");
    }

    #[test]
    fn cpu_mvp_operand_bytes_are_destination_bank_then_source_bank() {
        // Same order pin as the MVN test, for the decrementing variant.
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.a = 3; // move 4 bytes
        cpu.x = 0x2003; // source END offset (MVP decrements)
        cpu.y = 0x3003; // destination END offset

        // MVP with dest bank $7F, source bank $7E: raw bytes 44 7F 7E.
        wram.write_u8(0x7E0000, 0x44).unwrap();
        wram.write_u8(0x7E0001, 0x7F).unwrap(); // destination bank
        wram.write_u8(0x7E0002, 0x7E).unwrap(); // source bank
        for i in 0..4u32 {
            wram.write_u8(0x7E2000 + i, 0xB0 + i as u8).unwrap();
            wram.write_u8(0x7F2000 + i, 0x22).unwrap(); // decoy
        }

        cpu.step(&mut wram).unwrap();

        for i in 0..4u32 {
            assert_eq!(
                wram.read_u8(0x7F3000 + i).unwrap(),
                0xB0 + i as u8,
                "MVP byte {} must be copied FROM $7E TO $7F",
                i
            );
        }
        assert_eq!(cpu.db, 0x7F);
    }

    #[test]
    fn cpu_wake_if_interrupt_pending_clears_wai_even_when_irq_disabled() {
        // Regression test: WAI only ever cleared `waiting_for_interrupt`
        // inside nmi()/irq(), and callers only invoke irq() when
        // IRQ_DISABLE is clear -- so a WAI executed with I set (or right
        // before an SEI) used to hang forever even though real hardware
        // wakes on any asserted interrupt line regardless of I.
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.p.insert(CpuFlags::IRQ_DISABLE);

        wram.write_u8(0x7E0000, 0xCB).unwrap(); // WAI
        wram.write_u8(0x7E0001, 0xEA).unwrap(); // NOP
        cpu.step(&mut wram).unwrap();
        assert!(cpu.waiting_for_interrupt, "WAI must suspend fetch");

        // An interrupt line asserted while I is set must not dispatch a
        // handler, but must still wake WAI.
        cpu.wake_if_interrupt_pending(true);
        assert!(
            !cpu.waiting_for_interrupt,
            "an asserted interrupt line must wake WAI even though I is set"
        );

        let pc_before = cpu.pc;
        let cycles = cpu.step(&mut wram).unwrap();
        assert_eq!(cycles, 2, "fetch must resume normally and execute the NOP");
        assert_eq!(cpu.pc, pc_before.wrapping_add(1));
    }

    #[test]
    fn cpu_wake_if_interrupt_pending_is_a_noop_when_nothing_pending() {
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;

        wram.write_u8(0x7E0000, 0xCB).unwrap(); // WAI
        cpu.step(&mut wram).unwrap();
        assert!(cpu.waiting_for_interrupt);

        cpu.wake_if_interrupt_pending(false);
        assert!(cpu.waiting_for_interrupt, "no interrupt line asserted, so WAI must keep waiting");
    }

    #[test]
    fn jmp_indirect_0x6c_reads_pointer_from_bank_0_not_db() {
        // Real 65816 hardware always fetches the JMP ($addr) pointer from
        // bank 0, regardless of DB -- a 6502-inherited quirk. Set DB to a
        // bank that isn't mapped in this test's Wram-only bus, so the old
        // (buggy) `db`-based address would hit an invalid-address error
        // instead of silently succeeding.
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.db = 0x01;

        wram.write_u8(0x7E0000, 0x6C).unwrap(); // JMP ($0010)
        wram.write_u8(0x7E0001, 0x10).unwrap();
        wram.write_u8(0x7E0002, 0x00).unwrap();
        // Pointer target lives at bank-0 $0010/$0011, which mirrors WRAM's
        // low 8KB -- i.e. the same bytes as $7E0010/$7E0011.
        wram.write_u8(0x7E0010, 0x34).unwrap();
        wram.write_u8(0x7E0011, 0x12).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.pc, 0x1234, "JMP ($addr) must read its pointer from bank 0, not DB");
    }

    #[test]
    fn jmp_indirect_x_0x7c_reads_pointer_from_pb_not_db() {
        // JMP ($addr,X) is a same-bank computed jump: its pointer must be
        // fetched from the current Program Bank (PB), not DB. Set DB to a
        // bank that isn't mapped in this test's Wram-only bus, so the old
        // (buggy) `db`-based address would hit an invalid-address error
        // instead of silently succeeding.
        let mut cpu = Cpu::new();
        let mut wram = Wram::new();
        cpu.pb = 0x7E;
        cpu.pc = 0x0000;
        cpu.db = 0x01;
        cpu.x = 0x0005;

        wram.write_u8(0x7E0000, 0x7C).unwrap(); // JMP ($0010,X)
        wram.write_u8(0x7E0001, 0x10).unwrap();
        wram.write_u8(0x7E0002, 0x00).unwrap();
        // Effective pointer is $0010 + X ($0005) = $0015, read from PB ($7E).
        wram.write_u8(0x7E0015, 0x78).unwrap();
        wram.write_u8(0x7E0016, 0x56).unwrap();

        cpu.step(&mut wram).unwrap();
        assert_eq!(cpu.pc, 0x5678, "JMP ($addr,X) must read its pointer from PB, not DB");
    }
}