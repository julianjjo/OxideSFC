//! End-to-end validation against real cartridges, driven through the exact
//! same path the Tauri commands use (`EmulationController::load_rom` and
//! friends) rather than the lower-level `oxidesfc_core` API in isolation:
//! metadata is checksum-validated, frames are really rendered, and the audio
//! really comes out of the DSP.
//!
//! Run with `cargo test -p oxidesfc-frontend`. CI skips these, since ROMs are
//! never committed.

use super::controller::{EmulationController, InputState};
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
        (oxidesfc_core::SCREEN_WIDTH * oxidesfc_core::SCREEN_HEIGHT * 4),
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

/// Diagnostic tool, not an assertion: boots a ROM and writes raw RGBA
/// frames to disk so rendering can be compared against a reference
/// emulator's screenshots by eye. This is how the renderer's mid-frame
/// timing and color-math fixes were verified -- unit tests can pin
/// individual pixels, but only a real game's real title screen shows
/// whether sprites line up with backgrounds.
///
/// ```text
/// OXIDESFC_FRAME_DUMP_DIR=/tmp/frames \
/// OXIDESFC_FRAME_DUMP_ROM="/path/to/game.sfc" \
/// OXIDESFC_FRAME_DUMP_TAG=game \
/// OXIDESFC_FRAME_DUMP_LAST=900 \
///   cargo test -p oxidesfc-frontend dump_real_rom_frames -- --ignored
/// ```
///
/// Four evenly-spaced frames are written as `<tag>_frameNNNN_WxH.rgba`
/// (tightly packed RGBA8888, convertible to PNG with any tool).
#[test]
#[ignore = "diagnostic: dumps frames to OXIDESFC_FRAME_DUMP_DIR for visual inspection"]
fn dump_real_rom_frames() {
    let dir = std::env::var("OXIDESFC_FRAME_DUMP_DIR")
        .expect("set OXIDESFC_FRAME_DUMP_DIR to a writable directory");
    let rom = std::env::var("OXIDESFC_FRAME_DUMP_ROM").unwrap_or_else(|_| rom_path_str());
    let tag = std::env::var("OXIDESFC_FRAME_DUMP_TAG").unwrap_or_else(|_| "rom".into());
    let last: u32 = std::env::var("OXIDESFC_FRAME_DUMP_LAST")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(400);
    let mut controller = EmulationController::new();
    controller.load_rom(&rom).expect("ROM must load");
    controller.start(None).expect("must start");
    for n in 0..last {
        controller.step_frames_now(1);
        if n + 1 == last || (n > 0 && n % (last / 4).max(1) == 0) {
            let f = controller.get_frame();
            std::fs::write(
                format!("{}/{}_frame{:04}_{}x{}.rgba", dir, tag, n, f.width, f.height),
                &f.data,
            )
            .expect("write frame");
        }
    }
}

