//! The machine itself: a `Cpu` plus a `SystemBus`, stepped together.
//!
//! This is the reference composition of `oxidesfc_core`'s pieces -- the core
//! deliberately has no facade struct that owns and steps the whole system, so
//! this is where "one instruction, then tick the PPU/APU and dispatch
//! interrupts" is actually written down.

use tracing::warn;

/// Maps a cartridge header's region byte ($FFD9) to the video standard the
/// game expects, using bsnes' table (`SuperFamicom::videoRegion`): Japan
/// (0x00), USA (0x01), Taiwan (0x0B), Korea (0x0D), Canada (0x0F) and Brazil
/// (0x10) are 60 Hz; everything else is a 50 Hz PAL territory.
pub(super) fn video_mode_for_region(region_code: u8) -> oxidesfc_core::PpuMode {
    match region_code {
        0x00 | 0x01 | 0x0B | 0x0D | 0x0F | 0x10 => oxidesfc_core::PpuMode::Ntsc,
        _ => oxidesfc_core::PpuMode::Pal,
    }
}

/// Frames per second for a video standard, derived from the same constants
/// the core paces the machine with: 21,477,272 Hz master clock / (341 dots x
/// 4 master cycles x 262 lines) = 60.0988 for NTSC, and 21,281,370 /
/// (341 x 4 x 312) = 50.007 for PAL.
pub(super) fn target_fps(mode: oxidesfc_core::PpuMode) -> f64 {
    match mode {
        oxidesfc_core::PpuMode::Ntsc => 60.0988,
        oxidesfc_core::PpuMode::Pal => 50.0070,
    }
}

// Wrapper for the SNES emulator - composes core components
pub(super) struct Snes {
    cpu: oxidesfc_core::Cpu,
    bus: oxidesfc_core::SystemBus,
    /// Set when `cpu.step()` returns an error. Once set, `step()` becomes a
    /// no-op instead of silently retrying forever -- the previous behavior
    /// was to `warn!()` and keep going, which meant a halted CPU looked
    /// identical to a running one from every caller's perspective.
    halted: Option<String>,
}

impl Snes {
    pub(super) fn new() -> Self {
        Self {
            cpu: oxidesfc_core::Cpu::new(),
            bus: oxidesfc_core::SystemBus::new(),
            halted: None,
        }
    }

    pub(super) fn load_rom(&mut self, data: &[u8]) -> Result<(), String> {
        // Clone data to owned Vec as Cartridge::new requires Vec<u8>
        let rom_vec = data.to_vec();
        self.bus.load_cartridge(rom_vec).map_err(|e| format!("{:?}", e))?;
        // Put the PPU in the video mode the cartridge was built for. The core
        // has always supported PAL (312 lines, 50.007 Hz) but nothing ever
        // selected it: `SystemBus::new` hardcodes NTSC and the header's
        // region byte was only ever parsed into a display string. Every PAL
        // ROM therefore ran as NTSC -- 60.0988 / 50.007 = 1.2x too fast,
        // with the music at the right tempo (the SPC700 has its own clock),
        // which is exactly the "runs faster than bsnes" symptom.
        let mode = self
            .header()
            .map(|h| video_mode_for_region(h.region_code))
            .unwrap_or(oxidesfc_core::PpuMode::Ntsc);
        self.bus.set_video_mode(mode);
        self.cpu.reset(&mut self.bus).map_err(|e| format!("{:?}", e))?;
        self.halted = None;
        Ok(())
    }

    /// The parsed, checksum-validated cartridge header, if a ROM is loaded.
    /// This is the single source of truth for ROM metadata -- GameInfo is
    /// built from this rather than re-parsing the raw bytes separately.
    pub(super) fn header(&self) -> Option<&oxidesfc_core::CartridgeHeader> {
        self.bus.cartridge_ref().map(|c| c.header())
    }

