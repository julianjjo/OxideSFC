//! Register transfers, bank pushes, and the MVN/MVP block moves.

use crate::bus::MemoryBus;
use crate::cpu::{Cpu, CpuFlags};
use crate::wram::Wram;

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

