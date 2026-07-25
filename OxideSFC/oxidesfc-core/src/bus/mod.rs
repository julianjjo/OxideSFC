//! The system bus: the machine's memory map and the owner of every
//! component behind it.
//!
//! `SystemBus` holds the real `Ppu`/`Apu`/`Dma`/`Wram`/`Cartridge` and
//! dispatches the memory-mapped register space to them, so a CPU write to
//! (say) $2107 lands in actual PPU state rather than a scratch array.
//!
//! Layout:
//! - `core` -- construction and the component accessors.
//! - `read` / `write` -- the address dispatch, one `match` each, which is
//!   where a new or misbehaving register is added or debugged.
//! - `ports` -- the VRAM/CGRAM/OAM access ports and their address latches.
//! - `transfer` -- DMA and per-scanline HDMA execution.
//! - `timing` -- per-access master-cycle costs and the per-dot/per-scanline
//!   work those cycles drive (HDMA, NMI/IRQ edges, joypad latching, and the
//!   per-line register/palette snapshots the renderer draws from).
//! - `state` -- save-state serialization.

mod core;
mod ports;
mod read;
mod state;
mod timing;
mod transfer;
mod write;

#[cfg(test)]
mod tests;

use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::cgram::Cgram;
use crate::dma::Dma;
use crate::error::EmulationError;
use crate::ppu::{Ppu, PpuRegisters};
use crate::wram::Wram;

pub type BusResult<T> = Result<T, EmulationError>;

pub trait MemoryBus {
    fn read_u8(&mut self, addr: u32) -> BusResult<u8>;
    fn write_u8(&mut self, addr: u32, value: u8) -> BusResult<()>;

    fn read_u16(&mut self, addr: u32) -> BusResult<u16> {
        let lo = self.read_u8(addr)? as u16;
        let hi = self.read_u8(addr.wrapping_add(1))? as u16;
        Ok((hi << 8) | lo)
    }
}

