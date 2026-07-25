//! Fixtures shared by the renderer test modules.

use crate::cgram::Cgram;
use crate::oam::Oam;
use crate::ppu::PpuRegisters;
use crate::vram::Vram;

/// Encodes a tile in the REAL SNES row-interleaved bitplane-pair
/// layout (`[r0p0, r0p1, r1p0, r1p1, ...]`). An earlier version of
/// this helper wrote `[8 bytes p0][8 bytes p1]` -- the same wrong
/// (NES-style) layout the production decoder had, so the tests
/// self-consistently passed while every real cartridge tile rendered
/// as garbage.
pub(super) fn make_2bpp_tile(rows: [[u8; 8]; 8]) -> [u8; 16] {
    let mut data = [0u8; 16];
    for (y, row) in rows.iter().enumerate() {
        let mut lo = 0u8;
        let mut hi = 0u8;
        for (x, &pixel) in row.iter().enumerate() {
            let bit = 7 - x;
            lo |= (pixel & 1) << bit;
            hi |= ((pixel >> 1) & 1) << bit;
        }
        data[y * 2] = lo;
        data[y * 2 + 1] = hi;
    }
    data
}

/// Writes mode-7 tile data: assigns `tile` to every map entry of the
/// 128x128 field row `map_row`..(all rows if None isn't needed here),
/// and fills the given tile's 64 pixels with `value`.
pub(super) fn write_mode7_tile(vram: &mut Vram, tile: u8, value: u8) {
    for i in 0..64u16 {
        vram.write(((tile as u16) * 64 + i) * 2 + 1, value);
    }
}

/// A mode-1 setup with one solid red BG1 tile at the top-left, used by
/// the window tests: returns (vram, cgram, regs) ready to render.
pub(super) fn solid_bg1_setup() -> (Vram, Cgram, PpuRegisters) {
    let mut vram = Vram::new();
    let mut cgram = Cgram::new();
    let solid = make_2bpp_tile([[1u8; 8]; 8]);
    for (i, &b) in solid.iter().enumerate() {
        vram.write(i as u16, b); // tile 0
    }
    // 32x32 tilemap at word 0x400, every entry -> tile 0.
    for entry in 0..(32u16 * 32) {
        vram.write_word((0x400 + entry) * 2, 0x0000);
    }
    cgram.write(2, 0x1F); // CGRAM 1 = red
    cgram.write(3, 0x00);

    let mut regs = PpuRegisters::default();
    regs.inidisp = 0x0F;
    regs.bgmode = 1;
    regs.bg_sc[0] = 0x04;
    regs.tm = 0x01;
    (vram, cgram, regs)
}

pub(super) fn oam_empty() -> Oam {
    Oam::new()
}

/// Builds an OAM where each of `count` small sprites shows exactly ONE
/// opaque pixel at its top-left corner (tile 0 must have pixel value 1
/// at (0,0) only), sprite `i` at (x_step * i, 10). `large` also sets
/// every sprite's size bit (OBSEL pair 0: 8x8 small / 16x16 large).
pub(super) fn oam_with_sprite_row(count: usize, x_step: u8, large: bool) -> Oam {
    let mut oam = Oam::new();
    for i in 0..count {
        let base = (i * 4) as u16;
        oam.write(base, (i as u8).wrapping_mul(x_step)); // X
        oam.write(base + 1, 10); // Y
        oam.write(base + 2, 0); // tile 0
        oam.write(base + 3, 0x00); // palette 0, priority 0
    }
    // Park every other sprite off-screen (Y = 0xF0 = -16 with an 8px
    // sprite ends at line -8, never visible).
    for i in count..128 {
        let base = (i * 4) as u16;
        oam.write(base + 1, 0xF0);
    }
    if large {
        for i in 0..count {
            let byte = 512 + (i as u16) / 4;
            let old = oam.read(byte);
            oam.write(byte, old | (0x02 << ((i % 4) * 2)));
        }
    }
    oam
}

pub(super) fn single_pixel_sprite_setup() -> (Vram, Cgram, PpuRegisters) {
    let mut vram = Vram::new();
    let mut cgram = Cgram::new();
    // OBJ tile 0: pixel value 1 at (0,0), rest transparent.
    let mut tile_row0 = [0u8; 8];
    tile_row0[0] = 1;
    let tile = make_2bpp_tile([tile_row0, [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8]]);
    for (i, &b) in tile.iter().enumerate() {
        vram.write(i as u16, b);
    }
    cgram.write(129 * 2, 0xE0); // OBJ palette 0, pixel 1
    cgram.write(129 * 2 + 1, 0x03);
    let mut regs = PpuRegisters::default();
    regs.inidisp = 0x0F;
    regs.obsel = 0x00;
    regs.tm = 0x10; // sprites only
    (vram, cgram, regs)
}
