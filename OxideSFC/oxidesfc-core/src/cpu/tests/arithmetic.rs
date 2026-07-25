//! Comparisons, increments/decrements, and decimal-mode ADC/SBC.

use crate::bus::MemoryBus;
use crate::cpu::{Cpu, CpuFlags};
use crate::wram::Wram;

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

