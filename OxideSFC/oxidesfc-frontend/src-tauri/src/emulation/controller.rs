//! Start/pause/resume/stop state around a `Snes`, wall-clock frame pacing,
//! save-state file I/O, and the `GameInfo`/`InputState` types the Tauri
//! command layer exchanges with the frontend.

use super::snes::{target_fps, Snes};
use super::video::VideoFrame;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;
use tracing::{error, info, warn};

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
#[derive(Default)]
pub struct InputState {
    pub buttons: u16,
    pub x: i8,
    pub y: i8,
}

/// What one save-state slot currently holds, for the in-game save/load pickers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveSlotInfo {
    pub slot: u8,
    pub occupied: bool,
    /// Size of the snapshot on disk. `None` when the slot is free.
    pub size_bytes: Option<u64>,
    /// Last-write time as Unix milliseconds, so the frontend can render it in
    /// the user's own locale and timezone. `None` when the slot is free, or when
    /// the filesystem does not report a modification time.
    pub saved_at_ms: Option<u64>,
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
    /// Bumped whenever `current_frame` is (or is about to be) replaced.
    /// Together with `last_polled_serial` this lets `poll_frame()` tell a
    /// high-refresh-rate caller "nothing new" instead of re-cloning and
    /// re-base64-encoding an identical ~230KB frame: the frontend polls
    /// once per requestAnimationFrame (a 240Hz monitor = 240 polls/sec)
    /// while NTSC only produces ~60 new frames/sec, so without this ~3 of
    /// every 4 polls did that full encode for nothing -- load that
    /// competed with emulation stepping and audibly starved the audio
    /// pipeline.
    frame_serial: u64,
    /// The value of `frame_serial` at the previous `poll_frame()` call.
    last_polled_serial: u64,
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
            frame_serial: 0,
            last_polled_serial: 0,
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

    /// The machine, for tests that need to reach hardware state the public
    /// controller API deliberately does not expose.
    #[cfg(test)]
    pub(super) fn snes_mut(&mut self) -> &mut Snes {
        self.snes.as_mut().expect("a ROM must be loaded first")
    }

    #[cfg(test)]
    pub(super) fn snes_ref(&self) -> &Snes {
        self.snes.as_ref().expect("a ROM must be loaded first")
    }

    pub fn get_speed(&self) -> f64 {
        self.speed
    }

    pub fn load_rom(&mut self, path: &str) -> Result<GameInfo, String> {
        info!("Loading ROM: {}", path);

        // Same reason as in `start()`: loading another cartridge ends the previous
        // session, and its accumulated seconds have to be banked before the
        // `current_game_id` they belong to is replaced.
        self.flush_play_time();

        let path_buf = PathBuf::from(path);
        if !path_buf.exists() {
            return Err(format!("File not found: {}", path));
        }

        let file_size = std::fs::metadata(&path_buf)
            .map_err(|e| format!("Failed to read file metadata: {}", e))?
            .len();

        // Read the ROM file. ZIP archives go through the same extractor the
        // library scanner already uses -- previously only the scanner
        // understood archives, so a zipped ROM showed up in the library but
        // failed here (raw zip bytes aren't a parseable cartridge) the
        // moment the user hit Play.
        let is_zip = path_buf
            .extension()
            .map(|e| e.to_string_lossy().eq_ignore_ascii_case("zip"))
            .unwrap_or(false);
        let rom_data = if is_zip {
            crate::rom::extract_rom_from_zip(&path_buf)
                .map_err(|e| format!("Failed to extract ROM from archive: {}", e))?
        } else {
            std::fs::read(&path_buf)
                .map_err(|e| format!("Failed to read ROM file: {}", e))?
        };

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

        // Bank whatever the previous session accumulated before its identity is
        // overwritten below. Nothing forces a stop between games: the quick menu's
        // Settings item navigates away without pausing, and unmounting the play
        // view only stops audio -- so `session_start` can still be live from
        // another game when this runs. Without this flush its elapsed time was
        // silently dropped while `record_play_start` happily counted a new
        // session, which is how a game ended up reading "7 sessions / never
        // played".
        self.flush_play_time();

        self.is_running = true;
        self.is_paused = false;
        self.current_game_id = game_id;
        self.session_start = Some(Instant::now());

        // Stamp the library entry with this session. `flush_play_time` handles
        // the accumulated *duration* on pause/stop, but nothing recorded that a
        // session had happened at all, so `play_count` and `last_played` stayed
        // at their scan-time defaults for the life of the library. A failure here
        // must not stop the game from starting -- it is bookkeeping.
        if let Some(ref game_id) = self.current_game_id {
            if let Err(e) = crate::commands::library::record_play_start(game_id) {
                warn!("Failed to record play session for {}: {}", game_id, e);
            }
        }

        self.last_pace = None;
        self.frame_debt = 0.0;
        // A previous run of this ROM may have left samples behind (stop()
        // clears too, but a crash/restart path might not have gone through
        // it) -- starting playback with stale audio is an audible blip.
        self.audio_buffer.clear();
        if let Some(ref mut snes) = self.snes {
            snes.clear_audio_buffer();
        }
        // Force the first poll after start to deliver a frame (even the
        // pre-first-step default one) so the view has something to render
        // immediately, matching the old always-return behavior on mount.
        self.frame_serial = self.frame_serial.wrapping_add(1);
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
        // Discard queued audio on both levels (the controller's drain
        // buffer and the APU's internal sample queue) so nothing from this
        // session leaks into the next start() as a stale-audio blip.
        self.audio_buffer.clear();
        if let Some(ref mut snes) = self.snes {
            snes.clear_audio_buffer();
        }
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

        // Pace against the loaded cartridge's video standard, not always
        // NTSC -- a PAL game stepped at 60.0988 fps runs 20% fast.
        let fps = self
            .snes
            .as_ref()
            .map(|s| target_fps(s.video_mode()))
            .unwrap_or(60.0988);
        let now = Instant::now();
        let elapsed = match self.last_pace {
            // Cap the gap so a stall (debugger, OS sleep, long GC pause)
            // doesn't queue a huge catch-up burst.
            Some(prev) => (now - prev).as_secs_f64().min(0.1),
            None => 1.0 / fps,
        };
        self.last_pace = Some(now);
        self.frame_debt += elapsed * fps * self.speed;

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
    pub(super) fn step_frames_now(&mut self, frames: u32) {
        if let Some(ref mut snes) = self.snes {
            for _ in 0..frames {
                snes.step_until_frame_ready();
                self.audio_buffer.extend(snes.get_audio_samples(4096));
            }
            if frames > 0 {
                self.current_frame = snes.get_frame();
                self.frame_serial = self.frame_serial.wrapping_add(1);
            }
        }
    }

    pub fn get_frame(&self) -> VideoFrame {
        self.current_frame.clone()
    }

    /// Returns the current frame only if it changed since the previous
    /// poll (`None` otherwise), so callers polling faster than the
    /// emulated ~60fps -- the frontend polls at monitor refresh rate --
    /// don't pay the frame clone + base64 encode for identical content.
    pub fn poll_frame(&mut self) -> Option<VideoFrame> {
        if self.frame_serial == self.last_polled_serial {
            return None;
        }
        self.last_polled_serial = self.frame_serial;
        Some(self.current_frame.clone())
    }

    pub fn get_audio(&mut self) -> Vec<i16> {
        std::mem::take(&mut self.audio_buffer)
    }

    pub fn set_input(&mut self, input: InputState) {
        if let Some(ref mut snes) = self.snes {
            snes.set_controller_input(0, input.buttons, input.x, input.y);
        }
    }

    /// Directory holding the save-state slots.
    ///
    /// NOTE: slots are global, not per cartridge -- slot 1 is one file shared by
    /// every game, so saving in one game overwrites another game's slot 1. The
    /// payload itself only loads onto the cartridge it came from (the core's
    /// snapshot excludes the ROM and `load_snapshot` rejects a mismatch), so the
    /// failure is a refused load rather than corruption, but it is still a
    /// surprise worth fixing by keying the filename on the game id.
    fn save_dir() -> PathBuf {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("OxideSFC")
            .join("saves")
    }

    fn slot_path(slot: u8) -> PathBuf {
        Self::save_dir().join(format!("save_{}.state", slot))
    }

    /// Occupancy of every slot, for the in-game save/load pickers.
    ///
    /// The picker previously labelled all ten slots "Empty" unconditionally --
    /// a hardcoded string, with a comment conceding that a real implementation
    /// would show the save date. That made the list actively misleading: a slot
    /// holding a state you cared about looked exactly like a free one.
    pub fn list_save_states(slots: u8) -> Vec<SaveSlotInfo> {
        (0..slots)
            .map(|slot| {
                let path = Self::slot_path(slot);
                let metadata = std::fs::metadata(&path).ok();

                let (size_bytes, saved_at_ms) = match &metadata {
                    Some(meta) => {
                        let modified = meta
                            .modified()
                            .ok()
                            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|delta| delta.as_millis() as u64);
                        (Some(meta.len()), modified)
                    }
                    None => (None, None),
                };

                SaveSlotInfo {
                    slot,
                    occupied: metadata.is_some(),
                    size_bytes,
                    saved_at_ms,
                }
            })
            .collect()
    }

    pub fn save_state(&self, slot: u8) -> Result<(), String> {
        if let Some(ref snes) = self.snes {
            let save_dir = Self::save_dir();

            std::fs::create_dir_all(&save_dir).map_err(|e| e.to_string())?;

            let save_file = Self::slot_path(slot);
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

        let save_file = Self::slot_path(slot);

        if !save_file.exists() {
            return Err(format!("No save state found in slot {}", slot));
        }

        let state = std::fs::read(&save_file).map_err(|e| e.to_string())?;
        
        if let Some(ref mut snes) = self.snes {
            snes.load_state(&state).map_err(|e| e.to_string())?;
        }

        // The timeline just jumped: samples drained before the load belong
        // to the abandoned timeline and would play as a stale-audio blip.
        // (The core's `load_snapshot` already cleared the APU's own queue.)
        self.audio_buffer.clear();

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

