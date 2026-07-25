//! SPC700 execution: the opcode set, addressing modes, flags, timers,
//! reset, and the IPL boot ROM's real upload handshake.

use super::common::isolated_spc700;
use crate::apu::Apu;

/// Enough APU cycles for the real SPC700 IPL ROM to clear its 239-byte
/// page-0 RAM loop and reach the ready handshake (well under 100
/// instructions); generous headroom for slower paths through it too.
const ENOUGH_CYCLES_FOR_IPL_READY: u32 = 10_000;

#[test]
fn spc700_charges_each_instruction_its_real_cycle_cost() {
    // Regression guard: `Apu::tick` used to run one *instruction* per
    // SPC700 cycle and discard `step()`'s returned cycle count, so the
    // SPC700 executed at roughly 3.5x its real throughput. Tempo still
    // came out right (timers and the DSP divider were calibrated in
    // those instruction units), which is why it went unnoticed -- but
    // drivers whose timing depends on how much work fits between two
    // timer ticks saw a machine far faster than hardware.
    //
    // A straight run of `INC A` ($BC, 2 cycles) advances the program
    // counter exactly one byte per instruction and never branches, so
    // the PC delta counts instructions directly.
    let mut apu = Apu::new();
    for addr in 0x0200u16..0x1200 {
        apu.write_ram(addr, 0xBC);
    }
    apu.spc700.pc = 0x0200;

    // 5244 pacing-unit cycles = 2000 SPC700 cycles (unit rate is
    // 2,684,659 Hz against the SPC700's 1,024,000 Hz).
    apu.tick(5244);

    let instructions = apu.spc700.pc - 0x0200;
    assert!(
        (990..=1010).contains(&instructions),
        "2000 SPC700 cycles of a 2-cycle instruction must execute ~1000 \
         instructions; the old one-instruction-per-cycle model ran ~2000. \
         Got {}",
        instructions
    );
}

#[test]
fn spc700_timers_tick_at_the_real_8khz_rate() {
    // The SPC700 runs on its own 24.576MHz crystal at 1.024MHz --
    // NOT an integer fraction of the main clock. Timer 0/1's stage-1
    // divider advances every 128 SPC700 cycles = exactly 8000/sec, and
    // the music driver's tempo hangs directly off this rate. The old
    // `unit / 3` conversion stepped the SPC700 at 894.9kHz, making the
    // timers (and therefore all music) run 12.6% slow.
    //
    // One emulated second: 8000 stage-1 increments. With target 0
    // (=256), the 4-bit counter increments floor(8000/256) = 31 times
    // (31 & 0x0F = 15) and the divider is left at 8000 % 256 = 64.
    let mut apu = Apu::new();
    apu.spc700.timer_enable[0] = true;
    apu.spc700.timer_target[0] = 0;
    apu.spc700.timer_divider[0] = 0;
    apu.spc700.timer_counter[0] = 0;
    apu.spc700.timer_prescaler[0] = 0;

    const ONE_SECOND_OF_UNIT_CYCLES: u32 = 2_684_659;
    const CHUNK: u32 = 977; // deliberately awkward chunk size
    let mut remaining = ONE_SECOND_OF_UNIT_CYCLES;
    while remaining > 0 {
        let step = remaining.min(CHUNK);
        apu.tick(step);
        remaining -= step;
    }

    assert_eq!(
        apu.spc700.timer_divider[0], 64,
        "timer 0 stage-1 must have advanced exactly 8000 times in one emulated second \
         (8000 %% 256 = 64) -- the old 894.9kHz SPC700 clock left it at 6991 %% 256 = 79"
    );
    assert_eq!(apu.spc700.timer_counter[0], 15, "floor(8000/256) = 31 -> 31 & 0x0F = 15");
}

