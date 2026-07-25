//! Color operations: SNES color math, direct-color mode, BGR555 averaging
//! for the hi-res/interlace collapse, and BGR555 -> RGB888 output.

use crate::cgram::Cgram;

/// SNES direct-color mode (CGWSEL bit 0, 8bpp layers): the pixel byte is
/// its own BGR color -- bits 0-2 = red, 3-5 = green, 6-7 = blue -- with
/// the tilemap palette bits (zero in mode 7) contributing one extra low
/// bit per channel. Returns BGR555.
pub(super) fn direct_color(pixel: u8, palette: u8) -> u16 {
    let r = (((pixel & 0x07) << 2) | ((palette & 0x01) << 1)) as u16;
    let g = ((((pixel >> 3) & 0x07) << 2) | (palette & 0x02)) as u16;
    // The palette's blue bit lands in blue's bit 2, not bit 1: bsnes'
    // `directColor` places it with `paletteIndex << 10 & 0x1000`, which is
    // bit 12 of the BGR555 word = bit 2 of the 5-bit blue channel.
    let b = ((((pixel >> 6) & 0x03) << 3) | (palette & 0x04)) as u16;
    r | (g << 5) | (b << 10)
}

/// SNES color math on two BGR555 colors, per-channel in 5-bit space:
/// `main +/- operand`, optionally halved, clamped to 0..31 per channel.
/// Channel extraction/reassembly delegates to `Cgram`'s helpers rather
/// than reimplementing the same BGR555 mask/shift logic inline.
pub(super) fn color_math(main: u16, operand: u16, subtract: bool, half: bool) -> u16 {
    let combine = |m: i32, s: i32| -> u8 {
        let mut v = if subtract { m - s } else { m + s };
        if half {
            v >>= 1;
        }
        v.clamp(0, 31) as u8
    };
    let r = combine(Cgram::extract_red(main) as i32, Cgram::extract_red(operand) as i32);
    let g = combine(Cgram::extract_green(main) as i32, Cgram::extract_green(operand) as i32);
    let b = combine(Cgram::extract_blue(main) as i32, Cgram::extract_blue(operand) as i32);
    Cgram::make_color(r, g, b)
}

/// BGR555 -> RGB888, expanding each 5-bit channel by replicating its top 3
/// bits into the low bits (the standard technique for even 0-255 coverage).
pub(super) fn bgr555_to_rgb8(color: u16) -> (u8, u8, u8) {
    let r5 = (color & 0x1F) as u32;
    let g5 = ((color >> 5) & 0x1F) as u32;
    let b5 = ((color >> 10) & 0x1F) as u32;
    let expand = |c5: u32| ((c5 << 3) | (c5 >> 2)) as u8;
    (expand(r5), expand(g5), expand(b5))
}

/// Averages 1-4 BGR555 colors per channel (used to collapse hi-res dot
/// pairs / interlace line pairs into the 256x224 output raster).
pub(super) fn average_bgr555(colors: &[u16]) -> u16 {
    let n = colors.len() as u32;
    let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
    for &c in colors {
        r += (c & 0x1F) as u32;
        g += ((c >> 5) & 0x1F) as u32;
        b += ((c >> 10) & 0x1F) as u32;
    }
    ((r / n) as u16) | (((g / n) as u16) << 5) | (((b / n) as u16) << 10)
}

