use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;
use tracing::{error, info, warn};

// Wrapper for the SNES emulator - composes core components
struct Snes {
    cpu: oxidesfc_core::Cpu,
    bus: oxidesfc_core::SystemBus,
    /// Set when `cpu.step()` returns an error. Once set, `step()` becomes a
    /// no-op instead of silently retrying forever -- the previous behavior
    /// was to `warn!()` and keep going, which meant a halted CPU looked
    /// identical to a running one from every caller's perspective.
    halted: Option<String>,
}

impl Snes {
    fn new() -> Self {
        Self {
            cpu: oxidesfc_core::Cpu::new(),
            bus: oxidesfc_core::SystemBus::new(),
            halted: None,
        }
    }

    fn load_rom(&mut self, data: &[u8]) -> Result<(), String> {
        // Clone data to owned Vec as Cartridge::new requires Vec<u8>
        let rom_vec = data.to_vec();
        self.bus.load_cartridge(rom_vec).map_err(|e| format!("{:?}", e))?;
        self.cpu.reset(&mut self.bus).map_err(|e| format!("{:?}", e))?;
        self.halted = None;
        Ok(())
    }

    /// The parsed, checksum-validated cartridge header, if a ROM is loaded.
    /// This is the single source of truth for ROM metadata -- GameInfo is
    /// built from this rather than re-parsing the raw bytes separately.
    fn header(&self) -> Option<&oxidesfc_core::CartridgeHeader> {
        self.bus.cartridge_ref().map(|c| c.header())
    }