    pub(super) fn step(&mut self) {
        if self.halted.is_some() {
            return;
        }
        match self.cpu.step(&mut self.bus) {
            Ok(cycles) => {
                // Master-clock-accurate timing: the bus recorded every
                // access this instruction made with its real per-region
                // cost (6/8/12 master cycles, FastROM-aware); the
                // instruction's remaining (internal) cycles cost 6 master
                // cycles each. This replaces the old flat
                // 2-dots-per-CPU-cycle SlowROM approximation.
                let (accesses, access_master) = self.bus.take_step_access_costs();
                let internal = cycles.saturating_sub(accesses);
                self.bus.tick_master(access_master + internal * 6);
                let nmi_pending = self.bus.take_pending_nmi();
                // Real 65816 hardware wakes a WAI-suspended CPU on ANY
                // asserted interrupt line (NMI or IRQ), even when the I
                // flag would block the handler from actually running --
                // it just resumes normal fetch without dispatching in
                // that case. Without this, `WAI` executed with IRQ_DISABLE
                // set (or right before an SEI) would hang forever, since
                // the IRQ_DISABLE-gated `cpu.irq()` call below never runs
                // to clear `waiting_for_interrupt` itself.
                self.cpu
                    .wake_if_interrupt_pending(self.bus.irq_pending() || nmi_pending);
                if nmi_pending {
                    if let Err(e) = self.cpu.nmi(&mut self.bus) {
                        let reason = format!("{:?}", e);
                        warn!("CPU halted servicing NMI: {}", reason);
                        self.halted = Some(reason);
                    }
                }
                // Timer IRQ (level-triggered until the game reads $4211):
                // SMW arms this every in-level frame for its status-bar
                // raster split -- without dispatching it, the mid-frame
                // register changes never happen and stale layer-3 content
                // covers the whole screen.
                if self.bus.irq_pending()
                    && !self.cpu.p.contains(oxidesfc_core::CpuFlags::IRQ_DISABLE)
                {
                    if let Err(e) = self.cpu.irq(&mut self.bus) {
                        let reason = format!("{:?}", e);
                        warn!("CPU halted servicing IRQ: {}", reason);
                        self.halted = Some(reason);
                    }
                }
                // The NMI/IRQ dispatch sequences above also touched the
                // bus (stack pushes + vector reads); advance the clock by
                // their real access cost plus the sequence's ~2 internal
                // cycles so interrupt entry isn't free.
                let (int_accesses, int_master) = self.bus.take_step_access_costs();
                if int_accesses > 0 {
                    self.bus.tick_master(int_master + 2 * 6);
                }
            }
            Err(e) => {
                let reason = format!("{:?}", e);
                warn!("CPU halted: {}", reason);
                self.halted = Some(reason);
            }
        }
    }

    /// Steps the CPU until a full video frame completes (or the CPU
    /// halts), bounded by a generous safety cap so a stuck CPU can't loop
    /// here forever. This is what actually drives the emulation at a
    /// usable speed -- calling the single-instruction `step()` once per
    /// displayed frame (the previous behavior of the `get_video_frame`
    /// command this backs) would run the game at roughly one 65816
    /// instruction per ~16ms of real time, several orders of magnitude
    /// too slow to ever reach visible gameplay.
    pub(super) fn step_until_frame_ready(&mut self) {
        const MAX_INSTRUCTIONS_PER_FRAME: u32 = 200_000;
        self.bus.ppu_mut().clear_frame_ready();
        for _ in 0..MAX_INSTRUCTIONS_PER_FRAME {
            if self.halted.is_some() {
                break;
            }
            self.step();
            if self.bus.ppu_ref().is_frame_ready() {
                break;
            }
        }
    }

    /// The video standard this machine is running, which the frame pacer needs
    /// to know its target frame rate. Exposed as a method so the `bus` field
    /// stays private.
    pub(super) fn video_mode(&self) -> oxidesfc_core::PpuMode {
        self.bus.ppu_ref().mode()
    }

    /// Direct bus access, for tests that need to poke hardware registers (e.g.
    /// enabling auto-joypad-read at $4200) or read memory back.
    #[cfg(test)]
    pub(super) fn bus_mut(&mut self) -> &mut oxidesfc_core::SystemBus {
        &mut self.bus
    }

    /// Forces the halted state, so tests can check that every caller treats a
    /// halted machine as halted without having to actually crash the CPU.
    #[cfg(test)]
    pub(super) fn force_halt(&mut self, reason: &str) {
        self.halted = Some(reason.to_string());
    }

    /// The full 24-bit program counter, for tests asserting the CPU did (or
    /// did not) advance.
    #[cfg(test)]
    pub(super) fn program_counter(&self) -> u32 {
        ((self.cpu.pb as u32) << 16) | (self.cpu.pc as u32)
    }

    /// The bank-local PC and accumulator, for save-state round-trip tests that
    /// need to compare CPU state before and after a restore.
    #[cfg(test)]
    pub(super) fn pc_and_accumulator(&self) -> (u16, u16) {
        (self.cpu.pc, self.cpu.a)
    }

    pub(super) fn is_halted(&self) -> bool {
        self.halted.is_some()
    }

    pub(super) fn halt_reason(&self) -> Option<String> {
        self.halted.clone()
    }

