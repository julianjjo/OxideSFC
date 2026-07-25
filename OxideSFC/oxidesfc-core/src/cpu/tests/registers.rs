//! Register and flag basics: initial state, the flag opcodes, N/Z updates
//! at both widths, and the accumulator/stack transfer pairs.

use crate::bus::MemoryBus;
use crate::cpu::{Cpu, CpuFlags};
use crate::wram::Wram;

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

