//! Mode 7's affine transform: the M7A-M7D matrix sampling with M7SEL
//! screen-over/flip behavior, and the layer draw for BG1 and EXTBG BG2.

use super::color::direct_color;
use super::{Band, Frame, Target, LAYER_BG1, SCREEN_WIDTH};
use crate::ppu::PpuRegisters;
use crate::vram::Vram;

/// Sign-extends a 13-bit register value (M7X/M7Y/M7HOFS/M7VOFS) to i32.
pub(super) fn sign13(v: u16) -> i32 {
    (((v << 3) as i16) >> 3) as i32
}

/// Samples the mode-7 playing field at screen position (`x`, `y`),
/// returning the raw 8-bit pixel value, or `None` when the transformed
/// coordinate falls outside the 1024x1024 field and M7SEL's screen-over
/// mode says "transparent". The field is a 128x128 map of 8x8 8bpp tiles
/// stored interleaved in VRAM: word N's LOW byte is map entry N (a tile
/// number), and word N's HIGH byte is tile-data byte N (tile*64 + row*8 +
/// column). Transform per fullsnes/bsnes: the per-scanline origin uses
/// the matrix against (scroll - center) -- each product truncated to
/// ~-64/+63 sub-pixel steps via `& !63` -- plus the center, then steps by
/// M7A/M7C per screen pixel; all math in 8.8 signed fixed point.
pub(super) fn mode7_sample(vram: &Vram, regs: &PpuRegisters, x: usize, y: usize) -> Option<u8> {
    let a = regs.m7a as i16 as i32;
    let b = regs.m7b as i16 as i32;
    let c = regs.m7c as i16 as i32;
    let d = regs.m7d as i16 as i32;
    let cx = sign13(regs.m7x);
    let cy = sign13(regs.m7y);
    let hofs = sign13(regs.m7_hofs);
    let vofs = sign13(regs.m7_vofs);

    // The scroll-minus-center offsets are clipped to signed 11 bits
    // (documented hardware quirk of the mode-7 pipeline).
    fn clip(v: i32) -> i32 {
        if v & 0x2000 != 0 { v | !0x3FF } else { v & 0x3FF }
    }

    // M7SEL bits 0/1 flip the whole 256x224 screen before the transform.
    let sx = (if regs.m7sel & 0x01 != 0 { 255 - x } else { x }) as i32;
    let sy = (if regs.m7sel & 0x02 != 0 { 255 - y } else { y }) as i32;

    let ox = ((a * clip(hofs - cx)) & !63)
        + ((b * clip(vofs - cy)) & !63)
        + ((b * sy) & !63)
        + (cx << 8);
    let oy = ((c * clip(hofs - cx)) & !63)
        + ((d * clip(vofs - cy)) & !63)
        + ((d * sy) & !63)
        + (cy << 8);

    let px = (ox + a * sx) >> 8;
    let py = (oy + c * sx) >> 8;

    let out_of_field = ((px | py) as u32) & !0x3FF != 0;
    let screen_over = (regs.m7sel >> 6) & 0x03;
    if out_of_field && screen_over == 2 {
        return None; // outside the field renders transparent
    }

    let (fx, fy) = ((px & 0x3FF) as u16, (py & 0x3FF) as u16);
    let tile = if out_of_field && screen_over == 3 {
        0 // outside the field repeats tile 0
    } else {
        // Map entry: low byte of word (tile_y * 128 + tile_x).
        vram.read(((fy / 8) * 128 + (fx / 8)) * 2)
    };
    // Pixel: high byte of word (tile * 64 + row * 8 + column).
    let pixel_word = (tile as u16) * 64 + (fy % 8) * 8 + (fx % 8);
    Some(vram.read(pixel_word * 2 + 1))
}

/// Draws mode 7's BG1 (`extbg == false`: all 8 pixel bits are color, no
/// priority) or its EXTBG BG2 (`extbg == true`: pixel bit 7 is a priority
/// bit, bits 0-6 the color; only pixels matching `want_priority` draw).
pub(super) fn draw_mode7_layer(
    target: &mut Target,
    frame: &Frame,
    extbg: bool,
    want_priority: u8,
    skip: &[bool; SCREEN_WIDTH],
    band: Band,
) {
let Frame { vram, cgram, regs, .. } = *frame;
    let Band { y0, y1 } = band;
    let buf = &mut *target.color;
    let layer_buf = &mut *target.layer;
    let use_direct_color = regs.cgwsel & 0x01 != 0;
    // Mode 7 honors BG1's mosaic bit (and BG2's for the EXTBG layer) the
    // same way the tile-based path does: snap the sampled screen
    // coordinate to the block origin.
    let mosaic_bit = if extbg { 0x02 } else { 0x01 };
    let mosaic_size = if regs.mosaic & mosaic_bit != 0 {
        ((regs.mosaic >> 4) & 0x0F) as usize + 1
    } else {
        1
    };
    for py in y0..y1 {
        for (px, &masked) in skip.iter().enumerate() {
            if masked {
                continue; // window-masked
            }
            let Some(raw) = mode7_sample(vram, regs, px - px % mosaic_size, py - py % mosaic_size)
            else {
                continue;
            };
            let (color_index, layer) = if extbg {
                if (raw >> 7) != want_priority {
                    continue;
                }
                (raw & 0x7F, LAYER_BG1 + 1)
            } else {
                (raw, LAYER_BG1)
            };
            if color_index == 0 {
                continue; // transparent
            }
            let color = if use_direct_color && !extbg {
                direct_color(color_index, 0)
            } else {
                cgram.read_color(color_index) & 0x7FFF
            };
            let idx = py * SCREEN_WIDTH + px;
            buf[idx] = color;
            layer_buf[idx] = layer;
        }
    }
}

