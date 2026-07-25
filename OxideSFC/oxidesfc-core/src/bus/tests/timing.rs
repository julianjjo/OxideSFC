//! Timing and interrupts: per-region access costs, dot advancement, the
//! H/V timers, NMI edges, counter latches and WRAM refresh.

use super::common::tick_dots;
use crate::bus::{MemoryBus, SystemBus};

#[test]
fn h_only_irq_fires_at_configured_htime_not_every_scanline() {
    // $4200 bit 4 alone (H-timer only, no V): real hardware fires the
    // IRQ on EVERY scanline, at the exact dot the beam passes HTIME
    // ($4207/$4208) -- the flag-set point is HTIME*4 + 14 master
    // cycles into the line (snes9x `PPU.HTimerPosition`), i.e. ~3 dots
    // past the HTIME dot. It must NOT fire anywhere else on the line.
    let mut bus = SystemBus::new();
    bus.write_u8(0x004207, 100).unwrap(); // HTIME = 100 -> fires at ~dot 103
    bus.write_u8(0x004208, 0).unwrap();
    bus.write_u8(0x004200, 0x10).unwrap(); // H-IRQ enable only (bit 4)

    // Land just before the trigger dot: must not have fired yet.
    bus.tick_master(101 * 4); // h_counter = 101 < 103
    assert!(!bus.irq_pending(), "no IRQ before the beam reaches HTIME's trigger dot");

    // Cross the trigger dot within the same line: fires.
    bus.tick_master(4 * 4); // h_counter = 105 >= 103
    assert!(bus.irq_pending(), "H-IRQ must fire when the beam crosses HTIME");

    // Acknowledge, then finish the line and stop before the NEXT
    // line's trigger dot: must not fire in between (the old
    // scanline-granular model fired at boundaries, not at HTIME).
    assert_eq!(bus.read_u8(0x004211).unwrap() & 0x80, 0x80);
    assert!(!bus.irq_pending());
    bus.tick_master((341 - 105 + 50) * 4); // next line, h_counter = 50
    assert!(!bus.irq_pending(), "crossing a line boundary alone (dot 50 < HTIME) must not fire");

    // ...and crossing the new line's trigger dot fires again: the
    // H-timer is a once-PER-LINE event on real hardware.
    bus.tick_master(60 * 4); // h_counter = 110 >= 103
    assert!(bus.irq_pending(), "the H-timer re-fires on every line at HTIME");
}

#[test]
fn v_timer_irq_fires_at_vtime_and_is_acknowledged_by_reading_4211() {
    // The V-timer IRQ SMW arms every in-level frame for its status-bar
    // raster split: enabled via $4200 bit 5, fires when the scanline
    // reaches VTIME ($4209/$420A), stays asserted (level-triggered)
    // until $4211 is read, and is also cleared by disabling both timer
    // enables.
    let mut bus = SystemBus::new();
    bus.write_u8(0x004209, 100).unwrap(); // VTIME = 100
    bus.write_u8(0x00420A, 0).unwrap();
    bus.write_u8(0x004200, 0x20).unwrap(); // V-IRQ enable
    assert!(!bus.irq_pending(), "no IRQ before the target scanline");

    bus.tick_master(100 * 341 * 4); // advance exactly 100 scanlines
    assert!(bus.irq_pending(), "IRQ line must assert at scanline == VTIME");

    // Still asserted until acknowledged...
    bus.tick_master(341 * 4);
    assert!(bus.irq_pending());
    // ...reading $4211 reports bit 7 and acks.
    assert_eq!(bus.read_u8(0x004211).unwrap() & 0x80, 0x80);
    assert!(!bus.irq_pending(), "reading $4211 must deassert the line");
    assert_eq!(bus.read_u8(0x004211).unwrap() & 0x80, 0x00, "flag reads clear");

    // Disabling both timer IRQs also acknowledges a pending one.
    let mut bus2 = SystemBus::new();
    bus2.write_u8(0x004209, 50).unwrap();
    bus2.write_u8(0x00420A, 0).unwrap();
    bus2.write_u8(0x004200, 0x20).unwrap();
    bus2.tick_master(60 * 341 * 4);
    assert!(bus2.irq_pending());
    bus2.write_u8(0x004200, 0x80).unwrap(); // NMI only, timer IRQs off
    assert!(!bus2.irq_pending(), "clearing $4200 bits 4-5 must ack a pending IRQ");
}

