//! The memory map itself: WRAM and its mirrors, ROM/SRAM mapping, open
//! bus, and multi-byte access.

use super::common::build_lorom_with_sram;
use crate::bus::{MemoryBus, SystemBus};

#[test]
fn system_bus_new() {
    let bus = SystemBus::new();
    assert!(!bus.has_cartridge());
}

#[test]
fn system_bus_load_cartridge() {
    let mut bus = SystemBus::new();
    let rom = vec![0x42; 0x80000]; // 512KB ROM
    bus.load_cartridge(rom).unwrap();
    assert!(bus.has_cartridge());
}

#[test]
fn system_bus_load_empty_rom_fails() {
    let mut bus = SystemBus::new();
    assert!(bus.load_cartridge(vec![]).is_err());
}

#[test]
fn system_bus_wram_read_write() {
    let mut bus = SystemBus::new();
    
    // Write to WRAM
    bus.write_u8(0x7E1234, 0xAB).unwrap();
    
    // Read back
    let value = bus.read_u8(0x7E1234).unwrap();
    assert_eq!(value, 0xAB);
}

#[test]
fn system_bus_wram_mirror() {
    let mut bus = SystemBus::new();
    
    // Write to bank 0 address (WRAM mirror)
    bus.write_u8(0x1234, 0xCD).unwrap();
    
    // Read from $7E0000 mirror
    let value = bus.read_u8(0x7E1234).unwrap();
    assert_eq!(value, 0xCD);
}

#[test]
fn system_bus_wram_mirror_works_from_every_bank_not_just_bank_zero() {
    // Regression guard: $0000-$1FFF mirrors WRAM in every bank of the
    // SYSTEM group ($00-$3F and $80-$BF), not just bank $00. The bus
    // used to pass the *full* 24-bit address straight to `Wram`,
    // which only recognizes addresses literally in $7E0000-$7FFFFF or
    // literally below $10000 -- so e.g. a plain `LDA $1234` with
    // DB=$05 (bank $05, offset $1234, well within the WRAM-mirror
    // range) crashed with `InvalidAddress` instead of reading WRAM.
    // This was unreachable in early testing because nothing had
    // executed far enough to hit a non-zero-bank low-address access;
    // it became a real, repeatable crash once CPU coverage improved
    // enough to reach deeper SMW code.
    //
    // Banks $7E/$7F are deliberately excluded: they ARE WRAM itself
    // (the real, independent first/second 64KB halves), not a mirror
    // of it -- see `system_bus_wram_7e_and_7f_are_independent_not_mirrored`.
    // Banks $40-$7D are ALSO excluded: they are pure cartridge space
    // (LoROM maps SRAM at $70-$7D:$0000-$7FFF). An earlier version of
    // the bus wrongly gave them the WRAM mirror, which let SMW's
    // SaveTheGame routine ($009BB6+, `STA.L SaveData,X` = $700000+X)
    // overwrite the CPU stack at $01F5+ with save-file bytes -- the
    // RTL then popped zeros and execution escaped into WRAM/open bus.
    let mut bus = SystemBus::new();
    bus.write_u8(0x7E1234, 0xAB).unwrap();

    for bank in [0x00u32, 0x01, 0x05, 0x3F, 0x80, 0xBF] {
        let addr = (bank << 16) | 0x1234;
        assert_eq!(
            bus.read_u8(addr).unwrap(),
            0xAB,
            "bank ${:02X} offset $1234 must mirror WRAM, not crash or return something else",
            bank
        );
    }

    // And the reverse: writing through a non-zero bank's mirror must
    // land in the same underlying WRAM byte.
    bus.write_u8(0x051234, 0xCD).unwrap();
    assert_eq!(bus.read_u8(0x7E1234).unwrap(), 0xCD);

    // Banks $40-$7D must NOT reach WRAM through a phantom mirror: a
    // write to $70:1234 (LoROM SRAM space; no cartridge loaded here,
    // so it's simply ignored) must leave WRAM untouched.
    bus.write_u8(0x701234, 0x77).unwrap();
    assert_eq!(
        bus.read_u8(0x7E1234).unwrap(),
        0xCD,
        "a bank-$70 write must never land in low WRAM -- that's the SaveTheGame stack clobber"
    );
}

#[test]
fn lorom_sram_at_bank_70_is_readable_writable_and_isolated_from_wram_and_stack() {
    // End-to-end regression test for the SaveTheGame crash: with a
    // LoROM cartridge that declares SRAM (like SMW's 2KB), writes to
    // $70:0000-$7FFF must land in real SRAM, read back correctly, and
    // leave WRAM (especially the $0100-$01FF stack page) untouched.
    let mut bus = SystemBus::new();
    bus.load_cartridge(build_lorom_with_sram()).unwrap();

    // Seed the stack page area that SaveTheGame's SRAM offsets used
    // to clobber ($01F5-$01F7 held a JSL return address).
    bus.write_u8(0x0001F5, 0x9B).unwrap();
    bus.write_u8(0x0001F6, 0x51).unwrap();
    bus.write_u8(0x0001F7, 0x00).unwrap();

    // Write a save-file-like run of bytes across the same offsets in
    // SRAM (this is exactly what `STA.L SaveData,X` does).
    for x in 0x01F0u32..0x0200 {
        bus.write_u8(0x700000 + x, 0x00).unwrap();
    }
    bus.write_u8(0x700000, 0x42).unwrap();

    // SRAM reads back what was written...
    assert_eq!(bus.read_u8(0x700000).unwrap(), 0x42);
    assert_eq!(bus.read_u8(0x7001F5).unwrap(), 0x00);
    // ...2KB SRAM mirrors across the 32KB window (partial decoding)...
    assert_eq!(bus.read_u8(0x700800).unwrap(), 0x42);
    // ...and the CPU stack bytes are exactly as seeded, NOT zeroed.
    assert_eq!(bus.read_u8(0x0001F5).unwrap(), 0x9B, "stack must survive SRAM writes");
    assert_eq!(bus.read_u8(0x0001F6).unwrap(), 0x51, "stack must survive SRAM writes");
    assert_eq!(bus.read_u8(0x0001F7).unwrap(), 0x00, "stack must survive SRAM writes");
}