#[test]
fn test_real_spc700_execution_reaches_the_ipl_ready_handshake() {
    // Unlike before, $AA/$BB are no longer hardcoded -- they only
    // appear once the real, verified 64-byte IPL ROM (see `IPL_ROM`)
    // actually executes far enough to write them via genuine SPC700
    // instructions (`MOV $F4,#$AA` / `MOV $F5,#$BB`).
    let mut apu = Apu::new();
    assert_eq!(apu.read_port(0), 0x00, "nothing has run yet right after construction");

    apu.tick(ENOUGH_CYCLES_FOR_IPL_READY);

    assert_eq!(apu.spc700_halted(), None, "must not hit an opcode outside the validated subset getting here");
    assert_eq!(apu.read_port(0), 0xAA);
    assert_eq!(apu.read_port(1), 0xBB);
}

#[test]
fn test_real_spc700_ignores_stray_writes_before_seeing_the_cc_sentinel() {
    // The real IPL ROM's ready loop ("CMP $F4,#$CC / BNE -") only ever
    // reacts to the literal $CC value -- writes of anything else (e.g.
    // unrelated boot code touching $2140 while clearing hardware
    // registers) just fail the comparison and the loop keeps spinning,
    // never touching the ready signal.
    let mut apu = Apu::new();
    apu.tick(ENOUGH_CYCLES_FOR_IPL_READY);
    assert_eq!(apu.read_port(0), 0xAA);

    apu.write_port(0, 0x00); // looks like e.g. STZ $2140, not the real handshake
    apu.tick(1_000);

    assert_eq!(apu.read_port(0), 0xAA, "a stray write must not disturb the ready signal");
}

#[test]
fn test_real_spc700_executes_the_first_upload_command_and_echoes_it() {
    // Drives the real, executing SPC700 through the verified handshake
    // sequence: address setup on APUIO2/APUIO3, a flag on APUIO1, then
    // the $CC sentinel on APUIO0 -- and confirms the IPL ROM's own
    // "Start:" code (MOVW YA,$F6 / MOVW $00,YA / MOVW YA,$F4 / MOV
    // $F4,A / ...) actually runs and echoes $CC back, proving real
    // instruction execution drives this, not a scripted response.
    let mut apu = Apu::new();
    apu.tick(ENOUGH_CYCLES_FOR_IPL_READY);
    assert_eq!(apu.read_port(0), 0xAA);

    apu.write_port(2, 0x34); // target address low
    apu.write_port(3, 0x12); // target address high
    apu.write_port(1, 0x01); // nonzero -> "upload starts here", not execute
    apu.write_port(0, 0xCC);
    apu.tick(200);

    assert_eq!(apu.spc700_halted(), None);
    assert_eq!(apu.read_port(0), 0xCC, "the real IPL code must echo the command back");
    assert_eq!(apu.read_ram(0x0000), 0x34, "MOVW $00,YA must have staged the address's low byte");
    assert_eq!(apu.read_ram(0x0001), 0x12, "...and its high byte");
}

#[test]
fn test_real_spc700_executes_the_execute_command_and_jumps() {
    // flag=0 on APUIO1 means "jump to this address" rather than
    // "upload more data here" -- confirms the real IPL code's mode
    // check (MOV A,Y / MOV X,A / BNE Trans / JMP [$0000+X]) actually
    // transfers control to the requested address.
    let mut apu = Apu::new();
    apu.tick(ENOUGH_CYCLES_FOR_IPL_READY);

    apu.write_port(2, 0x00);
    apu.write_port(3, 0x03);
    apu.write_port(1, 0x00); // flag = 0 -> execute
    apu.write_port(0, 0xCC);

    // Tick in small increments and stop the instant PC reaches the
    // jump target -- the target address ($0300) is uninitialized RAM
    // (0x00 = NOP, a real valid opcode per the SPC700 instruction
    // chart, not garbage), so ticking further would just run NOPs and
    // advance PC past it.
    let mut reached_target = false;
    for _ in 0..50 {
        if apu.spc700().pc == 0x0300 {
            reached_target = true;
            break;
        }
        apu.tick(3);
    }

    assert!(reached_target, "the real IPL code must jump to the requested address; PC stuck at {:04X}", apu.spc700().pc);
    assert_eq!(apu.spc700_halted(), None);
}