#[test]
fn bus_accesses_accumulate_real_per_region_master_cycle_costs() {
    let mut bus = SystemBus::new();
    bus.take_step_access_costs(); // clear

    let _ = bus.read_u8(0x7E0000).unwrap(); // WRAM: 8 (slow)
    let _ = bus.read_u8(0x002100); // PPU register: 6 (fast)
    let _ = bus.read_u8(0x004016).unwrap(); // joypad port: 12 (extra-slow)
    let _ = bus.read_u8(0x008000).unwrap(); // SlowROM lower bank: 8

    let (count, master) = bus.take_step_access_costs();
    assert_eq!(count, 4);
    assert_eq!(master, 8 + 6 + 12 + 8, "each region must bill its real access speed");

    // FastROM (MEMSEL bit 0) speeds up UPPER-bank ROM only.
    bus.write_u8(0x00420D, 0x01).unwrap();
    bus.take_step_access_costs();
    let _ = bus.read_u8(0x808000).unwrap(); // FastROM upper bank: 6
    let _ = bus.read_u8(0x008000).unwrap(); // lower bank stays slow: 8
    let (_, master_fast) = bus.take_step_access_costs();
    assert_eq!(master_fast, 6 + 8, "FastROM must apply to $80+ banks only");
}

#[test]
fn tick_master_advances_dots_at_exactly_four_master_cycles_each() {
    let mut bus = SystemBus::new();
    let h0 = bus.ppu_ref().h_counter();
    bus.tick_master(6); // 1 dot + remainder 2
    assert_eq!(bus.ppu_ref().h_counter(), h0 + 1, "6 master cycles = 1 whole dot");
    bus.tick_master(2); // remainder 2 + 2 = 1 more dot, no loss to truncation
    assert_eq!(bus.ppu_ref().h_counter(), h0 + 2, "sub-dot remainders must carry across calls");
}

#[test]
fn slhv_latches_hv_counters_readable_at_213c_213d() {
    let mut bus = SystemBus::new();
    // Advance the PPU to a known position: 3 full 341-dot lines plus
    // 300 dots -> scanline = 3, h_counter = 300.
    tick_dots(&mut bus, 3 * 341 + 300);

    let _ = bus.read_u8(0x002137).unwrap(); // SLHV: latch now
    tick_dots(&mut bus, 123); // moving on must NOT change the latch

    // The high-byte reads only drive bit 0 (bits 7-1 are PPU2 open
    // bus), so mask like real games do.
    let h_lo = bus.read_u8(0x00213C).unwrap() as u16;
    let h_hi = (bus.read_u8(0x00213C).unwrap() & 0x01) as u16;
    let v_lo = bus.read_u8(0x00213D).unwrap() as u16;
    let v_hi = (bus.read_u8(0x00213D).unwrap() & 0x01) as u16;
    assert_eq!((h_hi << 8) | h_lo, 300, "OPHCT must report the latched dot position");
    // Hardware's V counter leads our internal scanline by one (the
    // picture is V=1..224), so internal scanline 3 latches as V=4.
    assert_eq!((v_hi << 8) | v_lo, 4, "OPVCT must report the latched hardware V position");

    // STAT78 must report the latch and clear it (and reset toggles).
    let stat = bus.read_u8(0x00213F).unwrap();
    assert_ne!(stat & 0x40, 0, "STAT78 bit 6 must be set after a latch");
    let stat2 = bus.read_u8(0x00213F).unwrap();
    assert_eq!(stat2 & 0x40, 0, "reading STAT78 must clear the latch flag");
}

#[test]
fn wrio_bit7_falling_edge_latches_counters() {
    let mut bus = SystemBus::new();
    tick_dots(&mut bus, 250);
    bus.write_u8(0x004201, 0xFF).unwrap(); // bit 7 high (also the power-on state)
    bus.write_u8(0x004201, 0x7F).unwrap(); // falling edge -> latch
    let stat = bus.read_u8(0x00213F).unwrap();
    assert_ne!(stat & 0x40, 0, "WRIO bit-7 falling edge must latch the counters");
    let h_lo = bus.read_u8(0x00213C).unwrap() as u16;
    let h_hi = (bus.read_u8(0x00213C).unwrap() & 0x01) as u16; // bits 7-1 are PPU2 open bus
    assert_eq!((h_hi << 8) | h_lo, 250);
}

#[test]
fn slhv_soft_latch_is_gated_by_wrio_bit7() {
    // The $2137 read-latch only works while WRIO ($4201) bit 7 drives
    // the latch pin high (snes9x `S9xLatchCounters` gates on
    // FillRAM[$4213] & 0x80).
    let mut bus = SystemBus::new();
    bus.write_u8(0x004201, 0x7F).unwrap(); // pin low (the falling edge itself latches once)
    let _ = bus.read_u8(0x00213F).unwrap(); // clear that latch flag
    tick_dots(&mut bus, 100);
    let _ = bus.read_u8(0x002137).unwrap();
    assert_eq!(
        bus.read_u8(0x00213F).unwrap() & 0x40,
        0,
        "$2137 must not latch while WRIO bit 7 is low"
    );
    bus.write_u8(0x004201, 0xFF).unwrap(); // pin high again (rising edge doesn't latch)
    let _ = bus.read_u8(0x002137).unwrap();
    assert_ne!(
        bus.read_u8(0x00213F).unwrap() & 0x40,
        0,
        "the $2137 soft latch works with WRIO bit 7 high"
    );
}