#[test]
fn real_rom_audio_is_actually_audible_not_just_a_stream_of_zeros() {
    // `step_frame_produces_real_audio_samples...` only proves samples
    // arrive; a fully broken synthesis path produces the right COUNT of
    // silence. This checks the whole audio chain end to end against a real
    // ROM: the SPC700 boots the IPL ROM, the game's driver uploads and runs,
    // it programs the DSP, and the DSP synthesizes a signal.
    //
    // It is the guard for the SPC700 cycle-accounting change in
    // `Apu::tick` (instructions now cost their real cycles instead of one
    // cycle each, cutting SPC700 throughput to ~1/3.5 of what it was): if
    // that had broken the $AA/$BB/$CC upload handshake or starved the
    // driver, the machine would still run and still emit 32kHz of perfect
    // silence, which every other test here would happily accept.
    let mut controller = EmulationController::new();
    controller.load_rom(&rom_path_str()).expect("ROM must load");
    controller.start(None).expect("must be able to start after loading");

    // A few seconds of emulated time: enough for the driver to upload,
    // start its music, and get past any initial silent fade.
    let mut loudest = 0i32;
    let mut nonzero = 0usize;
    let mut total = 0usize;
    for _ in 0..600 {
        controller.step_frames_now(1);
        for s in controller.get_audio() {
            total += 1;
            if s != 0 {
                nonzero += 1;
            }
            loudest = loudest.max((s as i32).abs());
        }
    }

    assert!(
        total > 100_000,
        "600 frames must yield roughly 10 seconds of 32kHz stereo samples, got {}",
        total
    );
    assert!(
        loudest > 512,
        "the DSP must synthesize an audible signal from the real ROM's own \
         sound driver, not silence; loudest sample magnitude was {}",
        loudest
    );
    // Sustained sound, not a single click: a healthy music stream has a
    // large fraction of nonzero samples.
    let ratio = nonzero as f64 / total as f64;
    assert!(
        ratio > 0.10,
        "audio must be sustained rather than a lone transient; only {:.1}% \
         of samples were nonzero",
        ratio * 100.0
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
    controller
        .snes_mut()
        .bus_mut()
        .write_u8(0x004200, 0x01)
        .unwrap();
    controller.set_input(InputState { buttons: 0x40, x: 0, y: 0 });

    // Cross a vblank-entry edge (scanline 230 of 262, NTSC) to trigger
    // the auto-read latch -- see oxidesfc_core::bus's own
    // tick_past_one_vblank_entry test helper for why this exact count.
    controller.snes_mut().bus_mut().tick_ppu(230 * 340 / 2);

    let joy1h = controller.snes_mut().bus_mut().read_u8(0x004219).unwrap();

    assert_eq!(
        joy1h & 0x10,
        0x10,
        "Start bit (d4 of $4219) must be set after EmulationController::set_input presses Start; \
         got {:#04X} -- if this is 0, set_controller_input regressed back to a no-op",
        joy1h
    );
}

#[test]
fn set_input_translates_x_and_y_buttons_to_their_hardware_bits() {
    // Regression guard: `set_controller_input` translated Up/Down/Left/
    // Right/A/B/Start/Select/L/R from the frontend's bitmask but had no
    // arms at all for X (0x400) or Y (0x800), so those bits were
    // silently dropped before reaching the bus -- any key (or gamepad
    // button) bound to X/Y, e.g. DKC's grab/throw on Y, simply never
    // registered. Confirms both bits now land on the real hardware
    // positions ($4218 d6 for X, $4219 d6 for Y) per SystemBus's own
    // bit-layout table.
    use oxidesfc_core::MemoryBus;

    let mut controller = EmulationController::new();
    controller.load_rom(&rom_path_str()).expect("ROM must load");
    controller.start(None).expect("must be able to start after loading");

    controller
        .snes_mut()
        .bus_mut()
        .write_u8(0x004200, 0x01)
        .unwrap();
    controller.set_input(InputState { buttons: 0x400 | 0x800, x: 0, y: 0 });

    controller.snes_mut().bus_mut().tick_ppu(230 * 340 / 2);

    let snes = controller.snes_mut();
    let joy1l = snes.bus_mut().read_u8(0x004218).unwrap();
    let joy1h = snes.bus_mut().read_u8(0x004219).unwrap();

    assert_eq!(
        joy1l & 0x40,
        0x40,
        "X bit (d6 of $4218) must be set after set_input presses X; got {:#04X}",
        joy1l
    );
    assert_eq!(
        joy1h & 0x40,
        0x40,
        "Y bit (d6 of $4219) must be set after set_input presses Y; got {:#04X}",
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
        let snes = controller.snes_mut();
        let state = snes.save_state();
        let probe: Vec<u8> = (0..64u32)
            .map(|i| snes.bus_mut().read_u8(0x7E0000 + i * 64).unwrap())
            .collect();
        let (pc, a) = snes.pc_and_accumulator();
        (state, pc, a, probe)
    };
    assert!(!saved_state.is_empty(), "the snapshot must not be the old empty stub");

    controller.step_frames_now(5);

    {
        let snes = controller.snes_mut();
        snes.load_state(&saved_state).expect("restoring the snapshot must succeed");
        let (pc, a) = snes.pc_and_accumulator();
        assert_eq!(pc, saved_pc, "PC must snap back to the saved moment");
        assert_eq!(a, saved_a, "A must snap back to the saved moment");
        let probe: Vec<u8> = (0..64u32)
            .map(|i| snes.bus_mut().read_u8(0x7E0000 + i * 64).unwrap())
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
        controller
            .snes_mut()
            .force_halt("InvalidAddress(0xDEAD)");
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
        let snes = controller.snes_ref();
        snes.program_counter()
    };
    controller.step();
    controller.step_frame();
    controller.step_frames_now(1);
    let pc_after = {
        let snes = controller.snes_ref();
        snes.program_counter()
    };
    assert_eq!(
        pc_at_halt, pc_after,
        "step()/step_frame() must not advance the CPU any further once halted"
    );
}

/// Loads a zipped ROM through the exact public path the frontend's Play
/// flow uses and proves it boots. This pins the user-facing failure that
/// motivated zip routing in `load_rom`: the library scanner already
/// understood archives, so a `.zip` showed up as a playable library
/// entry, but pressing Play handed the raw zip bytes to the cartridge
/// parser and the game never started.
fn assert_zipped_rom_boots(zip_name: &str, expected_title: &str) {
    let zip_path = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .join(zip_name);
    let zip_path = zip_path
        .canonicalize()
        .unwrap_or_else(|e| {
            panic!(
                "Could not locate the target ROM archive at '{}': {}. This test \
                 exists specifically to validate that this exact file loads \
                 correctly through EmulationController; it must not be silently \
                 skipped.",
                zip_path.display(),
                e
            )
        });
    let zip_path_str = zip_path.to_string_lossy().into_owned();

    let mut controller = EmulationController::new();
    let info = controller
        .load_rom(&zip_path_str)
        .expect("a zipped commercial ROM must load through the Play-flow path");

    assert_eq!(info.title, expected_title);
    assert_eq!(
        info.rom_size, 1_048_576,
        "rom_size must be the extracted cartridge's header-declared size, not the zip's"
    );
    assert_eq!(
        info.file_size,
        std::fs::metadata(&zip_path).unwrap().len(),
        "file_size must be the on-disk archive size"
    );
    assert!(
        info.is_valid && info.validation_errors.is_empty(),
        "a byte-for-byte correct dump must validate cleanly: {:?}",
        info.validation_errors
    );

    // And it must actually run: a few frames of real execution without
    // halting, producing a full-size rendered frame.
    controller.start(None).expect("must start after loading");
    controller.step_frames_now(3);
    assert!(
        !controller.is_halted(),
        "the zipped ROM must execute without halting: {:?}",
        controller.halt_reason()
    );
    let frame = controller.get_frame();
    assert_eq!(
        frame.data.len(),
        (oxidesfc_core::SCREEN_WIDTH * oxidesfc_core::SCREEN_HEIGHT * 4),
        "stepping a zipped ROM must produce a full-size RGBA frame"
    );
}

#[test]
fn load_rom_extracts_and_boots_zipped_castlevania4() {
    assert_zipped_rom_boots("Super Castlevania IV (USA).zip", "SUPER CASTLEVANIA 4");
}

#[test]
fn load_rom_extracts_and_boots_zipped_a_link_to_the_past() {
    assert_zipped_rom_boots(
        "Legend of Zelda, The - A Link to the Past (USA).zip",
        "THE LEGEND OF ZELDA",
    );
}

