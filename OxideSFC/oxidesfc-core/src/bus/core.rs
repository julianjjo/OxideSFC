//! Construction and the component accessors: everything that is about the
//! bus *owning* the machine's parts rather than about a particular register
//! or timing rule.

use super::SystemBus;
use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::dma::Dma;
use crate::error::EmulationError;
use crate::cgram::Cgram;
use crate::ppu::{Ppu, PpuRegisters};
use crate::wram::Wram;

impl SystemBus {
    pub fn new() -> Self {
        Self {
            wram: Wram::new(),
            cartridge: None,
            apu: Apu::new(),
            ppu: Ppu::new(),
            open_bus: 0x00,
            nmi_enable: false,
            nmi_status_flag: false,
            nmi_pending: false,
            was_in_vblank: false,
            was_in_hblank: false,
            dma: Dma::new(),
            hdma_enable_mask: 0,
            vmain: 0,
            vmadd: 0,
            cgadd: 0,
            cgram_high: false,
            oamadd: 0,
            oamadd_latch: 0,
            oam_high: false,
            ppu_regs: PpuRegisters::default(),
            joypad1_state: 0,
            auto_joypad_read_enable: false,
            joy1_auto: 0,
            joypad_strobe: false,
            joy1_shift: 0,
            joy1_bits_read: 0,
            joy1_ever_strobed: false,
            joypad2_state: 0,
            joy2_auto: 0,
            joy2_shift: 0,
            joy2_bits_read: 0,
            irq_h_enable: false,
            irq_v_enable: false,
            htime: 0x1FF,
            vtime: 0x1FF,
            irq_line: false,
            last_scanline: 0,
            scanline_regs: vec![PpuRegisters::default(); crate::renderer::SCREEN_HEIGHT],
            scanline_cgram: vec![Cgram::new(); crate::renderer::SCREEN_HEIGHT],
            wrmpya: 0xFF,
            wrdiv: 0xFFFF,
            rddiv: 0,
            rdmpy: 0,
            wrio: 0xFF,
            memsel: 0,
            wmadd: 0,
            mpy: 1, // power-on: M7A/M7B latches read back as $01 * $01 on real hardware
            ophct: 0,
            opvct: 0,
            ophct_high: false,
            opvct_high: false,
            counter_latched: false,
            ppu1_mdr: 0,
            ppu2_mdr: 0,
            vram_prefetch: 0,
            oam_lsb_latch: 0,
            oam_priority_rotation: false,
            range_time_over: 0,
            refresh_charged_this_line: false,
            last_h_dot: 0,
            hdma_ran_channels: 0,
            step_access_count: 0,
            step_access_master: 0,
            accounting_suspended: 0,
            dot_master_remainder: 0,
            apu_master_remainder: 0,
        }
    }

    /// Sets the live state of controller 1, in the SNES auto-read bit
    /// layout (bit15=B,14=Y,13=Select,12=Start,11=Up,10=Down,9=Left,
    /// 8=Right,7=A,6=X,5=L,4=R). Callers driving real input (keyboard,
    /// gamepad) should call this whenever pressed buttons change; the
    /// value only reaches the CPU-visible registers on the next
    /// auto-read latch (vblank entry) or manual $4016 strobe.
    pub fn set_joypad1_state(&mut self, state: u16) {
        self.joypad1_state = state;
    }

    /// Sets the live state of controller 2 -- same bit layout and latching
    /// rules as `set_joypad1_state` (visible via $4017 serial reads and the
    /// $421A/$421B auto-read registers).
    pub fn set_joypad2_state(&mut self, state: u16) {
        self.joypad2_state = state;
    }

    /// Renders the current VRAM/OAM contents to an RGBA8888 framebuffer
    /// using the PER-SCANLINE register and CGRAM snapshots captured
    /// during the frame (see `scanline_regs`/`scanline_cgram`), so
    /// mid-frame register changes (SMW's IRQ status-bar split, HDMA
    /// scroll/COLDATA effects) and mid-frame palette rewrites (PoP2's
    /// HDMA sky gradient on backdrop color 0) land on the correct rows.
    /// See `crate::renderer` for exactly what is and isn't modeled.
    pub fn render_frame(&mut self) -> Vec<u8> {
        let (frame, range_time_over) = crate::renderer::render_frame_per_scanline_with_cgram(
            self.ppu.vram_ref(),
            &self.scanline_cgram,
            self.ppu.oam_ref(),
            &self.scanline_regs,
        );
        // STAT77 ($213E) bits 6-7: sprite range/time-over, recomputed by
        // each frame's per-line sprite evaluation (real hardware clears
        // them at the start of every frame and accumulates while lines
        // render; recomputing per rendered frame lands in the same place
        // for code that polls them during vblank).
        self.range_time_over = range_time_over;
        frame
    }