#[test]
fn enabling_nmi_mid_vblank_with_the_flag_still_set_fires_immediately() {
    // Turning $4200 bit 7 on while RDNMI's flag is still set (i.e.
    // during vblank, before the game read $4210) must trigger an NMI
    // right away -- snes9x ppu.cpp $4200: "NMI can trigger immediately
    // during VBlank as long as NMI_read ($4210) wasn't cleared".
    let mut bus = SystemBus::new();
    bus.tick_master(225 * 341 * 4); // into vblank (flag set at the entry edge)
    assert!(!bus.take_pending_nmi(), "NMI disabled at the vblank edge: nothing pending");
    bus.write_u8(0x004200, 0x80).unwrap();
    assert!(bus.take_pending_nmi(), "enabling NMI while RDNMI bit 7 is set must fire immediately");

    // ...but not once the game already read (and cleared) $4210.
    let mut bus2 = SystemBus::new();
    bus2.tick_master(225 * 341 * 4);
    let _ = bus2.read_u8(0x004210).unwrap(); // clears the flag
    bus2.write_u8(0x004200, 0x80).unwrap();
    assert!(!bus2.take_pending_nmi(), "no immediate NMI after $4210 was read");
}

#[test]
fn rdnmi_and_timeup_mix_in_cpu_open_bus_bits() {
    let mut bus = SystemBus::new();
    bus.write_u8(0x7E0000, 0xFF).unwrap(); // drive the open bus to 0xFF
    // $4210: bit 7 = flag (clear), bits 6-4 = open bus, bits 3-0 = CPU
    // version 2.
    assert_eq!(bus.read_u8(0x004210).unwrap(), 0x72);
    // $4211: bit 7 = IRQ flag (clear), bits 6-0 = open bus -- which is
    // now 0x72, the byte the $4210 read just drove onto the bus.
    assert_eq!(bus.read_u8(0x004211).unwrap(), 0x72);
}

#[test]
fn hvbjoy_reports_auto_joypad_busy_for_the_first_two_vblank_lines() {
    let mut bus = SystemBus::new();
    bus.write_u8(0x004200, 0x01).unwrap(); // auto-joypad-read enable
    bus.tick_master((224 * 341 + 10) * 4); // scanline 224 dot 10, just inside vblank
    assert_eq!(
        bus.read_u8(0x004212).unwrap() & 0x81,
        0x81,
        "vblank flag + auto-joypad busy during the read window"
    );
    bus.tick_master(2 * 341 * 4); // scanline 226
    assert_eq!(
        bus.read_u8(0x004212).unwrap() & 0x81,
        0x80,
        "busy clears once the ~2-line auto-read window passes"
    );
}

#[test]
fn wram_refresh_stalls_the_cpu_40_master_cycles_once_per_scanline() {
    let mut bus = SystemBus::new();
    bus.take_step_access_costs(); // clear
    bus.tick_master(120 * 4); // dots 0-120: before the refresh position (~dot 134)
    assert_eq!(bus.take_step_access_costs().1, 0, "no refresh charge before dot ~134");
    bus.tick_master(20 * 4); // cross dot 134
    assert_eq!(
        bus.take_step_access_costs().1,
        40,
        "crossing the per-line refresh position must charge 40 master cycles"
    );
    bus.tick_master(100 * 4); // later on the same line
    assert_eq!(bus.take_step_access_costs().1, 0, "the refresh happens once per line");
    bus.tick_master(341 * 4); // same position on the NEXT line
    assert_eq!(bus.take_step_access_costs().1, 40, "every scanline refreshes again");
}

#[test]
fn rdnmi_flag_expires_at_the_end_of_vblank_even_if_never_read() {
    // Hardware clears RDNMI's bit 7 at the end of the blanking period
    // whether or not the game read $4210 (snes9x resets FillRAM[$4210]
    // at the V-counter wrap). A poll outside vblank must see 0.
    let mut bus = SystemBus::new();
    bus.tick_master(230 * 341 * 4); // into vblank; the entry edge set the flag
    bus.tick_master(33 * 341 * 4); // cross the frame wrap into the next frame's line 1
    assert_eq!(
        bus.read_u8(0x004210).unwrap() & 0x80,
        0,
        "the vblank flag must not survive past the end of vblank"
    );
}

#[test]
fn overscan_moves_vblank_and_the_nmi_to_line_239() {
    let mut bus = SystemBus::new();
    bus.write_u8(0x002133, 0x04).unwrap(); // SETINI overscan
    bus.write_u8(0x004200, 0x80).unwrap(); // NMI enable
    bus.tick_master(230 * 341 * 4); // line 230: visible in overscan mode
    assert_eq!(bus.read_u8(0x004212).unwrap() & 0x80, 0, "line 230 is not vblank with overscan on");
    assert!(!bus.take_pending_nmi(), "no NMI before line 239 in overscan mode");
    bus.tick_master(10 * 341 * 4); // line 240
    assert_eq!(bus.read_u8(0x004212).unwrap() & 0x80, 0x80, "vblank starts at line 239 with overscan");
    assert!(bus.take_pending_nmi(), "the NMI fires at the overscan vblank boundary");
}

