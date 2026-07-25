//! Character (tile) data decoding, shared by the background and sprite
//! paths.

use crate::vram::Vram;

/// Decodes one row (8 palette-index pixels, left to right) of a planar
/// tile. `depth` is 2, 4, or 8 bits/pixel; tiles are stored as
/// `depth/2` consecutive 16-byte "bitplane pairs". Within a pair, the two
/// bitplanes are interleaved ROW BY ROW: each of the 8 rows contributes 2
/// adjacent bytes (low plane, then high plane) -- byte layout
/// `[r0p0, r0p1, r1p0, r1p1, ... r7p0, r7p1]`. This matches real SNES
/// VRAM word organization (one word per tile row per pair, low plane in
/// the low byte). An earlier version read `[8 bytes of p0][8 bytes of
/// p1]` (the NES layout, planes NOT interleaved) -- decoding real
/// cartridge graphics into striped garbage: every tile on every screen
/// (BGs and sprites alike) rendered as a half-height double-struck smear,
/// which is exactly why Mario/enemies were unrecognizable in gameplay.
/// Verified against real SMW VRAM contents: the logo's letter tiles only
/// decode into coherent glyph shapes with the interleaved layout.
pub(super) fn decode_tile_row(vram: &Vram, tile_data_base_word: u16, tile_index: u16, depth: u8, row: u8) -> [u8; 8] {
    let bytes_per_tile = (depth as u16) * 8;
    let tile_byte_addr = tile_data_base_word
        .wrapping_mul(2)
        .wrapping_add(tile_index.wrapping_mul(bytes_per_tile));

    let mut out = [0u8; 8];
    let plane_pairs = depth / 2;
    for pair in 0..plane_pairs {
        let pair_base = tile_byte_addr.wrapping_add((pair as u16) * 16);
        let lo = vram.read(pair_base.wrapping_add((row as u16) * 2));
        let hi = vram.read(pair_base.wrapping_add((row as u16) * 2 + 1));
        for x in 0..8u8 {
            let bit = 7 - x;
            let b0 = (lo >> bit) & 1;
            let b1 = (hi >> bit) & 1;
            out[x as usize] |= (b0 | (b1 << 1)) << (pair * 2);
        }
    }
    out
}

