//! Controller input: the vblank auto-read latches and the manual $4016
//! serial strobe, for both ports.

use super::common::{tick_dots, tick_past_one_vblank_entry};
use crate::bus::{MemoryBus, SystemBus};

#[test]
fn auto_joypad_read_reports_zero_before_being_enabled() {
    let mut bus = SystemBus::new();
    bus.set_joypad1_state(0xFFFF);
    tick_past_one_vblank_entry(&mut bus);

    // $4200 bit0 (auto-read enable) was never set, so the vblank-entry
    // latch in `tick_ppu` must not have copied the live state in.
    assert_eq!(bus.read_u8(0x004218).unwrap(), 0x00);
    assert_eq!(bus.read_u8(0x004219).unwrap(), 0x00);
}

#[test]
fn auto_joypad_read_latches_live_state_at_vblank_entry() {
    let mut bus = SystemBus::new();
    bus.write_u8(0x004200, 0x01).unwrap(); // NMITIMEN bit0: enable auto-read

    // Press Start + Right (bit12 and bit8 in the SNES auto-read layout).
    bus.set_joypad1_state(0x1100);
    tick_past_one_vblank_entry(&mut bus);

    assert_eq!(bus.read_u8(0x004218).unwrap(), 0x00, "A/X/L/R byte: none of those pressed");
    assert_eq!(bus.read_u8(0x004219).unwrap(), 0x11, "Start (d4) and Right (d0) set");
}

#[test]
fn auto_joypad_read_maps_every_button_to_its_documented_bit() {
    // Cross-checks the full $4218/$4219 bit layout against the
    // documented SNES auto-read format (wiki.superfamicom.org):
    // $4218 d7=A d6=X d5=L d4=R; $4219 d7=B d6=Y d5=Select d4=Start
    // d3=Up d2=Down d1=Left d0=Right.
    let cases: &[(u16, u16, u8)] = &[
        (0x8000, 0x4219, 0x80), // B
        (0x4000, 0x4219, 0x40), // Y
        (0x2000, 0x4219, 0x20), // Select
        (0x1000, 0x4219, 0x10), // Start
        (0x0800, 0x4219, 0x08), // Up
        (0x0400, 0x4219, 0x04), // Down
        (0x0200, 0x4219, 0x02), // Left
        (0x0100, 0x4219, 0x01), // Right
        (0x0080, 0x4218, 0x80), // A
        (0x0040, 0x4218, 0x40), // X
        (0x0020, 0x4218, 0x20), // L
        (0x0010, 0x4218, 0x10), // R
    ];

    for &(snes_bits, reg_addr, expected) in cases {
        let mut bus = SystemBus::new();
        bus.write_u8(0x004200, 0x01).unwrap();
        bus.set_joypad1_state(snes_bits);
        tick_past_one_vblank_entry(&mut bus);
        assert_eq!(
            bus.read_u8(reg_addr as u32).unwrap(),
            expected,
            "button bits {:#06X} must set exactly {:#04X} at ${:04X}",
            snes_bits, expected, reg_addr
        );
    }
}

#[test]
fn auto_joypad_read_does_not_update_mid_frame() {
    // The latch only happens on the vblank-entry edge, not live on
    // every read -- pressing a button mid-frame (after the last latch)
    // must not be visible until the next vblank entry.
    let mut bus = SystemBus::new();
    bus.write_u8(0x004200, 0x01).unwrap();
    bus.set_joypad1_state(0x8000); // B held during the first vblank
    tick_past_one_vblank_entry(&mut bus);
    assert_eq!(bus.read_u8(0x004219).unwrap(), 0x80);

    // Change input mid-frame without crossing another vblank entry.
    bus.set_joypad1_state(0x0000);
    assert_eq!(
        bus.read_u8(0x004219).unwrap(),
        0x80,
        "must still report the last-latched value until the next vblank"
    );
}

#[test]
fn manual_joypad_strobe_shifts_out_bits_msb_first() {
    // Real controllers shift out B,Y,Select,Start,Up,Down,Left,Right,
    // A,X,L,R,0,0,0,0 -- MSB (B) first -- via the $4016 strobe/serial
    // protocol, independent of the auto-read mechanism.
    let mut bus = SystemBus::new();
    // B (bit15) and A (bit7) pressed.
    bus.set_joypad1_state(0x8080);

    // Strobe high then low to latch the snapshot.
    bus.write_u8(0x004016, 0x01).unwrap();
    bus.write_u8(0x004016, 0x00).unwrap();

    let mut bits = Vec::new();
    for _ in 0..16 {
        bits.push(bus.read_u8(0x004016).unwrap() & 0x01);
    }

    let expected = [
        1, 0, 0, 0, 0, 0, 0, 0, // B,Y,Select,Start,Up,Down,Left,Right
        1, 0, 0, 0, 0, 0, 0, 0, // A,X,L,R,0,0,0,0
    ];
    assert_eq!(bits, expected, "bits must shift out MSB-first matching real controller order");

    // A standard controller with nothing chained behind it reports 1
    // (pulled high) for any further reads past the 16 real bits.
    assert_eq!(bus.read_u8(0x004016).unwrap() & 0x01, 1);
    assert_eq!(bus.read_u8(0x004016).unwrap() & 0x01, 1);
}

