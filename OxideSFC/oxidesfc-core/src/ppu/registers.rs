//! The PPU's writable register state, as a plain snapshot-able struct.
//!
//! This exists as data separate from `Ppu` because the renderer needs a COPY
//! of it per scanline: games rewrite scroll, windows, color math and the
//! mode-7 matrix mid-frame (usually via HDMA), so a frame rendered from the
//! single live copy would apply the last line's values to all 224.


/// PPU register state controlling background/sprite rendering ($2100,
/// $2101, $2105, $2107-$2114, $212C). None of this existed anywhere in the
/// core before -- `SystemBus` owns an instance of this and updates it from
/// bus writes, the same way it already owns VMADD/CGADD/OAMADD directly
/// rather than inside `Ppu` itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PpuRegisters {
    /// $2100 INIDISP: bit 7 = forced blank, bits 0-3 = brightness (0-15).
    pub inidisp: u8,
    /// $2101 OBSEL: bits 0-2 = OBJ tile data base address (in 0x2000-word
    /// units), bits 3-4 = name select (gap to the second 256-tile table),
    /// bits 5-7 = sprite size pair. See `renderer.rs::draw_sprites` for
    /// the confirmed-correct decode (an earlier version of this comment
    /// had the base-address and size-pair bit ranges swapped).
    pub obsel: u8,
    /// $2105 BGMODE: bits 0-2 = mode (0-7).
    pub bgmode: u8,
    /// $2107-$210A BG1SC-BG4SC: bits 0-1 = screen size, bits 2-7 = tilemap
    /// base address (in 0x400-word units).
    pub bg_sc: [u8; 4],
    /// $210B BG12NBA: tile data base for BG1 (low nibble) / BG2 (high
    /// nibble), in 0x1000-word units.
    pub bg12nba: u8,
    /// $210C BG34NBA: same, for BG3 (low nibble) / BG4 (high nibble).
    pub bg34nba: u8,
    /// Current BG1-4 horizontal scroll values (latched via the shared
    /// 8-bit scroll latch -- see `SystemBus`'s scroll-register writes).
    pub bg_hofs: [u16; 4],
    /// Current BG1-4 vertical scroll values.
    pub bg_vofs: [u16; 4],
    /// Shared 8-bit latch used by every BGxHOFS/VOFS write.
    pub bg_scroll_latch: u8,
    /// $212C TM: main screen designation -- bits 0-3 enable BG1-4, bit 4
    /// enables sprites.
    pub tm: u8,
    /// $212D TS: subscreen designation -- same bit layout as TM, but for
    /// the subscreen that color math blends into the main screen.
    pub ts: u8,
    /// $2130 CGWSEL: color-math control. Only bit 1 (add-subscreen: 0 =
    /// blend with the fixed color, 1 = blend with the subscreen) is used
    /// here; the window-region and direct-color bits are not modeled.
    pub cgwsel: u8,
    /// $2131 CGADSUB: color-math enable/mode. Bit 7 = subtract (0 = add),
    /// bit 6 = halve the result, bits 0-4 = enable color math on BG1-4/OBJ
    /// respectively, bit 5 = enable on the backdrop.
    pub cgadsub: u8,
    /// $2132 COLDATA fixed color, as a BGR555 value assembled from the
    /// R/G/B intensity writes. Used as the subscreen operand when CGWSEL
    /// bit 1 is clear.
    pub coldata: u16,
    /// $2123 W12SEL / $2124 W34SEL: per-BG window-1/window-2 enable and
    /// invert bits (BG1/BG2 in w12sel, BG3/BG4 in w34sel). 2 bits per
    /// window per BG: bit0=W1 invert, bit1=W1 enable, bit2=W2 invert,
    /// bit3=W2 enable (then the high nibble for the second BG).
    pub w12sel: u8,
    pub w34sel: u8,
    /// $2125 WOBJSEL: same layout for OBJ (low nibble) and the color
    /// window (high nibble).
    pub wobjsel: u8,
    /// $2126-$2129 WH0-WH3: window 1 left/right and window 2 left/right
    /// screen X coordinates.
    pub wh0: u8,
    pub wh1: u8,
    pub wh2: u8,
    pub wh3: u8,
    /// $212A WBGLOG / $212B WOBJLOG: how windows 1 and 2 combine (OR/AND/
    /// XOR/XNOR), 2 bits per layer.
    pub wbglog: u8,
    pub wobjlog: u8,
    /// $212E TMW / $212F TSW: main/subscreen window-mask enable -- which
    /// layers have their window mask applied (same bit layout as TM/TS).
    pub tmw: u8,
    pub tsw: u8,
    /// $2106 MOSAIC: bits 0-3 enable mosaic on BG1-4, bits 4-7 = mosaic
    /// pixel size minus 1 (0 = 1x1 = no visible effect).
    pub mosaic: u8,
    /// $2133 SETINI: screen-mode select. Bit 6 (EXTBG) is what the
    /// renderer consumes -- it enables mode 7's second (priority-split)
    /// BG2 layer; the overscan/hi-res/interlace bits are stored for
    /// readback fidelity but not rendered.
    pub setini: u8,
    /// $211A M7SEL: bits 6-7 = screen-over behavior for out-of-map
    /// coordinates (0/1 = wrap, 2 = transparent, 3 = fill with tile 0),
    /// bit 1 = vertical screen flip, bit 0 = horizontal screen flip.
    pub m7sel: u8,
    /// $211B-$211E M7A-M7D: the 2x2 affine matrix, 8.8 signed fixed point,
    /// each written low-then-high through the shared `m7_latch`.
    pub m7a: u16,
    pub m7b: u16,
    pub m7c: u16,
    pub m7d: u16,
    /// $211F M7X / $2120 M7Y: 13-bit signed rotation-center coordinates.
    pub m7x: u16,
    pub m7y: u16,
    /// Mode 7's own 13-bit signed scroll values. $210D/$210E write these
    /// THROUGH THE M7 LATCH in addition to the normal BG1 scroll values
    /// (real dual-latch hardware behavior).
    pub m7_hofs: u16,
    pub m7_vofs: u16,
    /// Shared "mode 7 prev byte" latch used by every $211B-$2120 (and
    /// $210D/$210E's mode-7 side) write pair.
    pub m7_latch: u8,
    /// Where sprite priority evaluation starts: sprite 0 normally, or
    /// (OAMADD & $FE) >> 1 when $2103 bit 7 (priority rotation) is set.
    /// Maintained by `SystemBus`'s OAMADD writes and the vblank OAM-address
    /// reload; the renderer's per-line sprite evaluation and overlap
    /// ordering both start here.
    pub first_sprite: u8,
}

