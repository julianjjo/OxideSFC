//! Shifts and rotates on the accumulator.

use crate::bus::MemoryBus;
use crate::cpu::{Cpu, CpuFlags};
use crate::wram::Wram;

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

