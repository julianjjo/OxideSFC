/// Picture Processing Unit (PPU) for the SNES
/// 
/// The PPU handles all graphics rendering on the SNES.
/// It contains VRAM, CGRAM, and OAM memory modules,
/// plus scanline and pixel counters for timing.
/// 
/// NTSC Specifications:
/// - Scanlines: 262 per frame
/// - Horizontal period: 341 dots per scanline
/// - Vertical blanking: lines 225-262
/// - Base resolution: 256×224
/// 
/// PAL Specifications:
/// - Scanlines: 312 per frame
/// - Horizontal period: 341 dots per scanline
/// - Vertical blanking: lines 241-312
/// - Base resolution: 256×240

use crate::cgram::Cgram;
use crate::oam::Oam;
use crate::vram::Vram;

/// PPU mode/region
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PpuMode {
    /// NTSC mode (262 scanlines)
    Ntsc,
    /// PAL mode (312 scanlines)
    Pal,
}

impl Default for PpuMode {
    fn default() -> Self {
        Self::Ntsc
    }
}

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
        }
    }
}

/// Picture Processing Unit
pub struct Ppu {
    /// Video RAM (64KB)
    vram: Vram,
    /// Color Graphics RAM (512 bytes, 256 colors)
    cgram: Cgram,
    /// Object Attribute Memory (544 bytes)
    oam: Oam,
    
    /// Current scanline (0-261 for NTSC, 0-311 for PAL)
    scanline: u16,
    /// Current horizontal pixel position (0-339)
    h_counter: u16,
    /// Frame counter
    frame: u32,
    
    /// PPU mode (NTSC or PAL)
    mode: PpuMode,
    /// Whether the frame was just completed (for is_frame_ready)
    frame_ready: bool,
    /// Interlace field flag, toggled every frame (STAT78 bit 7). In
    /// interlaced modes the two fields carry the odd/even half-lines.
    field: bool,
}

impl Ppu {
    /// Creates a new PPU instance in NTSC mode
    pub fn new() -> Self {
        Self::with_mode(PpuMode::Ntsc)
    }

    /// Creates a new PPU instance with specified mode
    pub fn with_mode(mode: PpuMode) -> Self {
        Self {
            vram: Vram::new(),
            cgram: Cgram::new(),
            oam: Oam::new(),
            scanline: 0,
            h_counter: 0,
            frame: 0,
            mode,
            frame_ready: false,
            field: false,
        }
    }

    /// Creates a new PPU instance in PAL mode
    pub fn new_pal() -> Self {
        Self::with_mode(PpuMode::Pal)
    }

    // ==================== VRAM Access ====================

    /// Gets a mutable reference to VRAM
    pub fn vram(&mut self) -> &mut Vram {
        &mut self.vram
    }

    /// Gets an immutable reference to VRAM
    pub fn vram_ref(&self) -> &Vram {
        &self.vram
    }

    // ==================== CGRAM Access ====================

    /// Gets a mutable reference to CGRAM
    pub fn cgram(&mut self) -> &mut Cgram {
        &mut self.cgram
    }

    /// Gets an immutable reference to CGRAM
    pub fn cgram_ref(&self) -> &Cgram {
        &self.cgram
    }

    // ==================== OAM Access ====================

    /// Gets a mutable reference to OAM
    pub fn oam(&mut self) -> &mut Oam {
        &mut self.oam
    }

    /// Gets an immutable reference to OAM
    pub fn oam_ref(&self) -> &Oam {
        &self.oam
    }

    // ==================== Timing ====================

    /// Advances the PPU by one pixel (tick)
    /// 
    /// This increments the h_counter and handles scanline wrapping.
    /// At the end of each frame, frame_ready is set to true.
    pub fn tick(&mut self) {
        self.h_counter += 1;
        
        // Check for end of scanline
        if self.h_counter >= Self::pixels_per_line() {
            self.h_counter = 0;
            self.scanline += 1;
            
            // Check for end of frame
            if self.scanline >= self.scanlines_per_frame() {
                self.scanline = 0;
                self.frame += 1;
                self.frame_ready = true;
                self.field = !self.field; // interlace fields alternate per frame
            }
        }
    }

    /// Advances the PPU by multiple pixels
    /// 
    /// # Arguments
    /// * `pixels` - Number of pixels to advance
    pub fn tick_n(&mut self, pixels: u32) {
        for _ in 0..pixels {
            self.tick();
        }
    }

    /// Checks if a frame is ready (rendered)
    /// 
    /// This returns true once per frame, after the last scanline completes.
    /// Must be cleared by calling clear_frame_ready() or consuming the frame.
    /// 
    /// # Returns
    /// True if a new frame has been rendered
    pub fn is_frame_ready(&self) -> bool {
        self.frame_ready
    }

    /// Clears the frame ready flag
    /// Call this after processing a frame
    pub fn clear_frame_ready(&mut self) {
        self.frame_ready = false;
    }

