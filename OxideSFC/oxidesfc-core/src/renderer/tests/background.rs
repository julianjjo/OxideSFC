//! Background-layer rendering: palettes, tile sizes, offset-per-tile,
//! mosaic, priority and the hi-res/interlace collapse.

use super::common::{make_2bpp_tile, oam_empty};
use crate::renderer::color::bgr555_to_rgb8;
use crate::cgram::Cgram;
use crate::oam::Oam;
use crate::ppu::PpuRegisters;
use crate::renderer::compose::render_frame;
use crate::renderer::SCREEN_WIDTH;
use crate::vram::Vram;

#[test]
fn mode1_bg1_tile_renders_with_correct_palette_colors() {
    let mut vram = Vram::new();
    let mut cgram = Cgram::new();
    let oam = Oam::new();

    // A 4bpp tile needs 2 bitplane pairs (32 bytes); use only pixel
    // values 0 (transparent) and 1 so a single 2bpp pair suffices and
    // the second pair (all zero) contributes nothing.
    let mut tile_row0 = [0u8; 8];
    tile_row0[0] = 1; // first pixel uses palette index 1
    let tile = make_2bpp_tile([tile_row0, [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8]]);
    // Tile data base word 0 -> byte 0. Tile index 0.
    for (i, &b) in tile.iter().enumerate() {
        vram.write(i as u16, b);
    }

    // Tilemap base word 0x400 (so it doesn't overlap tile data).
    // Map entry 0 (top-left tile) = tile index 0, palette 0, no flip.
    vram.write_word(0x400 * 2, 0x0000);

    // CGRAM color for BG palette 0, pixel index 1 -> entry 1 -> pure
    // red.
    cgram.write(2, 0xFF);
    cgram.write(2 + 1, 0x7F); // low byte 0xFF, high byte 0x7F -> 0x7FFF -> R=31,G=31,B=31? recompute below

    let mut regs = PpuRegisters {
        inidisp: 0x0F, // not forced blank, full brightness
        bgmode: 1, // BG1 = 4bpp
        bg12nba: 0x00, // BG1 tile data base = 0
        tm: 0x01, // enable BG1 only
        ..Default::default()
    };
    regs.bg_sc[0] = 0x04; // tilemap base = (0x04>>2)*0x400 = 0x400, size 32x32

    let fb = render_frame(&vram, &cgram, &oam, &regs);
    let expected = bgr555_to_rgb8(cgram.read_color(1));
    let idx = 0; // pixel (0,0)
    assert_eq!((fb[idx], fb[idx + 1], fb[idx + 2]), expected);
    assert_eq!(fb[idx + 3], 0xFF);

    // A pixel using value 0 elsewhere in the same tile must show the
    // backdrop color (CGRAM index 0), not the tile's palette color.
    let backdrop = bgr555_to_rgb8(cgram.read_color(0));
    let idx2 = 4; // pixel (1,0), tile pixel value 0
    assert_eq!((fb[idx2], fb[idx2 + 1], fb[idx2 + 2]), backdrop);
}

#[test]
fn offset_per_tile_mode2_overrides_bg1_h_scroll_per_column() {
    // Mode 2: BG3's tilemap supplies per-8-pixel-column scroll
    // overrides. Column 0 always uses the normal scroll; column 1's
    // override entry (BG3 map tile (0,0)) redirects BG1's horizontal
    // offset so the solid tile at world column 0 repeats there;
    // column 2 has no valid override and stays at the normal scroll.
    let mut vram = Vram::new();
    let mut cgram = Cgram::new();

    // BG1 4bpp tile 1: solid pixel value 1 (plane 0 = 0xFF each row).
    for row in 0..8u16 {
        vram.write(32 + row * 2, 0xFF);
    }
    // BG1 tilemap at word 0x400: tile 1 at map position (0,0) only.
    vram.write(0x800, 0x01);
    vram.write(0x801, 0x00);
    // BG3 tilemap at word 0x800: OPT entry for screen column 1 --
    // valid-for-BG1 (bit 13) + H offset 0x3F8 (walks the sample back
    // to world column 0: 8 + 1016 wraps to tile 0 of the 256-wide map).
    vram.write(0x1000, 0xF8);
    vram.write(0x1001, 0x23);

    cgram.write(2, 0xE0); // BG palette 0, pixel 1
    cgram.write(2 + 1, 0x03);

    let mut regs = PpuRegisters {
        inidisp: 0x0F,
        bgmode: 2,
        tm: 0x01, // BG1 only
        ..Default::default()
    };
    regs.bg_sc[0] = 0x04; // BG1 tilemap base word 0x400
    regs.bg_sc[2] = 0x08; // BG3 tilemap base word 0x800 (the OPT table)

    let fb = render_frame(&vram, &cgram, &oam_empty(), &regs);
    let color = bgr555_to_rgb8(cgram.read_color(1));
    let px = |x: usize| {
        let i = x * 4; // row 0
        (fb[i], fb[i + 1], fb[i + 2])
    };
    assert_eq!(px(0), color, "column 0 uses the normal (zero) scroll: tile 1 shows");
    assert_eq!(px(8), color, "column 1's OPT entry must override BG1's H offset");
    assert_eq!(px(16), (0, 0, 0), "column 2 has no valid OPT entry: backdrop");
}

