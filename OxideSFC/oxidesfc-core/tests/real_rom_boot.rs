//! End-to-end validation that the actual "Super Mario World (U) [!].smc" file
//! loads and maps correctly, and that the CPU begins executing real cartridge
//! bytes from the genuine 65816 reset vector -- not silently-wrong/open-bus
//! data. This is the regression guard for the original bug: `Cartridge::new`
//! never stripped the 512-byte copier header this file ships with, so every
//! mapped ROM byte was silently read 512 bytes off from where it should be.
//!
//! All ROM-derived expected values below (checksum, reset vector, first
//! opcode bytes) were independently confirmed against the raw file bytes
//! before being written here -- they are not assumptions.

use oxidesfc_core::{Cpu, CpuFlags, EmulationError, MapperType, MemoryBus, SystemBus};
use std::collections::HashSet;

/// Shared interrupt dispatch for every stepping loop in this file: fire
/// the once-per-frame NMI, then the (level-triggered, maskable) timer IRQ
/// if the game has it armed and the CPU would accept it. Without the IRQ
/// half, SMW's in-level raster split (status bar) never runs its handler.
fn dispatch_interrupts(cpu: &mut Cpu, bus: &mut SystemBus) {
    if bus.take_pending_nmi() {
        cpu.nmi(bus).expect("NMI delivery must not fault");
    }
    if bus.irq_pending() && !cpu.p.contains(CpuFlags::IRQ_DISABLE) {
        cpu.irq(bus).expect("IRQ delivery must not fault");
    }
}

const ROM_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../Super Mario World (U) [!].smc"
);

fn load_real_rom() -> Vec<u8> {
    std::fs::read(ROM_PATH).unwrap_or_else(|e| {
        panic!(
            "Could not read the target ROM at '{}': {}. This test exists \
             specifically to validate that this exact file loads correctly; \
             it must not be silently skipped.",
            ROM_PATH, e
        )
    })
}

#[test]
fn real_rom_file_has_expected_raw_size() {
    let data = load_real_rom();
    // 524288-byte LoROM image + a 512-byte copier header.
    assert_eq!(data.len(), 524_800);
    assert_eq!(data.len() % 0x8000, 512, "must look like a headered dump");
}

#[test]
fn real_rom_loads_with_correct_header_and_checksum() {
    let data = load_real_rom();
    let mut bus = SystemBus::new();
    bus.load_cartridge(data).expect("a 524800-byte ROM must load successfully");

    let cart = bus.cartridge_ref().expect("cartridge must be present after load_cartridge");
    let header = cart.header();

    assert!(header.had_copier_header, "must detect the 512-byte copier header");
    assert_eq!(cart.rom_len(), 524_288, "stripped ROM must be exactly 512KB");

    assert_eq!(header.title, "SUPER MARIOWORLD", "title decoded from the wrong offset is the signature symptom of the un-stripped-header bug");
    assert_eq!(header.mapper, MapperType::LoRom);
    assert_eq!(header.rom_type, 0x02, "ROM+RAM+battery (offset 0x16, not the map-mode byte at 0x15)");
    assert_eq!(header.region_code, 0x01, "USA");

    // Values read directly from the file with a hex editor / independent
    // PowerShell byte dump, not derived from the code under test.
    assert_eq!(header.checksum, 0xA0DA);
    assert_eq!(header.checksum_complement, 0x5F25);
    assert!(header.checksum_complement_valid, "checksum ^ complement must equal 0xFFFF");

    // The strongest available proof: recomputing Nintendo's own checksum
    // algorithm over the entire (stripped) ROM image must match the value
    // baked into the cartridge. This can only pass if every single byte of
    // the 512KB ROM was read from the right place.
    assert!(
        header.computed_checksum_matches,
        "recomputed checksum {:#06X} must equal the header's stored checksum {:#06X} -- \
         this is the end-to-end proof that the .smc loaded correctly",
        header.computed_checksum, header.checksum
    );
}