    fn step(&mut self) {
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
                let internal = (cycles as u32).saturating_sub(accesses);
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
    fn step_until_frame_ready(&mut self) {
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

    fn is_halted(&self) -> bool {
        self.halted.is_some()
    }

    fn halt_reason(&self) -> Option<String> {
        self.halted.clone()
    }

    fn get_frame(&self) -> super::video::VideoFrame {
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
    fn get_audio_samples(&mut self, count: usize) -> Vec<i16> {
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

    /// Translates the frontend's raw keyboard/gamepad bitmask (see
    /// `EmulatorView.tsx`'s `keyToButton` map: bit0=Up,1=Down,2=Left,
    /// 3=Right,4=A,5=B,6=Start,7=Select,8=L,9=R) into the SNES's own
    /// auto-joypad-read bit layout and forwards it to the bus, where it's
    /// actually visible to the running game via $4016/$4218/$4219. `x`/`y`
    /// duplicate the D-pad bits and are intentionally unused here.
    fn set_controller_input(&mut self, port: usize, buttons: u16, _x: i8, _y: i8) {
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

        if port == 0 {
            self.bus.set_joypad1_state(snes_buttons);
        } else {
            self.bus.set_joypad2_state(snes_buttons);
        }
    }

    /// Serializes the whole machine (CPU + bus + PPU/APU/DMA + SRAM) via
    /// the core's versioned snapshot format. The ROM itself isn't
    /// included; a state only loads back onto the same cartridge.
    fn save_state(&self) -> Vec<u8> {
        oxidesfc_core::save_snapshot(&self.cpu, &self.bus)
    }

    fn load_state(&mut self, state: &[u8]) -> Result<(), String> {
        oxidesfc_core::load_snapshot(&mut self.cpu, &mut self.bus, state)
            .map_err(|e| format!("{:?}", e))?;
        // A freshly restored machine is by definition not halted -- any
        // halt belonged to the timeline being discarded.
        self.halted = None;
        Ok(())
    }
}

use super::video::VideoFrame;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameInfo {
    pub id: String,
    pub title: String,
    pub file_path: String,
    pub file_size: u64,
    pub rom_type: String,
    pub rom_size: u32,
    pub sram_size: u32,
    pub region: String,
    pub is_valid: bool,
    pub validation_errors: Vec<String>,
    /// Non-fatal validation findings: the ROM still loads and runs, but
    /// something about it is worth surfacing to the user (e.g. a stored
    /// checksum that doesn't match the file's contents -- normal for beta/
    /// prototype dumps and ROM hacks, whose headers were never finalized).
    pub validation_warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputState {
    pub buttons: u16,
    pub x: i8,
    pub y: i8,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            buttons: 0,
            x: 0,
            y: 0,
        }
    }
}

pub struct EmulationController {
    snes: Option<Snes>,
    current_game: Option<GameInfo>,
    is_running: bool,
    is_paused: bool,
    current_frame: VideoFrame,
    audio_buffer: Vec<i16>,
    /// The library `Game.id` (see `commands::library::Game`) tied to the
    /// currently loaded ROM, if the frontend supplied one when calling
    /// `start()`. `None` when a ROM was loaded outside the library flow (or
    /// no game database entry exists for it) -- play time simply isn't
    /// tracked in that case since there'd be nowhere to persist it.
    current_game_id: Option<String>,
    /// Wall-clock time the emulation was last (re)started/resumed. Reset to
    /// `None` after each pause/stop once its elapsed time has been flushed
    /// into `library.json`, so a pause immediately followed by another
    /// pause (or a stop after a pause) can't double-count the same elapsed
    /// interval.
    session_start: Option<Instant>,
    /// Emulation speed multiplier (1.0 = real NTSC speed). `step_frame()`
    /// paces by wall-clock time * this factor, so the game runs at the
    /// same speed regardless of the caller's invocation rate (the frontend
    /// calls once per requestAnimationFrame, i.e. at MONITOR refresh rate
    /// -- before this pacing existed, a 144Hz display ran the game 2.4x
    /// too fast).
    speed: f64,
    /// Fractional emulated frames owed but not yet stepped, carried
    /// between `step_frame()` calls so pacing loses no time to rounding.
    frame_debt: f64,
    /// Wall-clock time of the previous `step_frame()` call. `None` after
    /// start/pause/resume so the first paced call steps exactly one frame
    /// instead of "catching up" across the gap.
    last_pace: Option<Instant>,
}

impl EmulationController {
    pub fn new() -> Self {
        Self {
            snes: None,
            current_game: None,
            is_running: false,
            is_paused: false,
            current_frame: VideoFrame::default(),
            audio_buffer: Vec::new(),
            current_game_id: None,
            session_start: None,
            speed: 1.0,
            frame_debt: 0.0,
            last_pace: None,
        }
    }

    /// Sets the emulation speed multiplier, clamped to a sane range. The
    /// frontend exposes this so the play speed can be tuned live (and the
    /// audio service scales its playback rate by the same factor, so pitch
    /// and tempo follow, like overclocking a real console).
    pub fn set_speed(&mut self, speed: f64) {
        self.speed = speed.clamp(0.1, 4.0);
        info!("Emulation speed set to {:.2}x", self.speed);
    }

    pub fn get_speed(&self) -> f64 {
        self.speed
    }

    pub fn load_rom(&mut self, path: &str) -> Result<GameInfo, String> {
        info!("Loading ROM: {}", path);

        let path_buf = PathBuf::from(path);
        if !path_buf.exists() {
            return Err(format!("File not found: {}", path));
        }

        let file_size = std::fs::metadata(&path_buf)
            .map_err(|e| format!("Failed to read file metadata: {}", e))?
            .len();

        // Read the ROM file
        let rom_data = std::fs::read(&path_buf)
            .map_err(|e| format!("Failed to read ROM file: {}", e))?;

        // Try to create the SNES emulator and load the ROM. This is the
        // single place ROM bytes get parsed: GameInfo is built from
        // whatever oxidesfc_core::Cartridge actually mapped, so the
        // metadata we report can never silently disagree with what got
        // loaded into the CPU/bus.
        let mut snes = Snes::new();
        if let Err(e) = snes.load_rom(&rom_data) {
            error!("Failed to load ROM: {}", e);
            return Err(format!("Failed to load ROM: {}", e));
        }

        let game_info = Self::build_game_info(&snes, path, file_size);

        if !game_info.is_valid {
            warn!("ROM validation failed: {:?}", game_info.validation_errors);
            return Err(format!("Invalid ROM file: {}", game_info.validation_errors.join(", ")));
        }
        if !game_info.validation_warnings.is_empty() {
            warn!(
                "ROM loaded with warnings: {:?}",
                game_info.validation_warnings
            );
        }

        self.snes = Some(snes);
        self.current_game = Some(game_info.clone());

        info!("ROM loaded successfully: {}", game_info.title);

        Ok(game_info)
    }

    /// Builds `GameInfo` from the cartridge header `oxidesfc_core` actually
    /// parsed and validated for `snes`, rather than re-parsing the raw file
    /// bytes with separate logic. `is_valid` (which gates loading) only
    /// requires an internally-consistent header (checksum ^ complement ==
    /// 0xFFFF -- the "this is really a SNES header" signal). A stored
    /// checksum that doesn't match the recomputed one is reported as a
    /// WARNING, not an error: beta/prototype dumps and ROM hacks routinely
    /// ship with unfinalized checksums (e.g. "Prince of Persia 2 (USA)
    /// (Beta)" boots and plays fine), so hard-rejecting on it locked
    /// perfectly playable ROMs out of the emulator entirely.
    fn build_game_info(snes: &Snes, path: &str, file_size: u64) -> GameInfo {
        let file_name = PathBuf::from(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();

        let mut errors = Vec::new();
        let mut warnings = Vec::new();

        let (title, rom_type, rom_size, sram_size, region) = match snes.header() {
            Some(header) => {
                if !header.checksum_complement_valid {
                    errors.push(
                        "ROM header checksum/complement fields are inconsistent -- this likely isn't a real SNES header".to_string(),
                    );
                } else if !header.computed_checksum_matches {
                    warnings.push(format!(
                        "ROM checksum mismatch: header declares {:#06X} but the file's actual contents sum to {:#06X} -- common for beta/prototype dumps and ROM hacks, but it can also mean a corrupt or overdumped file",
                        header.checksum, header.computed_checksum
                    ));
                }

                let title = if header.title.is_empty() {
                    file_name.clone()
                } else {
                    header.title.clone()
                };
                // SNES region codes are individual values (0x00=Japan,
                // 0x01=USA, 0x02=Europe, ...), not 16-wide ranges -- reuse
                // crate::rom::header::Country's table instead of
                // re-deriving it (a range-based guess here was the
                // original, wrong, version of this code).
                let region = crate::rom::header::Country::from_byte(header.region_code)
                    .as_str()
                    .to_string();

                (
                    title,
                    format!("{:02X}", header.rom_type),
                    header.rom_size_bytes,
                    header.sram_size_bytes,
                    region,
                )
            }
            None => {
                errors.push("No cartridge loaded".to_string());
                (file_name.clone(), "Unknown".to_string(), file_size as u32, 0, "Unknown".to_string())
            }
        };

        GameInfo {
            id: uuid::Uuid::new_v4().to_string(),
            title,
            file_path: path.to_string(),
            file_size,
            rom_type,
            rom_size,
            sram_size,
            region,
            is_valid: errors.is_empty(),
            validation_errors: errors,
            validation_warnings: warnings,
        }
    }

    /// Starts emulation. `game_id` is the library `Game.id` (see
    /// `commands::library::Game`) the frontend has on hand from the `Game`
    /// object it just called `load_rom` with -- passing it through here is
    /// what lets play-time tracking (see `flush_play_time`) attribute
    /// accumulated seconds back to the right library entry. `None` when
    /// starting a ROM outside the normal library-play flow.
    pub fn start(&mut self, game_id: Option<String>) -> Result<(), String> {
        if self.snes.is_none() {
            return Err("No ROM loaded".to_string());
        }

        self.is_running = true;
        self.is_paused = false;
        self.current_game_id = game_id;
        self.session_start = Some(Instant::now());
        self.last_pace = None;
        self.frame_debt = 0.0;
        info!("Emulation started");
        Ok(())
    }

    pub fn pause(&mut self) -> Result<(), String> {
        if !self.is_running {
            return Err("Emulation not running".to_string());
        }

        self.is_paused = true;
        self.flush_play_time();
        info!("Emulation paused");
        Ok(())
    }

    pub fn resume(&mut self) -> Result<(), String> {
        if !self.is_running {
            return Err("Emulation not running".to_string());
        }

        if self.is_halted() {
            // A halted CPU cannot be un-halted by toggling is_paused -- the
            // only way out is loading a new ROM (which resets `halted` to
            // `None` in `Snes::load_rom`). Without this check, resume() used
            // to unconditionally clear is_paused and log success even though
            // `Snes::step`/`step_until_frame_ready` would keep silently
            // no-op'ing forever, so the frontend had no way to tell "resumed
            // and running" apart from "resumed but permanently stuck".
            let reason = self.halt_reason().unwrap_or_else(|| "unknown reason".to_string());
            warn!("Cannot resume: emulation is halted ({})", reason);
            return Err(format!("Cannot resume: emulation halted ({})", reason));
        }

        self.is_paused = false;
        // Restart the play-time clock -- the previous running interval was
        // already flushed by whichever pause() call led here.
        self.session_start = Some(Instant::now());
        // Restart the pacing clock too, so the pause gap isn't "caught up".
        self.last_pace = None;
        self.frame_debt = 0.0;
        info!("Emulation resumed");
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), String> {
        self.is_running = false;
        self.is_paused = false;
        self.flush_play_time();
        self.current_game_id = None;
        info!("Emulation stopped");
        Ok(())
    }

    /// Adds the real-world seconds elapsed since `session_start` to the
    /// current game's `total_play_seconds` in `library.json`, then clears
    /// `session_start` so the same interval is never counted twice (e.g. if
    /// `pause()` is called twice in a row, or `stop()` follows a `pause()`
    /// without an intervening `resume()`).
    ///
    /// This is intentionally a lightweight accumulate-on-stop/pause model,
    /// not a live-updating counter -- nothing polls this while running.
    fn flush_play_time(&mut self) {
        let Some(start) = self.session_start.take() else {
            return;
        };
        let Some(ref game_id) = self.current_game_id else {
            return;
        };

        let seconds = start.elapsed().as_secs();
        if let Err(e) = crate::commands::library::add_play_seconds_to_file(game_id, seconds) {
            warn!("Failed to persist play time for game {}: {}", game_id, e);
        }
    }

    /// Advances the emulation by a single CPU instruction. Does *not*
    /// refresh `current_frame` -- a video frame isn't actually complete
    /// until vblank, so re-rendering after every single instruction was
    /// both meaningless (most calls land mid-scanline) and, now that
    /// rendering does real tile/sprite compositing, very expensive to do
    /// at this granularity. Use `step_frame()` for frame-driven callers.
    pub fn step(&mut self) {
        if let Some(ref mut snes) = self.snes {
            if self.is_running && !self.is_paused {
                snes.step();
            }
        }
    }

    /// Advances the emulation by however many video frames real time (and
    /// the speed multiplier) say are due, refreshing `current_frame` and
    /// draining audio once per stepped frame.
    ///
    /// This is deliberately WALL-CLOCK paced rather than
    /// one-frame-per-call: the caller (`get_video_frame`, invoked once per
    /// requestAnimationFrame) runs at the MONITOR's refresh rate, so a
    /// call-paced version played at whatever Hz the display happened to be
    /// -- 2.4x too fast on a 144Hz panel, too slow in a throttled
    /// background tab. Here a 144Hz caller simply steps 0 frames on most
    /// calls and 1 on the rest, averaging the NTSC 60.0988 fps times
    /// `self.speed`.
    pub fn step_frame(&mut self) {
        if self.snes.is_none() || !self.is_running || self.is_paused {
            return;
        }

        const NTSC_FPS: f64 = 60.0988;
        let now = Instant::now();
        let elapsed = match self.last_pace {
            // Cap the gap so a stall (debugger, OS sleep, long GC pause)
            // doesn't queue a huge catch-up burst.
            Some(prev) => (now - prev).as_secs_f64().min(0.1),
            None => 1.0 / NTSC_FPS,
        };
        self.last_pace = Some(now);
        self.frame_debt += elapsed * NTSC_FPS * self.speed;

        // Never step more than a handful of frames per call: if the host
        // can't keep up, dropping the debt (running slow) beats spiraling
        // further behind.
        let frames = (self.frame_debt as u32).min(6);
        self.frame_debt -= frames as f64;
        if frames == 6 {
            self.frame_debt = self.frame_debt.min(1.0);
        }

        self.step_frames_now(frames);
    }

    /// Steps exactly `frames` video frames, unconditionally (the
    /// running/paused gate lives in the paced `step_frame()`). Split out
    /// so tests can advance a deterministic number of frames without
    /// depending on wall-clock pacing.
    fn step_frames_now(&mut self, frames: u32) {
        if let Some(ref mut snes) = self.snes {
            for _ in 0..frames {
                snes.step_until_frame_ready();
                self.audio_buffer.extend(snes.get_audio_samples(4096));
            }
            if frames > 0 {
                self.current_frame = snes.get_frame();
            }
        }
    }

    pub fn get_frame(&self) -> VideoFrame {
        self.current_frame.clone()
    }

    pub fn get_audio(&mut self) -> Vec<i16> {
        let audio = self.audio_buffer.clone();
        self.audio_buffer.clear();
        audio
    }

    pub fn set_input(&mut self, input: InputState) {
        if let Some(ref mut snes) = self.snes {
            snes.set_controller_input(0, input.buttons, input.x, input.y);
        }
    }

    pub fn save_state(&self, slot: u8) -> Result<(), String> {
        if let Some(ref snes) = self.snes {
            let save_dir = dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("OxideSFC")
                .join("saves");

            std::fs::create_dir_all(&save_dir).map_err(|e| e.to_string())?;

            let save_file = save_dir.join(format!("save_{}.state", slot));
            let state = snes.save_state();
            
            std::fs::write(&save_file, state).map_err(|e| e.to_string())?;
            
            info!("State saved to slot {}", slot);
            Ok(())
        } else {
            Err("No ROM loaded".to_string())
        }
    }

    pub fn load_state(&mut self, slot: u8) -> Result<(), String> {
        if self.snes.is_none() {
            return Err("No ROM loaded".to_string());
        }

        let save_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("OxideSFC")
            .join("saves");

        let save_file = save_dir.join(format!("save_{}.state", slot));
        
        if !save_file.exists() {
            return Err(format!("No save state found in slot {}", slot));
        }

        let state = std::fs::read(&save_file).map_err(|e| e.to_string())?;
        
        if let Some(ref mut snes) = self.snes {
            snes.load_state(&state).map_err(|e| e.to_string())?;
        }
        
        info!("State loaded from slot {}", slot);
        Ok(())
    }

    /// Reports whether the emulation is actively advancing. Deliberately
    /// `false` once the inner CPU has halted, even though the `is_running`
    /// flag set by `start()` is still `true` at that point -- `step()`/
    /// `step_frame()` become permanent no-ops after a halt (see
    /// `Snes::step`), so telling a caller "running" at that point would be a
    /// straightforward lie: nothing is actually advancing anymore.
    pub fn is_running(&self) -> bool {
        self.is_running && !self.is_halted()
    }

    /// Reports whether the emulation is paused (i.e. running but not
    /// currently stepping by user request). Also `false` once halted --
    /// "paused" implies resumable, and a halted session isn't.
    pub fn is_paused(&self) -> bool {
        self.is_paused && !self.is_halted()
    }

    /// True if the CPU hit an error (e.g. an unimplemented opcode) and
    /// stopped advancing. Distinct from `is_paused`: a paused emulator can
    /// resume; a halted one cannot until a new ROM is loaded.
    pub fn is_halted(&self) -> bool {
        self.snes.as_ref().map(|s| s.is_halted()).unwrap_or(false)
    }

    /// The reason the CPU halted, if it has.
    pub fn halt_reason(&self) -> Option<String> {
        self.snes.as_ref().and_then(|s| s.halt_reason())
    }

    pub fn get_game_info(&self) -> Option<GameInfo> {
        self.current_game.clone()
    }
}

#[cfg(test)]
mod real_rom_tests {
    //! End-to-end validation that loading the actual
    //! "Super Mario World (U) [!].smc" through the exact same path the
    //! Tauri `load_rom` command uses (`EmulationController::load_rom`)
    //! produces correct, checksum-validated metadata -- not just that the
    //! lower-level `oxidesfc_core` API works in isolation. Run via
    //! `cargo test -p oxidesfc-frontend`.

    use super::*;

    const ROM_PATH: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../Super Mario World (U) [!].smc"
    );

    fn rom_path_str() -> String {
        std::path::Path::new(ROM_PATH)
            .canonicalize()
            .unwrap_or_else(|e| {
                panic!(
                    "Could not locate the target ROM at '{}': {}. This test exists \
                     specifically to validate that this exact file loads correctly \
                     through EmulationController; it must not be silently skipped.",
                    ROM_PATH, e
                )
            })
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn load_rom_reports_correct_validated_metadata() {
        let mut controller = EmulationController::new();
        let path = rom_path_str();

        let info = controller
            .load_rom(&path)
            .expect("the real SMW ROM must load successfully through EmulationController");

        assert_eq!(info.title, "SUPER MARIOWORLD");
        assert_eq!(info.file_size, 524_800, "raw file size including the 512-byte copier header");
        assert_eq!(info.rom_size, 524_288, "header-declared ROM size, after stripping the copier header");
        assert_eq!(info.sram_size, 2048);
        assert_eq!(info.region, "USA");
        assert!(
            info.is_valid && info.validation_errors.is_empty(),
            "a byte-for-byte correct ROM must validate cleanly: {:?}",
            info.validation_errors
        );

        // get_game_info() must reflect the same data that was just loaded.
        let stored = controller.get_game_info().expect("a game must be loaded");
        assert_eq!(stored.title, info.title);
    }

    #[test]
    fn stepping_executes_real_cartridge_code_and_surfaces_halts_instead_of_swallowing_them() {
        let mut controller = EmulationController::new();
        controller.load_rom(&rom_path_str()).expect("ROM must load");
        controller.start(None).expect("must be able to start after loading");

        assert!(!controller.is_halted(), "freshly loaded ROM must not start out halted");

        // Step far enough to run past the current CPU's opcode coverage
        // (the real APU now genuinely executes its own boot ROM in
        // lockstep, so this takes noticeably more than a handful of
        // steps). Whatever happens, the controller must end up in an
        // observable state -- either still running, or explicitly halted
        // with a reason -- never silently stuck while claiming to be fine.
        let mut steps_taken = 0u32;
        for _ in 0..200_000 {
            controller.step();
            steps_taken += 1;
            if controller.is_halted() {
                break;
            }
        }
        eprintln!("stepped {} times; halted={}", steps_taken, controller.is_halted());

        if controller.is_halted() {
            let reason = controller.halt_reason().expect("halted implies a reason is recorded");
            eprintln!("Emulation halted as expected given current CPU opcode coverage: {}", reason);
            assert!(
                reason.contains("UnimplementedOpcode"),
                "expected a clean unimplemented-opcode halt, got: {}",
                reason
            );
        }
    }

    #[test]
    fn step_frame_advances_many_instructions_and_refreshes_the_rendered_frame() {
        // Regression guard for a real architecture gap: `get_video_frame`
        // (the Tauri command driving the frontend's per-displayed-frame
        // polling) used to call the single-instruction `step()`, so each
        // displayed frame only advanced the CPU by one 65816 instruction
        // -- several orders of magnitude too slow to ever reach visible
        // gameplay. `step_frame()` must run until a real PPU frame
        // completes (or the CPU halts) instead.
        let mut controller = EmulationController::new();
        controller.load_rom(&rom_path_str()).expect("ROM must load");
        controller.start(None).expect("must be able to start after loading");

        let before = controller.get_frame();
        assert_eq!(before.data.len(), 0, "freshly loaded ROM must not have a rendered frame yet");

        // Deterministic single frame -- the public `step_frame()` is
        // wall-clock paced (0 or more frames per call), which a unit test
        // must not depend on.
        controller.step_frames_now(1);

        let after = controller.get_frame();
        assert_eq!(
            after.data.len(),
            (oxidesfc_core::SCREEN_WIDTH * oxidesfc_core::SCREEN_HEIGHT * 4) as usize,
            "step_frame must populate a full-size RGBA8888 frame"
        );
        assert_eq!(after.width, oxidesfc_core::SCREEN_WIDTH as u32);
        assert_eq!(after.height, oxidesfc_core::SCREEN_HEIGHT as u32);

        // Every alpha byte must be opaque (0xFF) -- a strong, cheap signal
        // that the renderer actually ran end-to-end (an all-zero/garbage
        // buffer would not have this property reliably).
        assert!(
            after.data.chunks_exact(4).all(|px| px[3] == 0xFF),
            "every pixel's alpha channel must be fully opaque"
        );
    }

    #[test]
    fn step_frame_produces_real_audio_samples_not_the_old_permanently_empty_stub() {
        // Regression guard: `Snes::get_audio_samples` used to unconditionally
        // return `Vec::new()` ("APU integration would go here"), even though
        // `Apu::tick` already synthesizes real DSP samples into its own
        // internal buffer on every call -- the frontend's audio pipeline
        // was permanently starved despite the DSP doing real work under it.
        // Running several frames (enough for `Apu::tick`'s ~32kHz sample
        // generation, fixed alongside this, to have produced far more than
        // one frame's worth) must yield a non-empty, sanely-sized buffer.
        let mut controller = EmulationController::new();
        controller.load_rom(&rom_path_str()).expect("ROM must load");
        controller.start(None).expect("must be able to start after loading");

        controller.step_frames_now(5);

        let audio = controller.get_audio();
        assert!(
            !audio.is_empty(),
            "get_audio() must return real samples once the DSP has had time to generate them, not an empty Vec"
        );
        // Each stepped frame drains up to 4096 samples into `audio_buffer`
        // (which accumulates rather than overwrites: samples not yet
        // drained via `get_audio()` must not be discarded). Draining once
        // after 5 undrained frames can therefore legitimately return up to
        // 5 * 4096 samples, not just one frame's worth.
        assert!(
            audio.len() <= 4096 * 5,
            "get_audio() must not return more than the accumulated per-frame sample budget across the undrained frames, got {}",
            audio.len()
        );
    }

    #[test]
    fn set_input_reaches_the_bus_and_is_visible_via_auto_joypad_read() {
        // Regression guard for the exact bug this was written to fix:
        // `Snes::set_controller_input` used to be a complete no-op ("Input
        // handling would go here"), so no keypress from the frontend could
        // ever reach the emulated CPU -- games would sit on their title
        // screen forever since Start never registered. This drives the
        // real call path (EmulationController::set_input ->
        // Snes::set_controller_input -> SystemBus::set_joypad1_state) and
        // confirms the button is actually visible where the real ROM's
        // boot code reads it: the $4218/$4219 auto-joypad-read registers.
        use oxidesfc_core::MemoryBus;

        let mut controller = EmulationController::new();
        controller.load_rom(&rom_path_str()).expect("ROM must load");
        controller.start(None).expect("must be able to start after loading");

        // Enable auto-joypad-read the same way real boot code does (NMITIMEN
        // bit0), then press Start using the frontend's own bitmask (0x40,
        // per EmulatorView.tsx's keyToButton map).
        {
            let snes = controller.snes.as_mut().expect("snes must exist after load_rom");
            snes.bus.write_u8(0x004200, 0x01).unwrap();
        }
        controller.set_input(InputState { buttons: 0x40, x: 0, y: 0 });

        // Cross a vblank-entry edge (scanline 230 of 262, NTSC) to trigger
        // the auto-read latch -- see oxidesfc_core::bus's own
        // tick_past_one_vblank_entry test helper for why this exact count.
        {
            let snes = controller.snes.as_mut().unwrap();
            snes.bus.tick_ppu(230 * 340 / 2);
        }

        let joy1h = controller.snes.as_mut().unwrap().bus.read_u8(0x004219).unwrap();

        assert_eq!(
            joy1h & 0x10,
            0x10,
            "Start bit (d4 of $4219) must be set after EmulationController::set_input presses Start; \
             got {:#04X} -- if this is 0, set_controller_input regressed back to a no-op",
            joy1h
        );
    }

    #[test]
    fn checksum_mismatch_loads_with_a_warning_instead_of_being_rejected() {
        // A stored-vs-recomputed checksum mismatch cannot distinguish a
        // corrupt dump from a beta/prototype whose header was simply never
        // finalized ("Prince of Persia 2 (USA) (Beta)" is a real example
        // that boots and plays fine). Hard-rejecting on it locked such
        // ROMs out of the emulator entirely, so the mismatch must load
        // successfully but be surfaced in `validation_warnings`.
        let raw = std::fs::read(rom_path_str()).unwrap();
        let mut mismatched = raw.clone();
        mismatched[10_000] ^= 0xFF;

        let dir = std::env::temp_dir().join("oxidesfc_test_corrupted_rom");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("corrupted.smc");
        std::fs::write(&path, &mismatched).unwrap();

        let mut controller = EmulationController::new();
        let result = controller.load_rom(path.to_str().unwrap());

        let info = result.expect("a checksum mismatch alone must not block loading");
        assert!(info.is_valid, "the ROM must still be reported as loadable");
        assert!(
            info.validation_warnings.iter().any(|w| w.contains("checksum")),
            "the mismatch must be surfaced as a warning: {:?}",
            info.validation_warnings
        );
        assert!(
            info.validation_errors.is_empty(),
            "no hard errors expected: {:?}",
            info.validation_errors
        );

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn emulation_speed_is_clamped_to_a_sane_range() {
        let mut controller = EmulationController::new();
        assert_eq!(controller.get_speed(), 1.0, "default speed must be exactly 1.0 (real NTSC)");
        controller.set_speed(99.0);
        assert_eq!(controller.get_speed(), 4.0, "speed must clamp to the 4.0 upper bound");
        controller.set_speed(0.0);
        assert_eq!(controller.get_speed(), 0.1, "speed must clamp to the 0.1 lower bound");
        controller.set_speed(1.25);
        assert_eq!(controller.get_speed(), 1.25);
    }

    #[test]
    fn save_state_round_trips_a_running_real_rom() {
        // Boot the real ROM, advance it, snapshot, advance further (so the
        // machine state genuinely diverges), then restore -- the CPU and
        // WRAM must snap back to exactly the saved moment. This exercises
        // the real core snapshot payload end-to-end, replacing the old
        // no-op stub that returned an empty Vec and restored nothing.
        use oxidesfc_core::MemoryBus;

        let mut controller = EmulationController::new();
        controller.load_rom(&rom_path_str()).expect("ROM must load");
        controller.start(None).expect("must start");

        controller.step_frames_now(5);

        let (saved_state, saved_pc, saved_a, saved_wram_probe) = {
            let snes = controller.snes.as_mut().unwrap();
            let state = snes.save_state();
            let probe: Vec<u8> = (0..64u32)
                .map(|i| snes.bus.read_u8(0x7E0000 + i * 64).unwrap())
                .collect();
            (state, snes.cpu.pc, snes.cpu.a, probe)
        };
        assert!(!saved_state.is_empty(), "the snapshot must not be the old empty stub");

        controller.step_frames_now(5);

        {
            let snes = controller.snes.as_mut().unwrap();
            snes.load_state(&saved_state).expect("restoring the snapshot must succeed");
            assert_eq!(snes.cpu.pc, saved_pc, "PC must snap back to the saved moment");
            assert_eq!(snes.cpu.a, saved_a, "A must snap back to the saved moment");
            let probe: Vec<u8> = (0..64u32)
                .map(|i| snes.bus.read_u8(0x7E0000 + i * 64).unwrap())
                .collect();
            assert_eq!(probe, saved_wram_probe, "WRAM must snap back to the saved moment");
        }

        // And the restored machine must keep executing normally.
        controller.step_frames_now(1);
        assert!(!controller.is_halted(), "a restored machine must keep running");
    }

    #[test]
    fn halted_cpu_is_never_reported_as_running_and_resume_refuses_to_pretend_it_worked() {
        // Regression guard for the bug this whole change addresses:
        // EmulationController used to track is_running/is_paused as
        // independent booleans that were never reconciled against the
        // inner Snes's real `halted` state. That meant a CPU that hit a
        // fatal step() error still showed up as "running" from every
        // status getter, and resume() would happily flip is_paused back
        // to false and log "Emulation resumed" even though
        // step()/step_frame() had already become permanent no-ops.
        //
        // This test used to force the halt by executing opcode 0x3B, which
        // was one of two unimplemented 65816 opcodes at the time; the CPU
        // now implements all 256, so no fetchable byte can produce that
        // error anymore. Inject the halt directly instead -- what's under
        // test is the controller's honesty about an already-halted Snes,
        // not the (no-longer-reachable) CPU error path that used to set it.
        let mut controller = EmulationController::new();
        controller.load_rom(&rom_path_str()).expect("ROM must load");
        controller.start(None).expect("must be able to start after loading");

        assert!(!controller.is_halted(), "freshly loaded ROM must not start out halted");
        assert!(controller.is_running(), "freshly started emulation must report running");

        {
            let snes = controller.snes.as_mut().expect("snes must exist after load_rom");
            snes.halted = Some("InvalidAddress(0xDEAD)".to_string());
        }

        assert!(
            controller.is_halted(),
            "a Snes with `halted` set must be reported as halted by the controller"
        );
        let reason = controller.halt_reason().expect("halted implies a reason is recorded");
        assert!(
            reason.contains("InvalidAddress"),
            "the halt reason must surface the inner error, got: {}",
            reason
        );

        // The controller's own `is_running`/`is_running` flag is still
        // `true` internally (start() set it and nothing has called stop()),
        // but the *reported* status must not claim the emulation is
        // actually running or pausable once halted -- nothing is advancing
        // anymore.
        assert!(
            !controller.is_running(),
            "a halted emulation must never report is_running() == true, even though the \
             internal is_running flag set by start() hasn't been cleared"
        );
        assert!(
            !controller.is_paused(),
            "a halted emulation must never report is_paused() == true either"
        );

        // resume() must refuse outright rather than silently succeeding --
        // silently flipping is_paused back to false here would tell the
        // frontend "you're good to go" about a CPU that will never execute
        // another instruction.
        let resume_result = controller.resume();
        assert!(
            resume_result.is_err(),
            "resume() on a halted emulation must return an error, not silently succeed"
        );
        assert!(
            resume_result.unwrap_err().to_lowercase().contains("halt"),
            "resume()'s error should explain that the emulation is halted, not give a generic message"
        );

        // And stepping further must remain a genuine no-op: the PC must
        // not move past the halting instruction.
        let pc_at_halt = {
            let snes = controller.snes.as_ref().unwrap();
            ((snes.cpu.pb as u32) << 16) | (snes.cpu.pc as u32)
        };
        controller.step();
        controller.step_frame();
        controller.step_frames_now(1);
        let pc_after = {
            let snes = controller.snes.as_ref().unwrap();
            ((snes.cpu.pb as u32) << 16) | (snes.cpu.pc as u32)
        };
        assert_eq!(
            pc_at_halt, pc_after,
            "step()/step_frame() must not advance the CPU any further once halted"
        );
    }
}
