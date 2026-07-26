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

The core crate models each SNES subsystem as an independent module. The four big ones are directory modules split by responsibility; the rest are single files (`vram.rs`, `cgram.rs`, `wram.rs`, `state.rs`, `error.rs`, plus `oam/`, `dma/` and `cartridge/`, which are directories only to keep their tests separate):

- **`cpu/`** — one `impl Cpu` spread across files by instruction family: `core` (step loop, memory access, register-width and flag bookkeeping), `dispatch` (the 256-arm opcode match), `addressing`, then `load_store`, `add_sub`, `compare`, `incdec`, `logic`, `bittest`, `shift`, `branch`, `stack`, `transfer`, `flags`, `interrupts`.
- **`apu/`** — `spc700` plus its `opcodes` dispatch, `dsp`, `voice`, `brr`, `envelope`, and `mod.rs`'s `Apu` (the part the main CPU sees, which converts bus pacing cycles into SPC700 cycles and DSP samples).
- **`bus/`** — `core` (construction + component accessors), `read`/`write` (the address dispatch), `ports` (VRAM/CGRAM/OAM access ports), `transfer` (DMA/HDMA), `timing` (per-access costs and the per-dot/per-scanline work they drive), `state`.
- **`renderer/`** — `compose` (public entry points + the main/sub screen compositing pass) calling into `background`, `sprites`, `mode7`, plus shared `window`, `color`, `tile`.
- **`ppu/`** — `mod.rs` owns VRAM/CGRAM/OAM and the scanline counters; `registers.rs` holds `PpuRegisters` as separate data because the renderer needs a per-scanline *copy* of it.

`apu/opcodes.rs`, `cpu/dispatch.rs` and `bus/{read,write}.rs` are the long files (500-900 lines) and stay that way on purpose: each is a single exhaustive `match`. Both opcode dispatches have no wildcard arm, so the compiler proves all 256 encodings are handled (the SPC700 side is additionally pinned by `every_spc700_opcode_executes_without_halting_except_stop`); splitting the arms into per-range helpers would give each an unreachable catch-all and lose that.

**`SystemBus` owns real `Apu`/`Ppu`/`Dma` instances and fully dispatches the memory-mapped register space to them**: PPU registers ($2100-$213F, including Mode 7 $211A-$2120 with the MPY multiplier at $2134-$2136, the H/V counter latches $2137/$213C-$213D, and STAT77/78), the WRAM data port ($2180-$2183), APU communication ports ($2140-$217F), NMITIMEN/WRIO/hardware-multiply-divide/HDMAEN/MEMSEL and both joypads ($4200-$421B), and DMA channel registers ($4300-$437F) all route to real component state, including real immediate-DMA and HDMA execution, NMI/IRQ dispatch back into the CPU (via vblank/hblank edge-detection), and auto-joypad-read latching. `bus/read.rs`'s `read_bus` and `bus/write.rs`'s `write_bus` are the dispatch point if you need to add or debug a register.

The renderer implements modes 0-7 (mode 7 with EXTBG), 8x8/16x16 tiles per BGMODE bits 4-7, hi-res modes 5/6 (real 512-dot sampling with 16-wide tiles, collapsed onto the fixed 256x224 raster by dot-pair averaging; SETINI interlace averages the two field lines the same way), windowing, mosaic, color math, and direct color. Timing is master-clock-based: the bus bills every access its real per-region cost (6/8/12 master cycles, FastROM-aware per MEMSEL), the frontend step loop advances the machine via `take_step_access_costs`/`tick_master` (4 master cycles per PPU dot), and DMA/HDMA charge the hardware rate of 8 master cycles per byte. Cartridge region selects NTSC vs PAL through `SystemBus::set_video_mode`, which keeps the PPU's line count and the APU's master clock consistent — use it rather than `ppu_mut().set_mode()`.

`lib.rs` re-exports the public API as a flat set of types (`Cpu`, `SystemBus`, `MemoryBus`, `Ppu`, `Apu`, `Dma`, `Cartridge`, etc.) — there's no facade struct that owns and steps the whole system; callers compose these pieces themselves (see `oxidesfc-frontend`'s `Snes` struct for the reference composition).

### oxidesfc-frontend: composes the core end-to-end

