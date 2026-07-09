# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Repository layout

This is not a single project — it's a workspace with one Rust application and supporting planning docs:

- `OxideSFC/` — the actual code: a Cargo workspace with two members:
  - `oxidesfc-core/` — the SNES emulation core (CPU, PPU, APU, DMA, memory bus, cartridge). No UI, no I/O dependencies beyond `bitflags`.
  - `oxidesfc-frontend/` — a Tauri v2 + React/TypeScript desktop shell around the core.
- `plans/` — design/status documents, in Spanish and English. Treat these as **aspirational/historical**, not ground truth:
  - `emulator-implementation-status.md` — a (possibly stale) progress audit of `oxidesfc-core` by component.
  - `oxidesfc-frontend-specification.md` — a forward-looking technical spec for the frontend (SQLite library DB, WebGPU shaders, metadata scraping, etc.). Much of it describes intended future architecture, not what's implemented today (e.g. there is no SQLite database in the current code — settings/library state is simpler than the spec describes). Don't assume code matches the spec; check the actual source first.

There is no root `Cargo.toml` outside `OxideSFC/` — always run cargo commands from `OxideSFC/`.

## Commands

All Rust commands run from `OxideSFC/` (the workspace root):

```bash
cd OxideSFC
cargo build                       # build both workspace members
cargo build -p oxidesfc-core      # build just the emulation core
cargo test                        # run all tests in the workspace
cargo test -p oxidesfc-core       # run only core tests
cargo test bus::tests::system_bus_wram_mirror   # run a single test by path
```

Frontend (from `OxideSFC/oxidesfc-frontend/`):

```bash
npm install
npm run dev        # vite dev server only (no Tauri window)
npm run build      # tsc typecheck + vite build
cargo tauri dev    # full Tauri app: launches vite dev server + native window (requires Tauri CLI)
cargo tauri build  # produce a release bundle
```

`oxidesfc-frontend` is both a Rust crate (`src-tauri/`, binary `oxidesfc-frontend`) and an npm package (`src/`, the React UI) living in the same directory — `Cargo.toml` and `package.json` sit side by side at `oxidesfc-frontend/`, with Rust sources under `src-tauri/src/` and TS/React sources under `src/`.

The release profile (`OxideSFC/Cargo.toml`) uses `panic = "abort"`, `lto = true`, `opt-level = "s"` — optimized for small binary size, consistent with the project's stated preference for a lightweight emulator frontend over Electron.

## Architecture

### oxidesfc-core: components are wired together through SystemBus

The core crate models each SNES subsystem as an independent module (`cpu.rs`, `ppu.rs`, `apu.rs`, `dma.rs`, `vram.rs`, `cgram.rs`, `oam.rs`, `wram.rs`, `cartridge.rs`, `io.rs`, `bus.rs`, `renderer.rs`, `state.rs`), each fairly substantial in isolation (`cpu.rs` ~6000 lines, `apu.rs` ~3700 lines). The 65816 implements all 256 opcodes (the dispatch match has no wildcard arm — exhaustiveness is compiler-checked) and the SPC700 implements all 256 of its own (pinned by `every_spc700_opcode_executes_without_halting_except_stop`). **`SystemBus` (`bus.rs`) owns real `Apu`/`Ppu`/`Dma` instances and fully dispatches the memory-mapped register space to them**: PPU registers ($2100-$213F, including Mode 7 $211A-$2120 with the MPY multiplier at $2134-$2136, the H/V counter latches $2137/$213C-$213D, and STAT77/78), the WRAM data port ($2180-$2183), APU communication ports ($2140-$217F), NMITIMEN/WRIO/hardware-multiply-divide/HDMAEN/MEMSEL and both joypads ($4200-$421B), and DMA channel registers ($4300-$437F) all route to real component state, including real immediate-DMA and HDMA execution, NMI/IRQ dispatch back into the CPU (via vblank/hblank edge-detection), and auto-joypad-read latching. `read_bus`/`write_bus` are the dispatch point if you need to add or debug a register. The renderer implements modes 0-7 (mode 7 with EXTBG), 8x8/16x16 tiles per BGMODE bits 4-7, hi-res modes 5/6 (real 512-dot sampling with 16-wide tiles, collapsed onto the fixed 256x224 raster by dot-pair averaging; SETINI interlace averages the two field lines the same way), windowing, mosaic, color math, and direct color. Timing is master-clock-based: the bus bills every access its real per-region cost (6/8/12 master cycles, FastROM-aware per MEMSEL), the frontend step loop advances the machine via `take_step_access_costs`/`tick_master` (4 master cycles per PPU dot), and DMA/HDMA charge the hardware rate of 8 master cycles per byte. `io.rs`'s `IoRegisters` is a separate, unused, and known-incorrect register-map module (not called by `SystemBus`) — don't confuse it with the real dispatch logic in `bus.rs`.

`lib.rs` re-exports the public API as a flat set of types (`Cpu`, `SystemBus`, `MemoryBus`, `Ppu`, `Apu`, `Dma`, `Cartridge`, etc.) — there's no facade struct that owns and steps the whole system; callers compose these pieces themselves (see `oxidesfc-frontend`'s `Snes` struct for the reference composition).