#[test]
fn system_bus_wram_7e_and_7f_are_independent_not_mirrored() {
    // Banks $7E and $7F are the two contiguous 64KB halves of the same
    // 128KB WRAM chip, NOT mirrors of each other -- unlike the real
    // mirroring at $00-$3F/$80-$BF's $0000-$1FFF (which does alias the
    // low 8KB of $7E). Writing to one must not affect the other.
    let mut bus = SystemBus::new();

    bus.write_u8(0x7E8000, 0x12).unwrap();
    bus.write_u8(0x7F8000, 0x34).unwrap();

    assert_eq!(bus.read_u8(0x7E8000).unwrap(), 0x12);
    assert_eq!(bus.read_u8(0x7F8000).unwrap(), 0x34);
}

#[test]
fn system_bus_open_bus() {
    let mut bus = SystemBus::new();
    
    // First read from unmapped area should return 0 (initial open-bus)
    let value = bus.read_u8(0x5000).unwrap();
    assert_eq!(value, 0x00, "Initial open bus should be 0");
    
    // Write something to WRAM - this updates open-bus
    bus.write_u8(0x7E0000, 0xAA).unwrap();
    
    // Read from WRAM - this should return value and update open-bus
    let read_value = bus.read_u8(0x7E0000).unwrap();
    assert_eq!(read_value, 0xAA, "WRAM read should return written value");
    
    // Now read from unmapped area - should return last value (open-bus behavior)
    let value = bus.read_u8(0x5000).unwrap();
    assert_eq!(value, 0xAA, "Open bus should return last read value");
}

#[test]
fn system_bus_rom_read() {
    let mut bus = SystemBus::new();
    
    // Create a ROM (2MB) that will definitely be HiROM
    let mut rom = vec![0x00; 0x200000];
    // Fill ROM with known pattern
    for (i, byte) in rom.iter_mut().enumerate() {
        *byte = (i & 0xFF) as u8;
    }
    // Set HiROM mode byte at header position
    rom[0xFFD5] = 0x01; // Set bit 0 for HiROM
    
    bus.load_cartridge(rom).unwrap();
    
    // Verify cartridge is loaded
    assert!(bus.has_cartridge());
    
    // In HiROM: bank $C0 with offset 0x0000 maps to ROM offset 0x0000
    // 0xC00000 = bank 0xC0, offset 0x0000
    // ROM addr = ((0xC0 & 0x3F) * 0x10000) + 0 = 0x40 * 0x10000 = 0x400000
    // But ROM is only 0x200000, so this should wrap or be invalid
    // Let's try a valid offset
    
    // For HiROM: 0xC00000 maps to ROM offset 0, 0xC10000 maps to ROM offset 0x10000, etc.
    // Let's use offset 0x8000 in bank $C0 which should map to ROM offset 0x8000
    let value = bus.read_u8(0xC08000).unwrap();
    assert_eq!(value, 0x00, "HiROM read should return pattern at offset 0x8000");
}

#[test]
fn system_bus_io_stub() {
    let mut bus = SystemBus::new();
    
    // Read from I/O register area ($2100-$21FF)
    let value = bus.read_u8(0x2100).unwrap();
    assert_eq!(value, 0x00);
}

#[test]
fn system_bus_read_u16() {
    let mut bus = SystemBus::new();

    // Write two bytes to WRAM
    bus.write_u8(0x7E1000, 0x12).unwrap();
    bus.write_u8(0x7E1001, 0x34).unwrap();

    // Read as u16 (little-endian)
    let value = bus.read_u16(0x7E1000).unwrap();
    assert_eq!(value, 0x3412);
}

#[test]
fn wram_data_port_writes_sequentially_through_wmadd() {
    let mut bus = SystemBus::new();
    // Point WMADD at $7E:4000 (17-bit address 0x04000).
    bus.write_u8(0x002181, 0x00).unwrap();
    bus.write_u8(0x002182, 0x40).unwrap();
    bus.write_u8(0x002183, 0x00).unwrap();
    bus.write_u8(0x002180, 0x11).unwrap();
    bus.write_u8(0x002180, 0x22).unwrap();
    bus.write_u8(0x002180, 0x33).unwrap();
    assert_eq!(bus.read_u8(0x7E4000).unwrap(), 0x11);
    assert_eq!(bus.read_u8(0x7E4001).unwrap(), 0x22);
    assert_eq!(bus.read_u8(0x7E4002).unwrap(), 0x33);
}

#[test]
fn wram_data_port_reads_bank_7f_via_the_17th_address_bit() {
    let mut bus = SystemBus::new();
    bus.write_u8(0x7F0005, 0xEE).unwrap();
    // WMADD = 0x10005 -> $7F:0005.
    bus.write_u8(0x002181, 0x05).unwrap();
    bus.write_u8(0x002182, 0x00).unwrap();
    bus.write_u8(0x002183, 0x01).unwrap();
    assert_eq!(bus.read_u8(0x002180).unwrap(), 0xEE, "WMADDH bit 0 must reach the second 64KB (bank $7F)");
    // The read must have advanced the address.
    bus.write_u8(0x7F0006, 0xDD).unwrap();
    assert_eq!(bus.read_u8(0x002180).unwrap(), 0xDD);
}

