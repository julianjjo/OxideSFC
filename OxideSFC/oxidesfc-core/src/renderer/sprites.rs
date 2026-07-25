//! Sprite (OBJ) evaluation and drawing: OAM decoding, the per-scanline
//! 32-sprite / 34-tile hardware limits, and priority-ordered rendering.

use super::tile::decode_tile_row;
use super::{LAYER_OBJ, LAYER_OBJ_PAL03, SCREEN_WIDTH};
use crate::cgram::Cgram;
use crate::oam::Oam;
use crate::ppu::PpuRegisters;
use crate::vram::Vram;

/// Per-scanline sprite evaluation results for one band: bit N of
/// `masks[line - y0]` says sprite N survived the hardware's per-line
/// limits (at most 32 sprites in range, at most 34 8-pixel tiles) on that
/// line, plus the accumulated STAT77 flags (bit 6 = range over, bit 7 =
/// time over). Evaluation walks OAM starting at `PpuRegisters::
/// first_sprite` exactly like the hardware's priority scan (snes9x
/// gfx.cpp `SetupOBJ`).
pub(super) struct SpriteEval {
    y0: usize,
    masks: Vec<u128>,
    /// STAT77's range-over (bit 6) and time-over (bit 7) flags accumulated
    /// across the band, which the compositor returns to the caller so the
    /// bus can expose them at $213E.
    pub(super) range_time_over: u8,
}

pub(super) fn evaluate_sprites(oam: &Oam, regs: &PpuRegisters, y0: usize, y1: usize) -> SpriteEval {
    let (small_size, large_size) = sprite_size_pair((regs.obsel >> 5) & 0x07);
    let first = regs.first_sprite & 0x7F;

    // Decode each sprite's screen rectangle once.
    let mut geom = [(0i32, 0i32, 0u32, 0u32); 128];
    for (s, slot) in geom.iter_mut().enumerate() {
        let base = (s * 4) as u16;
        let x_low = oam.read(base);
        let y_raw = oam.read(base + 1);
        let high_table_byte = oam.read(512 + (s as u16) / 4);
        let shift = ((s % 4) * 2) as u8;
        let x_high_bit = (high_table_byte >> shift) & 0x01;
        let size_bit = (high_table_byte >> (shift + 1)) & 0x01;
        let (w, h) = if size_bit != 0 { large_size } else { small_size };
        let x_full = ((x_high_bit as u16) << 8) | (x_low as u16);
        let x: i32 = if x_full & 0x100 != 0 { (x_full as i32) - 512 } else { x_full as i32 };
        // Y stays the raw 0-255 register value: hardware decides whether a
        // sprite covers a line with `(line - y) & 0xFF < height`, so a tall
        // sprite parked near the bottom wraps and shows its lower rows at the
        // top of the screen. This used to bias `y_raw >= 0xF0` down by 256,
        // which reproduces the wrap only for sprites up to 16 pixels tall --
        // 32- and 64-pixel sprites silently lost their wrapped slice.
        *slot = (x, y_raw as i32, w, h);
    }

    let mut masks = vec![0u128; y1 - y0];
    let mut range_time_over = 0u8;
    for line in y0..y1 {
        let ly = line as i32;
        let mut in_range = 0u32;
        let mut tiles = 34i32;
        let mut mask = 0u128;
        for k in 0..128u8 {
            let s = (first.wrapping_add(k) & 0x7F) as usize;
            let (x, y, w, h) = geom[s];
            // Hardware's vertical range test, wrapping at 256 (see `geom`).
            if ((ly - y) & 0xFF) as u32 >= h {
                continue;
            }
            if x + (w as i32) <= 0 || x >= SCREEN_WIDTH as i32 {
                continue;
            }
            if in_range >= 32 {
                range_time_over |= 0x40; // range over: a 33rd sprite on this line
                continue;
            }
            in_range += 1;
            if tiles <= 0 {
                range_time_over |= 0x80; // no tile budget left: sprite dropped
                continue;
            }
            tiles -= (w / 8) as i32;
            if tiles < 0 {
                // Budget ran out inside this sprite: flag time-over. (Real
                // hardware truncates the sprite's trailing tiles; drawing
                // it whole keeps this simple and errs on the visible side.)
                range_time_over |= 0x80;
            }
            mask |= 1u128 << s;
        }
        masks[line - y0] = mask;
    }
    SpriteEval { y0, masks, range_time_over }
}