#[test]
fn bgmode_size_bit_selects_16x16_tiles_with_obj_style_cell_layout() {
    // Mode 1 with BGMODE bit 4 (BG1 16x16 tiles): the tile's four 8x8
    // cells are base, base+1, base+16, base+17 -- pixel (8,0) must
    // come from tile base+1 and pixel (0,8) from tile base+16.
    let mut vram = Vram::new();
    let mut cgram = Cgram::new();
    let oam = Oam::new();

    let solid = make_2bpp_tile([[1u8; 8]; 8]);
    // 4bpp tiles are 32 bytes each. Tile 2 (base+1) and tile 17
    // (base+16) are solid; base tile 1 and tile 18 stay transparent.
    for (i, &b) in solid.iter().enumerate() {
        vram.write(2 * 32 + i as u16, b);
        vram.write(17 * 32 + i as u16, b);
    }
    vram.write_word(0x400 * 2, 0x0001); // map (0,0) -> base tile 1
    cgram.write(2, 0x1F); // CGRAM 1 = red
    cgram.write(3, 0x00);

    let mut regs = PpuRegisters {
        inidisp: 0x0F,
        bgmode: 0x11, // mode 1 + BG1 16x16 tiles
        tm: 0x01,
        ..Default::default()
    };
    regs.bg_sc[0] = 0x04;

    let fb = render_frame(&vram, &cgram, &oam, &regs);
    let red = bgr555_to_rgb8(cgram.read_color(1));
    let backdrop = bgr555_to_rgb8(cgram.read_color(0));

    let at = |x: usize, y: usize| {
        let i = (y * SCREEN_WIDTH + x) * 4;
        (fb[i], fb[i + 1], fb[i + 2])
    };
    assert_eq!(at(0, 0), backdrop, "cell (0,0) is the transparent base tile");
    assert_eq!(at(8, 0), red, "cell (1,0) must be tile base+1");
    assert_eq!(at(0, 8), red, "cell (0,1) must be tile base+16");
    assert_eq!(at(8, 8), backdrop, "cell (1,1) is the transparent base+17");
}

#[test]
fn mode5_hires_maps_two_dots_per_output_pixel() {
    // Mode 5: tiles are 16 dots wide in a 512-dot space; each output
    // pixel covers two dots. With the LEFT 8x8 cell solid and the
    // right transparent, output pixels 0-3 (dots 0-7) show the color
    // and pixels 4-7 (dots 8-15) show the backdrop.
    let mut vram = Vram::new();
    let mut cgram = Cgram::new();
    let oam = Oam::new();

    let solid = make_2bpp_tile([[1u8; 8]; 8]);
    for (i, &b) in solid.iter().enumerate() {
        vram.write(32 + i as u16, b); // 4bpp tile 1 solid (left cell)
    }
    vram.write_word(0x400 * 2, 0x0001); // map (0,0) -> base tile 1 (cells 1,2)
    cgram.write(2, 0x1F);
    cgram.write(3, 0x00);

    let mut regs = PpuRegisters {
        inidisp: 0x0F,
        bgmode: 5, // BG1 = 4bpp, 16-wide tiles
        tm: 0x01,
        ..Default::default()
    };
    regs.bg_sc[0] = 0x04;

    let fb = render_frame(&vram, &cgram, &oam, &regs);
    let red = bgr555_to_rgb8(cgram.read_color(1));
    let backdrop = bgr555_to_rgb8(cgram.read_color(0));
    let at = |x: usize| {
        let i = x * 4;
        (fb[i], fb[i + 1], fb[i + 2])
    };
    assert_eq!(at(0), red, "dots 0/1 (left cell) -> output pixel 0");
    assert_eq!(at(3), red, "dots 6/7 (left cell) -> output pixel 3");
    assert_eq!(at(4), backdrop, "dots 8/9 (transparent right cell) -> output pixel 4");
}