    pub(super) fn get_frame(&mut self) -> super::video::VideoFrame {
        let data = self.bus.render_frame();
        super::video::VideoFrame::from_raw(
            oxidesfc_core::SCREEN_WIDTH as u32,
            oxidesfc_core::SCREEN_HEIGHT as u32,
            data,
        )
    }

    /// Drains up to `count` stereo sample frames that `Apu::tick` has
    /// already synthesized into its internal DSP sample buffer -- the
    /// DSP/SPC700 sample generation itself was already fully implemented
    /// and running on every `tick_apu` call; this was the one missing link
    /// that made it unreachable from the frontend.
    ///
    /// Returns interleaved stereo PCM (`L0, R0, L1, R1, ...`), i.e. up to
    /// `count * 2` `i16`s -- using the real `sample_stereo()` accessor
    /// (independent per-voice-panned left/right plus stereo echo) instead
    /// of the mono `sample()` accessor, which averaged `(left + right) /
    /// 2` into a single value and threw away the DSP's real stereo
    /// separation before it ever left this struct. `count` is the number
    /// of stereo frames, matching the caller's existing "samples per
    /// frame" budget (e.g. requesting 2048 now yields 2048 L/R pairs,
    /// 4096 `i16`s, rather than 2048 mono `i16`s).
    pub(super) fn get_audio_samples(&mut self, count: usize) -> Vec<i16> {
        let mut out = Vec::with_capacity(count * 2);
        for _ in 0..count {
            match self.bus.apu_mut().sample_stereo() {
                Some((left, right)) => {
                    out.push(left);
                    out.push(right);
                }
                None => break,
            }
        }
        out
    }

    /// Discards any DSP samples still queued inside the APU. Used when the
    /// emulated timeline is abandoned (stop) so a later start() doesn't
    /// begin by playing leftover audio from the previous session.
    /// (`load_snapshot` already clears this buffer itself on state load.)
    pub(super) fn clear_audio_buffer(&mut self) {
        self.bus.apu_mut().sample_buffer.clear();
    }

    /// Translates the frontend's raw keyboard/gamepad bitmask (see
    /// `EmulatorView.tsx`'s `keyToButton` map: bit0=Up,1=Down,2=Left,
    /// 3=Right,4=A,5=B,6=Start,7=Select,8=L,9=R,10=X,11=Y) into the SNES's
    /// own auto-joypad-read bit layout and forwards it to the bus, where
    /// it's actually visible to the running game via $4016/$4218/$4219.
    /// `x`/`y` duplicate the D-pad bits and are intentionally unused here.
    pub(super) fn set_controller_input(&mut self, port: usize, buttons: u16, _x: i8, _y: i8) {
        if port > 1 {
            return; // the two standard controller ports are modeled
        }

        let mut snes_buttons: u16 = 0;
        if buttons & 0x01 != 0 { snes_buttons |= 0x0800; } // Up
        if buttons & 0x02 != 0 { snes_buttons |= 0x0400; } // Down
        if buttons & 0x04 != 0 { snes_buttons |= 0x0200; } // Left
        if buttons & 0x08 != 0 { snes_buttons |= 0x0100; } // Right
        if buttons & 0x10 != 0 { snes_buttons |= 0x0080; } // A
        if buttons & 0x20 != 0 { snes_buttons |= 0x8000; } // B
        if buttons & 0x40 != 0 { snes_buttons |= 0x1000; } // Start
        if buttons & 0x80 != 0 { snes_buttons |= 0x2000; } // Select
        if buttons & 0x100 != 0 { snes_buttons |= 0x0020; } // L
        if buttons & 0x200 != 0 { snes_buttons |= 0x0010; } // R
        if buttons & 0x400 != 0 { snes_buttons |= 0x0040; } // X
        if buttons & 0x800 != 0 { snes_buttons |= 0x4000; } // Y

        if port == 0 {
            self.bus.set_joypad1_state(snes_buttons);
        } else {
            self.bus.set_joypad2_state(snes_buttons);
        }
    }

    /// Serializes the whole machine (CPU + bus + PPU/APU/DMA + SRAM) via
    /// the core's versioned snapshot format. The ROM itself isn't
    /// included; a state only loads back onto the same cartridge.
    pub(super) fn save_state(&self) -> Vec<u8> {
        oxidesfc_core::save_snapshot(&self.cpu, &self.bus)
    }

    pub(super) fn load_state(&mut self, state: &[u8]) -> Result<(), String> {
        oxidesfc_core::load_snapshot(&mut self.cpu, &mut self.bus, state)
            .map_err(|e| format!("{:?}", e))?;
        // A freshly restored machine is by definition not halted -- any
        // halt belonged to the timeline being discarded.
        self.halted = None;
        Ok(())
    }
}