#[test]
fn mov_a_indirect_dp_plus_y_reads_the_pointer_at_dp_then_indexes_by_y() {
    // Opcode 0xF7: MOV A,[dp]+Y. Unlike 0xE7 (MOV A,[dp+X], which
    // indexes the *direct-page fetch* by X before reading the
    // pointer), this reads the 16-bit pointer straight from `dp` and
    // adds Y to the *resulting address* afterward -- the SPC700
    // analogue of 6502/65816 "(dp),Y". Missing until now: it halted
    // the real uploaded sound engine's driver partway through, right
    // where hardware would start actually triggering notes.
    let mut spc = isolated_spc700();

    spc.write_mem(0x0200, 0xF7); // MOV A,[$10]+Y
    spc.write_mem(0x0201, 0x10);
    spc.write_mem(0x0010, 0x00); // pointer at dp $10/$11 = $3000
    spc.write_mem(0x0011, 0x30);
    spc.write_mem(0x3005, 0x99); // $3000 + Y(5) = $3005

    spc.pc = 0x0200;
    spc.y = 5;
    spc.step();

    assert_eq!(spc.a, 0x99);
    assert_eq!(spc.pc, 0x0202);
    assert_eq!(spc.halted, None);
}

#[test]
fn mov_y_dp_plus_x_reads_direct_page_indexed_by_x() {
    // Opcode 0xFB: MOV Y,dp+X. Missing until now for the same reason
    // as 0xF7 -- found by running the real ROM's SPC700 driver far
    // enough to reach it.
    let mut spc = isolated_spc700();

    spc.write_mem(0x0200, 0xFB); // MOV Y,$10+X
    spc.write_mem(0x0201, 0x10);
    spc.write_mem(0x0015, 0x77); // dp $10 + X(5) = $15

    spc.pc = 0x0200;
    spc.x = 5;
    spc.step();

    assert_eq!(spc.y, 0x77);
    assert_eq!(spc.pc, 0x0202);
    assert_eq!(spc.halted, None);
}

#[test]
fn mov_dp_dp_respects_the_p_flag_for_both_source_and_destination() {
    // Regression guard for fix #3: opcode 0xFA (MOV dp,dp) used to
    // cast its two direct-page operand bytes straight to u16 and
    // access page $00xx unconditionally, ignoring PSW.P -- unlike
    // every sibling direct-page opcode (see `dp_addr`'s doc comment).
    // With P set (SETP), both the source and destination must resolve
    // to page $01xx instead of $00xx.
    let mut spc = isolated_spc700();

    // Program: SETP; MOV $10,$20  (0xFA fetches src then dst, per the
    // existing implementation's own operand order).
    spc.write_mem(0x0200, 0x40); // SETP
    spc.write_mem(0x0201, 0xFA); // MOV dp,dp
    spc.write_mem(0x0202, 0x20); // src dp = $20
    spc.write_mem(0x0203, 0x10); // dst dp = $10

    // Seed page $01 (P=1 effective addresses) with a distinct value at
    // the source, and page $00 with a different value at the same
    // nominal offset, so the test fails loudly if P is ignored.
    spc.write_mem(0x0120, 0x77); // real source once P=1: $0120
    spc.write_mem(0x0020, 0x99); // decoy: what a P-ignoring read would see

    spc.pc = 0x0200;
    spc.step(); // SETP
    assert!(spc.psw.p, "SETP must have set P");
    spc.step(); // MOV dp,dp

    assert_eq!(spc.halted, None);
    assert_eq!(spc.read_mem(0x0110), 0x77, "with P=1, the destination must land at $0110 (page $01xx), carrying the value read from the real ($0120) source");
    assert_eq!(spc.read_mem(0x0010), 0x00, "page $00xx's destination slot must be untouched when P=1");
}

#[test]
fn mov_dp_dp_still_uses_page_zero_when_p_is_clear() {
    // Complementary case: with P clear (the default), behavior must be
    // unchanged from before -- both operands resolve to page $00xx.
    let mut spc = isolated_spc700();
    spc.write_mem(0x0200, 0xFA); // MOV dp,dp (P clear by default)
    spc.write_mem(0x0201, 0x20); // src dp = $20
    spc.write_mem(0x0202, 0x10); // dst dp = $10
    spc.write_mem(0x0020, 0x55);

    spc.pc = 0x0200;
    assert!(!spc.psw.p);
    spc.step();

    assert_eq!(spc.halted, None);
    assert_eq!(spc.read_mem(0x0010), 0x55);
}