#[test]
fn cpu_reset_reads_genuine_reset_vector_from_cartridge() {
    let data = load_real_rom();
    // Independently re-derive the expected reset vector and first
    // instruction bytes straight from the raw file, duplicating only the
    // simplest possible slice of LoROM-bank-0 mapping math, so this check
    // does not just re-exercise the same code path it's trying to verify.
    let stripped = &data[512..];
    let expected_pc = u16::from_le_bytes([stripped[0x7FFC], stripped[0x7FFD]]);
    let expected_first_byte = stripped[(expected_pc - 0x8000) as usize];

    let mut bus = SystemBus::new();
    bus.load_cartridge(data).unwrap();

    let mut cpu = Cpu::new();
    cpu.reset(&mut bus).expect("reset must read the vector through the bus without error");

    assert_eq!(cpu.pc, expected_pc, "CPU PC after reset must match the byte read directly from the file");
    assert_eq!(cpu.pb, 0x00, "reset always starts in bank 0");
    assert!(cpu.e, "CPU must start in 6502 emulation mode");

    // Read (without consuming/advancing PC) the very first opcode byte the
    // CPU is about to fetch, and cross-check it against the independently
    // computed expectation.
    let first_byte = bus.read_u8(((cpu.pb as u32) << 16) | (cpu.pc as u32)).unwrap();
    assert_eq!(first_byte, expected_first_byte);
    assert_eq!(first_byte, 0x78, "SMW's real boot code starts with SEI");
}

/// Runs the CPU from the real reset vector and reports exactly how far it
/// gets executing genuine cartridge code before hitting any error. This is
/// the test that proves "loaded the ROM" means more than "parsed a header":
/// the CPU must actually fetch and execute real instruction bytes.
///
/// The assertion is intentionally loose on the *count* (>=1, and growing as
/// CPU opcode coverage improves) but strict on *how* it can fail: anything
/// other than a clean `UnimplementedOpcode` from the real CPU pipeline means
/// something is silently wrong (e.g. reading garbage/open-bus and crashing,
/// or an addressing bug), which is exactly the failure mode this project has
/// historically hidden.
#[test]
fn cpu_executes_genuine_boot_instructions_until_a_known_gap() {
    let data = load_real_rom();
    let mut bus = SystemBus::new();
    bus.load_cartridge(data).unwrap();

    let mut cpu = Cpu::new();
    cpu.reset(&mut bus).unwrap();

    let mut steps_executed = 0u32;
    let mut last_error = None;
    const MAX_STEPS: u32 = 1_000_000;

    while steps_executed < MAX_STEPS {
        match cpu.step(&mut bus) {
            Ok(cycles) => {
                // The APU runs as a real, independent SPC700 executing its
                // own genuine IPL boot ROM (see oxidesfc_core::apu) -- it
                // must be ticked forward in lockstep with CPU cycles, same
                // as a real frontend driving the emulator would.
                bus.tick_apu(cycles);
                // Same for the PPU: real hardware fires NMI once per frame
                // at vblank entry, which is what actually unblocks SMW's
                // boot code (it spins on $4210/RDNMI waiting for vblank).
                bus.tick_ppu(cycles);
                dispatch_interrupts(&mut cpu, &mut bus);
                steps_executed += 1;
            }
            Err(e) => {
                last_error = Some(e);
                break;
            }
        }
    }

    eprintln!(
        "CPU executed {} real instruction(s) from the actual SMW reset vector before stopping. Last error: {:?}. PC at stop: {:02X}:{:04X}",
        steps_executed, last_error, cpu.pb, cpu.pc
    );

    assert!(
        steps_executed >= 1,
        "CPU must successfully execute at least the first real instruction (SEI) from the cartridge"
    );

    if let Some(err) = last_error {
        match err {
            EmulationError::UnimplementedOpcode(_) => {
                // Expected gap given current CPU coverage -- not a silent
                // failure, a clearly reported missing instruction.
            }
            other => panic!(
                "CPU halted on something other than a missing opcode after {} steps: {:?}. \
                 This points at a real bug (bad address mapping, bus error, etc.), not just missing coverage.",
                steps_executed, other
            ),
        }
    }
}

