//! Save-state round-tripping and rejection of incompatible snapshots.

use super::common::tick_dots;
use crate::bus::{MemoryBus, SystemBus};
use crate::error::EmulationError;

#[test]
fn snapshot_round_trips_cpu_bus_and_memory_state() {
    let mut cpu = crate::cpu::Cpu::new();
    let mut bus = SystemBus::new();
    bus.load_cartridge(vec![0x42; 0x80000]).unwrap();

    // Scatter distinctive state across subsystems.
    cpu.a = 0x1234;
    cpu.pc = 0xABCD;
    cpu.e = false;
    bus.write_u8(0x7E1234, 0x99).unwrap(); // WRAM
    bus.write_u8(0x002116, 0x34).unwrap(); // VMADD
    bus.write_u8(0x002117, 0x12).unwrap();
    bus.write_u8(0x002118, 0x77).unwrap(); // VRAM byte (also bumps VMADD)
    bus.write_u8(0x002105, 0x07).unwrap(); // BGMODE = 7
    bus.write_u8(0x004202, 0x10).unwrap();
    bus.write_u8(0x004203, 0x10).unwrap(); // RDMPY = 0x100
    bus.write_u8(0x002140, 0x5A).unwrap(); // CPU->APU port 0
    tick_dots(&mut bus, 5 * 340 + 123); // advance PPU counters

    let snapshot = crate::state::save_snapshot(&cpu, &bus);

    // Wreck everything, then restore.
    let mut cpu2 = crate::cpu::Cpu::new();
    let mut bus2 = SystemBus::new();
    bus2.load_cartridge(vec![0x42; 0x80000]).unwrap();
    crate::state::load_snapshot(&mut cpu2, &mut bus2, &snapshot).unwrap();

    assert_eq!(cpu2.a, 0x1234);
    assert_eq!(cpu2.pc, 0xABCD);
    assert!(!cpu2.e);
    assert_eq!(bus2.read_u8(0x7E1234).unwrap(), 0x99, "WRAM must round-trip");
    assert_eq!(bus2.ppu_ref().vram_ref().read(0x1234 * 2), 0x77, "VRAM must round-trip");
    assert_eq!(bus2.ppu_registers().bgmode, 0x07, "PPU registers must round-trip");
    assert_eq!(bus2.read_u8(0x004216).unwrap(), 0x00, "RDMPY low byte");
    assert_eq!(bus2.read_u8(0x004217).unwrap(), 0x01, "RDMPY high byte");
    assert_eq!(bus2.apu_ref().cpu_to_apu_port(0), 0x5A, "APU port latch must round-trip");
    assert_eq!(bus2.ppu_ref().scanline(), bus.ppu_ref().scanline(), "PPU timing must round-trip");
    assert_eq!(bus2.ppu_ref().h_counter(), bus.ppu_ref().h_counter());
}

#[test]
fn snapshot_with_bad_magic_or_wrong_sram_size_is_rejected() {
    let mut cpu = crate::cpu::Cpu::new();
    let mut bus = SystemBus::new();
    bus.load_cartridge(vec![0x42; 0x80000]).unwrap();
    let mut snapshot = crate::state::save_snapshot(&cpu, &bus);

    // Bad magic.
    let mut corrupted = snapshot.clone();
    corrupted[0] = b'X';
    assert!(matches!(
        crate::state::load_snapshot(&mut cpu, &mut bus, &corrupted),
        Err(EmulationError::InvalidSaveState(_))
    ));

    // Truncation anywhere must error, not panic.
    snapshot.truncate(snapshot.len() / 2);
    assert!(matches!(
        crate::state::load_snapshot(&mut cpu, &mut bus, &snapshot),
        Err(EmulationError::InvalidSaveState(_))
    ));
}

