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
- **PPU** — background modes 0–7, including Mode 7 with EXTBG; 8×8 and 16×16 tiles; hi-res modes 5/6 (real 512-dot sampling collapsed by dot-pair averaging) and pseudo-hires; overscan (vblank/NMI at line 239); sprites with the hardware's per-scanline 32-sprite/34-tile limits, priority rotation, and STAT77 range/time-over flags; offset-per-tile (modes 2/4); windowing; mosaic; color math; direct color; VRAM read prefetch buffer, VMAIN address remapping, and active-display VRAM write blocking; the two separate PPU1/PPU2 open-bus registers.
- **DMA / HDMA** — all 8 channels with real immediate-DMA and per-scanline HDMA execution, hardware setup/sync overheads, per-byte machine ticking (NMI/IRQ/HDMA fire mid-transfer at their real positions), and the same-channel HDMA-kills-DMA conflict.
- **Timing** — master-clock based: every bus access is billed its real per-region cost (6/8/12 master cycles, FastROM-aware via MEMSEL), 4 master cycles per PPU dot, 8 per DMA byte, 40-cycle WRAM refresh stalls per scanline, and dot-exact H/V timer IRQs.
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

## Acknowledgments

OxideSFC is an **original emulator implementation written from scratch in Rust — it is not a port** of any existing emulator. Every subsystem was written here, with two narrow exceptions that are fixed hardware data rather than logic and are called out explicitly at the end of this section.

That said, several hardware-accuracy improvements were implemented using [snes9x](https://github.com/snes9xgit/snes9x) (C++, snes9x team) as a *behavioral reference*: its source code documents, in working form, many S-CPU/PPU details that took the snes9x developers years of hardware research to get right. Areas where snes9x's behavior was studied and re-implemented independently in Rust include: NMI/IRQ timer edge cases (H/V timer trigger positions, NMI-enable-during-vblank), CPU and PPU open-bus behavior (including the two separate PPU MDRs), WRAM refresh stalls, DMA/HDMA timing overheads and frame scheduling, the VRAM read prefetch buffer and VMAIN address remapping, OAM write latching and sprite priority rotation, per-scanline sprite limits (STAT77 range/time-over), and offset-per-tile.

The same approach was later taken with [bsnes](https://github.com/bsnes-emu/bsnes) (C++, byuu/Near and the bsnes-emu maintainers), whose accuracy is the reference the audio and video work was measured against. Behavior studied there and re-implemented independently in Rust includes: the DSP's noise LFSR and how NON substitutes it for a voice's BRR source, pitch modulation (PMON), per-voice 16-bit accumulator saturation, the ADSR envelope's rate/offset gating tables, mode 7's coordinate clipping, direct-color's bit layout, the color-math rules that decide when the half-result is skipped and what the sub screen's backdrop is (`PPU::Line::pixel`), and the header byte → video standard table that tells a PAL cartridge from an NTSC one (`SuperFamicom::videoRegion`). The ±0.5% dynamic-rate-control approach in the audio worklet is the technique bsnes, snes9x and RetroArch all converged on.

Two pieces are not re-implementations but direct transcriptions, because they are fixed hardware data rather than logic: the DSP's 512-entry gaussian interpolation table, and the exact shift/filter arithmetic of the BRR block decoder. Both come from the `SPC_DSP` reference implementation that originates with blargg's `snes_spc` and ships in bsnes — the same decoder reused across most SNES emulators. **If you redistribute OxideSFC, check that its license is compatible with those sources** (bsnes is GPLv3, `snes_spc` is LGPL); this repository currently declares MIT in `oxidesfc-frontend/Cargo.toml` and ships no `LICENSE` file, which is a gap that needs resolving before any binary release is distributed under those terms.

Many thanks to the snes9x and bsnes teams, and to the SNES documentation community (fullsnes, anomie's docs, the SNESdev wiki) whose work makes accurate emulation possible.

## Legal

This project does not include, and will never include, any copyrighted ROM images, BIOS files, or game assets. You must supply your own legally obtained cartridge dumps. The `.gitignore` deliberately excludes ROM and save formats from version control.

## Contributing

Direct pushes to `main` are blocked — all changes go through pull requests.