/// Goes well beyond "doesn't crash immediately": runs the real CPU for
/// several million cycles and independently verifies the run wasn't just a
/// frozen CPU sitting on one address forever (e.g. a tight 2-instruction
/// spin loop would technically also report "zero errors"). Checks, all
/// derived from genuine execution against the real ROM, not assumptions:
///   - a large number of *distinct* PC values were actually visited
///     (proves real program flow, not a degenerate spin loop)
///   - the PPU's frame counter advanced multiple times (proves NMI is
///     actually firing once per ~frame and the CPU is responding to it)
///   - WRAM was actually mutated by the running program (proves real
///     game-state writes are happening, not just reads/branches)
#[test]
fn cpu_sustains_long_run_with_real_frame_and_state_progression() {
    let data = load_real_rom();
    let mut bus = SystemBus::new();
    bus.load_cartridge(data).unwrap();

    let mut cpu = Cpu::new();
    cpu.reset(&mut bus).unwrap();

    // Snapshot a chunk of low WRAM before running, to later prove the
    // program actually wrote to RAM (SMW's boot/init code clears and then
    // populates working RAM well within the first few frames).
    let wram_before: Vec<u8> = (0u32..0x2000)
        .map(|a| bus.read_u8(0x7E0000 + a).unwrap())
        .collect();

    let mut unique_pcs: HashSet<u32> = HashSet::new();
    let mut nmi_count: u32 = 0;
    let mut steps_executed: u64 = 0;
    let mut last_error = None;
    const MAX_STEPS: u64 = 5_000_000;

    while steps_executed < MAX_STEPS {
        unique_pcs.insert(((cpu.pb as u32) << 16) | (cpu.pc as u32));
        match cpu.step(&mut bus) {
            Ok(cycles) => {
                bus.tick_apu(cycles);
                bus.tick_ppu(cycles);
                if bus.take_pending_nmi() {
                    nmi_count += 1;
                    cpu.nmi(&mut bus).expect("NMI delivery must not fault mid-run");
                }
                if bus.irq_pending() && !cpu.p.contains(CpuFlags::IRQ_DISABLE) {
                    cpu.irq(&mut bus).expect("IRQ delivery must not fault mid-run");
                }
                steps_executed += 1;
            }
            Err(e) => {
                last_error = Some(e);
                break;
            }
        }
    }

    let wram_after: Vec<u8> = (0u32..0x2000)
        .map(|a| bus.read_u8(0x7E0000 + a).unwrap())
        .collect();
    let bytes_changed = wram_before.iter().zip(wram_after.iter()).filter(|(a, b)| a != b).count();

    eprintln!(
        "Sustained run: {} steps, {} distinct PCs visited, {} NMIs delivered, PPU frame={}, {} WRAM bytes changed in $7E0000-$7E1FFF. Last error: {:?}. PC at stop: {:02X}:{:04X}",
        steps_executed, unique_pcs.len(), nmi_count, bus.ppu_ref().frame(), bytes_changed, last_error, cpu.pb, cpu.pc
    );

    if let Some(err) = last_error {
        match err {
            EmulationError::UnimplementedOpcode(op) => {
                panic!(
                    "Hit a missing opcode (0x{:02X}) after {} steps -- this is real coverage \
                     work to do, not a pass/fail ambiguity. PC at stop: {:02X}:{:04X}",
                    op, steps_executed, cpu.pb, cpu.pc
                );
            }
            other => panic!(
                "CPU halted on something other than a missing opcode after {} steps: {:?}.",
                steps_executed, other
            ),
        }
    }

    assert_eq!(steps_executed, MAX_STEPS, "must run the full budget with zero errors to prove sustained stability");
    // CORRECTED baseline: an earlier ~55,800-distinct-PC reading was
    // recorded while the CPU was silently escaping into unmapped bank $EF
    // (a WRAM bank $7E/$7F aliasing bug -- `SystemBus` was folding every
    // $7Fxxxx access onto $7Exxxx instead of treating them as the two
    // independent 64KB halves of the real 128KB WRAM, so SMW's
    // self-modified `OAMResetRoutine` at $7F8000 kept getting clobbered by
    // unrelated $7E8000ish graphics-decompression writes). Wandering
    // through open-bus garbage after that escape inflated the distinct-PC
    // count; it was never a sign of healthy execution. With the bus fixed,
    // the CPU now correctly advances GameMode 0->6 within this budget
    // (see `gamemode_advances_past_the_former_bank_ef_escape_point`) and a
    // *healthy* run legitimately touches far fewer distinct addresses
    // (observed: ~5,864) because it's actually looping through SMW's real,
    // bounded per-frame code instead of chaotically executing garbage.
    // Kept comfortably below the observed number so the test isn't
    // fragile to minor timing/coverage changes, while still catching a
    // regression back to a genuinely narrow stuck loop.
    assert!(
        unique_pcs.len() > 3_000,
        "only {} distinct PC values visited across {} steps -- looks like a regression back to a narrow stuck loop",
        unique_pcs.len(), steps_executed
    );
    assert!(
        nmi_count >= 50,
        "expected many NMIs (frames) to fire over a {}-step run, got {} -- vblank/NMI timing may be broken",
        steps_executed, nmi_count
    );
    assert!(
        bus.ppu_ref().frame() >= 50,
        "PPU frame counter only reached {} -- NMI firing should correspond to real PPU frame advancement",
        bus.ppu_ref().frame()
    );
    assert!(
        bytes_changed >= 200,
        "only {} bytes of low WRAM changed -- expected substantially more state mutation from the \
         deeper execution this run is known to reach",
        bytes_changed
    );
}