    /// Read-only view of the per-scanline register snapshots that
    /// `render_frame` renders from -- lets tests/diagnostics inspect
    /// exactly what state each visible line was rendered with (IRQ raster
    /// splits, HDMA scroll/color effects).
    pub fn scanline_regs_ref(&self) -> &[PpuRegisters] {
        &self.scanline_regs
    }

    /// Read-only access to the current background/sprite rendering
    /// registers (e.g. for diagnostics or tests that need to check
    /// whether the screen has been turned on / which layers are enabled).
    pub fn ppu_registers(&self) -> &PpuRegisters {
        &self.ppu_regs
    }

    /// Advances the APU by `cycles` main-CPU cycles (the same unit
    /// `Cpu::step` returns). Callers should call this after every CPU step
    /// so the APU's own timing keeps pace with CPU execution.
    pub fn tick_apu(&mut self, cycles: u32) {
        self.apu.tick(cycles);
    }

    /// Read-only access to the APU (e.g. for pulling audio samples).
    pub fn apu_ref(&self) -> &Apu {
        &self.apu
    }

    /// Mutable access to the APU.
    pub fn apu_mut(&mut self) -> &mut Apu {
        &mut self.apu
    }

    /// Read-only access to the PPU (e.g. for pulling rendered frames).
    pub fn ppu_ref(&self) -> &Ppu {
        &self.ppu
    }

    /// Mutable access to the PPU.
    pub fn ppu_mut(&mut self) -> &mut Ppu {
        &mut self.ppu
    }

    /// Selects the machine's video standard, keeping every clock that
    /// depends on it consistent: the PPU's line count and vblank position,
    /// and the master clock the APU converts its pacing units against.
    ///
    /// Prefer this over `ppu_mut().set_mode()`, which changes only the PPU
    /// and would leave the APU converting against the other standard's
    /// master clock -- a ~0.9% error in the generated sample rate.
    pub fn set_video_mode(&mut self, mode: crate::ppu::PpuMode) {
        self.ppu.set_mode(mode);
        self.apu.set_master_clock_hz(match mode {
            crate::ppu::PpuMode::Ntsc => crate::apu::NTSC_MASTER_CLOCK_HZ,
            crate::ppu::PpuMode::Pal => crate::apu::PAL_MASTER_CLOCK_HZ,
        });
    }

    /// Read-only access to the DMA controller (e.g. for diagnostics or
    /// tests checking real transfer-state flags like `is_active()`/
    /// `check_done()`/`hdma_pending()`/`is_enabled()` -- see
    /// `execute_dma_channel`/`hdma_init`/`hdma_run_scanline` for where
    /// those are actually driven from real transfer state).
    pub fn dma_ref(&self) -> &Dma {
        &self.dma
    }

    /// Load a cartridge into the system bus
    pub fn load_cartridge(&mut self, rom: Vec<u8>) -> Result<(), EmulationError> {
        if rom.is_empty() {
            return Err(EmulationError::InvalidAddress(0));
        }
        
        self.cartridge = Some(Cartridge::new(rom));
        Ok(())
    }

    /// Get a mutable reference to WRAM for direct access if needed
    pub fn wram_mut(&mut self) -> &mut Wram {
        &mut self.wram
    }

    /// Get a mutable reference to the cartridge if loaded
    pub fn cartridge_mut(&mut self) -> Option<&mut Cartridge> {
        self.cartridge.as_mut()
    }

    /// Get a read-only reference to the cartridge if loaded
    pub fn cartridge_ref(&self) -> Option<&Cartridge> {
        self.cartridge.as_ref()
    }

    /// Check if a cartridge is loaded
    pub fn has_cartridge(&self) -> bool {
        self.cartridge.is_some()
    }
}
