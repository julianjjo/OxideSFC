//! Picture Processing Unit (PPU) for the SNES.
//!
//! Owns VRAM, CGRAM and OAM plus the scanline/dot counters that pace the
//! whole machine, and tracks the video standard (NTSC/PAL) those counters
//! follow. `registers` holds the writable register state as separate data,
//! because the renderer needs a per-scanline copy of it.
//!
//! NTSC: 262 scanlines, 341 dots per line, vblank from line 224, 256x224.
//! PAL:  312 scanlines, 341 dots per line, vblank from line 240, 256x240.

mod registers;

#[cfg(test)]
mod tests;

pub use registers::PpuRegisters;

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
    /// Whether the finished picture is ready to be displayed -- latched on
    /// the vblank-entry edge (see `tick`).
    frame_ready: bool,
    /// Whether `frame_ready` has already been latched for the frame in
    /// progress, so the vblank-entry edge fires exactly once even if
    /// `visible_scanlines` changes mid-frame (an overscan toggle).
    frame_ready_latched: bool,
    /// Interlace field flag, toggled every frame (STAT78 bit 7). In
    /// interlaced modes the two fields carry the odd/even half-lines.
    field: bool,
    /// SETINI ($2133) bit 2: overscan mode -- the picture spans 239
    /// lines instead of 224, so vblank (and the NMI) starts at line 239.
    /// Maintained by `SystemBus`'s $2133 write handler.
    overscan: bool,
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
            frame_ready_latched: false,
            field: false,
            overscan: false,
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
    /// `frame_ready` is latched on the vblank-entry edge.
    ///
    /// It used to be latched at the end of the *frame* instead, i.e. after
    /// the whole vblank had run. Because the frontend renders as soon as
    /// this flag appears (`Snes::step_until_frame_ready`), and because
    /// `SystemBus::render_frame` reads VRAM and OAM live rather than from
    /// per-scanline snapshots, that handed the renderer the tile and sprite
    /// data the game had just uploaded *during that vblank for the NEXT
    /// frame*, while the per-scanline register snapshots still described the
    /// frame just finished. Every scrolling game therefore drew its sprites
    /// one frame of camera motion away from its backgrounds, and the tilemap
    /// column freshly DMA'd for the upcoming scroll appeared against the old
    /// scroll position as a wrong column of tiles at the screen edge. It
    /// also meant the tick that set the flag had already overwritten
    /// `scanline_regs[0]` with post-vblank state, so the top line of every
    /// frame rendered with the next frame's registers.
    ///
    /// Latching at vblank entry instead means the picture is rendered from
    /// the VRAM/OAM/register state that was actually live while it was being
    /// scanned out -- and it is the same edge on which hardware raises NMI,
    /// so a game's vblank uploads land after the frame they follow.
    pub fn tick(&mut self) {
        self.h_counter += 1;

        // Check for end of scanline
        if self.h_counter >= Self::pixels_per_line() {
            self.h_counter = 0;
            self.scanline += 1;

            // Vblank entry: the visible picture is complete.
            if !self.frame_ready_latched && self.scanline >= self.visible_scanlines() {
                self.frame_ready_latched = true;
                self.frame_ready = true;
            }

            // Check for end of frame
            if self.scanline >= self.scanlines_per_frame() {
                self.scanline = 0;
                self.frame += 1;
                self.frame_ready_latched = false;
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
    /// Number of visible scanlines: 239 with SETINI's overscan bit set
    /// (vblank -- and the NMI -- start at line 239 in overscan mode),
    /// otherwise 224 for NTSC / 240 for PAL.
    pub fn visible_scanlines(&self) -> u16 {
        if self.overscan {
            return 239;
        }
        match self.mode {
            PpuMode::Ntsc => 224,
            PpuMode::Pal => 240,
        }
    }

    /// Sets SETINI ($2133) bit 2's overscan mode -- see
    /// `visible_scanlines`.
    pub fn set_overscan(&mut self, overscan: bool) {
        self.overscan = overscan;
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
    /// True if in hblank. The real HBlank flag window is dot 274 through
    /// dot 0 of the next line (snes9x `SNES_HBLANK_START_HC` = 1096
    /// master cycles = dot 274, `SNES_HBLANK_END_HC` = 4 = dot 1), NOT
    /// dot 256 -- the PPU keeps fetching sprite/BG data for the next line
    /// until dot ~274, and HDMA fires at that point too.
    pub fn in_hblank(&self) -> bool {
        self.h_counter >= 274 || self.h_counter < 1
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
        put_bool(out, self.frame_ready_latched);
        put_bool(out, self.field);
        put_bool(out, self.overscan);
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
        self.frame_ready_latched = r.bool()?;
        self.field = r.bool()?;
        self.overscan = r.bool()?;
        Ok(())
    }

    // ==================== Control ====================

    /// Resets the PPU to initial state
    pub fn reset(&mut self) {
        self.scanline = 0;
        self.h_counter = 0;
        self.frame = 0;
        self.frame_ready = false;
        self.frame_ready_latched = false;
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