#[test]
fn spc700_reset_clears_timers_and_dsp_address_latch() {
    // Regression guard for fix #6: `Spc700::reset()` restored
    // registers/PC/PSW but left the timer hardware (enable bits,
    // targets, dividers, output counters) and the $F2 DSP-register-
    // address latch at their pre-reset values -- real hardware zeroes
    // both on reset.
    let mut spc = isolated_spc700();

    // Arm all three timers with nonzero targets and let them run long
    // enough to accumulate nonzero divider/counter state.
    spc.write_mem(0xFA, 0x01); // timer 0 target
    spc.write_mem(0xFB, 0x01); // timer 1 target
    spc.write_mem(0xFC, 0x01); // timer 2 target
    spc.write_mem(0xF1, 0x07); // enable all three timers

    for _ in 0..2000 {
        spc.tick_timers();
    }
    // Sanity: at least the fast timer 2 (8x prescaler) must have
    // produced a nonzero readable counter by now.
    assert_ne!(spc.read_mem(0xFF), 0, "timer 2's counter must have advanced before reset (sanity check)");

    // Re-arm afterward since reading $FD-$FF above resets that
    // specific counter to 0 as a side effect -- set the DSP address
    // latch and re-enable timers with fresh nonzero state to confirm
    // reset (not an incidental read) is what clears them.
    spc.write_mem(0xF2, 0x0C); // select DSP register $0C (MVOLL)
    assert_eq!(spc.read_mem(0xF2), 0x0C, "sanity: latch must hold what was just written");
    spc.write_mem(0xF1, 0x07);
    for _ in 0..2000 {
        spc.tick_timers();
    }

    spc.reset();

    assert_eq!(spc.read_mem(0xF1), 0x00, "reset must clear the timer control byte");
    assert_eq!(spc.read_mem(0xFD), 0x00, "reset must clear timer 0's output counter");
    assert_eq!(spc.read_mem(0xFE), 0x00, "reset must clear timer 1's output counter");
    assert_eq!(spc.read_mem(0xFF), 0x00, "reset must clear timer 2's output counter");
    assert_eq!(spc.read_mem(0xF2), 0x00, "reset must clear the DSP register-address latch");

    // And timers must stay at zero afterward (disabled), not silently
    // resume ticking from leftover prescaler/divider state.
    for _ in 0..2000 {
        spc.tick_timers();
    }
    assert_eq!(spc.read_mem(0xFF), 0x00, "a disabled-by-reset timer must not resume advancing on its own");
}

#[test]
fn every_spc700_opcode_executes_without_halting_except_stop() {
    // Full-instruction-set coverage pin: with all-zero RAM (so every
    // operand byte is 0x00), stepping a fresh SPC700 onto each of the
    // 256 opcodes must execute it -- the only opcode allowed to set
    // `halted` is 0xFF (STOP), which genuinely halts real hardware.
    // If any dispatch arm (or guard predicate) is removed or broken,
    // that opcode falls through to the defensive halt arm and this
    // test names it exactly.
    for opcode in 0..=255u8 {
        let mut spc = isolated_spc700();
        spc.write_mem(0x0200, opcode);
        spc.pc = 0x0200;
        spc.step();
        if opcode == 0xFF {
            assert_eq!(spc.halted, Some(0xFF), "STOP must halt");
        } else {
            assert_eq!(
                spc.halted, None,
                "opcode 0x{:02X} must execute without halting",
                opcode
            );
        }
    }
}

#[test]
fn odd_numbered_tcalls_jump_through_their_descending_vectors() {
    // Regression guard: the TCALL guard used to match `& 0x1F == 0x01`,
    // which silently missed TCALL 1/3/5/7/9/11/13/15 (opcodes
    // $11/$31/.../$F1). TCALL 1's vector is $FFDC ($FFDE - 2*1).
    let mut spc = isolated_spc700();
    spc.write_mem(0xFFDC, 0x34);
    spc.write_mem(0xFFDD, 0x12);
    spc.write_mem(0x0200, 0x11); // TCALL 1
    spc.pc = 0x0200;
    spc.sp = 0xFF;
    spc.step();
    assert_eq!(spc.halted, None);
    assert_eq!(spc.pc, 0x1234, "TCALL 1 must jump through the $FFDC vector");
    // Return address ($0201) pushed high-then-low.
    assert_eq!(spc.read_mem(0x01FF), 0x02);
    assert_eq!(spc.read_mem(0x01FE), 0x01);
}