`src-tauri/src/emulation/snes.rs` defines a module-private `Snes` struct holding just a `Cpu`, a `SystemBus`, and a `halted` field (no separate `Cartridge` — cartridge access goes through `bus.cartridge_ref()`). `Snes::step()` calls `cpu.step(&mut bus)`, then ticks the PPU/APU (`bus.tick_ppu`/`tick_apu`) and dispatches pending NMI/IRQ; `get_frame()` returns a real rendered frame from `bus.render_frame()`, and `get_audio_samples()` drains real synthesized DSP samples from the APU. A real SMW ROM boots, executes, and renders a visible frame end-to-end (see `emulation/real_rom_tests.rs`, which exercises this against actual ROM files; `dump_real_rom_frames` there is an `#[ignore]`d diagnostic that writes raw RGBA frames for visual comparison against a reference emulator). `EmulationController` (`emulation/controller.rs`) wraps `Snes` with start/pause/resume/stop state and wall-clock frame pacing; save/load state does real file I/O around the save slot, and the payload is the core's versioned binary snapshot (`oxidesfc_core::save_snapshot`/`load_snapshot`, format in `oxidesfc-core/src/state.rs`) covering CPU + WRAM + PPU memory/registers + DMA + the complete APU (RAM, SPC700 with timers, DSP register file AND all transient synthesis state — per-voice envelopes, BRR cursors, echo ring, FIR window — so a restored state resumes mid-note) + cartridge SRAM. The ROM itself is not serialized — a state only loads onto the same cartridge.

Tauri command handlers live in `src-tauri/src/commands/` (`emulation.rs`, `settings.rs`, `folders.rs`, and `library/` — whose `scan` and `store` submodules do the filesystem and `library.json` work) and are registered in `src-tauri/src/lib.rs`'s `invoke_handler!` list — adding a new command requires both writing the `#[tauri::command]` fn and adding it to that list. Note that `invoke_handler!` resolves a companion macro from the command's own module path, so a `#[tauri::command]` fn must live directly in the module named there, not in a submodule of it. Several TS-side UI features (folder-based collections, favorites, cover art, cheats-enable toggle, screenshot) currently call `invoke()` with command names that have no matching Rust command — check `lib.rs`'s `invoke_handler!` list before assuming a frontend call site has a working backend. `AppState` (also in `lib.rs`) holds `Mutex<EmulationController>`, `Mutex<InputManager>`, and `library_lock: Mutex<()>` (guarding `library.json` read-modify-write) shared across all commands.

### Frontend module boundaries (TypeScript)

