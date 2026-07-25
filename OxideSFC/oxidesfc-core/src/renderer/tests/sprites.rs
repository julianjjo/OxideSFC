//! Sprite decoding, geometry and the per-scanline hardware limits.

use super::common::{
    make_2bpp_tile, oam_empty, oam_with_sprite_row, single_pixel_sprite_setup,
};
use crate::cgram::Cgram;
use crate::oam::Oam;
use crate::ppu::PpuRegisters;
use crate::renderer::compose::{render_frame, render_frame_per_scanline_with_status};
use crate::renderer::color::bgr555_to_rgb8;
use crate::renderer::sprites::sprite_size_pair;
use crate::renderer::{SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::vram::Vram;

#[test]
fn sprite_size_pair_codes_6_and_7_are_non_square() {
    // OBSEL size-pair codes 6 and 7 are the two undocumented,
    // non-square pairs. An earlier version approximated both as
    // square (16x16/32x32), which is wrong for the "large" size of
    // code 6 (real hardware: 32x64) and for the "small" size of both
    // codes (real hardware: 16x32, not 16x16). Values cross-checked
    // against the SNESdev wiki PPU registers page and fullsnes's
    // OBJSEL OBJ Size table, which agree exactly:
    //   code 6: small 16x32, large 32x64
    //   code 7: small 16x32, large 32x32
    let (small6, large6) = sprite_size_pair(6);
    assert_eq!(small6, (16, 32), "code 6 small size must be 16x32, not square 16x16");
    assert_eq!(large6, (32, 64), "code 6 large size must be 32x64, not square 32x32");

    let (small7, large7) = sprite_size_pair(7);
    assert_eq!(small7, (16, 32), "code 7 small size must be 16x32, not square 16x16");
    assert_eq!(large7, (32, 32), "code 7 large size is genuinely square 32x32");
}

#[test]
fn sprite_renders_at_its_oam_position_with_object_palette() {
    let mut vram = Vram::new();
    let mut cgram = Cgram::new();
    let mut oam = Oam::new();

    // 4bpp tile, single pixel value 1 at (0,0), rest transparent.
    let mut tile_row0 = [0u8; 8];
    tile_row0[0] = 1;
    let tile = make_2bpp_tile([tile_row0, [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8]]);
    for (i, &b) in tile.iter().enumerate() {
        vram.write(i as u16, b);
    }

    // Sprite 0: X=20, Y=10, tile=0, palette=0, no flip, small size.
    // Raw OAM byte order per fullsnes: byte 0 = X, byte 1 = Y (an
    // earlier renderer read these swapped, and this test encoded the
    // same swapped order, so it kept passing).
    oam.write(0, 20); // X
    oam.write(1, 10); // Y
    oam.write(2, 0); // tile
    oam.write(3, 0x00); // attrs: palette 0
    oam.write(512, 0x00); // high table byte for sprites 0-3: X high bit 0, size 0

    // Object palette 0, pixel index 1 -> CGRAM index 128 + 1 = 129.
    cgram.write(129 * 2, 0xE0); // arbitrary nonzero BGR555 low byte
    cgram.write(129 * 2 + 1, 0x03);

    let mut regs = PpuRegisters::default();
    regs.inidisp = 0x0F;
    regs.obsel = 0x00; // size pair 0 (8x8/16x16), tile base word 0
    regs.tm = 0x10; // enable sprites only

    let fb = render_frame(&vram, &cgram, &oam, &regs);
    let expected = bgr555_to_rgb8(cgram.read_color(129));
    let idx = (10 * SCREEN_WIDTH + 20) * 4;
    assert_eq!((fb[idx], fb[idx + 1], fb[idx + 2]), expected);
}

#[test]
fn tall_sprite_near_the_bottom_wraps_its_rows_to_the_top_of_the_screen() {
    // Hardware's vertical range test is `(line - y) & 0xFF < height`, so a
    // 32-pixel sprite at Y=250 shows rows 6..31 on screen lines 0..25.
    // The old `y >= 0xF0 ? y - 256 : y` biasing reproduces that only for
    // sprites up to 16 pixels tall; taller ones lost the wrapped slice.
    let mut vram = Vram::new();
    let mut cgram = Cgram::new();
    let mut oam = Oam::new();

    // A tile whose every row has pixel value 1 in column 0, so any row of
    // the sprite that draws is detectable. A 32x32 sprite spans 4 tile
    // rows, and consecutive tile rows step the tile number by 16, so
    // tiles 0/16/32/48 are the four covering the sprite's first column.
    // Each 4bpp tile occupies 32 bytes of VRAM.
    let solid_col = [1u8, 0, 0, 0, 0, 0, 0, 0];
    let tile = make_2bpp_tile([solid_col; 8]);
    for tile_num in [0u16, 16, 32, 48] {
        for (i, &b) in tile.iter().enumerate() {
            vram.write(tile_num * 32 + i as u16, b);
        }
    }

    // Sprite 0: X=8, Y=250, large size (32x32 via OBSEL size pair 1).
    oam.write(0, 8); // X
    oam.write(1, 250); // Y
    oam.write(2, 0); // tile
    oam.write(3, 0x00); // attrs: palette 0, priority 0
    oam.write(512, 0x02); // high table: sprite 0 size bit set -> large

    cgram.write(129 * 2, 0xE0);
    cgram.write(129 * 2 + 1, 0x03);

    let mut regs = PpuRegisters::default();
    regs.inidisp = 0x0F;
    regs.obsel = 0x20; // size pair 1 = 8x8 / 32x32
    regs.tm = 0x10; // sprites only

    let fb = render_frame(&vram, &cgram, &oam, &regs);
    let expected = bgr555_to_rgb8(cgram.read_color(129));

    // Row 6 of the sprite lands on screen line 0 ((250 + 6) & 0xFF).
    let top = (0 * SCREEN_WIDTH + 8) * 4;
    assert_eq!(
        (fb[top], fb[top + 1], fb[top + 2]),
        expected,
        "the wrapped rows of a tall low-parked sprite must appear at the top"
    );
    // Row 25 lands on line 25; row 31 wraps to line 25 + 6 = ... the last
    // visible wrapped line is 31 - 6 = 25.
    let last = (25 * SCREEN_WIDTH + 8) * 4;
    assert_eq!(
        (fb[last], fb[last + 1], fb[last + 2]),
        expected,
        "the whole wrapped slice must draw, not only its first line"
    );
    // Line 26 is past the sprite's last row, so it must be backdrop.
    let past = (26 * SCREEN_WIDTH + 8) * 4;
    assert_ne!(
        (fb[past], fb[past + 1], fb[past + 2]),
        expected,
        "the sprite must not extend past its 32-pixel height"
    );
}

#[test]
fn obsel_and_oam_attributes_decode_per_real_hardware_bit_layout() {
    // Regression guard for two systematically-swapped decodes that
    // made every sprite unrecognizable mush: OBSEL's base address is
    // bits 0-2 (NOT 5-7, which are the size pair), and the OAM
    // attribute byte is `vhoopppN` (palette in bits 1-3, flips in
    // bits 6-7, tile bit 8 in bit 0 -- NOT palette 0-2 / flips 5-6).
    let mut vram = Vram::new();
    let mut cgram = Cgram::new();
    let mut oam = Oam::new();

    // OBJ tile base = OBSEL bits 0-2 = 1 -> word 0x2000 -> byte 0x4000.
    // Tile 0 there: pixel value 1 at (0,0), rest transparent.
    let mut tile_row0 = [0u8; 8];
    tile_row0[0] = 1;
    let tile = make_2bpp_tile([tile_row0, [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8]]);
    for (i, &b) in tile.iter().enumerate() {
        vram.write(0x4000 + i as u16, b);
    }
    // The SECOND tile table (tiles 256-511) for OBSEL base 1, name 0
    // sits at word 0x2000 + 0x1000 = 0x3000 -> byte 0x6000. Its tile 0
    // (= sprite tile number 0x100): pixel value 1 at (1,0).
    let mut tile2_row0 = [0u8; 8];
    tile2_row0[1] = 1;
    let tile2 = make_2bpp_tile([tile2_row0, [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8]]);
    for (i, &b) in tile2.iter().enumerate() {
        vram.write(0x6000 + i as u16, b);
    }

    // Sprite 0 at (20,10), tile 0, attrs = palette 1 (bits 1-3).
    // (raw byte order: byte 0 = X, byte 1 = Y)
    oam.write(0, 20);
    oam.write(1, 10);
    oam.write(2, 0);
    oam.write(3, 0b0000_0010); // palette 1
    oam.write(512, 0x00);
    // Sprite 1 at (40,10), tile bit8 set via attr bit 0 -> tile 0x100
    // from the second table, palette 0, H-FLIP via bit 6.
    oam.write(4, 40);
    oam.write(5, 10);
    oam.write(6, 0);
    oam.write(7, 0b0100_0001); // hflip + tile bit 8
    // (sprite 1 shares high-table byte 512; both X high bits 0, small size)

    // OBJ palette 1, index 1 -> CGRAM 128 + 16 + 1 = 145.
    cgram.write(145 * 2, 0x1F);
    cgram.write(145 * 2 + 1, 0x00);
    // OBJ palette 0, index 1 -> CGRAM 129.
    cgram.write(129 * 2, 0xE0);
    cgram.write(129 * 2 + 1, 0x03);

    let mut regs = PpuRegisters::default();
    regs.inidisp = 0x0F;
    regs.obsel = 0x01; // size pair 0 (8x8), name 0, BASE = word 0x2000
    regs.tm = 0x10;

    let fb = render_frame(&vram, &cgram, &oam, &regs);

    // Sprite 0's pixel must use OBJ palette 1 (CGRAM 145).
    let expected_pal1 = bgr555_to_rgb8(cgram.read_color(145));
    let i0 = (10 * SCREEN_WIDTH + 20) * 4;
    assert_eq!((fb[i0], fb[i0 + 1], fb[i0 + 2]), expected_pal1,
        "attr bits 1-3 must select the OBJ palette");

    // Sprite 1: tile 0x100 comes from the second table; its source
    // pixel at x=1 lands at x = 7-1 = 6 after the H-flip (attr bit 6).
    let expected_pal0 = bgr555_to_rgb8(cgram.read_color(129));
    let i1 = (10 * SCREEN_WIDTH + 40 + 6) * 4;
    assert_eq!((fb[i1], fb[i1 + 1], fb[i1 + 2]), expected_pal0,
        "attr bit 0 must select the second tile table and bit 6 must H-flip");
}

#[test]
fn large_sprite_tile_column_wraps_within_low_nibble() {
    // Regression guard for the multi-tile sprite tile-number bug: the
    // column component must wrap WITHIN the tile number's low nibble,
    // not carry into the row/table bits. Base tile 0x0E plus a column
    // offset of 3 (the 4th 8x8 cell of a 32x32 sprite) must land on
    // tile (0x0E + 3) & 0x0F = 0x01, NOT the unwrapped 0x0E + 3 = 0x11.
    let mut vram = Vram::new();
    let mut cgram = Cgram::new();
    let mut oam = Oam::new();

    // Tile index 1 (the CORRECT wrapped target): pixel value 1 at (0,0).
    let mut tile1_row0 = [0u8; 8];
    tile1_row0[0] = 1;
    let tile1 = make_2bpp_tile([tile1_row0, [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8]]);
    for (i, &b) in tile1.iter().enumerate() {
        vram.write(1 * 32 + i as u16, b); // 4bpp tile 1 starts at byte 32
    }
    // Tile index 0x11 = 17 (the WRONG unwrapped target) is left all
    // zero/transparent, so if the bug regresses the sprite pixel
    // silently disappears instead of showing the wrong graphics.

    // Sprite 0 at (20,10), base tile 0x0E, palette 0, no flip.
    // (raw byte order: byte 0 = X, byte 1 = Y)
    oam.write(0, 20); // X
    oam.write(1, 10); // Y
    oam.write(2, 0x0E); // tile low byte
    oam.write(3, 0x00); // attrs: palette 0, no flip, tile bit 8 = 0
    // Secondary OAM byte for sprites 0-3: sprite 0's size bit (bit 1)
    // set -> large size; X high bit (bit 0) clear.
    oam.write(512, 0x02);

    // OBJ palette 0, pixel index 1 -> CGRAM 128 + 1 = 129.
    cgram.write(129 * 2, 0x1F);
    cgram.write(129 * 2 + 1, 0x00);

    let mut regs = PpuRegisters::default();
    regs.inidisp = 0x0F;
    regs.obsel = 0x20; // size-pair code 1 (8x8/32x32), tile base word 0
    regs.tm = 0x10; // enable sprites only

    let fb = render_frame(&vram, &cgram, &oam, &regs);

    // Screen pixel for sprite-local (tx=24, ty=0) -> tile_row_idx=0,
    // tile_col_idx=3 -> must read wrapped tile index 1.
    let expected = bgr555_to_rgb8(cgram.read_color(129));
    let idx = (10 * SCREEN_WIDTH + 20 + 24) * 4;
    assert_eq!((fb[idx], fb[idx + 1], fb[idx + 2]), expected,
        "column offset must wrap within the tile number's low nibble, not carry into higher bits");
}

#[test]
fn oam_entry_byte_order_is_x_then_y_per_real_hardware() {
    // Pins the raw OAM byte order (fullsnes: byte 0 = X low 8 bits,
    // byte 1 = Y). The renderer used to read these swapped, which
    // transposed every sprite around the screen diagonal AND painted a
    // permanent garbage column at x=240: sprites parked offscreen with
    // Y=$F0 were drawn at X=240 with Y = whatever stale X they had.
    // Deliberately uses X != Y and an asymmetric assertion so a swap
    // cannot pass.
    let mut vram = Vram::new();
    for y in 0..8u16 {
        vram.write(y * 2, 0xFF); // tile 0: all pixels value 1
    }
    let mut cgram = Cgram::new();
    cgram.write(129 * 2, 0xFF);
    cgram.write(129 * 2 + 1, 0x7F);
    let mut oam = Oam::new();
    for s in 0..128u16 {
        oam.write(s * 4 + 1, 240); // park everything: Y byte = 240
    }
    // Sprite 0 raw bytes: [X=200, Y=50, tile=0, attr=0].
    oam.write(0, 200);
    oam.write(1, 50);
    oam.write(2, 0);
    oam.write(3, 0);
    oam.write(512, 0);

    let mut regs = PpuRegisters::default();
    regs.inidisp = 0x0F;
    regs.obsel = 0;
    regs.tm = 0x10;

    let fb = render_frame(&vram, &cgram, &oam, &regs);
    let at = |x: usize, y: usize| (fb[(y * SCREEN_WIDTH + x) * 4], fb[(y * SCREEN_WIDTH + x) * 4 + 1]) != (0, 0);
    assert!(
        at(200, 50),
        "raw OAM [200, 50] must place the sprite at X=200, Y=50"
    );
    assert!(
        !at(50, 200),
        "the transposed position must stay empty -- byte 0 is X, not Y"
    );
    // The parked sprites (Y byte = 240) must not paint anything at the
    // right edge -- the old swap drew them all in a column at x=240.
    for y in 0..SCREEN_HEIGHT {
        for x in 240..SCREEN_WIDTH {
            if y == 50 && x >= 200 {
                continue; // (not reachable: sprite spans 200-207)
            }
            assert!(
                !at(x, y),
                "parked sprite leaked to ({}, {}) -- Y=240 must keep it offscreen",
                x,
                y
            );
        }
    }
}

#[test]
fn sprite_range_limit_drops_the_33rd_sprite_on_a_line_and_flags_stat77() {
    // Hardware evaluates at most 32 sprites per scanline; a 33rd
    // in-range sprite is not drawn and sets STAT77 bit 6 (range over).
    let (vram, cgram, regs) = single_pixel_sprite_setup();
    let oam = oam_with_sprite_row(33, 7, false); // all 33 share lines 10-17
    let lines = vec![regs; SCREEN_HEIGHT];
    let (fb, flags) = render_frame_per_scanline_with_status(&vram, &cgram, &oam, &lines);

    let drawn = (10 * SCREEN_WIDTH) * 4; // sprite 0's pixel at (0,10)
    assert_ne!((fb[drawn], fb[drawn + 1], fb[drawn + 2]), (0, 0, 0), "sprite 0 must render");
    let dropped = (10 * SCREEN_WIDTH + 32 * 7) * 4; // sprite 32's pixel
    assert_eq!(
        (fb[dropped], fb[dropped + 1], fb[dropped + 2]),
        (0, 0, 0),
        "the 33rd sprite on the line must be dropped by the range limit"
    );
    assert_ne!(flags & 0x40, 0, "STAT77 range-over flag must be set");
    assert_eq!(flags & 0x80, 0, "33 8x8 sprites = 32 evaluated tiles: no time-over");
}

#[test]
fn sprite_tile_budget_drops_sprites_past_34_tiles_and_flags_time_over() {
    // 18 sprites of 16x16 = 2 tiles each on one line: the first 17
    // consume the full 34-tile budget, the 18th is dropped and STAT77
    // bit 7 (time over) sets.
    let (vram, cgram, regs) = single_pixel_sprite_setup();
    let oam = oam_with_sprite_row(18, 14, true);
    let lines = vec![regs; SCREEN_HEIGHT];
    let (fb, flags) = render_frame_per_scanline_with_status(&vram, &cgram, &oam, &lines);

    let drawn = (10 * SCREEN_WIDTH) * 4;
    assert_ne!((fb[drawn], fb[drawn + 1], fb[drawn + 2]), (0, 0, 0), "sprite 0 must render");
    let dropped = (10 * SCREEN_WIDTH + 17 * 14) * 4; // sprite 17's top-left pixel
    assert_eq!(
        (fb[dropped], fb[dropped + 1], fb[dropped + 2]),
        (0, 0, 0),
        "the sprite past the 34-tile budget must be dropped"
    );
    assert_ne!(flags & 0x80, 0, "STAT77 time-over flag must be set");
}

#[test]
fn first_sprite_rotation_changes_which_sprite_wins_overlaps() {
    // With $2103 bit 7 priority rotation, evaluation (and overlap
    // priority) starts at FirstSprite instead of sprite 0.
    let (vram, mut cgram, mut regs) = single_pixel_sprite_setup();
    // Sprite 1 uses OBJ palette 1 so the two sprites are colorimetrically
    // distinguishable (CGRAM 128 + 16 + 1 = 145).
    cgram.write(145 * 2, 0x1F);
    cgram.write(145 * 2 + 1, 0x00);

    let mut oam = oam_empty();
    for i in 0..2u16 {
        oam.write(i * 4, 20); // both at (20, 10)
        oam.write(i * 4 + 1, 10);
        oam.write(i * 4 + 2, 0);
        oam.write(i * 4 + 3, if i == 1 { 0b0000_0010 } else { 0 }); // sprite 1: palette 1
    }
    for i in 2..128u16 {
        oam.write(i * 4 + 1, 0xF0);
    }

    let idx = (10 * SCREEN_WIDTH + 20) * 4;
    let fb = render_frame(&vram, &cgram, &oam, &regs);
    let sprite0_color = bgr555_to_rgb8(cgram.read_color(129));
    assert_eq!((fb[idx], fb[idx + 1], fb[idx + 2]), sprite0_color, "FirstSprite 0: sprite 0 wins the overlap");

    regs.first_sprite = 1;
    let fb = render_frame(&vram, &cgram, &oam, &regs);
    let sprite1_color = bgr555_to_rgb8(cgram.read_color(145));
    assert_eq!((fb[idx], fb[idx + 1], fb[idx + 2]), sprite1_color, "FirstSprite 1: sprite 1 now has the highest priority");
}
