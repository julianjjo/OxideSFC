//! Tile-based background layers (modes 0-6), including offset-per-tile,
//! mosaic, 16x16 tiles and the hi-res 512-dot sampling path.

use super::color::{average_bgr555, direct_color};
use super::tile::decode_tile_row;
use super::{Band, Frame, Target, LAYER_BG1, SCREEN_WIDTH};

pub(super) fn draw_bg_layer(
    target: &mut Target,
    frame: &Frame,
    bg: usize,
    depth: u8,
    want_priority: u8,
    skip: &[bool; SCREEN_WIDTH],
    band: Band,
) {
let Frame { vram, cgram, regs, .. } = *frame;
    let Band { y0, y1 } = band;
    let buf = &mut *target.color;
    let layer_buf = &mut *target.layer;
    let mode = regs.bgmode & 0x07;
    // Modes 5/6 are the hi-res modes: BG pixels exist in a 512-dot
    // horizontal space (tiles forced 16 wide), collapsed into this
    // renderer's 256-wide raster by averaging each adjacent dot pair.
    // With SETINI's interlace bit the vertical resolution doubles too
    // (448 half-lines), collapsed the same way by averaging the two
    // field lines each output row spans.
    let hires = mode == 5 || mode == 6;
    let interlaced = hires && (regs.setini & 0x01) != 0;

    let tilemap_base_word = ((regs.bg_sc[bg] >> 2) as u16) * 0x400;
    let screen_size = regs.bg_sc[bg] & 0x03; // 0=32x32, 1=64x32, 2=32x64, 3=64x64
    let nba = if bg < 2 { regs.bg12nba } else { regs.bg34nba };
    let nibble = if bg.is_multiple_of(2) { nba & 0x0F } else { (nba >> 4) & 0x0F };
    let tile_data_base_word = (nibble as u16) * 0x1000;

    let hofs = regs.bg_hofs[bg];
    let vofs = regs.bg_vofs[bg];

    // BGMODE bits 4-7: 16x16 tiles for BG1-4. In modes 5/6 the tile is
    // always 16 wide (the hi-res fetch pattern); the size bit then only
    // selects 8- vs 16-pixel height.
    let size16 = regs.bgmode & (0x10 << bg) != 0;
    let tile_w: u32 = if hires || size16 { 16 } else { 8 };
    let tile_h: u32 = if size16 { 16 } else { 8 };

    // Mosaic (MOSAIC $2106): when enabled for this BG, every size x size
    // screen-space block repeats its top-left pixel -- implemented by
    // snapping the sampled coordinate down to the block origin while
    // still writing every screen pixel.
    let mosaic_size = if regs.mosaic & (1 << bg) != 0 {
        ((regs.mosaic >> 4) & 0x0F) as usize + 1
    } else {
        1
    };

    // In mode 0 every BG gets its own 32-color block of CGRAM: BG1 uses
    // colors 0-31, BG2 32-63, BG3 64-95, BG4 96-127 (higan's
    // `paletteOffset = bgMode == 0 ? id << 5 : 0`). Without this offset all
    // four 2bpp layers indexed BG1's palettes, so three of the four layers
    // on every mode-0 screen rendered in visibly wrong colors. Mode 1's
    // 2bpp BG3 correctly takes no offset, so this is mode-0-specific.
    let mode0_palette_base: u16 = if mode == 0 { (bg as u16) << 5 } else { 0 };

    let (map_w_tiles, map_h_tiles): (u32, u32) = match screen_size {
        0 => (32, 32),
        1 => (64, 32),
        2 => (32, 64),
        _ => (64, 64),
    };

    // Samples this layer at a WORLD coordinate (scroll already applied),
    // returning the BGR555 color, or None when transparent / not part of
    // the current priority pass. Multi-cell (16-wide/-tall) tiles select
    // their 8x8 sub-cell the same way OBJ tiles do: +1 tile number per
    // horizontal cell, +16 per vertical cell, wrapping in the 10-bit
    // tile-number space; flips mirror across the WHOLE tile.
    let sample = |world_x: u32, world_y: u32| -> Option<u16> {
        let tile_col = (world_x / tile_w) % map_w_tiles;
        let tile_row = (world_y / tile_h) % map_h_tiles;

        // Sizes larger than 32x32 are 2-4 separate contiguous 32x32
        // (0x400-entry) maps in VRAM; resolve which one this tile is in.
        let (quad_col, local_col) = (tile_col / 32, tile_col % 32);
        let (quad_row, local_row) = (tile_row / 32, tile_row % 32);
        let quadrant: u32 = match screen_size {
            0 => 0,
            1 => quad_col,
            2 => quad_row,
            _ => quad_row * 2 + quad_col,
        };

        let map_entry_word = tilemap_base_word
            .wrapping_add((quadrant * 0x400) as u16)
            .wrapping_add((local_row * 32 + local_col) as u16);
        let entry = vram.read_word(map_entry_word.wrapping_mul(2));

        // Tilemap entry bit 13 is the per-tile priority bit. Only draw
        // tiles matching the priority pass currently being composited.
        let tile_priority = ((entry >> 13) & 0x01) as u8;
        if tile_priority != want_priority {
            return None;
        }

        let base_tile = entry & 0x3FF;
        let palette_num = (entry >> 10) & 0x07;
        let flip_h = (entry & 0x4000) != 0;
        let flip_v = (entry & 0x8000) != 0;

        let mut in_x = world_x % tile_w;
        let mut in_y = world_y % tile_h;
        if flip_h {
            in_x = tile_w - 1 - in_x;
        }
        if flip_v {
            in_y = tile_h - 1 - in_y;
        }
        let cell_x = (in_x / 8) as u16;
        let cell_y = (in_y / 8) as u16;
        let tile_index = base_tile.wrapping_add(cell_x).wrapping_add(cell_y * 0x10) & 0x3FF;

        let row_pixels = decode_tile_row(vram, tile_data_base_word, tile_index, depth, (in_y % 8) as u8);
        let pixel_value = row_pixels[(in_x % 8) as usize];
        if pixel_value == 0 {
            return None; // transparent -- leave whatever's already drawn beneath
        }

        let cgram_index = match depth {
            2 => (mode0_palette_base + palette_num * 4 + pixel_value as u16) as u8,
            4 => (palette_num * 16 + pixel_value as u16) as u8,
            _ => pixel_value, // 8bpp: direct index, no palette grouping
        };
        // 8bpp layers honor CGWSEL's direct-color mode (the tilemap
        // palette bits feed the channels' extra low bits).
        Some(if depth == 8 && regs.cgwsel & 0x01 != 0 {
            direct_color(pixel_value, palette_num as u8)
        } else {
            cgram.read_color(cgram_index) & 0x7FFF
        })
    };

    // Offset-per-tile (modes 2/4, and 6 on hardware): BG3's tilemap
    // doubles as a table of per-8-pixel-column scroll overrides for
    // BG1/BG2. The first visible column always uses the normal scroll;
    // for screen column N >= 1 the entry fetched from BG3's tilemap at
    // world position ((N-1)*8 + (BG3HOFS & ~7), BG3VOFS) replaces the
    // horizontal offset (the BG's own fine scroll, HOFS & 7, still
    // applies), and in mode 2 the entry one tile-row below (BG3VOFS + 8)
    // replaces the vertical offset. Mode 4 fetches a single entry whose
    // bit 15 selects H or V. Entry bit 13 gates the override for BG1,
    // bit 14 for BG2 (fullsnes "OPT"; snes9x gfx.cpp
    // DrawBackgroundOffset). Hi-res mode 6's 512-dot variant is not
    // modeled.
    let opt_active = !hires && (mode == 2 || mode == 4) && bg < 2;
    let opt_valid_mask: u16 = 0x2000 << bg;
    let bg3_tilemap_base_word = ((regs.bg_sc[2] >> 2) as u16) * 0x400;
    let bg3_screen_size = regs.bg_sc[2] & 0x03;
    let bg3_hofs = regs.bg_hofs[2];
    let bg3_vofs = regs.bg_vofs[2];
    let bg3_entry = |world_x: u32, world_y: u32| -> u16 {
        let (map_w, map_h): (u32, u32) = match bg3_screen_size {
            0 => (32, 32),
            1 => (64, 32),
            2 => (32, 64),
            _ => (64, 64),
        };
        let tile_col = (world_x / 8) % map_w;
        let tile_row = (world_y / 8) % map_h;
        let (quad_col, local_col) = (tile_col / 32, tile_col % 32);
        let (quad_row, local_row) = (tile_row / 32, tile_row % 32);
        let quadrant: u32 = match bg3_screen_size {
            0 => 0,
            1 => quad_col,
            2 => quad_row,
            _ => quad_row * 2 + quad_col,
        };
        let word = bg3_tilemap_base_word
            .wrapping_add((quadrant * 0x400) as u16)
            .wrapping_add((local_row * 32 + local_col) as u16);
        vram.read_word(word.wrapping_mul(2))
    };

    for py in y0..y1 {
        let eff_py = py - py % mosaic_size;
        for (px, &masked) in skip.iter().enumerate() {
            if masked {
                continue; // window-masked
            }
            let eff_px = px - px % mosaic_size;

            let color = if hires {
                // Sample both dots of the pair (and both field lines when
                // interlaced), averaging whatever is opaque.
                let dots = [(2 * eff_px) as u32, (2 * eff_px + 1) as u32];
                let lines: &[u32] = if interlaced {
                    &[0, 1]
                } else {
                    &[0]
                };
                let mut samples = [0u16; 4];
                let mut count = 0;
                for &line in lines {
                    let world_y = if interlaced {
                        ((2 * eff_py) as u32 + line).wrapping_add(vofs as u32)
                    } else {
                        (eff_py as u32).wrapping_add(vofs as u32)
                    };
                    for &dot in &dots {
                        if let Some(c) = sample(dot.wrapping_add(hofs as u32), world_y) {
                            samples[count] = c;
                            count += 1;
                        }
                    }
                }
                if count == 0 {
                    continue;
                }
                average_bgr555(&samples[..count])
            } else {
                let (mut eff_hofs, mut eff_vofs) = (hofs, vofs);
                if opt_active {
                    let col = ((px as u32) + ((hofs as u32) & 7)) / 8;
                    if col > 0 {
                        let opt_x = ((col - 1) * 8).wrapping_add((bg3_hofs as u32) & !7u32);
                        let hentry = bg3_entry(opt_x, bg3_vofs as u32);
                        if mode == 4 {
                            if hentry & opt_valid_mask != 0 {
                                if hentry & 0x8000 != 0 {
                                    eff_vofs = hentry & 0x3FF;
                                } else {
                                    eff_hofs = (hentry & 0x3F8) | (hofs & 7);
                                }
                            }
                        } else {
                            let ventry = bg3_entry(opt_x, (bg3_vofs as u32).wrapping_add(8));
                            if hentry & opt_valid_mask != 0 {
                                eff_hofs = (hentry & 0x3F8) | (hofs & 7);
                            }
                            if ventry & opt_valid_mask != 0 {
                                eff_vofs = ventry & 0x3FF;
                            }
                        }
                    }
                }
                let world_x = (eff_px as u32).wrapping_add(eff_hofs as u32);
                let world_y = (eff_py as u32).wrapping_add(eff_vofs as u32);
                match sample(world_x, world_y) {
                    Some(c) => c,
                    None => continue,
                }
            };

            let idx = py * SCREEN_WIDTH + px;
            buf[idx] = color;
            layer_buf[idx] = LAYER_BG1 + bg as u8;
        }
    }
}