/// Validates that the CPU's boot code actually drives real DMA transfers
/// that upload genuine cartridge graphics data into VRAM/CGRAM/OAM -- not
/// just that the CPU runs, but that it produces real PPU memory side
/// effects. This is the regression guard for a second silent-failure bug
/// found in this exact area: `Dma`'s transfer-execution code looked
/// complete (register storage, transfer-mode dispatch, an apparent
/// transfer loop) but its B-bus write path was a stub that read real
/// source bytes and then discarded every one of them -- so MDMAEN writes
/// appeared to "work" (no error, registers updated) while VRAM/CGRAM/OAM
/// silently stayed all-zero forever.
#[test]
fn real_dma_transfers_populate_vram_cgram_oam_with_genuine_cartridge_data() {
    let data = load_real_rom();
    let mut bus = SystemBus::new();
    bus.load_cartridge(data).unwrap();

    let mut cpu = Cpu::new();
    cpu.reset(&mut bus).unwrap();

    const MAX_STEPS: u64 = 3_000_000;
    let mut steps_executed = 0u64;
    for _ in 0..MAX_STEPS {
        match cpu.step(&mut bus) {
            Ok(cycles) => {
                bus.tick_apu(cycles);
                bus.tick_ppu(cycles);
                dispatch_interrupts(&mut cpu, &mut bus);
                steps_executed += 1;
            }
            Err(e) => panic!("CPU halted unexpectedly after {} steps: {:?}", steps_executed, e),
        }
    }

    let vram_nonzero = (0u32..65536).filter(|&a| bus.ppu_ref().vram_ref().read(a as u16) != 0).count();
    let cgram_nonzero = (0u32..512).filter(|&a| bus.ppu_ref().cgram_ref().read(a as u16) != 0).count();
    let oam_nonzero = (0u32..544).filter(|&a| bus.ppu_ref().oam_ref().read(a as u16) != 0).count();

    eprintln!(
        "After {} steps: VRAM {}/65536 nonzero bytes, CGRAM {}/512 nonzero bytes, OAM {}/544 nonzero bytes",
        steps_executed, vram_nonzero, cgram_nonzero, oam_nonzero
    );

    // Thresholds are well below what a real run produces (observed: ~13K
    // VRAM bytes, ~280 CGRAM bytes, ~129 OAM bytes) -- low enough to never
    // be timing-fragile, high enough that a regression back to the
    // discard-everything stub (which produces exactly 0 in all three)
    // fails loudly.
    assert!(
        vram_nonzero > 1000,
        "only {} nonzero VRAM bytes -- DMA should have uploaded real tile/tilemap data from the cartridge by now",
        vram_nonzero
    );
    assert!(
        cgram_nonzero > 50,
        "only {} nonzero CGRAM bytes -- DMA should have uploaded a real color palette from the cartridge by now",
        cgram_nonzero
    );
    assert!(
        oam_nonzero > 0,
        "0 nonzero OAM bytes -- sprite table initialization should have written something by now"
    );
}

