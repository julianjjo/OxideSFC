//! Emulation vs native mode: the M/X width flags, what forces registers back
//! to 8 bits, and the PHP/PLP/REP/RTI paths that must not corrupt widths.

use crate::bus::MemoryBus;
use crate::cpu::{Cpu, CpuFlags};
use crate::wram::Wram;

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