The React side under `src/` follows a layered structure (loosely matching `plans/oxidesfc-frontend-specification.md` section 2.2):
- `components/` — React UI grouped by feature (`library/`, `settings/`, `emulator/`, `cheats/`, `wizard/`, `common/`).
- `stores/` — Zustand stores (`emulationStore.ts`, `libraryStore.ts`, `settingsStore.ts`); these call `invoke()` from `@tauri-apps/api/core` directly to reach Tauri commands. `emulationStore.ts` is the only emulation-wiring path — an earlier parallel `EmulationService`/`TauriEmulationCore`/`useEmulationLoop` stack under `infrastructure/emulation/` and `hooks/` was dead code with no callers and has been removed.
- `infrastructure/` — adapters to the outside world: `filesystem/`, `network/` (IGDB and Screenscraper metadata clients), `input/`.
- `services/` — cross-cutting logic not tied to a single store: audio playback, WebGL rendering (`renderer/WebGLRenderer.ts`, `ShaderService.ts`), hotkeys, controller profiles, an event bus.
- `domain/` — logic and types independent of any store, service or component: `types.ts` (shared game/ROM types), `keyboardDefaults.ts` and `keyLabel.ts` (the key-code ↔ SNES-button vocabulary), `romFormat.ts` (how header values are rendered — sizes in **Mbit**, not MB), `cartTone.ts` (the library card's colour hash).
- `shell/` — the app frame: `NavRail.tsx`, the 60px icon rail `App.tsx` renders beside the active view.

When adding a feature that touches both Rust and TS, the typical chain is: Tauri command (`src-tauri/src/commands/*.rs`) → registered in `src-tauri/src/lib.rs` → called via `invoke()` in an `infrastructure/` adapter or directly in a `stores/*.ts` store → consumed by a component in `components/`.

### Styling: tokens, not theme branches

**No component may branch on the active theme in JavaScript.** All colour resolves through CSS custom properties declared in `src/styles/tokens.css` and keyed off two attributes on `<html>`:

- `data-theme="dark" | "light"` — surfaces and text. Dark is a warm charcoal (the shadow inside a cartridge slot); light is the Super Famicom's own warm-grey shell.
- `data-accent="red" | "yellow" | "green" | "blue"` — the interactive hue, named after the face button it borrows. Both themes define all four ramps, so all eight combinations work.

`src/theme.ts` owns `applyAppearance()`; `stores/settingsStore.ts` calls it on every load/save so appearance can never lag the persisted value. `tailwind.config.js` maps the tokens to utility names (`bg-panel`, `text-ink`, `text-dim`, `border-line`, `bg-accent-soft`, …) — there is deliberately **no** `dark:` variant, since that would be a second competing mechanism. `src/styles/index.css` holds the component primitives (`.panel`, `.btn`, `.field`, `.seg`, `.switch`, `.range`, `.cart`, `.rom-table`, `.rail`, plus the play deck's `.control-deck`/`.deck-*`).

Three typographic roles carry meaning and must not be used decoratively:

- **`.register`** — monospace microtext with tabular figures, for *recorded factual values*: both what the machine reports (`256×224`, `60 Hz`, `LoROM`, `8 Mbit`, `$2100`) and what the app has measured (playtime, session counts, buffer fill, timestamps). Never for prose, and never to label a control. The tell for whether a use belongs is the tabular figures: if the content isn't a value worth aligning digit-for-digit, it isn't a register.
- **`.eyebrow`** — heads a *section*, naming the hardware it governs where the section maps to hardware (`PPU / OUTPUT`, `S-DSP / APU`, `JOYPAD 1-2`).
- **`.microlabel`** — the same visual treatment as `.eyebrow` (mono micro caps, tracked wide) for captions and data labels, e.g. the `<dt>`s in the library's stat lists. It exists so `.eyebrow` keeps meaning "section heading": the two were conflated at first, and reusing `.eyebrow` as a data label erodes the one thing its name promises.

No webfont is bundled or fetched; the stacks resolve to Segoe UI Variable / Cascadia Mono on Windows.

### Settings screen

`components/settings/Settings.tsx` is only a shell: a panel list, a jump-to search index, and a scroll container. Every control lives in the panel that owns it (`VideoSettings`, `AudioSettings`, `ControllerSettings`, `LibrarySettings`, `GeneralSettings`), all built from the shared `SettingsSection`/`SettingRow`/`SettingNote` chrome. Adding a setting means editing one panel plus a row in `settingsIndex.ts` (whose `keywords` deliberately carry synonyms that do *not* appear in the visible label — that is the point of the field). `panels.ts` exists separately from the index so the index can name panel ids without importing React components.

Option lists in these panels must match what the code actually implements. `scale_mode` drives both texture filtering and the xBRZ/HQ2x shader selection in `WebGLRenderer.resolveShaderType()`; `shader` only drives its `crtMode` flag. Offering a value the renderer has no branch for silently does nothing.

### Cover art

`src-tauri/src/commands/covers.rs` resolves box art in two tiers, both matched on the ROM's **file name**: images already sitting beside the ROM (or in a `covers/`, `media/`, `boxart/` sibling folder), then the Libretro thumbnail CDN. Libretro is the online default specifically because it needs no credentials — ScreenScraper issues per-application developer IDs and IGDB requires a Twitch client *secret*, neither of which can ship in an open-source desktop binary. `infrastructure/network/{IGDBClient,ScreenscraperClient}.ts` remain unused, awaiting a user-credentialed tier that would match on ROM CRC32 instead of name.

Two details that are easy to get wrong:

- **Rendering needs the asset protocol.** A raw filesystem path in `<img src>` will not load in the webview. `tauri.conf.json` enables `assetProtocol` with its scope deliberately narrowed to `$DATA/OxideSFC/covers/*`, and `domain/coverArt.ts`'s `coverSrc()` runs paths through `convertFileSrc()`. Widening that scope would hand the webview read access to arbitrary files. Note `$DATA/OxideSFC` — not `$APPDATA`, which Tauri resolves to `Roaming/<identifier>` and would not match where the Rust side actually writes.
- **Names, not hashes.** Libretro is keyed on No-Intro release names, which real ROM files usually already are. `name_candidates()` tries the literal name first (so a correct set costs one request per game), then a GoodSNES reading — `[...]` dump flags stripped, `(U)` → `(USA)` — which is what lets `Super Mario World (U) [!].smc` find `Super Mario World (USA).png`. A renamed dump legitimately misses; misses are recorded as `<key>.miss` markers so they cost one request ever rather than one per launch. A failure to *reach* the CDN is never recorded as a miss.

Concurrency lives in `fetchCovers()` on the TypeScript side rather than in Rust: cancelling is then just "stop queueing", progress is exact without an event channel, and each `fetch_cover` call stays an independent, retryable unit.

### Input handling

Gamepad input on the Rust side uses `gilrs` only, via `InputManager` in `src-tauri/src/input/gamepad.rs`; the `poll_gamepad_events` Tauri command locks `AppState.input_manager` and returns real polled events. Keyboard handling is entirely frontend-side (DOM key events) — there is deliberately no Rust keyboard module.

### Logging and crash reports

`src-tauri/src/lib.rs`'s `init_logging()` writes daily-rolling logs via `tracing-appender` to `%DATA_DIR%/OxideSFC/logs/` (Windows: `%APPDATA%\OxideSFC\logs`) in addition to stdout, and returns a `WorkerGuard` that `run()` holds for its entire lifetime — dropping that guard early stops the background log-writer thread, so don't refactor `init_logging()`'s call site without keeping the returned guard alive for as long as logging is needed. `init_panic_handler()` installs a panic hook that writes timestamped crash reports (including a backtrace) to `%DATA_DIR%/OxideSFC/crashes/`. Both run before the Tauri builder starts in `run()`.