#[test]
fn manual_joypad_read_while_strobe_high_always_reports_first_bit() {
    // While strobe is held high, the register continuously reflects
    // the live state's first bit (B) rather than shifting -- matching
    // real hardware, which keeps re-latching as long as strobe is 1.
    let mut bus = SystemBus::new();
    bus.write_u8(0x004016, 0x01).unwrap(); // strobe high

    bus.set_joypad1_state(0x8000); // B pressed
    assert_eq!(bus.read_u8(0x004016).unwrap() & 0x01, 1);
    assert_eq!(bus.read_u8(0x004016).unwrap() & 0x01, 1, "must not advance/shift while strobe is high");

    bus.set_joypad1_state(0x0000); // B released, still strobing
    assert_eq!(bus.read_u8(0x004016).unwrap() & 0x01, 0, "must reflect the live state, not a stale latch");
}

#[test]
fn joyser1_reads_zero_before_any_strobe_regardless_of_controller_state() {
    // Un-strobed $4017 reads must keep the serial DATA bit (bit 0) at
    // the deliberately safe 0 -- an always-1 ("pulled high"/no
    // controller) stub was tried and caused a real boot-time
    // regression in the real ROM (see the $4017 read handler). The
    // hardwired bits 4-2 (always 1 on port 2) and the open-bus high
    // bits are real hardware behavior and stay.
    let mut bus = SystemBus::new();
    bus.set_joypad1_state(0xFFFF);
    bus.set_joypad2_state(0xFFFF);
    let value = bus.read_u8(0x004017).unwrap();
    assert_eq!(value & 0x03, 0x00, "the un-strobed data bits must read 0");
    assert_eq!(value & 0x1C, 0x1C, "port 2 hardwires bits 4-2 high");
}

#[test]
fn joypad2_serial_read_shifts_out_its_own_snapshot_after_the_shared_strobe() {
    let mut bus = SystemBus::new();
    bus.set_joypad1_state(0x0000); // controller 1: nothing pressed
    bus.set_joypad2_state(0x8010); // controller 2: B (bit15) + R (bit4)

    // One strobe cycle on the shared $4016 line latches BOTH ports.
    bus.write_u8(0x004016, 0x01).unwrap();
    bus.write_u8(0x004016, 0x00).unwrap();

    let mut joy2_bits = Vec::new();
    for _ in 0..16 {
        joy2_bits.push(bus.read_u8(0x004017).unwrap() & 1);
    }
    let mut expected = vec![0u8; 16];
    expected[0] = 1; // B (bit15, shifted out first)
    expected[11] = 1; // R (bit4)
    assert_eq!(joy2_bits, expected, "$4017 must shift controller 2's own snapshot");
    // Controller 1's shift register must be untouched by $4017 reads.
    assert_eq!(bus.read_u8(0x004016).unwrap() & 1, 0, "controller 1 stream must be independent");
    // Past 16 bits, a connected controller reports 1 (no more data).
    assert_eq!(bus.read_u8(0x004017).unwrap() & 1, 1);
}

#[test]
fn joypad2_auto_read_latches_at_vblank_into_421a_421b() {
    let mut bus = SystemBus::new();
    bus.write_u8(0x004200, 0x01).unwrap(); // auto-joypad-read enable
    bus.set_joypad2_state(0x8010); // B + R
    tick_dots(&mut bus, 225 * 341); // cross the vblank-entry edge
    assert_eq!(bus.read_u8(0x00421A).unwrap(), 0x10, "JOY2L must hold the low byte of the latch");
    assert_eq!(bus.read_u8(0x00421B).unwrap(), 0x80, "JOY2H must hold the high byte of the latch");
}

#[test]
fn manual_joypad_strobe_snapshots_state_at_the_falling_edge_not_the_rising_edge() {
    // The latch must happen when strobe transitions from high to low,
    // using whatever the live state is AT THAT MOMENT -- previously it
    // snapshotted on the RISING edge instead, so a button pressed while
    // strobe was already held high (a common polling pattern) would be
    // missed entirely.
    let mut bus = SystemBus::new();
    bus.set_joypad1_state(0x0000);
    bus.write_u8(0x004016, 0x01).unwrap(); // strobe high; live state is 0 right now

    // Change the live state while strobe is still asserted.
    bus.set_joypad1_state(0x8000); // B pressed

    bus.write_u8(0x004016, 0x00).unwrap(); // falling edge: must snapshot THIS state

    assert_eq!(
        bus.read_u8(0x004016).unwrap() & 0x01,
        1,
        "falling-edge snapshot must reflect the state at the falling edge, not the rising edge"
    );
}

// ========================================================================
// VRAM/OAM/CGRAM readback register tests
// ========================================================================