#[test]
fn alu_dp_dp_and_dp_imm_and_ix_iy_store_results_with_flags() {
    // ADC dp,dp: src-then-dst operand order, result to dst.
    let mut spc = isolated_spc700();
    spc.write_mem(0x0010, 0x22); // src
    spc.write_mem(0x0011, 0x33); // dst
    spc.write_mem(0x0200, 0x89); // ADC dp,dp
    spc.write_mem(0x0201, 0x10); // src dp
    spc.write_mem(0x0202, 0x11); // dst dp
    spc.pc = 0x0200;
    spc.psw.c = false;
    spc.step();
    assert_eq!(spc.read_mem(0x0011), 0x55, "ADC dp,dp must store dst+src into dst");
    assert!(!spc.psw.c);

    // OR dp,#imm: imm-then-dp operand order, result to dp.
    let mut spc = isolated_spc700();
    spc.write_mem(0x0020, 0x0F);
    spc.write_mem(0x0200, 0x18); // OR dp,#imm
    spc.write_mem(0x0201, 0xF0); // imm
    spc.write_mem(0x0202, 0x20); // dp
    spc.pc = 0x0200;
    spc.step();
    assert_eq!(spc.read_mem(0x0020), 0xFF);
    assert!(spc.psw.n, "N must reflect the stored result");

    // CMP (X),(Y): flags only, no store.
    let mut spc = isolated_spc700();
    spc.write_mem(0x0030, 0x40); // (X)
    spc.write_mem(0x0031, 0x50); // (Y)
    spc.write_mem(0x0200, 0x79); // CMP (X),(Y)
    spc.pc = 0x0200;
    spc.x = 0x30;
    spc.y = 0x31;
    spc.step();
    assert_eq!(spc.read_mem(0x0030), 0x40, "CMP must not store");
    assert!(!spc.psw.c, "0x40 < 0x50 must clear carry (borrow needed)");

    // SBC (X),(Y): result stored through (X).
    let mut spc = isolated_spc700();
    spc.write_mem(0x0030, 0x50);
    spc.write_mem(0x0031, 0x20);
    spc.write_mem(0x0200, 0xB9); // SBC (X),(Y)
    spc.pc = 0x0200;
    spc.x = 0x30;
    spc.y = 0x31;
    spc.psw.c = true; // no incoming borrow
    spc.step();
    assert_eq!(spc.read_mem(0x0030), 0x30, "SBC (X),(Y) must store dst-src into (X)");
    assert!(spc.psw.c, "no borrow must leave carry set");
}

#[test]
fn carry_bit_instructions_use_13_bit_address_and_3_bit_bit_operand() {
    // MOV1 C, m.b: address $0123, bit 5 -> operand word $0123 | (5<<13).
    let mut spc = isolated_spc700();
    spc.write_mem(0x0123, 1 << 5);
    let operand: u16 = 0x0123 | (5 << 13);
    spc.write_mem(0x0200, 0xAA); // MOV1 C,m.b
    spc.write_mem(0x0201, (operand & 0xFF) as u8);
    spc.write_mem(0x0202, (operand >> 8) as u8);
    spc.pc = 0x0200;
    spc.psw.c = false;
    spc.step();
    assert!(spc.psw.c, "MOV1 C,m.b must load the addressed bit into carry");

    // MOV1 m.b, C writes the carry back into the addressed bit.
    let mut spc = isolated_spc700();
    let operand: u16 = 0x0040 | (3 << 13);
    spc.write_mem(0x0200, 0xCA); // MOV1 m.b,C
    spc.write_mem(0x0201, (operand & 0xFF) as u8);
    spc.write_mem(0x0202, (operand >> 8) as u8);
    spc.pc = 0x0200;
    spc.psw.c = true;
    spc.step();
    assert_eq!(spc.read_mem(0x0040), 1 << 3, "MOV1 m.b,C must set exactly the addressed bit");

    // NOT1 m.b toggles the addressed bit in place.
    let mut spc = isolated_spc700();
    spc.write_mem(0x0055, 0xFF);
    let operand: u16 = 0x0055 | (7 << 13);
    spc.write_mem(0x0200, 0xEA); // NOT1 m.b
    spc.write_mem(0x0201, (operand & 0xFF) as u8);
    spc.write_mem(0x0202, (operand >> 8) as u8);
    spc.pc = 0x0200;
    spc.step();
    assert_eq!(spc.read_mem(0x0055), 0x7F, "NOT1 must flip only the addressed bit");
}

