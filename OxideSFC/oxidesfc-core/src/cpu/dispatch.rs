//! The 65816's opcode dispatch.
//!
//! One exhaustive `match` on purpose: with no wildcard arm, the compiler
//! itself proves all 256 opcodes are handled. Splitting the arms into
//! per-range helpers would give each an unreachable catch-all and throw that
//! guarantee away, so this file stays long rather than being split further.

use super::Cpu;
use crate::bus::{BusResult, MemoryBus};

impl Cpu {
    /// Dispatches a single opcode. Every handler except `op_mvn`/`op_mvp`
    /// returns its cycle cost directly as `BusResult<u8>`; those two
    /// instead stash their (potentially much larger) true cost in
    /// `self.pending_cycle_adjustment` and return `Ok(0)`, which gets
    /// folded into the widened `u32` result below. This keeps every other
    /// opcode handler's `BusResult<u8>` signature untouched.
    pub(super) fn execute(&mut self, opcode: u8, bus: &mut impl MemoryBus) -> BusResult<u32> {
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
}
