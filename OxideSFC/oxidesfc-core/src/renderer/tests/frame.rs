//! Whole-frame behavior: forced blank, tile decoding, and the
//! per-scanline register/palette bands.

use super::common::make_2bpp_tile;
use crate::cgram::Cgram;
use crate::oam::Oam;
use crate::ppu::PpuRegisters;
use crate::renderer::compose::{
    render_frame, render_frame_per_scanline, render_frame_per_scanline_with_cgram,
};
use crate::renderer::color::bgr555_to_rgb8;
use crate::renderer::tile::decode_tile_row;
use crate::renderer::{SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::vram::Vram;

#[test]
fn tile_decode_uses_row_interleaved_snes_bitplane_layout() {
    // Pins the REAL SNES tile byte layout with hand-written raw bytes
    // (deliberately NOT built via make_2bpp_tile, so this test cannot
    // become self-consistently wrong alongside the encoder again).
    // 2bpp row-interleaved: byte 0 = row0 plane0, byte 1 = row0 plane1.
    let mut vram = Vram::new();
    vram.write(0, 0b1111_0000); // row 0, plane 0
    vram.write(1, 0b0000_1111); // row 0, plane 1
    vram.write(2, 0b1000_0001); // row 1, plane 0
    vram.write(3, 0b1000_0000); // row 1, plane 1

    let row0 = decode_tile_row(&vram, 0, 0, 2, 0);
    assert_eq!(row0, [1, 1, 1, 1, 2, 2, 2, 2],
        "row 0 must combine byte0 (plane0) and byte1 (plane1)");
    let row1 = decode_tile_row(&vram, 0, 0, 2, 1);
    assert_eq!(row1, [3, 0, 0, 0, 0, 0, 0, 1],
        "row 1 must come from bytes 2/3 (row-interleaved), not bytes 1/9 (planar)");

    // 4bpp: second bitplane pair starts 16 bytes in; its two planes
    // are also row-interleaved and contribute pixel bits 2/3.
    let mut vram4 = Vram::new();
    vram4.write(0, 0xFF); // row 0, plane 0
    vram4.write(16, 0xFF); // row 0, plane 2
    vram4.write(17, 0xFF); // row 0, plane 3
    let row0_4bpp = decode_tile_row(&vram4, 0, 0, 4, 0);
    assert_eq!(row0_4bpp, [0x0D; 8],
        "4bpp planes 0/2/3 set -> pixel value 0b1101 for every pixel of row 0");
}

#[test]
fn forced_blank_renders_solid_black() {
    let vram = Vram::new();
    let cgram = Cgram::new();
    let oam = Oam::new();
    let mut regs = PpuRegisters::default();
    regs.inidisp = 0x80;

    let fb = render_frame(&vram, &cgram, &oam, &regs);
    assert_eq!(fb.len(), SCREEN_WIDTH * SCREEN_HEIGHT * 4);
    assert!(fb.chunks_exact(4).all(|px| px == [0, 0, 0, 0xFF]));
}

#[test]
fn per_scanline_register_bands_render_each_row_with_its_own_state() {
    // The banded renderer must apply each scanline's captured register
    // state to that scanline only -- this is what makes SMW's
    // IRQ-driven status-bar split (different BG3 scroll above/below
    // the bar) and HDMA effects renderable at all with a
    // snapshot-based renderer.
    let mut vram = Vram::new();
    let mut cgram = Cgram::new();
    let oam = Oam::new();

    // Solid 4bpp-compatible tile (value 1) as tile 1 (tile 0 stays
    // all-transparent, since the zero-filled tilemap references it
    // everywhere); only map entry (0,0) uses the solid tile, so it is
    // visible exactly when hofs/vofs = 0.
    let solid = make_2bpp_tile([[1u8; 8]; 8]);
    for (i, &b) in solid.iter().enumerate() {
        vram.write(32 + i as u16, b); // 4bpp tile 1 starts at byte 32
    }
    vram.write_word(0x400 * 2, 0x0001);
    cgram.write(2, 0x1F); // CGRAM 1 = red
    cgram.write(3, 0x00);

    let mut top = PpuRegisters::default();
    top.inidisp = 0x0F;
    top.bgmode = 1;
    top.bg_sc[0] = 0x04; // tilemap word 0x400, 32x32
    top.tm = 0x01;

    let mut bottom = top;
    bottom.bg_hofs[0] = 64; // scroll the tile out of view below the split

    let mut lines = vec![top; SCREEN_HEIGHT];
    for line in lines.iter_mut().skip(4) {
        *line = bottom;
    }

    let fb = render_frame_per_scanline(&vram, &cgram, &oam, &lines);
    let red = bgr555_to_rgb8(cgram.read_color(1));
    let backdrop = bgr555_to_rgb8(cgram.read_color(0));

    // Row 2 (above the split): tile pixel visible at x=0.
    let above = (2 * SCREEN_WIDTH) * 4;
    assert_eq!((fb[above], fb[above + 1], fb[above + 2]), red,
        "rows before the split must use the first band's scroll");
    // Row 6 (below the split): the tile is scrolled away, backdrop shows.
    let below = (6 * SCREEN_WIDTH) * 4;
    assert_eq!((fb[below], fb[below + 1], fb[below + 2]), backdrop,
        "rows after the split must use the second band's scroll");
}

#[test]
fn per_scanline_cgram_renders_each_row_with_its_own_palette() {
    // Mid-frame palette rewrites are as common as mid-frame register
    // writes: Prince of Persia 2 HDMAs backdrop color 0 every line
    // for its sky gradient and restores the palette during vblank,
    // so rendering every row from one end-of-frame CGRAM flattens
    // the gradient into a single solid color. Each row must be
    // composited with the palette captured for ITS scanline.
    let vram = Vram::new();
    let oam = Oam::new();

    let mut regs = PpuRegisters::default();
    regs.inidisp = 0x0F; // full brightness
    regs.bgmode = 1;
    regs.tm = 0x00; // backdrop-only frame
    let lines = vec![regs; SCREEN_HEIGHT];

    let mut sky_top = Cgram::new();
    sky_top.write_color(0, 0x001F); // red backdrop
    let mut sky_bottom = Cgram::new();
    sky_bottom.write_color(0, 0x7C00); // blue backdrop

    let mut cgram_lines = vec![sky_top; SCREEN_HEIGHT];
    for pal in cgram_lines.iter_mut().skip(100) {
        *pal = sky_bottom.clone();
    }

    let (fb, _) =
        render_frame_per_scanline_with_cgram(&vram, &cgram_lines, &oam, &lines);

    let above = (50 * SCREEN_WIDTH) * 4;
    assert_eq!((fb[above], fb[above + 1], fb[above + 2]), (255, 0, 0),
        "rows before the palette change must use their own line's backdrop color");
    let below = (150 * SCREEN_WIDTH) * 4;
    assert_eq!((fb[below], fb[below + 1], fb[below + 2]), (0, 0, 255),
        "rows after the palette change must use the rewritten backdrop color");
}
