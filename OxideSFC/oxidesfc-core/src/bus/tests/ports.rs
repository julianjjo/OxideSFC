//! The VRAM/CGRAM/OAM access ports: word latching, auto-increment, the
//! address remap modes and read-prefetch semantics.

use super::common::tick_dots;
use crate::bus::{MemoryBus, SystemBus};

#[test]
fn vram_write_via_2118_2119_lands_at_word_address_times_two() {
    let mut bus = SystemBus::new();
    // VMAIN = 0 (increment by 1 word, after low-byte write)
    bus.write_u8(0x002115, 0x00).unwrap();
    // VMADD = $0010
    bus.write_u8(0x002116, 0x10).unwrap();
    bus.write_u8(0x002117, 0x00).unwrap();
    bus.write_u8(0x002118, 0xAB).unwrap(); // low byte -- should also auto-increment
    bus.write_u8(0x002119, 0xCD).unwrap(); // high byte of the NEXT word now ($0011)

    assert_eq!(bus.ppu_ref().vram_ref().read(0x0020), 0xAB, "low byte of word $0010");
    assert_eq!(bus.ppu_ref().vram_ref().read(0x0023), 0xCD, "high byte of word $0011, after auto-increment");
}

#[test]
fn vram_write_does_not_increment_until_high_byte_when_vmain_bit7_set() {
    let mut bus = SystemBus::new();
    bus.write_u8(0x002115, 0x80).unwrap(); // increment after high-byte write
    bus.write_u8(0x002116, 0x00).unwrap();
    bus.write_u8(0x002117, 0x00).unwrap();
    bus.write_u8(0x002118, 0x11).unwrap(); // low byte: must NOT increment yet
    bus.write_u8(0x002119, 0x22).unwrap(); // high byte: increments after this

    // Both bytes belong to word $0000 (addresses 0,1), confirming the
    // address didn't advance between the low and high writes.
    assert_eq!(bus.ppu_ref().vram_ref().read(0x0000), 0x11);
    assert_eq!(bus.ppu_ref().vram_ref().read(0x0001), 0x22);
}

#[test]
fn cgram_write_pairs_low_then_high_byte_and_advances_on_second_write() {
    let mut bus = SystemBus::new();
    bus.write_u8(0x002121, 0x05).unwrap(); // CGADD = color index 5
    bus.write_u8(0x002122, 0x34).unwrap(); // low byte of color 5
    bus.write_u8(0x002122, 0x12).unwrap(); // high byte of color 5, advances CGADD to 6
    bus.write_u8(0x002122, 0x78).unwrap(); // low byte of color 6

    assert_eq!(bus.ppu_ref().cgram_ref().read(10), 0x34);
    assert_eq!(bus.ppu_ref().cgram_ref().read(11), 0x12);
    assert_eq!(bus.ppu_ref().cgram_ref().read(12), 0x78);
}

#[test]
fn oam_write_pairs_low_then_high_byte() {
    let mut bus = SystemBus::new();
    bus.write_u8(0x002102, 0x00).unwrap(); // OAMADDL = 0, resets toggle
    bus.write_u8(0x002103, 0x00).unwrap();
    bus.write_u8(0x002104, 0xAA).unwrap(); // sprite 0 Y
    bus.write_u8(0x002104, 0xBB).unwrap(); // sprite 0 X

    assert_eq!(bus.ppu_ref().oam_ref().read(0), 0xAA);
    assert_eq!(bus.ppu_ref().oam_ref().read(1), 0xBB);
}