### oxidesfc-frontend: composes the core end-to-end

`src-tauri/src/emulation/controller.rs` defines a private `Snes` struct holding just a `Cpu`, a `SystemBus`, and a `halted` field (no separate `Cartridge` — cartridge access goes through `bus.cartridge_ref()`). `Snes::step()` calls `cpu.step(&mut bus)`, then ticks the PPU/APU (`bus.tick_ppu`/`tick_apu`) and dispatches pending NMI/IRQ; `get_frame()` returns a real rendered frame from `bus.render_frame()`, and `get_audio_samples()` drains real synthesized DSP samples from the APU. A real SMW ROM boots, executes, and renders a visible frame end-to-end (see `oxidesfc-frontend/src-tauri/src/emulation/controller.rs`'s `real_rom_tests` module, which exercises this against an actual ROM file). `EmulationController` (same file) wraps `Snes` with start/pause/resume/stop state; save/load state does real file I/O around the save slot, and the payload is the core's versioned binary snapshot (`oxidesfc_core::save_snapshot`/`load_snapshot`, format in `oxidesfc-core/src/state.rs`) covering CPU + WRAM + PPU memory/registers + DMA + the complete APU (RAM, SPC700 with timers, DSP register file AND all transient synthesis state — per-voice envelopes, BRR cursors, echo ring, FIR window — so a restored state resumes mid-note) + cartridge SRAM. The ROM itself is not serialized — a state only loads onto the same cartridge.

Tauri command handlers live in `src-tauri/src/commands/{emulation,library,settings}.rs` and are registered in `src-tauri/src/lib.rs`'s `invoke_handler!` list — adding a new command requires both writing the `#[tauri::command]` fn and adding it to that list. Several TS-side UI features (folder-based collections, favorites, cover art, cheats-enable toggle, screenshot) currently call `invoke()` with command names that have no matching Rust command — check `lib.rs`'s `invoke_handler!` list before assuming a frontend call site has a working backend. `AppState` (also in `lib.rs`) holds `Mutex<EmulationController>`, `Mutex<InputManager>`, and `library_lock: Mutex<()>` (guarding `library.json` read-modify-write) shared across all commands.

### Frontend module boundaries (TypeScript)

The React side under `src/` follows a layered structure (loosely matching `plans/oxidesfc-frontend-specification.md` section 2.2):
- `components/` — React UI grouped by feature (`library/`, `settings/`, `emulator/`, `cheats/`, `wizard/`, `common/`).
- `stores/` — Zustand stores (`emulationStore.ts`, `libraryStore.ts`, `settingsStore.ts`); these call `invoke()` from `@tauri-apps/api/core` directly to reach Tauri commands. `emulationStore.ts` is the only emulation-wiring path — an earlier parallel `EmulationService`/`TauriEmulationCore`/`useEmulationLoop` stack under `infrastructure/emulation/` and `hooks/` was dead code with no callers and has been removed.
- `infrastructure/` — adapters to the outside world: `filesystem/`, `network/` (IGDB and Screenscraper metadata clients), `input/`.
- `services/` — cross-cutting logic not tied to a single store: audio playback, WebGL rendering (`renderer/WebGLRenderer.ts`, `ShaderService.ts`), hotkeys, controller profiles, an event bus.
- `domain/types.ts` — shared TS types for games/ROMs independent of any particular store or service.

When adding a feature that touches both Rust and TS, the typical chain is: Tauri command (`src-tauri/src/commands/*.rs`) → registered in `src-tauri/src/lib.rs` → called via `invoke()` in an `infrastructure/` adapter or directly in a `stores/*.ts` store → consumed by a component in `components/`.

### Input handling

Gamepad input on the Rust side uses `gilrs` only, via `InputManager` in `src-tauri/src/input/gamepad.rs`; the `poll_gamepad_events` Tauri command locks `AppState.input_manager` and returns real polled events. The `windows` crate's `Gaming_Input` feature is declared as a dependency (`[target.'cfg(windows)'.dependencies]` in `oxidesfc-frontend/Cargo.toml`) but is not referenced anywhere in `src-tauri/src` — it's an unused dependency, not an active backend. `src-tauri/src/input/keyboard.rs` defines a standalone `KeyboardState` struct that is never constructed or wired into `InputManager`; it's dead code (real keyboard handling happens entirely in the frontend via DOM key events, per the module's own comment).

### Logging and crash reports

`src-tauri/src/lib.rs`'s `init_logging()` writes daily-rolling logs via `tracing-appender` to `%DATA_DIR%/OxideSFC/logs/` (Windows: `%APPDATA%\OxideSFC\logs`) in addition to stdout, and returns a `WorkerGuard` that `run()` holds for its entire lifetime — dropping that guard early stops the background log-writer thread, so don't refactor `init_logging()`'s call site without keeping the returned guard alive for as long as logging is needed. `init_panic_handler()` installs a panic hook that writes timestamped crash reports (including a backtrace) to `%DATA_DIR%/OxideSFC/crashes/`. Both run before the Tauri builder starts in `run()`.