impl PpuRegisters {
    /// Serializes every rendering register for save states.
    pub fn save_state(&self, out: &mut Vec<u8>) {
        use crate::state::{put_u16, put_u8};
        put_u8(out, self.inidisp);
        put_u8(out, self.obsel);
        put_u8(out, self.bgmode);
        for &v in &self.bg_sc {
            put_u8(out, v);
        }
        put_u8(out, self.bg12nba);
        put_u8(out, self.bg34nba);
        for &v in &self.bg_hofs {
            put_u16(out, v);
        }
        for &v in &self.bg_vofs {
            put_u16(out, v);
        }
        put_u8(out, self.bg_scroll_latch);
        put_u8(out, self.tm);
        put_u8(out, self.ts);
        put_u8(out, self.cgwsel);
        put_u8(out, self.cgadsub);
        put_u16(out, self.coldata);
        put_u8(out, self.w12sel);
        put_u8(out, self.w34sel);
        put_u8(out, self.wobjsel);
        put_u8(out, self.wh0);
        put_u8(out, self.wh1);
        put_u8(out, self.wh2);
        put_u8(out, self.wh3);
        put_u8(out, self.wbglog);
        put_u8(out, self.wobjlog);
        put_u8(out, self.tmw);
        put_u8(out, self.tsw);
        put_u8(out, self.mosaic);
        put_u8(out, self.setini);
        put_u8(out, self.m7sel);
        put_u16(out, self.m7a);
        put_u16(out, self.m7b);
        put_u16(out, self.m7c);
        put_u16(out, self.m7d);
        put_u16(out, self.m7x);
        put_u16(out, self.m7y);
        put_u16(out, self.m7_hofs);
        put_u16(out, self.m7_vofs);
        put_u8(out, self.m7_latch);
        put_u8(out, self.first_sprite);
    }

    /// Restores state produced by `save_state`.
    pub(crate) fn load_state(&mut self, r: &mut crate::state::StateReader) -> Result<(), crate::error::EmulationError> {
        self.inidisp = r.u8()?;
        self.obsel = r.u8()?;
        self.bgmode = r.u8()?;
        for v in self.bg_sc.iter_mut() {
            *v = r.u8()?;
        }
        self.bg12nba = r.u8()?;
        self.bg34nba = r.u8()?;
        for v in self.bg_hofs.iter_mut() {
            *v = r.u16()?;
        }
        for v in self.bg_vofs.iter_mut() {
            *v = r.u16()?;
        }
        self.bg_scroll_latch = r.u8()?;
        self.tm = r.u8()?;
        self.ts = r.u8()?;
        self.cgwsel = r.u8()?;
        self.cgadsub = r.u8()?;
        self.coldata = r.u16()?;
        self.w12sel = r.u8()?;
        self.w34sel = r.u8()?;
        self.wobjsel = r.u8()?;
        self.wh0 = r.u8()?;
        self.wh1 = r.u8()?;
        self.wh2 = r.u8()?;
        self.wh3 = r.u8()?;
        self.wbglog = r.u8()?;
        self.wobjlog = r.u8()?;
        self.tmw = r.u8()?;
        self.tsw = r.u8()?;
        self.mosaic = r.u8()?;
        self.setini = r.u8()?;
        self.m7sel = r.u8()?;
        self.m7a = r.u16()?;
        self.m7b = r.u16()?;
        self.m7c = r.u16()?;
        self.m7d = r.u16()?;
        self.m7x = r.u16()?;
        self.m7y = r.u16()?;
        self.m7_hofs = r.u16()?;
        self.m7_vofs = r.u16()?;
        self.m7_latch = r.u8()?;
        self.first_sprite = r.u8()?;
        Ok(())
    }
}

impl Default for PpuRegisters {
    fn default() -> Self {
        Self {
            inidisp: 0x80, // power-on: forced blank
            obsel: 0,
            bgmode: 0,
            bg_sc: [0; 4],
            bg12nba: 0,
            bg34nba: 0,
            bg_hofs: [0; 4],
            bg_vofs: [0; 4],
            bg_scroll_latch: 0,
            tm: 0,
            ts: 0,
            cgwsel: 0,
            cgadsub: 0,
            coldata: 0,
            w12sel: 0,
            w34sel: 0,
            wobjsel: 0,
            wh0: 0,
            wh1: 0,
            wh2: 0,
            wh3: 0,
            wbglog: 0,
            wobjlog: 0,
            tmw: 0,
            tsw: 0,
            mosaic: 0,
            setini: 0,
            m7sel: 0,
            m7a: 0,
            m7b: 0,
            m7c: 0,
            m7d: 0,
            m7x: 0,
            m7y: 0,
            m7_hofs: 0,
            m7_vofs: 0,
            m7_latch: 0,
            first_sprite: 0,
        }
    }
}