#[test]
fn daa_and_das_decimal_adjust_the_accumulator() {
    // 0x19 + 0x28 = 0x41 binary; DAA must correct it to 0x47 (19+28=47 BCD).
    let mut spc = isolated_spc700();
    spc.a = 0x19;
    spc.write_mem(0x0200, 0x88); // ADC A,#imm
    spc.write_mem(0x0201, 0x28);
    spc.write_mem(0x0202, 0xDF); // DAA A
    spc.pc = 0x0200;
    spc.psw.c = false;
    spc.step();
    assert_eq!(spc.a, 0x41, "sanity: binary add result before adjustment");
    assert!(spc.psw.h, "half-carry from bit 3 (9+8=17) must set H");
    spc.step();
    assert_eq!(spc.a, 0x47, "DAA must produce the BCD sum 47");

    // 0x42 - 0x15 = 0x2D binary; DAS must correct it to 0x27 (42-15=27 BCD).
    let mut spc = isolated_spc700();
    spc.a = 0x42;
    spc.write_mem(0x0200, 0xA8); // SBC A,#imm
    spc.write_mem(0x0201, 0x15);
    spc.write_mem(0x0202, 0xBE); // DAS A
    spc.pc = 0x0200;
    spc.psw.c = true; // no incoming borrow
    spc.step();
    assert_eq!(spc.a, 0x2D, "sanity: binary subtract result before adjustment");
    spc.step();
    assert_eq!(spc.a, 0x27, "DAS must produce the BCD difference 27");
}

#[test]
fn mov_indirect_dp_x_store_writes_through_the_pointer() {
    // MOV [dp+X],A (0xC7): pointer at dp+X, store A through it.
    let mut spc = isolated_spc700();
    spc.write_mem(0x0014, 0x00); // pointer low  (at $10 + X=4)
    spc.write_mem(0x0015, 0x03); // pointer high -> $0300
    spc.write_mem(0x0200, 0xC7);
    spc.write_mem(0x0201, 0x10);
    spc.pc = 0x0200;
    spc.x = 0x04;
    spc.a = 0x99;
    spc.step();
    assert_eq!(spc.read_mem(0x0300), 0x99, "MOV [dp+X],A must store through the pointer at dp+X");
}

#[test]
fn brk_pushes_pc_and_psw_then_jumps_through_ffde() {
    let mut spc = isolated_spc700();
    spc.write_mem(0xFFDE, 0x00);
    spc.write_mem(0xFFDF, 0x40); // vector -> $4000
    spc.write_mem(0x0200, 0x0F); // BRK
    spc.pc = 0x0200;
    spc.sp = 0xFF;
    spc.psw.i = true;
    spc.step();
    assert_eq!(spc.pc, 0x4000, "BRK must jump through the $FFDE vector");
    assert!(spc.psw.b, "BRK must set the Break flag");
    assert!(!spc.psw.i, "BRK must clear the Interrupt-enable flag");
    assert_eq!(spc.read_mem(0x01FF), 0x02, "pushed return PC high byte");
    assert_eq!(spc.read_mem(0x01FE), 0x01, "pushed return PC low byte");
    // Pushed PSW: I was set, B not yet set at push time.
    let pushed_psw = spc.read_mem(0x01FD);
    assert_ne!(pushed_psw & 0x04, 0, "pushed PSW must have I as it was before BRK");
}