/// SystemBus - Main bus that routes memory accesses to the correct components
/// Implements the SNES memory map:
/// - Banks $00-$3F, $40-$7F, $80-$BF: Lower memory (WRAM mirrors, I/O, ROM)
/// - Banks $C0-$FF: Cartridge ROM
/// - $7E0000-$7FFFFF: WRAM (128KB)
/// - $7F0000-$7FFFFF: WRAM mirrors
/// - $2140-$217F: APU communication ports (4 ports, mirrored every 4 bytes)
pub struct SystemBus {
    wram: Wram,
    cartridge: Option<Cartridge>,
    apu: Apu,
    ppu: Ppu,
    /// Open-bus: last byte read from the bus
    open_bus: u8,
    /// Mirrors $4200 (NMITIMEN) bit 7: NMI generation enabled.
    nmi_enable: bool,
    /// Mirrors $4210 (RDNMI) bit 7: latched on vblank entry, cleared when
    /// the CPU reads $4210.
    nmi_status_flag: bool,
    /// Set on the vblank-entry edge iff NMI was enabled at that moment;
    /// consumed (and cleared) by `take_pending_nmi()` once per occurrence
    /// so the CPU stepping loop fires exactly one NMI per frame instead of
    /// re-triggering on every subsequent poll while still in vblank.
    nmi_pending: bool,
    /// Previous tick's `ppu.in_vblank()`, used to detect the rising edge
    /// (entering vblank) rather than firing continuously while in vblank.
    was_in_vblank: bool,
    /// Previous tick's `ppu.in_hblank()`, used to detect the per-scanline
    /// rising edge (entering hblank) that drives one HDMA transfer step,
    /// the same edge-detection idiom `was_in_vblank` uses for NMI.
    was_in_hblank: bool,
    /// DMA channel register storage ($4300-$437F). Transfer execution is
    /// driven by `execute_dma_channel` below, not by this struct -- see
    /// its doc comment for why.
    dma: Dma,
    /// Mirrors $420C (HDMAEN): which channels have HDMA armed for the
    /// current frame. Drives `hdma_init`/`hdma_run_scanline`, called from
    /// `tick_ppu` on the vblank-exit and per-scanline hblank-entry edges.
    hdma_enable_mask: u8,
    /// $2115 VMAIN: VRAM address increment control.
    vmain: u8,
    /// $2116/$2117 VMADD: current VRAM word address.
    vmadd: u16,
    /// $2121 CGADD: current CGRAM byte-pair (color) index.
    cgadd: u8,
    /// Toggles low/high byte on each $2122 write; reset by writing $2121.
    cgram_high: bool,
    /// Current (live) OAM byte-pair (word) address -- advances as $2104
    /// writes / $2138 reads consume bytes.
    oamadd: u16,
    /// The reload value software last wrote via $2102/$2103. Real hardware
    /// reloads the live OAM address from this latch at the START of every
    /// vblank (unless in forced blank) -- games like DKC set OAMADD once
    /// and rely entirely on that per-frame auto-reset before their vblank
    /// OAM DMA. Without modeling it, the live address marched off the end
    /// of OAM by +0x110 words every frame and every subsequent sprite
    /// upload landed wrapped at garbage offsets (DKC gameplay rendered
    /// with no sprites at all: DK, enemies, bananas all invisible).
    oamadd_latch: u16,
    /// Toggles low/high byte on each $2104 write; reset by writing
    /// $2102/$2103 and by the vblank reload.
    oam_high: bool,
    /// Background/sprite rendering register state ($2100, $2101, $2105,
    /// $2107-$2114, $212C) -- see `render_frame`.
    ppu_regs: PpuRegisters,
    /// Live controller-1 button state, in the SNES's own auto-read bit
    /// layout (bit15=B,14=Y,13=Select,12=Start,11=Up,10=Down,9=Left,
    /// 8=Right,7=A,6=X,5=L,4=R,3-0=unused), set by the frontend via
    /// `set_joypad1_state` whenever input changes.
    joypad1_state: u16,
    /// Mirrors $4200 (NMITIMEN) bit 0: auto-joypad-read enable.
    auto_joypad_read_enable: bool,
    /// Snapshot of `joypad1_state` taken on the vblank-entry edge when
    /// auto-read is enabled -- what $4218/$4219 actually report, matching
    /// real hardware's "latched once per frame at vblank" timing rather
    /// than exposing the live, still-changing state mid-frame.
    joy1_auto: u16,
    /// Current $4016 bit0 strobe line state.
    joypad_strobe: bool,
    /// Snapshot of `joypad1_state` latched while the strobe line is high,
    /// shifted out one bit at a time (MSB/B first) by manual $4016 reads.
    joy1_shift: u16,
    /// How many bits have been shifted out of `joy1_shift` since strobe
    /// last went low. Reads past 16 report 1 (no more data), matching a
    /// standard controller with no multitap chained behind it.
    joy1_bits_read: u8,
    /// True once the game has written to $4016 at least once. Before that,
    /// reads always report 0 (matching the previous, deliberately safe
    /// "no buttons pressed" stub) regardless of how many times $4016 has
    /// been read -- code that polls $4016 without ever asserting the
    /// strobe line is not actually using the serial-read protocol (e.g. an
    /// incidental/defensive probe during boot), and letting the 16-bit
    /// shift counter advance on those reads anyway caused it to start
    /// returning 1 after the 16th read even though no real strobe cycle
    /// ever happened, which changed a boot-time polling loop's outcome
    /// (regression: CPU coverage collapsed from >55,000 to ~7,300 distinct
    /// PCs over a 5M-step run).
    joy1_ever_strobed: bool,
    /// Live controller-2 button state, same bit layout as `joypad1_state`.
    joypad2_state: u16,
    /// Controller 2's vblank auto-read snapshot ($421A/$421B), latched on
    /// the same edge as `joy1_auto`.
    joy2_auto: u16,
    /// Controller 2's serial-read shift snapshot, latched by the same
    /// $4016 strobe falling edge as `joy1_shift` (the strobe line is
    /// shared by both controller ports on real hardware).
    joy2_shift: u16,
    /// Bits shifted out of `joy2_shift` so far -- see `joy1_bits_read`.
    joy2_bits_read: u8,
    /// Mirrors $4200 (NMITIMEN) bit 4: H-timer IRQ enable.
    irq_h_enable: bool,
    /// Mirrors $4200 (NMITIMEN) bit 5: V-timer IRQ enable. SMW arms this
    /// every in-level frame (with VTIME = the status-bar boundary line)
    /// for its raster split -- the IRQ handler rewrites BG3 scroll etc.
    /// mid-frame.
    irq_v_enable: bool,
    /// $4207/$4208 HTIME (9-bit). Stored but only modeled at scanline
    /// granularity -- an H+V IRQ fires at the start of the matching
    /// scanline rather than at the exact dot.
    htime: u16,
    /// $4209/$420A VTIME (9-bit): the scanline the V-timer IRQ fires on.
    vtime: u16,
    /// The level-triggered IRQ line, doubling as $4211 (TIMEUP) bit 7:
    /// set when the timer condition matches, held until software
    /// acknowledges by reading $4211 (or disables both timer enables via
    /// $4200). `Cpu::irq` dispatch is gated on this via `irq_pending()`.
    irq_line: bool,
    /// Previous tick's scanline, to detect scanline boundaries crossed by
    /// `tick_ppu` (for timer-IRQ matching and per-scanline register
    /// snapshots) even when one tick spans multiple lines.
    last_scanline: u16,
    /// Snapshot of `ppu_regs` taken at the start of each visible scanline
    /// (0-223). This is what `render_frame` actually renders from, so
    /// mid-frame register changes -- SMW's IRQ status-bar split, HDMA
    /// scroll/color gradients -- show up on the correct rows instead of
    /// the whole frame being painted with one end-of-frame register state.
    scanline_regs: Vec<PpuRegisters>,
    /// Per-scanline CGRAM snapshots, captured at the same instant as
    /// `scanline_regs`. Games rewrite palette entries mid-frame via HDMA
    /// to $2121/$2122 (Prince of Persia 2 repaints backdrop color 0 every
    /// line for its sky gradient, then restores it during vblank), so
    /// rendering from one end-of-frame CGRAM would paint every line with
    /// the vblank palette. Not serialized in save states -- reseeded from
    /// the live CGRAM on load, like `scanline_regs`.
    scanline_cgram: Vec<Cgram>,
    /// $4202 WRMPYA: 8-bit multiplicand for the hardware multiplier.
    wrmpya: u8,
    /// $4204/$4205 WRDIVL/WRDIVH: 16-bit dividend for the hardware divider.
    wrdiv: u16,
    /// $4214/$4215 RDDIVL/RDDIVH: division quotient. Real hardware takes
    /// 16 CPU cycles to produce this; here it's available immediately
    /// after the $4206 write (same honest simplification as immediate DMA).
    rddiv: u16,
    /// $4216/$4217 RDMPYL/RDMPYH: multiplication product, or division
    /// remainder after a $4206 write. Ready immediately (real hardware:
    /// 8 cycles for multiply).
    rdmpy: u16,
    /// $4201 WRIO: programmable I/O port output latch, read back at $4213
    /// (RDIO). A falling edge on bit 7 latches the PPU H/V counters, the
    /// same latch $2137 (SLHV) triggers.
    wrio: u8,
    /// $420D MEMSEL bit 0: FastROM waitstate select, consumed by
    /// `access_master_cycles` (upper-bank ROM reads cost 6 master cycles
    /// instead of 8 while set).
    memsel: u8,
    /// $2181-$2183 WMADD: 17-bit WRAM address for the $2180 sequential
    /// data port, auto-incremented by every $2180 read/write.
    wmadd: u32,
    /// $2134-$2136 MPY: the PPU's mode-7 multiplier result, recomputed on
    /// every $211C (M7B) byte write as M7A (signed 16) * that byte
    /// (signed 8). Games use this as a free 16x8 signed multiplier during
    /// non-mode-7 rendering too.
    mpy: i32,
    /// H/V counter latch ($2137 SLHV or a WRIO bit-7 falling edge copies
    /// the live dot/scanline counters here; $213C/$213D read them out).
    ophct: u16,
    opvct: u16,
    /// Per-counter double-read toggles (low byte first, then the 9th bit);
    /// both reset by reading $213F (STAT78).
    ophct_high: bool,
    opvct_high: bool,
    /// STAT78 bit 6: set when the counters have been latched since the
    /// last $213F read.
    counter_latched: bool,
    /// PPU1's "MDR" (open-bus register): the last byte PPU1 drove onto the
    /// B-bus. Real hardware has TWO separate PPU open-bus registers --
    /// PPU1's is returned by reads of $2134-$2136/$2138-$213A/$213E and of
    /// the write-only PPU1 registers, PPU2's fills the unused bits of
    /// $213B-$213D/$213F. Mirrors snes9x's `PPU.OpenBus1`/`PPU.OpenBus2`
    /// (ppu.cpp `S9xGetPPU`).
    ppu1_mdr: u8,
    /// PPU2's "MDR" -- see `ppu1_mdr`.
    ppu2_mdr: u8,
    /// $2139/$213A VRAM read prefetch buffer. Real hardware returns THIS
    /// word on data-port reads and only reloads it (from the pre-increment
    /// address) when the read phase matches VMAIN's increment phase --
    /// which is why games must issue a dummy read after setting $2116/17.
    /// Mirrors snes9x's `PPU.VRAMReadBuffer`/`S9xUpdateVRAMReadBuffer`.
    vram_prefetch: u16,
    /// OAM low-table (bytes $000-$1FF) write latch: the even byte of each
    /// word is held here and only committed together with the odd-byte
    /// write (real hardware writes the low table word-at-a-time; the high
    /// table at $200+ writes byte-at-a-time with no latch).
    oam_lsb_latch: u8,
    /// $2103 bit 7: OAM priority rotation -- when set, sprite priority
    /// evaluation starts at FirstSprite = (OAMADD & $FE) >> 1 instead of
    /// sprite 0 (snes9x ppu.cpp $2102/$2103 handlers).
    oam_priority_rotation: bool,
    /// STAT77 ($213E) bits 6-7: sprite range-over (>32 sprites on a line)
    /// and time-over (>34 tiles on a line), computed by `render_frame`'s
    /// per-line sprite evaluation and cleared at the start of each frame
    /// (the same lifecycle as snes9x's `PPU.RangeTimeOver`).
    range_time_over: u8,
    /// Whether the current scanline's WRAM refresh stall (40 master
    /// cycles at ~dot 134, snes9x `SNES_WRAM_REFRESH_CYCLES` at
    /// `WRAMRefreshPos` 538 master cycles) has been charged yet.
    refresh_charged_this_line: bool,
    /// The dot position within the current scanline reached by the
    /// previous `tick_ppu_dots` call -- lets the H/V timer IRQ detect the
    /// exact dot crossing instead of firing at scanline granularity.
    last_h_dot: u16,
    /// Channels HDMA has touched since a general DMA last checked: when
    /// a per-line HDMA fires mid-transfer on the SAME channel a $420B
    /// DMA is using, the DMA is killed immediately and its $43x2/$43x5
    /// stop updating (snes9x `CPU.HDMARanInDMA`, dma.cpp). Ephemeral
    /// within one $420B write -- not serialized.
    hdma_ran_channels: u8,
    /// Bus accesses made since the last `take_step_access_costs` call,
    /// and their summed master-cycle cost per the real per-region access
    /// speeds (6/8/12 master cycles, FastROM-aware -- see
    /// `access_master_cycles`). The CPU stepping loop drains this after
    /// each instruction to advance the machine by true master-clock time
    /// instead of a flat cycles-times-constant approximation.
    step_access_count: u32,
    step_access_master: u32,
    /// Nesting counter: while nonzero, read_bus/write_bus skip access-cost
    /// accounting. Raised around DMA/HDMA engine transfers, whose bus
    /// traffic is billed by the engine itself at the hardware rate (8
    /// master cycles/byte), not as CPU accesses.
    accounting_suspended: u32,
    /// Master cycles not yet converted into whole PPU dots (1 dot = 4
    /// master cycles) / whole APU base cycles (1 = 8 master cycles) by
    /// `tick_master`.
    dot_master_remainder: u32,
    apu_master_remainder: u32,
}

/// Which B-bus offsets (relative to BBADx) successive transferred bytes
/// cycle through for a given DMAPx transfer-unit mode (0-7). Shared by
/// immediate DMA (`execute_dma_channel`) and per-scanline HDMA
/// (`hdma_transfer_one_line`) -- the same hardware-defined mode table
/// applies to both (e.g. mode 1 alternates BBADx/BBADx+1 for register
/// pairs like $2118/$2119; verified against wiki.superfamicom.org/DMA).
fn bbus_pattern_for_mode(mode: u8) -> &'static [u8] {
    match mode {
        0 => &[0],
        1 => &[0, 1],
        2 => &[0, 0],
        3 => &[0, 0, 1, 1],
        4 => &[0, 1, 2, 3],
        5 => &[0, 1, 0, 1],
        6 => &[0, 0],
        7 => &[0, 0, 1, 1],
        _ => &[0],
    }
}

impl Default for SystemBus {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryBus for SystemBus {
    fn read_u8(&mut self, addr: u32) -> BusResult<u8> {
        self.read_bus(addr)
    }

    fn write_u8(&mut self, addr: u32, value: u8) -> BusResult<()> {
        self.write_bus(addr, value)
    }
}

