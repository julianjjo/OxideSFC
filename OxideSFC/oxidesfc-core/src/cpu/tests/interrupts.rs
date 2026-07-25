//! Interrupt dispatch: COP vectors per mode, and waking from WAI.

use super::common::*;
use crate::bus::MemoryBus;
use crate::cpu::{Cpu, CpuFlags};
use crate::wram::Wram;

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