#[test]
fn oam_address_reloads_from_the_2102_latch_at_every_vblank_start() {
    // Real hardware re-applies the last $2102/$2103 value to the live
    // OAM address at the start of each vblank (unless forced blank).
    // DKC sets OAMADD=0 once and then relies on this auto-reload for
    // its every-frame 544-byte OAM DMA; without it, the live address
    // marched +0x110 words per frame past the end of OAM and no
    // sprite upload ever landed again -- gameplay rendered with no
    // sprites at all (no player, no enemies).
    let mut bus = SystemBus::new();
    bus.write_u8(0x002100, 0x0F).unwrap(); // screen on (reload is gated on !forced-blank)
    bus.write_u8(0x002102, 0x00).unwrap();
    bus.write_u8(0x002103, 0x00).unwrap();

    // Consume two full words (the low table commits word-at-a-time
    // through the $2104 write latch), leaving the live address at
    // word 2.
    bus.write_u8(0x002104, 0x11).unwrap();
    bus.write_u8(0x002104, 0x22).unwrap();
    bus.write_u8(0x002104, 0x33).unwrap();
    bus.write_u8(0x002104, 0x44).unwrap();
    assert_eq!(bus.ppu_ref().oam_ref().read(0), 0x11);
    assert_eq!(bus.ppu_ref().oam_ref().read(2), 0x33);

    // Cross one vblank-entry edge WITHOUT touching $2102/$2103.
    tick_dots(&mut bus, 230 * 341);

    // The next writes must land back at word 0 (and with the byte
    // toggle reset), exactly as if software had rewritten OAMADD.
    bus.write_u8(0x002104, 0xAA).unwrap();
    bus.write_u8(0x002104, 0xBB).unwrap();
    assert_eq!(bus.ppu_ref().oam_ref().read(0), 0xAA, "low byte of word 0 -- the vblank reload must reset the live address to the $2102/$2103 latch");
    assert_eq!(bus.ppu_ref().oam_ref().read(1), 0xBB, "high byte of word 0");
    assert_eq!(bus.ppu_ref().oam_ref().read(2), 0x33, "word 1 must be untouched by the post-reload writes");
}

#[test]
fn oam_address_does_not_reload_during_forced_blank() {
    // The vblank auto-reload is suppressed while INIDISP bit 7 (forced
    // blank) is set -- writes keep streaming from wherever the live
    // address is, which is exactly what boot-time OAM-clear loops that
    // span several (blanked) frames rely on.
    let mut bus = SystemBus::new();
    bus.write_u8(0x002100, 0x8F).unwrap(); // forced blank ON
    bus.write_u8(0x002102, 0x00).unwrap();
    bus.write_u8(0x002103, 0x00).unwrap();

    bus.write_u8(0x002104, 0x11).unwrap();
    bus.write_u8(0x002104, 0x22).unwrap();

    tick_dots(&mut bus, 230 * 341); // vblank entry while blanked: no reload

    bus.write_u8(0x002104, 0x33).unwrap();
    bus.write_u8(0x002104, 0x44).unwrap();
    assert_eq!(bus.ppu_ref().oam_ref().read(0), 0x11, "word 0 must NOT be overwritten -- no reload happened");
    assert_eq!(bus.ppu_ref().oam_ref().read(2), 0x33, "the stream must continue at word 1");
    assert_eq!(bus.ppu_ref().oam_ref().read(3), 0x44);
}

#[test]
fn oam_read_via_2138_round_trips_after_write_and_auto_increments() {
    let mut bus = SystemBus::new();
    bus.write_u8(0x002102, 0x00).unwrap(); // OAMADDL = 0, resets toggle
    bus.write_u8(0x002103, 0x00).unwrap();
    bus.write_u8(0x002104, 0xAA).unwrap(); // sprite 0 Y
    bus.write_u8(0x002104, 0xBB).unwrap(); // sprite 0 X

    // Reset OAMADD/toggle back to the start to read back what was written.
    bus.write_u8(0x002102, 0x00).unwrap();
    bus.write_u8(0x002103, 0x00).unwrap();
    assert_eq!(bus.read_u8(0x002138).unwrap(), 0xAA, "low byte (Y) of sprite 0");
    assert_eq!(bus.read_u8(0x002138).unwrap(), 0xBB, "high byte (X), after auto-increment");
}

#[test]
fn vram_read_via_2139_213a_round_trips_after_write_and_auto_increments() {
    let mut bus = SystemBus::new();
    bus.write_u8(0x002115, 0x80).unwrap(); // increment after high-byte access
    bus.write_u8(0x002116, 0x10).unwrap();
    bus.write_u8(0x002117, 0x00).unwrap();
    bus.write_u8(0x002118, 0xAB).unwrap();
    bus.write_u8(0x002119, 0xCD).unwrap();

    // Point VMADD back at word $0010 to read back what was written.
    bus.write_u8(0x002116, 0x10).unwrap();
    bus.write_u8(0x002117, 0x00).unwrap();
    assert_eq!(bus.read_u8(0x002139).unwrap(), 0xAB, "low byte read back");
    assert_eq!(bus.read_u8(0x00213A).unwrap(), 0xCD, "high byte read back");

    // VMADD must have auto-incremented to word $0011 after the
    // high-byte read (same VMAIN-driven timing as the write side).
    bus.write_u8(0x002118, 0x11).unwrap();
    bus.write_u8(0x002119, 0x22).unwrap();
    assert_eq!(bus.ppu_ref().vram_ref().read(0x0022), 0x11, "word $0011 low byte, confirming VMADD advanced");
    assert_eq!(bus.ppu_ref().vram_ref().read(0x0023), 0x22);
}