    /// Gets the current scanline
    /// 
    /// # Returns
    /// Current scanline number (0-261 for NTSC, 0-311 for PAL)
    pub fn scanline(&self) -> u16 {
        self.scanline
    }

    /// Gets the current horizontal pixel position
    /// 
    /// # Returns
    /// Current pixel position within scanline (0-339)
    pub fn h_counter(&self) -> u16 {
        self.h_counter
    }

    /// Gets the current frame number
    /// 
    /// # Returns
    /// Frame counter (increments each frame)
    pub fn frame(&self) -> u32 {
        self.frame
    }

    /// Gets the PPU mode
    /// 
    /// # Returns
    /// NTSC or PAL mode
    pub fn mode(&self) -> PpuMode {
        self.mode
    }

    /// Gets the number of scanlines per frame
    /// 
    /// # Returns
    /// 262 for NTSC, 312 for PAL
    pub fn scanlines_per_frame(&self) -> u16 {
        match self.mode {
            PpuMode::Ntsc => 262,
            PpuMode::Pal => 312,
        }
    }

    /// Gets the number of dots per scanline
    ///
    /// # Returns
    /// 341 dots per scanline (including hblank). This is the real
    /// hardware count (dot positions 0-340, 1364 master cycles at 4
    /// master cycles per dot): 341 x 262 x 4 = 357,368 master cycles per
    /// frame = the canonical 60.0988 NTSC frame rate. The previous value
    /// of 340 made every emulated frame 0.29% shorter than real time, so
    /// everything paced off frames-per-wall-second (notably the 32kHz
    /// audio stream) fell 0.29% behind and periodically underran.
    pub const fn pixels_per_line() -> u16 {
        341
    }

    /// Gets the visible scanlines (not in vblank)
    /// 
    /// # Returns
    /// Number of visible scanlines (224 for NTSC, 240 for PAL)
    pub fn visible_scanlines(&self) -> u16 {
        match self.mode {
            PpuMode::Ntsc => 224,
            PpuMode::Pal => 240,
        }
    }

    /// Checks if currently in vertical blanking period
    ///
    /// # Returns
    /// True if in vblank
    pub fn in_vblank(&self) -> bool {
        self.scanline >= self.visible_scanlines()
    }

    /// The interlace field flag (STAT78 bit 7), toggled every frame.
    pub fn field(&self) -> bool {
        self.field
    }

    /// Checks if currently in horizontal blanking period
    /// 
    /// # Returns
    /// True if in hblank (pixel 256-339)
    pub fn in_hblank(&self) -> bool {
        self.h_counter >= 256
    }

    // ==================== Save states ====================

    /// Serializes VRAM/CGRAM/OAM contents plus the timing counters.
    pub fn save_state(&self, out: &mut Vec<u8>) {
        use crate::state::{put_bool, put_bytes, put_u16, put_u32, put_u8};
        put_bytes(out, self.vram.as_slice());
        put_bytes(out, self.cgram.as_slice());
        put_bytes(out, self.oam.as_slice());
        put_u16(out, self.scanline);
        put_u16(out, self.h_counter);
        put_u32(out, self.frame);
        put_u8(out, match self.mode {
            PpuMode::Ntsc => 0,
            PpuMode::Pal => 1,
        });
        put_bool(out, self.frame_ready);
        put_bool(out, self.field);
    }

    /// Restores state produced by `save_state`.
    pub(crate) fn load_state(&mut self, r: &mut crate::state::StateReader) -> Result<(), crate::error::EmulationError> {
        let vram_len = self.vram.as_slice().len();
        let cgram_len = self.cgram.as_slice().len();
        let oam_len = self.oam.as_slice().len();
        let vram = r.bytes(vram_len)?.to_vec();
        self.vram.as_mut_slice().copy_from_slice(&vram);
        let cgram = r.bytes(cgram_len)?.to_vec();
        self.cgram.as_mut_slice().copy_from_slice(&cgram);
        let oam = r.bytes(oam_len)?.to_vec();
        self.oam.as_mut_slice().copy_from_slice(&oam);
        self.scanline = r.u16()?;
        self.h_counter = r.u16()?;
        self.frame = r.u32()?;
        self.mode = if r.u8()? == 1 { PpuMode::Pal } else { PpuMode::Ntsc };
        self.frame_ready = r.bool()?;
        self.field = r.bool()?;
        Ok(())
    }

    // ==================== Control ====================

    /// Resets the PPU to initial state
    pub fn reset(&mut self) {
        self.scanline = 0;
        self.h_counter = 0;
        self.frame = 0;
        self.frame_ready = false;
        self.field = false;
        self.vram.clear();
        self.cgram.clear();
        self.oam.clear();
    }

    /// Sets the PPU mode
    /// 
    /// # Arguments
    /// * `mode` - NTSC or PAL mode
    pub fn set_mode(&mut self, mode: PpuMode) {
        self.mode = mode;
    }

    /// Converts to NTSC mode
    pub fn set_ntsc(&mut self) {
        self.set_mode(PpuMode::Ntsc);
    }

    /// Converts to PAL mode
    pub fn set_pal(&mut self) {
        self.set_mode(PpuMode::Pal);
    }
}

