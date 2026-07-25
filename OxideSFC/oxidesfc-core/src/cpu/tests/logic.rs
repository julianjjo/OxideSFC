//! Bitwise logic instructions.

use crate::bus::MemoryBus;
use crate::cpu::{Cpu, CpuFlags};
use crate::wram::Wram;

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