#[test]
fn mode5_hires_averages_the_two_dots_of_each_output_pixel() {
    // A tile row alternating between two palette indices means every
    // output pixel spans one dot of each color -- the result must be
    // their per-channel average, not either dot alone.
    let mut vram = Vram::new();
    let mut cgram = Cgram::new();
    let oam = Oam::new();

    let tile = make_2bpp_tile([[1, 2, 1, 2, 1, 2, 1, 2]; 8]);
    for (i, &b) in tile.iter().enumerate() {
        vram.write(32 + i as u16, b);
    }
    vram.write_word(0x400 * 2, 0x0001);
    // Color 1 = pure red (r=31), color 2 = pure blue (b=31).
    cgram.write(2, 0x1F);
    cgram.write(3, 0x00);
    cgram.write(4, 0x00);
    cgram.write(5, 0x7C);

    let mut regs = PpuRegisters {
        inidisp: 0x0F,
        bgmode: 5,
        tm: 0x01,
        ..Default::default()
    };
    regs.bg_sc[0] = 0x04;

    let fb = render_frame(&vram, &cgram, &oam, &regs);
    // Average of (31,0,0) and (0,0,31) in 5-bit space = (15,0,15).
    let expected = bgr555_to_rgb8(15 | (15 << 10));
    assert_eq!((fb[0], fb[1], fb[2]), expected,
        "output pixel 0 must average its red and blue dots");
}

#[test]
fn mode5_interlace_averages_both_field_lines() {
    // With SETINI bit 0 (interlace) in mode 5, each output row spans
    // two half-lines (the two fields). A tile whose row 0 is red and
    // row 1 is blue must render output row 0 as their average; without
    // the interlace bit, row 0 is pure red.
    let mut vram = Vram::new();
    let mut cgram = Cgram::new();
    let oam = Oam::new();

    let mut rows = [[0u8; 8]; 8];
    rows[0] = [1; 8];
    rows[1] = [2; 8];
    let tile = make_2bpp_tile(rows);
    for (i, &b) in tile.iter().enumerate() {
        vram.write(32 + i as u16, b);
    }
    vram.write_word(0x400 * 2, 0x0001);
    cgram.write(2, 0x1F); // color 1 = red
    cgram.write(3, 0x00);
    cgram.write(4, 0x00); // color 2 = blue
    cgram.write(5, 0x7C);

    let mut regs = PpuRegisters {
        inidisp: 0x0F,
        bgmode: 5,
        tm: 0x01,
        ..Default::default()
    };
    regs.bg_sc[0] = 0x04;

    let fb = render_frame(&vram, &cgram, &oam, &regs);
    let red = bgr555_to_rgb8(cgram.read_color(1));
    assert_eq!((fb[0], fb[1], fb[2]), red, "non-interlaced row 0 is the tile's row 0");

    regs.setini = 0x01; // interlace
    let fb2 = render_frame(&vram, &cgram, &oam, &regs);
    let expected = bgr555_to_rgb8(15 | (15 << 10)); // avg of red and blue
    assert_eq!((fb2[0], fb2[1], fb2[2]), expected,
        "interlaced row 0 must average the tile's rows 0 and 1 (the two fields)");
}

