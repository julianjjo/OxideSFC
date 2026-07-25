//! Fixtures shared by the bus test modules.

use crate::bus::SystemBus;

/// Minimal valid LoROM image (with correct checksum fields) declaring
/// 2KB of SRAM, mirroring what `Cartridge::new` needs to map SRAM.
pub(super) fn build_lorom_with_sram() -> Vec<u8> {
    let mut rom = vec![0u8; 0x8000];
    let h = 0x7FC0;
    rom[h..h + 21].copy_from_slice(b"SRAM TEST CART       ");
    rom[h + 0x15] = 0x20; // LoROM
    rom[h + 0x16] = 0x02; // ROM+RAM+battery
    rom[h + 0x17] = 0x08; // 256KB declared (code doesn't verify size here)
    rom[h + 0x18] = 0x01; // 2KB SRAM
    rom[h + 0x19] = 0x01; // region
    // checksum/complement: compute over the image with the fields
    // zeroed the way Cartridge::compute_checksum expects.
    rom[h + 0x1C] = 0xFF;
    rom[h + 0x1D] = 0xFF;
    rom[h + 0x1E] = 0x00;
    rom[h + 0x1F] = 0x00;
    let sum: u32 = rom.iter().map(|&b| b as u32).sum();
    let checksum = (sum & 0xFFFF) as u16;
    let complement = !checksum;
    rom[h + 0x1C] = (complement & 0xFF) as u8;
    rom[h + 0x1D] = (complement >> 8) as u8;
    rom[h + 0x1E] = (checksum & 0xFF) as u8;
    rom[h + 0x1F] = (checksum >> 8) as u8;
    rom
}

/// Ticks the bus through exactly one edge transition (vblank-exit, or
/// one scanline's hblank-entry), landing safely past the boundary
/// rather than exactly on it, without overshooting into the *next*
/// edge -- `tick_ppu` only compares state once per call (start vs.
/// end), so crossing more than one edge in a single call would hide
/// intermediate ones (see `tick_past_one_vblank_entry`'s doc comment
/// for the same caveat).
pub(super) fn tick_dots(bus: &mut SystemBus, dots: u32) {
    // 4 master cycles per dot -- dot-granular, so odd counts (a real
    // scanline is 341 dots) advance exactly.
    bus.tick_master(dots * 4);
}

/// Ticks the bus forward to land just inside vblank (NTSC: scanline 224
/// of 262, 341 dots/line), which is what actually latches the
/// auto-joypad-read result into $4218/$4219 (see `tick_ppu`). Must not
/// overshoot past scanline 262 back into the next frame's active
/// scanlines, since `tick_ppu` only compares vblank state once per
/// call (before vs. after the whole batch), not per-scanline within it.
pub(super) fn tick_past_one_vblank_entry(bus: &mut SystemBus) {
    const DOTS_TO_MIDDLE_OF_VBLANK: u32 = 230 * 341; // scanline 230, safely within 224-261
    bus.tick_ppu(DOTS_TO_MIDDLE_OF_VBLANK / 2); // tick_ppu doubles cycles to dots
}

