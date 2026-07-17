# OxideSFC

A Super Nintendo (SNES) emulator written in Rust, with a lightweight desktop frontend built on Tauri v2 + React.

The emulation core boots and runs real commercial ROMs end to end — a real Super Mario World cartridge image loads, executes, renders video, and synthesizes audio through the full CPU → PPU → APU pipeline.

> **Work in progress.** Accuracy and compatibility are improving continuously; expect rough edges.

## Repository layout

```
OxideSFC/               Cargo workspace (run all cargo commands from here)
├── oxidesfc-core/      SNES emulation core — no UI or I/O dependencies
└── oxidesfc-frontend/  Tauri v2 + React/TypeScript desktop shell
plans/                  Design & status documents (aspirational/historical, not ground truth)
```

`oxidesfc-frontend` is both a Rust crate (`src-tauri/`) and an npm package (`src/`) living in the same directory.

## Emulation core (`oxidesfc-core`)

- **CPU** — 65C816 with all 256 opcodes implemented (the dispatch match is compiler-checked for exhaustiveness), NMI/IRQ handling, and hardware multiply/divide registers.
- **APU** — SPC700 with all 256 opcodes, timers, and a full S-DSP synthesizer: BRR sample decoding, ADSR/GAIN envelopes, pitch modulation, echo with FIR filter, and noise.
- **PPU** — background modes 0–7, including Mode 7 with EXTBG; 8×8 and 16×16 tiles; hi-res modes 5/6 (real 512-dot sampling collapsed by dot-pair averaging); sprites; windowing; mosaic; color math; direct color.
- **DMA / HDMA** — all 8 channels with real immediate-DMA and per-scanline HDMA execution.
- **Timing** — master-clock based: every bus access is billed its real per-region cost (6/8/12 master cycles, FastROM-aware via MEMSEL), 4 master cycles per PPU dot, 8 per DMA byte.
- **Save states** — versioned binary snapshots covering CPU, WRAM, PPU memory/registers, DMA, the complete APU (including transient synthesis state, so a restored state resumes mid-note), and cartridge SRAM.

## Frontend (`oxidesfc-frontend`)

- ROM library with metadata, filtering, and collections
- Video output via WebGL with a shader pipeline; live emulation-speed control with wall-clock frame pacing
- Real-time audio playback of the synthesized DSP output
- Save/load state slots backed by the core's snapshot format
- Gamepad support (via `gilrs`) and keyboard input
- Built with Tauri v2, React 18, Zustand, and Tailwind CSS — a native window, not an Electron bundle; the release profile is tuned for small binary size

## Building

Prerequisites:

- [Rust](https://rustup.rs/) (stable toolchain)
- [Node.js](https://nodejs.org/) 18+
- [Tauri v2 prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform (WebView2 on Windows, webkit2gtk on Linux, etc.)

Core and workspace:

```bash
cd OxideSFC
cargo build                    # build both workspace members
cargo build -p oxidesfc-core   # build just the emulation core
```

Desktop app:

```bash
cd OxideSFC/oxidesfc-frontend
npm install
cargo tauri dev                # full app: vite dev server + native window
cargo tauri build              # release bundle
```

## Testing

```bash
cd OxideSFC
cargo test                     # run all tests in the workspace
cargo test -p oxidesfc-core    # core tests only
```

Unit tests cover the CPU, SPC700, PPU, DMA, and bus in isolation (every opcode of both processors is pinned by tests). In addition, end-to-end tests boot a real ROM through the full system; these expect `Super Mario World (U) [!].smc` at the repository root and **fail loudly if it's missing** — the ROM is not included (see below), so skip those tests or provide your own legally obtained copy.

## Legal

This project does not include, and will never include, any copyrighted ROM images, BIOS files, or game assets. You must supply your own legally obtained cartridge dumps. The `.gitignore` deliberately excludes ROM and save formats from version control.

## Contributing

Direct pushes to `main` are blocked — all changes go through pull requests.