/// The strongest validation this suite has: not just "the CPU runs" or
/// "DMA moved bytes into PPU memory", but that the renderer, driven by
/// that real PPU state, produces an actual visible, structured image --
/// not a blank/solid-black frame. This is the direct regression guard for
/// a serious bug found via this exact ROM: `op_php` forced the M/X width
/// bits to 1 when pushing status (a 6502-only quirk that doesn't apply to
/// the 65816 in native mode), which corrupted the accumulator/index width
/// every time SMW's NMI handler ran its own `PHP ... PLP` prologue/
/// epilogue. That silently desynced instruction decoding for whatever
/// code resumed after the interrupt, which manifested as the graphics-
/// decompression routine being re-entered ~374 times per frame instead of
/// once, permanently stuck rendering a solid black screen. After the fix,
/// this routine runs exactly once and the game reaches a real, rendered
/// screen with 11 distinct on-screen colors by ~3M cycles.
#[test]
fn real_rom_eventually_renders_a_visible_non_black_frame() {
    let data = load_real_rom();
    let mut bus = SystemBus::new();
    bus.load_cartridge(data).unwrap();

    let mut cpu = Cpu::new();
    cpu.reset(&mut bus).unwrap();

    const MAX_STEPS: u64 = 10_000_000;
    let mut steps_executed = 0u64;
    for _ in 0..MAX_STEPS {
        match cpu.step(&mut bus) {
            Ok(cycles) => {
                bus.tick_apu(cycles);
                bus.tick_ppu(cycles);
                dispatch_interrupts(&mut cpu, &mut bus);
                steps_executed += 1;
            }
            Err(e) => panic!("CPU halted unexpectedly after {} steps: {:?}", steps_executed, e),
        }
    }

    let regs = *bus.ppu_registers();
    let frame = bus.render_frame();
    assert_eq!(frame.len(), oxidesfc_core::SCREEN_WIDTH * oxidesfc_core::SCREEN_HEIGHT * 4);

    let distinct_colors: HashSet<(u8, u8, u8)> =
        frame.chunks_exact(4).map(|px| (px[0], px[1], px[2])).collect();
    let nonblack_pixels = frame.chunks_exact(4).filter(|px| (px[0], px[1], px[2]) != (0, 0, 0)).count();

    eprintln!(
        "After {} steps: inidisp={:02X} tm={:02X} PC={:02X}:{:04X} -- {} distinct colors, {} non-black pixels",
        steps_executed, regs.inidisp, regs.tm, cpu.pb, cpu.pc, distinct_colors.len(), nonblack_pixels
    );

    assert_eq!(
        regs.inidisp & 0x80,
        0,
        "screen is still in forced blank (INIDISP bit 7 set) after {} steps -- boot never turned the display on",
        steps_executed
    );
    assert!(
        distinct_colors.len() >= 5,
        "only {} distinct colors in the rendered frame -- expected a real, structured screen, not a blank/degenerate one",
        distinct_colors.len()
    );
    assert!(
        nonblack_pixels > 1000,
        "only {} non-black pixels -- expected substantial real visual content on screen",
        nonblack_pixels
    );
}

/// Regression guard for the WRAM bank $7E/$7F aliasing bug:
/// `SystemBus::read_bus`/`write_bus` used to remap every $7Fxxxx access
/// onto $7Exxxx ("mirror"), when in reality $7E and $7F are the two
/// independent 64KB halves of the SNES's real 128KB WRAM. SMW's boot code
/// synthesizes a small self-modified "OAMResetRoutine" at $7F8000 (built
/// byte-by-byte via `STA.L` from `I_RESET`) and calls it via `JSL`
/// dozens of times per frame; with the aliasing bug, an unrelated
/// graphics-decompression routine's writes to $7E8000ish silently
/// clobbered that routine's bytes, so `JSL OAMResetRoutine` eventually
/// executed garbage, corrupted the stack, and the CPU escaped into
/// unmapped bank $EF partway through GameMode 4 (`GM04PrepTitleScreen`)
/// -- well before the game could ever become interactive. This test
/// proves boot now gets well past that point.
#[test]
fn gamemode_advances_past_the_former_bank_ef_escape_point() {
    let data = load_real_rom();
    let mut bus = SystemBus::new();
    bus.load_cartridge(data).unwrap();

    let mut cpu = Cpu::new();
    cpu.reset(&mut bus).unwrap();

    let mut steps_executed: u64 = 0;
    const MAX_STEPS: u64 = 5_000_000;
    let mut max_gamemode_seen: u8 = 0;

    while steps_executed < MAX_STEPS {
        let gamemode = bus.read_u8(0x7E0100).unwrap();
        if gamemode > max_gamemode_seen {
            max_gamemode_seen = gamemode;
        }

        assert_ne!(
            cpu.pb, 0xEF,
            "CPU escaped into unmapped bank $EF at step {} (PC={:02X}:{:04X}) -- the WRAM \
             bank $7E/$7F aliasing bug (or something with the same symptom) is back",
            steps_executed, cpu.pb, cpu.pc
        );

        match cpu.step(&mut bus) {
            Ok(cycles) => {
                bus.tick_apu(cycles);
                bus.tick_ppu(cycles);
                dispatch_interrupts(&mut cpu, &mut bus);
                steps_executed += 1;
            }
            Err(e) => panic!(
                "CPU halted unexpectedly after {} steps at GameMode {:#04X}: {:?}",
                steps_executed, max_gamemode_seen, e
            ),
        }
    }

    assert!(
        max_gamemode_seen >= 6,
        "GameMode only reached {:#04X} within {} steps -- expected it to advance well past the \
         GameMode 4 (GM04PrepTitleScreen) point where the bank $EF escape used to happen",
        max_gamemode_seen, steps_executed
    );
}