#[test]
fn cgram_read_via_213b_round_trips_after_write_and_auto_increments() {
    let mut bus = SystemBus::new();
    bus.write_u8(0x002121, 0x05).unwrap(); // CGADD = color index 5
    bus.write_u8(0x002122, 0x34).unwrap(); // low byte of color 5
    bus.write_u8(0x002122, 0x12).unwrap(); // high byte of color 5, advances CGADD to 6

    bus.write_u8(0x002121, 0x05).unwrap(); // back to color 5, resets toggle
    assert_eq!(bus.read_u8(0x00213B).unwrap(), 0x34, "low byte of color 5");
    assert_eq!(bus.read_u8(0x00213B).unwrap(), 0x12, "high byte of color 5");

    // CGADD must have auto-incremented to color 6 (never written -> 0).
    assert_eq!(bus.read_u8(0x00213B).unwrap(), 0x00, "low byte of color 6");
}

// ========================================================================
// DMA/HDMA real transfer-state flag tests
// ========================================================================

#[test]
fn vram_read_returns_the_prefetch_buffer_with_dummy_read_semantics() {
    // $2139/$213A return the prefetch buffer; the buffer refills from
    // the PRE-increment address on the increment-phase read. Net
    // effect: after setting $2116/$2117 the first TWO word reads both
    // return the addressed word -- which is exactly why real code does
    // one dummy read before consuming data.
    let mut bus = SystemBus::new();
    bus.write_u8(0x002115, 0x80).unwrap(); // word step on the high-byte access
    bus.write_u8(0x002116, 0x00).unwrap();
    bus.write_u8(0x002117, 0x00).unwrap();
    bus.write_u8(0x002118, 0x22).unwrap(); // word 0 = 0x1122
    bus.write_u8(0x002119, 0x11).unwrap();
    bus.write_u8(0x002118, 0x44).unwrap(); // word 1 = 0x3344
    bus.write_u8(0x002119, 0x33).unwrap();

    bus.write_u8(0x002116, 0x00).unwrap(); // point back at word 0 (primes the buffer)
    bus.write_u8(0x002117, 0x00).unwrap();
    assert_eq!(bus.read_u8(0x002139).unwrap(), 0x22, "1st read: the primed word 0");
    assert_eq!(bus.read_u8(0x00213A).unwrap(), 0x11);
    assert_eq!(bus.read_u8(0x002139).unwrap(), 0x22, "2nd read: STILL word 0 (refill was pre-increment)");
    assert_eq!(bus.read_u8(0x00213A).unwrap(), 0x11);
    assert_eq!(bus.read_u8(0x002139).unwrap(), 0x44, "3rd read: word 1's data finally streams out");
    assert_eq!(bus.read_u8(0x00213A).unwrap(), 0x33);
}

#[test]
fn vmain_remap_mode_rotates_the_data_port_address() {
    // VMAIN bits 2-3 = 01 (8-bit rotate): word address aaaaaaaaBBBccccc
    // is accessed as aaaaaaaacccccBBB. Nominal word 0x0021 (BBB=001,
    // ccccc=00001) must land at physical word 0x0009 (00001_001).
    let mut bus = SystemBus::new();
    bus.write_u8(0x002115, 0x84).unwrap(); // increment on high + remap mode 1
    bus.write_u8(0x002116, 0x21).unwrap();
    bus.write_u8(0x002117, 0x00).unwrap();
    bus.write_u8(0x002118, 0xCD).unwrap();
    bus.write_u8(0x002119, 0xAB).unwrap();
    assert_eq!(
        bus.ppu_ref().vram_ref().read_word(0x0009 * 2),
        0xABCD,
        "the remap must rotate the low byte's bit groups"
    );
    assert_eq!(bus.ppu_ref().vram_ref().read_word(0x0021 * 2), 0x0000, "nothing lands at the nominal address");
}