#[test]
fn mosaic_repeats_each_blocks_top_left_pixel() {
    // BG1 with a single red tile at map (0,0): normally pixel (8,0) is
    // backdrop (tile 1 of the map is transparent). With an 8x8 mosaic
    // whose block origin (8,0) is transparent, nothing changes there --
    // but pixel (7,0)..(0,0) belong to block origin (0,0), so the whole
    // first block is red. More telling: with mosaic 16x16, pixel
    // (12, 12) samples block origin (0,0) -- INSIDE the tile -- so it
    // must be red even though the un-mosaicked tile only spans 8x8.
    let mut vram = Vram::new();
    let mut cgram = Cgram::new();
    let oam = Oam::new();

    let solid = make_2bpp_tile([[1u8; 8]; 8]);
    for (i, &b) in solid.iter().enumerate() {
        vram.write(32 + i as u16, b); // 4bpp tile 1 (bytes 32..)
    }
    vram.write_word(0x400 * 2, 0x0001); // map (0,0) -> tile 1
    cgram.write(2, 0x1F); // CGRAM 1 = red
    cgram.write(3, 0x00);

    let mut regs = PpuRegisters {
        inidisp: 0x0F,
        bgmode: 1,
        tm: 0x01,
        mosaic: 0xF1, // size 16, enabled for BG1
        ..Default::default()
    };
    regs.bg_sc[0] = 0x04;

    let fb = render_frame(&vram, &cgram, &oam, &regs);
    let red = bgr555_to_rgb8(cgram.read_color(1));
    let idx = (12 * SCREEN_WIDTH + 12) * 4;
    assert_eq!((fb[idx], fb[idx + 1], fb[idx + 2]), red,
        "pixel (12,12) must repeat block origin (0,0)'s red with 16x16 mosaic");

    // Without mosaic the same pixel is backdrop (outside the 8x8 tile).
    regs.mosaic = 0x00;
    let fb2 = render_frame(&vram, &cgram, &oam, &regs);
    let backdrop = bgr555_to_rgb8(cgram.read_color(0));
    assert_eq!((fb2[idx], fb2[idx + 1], fb2[idx + 2]), backdrop);
}

#[test]
fn mode1_high_priority_bg3_tile_draws_in_front_of_bg1() {
    // Per-tile priority regression guard: in mode 1 with the BGMODE
    // bit-3 BG3-priority flag set, a BG3 tile whose tilemap priority
    // bit is set is the frontmost layer -- it must overwrite a BG1
    // pixel at the same location. The old renderer used a fixed
    // BG4<BG3<BG2<BG1 order and always drew BG1 on top, which is wrong
    // for this (very common in SMW) configuration.
    let mut vram = Vram::new();
    let mut cgram = Cgram::new();
    let oam = Oam::new();

    // Two distinct solid tiles: tile 0 (all pixel value 1) for BG1,
    // tile 1 (all pixel value 1) for BG3 -- same pixel value, different
    // palettes so we can tell which layer won.
    let solid = make_2bpp_tile([[1u8; 8]; 8]);
    for (i, &b) in solid.iter().enumerate() {
        vram.write(i as u16, b); // tile 0 at bytes 0..
    }

    // BG1 tilemap at word 0x1000, entry 0 -> tile 0, palette 0, priority 0.
    vram.write_word(0x1000 * 2, 0x0000);
    // BG3 tilemap at word 0x2000, entry 0 -> tile 0, palette 1, priority 1 (0x2000).
    vram.write_word(0x2000 * 2, 0x2000 | (1 << 10));

    // BG1 palette 0 index 1 -> CGRAM 1 = red-ish; BG3 palette 1 (2bpp)
    // index 1 -> CGRAM (1*4 + 1) = 5 = green-ish. Distinct colors.
    cgram.write(2, 0x1F); cgram.write(2 + 1, 0x00); // CGRAM1 = red
    cgram.write(5 * 2, 0xE0); cgram.write(5 * 2 + 1, 0x03); // CGRAM5 = green

    let mut regs = PpuRegisters {
        inidisp: 0x0F,
        bgmode: 0x09, // mode 1 + BG3 priority flag (bit 3)
        bg12nba: 0x00, // BG1 tile data base word 0
        bg34nba: 0x00, // BG3 tile data base word 0
        tm: 0x05, // enable BG1 (bit0) + BG3 (bit2)
        ..Default::default()
    };
    regs.bg_sc[0] = 0x10; // BG1 tilemap base word = (0x10>>2)*0x400 = 0x1000
    regs.bg_sc[2] = 0x20; // BG3 tilemap base word = (0x20>>2)*0x400 = 0x2000

    let fb = render_frame(&vram, &cgram, &oam, &regs);
    let green = bgr555_to_rgb8(cgram.read_color(5));
    assert_eq!((fb[0], fb[1], fb[2]), green,
        "high-priority BG3 must render in front of BG1 in mode 1 with the BG3-priority flag set");
}
