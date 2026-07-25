//! Turns VRAM/CGRAM/OAM contents plus PPU registers into an actual RGBA8888
//! framebuffer. None of this existed before -- DMA could upload real
//! cartridge graphics data into PPU memory, but nothing ever read it back
//! out into pixels, so the emulator could run indefinitely with completely
//! correct internal state and still never produce a single visible pixel.
//!
//! Feature coverage:
//!   - Background modes 0-6 via a shared tile path (8x8 and, per BGMODE
//!     bits 4-7, 16x16 tiles), and mode 7's affine single-layer path
//!     (128x128 map of 8x8 8bpp tiles, the M7A-M7D matrix, M7SEL flips
//!     and screen-over behavior, and the SETINI EXTBG priority-split
//!     second layer).
//!   - Hi-res modes 5/6: BG pixels are sampled in their real 512-dot
//!     horizontal space (16-wide tiles) and collapsed into the fixed
//!     256x224 output raster by averaging each adjacent dot pair; with
//!     SETINI's interlace bit the two field lines are averaged the same
//!     way (the output raster itself stays 256x224 -- the collapse is
//!     the documented equivalent of hardware's dot/field interleave on
//!     a fixed-size framebuffer).
//!   - Windowing, mosaic, per-tile BG priority, per-mode sprite/BG
//!     priority interleaving, color math with subscreen/fixed-color
//!     operands, direct-color mode, both OBJ tile tables, and
//!     per-scanline register bands.
//!
//! Layout: `compose` owns the public entry points and the main/sub screen
//! compositing pass, and calls into one module per layer kind
//! (`background`, `sprites`, `mode7`) plus the shared `window`, `color` and
//! `tile` helpers. This module keeps only what all of them share: the
//! output dimensions, the pixel source-layer ids, and the per-mode BG depth
//! table.

mod background;
mod color;
mod compose;
mod mode7;
mod sprites;
mod tile;
mod window;

#[cfg(test)]
mod tests;

// `render_frame_per_scanline_with_status` is deliberately not re-exported:
// only this module's own tests use it, and they reach it through `compose`.
pub use compose::{
    render_frame, render_frame_per_scanline, render_frame_per_scanline_with_cgram,
};

pub const SCREEN_WIDTH: usize = 256;
pub const SCREEN_HEIGHT: usize = 224;

/// Per-pixel source-layer id, used to decide (via CGADSUB) whether color
/// math applies to a given main-screen pixel. Values line up with
/// CGADSUB's enable bits: BG1-4 = 0-3, sprites (OBJ) = 4, backdrop = 5.
const LAYER_BG1: u8 = 0;
const LAYER_OBJ: u8 = 4;
const LAYER_BACKDROP: u8 = 5;
/// Sprite pixels using OBJ palettes 0-3. Real hardware NEVER applies color
/// math to these -- CGADSUB bit 4 only enables math for sprites on palettes
/// 4-7 (fullsnes "Color Math"). This id is 6, and the math gate masks
/// CGADSUB to its 6 enable bits, so bit 6 (the half-color flag) can never
/// accidentally enable math for it.
const LAYER_OBJ_PAL03: u8 = 6;

/// Bits per pixel of each of the 4 BG layers for a given BGMODE (0-6);
/// `None` means the layer doesn't exist in this mode. Verified against
/// wiki.superfamicom.org/Backgrounds.
fn bg_depths(mode: u8) -> [Option<u8>; 4] {
    match mode {
        0 => [Some(2), Some(2), Some(2), Some(2)],
        1 => [Some(4), Some(4), Some(2), None],
        2 => [Some(4), Some(4), None, None],
        3 => [Some(8), Some(4), None, None],
        4 => [Some(8), Some(2), None, None],
        5 => [Some(4), Some(2), None, None],
        6 => [Some(4), None, None, None],
        _ => [None, None, None, None],
    }
}