#[test]
fn oam_low_table_commits_word_at_a_time_through_the_write_latch() {
    let mut bus = SystemBus::new();
    bus.write_u8(0x002102, 0x00).unwrap();
    bus.write_u8(0x002103, 0x00).unwrap();
    bus.write_u8(0x002104, 0x55).unwrap(); // even byte: held in the latch
    assert_eq!(
        bus.ppu_ref().oam_ref().read(0),
        0x00,
        "the even byte must sit in the latch until the odd-byte write commits the word"
    );
    bus.write_u8(0x002104, 0x66).unwrap(); // odd byte: commits both
    assert_eq!(bus.ppu_ref().oam_ref().read(0), 0x55);
    assert_eq!(bus.ppu_ref().oam_ref().read(1), 0x66);

    // The high table ($200+) has no latch: each byte writes immediately.
    bus.write_u8(0x002102, 0x00).unwrap();
    bus.write_u8(0x002103, 0x01).unwrap(); // OAMADD word 0x100 -> byte 0x200
    bus.write_u8(0x002104, 0x77).unwrap();
    assert_eq!(bus.ppu_ref().oam_ref().read(512), 0x77, "high-table bytes commit immediately");
}

#[test]
fn forced_blank_off_during_vblank_reloads_the_oam_address() {
    // The vblank-entry OAM-address reload is skipped in forced blank;
    // turning forced blank OFF while still inside vblank performs the
    // reload right then (snes9x's $2100 handler).
    let mut bus = SystemBus::new();
    bus.write_u8(0x002100, 0x8F).unwrap(); // forced blank ON
    bus.write_u8(0x002102, 0x00).unwrap();
    bus.write_u8(0x002103, 0x00).unwrap();
    bus.write_u8(0x002104, 0x11).unwrap(); // consume word 0
    bus.write_u8(0x002104, 0x22).unwrap();

    tick_dots(&mut bus, 230 * 341); // vblank entry happened while blanked: no reload
    bus.write_u8(0x002100, 0x0F).unwrap(); // un-blank DURING vblank -> reload now

    bus.write_u8(0x002104, 0xAA).unwrap();
    bus.write_u8(0x002104, 0xBB).unwrap();
    assert_eq!(bus.ppu_ref().oam_ref().read(0), 0xAA, "the un-blank write must have reloaded OAMADD to the latch");
    assert_eq!(bus.ppu_ref().oam_ref().read(1), 0xBB);
}

#[test]
fn vram_writes_are_blocked_during_active_display() {
    // The PPU owns VRAM while drawing: data-port writes only land
    // during vblank or forced blank (snes9x BlockInvalidVRAMAccess /
    // CHECK_INBLANK); blocked writes don't advance VMADD either.
    let mut bus = SystemBus::new();
    bus.write_u8(0x002100, 0x0F).unwrap(); // screen ON, scanline 0 = active display
    bus.write_u8(0x002116, 0x00).unwrap();
    bus.write_u8(0x002117, 0x00).unwrap();
    bus.write_u8(0x002118, 0xAA).unwrap(); // active display: dropped
    assert_eq!(bus.ppu_ref().vram_ref().read(0), 0x00, "active-display VRAM writes must be dropped");

    tick_dots(&mut bus, 230 * 341); // into vblank
    bus.write_u8(0x002118, 0xBB).unwrap(); // now it lands -- at word 0 (no phantom increment)
    assert_eq!(bus.ppu_ref().vram_ref().read(0), 0xBB, "vblank writes land at the unmoved address");
}

#[test]
fn cgdata_high_byte_masks_bit15_and_213b_reads_it_back_with_open_bus_bit7() {
    let mut bus = SystemBus::new();
    bus.write_u8(0x002121, 0x00).unwrap();
    bus.write_u8(0x002122, 0xFF).unwrap();
    bus.write_u8(0x002122, 0xFF).unwrap(); // high byte: bit 7 doesn't exist in CGRAM
    assert_eq!(
        bus.ppu_ref().cgram_ref().read_color(0),
        0x7FFF,
        "CGRAM colors are 15-bit: the write must mask bit 15"
    );

    bus.write_u8(0x002121, 0x00).unwrap();
    assert_eq!(bus.read_u8(0x00213B).unwrap(), 0xFF, "low byte reads back whole");
    // High-byte read: bits 6-0 from CGRAM (0x7F), bit 7 from PPU2's
    // open bus -- which the low-byte read just set to 0xFF.
    assert_eq!(bus.read_u8(0x00213B).unwrap(), 0xFF);
}