/// (width, height) in pixels for each of OBSEL's 8 size-pair codes,
/// returned as (small, large). Codes 6/7 are the two undocumented,
/// non-square pairs (16x32/32x64 and 16x32/32x32 respectively) --
/// cross-checked against the SNESdev wiki PPU registers page and
/// fullsnes's OBJSEL OBJ Size table, which agree exactly.
/// `size_code` is OBSEL bits 5-7 (already shifted down by the caller).
pub(super) fn sprite_size_pair(size_code: u8) -> ((u32, u32), (u32, u32)) {
    match size_code & 0x07 {
        0 => ((8, 8), (16, 16)),
        1 => ((8, 8), (32, 32)),
        2 => ((8, 8), (64, 64)),
        3 => ((16, 16), (32, 32)),
        4 => ((16, 16), (64, 64)),
        5 => ((32, 32), (64, 64)),
        6 => ((16, 32), (32, 64)),
        _ => ((16, 32), (32, 32)),
    }
}

pub(super) fn draw_sprites(
    buf: &mut [u16],
    layer_buf: &mut [u8],
    vram: &Vram,
    cgram: &Cgram,
    oam: &Oam,
    regs: &PpuRegisters,
    sprite_eval: &SpriteEval,
    want_priority: u8,
    skip: &[bool; SCREEN_WIDTH],
    y0: usize,
    y1: usize,
) {
    // OBSEL ($2101) layout is `sssnnbbb`: bits 0-2 = OBJ tile base (8K-word
    // steps), bits 3-4 = name select (gap to the second 256-tile table),
    // bits 5-7 = the size-pair code. An earlier version read the BASE from
    // bits 5-7 and the SIZE from bits 0-2 (exactly swapped) -- with SMW's
    // in-level OBSEL=$03 that decoded sprites from VRAM word 0 (background
    // tile data!) at 16x16/32x32 instead of the real sprite graphics at
    // word $6000 at 8x8/16x16, turning Mario and every enemy into
    // unrecognizable colored mush.
    let tile_data_base_word = ((regs.obsel & 0x07) as u16) * 0x2000;
    let name_select = ((regs.obsel >> 3) & 0x03) as u16;
    let (small_size, large_size) = sprite_size_pair((regs.obsel >> 5) & 0x07);

    // Iterate in reverse rotation order so that, within this priority
    // level, the sprite closest to FirstSprite ends up drawn last (on
    // top) -- hardware's overlap rule is "closest to FirstSprite in
    // evaluation order wins", which reduces to "lower OAM index wins"
    // when priority rotation is off (FirstSprite = 0). Only sprites whose
    // OAM priority (attr bits 4-5) equals `want_priority` are drawn in
    // this pass; the caller invokes the four priority levels in the
    // correct back-to-front slots for the current BG mode.
    let first_sprite = regs.first_sprite & 0x7F;
    for k in (0..128u8).rev() {
        let sprite_idx = first_sprite.wrapping_add(k) & 0x7F;
        let base = (sprite_idx as usize) * 4;
        // OAM entry layout per fullsnes: byte 0 = X (low 8 bits), byte 1 =
        // Y, byte 2 = tile, byte 3 = attributes. These first two used to
        // be read SWAPPED (byte 0 as Y), which transposed every sprite
        // around the screen diagonal -- subtle on near-diagonal scenes,
        // but it scattered SMW's walking enemies into vertical stacks and
        // painted a permanent garbage column at x=240 (sprites parked
        // offscreen with Y=$F0 came back as X=240 with Y = their stale X).
        let x_low = oam.read(base as u16);
        let y_raw = oam.read(base as u16 + 1);
        let tile_low = oam.read(base as u16 + 2) as u16;
        let attrs = oam.read(base as u16 + 3);

        // OAM attribute bits 4-5 = sprite priority (0-3).
        if (attrs >> 4) & 0x03 != want_priority {
            continue;
        }

        let high_table_byte = oam.read(512 + (sprite_idx as u16) / 4);
        let shift = (sprite_idx % 4) * 2;
        let x_high_bit = (high_table_byte >> shift) & 0x01;
        let size_bit = (high_table_byte >> (shift + 1)) & 0x01;

        let (w, h) = if size_bit != 0 { large_size } else { small_size };

        let x_full = ((x_high_bit as u16) << 8) | (x_low as u16);
        let x: i32 = if x_full & 0x100 != 0 { (x_full as i32) - 512 } else { x_full as i32 };
        // Raw 0-255 Y; rows are placed mod 256 below, matching hardware's
        // wrapping range test (see `evaluate_sprites`).
        let y: i32 = y_raw as i32;

        if x + (w as i32) <= 0 || x >= SCREEN_WIDTH as i32 {
            continue;
        }

        // OAM attribute byte layout is `vhoopppN`: bit 7 = v-flip, bit 6 =
        // h-flip, bits 5-4 = priority, bits 3-1 = palette, bit 0 = tile
        // number bit 8 (selects the second 256-tile table). An earlier
        // version read palette from bits 0-2 and flips from bits 5/6 --
        // every sprite got the wrong palette and priority-bit-contaminated
        // "flips", compounding the OBSEL swap above.
        let palette_num = ((attrs >> 1) & 0x07) as u16;
        let flip_h = (attrs & 0x40) != 0;
        let flip_v = (attrs & 0x80) != 0;
        let tile_base = tile_low | (((attrs & 0x01) as u16) << 8);

        for ty in 0..h {
            let screen_y = (y + ty as i32) & 0xFF;
            if screen_y < y0 as i32 || screen_y >= y1 as i32 {
                continue;
            }
            // Hardware per-line limits: skip lines where this sprite lost
            // the 32-sprites/34-tiles evaluation (see `evaluate_sprites`).
            if sprite_eval.masks[screen_y as usize - sprite_eval.y0] & (1u128 << sprite_idx) == 0 {
                continue;
            }
            let src_ty = if flip_v { h - 1 - ty } else { ty };
            let tile_row_idx = src_ty / 8;
            let pixel_row_in_tile = (src_ty % 8) as u8;

            for tx in 0..w {
                let screen_x = x + tx as i32;
                if screen_x < 0 || screen_x >= SCREEN_WIDTH as i32 {
                    continue;
                }
                if skip[screen_x as usize] {
                    continue; // window-masked
                }
                let src_tx = if flip_h { w - 1 - tx } else { tx };
                let tile_col_idx = src_tx / 8;
                let pixel_col_in_tile = (src_tx % 8) as u8;

                // Sprite tiles are laid out in a 16-tiles-wide grid in VRAM.
                // The column component must wrap WITHIN the tile number's
                // low nibble (real hardware behavior): a multi-tile sprite
                // whose base tile's low nibble plus the column offset
                // exceeds 15 wraps back into the same row rather than
                // carrying into the next row/table -- e.g. base tile 0x0A
                // with column offset 7 must land on tile 0x01, not 0x11.
                // The row component (already a multiple of 16) is added
                // separately on top of the untouched high bits and may
                // carry normally.
                let row_component = (tile_row_idx as u16).wrapping_mul(16);
                let col_component = tile_base.wrapping_add(tile_col_idx as u16) & 0x0F;
                let tile_index = (tile_base & 0xFFF0)
                    .wrapping_add(row_component)
                    .wrapping_add(col_component)
                    & 0x1FF;
                // Tiles 256-511 live in the second table, offset from the
                // base by (name_select + 1) * 0x1000 words per OBSEL.
                let (table_base_word, tile_in_table) = if tile_index >= 0x100 {
                    (
                        tile_data_base_word.wrapping_add((name_select + 1) * 0x1000),
                        tile_index & 0xFF,
                    )
                } else {
                    (tile_data_base_word, tile_index)
                };

                let row_pixels = decode_tile_row(vram, table_base_word, tile_in_table, 4, pixel_row_in_tile);
                let pixel_value = row_pixels[pixel_col_in_tile as usize];
                if pixel_value == 0 {
                    continue; // transparent
                }

                let cgram_index = 128 + (palette_num * 16 + pixel_value as u16) as u8;
                let idx = screen_y as usize * SCREEN_WIDTH + screen_x as usize;
                buf[idx] = cgram.read_color(cgram_index) & 0x7FFF;
                // Hardware rule: color math only ever applies to sprites
                // using OBJ palettes 4-7; palettes 0-3 are exempt even when
                // CGADSUB bit 4 is set.
                layer_buf[idx] = if palette_num < 4 { LAYER_OBJ_PAL03 } else { LAYER_OBJ };
            }
        }
    }
}