impl Default for Ppu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ppu_default_is_ntsc() {
        let ppu = Ppu::new();
        assert_eq!(ppu.mode(), PpuMode::Ntsc);
        assert_eq!(ppu.scanlines_per_frame(), 262);
    }

    #[test]
    fn ppu_pal_mode() {
        let ppu = Ppu::new_pal();
        assert_eq!(ppu.mode(), PpuMode::Pal);
        assert_eq!(ppu.scanlines_per_frame(), 312);
    }

    #[test]
    fn ppu_initial_state() {
        let ppu = Ppu::new();
        assert_eq!(ppu.scanline(), 0);
        assert_eq!(ppu.h_counter(), 0);
        assert_eq!(ppu.frame(), 0);
        assert!(!ppu.is_frame_ready());
    }

    #[test]
    fn ppu_tick() {
        let mut ppu = Ppu::new();
        
        // Tick a few times
        ppu.tick();
        assert_eq!(ppu.h_counter(), 1);
        
        ppu.tick();
        assert_eq!(ppu.h_counter(), 2);
    }

    #[test]
    fn ppu_scanline_wrap() {
        let mut ppu = Ppu::new();
        
        // Advance past end of scanline
        for _ in 0..Ppu::pixels_per_line() {
            ppu.tick();
        }
        
        assert_eq!(ppu.h_counter(), 0);
        assert_eq!(ppu.scanline(), 1);
    }

    #[test]
    fn ppu_frame_complete() {
        let mut ppu = Ppu::new();
        
        // Advance to end of frame (262 scanlines * 341 dots)
        let pixels_per_frame = 262u32 * 341;
        
        for _ in 0..pixels_per_frame {
            ppu.tick();
        }
        
        assert_eq!(ppu.scanline(), 0);
        assert_eq!(ppu.h_counter(), 0);
        assert_eq!(ppu.frame(), 1);
        assert!(ppu.is_frame_ready());
    }

    #[test]
    fn ppu_clear_frame_ready() {
        let mut ppu = Ppu::new();
        
        // Advance to end of frame
        for _ in 0..(262 * 341) {
            ppu.tick();
        }
        
        assert!(ppu.is_frame_ready());
        
        // Clear and verify
        ppu.clear_frame_ready();
        assert!(!ppu.is_frame_ready());
    }

    #[test]
    fn ppu_vblank() {
        let mut ppu = Ppu::new();
        
        // Scanline 224 starts vblank for NTSC
        for _ in 0..(224 * 341) {
            ppu.tick();
        }
        
        assert!(ppu.in_vblank());
        assert_eq!(ppu.scanline(), 224);
    }

    #[test]
    fn ppu_hblank() {
        let mut ppu = Ppu::new();
        
        // Pixel 256 starts hblank
        for _ in 0..256 {
            ppu.tick();
        }
        
        assert!(ppu.in_hblank());
        assert_eq!(ppu.h_counter(), 256);
    }

    #[test]
    fn ppu_reset() {
        let mut ppu = Ppu::new();
        
        // Advance to some state
        for _ in 0..1000 {
            ppu.tick();
        }
        
        // Write something to memory
        ppu.vram().write(0x1000, 0xAB);
        
        // Reset
        ppu.reset();
        
        assert_eq!(ppu.scanline(), 0);
        assert_eq!(ppu.h_counter(), 0);
        assert_eq!(ppu.frame(), 0);
        assert!(!ppu.is_frame_ready());
    }

    #[test]
    fn ppu_mode_switch() {
        let mut ppu = Ppu::new();
        
        // Switch to PAL
        ppu.set_pal();
        assert_eq!(ppu.mode(), PpuMode::Pal);
        assert_eq!(ppu.scanlines_per_frame(), 312);
        
        // Switch back to NTSC
        ppu.set_ntsc();
        assert_eq!(ppu.mode(), PpuMode::Ntsc);
        assert_eq!(ppu.scanlines_per_frame(), 262);
    }

    #[test]
    fn ppu_vram_access() {
        let mut ppu = Ppu::new();
        
        ppu.vram().write(0x1234, 0xAB);
        assert_eq!(ppu.vram_ref().read(0x1234), 0xAB);
    }

    #[test]
    fn ppu_cgram_access() {
        let mut ppu = Ppu::new();
        
        ppu.cgram().write(0x00, 0xAB);
        assert_eq!(ppu.cgram_ref().read(0x00), 0xAB);
    }

    #[test]
    fn ppu_oam_access() {
        let mut ppu = Ppu::new();
        
        ppu.oam().write(0x00, 0xAB);
        assert_eq!(ppu.oam_ref().read(0x00), 0xAB);
    }

    #[test]
    fn ppu_tick_n() {
        let mut ppu = Ppu::new();
        
        ppu.tick_n(100);
        assert_eq!(ppu.h_counter(), 100);
    }

    #[test]
    fn ppu_constants() {
        assert_eq!(Ppu::pixels_per_line(), 341);
    }
}
