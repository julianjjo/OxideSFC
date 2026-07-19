use crate::apu::Apu;
use crate::dma::Dma;
use crate::error::EmulationError;
use crate::cartridge::Cartridge;
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

impl SystemBus {
    pub fn new() -> Self {
        Self {
            wram: Wram::new(),
            cartridge: None,
            apu: Apu::new(),
            ppu: Ppu::new(),
            open_bus: 0x00,
            nmi_enable: false,
            nmi_status_flag: false,
            nmi_pending: false,
            was_in_vblank: false,
            was_in_hblank: false,
            dma: Dma::new(),
            hdma_enable_mask: 0,
            vmain: 0,
            vmadd: 0,
            cgadd: 0,
            cgram_high: false,
            oamadd: 0,
            oamadd_latch: 0,
            oam_high: false,
            ppu_regs: PpuRegisters::default(),
            joypad1_state: 0,
            auto_joypad_read_enable: false,
            joy1_auto: 0,
            joypad_strobe: false,
            joy1_shift: 0,
            joy1_bits_read: 0,
            joy1_ever_strobed: false,
            joypad2_state: 0,
            joy2_auto: 0,
            joy2_shift: 0,
            joy2_bits_read: 0,
            irq_h_enable: false,
            irq_v_enable: false,
            htime: 0x1FF,
            vtime: 0x1FF,
            irq_line: false,
            last_scanline: 0,
            scanline_regs: vec![PpuRegisters::default(); crate::renderer::SCREEN_HEIGHT],
            wrmpya: 0xFF,
            wrdiv: 0xFFFF,
            rddiv: 0,
            rdmpy: 0,
            wrio: 0xFF,
            memsel: 0,
            wmadd: 0,
            mpy: 1, // power-on: M7A/M7B latches read back as $01 * $01 on real hardware
            ophct: 0,
            opvct: 0,
            ophct_high: false,
            opvct_high: false,
            counter_latched: false,
            ppu1_mdr: 0,
            ppu2_mdr: 0,
            vram_prefetch: 0,
            oam_lsb_latch: 0,
            oam_priority_rotation: false,
            range_time_over: 0,
            refresh_charged_this_line: false,
            last_h_dot: 0,
            hdma_ran_channels: 0,
            step_access_count: 0,
            step_access_master: 0,
            accounting_suspended: 0,
            dot_master_remainder: 0,
            apu_master_remainder: 0,
        }
    }

    /// Master-cycle cost of one bus access, per the real SNES memory
    /// speed map: 6 ("fast") for most I/O registers and FastROM-enabled
    /// upper-bank ROM, 8 ("slow") for WRAM/cartridge/SlowROM, 12
    /// ("extra-slow") for the $4000-$41FF joypad region. Verified against
    /// fullsnes's "Memory Access Cycles" table.
    fn access_master_cycles(&self, addr: u32) -> u32 {
        let bank = (addr >> 16) as u8;
        let offset = addr & 0xFFFF;
        if bank <= 0x3F || (0x80..=0xBF).contains(&bank) {
            return match offset {
                0x0000..=0x1FFF => 8,  // WRAM mirror
                0x2000..=0x3FFF => 6,  // PPU/APU registers
                0x4000..=0x41FF => 12, // joypad ports (extra-slow)
                0x4200..=0x5FFF => 6,  // CPU I/O registers
                0x6000..=0x7FFF => 8,  // expansion / HiROM SRAM window
                _ => {
                    // ROM half: FastROM only applies to the upper banks.
                    if bank >= 0x80 && self.memsel & 0x01 != 0 {
                        6
                    } else {
                        8
                    }
                }
            };
        }
        if bank >= 0xC0 {
            return if self.memsel & 0x01 != 0 { 6 } else { 8 };
        }
        8 // banks $40-$7D and WRAM banks $7E/$7F
    }

    /// Returns and clears the (access count, summed master cycles) of
    /// every CPU-visible bus access since the previous call. The stepping
    /// loop combines this with the instruction's internal-cycle count
    /// (6 master cycles each) to advance real master-clock time -- see
    /// `tick_master`.
    pub fn take_step_access_costs(&mut self) -> (u32, u32) {
        let out = (self.step_access_count, self.step_access_master);
        self.step_access_count = 0;
        self.step_access_master = 0;
        out
    }

    /// Advances the whole machine by `master` master-clock cycles: the
    /// PPU dot clock runs at master/4 and the APU pacing base (the "CPU
    /// cycle" unit `Apu::tick` was calibrated in, ~2.68MHz) at master/8,
    /// with sub-unit remainders carried across calls so no time is lost
    /// to truncation.
    pub fn tick_master(&mut self, master: u32) {
        self.dot_master_remainder += master;
        let dots = self.dot_master_remainder / 4;
        self.dot_master_remainder %= 4;
        self.tick_ppu_dots(dots);

        self.apu_master_remainder += master;
        let apu_cycles = self.apu_master_remainder / 8;
        self.apu_master_remainder %= 8;
        self.apu.tick(apu_cycles);
    }

    /// Copies the live PPU dot/scanline counters into the $213C/$213D
    /// latches and flags STAT78. Triggered by reading $2137 (SLHV) or by a
    /// falling edge on WRIO ($4201) bit 7 -- both real-hardware sources.
    fn latch_hv_counters(&mut self) {
        self.ophct = self.ppu.h_counter();
        // Hardware's V counter leads our internal scanline by one: the
        // visible picture is V=1..224 (our scanlines 0..223) and NMI
        // fires at V=225 (our 224), so OPVCT reports scanline+1 (mod
        // lines-per-frame) -- the same mapping the VTIME comparison in
        // `tick_ppu_dots` uses.
        self.opvct = (self.ppu.scanline() + 1) % self.ppu.scanlines_per_frame();
        self.counter_latched = true;
    }

    /// True while the (level-triggered) timer-IRQ line is asserted. The
    /// CPU stepping loop should call `Cpu::irq` when this is true and the
    /// CPU's I flag is clear -- see `Cpu::irq`'s doc comment. The line
    /// stays asserted until the game acknowledges it by reading $4211.
    pub fn irq_pending(&self) -> bool {
        self.irq_line
    }

    /// Sets the live state of controller 1, in the SNES auto-read bit
    /// layout (bit15=B,14=Y,13=Select,12=Start,11=Up,10=Down,9=Left,
    /// 8=Right,7=A,6=X,5=L,4=R). Callers driving real input (keyboard,
    /// gamepad) should call this whenever pressed buttons change; the
    /// value only reaches the CPU-visible registers on the next
    /// auto-read latch (vblank entry) or manual $4016 strobe.
    pub fn set_joypad1_state(&mut self, state: u16) {
        self.joypad1_state = state;
    }

    /// Sets the live state of controller 2 -- same bit layout and latching
    /// rules as `set_joypad1_state` (visible via $4017 serial reads and the
    /// $421A/$421B auto-read registers).
    pub fn set_joypad2_state(&mut self, state: u16) {
        self.joypad2_state = state;
    }

    /// Renders the current VRAM/CGRAM/OAM contents to an RGBA8888
    /// framebuffer using the PER-SCANLINE register snapshots captured
    /// during the frame (see `scanline_regs`), so mid-frame register
    /// changes (SMW's IRQ status-bar split, HDMA scroll/COLDATA effects)
    /// land on the correct rows. See `crate::renderer` for exactly what
    /// is and isn't modeled.
    pub fn render_frame(&mut self) -> Vec<u8> {
        let (frame, range_time_over) = crate::renderer::render_frame_per_scanline_with_status(
            self.ppu.vram_ref(),
            self.ppu.cgram_ref(),
            self.ppu.oam_ref(),
            &self.scanline_regs,
        );
        // STAT77 ($213E) bits 6-7: sprite range/time-over, recomputed by
        // each frame's per-line sprite evaluation (real hardware clears
        // them at the start of every frame and accumulates while lines
        // render; recomputing per rendered frame lands in the same place
        // for code that polls them during vblank).
        self.range_time_over = range_time_over;
        frame
    }

    /// Read-only view of the per-scanline register snapshots that
    /// `render_frame` renders from -- lets tests/diagnostics inspect
    /// exactly what state each visible line was rendered with (IRQ raster
    /// splits, HDMA scroll/color effects).
    pub fn scanline_regs_ref(&self) -> &[PpuRegisters] {
        &self.scanline_regs
    }



    /// Read-only access to the current background/sprite rendering
    /// registers (e.g. for diagnostics or tests that need to check
    /// whether the screen has been turned on / which layers are enabled).
    pub fn ppu_registers(&self) -> &PpuRegisters {
        &self.ppu_regs
    }

    /// Advances the APU by `cycles` main-CPU cycles (the same unit
    /// `Cpu::step` returns). Callers should call this after every CPU step
    /// so the APU's own timing keeps pace with CPU execution.
    pub fn tick_apu(&mut self, cycles: u32) {
        self.apu.tick(cycles);
    }

    /// Read-only access to the APU (e.g. for pulling audio samples).
    pub fn apu_ref(&self) -> &Apu {
        &self.apu
    }

    /// Mutable access to the APU.
    pub fn apu_mut(&mut self) -> &mut Apu {
        &mut self.apu
    }

    /// Read-only access to the PPU (e.g. for pulling rendered frames).
    pub fn ppu_ref(&self) -> &Ppu {
        &self.ppu
    }

    /// Mutable access to the PPU.
    pub fn ppu_mut(&mut self) -> &mut Ppu {
        &mut self.ppu
    }

    /// Read-only access to the DMA controller (e.g. for diagnostics or
    /// tests checking real transfer-state flags like `is_active()`/
    /// `check_done()`/`hdma_pending()`/`is_enabled()` -- see
    /// `execute_dma_channel`/`hdma_init`/`hdma_run_scanline` for where
    /// those are actually driven from real transfer state).
    pub fn dma_ref(&self) -> &Dma {
        &self.dma
    }

    /// Advances the PPU by `cycles` main-CPU cycles at the fixed SlowROM
    /// ratio of 2 dots/cycle -- a compatibility wrapper around
    /// `tick_ppu_dots` for callers that count in CPU cycles. The
    /// master-clock-accurate path is `tick_master` (fed by
    /// `take_step_access_costs`'s real per-region access costs), which
    /// converts at the exact 4-master-cycles-per-dot hardware rate.
    ///
    /// Detects the vblank-entry edge and, if NMI is enabled via $4200,
    /// latches a pending NMI for `take_pending_nmi()` to consume. Also
    /// performs the auto-joypad-read latch on the same edge, matching real
    /// hardware (the auto-read happens automatically during vblank, not on
    /// demand): if enabled via $4200 bit 0, snapshots the live controller
    /// state into `joy1_auto`, which is what $4218/$4219 actually report.
    ///
    /// Also drives HDMA: the vblank-exit edge (leaving vblank, about to
    /// start a new frame's visible scanlines) re-initializes every channel
    /// armed in `hdma_enable_mask`, and the per-scanline hblank-entry edge
    /// (while not in vblank) runs one line's worth of transfers for them --
    /// see `hdma_init`/`hdma_run_scanline`.
    pub fn tick_ppu(&mut self, cycles: u32) {
        self.tick_ppu_dots(cycles.saturating_mul(2));
    }

    /// Dot-granular core of `tick_ppu`, also driven by the master-clock
    /// path (`tick_master`, at the exact 4-master-cycles-per-dot rate).
    fn tick_ppu_dots(&mut self, dots: u32) {
        self.ppu.tick_n(dots);

        let lines_per_frame = self.ppu.scanlines_per_frame();
        let current_line = self.ppu.scanline();
        let current_h = self.ppu.h_counter();
        let dots_per_line = crate::ppu::Ppu::pixels_per_line();

        // H/V timer IRQ targets, in dots. snes9x models the H-timer's
        // flag-set point as HTIME*4 + 14 master cycles into the line
        // (ppu.cpp `S9xUpdateIRQPositions`, `Timings.IRQTriggerCycles` =
        // 14), i.e. ~3.5 dots past the HTIME dot; a V-only IRQ uses the
        // HTIME=0 position (~dot 2.5). VTIME compares against the
        // HARDWARE V counter, which leads our internal scanline by one
        // (the picture is V=1..224 = our 0..223 and NMI fires at V=225 =
        // our 224), so the internal target line is VTIME-1, wrapping.
        let h_target: u16 = if self.htime == 0 { 2 } else { self.htime.saturating_add(3) };
        let v_target: Option<u16> = if self.vtime < lines_per_frame {
            Some(if self.vtime == 0 { lines_per_frame - 1 } else { self.vtime - 1 })
        } else {
            None
        };

        // Walk every scanline boundary this tick crossed (usually 0 or 1):
        // snapshot the rendering registers for visible lines, check the
        // timer-IRQ dot crossings, and charge each crossed line's WRAM
        // refresh stall -- real hardware stalls the CPU for 40 master
        // cycles while the S-CPU refreshes WRAM (snes9x
        // `SNES_WRAM_REFRESH_CYCLES` at position 538 master = dot ~134);
        // the PPU keeps running during the stall, so the cost is billed
        // to the CPU's pending access budget rather than ticked here.
        const WRAM_REFRESH_DOT: u16 = 134;
        const HDMA_DOT: u16 = 276; // snes9x SNES_HDMA_START_HC = 1106 master / 4
        let mut line = self.last_scanline;
        let mut h_from: i32 = self.last_h_dot as i32;
        loop {
            let done = line == current_line;
            let h_to: i32 = if done { current_h as i32 } else { dots_per_line as i32 - 1 };
            let hf = h_from;
            let crosses = move |target: u16| hf < target as i32 && (target as i32) <= h_to;

            let fire = match (self.irq_h_enable, self.irq_v_enable) {
                (false, false) => false,
                // H-timer alone: fires at the target dot of EVERY line.
                (true, false) => h_target < dots_per_line && crosses(h_target),
                // V-timer alone: fires once, at the start of VTIME's line.
                (false, true) => v_target == Some(line) && crosses(2),
                // H+V: fires at the exact dot of VTIME's line only.
                (true, true) => {
                    v_target == Some(line) && h_target < dots_per_line && crosses(h_target)
                }
            };
            if fire {
                self.irq_line = true;
            }

            if !self.refresh_charged_this_line && crosses(WRAM_REFRESH_DOT) {
                self.step_access_master += 40;
                self.refresh_charged_this_line = true;
            }

            // Per-line HDMA at the real transfer position, ~dot 276
            // (snes9x `SNES_HDMA_START_HC` = 1106 master cycles), on
            // every visible line plus the pre-visible line (hardware runs
            // HDMA on V=0..224 -- V=0 is our LAST internal line, whose
            // frame init just ran above). Checking the crossing per
            // walked line (instead of edge-detecting the final state)
            // means a tick spanning several lines runs EVERY line's
            // transfer.
            if crosses(HDMA_DOT)
                && (line < self.ppu.visible_scanlines() || line == lines_per_frame - 1)
            {
                self.hdma_run_scanline();
            }

            if done {
                break;
            }
            line = (line + 1) % lines_per_frame;
            h_from = -1; // a fresh line's window includes dot 0
            self.refresh_charged_this_line = false;
            if (line as usize) < self.scanline_regs.len() {
                self.scanline_regs[line as usize] = self.ppu_regs;
            }
            // HDMA frame init happens on hardware's line V=0 -- the LAST
            // internal scanline, one line before the first visible one
            // (snes9x: HC_HDMA_INIT_EVENT at V=0, HC=20). That line's own
            // hblank then runs the first per-line transfer, so the first
            // visible row already renders with the first table entry's
            // values. The RDNMI vblank flag also expires here: hardware
            // clears it at the end of the blanking period even if $4210
            // was never read (snes9x resets FillRAM[$4210] to the CPU
            // version at the V-counter wrap).
            if line == lines_per_frame - 1 {
                self.nmi_status_flag = false;
                self.hdma_init();
            }
        }
        self.last_scanline = current_line;
        self.last_h_dot = current_h;

        let in_vblank = self.ppu.in_vblank();
        if in_vblank && !self.was_in_vblank {
            self.nmi_status_flag = true;
            if self.nmi_enable {
                self.nmi_pending = true;
            }
            if self.auto_joypad_read_enable {
                self.joy1_auto = self.joypad1_state;
                self.joy2_auto = self.joypad2_state;
                // The auto-read physically strobes the controller ports
                // and clocks out all 16 bits, so it consumes the manual
                // serial-read state too: subsequent $4016/$4017 reads
                // (without a fresh manual strobe) report 1, exactly like
                // reads past the 16th bit. snes9x does the same
                // (controls.cpp `S9xDoAutoJoypad` sets `read_idx = 16`).
                self.joy1_shift = self.joypad1_state;
                self.joy2_shift = self.joypad2_state;
                self.joy1_bits_read = 16;
                self.joy2_bits_read = 16;
                self.joy1_ever_strobed = true;
            }
            // Real hardware reloads the live OAM address from the
            // $2102/$2103 latch at the start of every vblank (unless the
            // screen is in forced blank). Games rely on this instead of
            // rewriting OAMADD each frame -- DKC sets it once and DMAs 544
            // bytes to $2104 every vblank; without the reload the live
            // address kept marching past the end of OAM and no sprite
            // upload ever landed again.
            if self.ppu_regs.inidisp & 0x80 == 0 {
                self.oamadd = self.oamadd_latch;
                self.oam_high = false;
                self.refresh_first_sprite();
            }
        }
        self.was_in_vblank = in_vblank;
        self.was_in_hblank = self.ppu.in_hblank();
    }

    /// Returns true (and clears the flag) exactly once per frame when a
    /// vblank-entry NMI is due. Callers should check this between
    /// `Cpu::step()` calls and invoke `Cpu::nmi()` when it returns true --
    /// real hardware only takes interrupts at instruction boundaries.
    pub fn take_pending_nmi(&mut self) -> bool {
        let pending = self.nmi_pending;
        self.nmi_pending = false;
        pending
    }

    /// Applies $2115 (VMAIN) bits 2-3's "full graphic" address remapping
    /// to a VRAM word address: the selected bit-groups rotate so that
    /// sequential data-port accesses walk a bitmap column-major within
    /// 8-line strips. Formulas match snes9x's `S9xUpdateVRAMReadBuffer` /
    /// REGISTER_2118 remap tables (Shift 5/6/7, IncCount 32/64/128):
    ///   01: aaaaaaaa BBBccccc -> aaaaaaaa cccccBBB
    ///   10: aaaaaaaB BBcccccc -> aaaaaaac cccccBBB
    ///   11: aaaaaaBB Bccccccc -> aaaaaccc ccccBBB
    fn vram_remap(&self, word_addr: u16) -> u16 {
        match (self.vmain >> 2) & 0x03 {
            0 => word_addr,
            1 => (word_addr & 0xFF00) | ((word_addr & 0x00E0) >> 5) | ((word_addr & 0x001F) << 3),
            2 => (word_addr & 0xFE00) | ((word_addr & 0x01C0) >> 6) | ((word_addr & 0x003F) << 3),
            _ => (word_addr & 0xFC00) | ((word_addr & 0x0380) >> 7) | ((word_addr & 0x007F) << 3),
        }
    }

    /// Reloads the $2139/$213A read prefetch buffer with the word at the
    /// CURRENT (pre-increment) VMADD address -- the hardware sequence is
    /// "return buffer, refill buffer from the current address, then
    /// increment" (snes9x `S9xUpdateVRAMReadBuffer`).
    fn reload_vram_prefetch(&mut self) {
        let base = self.vram_remap(self.vmadd).wrapping_mul(2);
        let lo = self.ppu.vram_ref().read(base) as u16;
        let hi = self.ppu.vram_ref().read(base.wrapping_add(1)) as u16;
        self.vram_prefetch = (hi << 8) | lo;
    }

    /// $2118/$2119 VMDATAL/VMDATAH: writes one byte of the word at the
    /// current VRAM address (after VMAIN's bits-2-3 address remap), then
    /// advances that address per $2115 (VMAIN) -- bit 7 selects whether
    /// the increment happens after the low-byte write (bit clear) or the
    /// high-byte write (bit set), bits 0-1 select the increment amount
    /// (1/32/128 words).
    ///
    /// The PPU only grants the data port VRAM access during vblank or
    /// forced blank -- writes during active display are silently dropped
    /// (address increment included), matching snes9x's
    /// `BlockInvalidVRAMAccess` / `CHECK_INBLANK` behavior.
    fn vram_write(&mut self, is_high_byte: bool, value: u8) {
        if !self.ppu.in_vblank() && self.ppu_regs.inidisp & 0x80 == 0 {
            return;
        }
        let byte_addr = self
            .vram_remap(self.vmadd)
            .wrapping_mul(2)
            .wrapping_add(if is_high_byte { 1 } else { 0 });
        self.ppu.vram().write(byte_addr, value);

        let increments_now = if (self.vmain & 0x80) != 0 { is_high_byte } else { !is_high_byte };
        if increments_now {
            let step: u16 = match self.vmain & 0x03 {
                0 => 1,
                1 => 32,
                _ => 128,
            };
            self.vmadd = self.vmadd.wrapping_add(step);
        }
    }

    /// $2122 CGDATA: CGRAM is written as low/high byte pairs -- the first
    /// write after setting $2121 (CGADD) goes to the low byte, the second
    /// goes to the high byte and advances CGADD to the next color. Colors
    /// are 15-bit: the high byte's bit 7 doesn't exist in CGRAM and is
    /// masked off on write (snes9x REGISTER_2122: `(Byte & 0x7f) << 8`).
    fn cgram_write(&mut self, value: u8) {
        let byte_addr = (self.cgadd as u16).wrapping_mul(2).wrapping_add(if self.cgram_high { 1 } else { 0 });
        let value = if self.cgram_high { value & 0x7F } else { value };
        self.ppu.cgram().write(byte_addr, value);
        if self.cgram_high {
            self.cgadd = self.cgadd.wrapping_add(1);
        }
        self.cgram_high = !self.cgram_high;
    }

    /// Recomputes FirstSprite -- where sprite priority evaluation starts.
    /// With $2103 bit 7 (priority rotation) clear it's sprite 0; set, it
    /// follows the current OAM word address ((OAMADD & $FE) >> 1, snes9x
    /// ppu.cpp $2102/$2103 handlers). Stored in `ppu_regs` so the
    /// per-scanline register snapshots carry it into the renderer.
    fn refresh_first_sprite(&mut self) {
        self.ppu_regs.first_sprite = if self.oam_priority_rotation {
            ((self.oamadd & 0xFE) >> 1) as u8
        } else {
            0
        };
    }

    /// $2104 OAMDATA: the low table (bytes $000-$1FF) is written
    /// word-at-a-time through a latch -- the even byte is held in
    /// `oam_lsb_latch` and only committed together with the odd-byte
    /// write; the high table ($200+) writes each byte immediately (real
    /// hardware behavior, snes9x REGISTER_2104).
    fn oam_write(&mut self, value: u8) {
        let byte_addr = self.oamadd.wrapping_mul(2).wrapping_add(if self.oam_high { 1 } else { 0 });
        if byte_addr < 0x200 {
            if self.oam_high {
                self.ppu.oam().write(byte_addr.wrapping_sub(1), self.oam_lsb_latch);
                self.ppu.oam().write(byte_addr, value);
            } else {
                self.oam_lsb_latch = value;
            }
        } else {
            self.ppu.oam().write(byte_addr, value);
        }
        if self.oam_high {
            self.oamadd = self.oamadd.wrapping_add(1);
        }
        self.oam_high = !self.oam_high;
    }

    /// $2139/$213A VMDATALREAD/VMDATAHREAD: returns the low/high byte of
    /// the READ PREFETCH BUFFER, not of VRAM directly. On the read whose
    /// phase matches VMAIN bit 7's increment phase, the buffer is then
    /// refilled from the current (pre-increment) address and VMADD
    /// advances -- which is why real code issues one dummy read after
    /// setting $2116/$2117 before the actual data comes out. Mirrors
    /// snes9x's `IPPU.VRAMReadBuffer` handling in S9xGetPPU $2139/$213A.
    fn vram_read(&mut self, is_high_byte: bool) -> u8 {
        let value = if is_high_byte {
            (self.vram_prefetch >> 8) as u8
        } else {
            (self.vram_prefetch & 0xFF) as u8
        };

        let increments_now = if (self.vmain & 0x80) != 0 { is_high_byte } else { !is_high_byte };
        if increments_now {
            self.reload_vram_prefetch();
            let step: u16 = match self.vmain & 0x03 {
                0 => 1,
                1 => 32,
                _ => 128,
            };
            self.vmadd = self.vmadd.wrapping_add(step);
        }
        value
    }

    /// $213B CGDATAREAD: same low/high byte pairing idiom as `cgram_write`,
    /// auto-incrementing CGADD after the high-byte read. The high byte
    /// only drives 7 real bits -- bit 7 is PPU2 open bus (snes9x:
    /// `(PPU.OpenBus2 & 0x80) | (... >> 8) & 0x7f`).
    fn cgram_read(&mut self) -> u8 {
        let byte_addr = (self.cgadd as u16).wrapping_mul(2).wrapping_add(if self.cgram_high { 1 } else { 0 });
        let raw = self.ppu.cgram().read(byte_addr);
        let value = if self.cgram_high {
            (self.ppu2_mdr & 0x80) | (raw & 0x7F)
        } else {
            raw
        };
        if self.cgram_high {
            self.cgadd = self.cgadd.wrapping_add(1);
        }
        self.cgram_high = !self.cgram_high;
        value
    }

    /// $2138 OAMDATAREAD: same low/high byte pairing idiom as `oam_write`,
    /// auto-incrementing OAMADD after the high-byte read.
    fn oam_read(&mut self) -> u8 {
        let byte_addr = self.oamadd.wrapping_mul(2).wrapping_add(if self.oam_high { 1 } else { 0 });
        let value = self.ppu.oam().read(byte_addr);
        if self.oam_high {
            self.oamadd = self.oamadd.wrapping_add(1);
        }
        self.oam_high = !self.oam_high;
        value
    }

    /// Executes one DMA channel's transfer immediately and synchronously
    /// (real hardware spreads this over many cycles and halts the CPU
    /// meanwhile; since nothing here models cycle-accurate CPU/DMA
    /// interleaving yet, doing the whole transfer in one step is an
    /// honest simplification rather than a silent one -- it produces the
    /// same final VRAM/CGRAM/OAM/APU contents and register end-state).
    ///
    /// Both transfer directions (DMAP bit 7) go through the same
    /// `read_bus`/`write_bus` dispatch: CPU->PPU (bit 7 clear) reads the
    /// A-bus (cartridge/WRAM) and writes the B-bus (so a transfer aimed at
    /// $2118/$2119/$2122/$2104 lands in real VRAM/CGRAM/OAM via the helpers
    /// above, and one aimed at $2140-$2143 reaches the APU ports the same
    /// way a CPU-driven write would); PPU->CPU readback (bit 7 set) simply
    /// swaps source and destination, reading the B-bus register and
    /// writing the A-bus address.
    fn execute_dma_channel(&mut self, channel: usize) {
        let base = (channel as u8) * 0x10;
        let dmap = self.dma.read_register(base);
        let bbad = self.dma.read_register(base.wrapping_add(1));
        let a1t_lo = self.dma.read_register(base.wrapping_add(2));
        let a1t_hi = self.dma.read_register(base.wrapping_add(3));
        let a1b = self.dma.read_register(base.wrapping_add(4));
        let das_lo = self.dma.read_register(base.wrapping_add(5));
        let das_hi = self.dma.read_register(base.wrapping_add(6));

        let das_raw = ((das_hi as u16) << 8) | (das_lo as u16);
        // A DAS of 0 means "transfer 0x10000 bytes" on real hardware (it
        // wraps past 0xFFFF back through 0), a well-documented trick games
        // use for full-VRAM clears/fills -- not "nothing to transfer".
        let mut remaining: u32 = if das_raw == 0 { 0x10000 } else { das_raw as u32 };

        // DMAP ($43x0) layout: bit 7 = direction (B->A readback), bit 6 =
        // HDMA indirect, bits 4-3 = A-bus address step as a 2-BIT FIELD
        // (00 = increment, 10 = decrement, 01/11 = FIXED), bits 0-2 =
        // transfer-unit mode. Two earlier bugs here: bit 3 was tested as
        // the direction bit (silently skipping SMW's fixed-source
        // tilemap-clear fills, dmap=$08/$09, as "unimplemented readback"),
        // and bit 4 alone was treated as "fixed" (it actually means
        // decrement) -- either way the overworld's layer-1/3 tilemap
        // clears never landed and the map rendered as stale garbage.
        let direction_ppu_to_cpu = (dmap & 0x80) != 0;
        let step: i32 = match (dmap >> 3) & 0x03 {
            0b00 => 1,
            0b10 => -1,
            _ => 0, // 0b01 / 0b11: fixed source address
        };
        let mode = dmap & 0x07;
        let bbus_pattern = bbus_pattern_for_mode(mode);

        // Reflect real transfer state via `Dma::is_active()`/`check_done()`
        // -- previously nothing ever set these, so they were permanently
        // stuck reporting "never active, never done" regardless of what
        // transfers actually ran. This must happen for BOTH directions --
        // the PPU->CPU (B->A readback) path used to return before ever
        // reaching this point, leaving whatever `done`/`dma_active` state
        // was left over from an earlier, unrelated transfer stuck in place
        // (a poller reading `check_done()` could see a stale "finished"
        // signal that actually belonged to a previous transfer).
        self.dma.dma_active = true;
        if let Some(ch) = self.dma.channel_mut(channel) {
            ch.done = false;
        }

        let a_bank = (a1b as u32) << 16;
        let mut a_offset = ((a1t_hi as u16) << 8) | (a1t_lo as u16);
        let mut i: usize = 0;
        // The engine's own bus traffic is billed at the hardware rate (8
        // master cycles per byte, ticked per byte below), not as CPU
        // accesses. Per-channel setup costs another 8 master cycles up
        // front (snes9x `addCyclesInDMA`'s one-shot SLOW_ONE_CYCLE).
        self.accounting_suspended += 1;
        self.hdma_ran_channels &= !(1u8 << channel);
        self.tick_master(8);
        while remaining > 0 {
            let a_addr = a_bank | (a_offset as u32);
            let b_addr = 0x2100u32 + (bbad as u32) + (bbus_pattern[i % bbus_pattern.len()] as u32);
            if direction_ppu_to_cpu {
                // B-bus (PPU/APU register) -> A-bus (WRAM/cartridge) readback.
                let value = self.read_bus(b_addr).unwrap_or(self.open_bus);
                let _ = self.write_bus(a_addr, value);
            } else {
                // A-bus (WRAM/cartridge) -> B-bus (PPU/APU register).
                let value = self.read_bus(a_addr).unwrap_or(self.open_bus);
                let _ = self.write_bus(b_addr, value);
            }
            // Advance the whole machine 8 master cycles PER BYTE, exactly
            // like snes9x's `addCyclesInDMA` draining H-events after every
            // byte: NMI/IRQ flags latch and HDMA lines run at their real
            // positions DURING a long transfer instead of collapsing into
            // one giant tick at the end (a full 64KB DMA spans more than
            // an entire frame of machine time).
            self.tick_master(8);
            // The A-bus address steps within its bank (the bank byte never
            // carries/borrows on real hardware).
            a_offset = a_offset.wrapping_add(step as u16);
            i += 1;
            remaining -= 1;
            // If a per-line HDMA just ran on THIS channel, the DMA dies
            // on the spot: the transfer stops mid-way and $43x2/$43x5
            // reflect the partial progress (snes9x dma.cpp: "If HDMA
            // triggers in the middle of DMA transfer and it uses the
            // same channel, it kills the DMA transfer immediately").
            if self.hdma_ran_channels & (1 << channel) != 0 {
                break;
            }
        }
        let final_a1t = a_offset;
        self.dma.write_register(base.wrapping_add(2), (final_a1t & 0xFF) as u8);
        self.dma.write_register(base.wrapping_add(3), ((final_a1t >> 8) & 0xFF) as u8);
        self.dma.write_register(base.wrapping_add(5), (remaining & 0xFF) as u8);
        self.dma.write_register(base.wrapping_add(6), ((remaining >> 8) & 0xFF) as u8);

        if let Some(ch) = self.dma.channel_mut(channel) {
            ch.done = true;
        }
        self.dma.dma_active = false;
        self.accounting_suspended -= 1;
    }

    /// Re-initializes HDMA for every channel armed in `hdma_enable_mask`
    /// ($420C): resets that channel's table read pointer to its configured
    /// start address (`A1B:A1T`) and loads the first line-count entry (and,
    /// for indirect-addressing channels, the first indirect address). Real
    /// hardware does this once per frame during the tail of vblank, before
    /// scanline 0's first HDMA transfer; called from `tick_ppu` on the
    /// vblank-exit edge, which lands in the same place functionally.
    fn hdma_init(&mut self) {
        // Table reads are engine traffic, not CPU accesses.
        self.accounting_suspended += 1;
        for i in 0..8usize {
            if self.hdma_enable_mask & (1 << i) == 0 {
                continue;
            }
            let start = match self.dma.channel(i) {
                Some(ch) => ch.source_address(),
                None => continue,
            };
            if let Some(ch) = self.dma.channel_mut(i) {
                ch.a2a = (start & 0xFFFF) as u16;
                ch.hdma_terminated = false;
            }
            self.hdma_load_next_entry(i);
        }
        self.accounting_suspended -= 1;
        // The frame-start re-init stalls the CPU one DMA<->CPU clock sync
        // when any channel is armed (snes9x `S9xStartHDMA` charges
        // `Timings.DMACPUSync` = 18 once). Billed to the CPU's pending
        // access budget -- this runs inside `tick_ppu_dots`, so it can't
        // re-tick the machine from here.
        if self.hdma_enable_mask != 0 {
            self.step_access_master += 18;
        }
        self.update_hdma_pending();
    }

    /// Recomputes `Dma`'s `hdma_pending` flag from real per-channel state:
    /// true iff at least one channel armed in `hdma_enable_mask` has not
    /// yet hit its table's end-of-table marker. Called after `hdma_init`
    /// arms channels and after `hdma_run_scanline` runs a scanline (which
    /// may terminate a channel via `hdma_load_next_entry`), so
    /// `Dma::hdma_pending()` reflects real transfer state instead of never
    /// being touched.
    fn update_hdma_pending(&mut self) {
        let any_pending = (0..8usize).any(|i| {
            self.hdma_enable_mask & (1 << i) != 0
                && self.dma.channel(i).map(|ch| !ch.hdma_terminated).unwrap_or(false)
        });
        self.dma.set_hdma_pending(any_pending);
    }

    /// Reads the next line-count byte from a channel's HDMA table
    /// (advancing its table pointer past it), and for indirect-addressing
    /// channels also reads the following 2-byte indirect address.
    /// Terminates the channel for the rest of the frame if the freshly-read
    /// line-count byte is the 0x00 end-of-table marker.
    fn hdma_load_next_entry(&mut self, channel: usize) {
        let (table_addr, indirect, bank) = match self.dma.channel(channel) {
            Some(ch) => (ch.table_addr(), ch.hdma_indirect_mode(), ch.a1b),
            None => return,
        };

        let line_counter = self.read_bus(table_addr).unwrap_or(self.open_bus);
        // The count-byte fetch stalls the CPU 8 master cycles; an
        // indirect-address fetch costs 16 more (snes9x
        // `HDMAReadLineCount`: SLOW_ONE_CYCLE + SLOW_ONE_CYCLE<<1).
        self.step_access_master += 8;
        let mut next_offset = (table_addr.wrapping_add(1) & 0xFFFF) as u16;

        let mut indirect_addr = 0u16;
        if line_counter != 0 && indirect {
            self.step_access_master += 16;
            let ptr = ((bank as u32) << 16) | (next_offset as u32);
            let lo = self.read_bus(ptr).unwrap_or(self.open_bus) as u16;
            // The high byte's address must wrap within the same fixed
            // table bank, exactly like `next_offset` itself does above --
            // `ptr.wrapping_add(1)` operates on the full 24-bit pointer and
            // carries into `bank+1` when `next_offset == 0xFFFF`, since
            // `next_offset` is a `u16` its own `wrapping_add` naturally
            // stays within the bank.
            let hi_offset = next_offset.wrapping_add(1);
            let hi_addr = ((bank as u32) << 16) | (hi_offset as u32);
            let hi = self.read_bus(hi_addr).unwrap_or(self.open_bus) as u16;
            indirect_addr = (hi << 8) | lo;
            next_offset = next_offset.wrapping_add(2);
        }

        if let Some(ch) = self.dma.channel_mut(channel) {
            ch.hdma_line_counter = line_counter;
            ch.a2a = next_offset;
            ch.hdma_terminated = line_counter == 0;
            if indirect && line_counter != 0 {
                ch.das = indirect_addr;
            }
            // Every freshly-loaded entry transfers on its first line,
            // repeat bit or not (canonical hardware HDMA state machine).
            ch.hdma_do_transfer = line_counter != 0;
        }
    }

    /// Runs one scanline's worth of HDMA for every channel armed in
    /// `hdma_enable_mask` and not yet terminated, called once per scanline
    /// at the hblank-entry edge (from `tick_ppu`) for every non-vblank
    /// scanline. This is the canonical hardware state machine
    /// (anomie/fullsnes): transfer only when `hdma_do_transfer` is set
    /// (every entry's first line, and every line of repeat entries),
    /// decrement the WHOLE raw line-counter byte, re-derive
    /// `hdma_do_transfer` from the repeat bit, and reload when the 7-bit
    /// count hits zero.
    ///
    /// The previous version transferred on every line of every entry and
    /// never advanced a direct-mode channel's table pointer past its
    /// inline data, so the moment a non-repeat entry expired, the "next
    /// line-count byte" it read was actually the previous entry's DATA
    /// byte -- the whole table walk slid out of sync. DKC's intro drives
    /// BGMODE/CGADSUB/TS through exactly such tables (`7F 03 18 03 03 03
    /// 00`), and the desync sprayed the count byte 0x18 into all three
    /// registers for a few scanlines mid-screen (visible garbage bands).
    fn hdma_run_scanline(&mut self) {
        // Per-line transfers are engine traffic, billed at the hardware
        // rate inside `hdma_transfer_one_line`, not as CPU accesses.
        self.accounting_suspended += 1;
        let mut any_active = false;
        for i in 0..8usize {
            if self.hdma_enable_mask & (1 << i) == 0 {
                continue;
            }
            let (terminated, do_transfer) = match self.dma.channel(i) {
                Some(ch) => (ch.hdma_terminated, ch.hdma_do_transfer),
                None => continue,
            };
            if terminated {
                continue;
            }
            any_active = true;
            self.hdma_ran_channels |= 1 << i;

            if do_transfer {
                self.hdma_transfer_one_line(i);
            }

            let mut reload = false;
            if let Some(ch) = self.dma.channel_mut(i) {
                // Whole-byte decrement: the repeat bit is consumed along
                // with the count, so a raw 0x80 naturally behaves as
                // "transfer once, then wait 127 lines" like real hardware.
                ch.hdma_line_counter = ch.hdma_line_counter.wrapping_sub(1);
                ch.hdma_do_transfer = ch.hdma_line_counter & 0x80 != 0;
                reload = ch.hdma_line_counter & 0x7F == 0;
            }
            if reload {
                // Sets `hdma_do_transfer` for the fresh entry's first line
                // (or terminates on the 0x00 end-of-table marker). The
                // fetch costs are billed inside `hdma_load_next_entry`.
                self.hdma_load_next_entry(i);
            } else {
                // Non-reload lines still stall the CPU 8 master cycles
                // for the line-counter bookkeeping (snes9x `S9xDoHDMA`'s
                // end-of-line `ADD_CYCLES(SLOW_ONE_CYCLE)`).
                self.step_access_master += 8;
            }
        }
        // One CPU<->DMA clock sync per scanline with any active channel
        // (snes9x charges `Timings.DMACPUSync` once per S9xDoHDMA call).
        if any_active {
            self.step_access_master += 18;
        }
        self.accounting_suspended -= 1;
        self.update_hdma_pending();
    }

    /// Performs the actual B-bus write(s) for one channel's current
    /// scanline, reading source bytes from the table's current position
    /// (direct mode) or the latched indirect address (indirect mode).
    /// The source pointer ALWAYS advances past the bytes just read --
    /// direct-mode data lives inline in the table, so the table pointer
    /// must move past it or the next line-count read lands on data bytes
    /// (an earlier version only advanced when the repeat bit was set,
    /// which desynced every direct-mode table walk -- see
    /// `hdma_run_scanline`). Repeat entries re-transfer fresh bytes each
    /// line on real hardware too; "reuse the same bytes" was never how the
    /// hardware works.
    fn hdma_transfer_one_line(&mut self, channel: usize) {
        let (mode, bbad, indirect, src_bank, mut src_offset) = match self.dma.channel(channel) {
            Some(ch) => {
                let indirect = ch.hdma_indirect_mode();
                let (bank, offset) = if indirect {
                    (ch.dasb, ch.das)
                } else {
                    (ch.a1b, ch.a2a)
                };
                (ch.transfer_mode(), ch.bbad, indirect, bank, offset)
            }
            None => return,
        };

        let pattern = bbus_pattern_for_mode(mode);
        for &offset in pattern {
            let src_addr = ((src_bank as u32) << 16) | (src_offset as u32);
            let value = self.read_bus(src_addr).unwrap_or(self.open_bus);
            let dest_addr = 0x2100u32 + (bbad as u32) + (offset as u32);
            let _ = self.write_bus(dest_addr, value);
            // The source address steps within its bank only -- the bank
            // byte never carries/borrows on real hardware, matching every
            // sibling address-stepping path in this file (e.g. `next_offset`
            // in `hdma_load_next_entry` and `src_offset` in
            // `execute_dma_channel`).
            src_offset = src_offset.wrapping_add(1);
        }

        if let Some(ch) = self.dma.channel_mut(channel) {
            if indirect {
                ch.das = src_offset;
            } else {
                ch.a2a = src_offset;
            }
        }

        // Same hardware cost as immediate DMA: 8 MASTER cycles per byte,
        // billed to the CPU's pending access budget -- HDMA steals bus
        // time from the CPU, and this method runs from inside
        // `tick_ppu_dots`' own line walk, where advancing the PPU counter
        // directly would corrupt the walk's captured positions. The next
        // `take_step_access_costs`/`tick_master` round advances the whole
        // machine by these cycles, so no wall time is lost.
        self.step_access_master += 8u32.saturating_mul(pattern.len() as u32);
    }

    /// Serializes the complete bus-visible machine state (everything
    /// except the CPU itself and the immutable ROM): WRAM, all PPU memory
    /// and registers, DMA/HDMA, the APU, every $21xx/$42xx latch this bus
    /// models, and the cartridge's SRAM. See `crate::save_snapshot` for
    /// the versioned whole-machine entry point.
    pub fn save_state(&self, out: &mut Vec<u8>) {
        use crate::state::{put_bool, put_bytes, put_i32, put_u16, put_u32, put_u8};
        put_u8(out, self.open_bus);
        put_bool(out, self.nmi_enable);
        put_bool(out, self.nmi_status_flag);
        put_bool(out, self.nmi_pending);
        put_bool(out, self.was_in_vblank);
        put_bool(out, self.was_in_hblank);
        put_u8(out, self.hdma_enable_mask);
        put_u8(out, self.vmain);
        put_u16(out, self.vmadd);
        put_u8(out, self.cgadd);
        put_bool(out, self.cgram_high);
        put_u16(out, self.oamadd);
        put_u16(out, self.oamadd_latch);
        put_bool(out, self.oam_high);
        put_u16(out, self.joypad1_state);
        put_bool(out, self.auto_joypad_read_enable);
        put_u16(out, self.joy1_auto);
        put_bool(out, self.joypad_strobe);
        put_u16(out, self.joy1_shift);
        put_u8(out, self.joy1_bits_read);
        put_bool(out, self.joy1_ever_strobed);
        put_u16(out, self.joypad2_state);
        put_u16(out, self.joy2_auto);
        put_u16(out, self.joy2_shift);
        put_u8(out, self.joy2_bits_read);
        put_bool(out, self.irq_h_enable);
        put_bool(out, self.irq_v_enable);
        put_u16(out, self.htime);
        put_u16(out, self.vtime);
        put_bool(out, self.irq_line);
        put_u16(out, self.last_scanline);
        put_u8(out, self.wrmpya);
        put_u16(out, self.wrdiv);
        put_u16(out, self.rddiv);
        put_u16(out, self.rdmpy);
        put_u8(out, self.wrio);
        put_u8(out, self.memsel);
        put_u32(out, self.wmadd);
        put_i32(out, self.mpy);
        put_u16(out, self.ophct);
        put_u16(out, self.opvct);
        put_bool(out, self.ophct_high);
        put_bool(out, self.opvct_high);
        put_bool(out, self.counter_latched);
        put_u32(out, self.dot_master_remainder);
        put_u32(out, self.apu_master_remainder);
        put_u8(out, self.ppu1_mdr);
        put_u8(out, self.ppu2_mdr);
        put_u16(out, self.vram_prefetch);
        put_u8(out, self.oam_lsb_latch);
        put_bool(out, self.oam_priority_rotation);
        put_u8(out, self.range_time_over);
        put_bool(out, self.refresh_charged_this_line);
        put_u16(out, self.last_h_dot);
        self.ppu_regs.save_state(out);
        put_bytes(out, self.wram.as_slice());
        self.ppu.save_state(out);
        self.dma.save_state(out);
        self.apu.save_state(out);
        match &self.cartridge {
            Some(cart) => {
                put_u32(out, cart.sram().len() as u32);
                put_bytes(out, cart.sram());
            }
            None => put_u32(out, u32::MAX),
        }
    }

    /// Restores state produced by `save_state`. The same cartridge must
    /// already be loaded -- the ROM itself isn't serialized, and an SRAM
    /// size mismatch is rejected as a foreign save state.
    pub(crate) fn load_state(&mut self, r: &mut crate::state::StateReader) -> Result<(), EmulationError> {
        self.open_bus = r.u8()?;
        self.nmi_enable = r.bool()?;
        self.nmi_status_flag = r.bool()?;
        self.nmi_pending = r.bool()?;
        self.was_in_vblank = r.bool()?;
        self.was_in_hblank = r.bool()?;
        self.hdma_enable_mask = r.u8()?;
        self.vmain = r.u8()?;
        self.vmadd = r.u16()?;
        self.cgadd = r.u8()?;
        self.cgram_high = r.bool()?;
        self.oamadd = r.u16()?;
        self.oamadd_latch = r.u16()?;
        self.oam_high = r.bool()?;
        self.joypad1_state = r.u16()?;
        self.auto_joypad_read_enable = r.bool()?;
        self.joy1_auto = r.u16()?;
        self.joypad_strobe = r.bool()?;
        self.joy1_shift = r.u16()?;
        self.joy1_bits_read = r.u8()?;
        self.joy1_ever_strobed = r.bool()?;
        self.joypad2_state = r.u16()?;
        self.joy2_auto = r.u16()?;
        self.joy2_shift = r.u16()?;
        self.joy2_bits_read = r.u8()?;
        self.irq_h_enable = r.bool()?;
        self.irq_v_enable = r.bool()?;
        self.htime = r.u16()?;
        self.vtime = r.u16()?;
        self.irq_line = r.bool()?;
        self.last_scanline = r.u16()?;
        self.wrmpya = r.u8()?;
        self.wrdiv = r.u16()?;
        self.rddiv = r.u16()?;
        self.rdmpy = r.u16()?;
        self.wrio = r.u8()?;
        self.memsel = r.u8()?;
        self.wmadd = r.u32()?;
        self.mpy = r.i32()?;
        self.ophct = r.u16()?;
        self.opvct = r.u16()?;
        self.ophct_high = r.bool()?;
        self.opvct_high = r.bool()?;
        self.counter_latched = r.bool()?;
        self.dot_master_remainder = r.u32()?;
        self.apu_master_remainder = r.u32()?;
        self.ppu1_mdr = r.u8()?;
        self.ppu2_mdr = r.u8()?;
        self.vram_prefetch = r.u16()?;
        self.oam_lsb_latch = r.u8()?;
        self.oam_priority_rotation = r.bool()?;
        self.range_time_over = r.u8()?;
        self.refresh_charged_this_line = r.bool()?;
        self.last_h_dot = r.u16()?;
        self.step_access_count = 0;
        self.step_access_master = 0;
        self.accounting_suspended = 0;
        self.ppu_regs.load_state(r)?;
        {
            let len = self.wram.as_slice().len();
            let bytes = r.bytes(len)?.to_vec();
            self.wram.as_mut_slice().copy_from_slice(&bytes);
        }
        self.ppu.load_state(r)?;
        self.dma.load_state(r)?;
        self.apu.load_state(r)?;
        let sram_len = r.u32()?;
        match (&mut self.cartridge, sram_len) {
            (_, u32::MAX) => {}
            (Some(cart), len) => {
                if cart.sram().len() != len as usize {
                    return Err(EmulationError::InvalidSaveState(
                        "save state's SRAM size doesn't match the loaded cartridge",
                    ));
                }
                let bytes = r.bytes(len as usize)?.to_vec();
                cart.sram_mut().copy_from_slice(&bytes);
            }
            (None, _) => {
                return Err(EmulationError::InvalidSaveState(
                    "save state contains cartridge SRAM but no cartridge is loaded",
                ));
            }
        }
        // The per-scanline snapshots aren't serialized; seed every line
        // with the restored register state (they re-capture as the next
        // frame's scanlines are ticked).
        self.scanline_regs.fill(self.ppu_regs);
        Ok(())
    }

    /// Load a cartridge into the system bus
    pub fn load_cartridge(&mut self, rom: Vec<u8>) -> Result<(), EmulationError> {
        if rom.is_empty() {
            return Err(EmulationError::InvalidAddress(0));
        }
        
        self.cartridge = Some(Cartridge::new(rom));
        Ok(())
    }

    /// Get a mutable reference to WRAM for direct access if needed
    pub fn wram_mut(&mut self) -> &mut Wram {
        &mut self.wram
    }

    /// Get a mutable reference to the cartridge if loaded
    pub fn cartridge_mut(&mut self) -> Option<&mut Cartridge> {
        self.cartridge.as_mut()
    }

    /// Get a read-only reference to the cartridge if loaded
    pub fn cartridge_ref(&self) -> Option<&Cartridge> {
        self.cartridge.as_ref()
    }

    /// Check if a cartridge is loaded
    pub fn has_cartridge(&self) -> bool {
        self.cartridge.is_some()
    }

    /// Read from the bus with SNES memory mapping
    fn read_bus(&mut self, addr: u32) -> BusResult<u8> {
        if self.accounting_suspended == 0 {
            self.step_access_count += 1;
            self.step_access_master += self.access_master_cycles(addr);
        }
        let bank = (addr >> 16) as u8;
        let offset = addr & 0xFFFF;

        // $7E0000-$7FFFFF: WRAM (128KB, one contiguous address space --
        // bank $7E is the first 64KB and bank $7F is the second 64KB, NOT
        // a mirror of $7E. Aliasing them here previously made any WRAM
        // buffer at $7Fxxxx collide with whatever real code kept at the
        // matching $7Exxxx offset (e.g. SMW's self-modified OAMResetRoutine
        // at $7F8000 got clobbered by the graphics decompressor's unrelated
        // $7E8000ish writes).
        if (0x7E0000..0x800000).contains(&addr) {
            let result = self.wram.read_u8(addr)?;
            self.open_bus = result;
            return Ok(result);
        }

        // Banks $00-$3F and $80-$BF ONLY: the "system" banks, whose low
        // half holds the WRAM mirror and I/O registers. Banks $40-$7D and
        // $C0-$FF have NO WRAM mirror and NO I/O -- on real hardware they
        // are cartridge space across the entire 64KB (LoROM maps SRAM at
        // $70-$7D:$0000-$7FFF). Including $40-$7F in this group previously
        // routed SRAM accesses like SMW's `STA.L SaveData,X` ($700000+X)
        // into the low-WRAM mirror, letting the save-game routine
        // overwrite the CPU stack at $01F5+ with save-file bytes -- the
        // RTL then popped a zeroed return address and crashed into WRAM.
        if bank <= 0x3F || (0x80..=0xBF).contains(&bank) {
            // $0000-$1FFF: WRAM mirror (Direct Page). Every bank in this
            // group mirrors the SAME low 8KB of WRAM, not just bank $00 --
            // pass `offset` (always < 0x2000 here, so always < `Wram`'s
            // 0x10000 "bank 0" branch) rather than the full 24-bit `addr`.
            // The latter silently crashed (`InvalidAddress`) the instant
            // any code executed from/addressed a non-zero bank in this
            // range, e.g. a plain `LDA $1234` with DB != 0 -- previously
            // unreachable only because nothing had run that far yet.
            if offset < 0x2000 {
                let result = self.wram.read_u8(offset as u32)?;
                self.open_bus = result;
                return Ok(result);
            }

            // $2140-$217F: APU communication ports (mirrored every 4 bytes)
            if (0x2140..0x2180).contains(&offset) {
                let port = ((offset - 0x2140) % 4) as u8;
                let result = self.apu.read_port(port);
                self.open_bus = result;
                return Ok(result);
            }

            // $2134-$2136 MPYL/MPYM/MPYH: the mode-7 hardware multiplier's
            // 24-bit signed product (M7A * M7B's last written byte) -- see
            // the $211C write handler.
            if (0x2134..=0x2136).contains(&offset) {
                let shift = (offset - 0x2134) * 8;
                let result = ((self.mpy as u32) >> shift) as u8;
                self.ppu1_mdr = result;
                self.open_bus = result;
                return Ok(result);
            }

            // $2138: OAMDATAREAD -- readback mirroring $2104's write side
            // (see `oam_read`'s doc comment).
            if offset == 0x2138 {
                let result = self.oam_read();
                self.ppu1_mdr = result;
                self.open_bus = result;
                return Ok(result);
            }
            // $2139/$213A: VMDATALREAD/VMDATAHREAD -- returns the prefetch
            // buffer, reloading it per VMAIN's increment phase (see
            // `vram_read`'s doc comment for the exact hardware sequence).
            if offset == 0x2139 {
                let result = self.vram_read(false);
                self.ppu1_mdr = result;
                self.open_bus = result;
                return Ok(result);
            }
            if offset == 0x213A {
                let result = self.vram_read(true);
                self.ppu1_mdr = result;
                self.open_bus = result;
                return Ok(result);
            }
            // $213B: CGDATAREAD -- readback mirroring $2122's write side.
            // The second (high) byte only drives 7 bits; bit 7 comes from
            // PPU2's open bus (snes9x: `(PPU.OpenBus2 & 0x80) | ...`).
            if offset == 0x213B {
                let result = self.cgram_read();
                self.ppu2_mdr = result;
                self.open_bus = result;
                return Ok(result);
            }

            // $2137 SLHV: reading (the value itself is PPU1 open-bus)
            // latches the current H/V counters for $213C/$213D -- but only
            // while WRIO ($4201) bit 7 is high; with the latch pin held
            // low the soft-latch is disabled (snes9x `S9xLatchCounters`
            // gates on `Memory.FillRAM[0x4213] & 0x80`).
            if offset == 0x2137 {
                if self.wrio & 0x80 != 0 {
                    self.latch_hv_counters();
                }
                let result = self.ppu1_mdr;
                self.open_bus = result;
                return Ok(result);
            }
            // $213C OPHCT / $213D OPVCT: latched dot/scanline counters,
            // read low byte first then the 9th bit. The high-byte read
            // only drives bit 0; bits 7-1 come from PPU2's open bus
            // (snes9x: `(PPU.OpenBus2 & 0xfe) | ...`). Toggles reset by
            // reading $213F.
            if offset == 0x213C {
                let result = if self.ophct_high {
                    (self.ppu2_mdr & 0xFE) | ((self.ophct >> 8) & 0x01) as u8
                } else {
                    (self.ophct & 0xFF) as u8
                };
                self.ophct_high = !self.ophct_high;
                self.ppu2_mdr = result;
                self.open_bus = result;
                return Ok(result);
            }
            if offset == 0x213D {
                let result = if self.opvct_high {
                    (self.ppu2_mdr & 0xFE) | ((self.opvct >> 8) & 0x01) as u8
                } else {
                    (self.opvct & 0xFF) as u8
                };
                self.opvct_high = !self.opvct_high;
                self.ppu2_mdr = result;
                self.open_bus = result;
                return Ok(result);
            }
            // $213E STAT77: PPU1 status -- bit 7 = sprite time-over (>34
            // tiles on a line), bit 6 = range-over (>32 sprites on a
            // line), both computed by `render_frame`'s per-line sprite
            // evaluation; bit 4 = PPU1 open bus; low nibble = version 1.
            if offset == 0x213E {
                let result = (self.ppu1_mdr & 0x10) | self.range_time_over | 0x01;
                self.ppu1_mdr = result;
                self.open_bus = result;
                return Ok(result);
            }
            // $213F STAT78: PPU2 status -- bit 7 = interlace field (toggles
            // every frame), bit 6 = counters-latched flag, bit 5 = PPU2
            // open bus, bit 4 = PAL mode, low nibble = version (3, a
            // common late revision). Reading clears the latch flag and
            // resets both counter read toggles.
            if offset == 0x213F {
                let pal = matches!(self.ppu.mode(), crate::ppu::PpuMode::Pal);
                let result = (if self.ppu.field() { 0x80 } else { 0 })
                    | (if self.counter_latched { 0x40 } else { 0 })
                    | (self.ppu2_mdr & 0x20)
                    | (if pal { 0x10 } else { 0 })
                    | 0x03;
                self.counter_latched = false;
                self.ophct_high = false;
                self.opvct_high = false;
                self.ppu2_mdr = result;
                self.open_bus = result;
                return Ok(result);
            }

            // $2180 WMDATA: sequential WRAM data port -- reads the byte at
            // the 17-bit $2181-$2183 address, then auto-increments it
            // (wrapping within the 128KB).
            if offset == 0x2180 {
                let result = self
                    .wram
                    .read_u8(0x7E0000 + (self.wmadd & 0x1FFFF))
                    .unwrap_or(self.open_bus);
                self.wmadd = (self.wmadd + 1) & 0x1FFFF;
                self.open_bus = result;
                return Ok(result);
            }

            // Write-only PPU1 registers: reads return PPU1's open-bus
            // register, not the CPU's -- the PPU actively drives the
            // B-bus with its own MDR for these addresses (the exact set
            // snes9x returns `PPU.OpenBus1` for in S9xGetPPU).
            if matches!(offset, 0x2104..=0x2106 | 0x2108..=0x210A | 0x2114..=0x211A | 0x2124..=0x212A)
            {
                let result = self.ppu1_mdr;
                self.open_bus = result;
                return Ok(result);
            }

            // $2000-$3FFF: I/O registers (stub - return open-bus)
            if (0x2000..0x4000).contains(&offset) {
                return Ok(self.open_bus);
            }

            // $4213 RDIO: reads the programmable I/O port. With nothing
            // attached driving the pins, they follow the $4201 output latch.
            if offset == 0x4213 {
                let result = self.wrio;
                self.open_bus = result;
                return Ok(result);
            }
            // $4214/$4215 RDDIVL/RDDIVH: division quotient.
            if offset == 0x4214 {
                let result = (self.rddiv & 0xFF) as u8;
                self.open_bus = result;
                return Ok(result);
            }
            if offset == 0x4215 {
                let result = (self.rddiv >> 8) as u8;
                self.open_bus = result;
                return Ok(result);
            }
            // $4216/$4217 RDMPYL/RDMPYH: multiplication product / division
            // remainder.
            if offset == 0x4216 {
                let result = (self.rdmpy & 0xFF) as u8;
                self.open_bus = result;
                return Ok(result);
            }
            if offset == 0x4217 {
                let result = (self.rdmpy >> 8) as u8;
                self.open_bus = result;
                return Ok(result);
            }

            // $4210: RDNMI - bit 7 is the latched vblank-NMI flag, cleared
            // by this read; bits 6-4 are CPU open bus; bits 3-0 are the
            // 5A22 CPU version (2). Matches snes9x's `(byte & 0x80) |
            // (OpenBus & 0x70) | Model->_5A22` (ppu.cpp S9xGetCPU $4210).
            if offset == 0x4210 {
                let result = (if self.nmi_status_flag { 0x80 } else { 0x00 })
                    | (self.open_bus & 0x70)
                    | 0x02;
                self.nmi_status_flag = false;
                self.open_bus = result;
                return Ok(result);
            }

            // $4211: TIMEUP - bit 7 is the timer-IRQ flag; reading it
            // acknowledges the IRQ (deasserts the level-triggered line).
            // Bits 6-0 are CPU open bus (snes9x: `byte | (OpenBus & 0x7f)`).
            if offset == 0x4211 {
                let result =
                    (if self.irq_line { 0x80 } else { 0x00 }) | (self.open_bus & 0x7F);
                self.irq_line = false;
                self.open_bus = result;
                return Ok(result);
            }

            // $4300-$437F: DMA channel registers readback.
            if (0x4300..0x4380).contains(&offset) {
                let result = self.dma.read_register((offset - 0x4300) as u8);
                self.open_bus = result;
                return Ok(result);
            }

            // $4016: JOYSER0 manual joypad serial read (controller 1).
            // While the strobe line ($4016 bit0, see write_bus) is high,
            // the register continuously reflects the live state's first
            // bit (B) unshifted, matching real hardware. Once strobe goes
            // low, each read shifts out the next bit of the snapshot taken
            // at that moment (MSB/B first); after 16 bits, further reads
            // report 1 (pulled high), signaling "no more data" the same
            // way a standard controller with nothing chained behind it
            // does.
            if offset == 0x4016 {
                let bit = if !self.joy1_ever_strobed {
                    0
                } else if self.joypad_strobe {
                    (self.joypad1_state >> 15) & 1
                } else if self.joy1_bits_read < 16 {
                    let b = (self.joy1_shift >> (15 - self.joy1_bits_read)) & 1;
                    self.joy1_bits_read += 1;
                    b
                } else {
                    1
                };
                // Only bits 1-0 are driven by the controller port; bits
                // 7-2 are open bus (snes9x `S9xReadJOYSERn`:
                // `(OpenBus & ~3) | ...`).
                let result = (self.open_bus & 0xFC) | bit as u8;
                self.open_bus = result;
                return Ok(result);
            }
            // $4017: JOYSER1 manual joypad serial read (controller 2).
            // Mirrors the $4016 handler exactly -- the strobe line is
            // shared by both ports, so `joypad_strobe`/`joy1_ever_strobed`
            // gate this port too. The `ever_strobed` guard keeps the old
            // deliberately-safe "always 0 before any strobe" behavior that
            // an earlier always-1 stub regressed (SMW's boot code visited
            // far fewer distinct PCs when un-strobed reads returned 1).
            if offset == 0x4017 {
                let bit = if !self.joy1_ever_strobed {
                    0
                } else if self.joypad_strobe {
                    (self.joypad2_state >> 15) & 1
                } else if self.joy2_bits_read < 16 {
                    let b = (self.joy2_shift >> (15 - self.joy2_bits_read)) & 1;
                    self.joy2_bits_read += 1;
                    b
                } else {
                    1
                };
                // Port 2 additionally hardwires bits 4-2 high on real
                // hardware; bits 7-5 are open bus (snes9x:
                // `(OpenBus & ~3) | 0x1c | ...`).
                let result = (self.open_bus & 0xE0) | 0x1C | bit as u8;
                self.open_bus = result;
                return Ok(result);
            }
            // $4212: HVBJOY status -- bit7 = in vblank, bit6 = in hblank,
            // bit0 = auto-joypad-read in progress, bits 5-1 = CPU open
            // bus. The auto-read busy window spans the first two vblank
            // scanlines, matching snes9x's REGISTER_4212 (ppu.h: V in
            // [ScreenHeight+1, ScreenHeight+3) on their 1-based V
            // counter); the latch itself already happened on the vblank
            // edge, so a game that waits for busy to clear then reads
            // $4218+ sees exactly the values this frame's read produced.
            if offset == 0x4212 {
                let in_vblank = self.ppu.in_vblank();
                let in_hblank = self.ppu.in_hblank();
                let vs = self.ppu.visible_scanlines();
                let joy_busy = self.auto_joypad_read_enable
                    && self.ppu.scanline() >= vs
                    && self.ppu.scanline() < vs + 2;
                let result = (if in_vblank { 0x80 } else { 0 })
                    | (if in_hblank { 0x40 } else { 0 })
                    | (self.open_bus & 0x3E)
                    | (if joy_busy { 0x01 } else { 0 });
                self.open_bus = result;
                return Ok(result);
            }
            // $4218/$4219: JOY1L/JOY1H -- the auto-joypad-read result,
            // latched once per frame at vblank entry (see `tick_ppu`).
            // Layout: $4218 (low) d7=A d6=X d5=L d4=R d3-0=0;
            // $4219 (high) d7=B d6=Y d5=Select d4=Start d3=Up d2=Down
            // d1=Left d0=Right.
            if offset == 0x4218 {
                let result = (self.joy1_auto & 0xFF) as u8;
                self.open_bus = result;
                return Ok(result);
            }
            if offset == 0x4219 {
                let result = ((self.joy1_auto >> 8) & 0xFF) as u8;
                self.open_bus = result;
                return Ok(result);
            }
            // $421A/$421B: JOY2L/JOY2H -- controller 2's auto-read result,
            // latched on the same vblank edge as JOY1 (see `tick_ppu`).
            if offset == 0x421A {
                let result = (self.joy2_auto & 0xFF) as u8;
                self.open_bus = result;
                return Ok(result);
            }
            if offset == 0x421B {
                let result = ((self.joy2_auto >> 8) & 0xFF) as u8;
                self.open_bus = result;
                return Ok(result);
            }

            // $4000-$5FFF: I/O registers (stub - return open-bus)
            if (0x4000..0x6000).contains(&offset) {
                return Ok(self.open_bus);
            }

            // $6000-$7FFF: cartridge window (HiROM SRAM lives at
            // $20-$3F/$A0-$BF:$6000-$7FFF -- the cartridge's own mapper
            // decides whether this bank/offset hits SRAM); open-bus if the
            // cartridge doesn't claim it.
            if (0x6000..0x8000).contains(&offset) {
                if let Some(ref mut cart) = self.cartridge {
                    if let Ok(value) = cart.read_u8(addr) {
                        self.open_bus = value;
                        return Ok(value);
                    }
                }
                return Ok(self.open_bus);
            }

            // $8000-$FFFF: ROM or WRAM mirror
            if offset >= 0x8000 {
                // Try cartridge ROM first
                if let Some(ref mut cart) = self.cartridge {
                    match cart.read_u8(addr) {
                        Ok(value) => {
                            self.open_bus = value;
                            return Ok(value);
                        }
                        Err(EmulationError::OpenBus) => {
                            // Fall through to WRAM mirror
                        }
                        Err(e) => return Err(e),
                    }
                }
                
                // If no cartridge or ROM read failed, check for WRAM mirror
                // In SNES, banks $00-$3F at $8000-$FFFF can access WRAM when cart isn't mapped
                // For simplicity, we'll return open-bus here
                return Ok(self.open_bus);
            }
        }

        // Banks $40-$7D and $C0-$FF: cartridge space across the full 64KB
        // (ROM mirrors, and for LoROM banks $70-$7D:$0000-$7FFF the SRAM
        // window -- see `Cartridge`'s mapping).
        if (0x40..=0x7D).contains(&bank) || bank >= 0xC0 {
            if let Some(ref mut cart) = self.cartridge {
                match cart.read_u8(addr) {
                    Ok(value) => {
                        self.open_bus = value;
                        return Ok(value);
                    }
                    Err(EmulationError::OpenBus) => {
                        return Ok(self.open_bus);
                    }
                    Err(e) => return Err(e),
                }
            }
            // No cartridge - return open-bus
            return Ok(self.open_bus);
        }

        // For any unmapped area, return open-bus value (last value on bus)
        Ok(self.open_bus)
    }

    /// Write to the bus with SNES memory mapping
    fn write_bus(&mut self, addr: u32, value: u8) -> BusResult<()> {
        if self.accounting_suspended == 0 {
            self.step_access_count += 1;
            self.step_access_master += self.access_master_cycles(addr);
        }
        // Update open-bus on writes too
        self.open_bus = value;
        let bank = (addr >> 16) as u8;
        let offset = addr & 0xFFFF;

        // $7E0000-$7FFFFF: WRAM (128KB, one contiguous address space --
        // see the matching comment in `read_bus` for why $7F must NOT be
        // aliased onto $7E).
        if (0x7E0000..0x800000).contains(&addr) {
            return self.wram.write_u8(addr, value);
        }

        // Banks $00-$3F and $80-$BF ONLY -- see the matching comment in
        // `read_bus`: banks $40-$7D and $C0-$FF are pure cartridge space
        // (including the LoROM SRAM window at $70-$7D:$0000-$7FFF), and
        // routing them here is what let SMW's save routine overwrite the
        // CPU stack through the phantom WRAM mirror.
        if bank <= 0x3F || (0x80..=0xBF).contains(&bank) {
            // $0000-$1FFF: WRAM mirror (Direct Page) -- see the matching
            // comment in `read_bus` for why `offset`, not `addr`, is correct.
            if offset < 0x2000 {
                return self.wram.write_u8(offset as u32, value);
            }

            // $2140-$217F: APU communication ports (mirrored every 4 bytes)
            if (0x2140..0x2180).contains(&offset) {
                let port = ((offset - 0x2140) % 4) as u8;
                self.apu.write_port(port, value);
                return Ok(());
            }

            // $2100: INIDISP. Turning forced blank OFF while inside
            // vblank re-applies the $2102/$2103 OAM-address latch right
            // away -- the reload that this vblank's entry edge skipped
            // while the screen was blanked (snes9x mirrors the reload
            // into its $2100 handler for exactly this case).
            if offset == 0x2100 {
                let was_blanked = self.ppu_regs.inidisp & 0x80 != 0;
                self.ppu_regs.inidisp = value;
                if was_blanked && value & 0x80 == 0 && self.ppu.in_vblank() {
                    self.oamadd = self.oamadd_latch;
                    self.oam_high = false;
                    self.refresh_first_sprite();
                }
                return Ok(());
            }
            // $2101: OBSEL
            if offset == 0x2101 {
                self.ppu_regs.obsel = value;
                return Ok(());
            }
            // $2105: BGMODE
            if offset == 0x2105 {
                self.ppu_regs.bgmode = value;
                return Ok(());
            }
            // $2107-$210A: BG1SC-BG4SC
            if (0x2107..=0x210A).contains(&offset) {
                self.ppu_regs.bg_sc[(offset - 0x2107) as usize] = value;
                return Ok(());
            }
            // $210B: BG12NBA
            if offset == 0x210B {
                self.ppu_regs.bg12nba = value;
                return Ok(());
            }
            // $210C: BG34NBA
            if offset == 0x210C {
                self.ppu_regs.bg34nba = value;
                return Ok(());
            }
            // $210D-$2114: BG1HOFS/VOFS .. BG4HOFS/VOFS. All eight share a
            // single 8-bit latch (see `PpuRegisters::bg_scroll_latch`):
            // HOFS combines the new byte with the low 3 bits of the
            // previous full value (real hardware's documented behavior,
            // since H position only needs 10-13 significant bits), VOFS
            // simply combines the new byte with the latch.
            if (0x210D..=0x2114).contains(&offset) {
                let reg = offset - 0x210D;
                let bg = (reg / 2) as usize;
                let latch = self.ppu_regs.bg_scroll_latch;
                if reg % 2 == 0 {
                    let old = self.ppu_regs.bg_hofs[bg];
                    self.ppu_regs.bg_hofs[bg] =
                        ((value as u16) << 8) | ((latch as u16) & 0xF8) | ((old >> 8) & 0x07);
                } else {
                    self.ppu_regs.bg_vofs[bg] = ((value as u16) << 8) | (latch as u16);
                }
                self.ppu_regs.bg_scroll_latch = value;
                // $210D/$210E are ALSO M7HOFS/M7VOFS: real hardware runs
                // these two through the separate mode-7 latch in parallel
                // with the normal BG1 scroll latch above.
                if offset == 0x210D {
                    self.ppu_regs.m7_hofs =
                        (((value as u16) << 8) | (self.ppu_regs.m7_latch as u16)) & 0x1FFF;
                    self.ppu_regs.m7_latch = value;
                }
                if offset == 0x210E {
                    self.ppu_regs.m7_vofs =
                        (((value as u16) << 8) | (self.ppu_regs.m7_latch as u16)) & 0x1FFF;
                    self.ppu_regs.m7_latch = value;
                }
                return Ok(());
            }
            // $2106 MOSAIC: per-BG enable bits + pixel size.
            if offset == 0x2106 {
                self.ppu_regs.mosaic = value;
                return Ok(());
            }
            // $2133 SETINI: screen-mode select. EXTBG (bit 6) and
            // pseudo-hires (bit 3) are consumed by the renderer; bit 2
            // (overscan) moves the vblank boundary -- and with it the
            // NMI, HVBJOY, auto-joypad and OAM-reload edges -- to line
            // 239.
            if offset == 0x2133 {
                self.ppu_regs.setini = value;
                self.ppu.set_overscan(value & 0x04 != 0);
                return Ok(());
            }
            // $211A M7SEL: mode-7 screen-over / flip control.
            if offset == 0x211A {
                self.ppu_regs.m7sel = value;
                return Ok(());
            }
            // $211B-$211E M7A-M7D: affine matrix, written low-then-high
            // through the shared mode-7 latch. Each $211C (M7B) byte write
            // additionally triggers the hardware multiplier: MPY ($2134-
            // $2136) = M7A (signed 16-bit) * the byte just written (signed
            // 8-bit) -- available immediately on real hardware too.
            if (0x211B..=0x211E).contains(&offset) {
                let word = ((value as u16) << 8) | (self.ppu_regs.m7_latch as u16);
                self.ppu_regs.m7_latch = value;
                match offset {
                    0x211B => {
                        self.ppu_regs.m7a = word;
                        // The multiplier is combinational on M7A and M7B's
                        // high byte: writing EITHER operand refreshes MPY
                        // (snes9x sets `Need16x8Mulitply` on both $211B
                        // and $211C and computes MatrixA * (MatrixB >> 8)).
                        self.mpy = (word as i16 as i32)
                            * ((self.ppu_regs.m7b >> 8) as u8 as i8 as i32);
                    }
                    0x211C => {
                        self.ppu_regs.m7b = word;
                        self.mpy = (self.ppu_regs.m7a as i16 as i32) * (value as i8 as i32);
                    }
                    0x211D => self.ppu_regs.m7c = word,
                    _ => self.ppu_regs.m7d = word,
                }
                return Ok(());
            }
            // $211F M7X / $2120 M7Y: 13-bit signed center, same latch.
            if offset == 0x211F || offset == 0x2120 {
                let word = (((value as u16) << 8) | (self.ppu_regs.m7_latch as u16)) & 0x1FFF;
                self.ppu_regs.m7_latch = value;
                if offset == 0x211F {
                    self.ppu_regs.m7x = word;
                } else {
                    self.ppu_regs.m7y = word;
                }
                return Ok(());
            }
            // $212C: TM (main screen designation)
            if offset == 0x212C {
                self.ppu_regs.tm = value;
                return Ok(());
            }
            // $212D: TS (subscreen designation)
            if offset == 0x212D {
                self.ppu_regs.ts = value;
                return Ok(());
            }
            // $2123-$212B, $212E-$212F: window mask registers.
            if offset == 0x2123 { self.ppu_regs.w12sel = value; return Ok(()); }
            if offset == 0x2124 { self.ppu_regs.w34sel = value; return Ok(()); }
            if offset == 0x2125 { self.ppu_regs.wobjsel = value; return Ok(()); }
            if offset == 0x2126 { self.ppu_regs.wh0 = value; return Ok(()); }
            if offset == 0x2127 { self.ppu_regs.wh1 = value; return Ok(()); }
            if offset == 0x2128 { self.ppu_regs.wh2 = value; return Ok(()); }
            if offset == 0x2129 { self.ppu_regs.wh3 = value; return Ok(()); }
            if offset == 0x212A { self.ppu_regs.wbglog = value; return Ok(()); }
            if offset == 0x212B { self.ppu_regs.wobjlog = value; return Ok(()); }
            if offset == 0x212E { self.ppu_regs.tmw = value; return Ok(()); }
            if offset == 0x212F { self.ppu_regs.tsw = value; return Ok(()); }
            // $2130: CGWSEL (color-math control)
            if offset == 0x2130 {
                self.ppu_regs.cgwsel = value;
                return Ok(());
            }
            // $2131: CGADSUB (color-math enable/mode)
            if offset == 0x2131 {
                self.ppu_regs.cgadsub = value;
                return Ok(());
            }
            // $2132: COLDATA -- fixed subscreen color. Bit 5/6/7 select
            // which of B/G/R the low 5 bits are written to; multiple can
            // be set at once, and each write only updates the selected
            // channels (so software builds the color across several writes).
            if offset == 0x2132 {
                let intensity = (value & 0x1F) as u16;
                let mut c = self.ppu_regs.coldata;
                if value & 0x20 != 0 { c = (c & !0x001F) | intensity; }        // red (bits 0-4)
                if value & 0x40 != 0 { c = (c & !0x03E0) | (intensity << 5); } // green (bits 5-9)
                if value & 0x80 != 0 { c = (c & !0x7C00) | (intensity << 10); }// blue (bits 10-14)
                self.ppu_regs.coldata = c;
                return Ok(());
            }

            // $2102/$2103: OAMADDL/OAMADDH -- sets both the reload latch
            // (re-applied to the live address at each vblank start, see
            // `tick_ppu_dots`) and the live address itself, and resets the
            // low/high byte toggle.
            if offset == 0x2102 {
                self.oamadd_latch = (self.oamadd_latch & 0xFF00) | (value as u16);
                self.oamadd = self.oamadd_latch;
                self.oam_high = false;
                self.refresh_first_sprite();
                return Ok(());
            }
            if offset == 0x2103 {
                self.oamadd_latch = (self.oamadd_latch & 0x00FF) | (((value & 0x01) as u16) << 8);
                self.oamadd = self.oamadd_latch;
                self.oam_high = false;
                // Bit 7: sprite priority rotation -- evaluation starts at
                // FirstSprite = (OAMADD & $FE) >> 1 instead of sprite 0.
                self.oam_priority_rotation = value & 0x80 != 0;
                self.refresh_first_sprite();
                return Ok(());
            }
            // $2104: OAMDATA
            if offset == 0x2104 {
                self.oam_write(value);
                return Ok(());
            }
            // $2115: VMAIN
            if offset == 0x2115 {
                self.vmain = value;
                return Ok(());
            }
            // $2116/$2117: VMADDL/VMADDH. Writing either half also
            // reloads the $2139/$213A read prefetch buffer from the new
            // address (hardware behavior -- this is what the post-address
            // "dummy read" idiom actually consumes).
            if offset == 0x2116 {
                self.vmadd = (self.vmadd & 0xFF00) | (value as u16);
                self.reload_vram_prefetch();
                return Ok(());
            }
            if offset == 0x2117 {
                self.vmadd = (self.vmadd & 0x00FF) | ((value as u16) << 8);
                self.reload_vram_prefetch();
                return Ok(());
            }
            // $2118/$2119: VMDATAL/VMDATAH
            if offset == 0x2118 {
                self.vram_write(false, value);
                return Ok(());
            }
            if offset == 0x2119 {
                self.vram_write(true, value);
                return Ok(());
            }
            // $2121: CGADD -- also resets the low/high byte toggle.
            if offset == 0x2121 {
                self.cgadd = value;
                self.cgram_high = false;
                return Ok(());
            }
            // $2122: CGDATA
            if offset == 0x2122 {
                self.cgram_write(value);
                return Ok(());
            }

            // $2180 WMDATA: sequential WRAM data port (write side) -- see
            // the read handler. DMA aimed at B-bus $80 lands here, which is
            // how games bulk-clear/fill WRAM without a CPU copy loop.
            if offset == 0x2180 {
                let _ = self.wram.write_u8(0x7E0000 + (self.wmadd & 0x1FFFF), value);
                self.wmadd = (self.wmadd + 1) & 0x1FFFF;
                return Ok(());
            }
            // $2181-$2183 WMADDL/WMADDM/WMADDH: the port's 17-bit address.
            if offset == 0x2181 {
                self.wmadd = (self.wmadd & 0x1FF00) | (value as u32);
                return Ok(());
            }
            if offset == 0x2182 {
                self.wmadd = (self.wmadd & 0x100FF) | ((value as u32) << 8);
                return Ok(());
            }
            if offset == 0x2183 {
                self.wmadd = (self.wmadd & 0x0FFFF) | (((value & 0x01) as u32) << 16);
                return Ok(());
            }

            // $2000-$3FFF: I/O registers (write ignored)
            if (0x2000..0x4000).contains(&offset) {
                return Ok(());
            }

            // $4201 WRIO: programmable I/O port output latch (read back at
            // $4213). A falling edge on bit 7 latches the PPU H/V
            // counters -- same effect as reading $2137 (SLHV).
            if offset == 0x4201 {
                if (self.wrio & 0x80) != 0 && (value & 0x80) == 0 {
                    self.latch_hv_counters();
                }
                self.wrio = value;
                return Ok(());
            }
            // $4202 WRMPYA: multiplicand. Writing it alone starts nothing.
            if offset == 0x4202 {
                self.wrmpya = value;
                return Ok(());
            }
            // $4203 WRMPYB: writing the multiplier starts the unsigned
            // 8x8->16 multiply. Real hardware needs 8 CPU cycles before
            // $4216/$4217 are valid; the result is available immediately
            // here (same honest simplification as immediate DMA).
            if offset == 0x4203 {
                self.rdmpy = (self.wrmpya as u16).wrapping_mul(value as u16);
                return Ok(());
            }
            // $4204/$4205 WRDIVL/WRDIVH: 16-bit dividend.
            if offset == 0x4204 {
                self.wrdiv = (self.wrdiv & 0xFF00) | (value as u16);
                return Ok(());
            }
            if offset == 0x4205 {
                self.wrdiv = (self.wrdiv & 0x00FF) | ((value as u16) << 8);
                return Ok(());
            }
            // $4206 WRDIVB: writing the divisor starts the 16/8 divide
            // (quotient -> $4214/$4215, remainder -> $4216/$4217). Divide
            // by zero yields quotient 0xFFFF and remainder = dividend,
            // matching real hardware.
            if offset == 0x4206 {
                if value == 0 {
                    self.rddiv = 0xFFFF;
                    self.rdmpy = self.wrdiv;
                } else {
                    self.rddiv = self.wrdiv / (value as u16);
                    self.rdmpy = self.wrdiv % (value as u16);
                }
                return Ok(());
            }
            // $420D MEMSEL: FastROM select -- stored only (no fast/slow
            // cycle timing is modeled; see the field's doc comment).
            if offset == 0x420D {
                self.memsel = value & 0x01;
                return Ok(());
            }

            // $4200: NMITIMEN - bit 7 enables vblank NMI generation,
            // bits 4/5 enable the H/V timer IRQ, bit 0 enables the
            // automatic joypad read at vblank.
            if offset == 0x4200 {
                let was_enabled = self.nmi_enable;
                self.nmi_enable = (value & 0x80) != 0;
                // Enabling NMI while the vblank flag ($4210 bit 7) is
                // still set triggers an NMI immediately -- games that
                // turn NMI on mid-vblank rely on it firing right away
                // instead of waiting a full frame (snes9x ppu.cpp $4200:
                // "NMI can trigger immediately during VBlank as long as
                // NMI_read ($4210) wasn't cleared").
                if !was_enabled
                    && self.nmi_enable
                    && self.nmi_status_flag
                    && self.ppu.in_vblank()
                {
                    self.nmi_pending = true;
                }
                self.irq_h_enable = (value & 0x10) != 0;
                self.irq_v_enable = (value & 0x20) != 0;
                if value & 0x30 == 0 {
                    // Disabling both timer IRQs acknowledges any pending
                    // one (matching real hardware -- SMW relies on being
                    // able to shut the raster IRQ off from inside its
                    // handler without a stale line re-firing).
                    self.irq_line = false;
                }
                self.auto_joypad_read_enable = (value & 0x01) != 0;
                return Ok(());
            }
            // $4207-$420A: HTIMEL/HTIMEH/VTIMEL/VTIMEH (9-bit each).
            if offset == 0x4207 {
                self.htime = (self.htime & 0x100) | (value as u16);
                return Ok(());
            }
            if offset == 0x4208 {
                self.htime = (self.htime & 0x00FF) | (((value & 0x01) as u16) << 8);
                return Ok(());
            }
            if offset == 0x4209 {
                self.vtime = (self.vtime & 0x100) | (value as u16);
                return Ok(());
            }
            if offset == 0x420A {
                self.vtime = (self.vtime & 0x00FF) | (((value & 0x01) as u16) << 8);
                return Ok(());
            }

            // $4016: JOYSER0 -- bit0 is the strobe line. While held high,
            // reads continuously reflect the live state's first bit (see
            // the $4016 read handler above); the falling edge (strobe
            // transitioning from 1 to 0) freezes `joy1_shift` with a
            // snapshot of the live state and resets the read position so
            // the next $4016 reads shift that snapshot out from the top
            // bit.
            if offset == 0x4016 {
                self.joy1_ever_strobed = true;
                let new_strobe = (value & 0x01) != 0;
                let old_strobe = self.joypad_strobe;
                if old_strobe && !new_strobe {
                    self.joy1_shift = self.joypad1_state;
                    self.joy1_bits_read = 0;
                    self.joy2_shift = self.joypad2_state;
                    self.joy2_bits_read = 0;
                }
                self.joypad_strobe = new_strobe;
                return Ok(());
            }

            // $420B: MDMAEN - triggers an immediate transfer on each set
            // bit's channel. Ignored while the DMA/HDMA engine itself is
            // on the bus (snes9x guards with `CPU.InDMAorHDMA`), which
            // also prevents a transfer aimed at $420B from recursing.
            // A non-zero trigger costs a one-time CPU<->DMA clock sync,
            // averaged to 18 master cycles like snes9x's
            // `Timings.DMACPUSync` (the real cost is 12-24 depending on
            // clock phase).
            if offset == 0x420B {
                if self.accounting_suspended > 0 {
                    return Ok(());
                }
                if value != 0 {
                    self.tick_master(18);
                }
                for ch in 0..8u8 {
                    if (value & (1 << ch)) != 0 {
                        self.execute_dma_channel(ch as usize);
                    }
                }
                return Ok(());
            }
            // $420C: HDMAEN - which channels run HDMA. Per-scanline
            // execution is driven from `tick_ppu` (see `hdma_init`/
            // `hdma_run_scanline`), keyed off this mask.
            if offset == 0x420C {
                self.hdma_enable_mask = value;
                // Mirror into `Dma` so `is_enabled()` has a real source of
                // truth instead of guessing from register contents (see
                // `Dma::is_enabled`'s doc comment).
                self.dma.set_enable_mask(value);
                return Ok(());
            }
            // $4300-$437F: DMA channel registers (8 channels x 16 bytes).
            if (0x4300..0x4380).contains(&offset) {
                self.dma.write_register((offset - 0x4300) as u8, value);
                return Ok(());
            }

            // $4000-$5FFF: I/O registers (write ignored)
            if (0x4000..0x6000).contains(&offset) {
                return Ok(());
            }

            // $6000-$7FFF: cartridge window (HiROM SRAM) -- see the
            // matching comment in `read_bus`. Ignored if the cartridge
            // doesn't claim it.
            if (0x6000..0x8000).contains(&offset) {
                if let Some(ref mut cart) = self.cartridge {
                    let _ = cart.write_u8(addr, value);
                }
                return Ok(());
            }

            // $8000-$FFFF: Try cartridge SRAM, else ignore
            if offset >= 0x8000 {
                if let Some(ref mut cart) = self.cartridge {
                    // Try writing to cartridge (SRAM)
                    match cart.write_u8(addr, value) {
                        Ok(()) => return Ok(()),
                        Err(EmulationError::OpenBus) => {
                            // SRAM write failed/mapped, ignore
                            return Ok(());
                        }
                        Err(e) => return Err(e),
                    }
                }
                return Ok(());
            }
        }

        // Banks $40-$7D and $C0-$FF: cartridge space (LoROM SRAM lives at
        // $70-$7D:$0000-$7FFF; ROM writes are ignored as open-bus).
        if (0x40..=0x7D).contains(&bank) || bank >= 0xC0 {
            if let Some(ref mut cart) = self.cartridge {
                match cart.write_u8(addr, value) {
                    Ok(()) => return Ok(()),
                    Err(EmulationError::OpenBus) => {
                        // ROM/unmapped: write ignored
                        return Ok(());
                    }
                    Err(e) => return Err(e),
                }
            }
            return Ok(());
        }

        // For unmapped areas, just ignore the write
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_bus_new() {
        let bus = SystemBus::new();
        assert!(!bus.has_cartridge());
    }

    #[test]
    fn system_bus_load_cartridge() {
        let mut bus = SystemBus::new();
        let rom = vec![0x42; 0x80000]; // 512KB ROM
        bus.load_cartridge(rom).unwrap();
        assert!(bus.has_cartridge());
    }

    #[test]
    fn system_bus_load_empty_rom_fails() {
        let mut bus = SystemBus::new();
        assert!(bus.load_cartridge(vec![]).is_err());
    }

    #[test]
    fn system_bus_wram_read_write() {
        let mut bus = SystemBus::new();
        
        // Write to WRAM
        bus.write_u8(0x7E1234, 0xAB).unwrap();
        
        // Read back
        let value = bus.read_u8(0x7E1234).unwrap();
        assert_eq!(value, 0xAB);
    }

    #[test]
    fn system_bus_wram_mirror() {
        let mut bus = SystemBus::new();
        
        // Write to bank 0 address (WRAM mirror)
        bus.write_u8(0x1234, 0xCD).unwrap();
        
        // Read from $7E0000 mirror
        let value = bus.read_u8(0x7E1234).unwrap();
        assert_eq!(value, 0xCD);
    }

    #[test]
    fn system_bus_wram_mirror_works_from_every_bank_not_just_bank_zero() {
        // Regression guard: $0000-$1FFF mirrors WRAM in every bank of the
        // SYSTEM group ($00-$3F and $80-$BF), not just bank $00. The bus
        // used to pass the *full* 24-bit address straight to `Wram`,
        // which only recognizes addresses literally in $7E0000-$7FFFFF or
        // literally below $10000 -- so e.g. a plain `LDA $1234` with
        // DB=$05 (bank $05, offset $1234, well within the WRAM-mirror
        // range) crashed with `InvalidAddress` instead of reading WRAM.
        // This was unreachable in early testing because nothing had
        // executed far enough to hit a non-zero-bank low-address access;
        // it became a real, repeatable crash once CPU coverage improved
        // enough to reach deeper SMW code.
        //
        // Banks $7E/$7F are deliberately excluded: they ARE WRAM itself
        // (the real, independent first/second 64KB halves), not a mirror
        // of it -- see `system_bus_wram_7e_and_7f_are_independent_not_mirrored`.
        // Banks $40-$7D are ALSO excluded: they are pure cartridge space
        // (LoROM maps SRAM at $70-$7D:$0000-$7FFF). An earlier version of
        // the bus wrongly gave them the WRAM mirror, which let SMW's
        // SaveTheGame routine ($009BB6+, `STA.L SaveData,X` = $700000+X)
        // overwrite the CPU stack at $01F5+ with save-file bytes -- the
        // RTL then popped zeros and execution escaped into WRAM/open bus.
        let mut bus = SystemBus::new();
        bus.write_u8(0x7E1234, 0xAB).unwrap();

        for bank in [0x00u32, 0x01, 0x05, 0x3F, 0x80, 0xBF] {
            let addr = (bank << 16) | 0x1234;
            assert_eq!(
                bus.read_u8(addr).unwrap(),
                0xAB,
                "bank ${:02X} offset $1234 must mirror WRAM, not crash or return something else",
                bank
            );
        }

        // And the reverse: writing through a non-zero bank's mirror must
        // land in the same underlying WRAM byte.
        bus.write_u8(0x051234, 0xCD).unwrap();
        assert_eq!(bus.read_u8(0x7E1234).unwrap(), 0xCD);

        // Banks $40-$7D must NOT reach WRAM through a phantom mirror: a
        // write to $70:1234 (LoROM SRAM space; no cartridge loaded here,
        // so it's simply ignored) must leave WRAM untouched.
        bus.write_u8(0x701234, 0x77).unwrap();
        assert_eq!(
            bus.read_u8(0x7E1234).unwrap(),
            0xCD,
            "a bank-$70 write must never land in low WRAM -- that's the SaveTheGame stack clobber"
        );
    }

    #[test]
    fn dma_fixed_source_fill_writes_the_same_byte_across_vram() {
        // DMAP bits 4-3 are a 2-bit A-bus step FIELD: 01 (dmap=$08/$09)
        // means FIXED source. SMW clears its layer tilemaps with exactly
        // this (one constant byte streamed $1000 times to $2118/$2119) --
        // an earlier version misread bit 3 as "B->A direction" and
        // silently skipped these fills entirely, leaving stale garbage in
        // every tilemap the game thought it had cleared.
        let mut bus = SystemBus::new();
        bus.write_u8(0x7E0010, 0x5A).unwrap(); // the fill byte, in WRAM

        bus.write_u8(0x002115, 0x80).unwrap(); // VMAIN: word step on high byte
        bus.write_u8(0x002116, 0x00).unwrap(); // VMADD = word 0x0100
        bus.write_u8(0x002117, 0x01).unwrap();

        bus.write_u8(0x004300, 0x09).unwrap(); // DMAP0: fixed source, mode 1
        bus.write_u8(0x004301, 0x18).unwrap(); // BBAD0: $2118/$2119
        bus.write_u8(0x004302, 0x10).unwrap(); // A1T = $7E0010
        bus.write_u8(0x004303, 0x00).unwrap();
        bus.write_u8(0x004304, 0x7E).unwrap();
        bus.write_u8(0x004305, 0x08).unwrap(); // DAS = 8 bytes = 4 words
        bus.write_u8(0x004306, 0x00).unwrap();
        bus.write_u8(0x00420B, 0x01).unwrap(); // fire channel 0

        for word in 0x0100u16..0x0104 {
            assert_eq!(
                bus.ppu_ref().vram_ref().read_word(word.wrapping_mul(2)),
                0x5A5A,
                "fixed-source fill must write the same byte to every word (word {:#06X})",
                word
            );
        }
    }

    #[test]
    fn dma_decrement_mode_streams_the_source_backwards() {
        // DMAP bits 4-3 = 10 (dmap=$10 | mode) means the A-bus address
        // DECREMENTS -- previously misread as "fixed".
        let mut bus = SystemBus::new();
        bus.write_u8(0x7E0020, 0x11).unwrap();
        bus.write_u8(0x7E001F, 0x22).unwrap();
        bus.write_u8(0x7E001E, 0x33).unwrap();
        bus.write_u8(0x7E001D, 0x44).unwrap();

        bus.write_u8(0x002115, 0x80).unwrap();
        bus.write_u8(0x002116, 0x00).unwrap();
        bus.write_u8(0x002117, 0x02).unwrap(); // VMADD = word 0x0200

        bus.write_u8(0x004300, 0x11).unwrap(); // DMAP0: decrement, mode 1
        bus.write_u8(0x004301, 0x18).unwrap();
        bus.write_u8(0x004302, 0x20).unwrap(); // A1T = $7E0020, walking down
        bus.write_u8(0x004303, 0x00).unwrap();
        bus.write_u8(0x004304, 0x7E).unwrap();
        bus.write_u8(0x004305, 0x04).unwrap();
        bus.write_u8(0x004306, 0x00).unwrap();
        bus.write_u8(0x00420B, 0x01).unwrap();

        assert_eq!(bus.ppu_ref().vram_ref().read_word(0x0200 * 2), 0x2211,
            "first word = bytes at $20 (low) then $1F (high)");
        assert_eq!(bus.ppu_ref().vram_ref().read_word(0x0201 * 2), 0x4433,
            "second word = bytes at $1E then $1D -- the source must walk backwards");
    }

    #[test]
    fn h_only_irq_fires_at_configured_htime_not_every_scanline() {
        // $4200 bit 4 alone (H-timer only, no V): real hardware fires the
        // IRQ on EVERY scanline, at the exact dot the beam passes HTIME
        // ($4207/$4208) -- the flag-set point is HTIME*4 + 14 master
        // cycles into the line (snes9x `PPU.HTimerPosition`), i.e. ~3 dots
        // past the HTIME dot. It must NOT fire anywhere else on the line.
        let mut bus = SystemBus::new();
        bus.write_u8(0x004207, 100).unwrap(); // HTIME = 100 -> fires at ~dot 103
        bus.write_u8(0x004208, 0).unwrap();
        bus.write_u8(0x004200, 0x10).unwrap(); // H-IRQ enable only (bit 4)

        // Land just before the trigger dot: must not have fired yet.
        bus.tick_master(101 * 4); // h_counter = 101 < 103
        assert!(!bus.irq_pending(), "no IRQ before the beam reaches HTIME's trigger dot");

        // Cross the trigger dot within the same line: fires.
        bus.tick_master(4 * 4); // h_counter = 105 >= 103
        assert!(bus.irq_pending(), "H-IRQ must fire when the beam crosses HTIME");

        // Acknowledge, then finish the line and stop before the NEXT
        // line's trigger dot: must not fire in between (the old
        // scanline-granular model fired at boundaries, not at HTIME).
        assert_eq!(bus.read_u8(0x004211).unwrap() & 0x80, 0x80);
        assert!(!bus.irq_pending());
        bus.tick_master((341 - 105 + 50) * 4); // next line, h_counter = 50
        assert!(!bus.irq_pending(), "crossing a line boundary alone (dot 50 < HTIME) must not fire");

        // ...and crossing the new line's trigger dot fires again: the
        // H-timer is a once-PER-LINE event on real hardware.
        bus.tick_master(60 * 4); // h_counter = 110 >= 103
        assert!(bus.irq_pending(), "the H-timer re-fires on every line at HTIME");
    }

    #[test]
    fn v_timer_irq_fires_at_vtime_and_is_acknowledged_by_reading_4211() {
        // The V-timer IRQ SMW arms every in-level frame for its status-bar
        // raster split: enabled via $4200 bit 5, fires when the scanline
        // reaches VTIME ($4209/$420A), stays asserted (level-triggered)
        // until $4211 is read, and is also cleared by disabling both timer
        // enables.
        let mut bus = SystemBus::new();
        bus.write_u8(0x004209, 100).unwrap(); // VTIME = 100
        bus.write_u8(0x00420A, 0).unwrap();
        bus.write_u8(0x004200, 0x20).unwrap(); // V-IRQ enable
        assert!(!bus.irq_pending(), "no IRQ before the target scanline");

        bus.tick_master(100 * 341 * 4); // advance exactly 100 scanlines
        assert!(bus.irq_pending(), "IRQ line must assert at scanline == VTIME");

        // Still asserted until acknowledged...
        bus.tick_master(341 * 4);
        assert!(bus.irq_pending());
        // ...reading $4211 reports bit 7 and acks.
        assert_eq!(bus.read_u8(0x004211).unwrap() & 0x80, 0x80);
        assert!(!bus.irq_pending(), "reading $4211 must deassert the line");
        assert_eq!(bus.read_u8(0x004211).unwrap() & 0x80, 0x00, "flag reads clear");

        // Disabling both timer IRQs also acknowledges a pending one.
        let mut bus2 = SystemBus::new();
        bus2.write_u8(0x004209, 50).unwrap();
        bus2.write_u8(0x00420A, 0).unwrap();
        bus2.write_u8(0x004200, 0x20).unwrap();
        bus2.tick_master(60 * 341 * 4);
        assert!(bus2.irq_pending());
        bus2.write_u8(0x004200, 0x80).unwrap(); // NMI only, timer IRQs off
        assert!(!bus2.irq_pending(), "clearing $4200 bits 4-5 must ack a pending IRQ");
    }

    #[test]
    fn lorom_sram_at_bank_70_is_readable_writable_and_isolated_from_wram_and_stack() {
        // End-to-end regression test for the SaveTheGame crash: with a
        // LoROM cartridge that declares SRAM (like SMW's 2KB), writes to
        // $70:0000-$7FFF must land in real SRAM, read back correctly, and
        // leave WRAM (especially the $0100-$01FF stack page) untouched.
        let mut bus = SystemBus::new();
        bus.load_cartridge(build_lorom_with_sram()).unwrap();

        // Seed the stack page area that SaveTheGame's SRAM offsets used
        // to clobber ($01F5-$01F7 held a JSL return address).
        bus.write_u8(0x0001F5, 0x9B).unwrap();
        bus.write_u8(0x0001F6, 0x51).unwrap();
        bus.write_u8(0x0001F7, 0x00).unwrap();

        // Write a save-file-like run of bytes across the same offsets in
        // SRAM (this is exactly what `STA.L SaveData,X` does).
        for x in 0x01F0u32..0x0200 {
            bus.write_u8(0x700000 + x, 0x00).unwrap();
        }
        bus.write_u8(0x700000, 0x42).unwrap();

        // SRAM reads back what was written...
        assert_eq!(bus.read_u8(0x700000).unwrap(), 0x42);
        assert_eq!(bus.read_u8(0x7001F5).unwrap(), 0x00);
        // ...2KB SRAM mirrors across the 32KB window (partial decoding)...
        assert_eq!(bus.read_u8(0x700800).unwrap(), 0x42);
        // ...and the CPU stack bytes are exactly as seeded, NOT zeroed.
        assert_eq!(bus.read_u8(0x0001F5).unwrap(), 0x9B, "stack must survive SRAM writes");
        assert_eq!(bus.read_u8(0x0001F6).unwrap(), 0x51, "stack must survive SRAM writes");
        assert_eq!(bus.read_u8(0x0001F7).unwrap(), 0x00, "stack must survive SRAM writes");
    }

    /// Minimal valid LoROM image (with correct checksum fields) declaring
    /// 2KB of SRAM, mirroring what `Cartridge::new` needs to map SRAM.
    fn build_lorom_with_sram() -> Vec<u8> {
        let mut rom = vec![0u8; 0x8000];
        let h = 0x7FC0;
        rom[h..h + 21].copy_from_slice(b"SRAM TEST CART       ");
        rom[h + 0x15] = 0x20; // LoROM
        rom[h + 0x16] = 0x02; // ROM+RAM+battery
        rom[h + 0x17] = 0x08; // 256KB declared (code doesn't verify size here)
        rom[h + 0x18] = 0x01; // 2KB SRAM
        rom[h + 0x19] = 0x01; // region
        // checksum/complement: compute over the image with the fields
        // zeroed the way Cartridge::compute_checksum expects.
        rom[h + 0x1C] = 0xFF;
        rom[h + 0x1D] = 0xFF;
        rom[h + 0x1E] = 0x00;
        rom[h + 0x1F] = 0x00;
        let sum: u32 = rom.iter().map(|&b| b as u32).sum();
        let checksum = (sum & 0xFFFF) as u16;
        let complement = !checksum;
        rom[h + 0x1C] = (complement & 0xFF) as u8;
        rom[h + 0x1D] = (complement >> 8) as u8;
        rom[h + 0x1E] = (checksum & 0xFF) as u8;
        rom[h + 0x1F] = (checksum >> 8) as u8;
        rom
    }

    #[test]
    fn system_bus_wram_7e_and_7f_are_independent_not_mirrored() {
        // Banks $7E and $7F are the two contiguous 64KB halves of the same
        // 128KB WRAM chip, NOT mirrors of each other -- unlike the real
        // mirroring at $00-$3F/$80-$BF's $0000-$1FFF (which does alias the
        // low 8KB of $7E). Writing to one must not affect the other.
        let mut bus = SystemBus::new();

        bus.write_u8(0x7E8000, 0x12).unwrap();
        bus.write_u8(0x7F8000, 0x34).unwrap();

        assert_eq!(bus.read_u8(0x7E8000).unwrap(), 0x12);
        assert_eq!(bus.read_u8(0x7F8000).unwrap(), 0x34);
    }

    #[test]
    fn system_bus_open_bus() {
        let mut bus = SystemBus::new();
        
        // First read from unmapped area should return 0 (initial open-bus)
        let value = bus.read_u8(0x5000).unwrap();
        assert_eq!(value, 0x00, "Initial open bus should be 0");
        
        // Write something to WRAM - this updates open-bus
        bus.write_u8(0x7E0000, 0xAA).unwrap();
        
        // Read from WRAM - this should return value and update open-bus
        let read_value = bus.read_u8(0x7E0000).unwrap();
        assert_eq!(read_value, 0xAA, "WRAM read should return written value");
        
        // Now read from unmapped area - should return last value (open-bus behavior)
        let value = bus.read_u8(0x5000).unwrap();
        assert_eq!(value, 0xAA, "Open bus should return last read value");
    }

    #[test]
    fn system_bus_rom_read() {
        let mut bus = SystemBus::new();
        
        // Create a ROM (2MB) that will definitely be HiROM
        let mut rom = vec![0x00; 0x200000];
        // Fill ROM with known pattern
        for i in 0..rom.len() {
            rom[i] = (i & 0xFF) as u8;
        }
        // Set HiROM mode byte at header position
        rom[0xFFD5] = 0x01; // Set bit 0 for HiROM
        
        bus.load_cartridge(rom).unwrap();
        
        // Verify cartridge is loaded
        assert!(bus.has_cartridge());
        
        // In HiROM: bank $C0 with offset 0x0000 maps to ROM offset 0x0000
        // 0xC00000 = bank 0xC0, offset 0x0000
        // ROM addr = ((0xC0 & 0x3F) * 0x10000) + 0 = 0x40 * 0x10000 = 0x400000
        // But ROM is only 0x200000, so this should wrap or be invalid
        // Let's try a valid offset
        
        // For HiROM: 0xC00000 maps to ROM offset 0, 0xC10000 maps to ROM offset 0x10000, etc.
        // Let's use offset 0x8000 in bank $C0 which should map to ROM offset 0x8000
        let value = bus.read_u8(0xC08000).unwrap();
        assert_eq!(value, 0x00, "HiROM read should return pattern at offset 0x8000");
    }

    #[test]
    fn system_bus_io_stub() {
        let mut bus = SystemBus::new();
        
        // Read from I/O register area ($2100-$21FF)
        let value = bus.read_u8(0x2100).unwrap();
        assert_eq!(value, 0x00);
    }

    #[test]
    fn system_bus_read_u16() {
        let mut bus = SystemBus::new();

        // Write two bytes to WRAM
        bus.write_u8(0x7E1000, 0x12).unwrap();
        bus.write_u8(0x7E1001, 0x34).unwrap();

        // Read as u16 (little-endian)
        let value = bus.read_u16(0x7E1000).unwrap();
        assert_eq!(value, 0x3412);
    }

    #[test]
    fn vram_write_via_2118_2119_lands_at_word_address_times_two() {
        let mut bus = SystemBus::new();
        // VMAIN = 0 (increment by 1 word, after low-byte write)
        bus.write_u8(0x002115, 0x00).unwrap();
        // VMADD = $0010
        bus.write_u8(0x002116, 0x10).unwrap();
        bus.write_u8(0x002117, 0x00).unwrap();
        bus.write_u8(0x002118, 0xAB).unwrap(); // low byte -- should also auto-increment
        bus.write_u8(0x002119, 0xCD).unwrap(); // high byte of the NEXT word now ($0011)

        assert_eq!(bus.ppu_ref().vram_ref().read(0x0020), 0xAB, "low byte of word $0010");
        assert_eq!(bus.ppu_ref().vram_ref().read(0x0023), 0xCD, "high byte of word $0011, after auto-increment");
    }

    #[test]
    fn vram_write_does_not_increment_until_high_byte_when_vmain_bit7_set() {
        let mut bus = SystemBus::new();
        bus.write_u8(0x002115, 0x80).unwrap(); // increment after high-byte write
        bus.write_u8(0x002116, 0x00).unwrap();
        bus.write_u8(0x002117, 0x00).unwrap();
        bus.write_u8(0x002118, 0x11).unwrap(); // low byte: must NOT increment yet
        bus.write_u8(0x002119, 0x22).unwrap(); // high byte: increments after this

        // Both bytes belong to word $0000 (addresses 0,1), confirming the
        // address didn't advance between the low and high writes.
        assert_eq!(bus.ppu_ref().vram_ref().read(0x0000), 0x11);
        assert_eq!(bus.ppu_ref().vram_ref().read(0x0001), 0x22);
    }

    #[test]
    fn cgram_write_pairs_low_then_high_byte_and_advances_on_second_write() {
        let mut bus = SystemBus::new();
        bus.write_u8(0x002121, 0x05).unwrap(); // CGADD = color index 5
        bus.write_u8(0x002122, 0x34).unwrap(); // low byte of color 5
        bus.write_u8(0x002122, 0x12).unwrap(); // high byte of color 5, advances CGADD to 6
        bus.write_u8(0x002122, 0x78).unwrap(); // low byte of color 6

        assert_eq!(bus.ppu_ref().cgram_ref().read(10), 0x34);
        assert_eq!(bus.ppu_ref().cgram_ref().read(11), 0x12);
        assert_eq!(bus.ppu_ref().cgram_ref().read(12), 0x78);
    }

    #[test]
    fn oam_write_pairs_low_then_high_byte() {
        let mut bus = SystemBus::new();
        bus.write_u8(0x002102, 0x00).unwrap(); // OAMADDL = 0, resets toggle
        bus.write_u8(0x002103, 0x00).unwrap();
        bus.write_u8(0x002104, 0xAA).unwrap(); // sprite 0 Y
        bus.write_u8(0x002104, 0xBB).unwrap(); // sprite 0 X

        assert_eq!(bus.ppu_ref().oam_ref().read(0), 0xAA);
        assert_eq!(bus.ppu_ref().oam_ref().read(1), 0xBB);
    }

    #[test]
    fn dma_mode1_transfer_uploads_real_rom_bytes_into_vram() {
        let mut bus = SystemBus::new();
        let mut rom = vec![0u8; 0x80000];
        // Distinctive payload at LoROM bank 0, $8000+ -- 4 bytes that must
        // end up in VRAM byte-for-byte if the transfer is wired correctly.
        rom[0x0000] = 0x11;
        rom[0x0001] = 0x22;
        rom[0x0002] = 0x33;
        rom[0x0003] = 0x44;
        bus.load_cartridge(rom).unwrap();

        // Target VRAM address $0000, increment by 1 word after high-byte write.
        bus.write_u8(0x002115, 0x80).unwrap();
        bus.write_u8(0x002116, 0x00).unwrap();
        bus.write_u8(0x002117, 0x00).unwrap();

        // DMA channel 0: mode 1 (word, alternates $2118/$2119), CPU->PPU,
        // dest BBAD=$18 (so $2118 then $2119), source = bank $00:$8000, 4 bytes.
        bus.write_u8(0x004300, 0x01).unwrap(); // DMAPx: mode 1, direction CPU->PPU
        bus.write_u8(0x004301, 0x18).unwrap(); // BBADx = $18 (VMDATAL)
        bus.write_u8(0x004302, 0x00).unwrap(); // A1Tx low
        bus.write_u8(0x004303, 0x80).unwrap(); // A1Tx high ($8000)
        bus.write_u8(0x004304, 0x00).unwrap(); // A1Bx = bank 0
        bus.write_u8(0x004305, 0x04).unwrap(); // DASx low = 4 bytes
        bus.write_u8(0x004306, 0x00).unwrap(); // DASx high

        bus.write_u8(0x00420B, 0x01).unwrap(); // MDMAEN: trigger channel 0

        assert_eq!(bus.ppu_ref().vram_ref().read(0x0000), 0x11);
        assert_eq!(bus.ppu_ref().vram_ref().read(0x0001), 0x22);
        assert_eq!(bus.ppu_ref().vram_ref().read(0x0002), 0x33);
        assert_eq!(bus.ppu_ref().vram_ref().read(0x0003), 0x44);

        // DAS must read back as 0 (transfer complete) per real hardware.
        assert_eq!(bus.read_u8(0x004305).unwrap(), 0x00);
        assert_eq!(bus.read_u8(0x004306).unwrap(), 0x00);
    }

    #[test]
    fn dma_with_zero_das_transfers_a_full_64kb_block() {
        // Documented real-hardware behavior: DAS=0 means 0x10000 bytes,
        // not "nothing to transfer" -- games rely on this for full-VRAM
        // clears/fills using a single DMA trigger. Uses a fixed source
        // address (the real pattern for memory-clear/fill DMA) so the
        // 65536-byte transfer doesn't depend on how a single ROM bank's
        // address space is carved up between ROM and WRAM mirror/I-O.
        let mut bus = SystemBus::new();
        let rom = vec![0x7Eu8; 0x80000];
        bus.load_cartridge(rom).unwrap();

        bus.write_u8(0x002115, 0x80).unwrap();
        bus.write_u8(0x002116, 0x00).unwrap();
        bus.write_u8(0x002117, 0x00).unwrap();

        bus.write_u8(0x004300, 0x11).unwrap(); // mode 1, fixed source address
        bus.write_u8(0x004301, 0x18).unwrap();
        bus.write_u8(0x004302, 0x00).unwrap();
        bus.write_u8(0x004303, 0x80).unwrap();
        bus.write_u8(0x004304, 0x00).unwrap();
        bus.write_u8(0x004305, 0x00).unwrap(); // DAS = 0 -> 65536 bytes
        bus.write_u8(0x004306, 0x00).unwrap();

        bus.write_u8(0x00420B, 0x01).unwrap();

        // Every VRAM byte should now be 0x7E (all 65536 bytes transferred).
        assert_eq!(bus.ppu_ref().vram_ref().read(0x0000), 0x7E);
        assert_eq!(bus.ppu_ref().vram_ref().read(0xFFFF), 0x7E);
    }

    // ========================================================================
    // HDMA tests
    // ========================================================================

    /// Ticks the bus through exactly one edge transition (vblank-exit, or
    /// one scanline's hblank-entry), landing safely past the boundary
    /// rather than exactly on it, without overshooting into the *next*
    /// edge -- `tick_ppu` only compares state once per call (start vs.
    /// end), so crossing more than one edge in a single call would hide
    /// intermediate ones (see `tick_past_one_vblank_entry`'s doc comment
    /// for the same caveat).
    fn tick_dots(bus: &mut SystemBus, dots: u32) {
        // 4 master cycles per dot -- dot-granular, so odd counts (a real
        // scanline is 341 dots) advance exactly.
        bus.tick_master(dots * 4);
    }

    #[test]
    fn oam_address_reloads_from_the_2102_latch_at_every_vblank_start() {
        // Real hardware re-applies the last $2102/$2103 value to the live
        // OAM address at the start of each vblank (unless forced blank).
        // DKC sets OAMADD=0 once and then relies on this auto-reload for
        // its every-frame 544-byte OAM DMA; without it, the live address
        // marched +0x110 words per frame past the end of OAM and no
        // sprite upload ever landed again -- gameplay rendered with no
        // sprites at all (no player, no enemies).
        let mut bus = SystemBus::new();
        bus.write_u8(0x002100, 0x0F).unwrap(); // screen on (reload is gated on !forced-blank)
        bus.write_u8(0x002102, 0x00).unwrap();
        bus.write_u8(0x002103, 0x00).unwrap();

        // Consume two full words (the low table commits word-at-a-time
        // through the $2104 write latch), leaving the live address at
        // word 2.
        bus.write_u8(0x002104, 0x11).unwrap();
        bus.write_u8(0x002104, 0x22).unwrap();
        bus.write_u8(0x002104, 0x33).unwrap();
        bus.write_u8(0x002104, 0x44).unwrap();
        assert_eq!(bus.ppu_ref().oam_ref().read(0), 0x11);
        assert_eq!(bus.ppu_ref().oam_ref().read(2), 0x33);

        // Cross one vblank-entry edge WITHOUT touching $2102/$2103.
        tick_dots(&mut bus, 230 * 341);

        // The next writes must land back at word 0 (and with the byte
        // toggle reset), exactly as if software had rewritten OAMADD.
        bus.write_u8(0x002104, 0xAA).unwrap();
        bus.write_u8(0x002104, 0xBB).unwrap();
        assert_eq!(bus.ppu_ref().oam_ref().read(0), 0xAA, "low byte of word 0 -- the vblank reload must reset the live address to the $2102/$2103 latch");
        assert_eq!(bus.ppu_ref().oam_ref().read(1), 0xBB, "high byte of word 0");
        assert_eq!(bus.ppu_ref().oam_ref().read(2), 0x33, "word 1 must be untouched by the post-reload writes");
    }

    #[test]
    fn oam_address_does_not_reload_during_forced_blank() {
        // The vblank auto-reload is suppressed while INIDISP bit 7 (forced
        // blank) is set -- writes keep streaming from wherever the live
        // address is, which is exactly what boot-time OAM-clear loops that
        // span several (blanked) frames rely on.
        let mut bus = SystemBus::new();
        bus.write_u8(0x002100, 0x8F).unwrap(); // forced blank ON
        bus.write_u8(0x002102, 0x00).unwrap();
        bus.write_u8(0x002103, 0x00).unwrap();

        bus.write_u8(0x002104, 0x11).unwrap();
        bus.write_u8(0x002104, 0x22).unwrap();

        tick_dots(&mut bus, 230 * 341); // vblank entry while blanked: no reload

        bus.write_u8(0x002104, 0x33).unwrap();
        bus.write_u8(0x002104, 0x44).unwrap();
        assert_eq!(bus.ppu_ref().oam_ref().read(0), 0x11, "word 0 must NOT be overwritten -- no reload happened");
        assert_eq!(bus.ppu_ref().oam_ref().read(2), 0x33, "the stream must continue at word 1");
        assert_eq!(bus.ppu_ref().oam_ref().read(3), 0x44);
    }

    #[test]
    fn hdma_direct_mode_non_repeat_entry_writes_once_then_waits_without_touching_the_bbus() {
        // Regression guard for the DKC intro table-desync bug: a
        // non-repeat entry (bit7 CLEAR, count $01-$80) transfers on its
        // FIRST line only -- the remaining "wait" lines perform no B-bus
        // writes at all -- and the table pointer must end up past the
        // entry's inline data so the next line-count read lands on the
        // real next entry, not on data bytes. The old engine transferred
        // on every line and never advanced the pointer, so tables like
        // DKC's `7F 03 18 03 03 03 00` slid out of sync the moment the
        // first entry expired (count bytes written to the PPU as data,
        // data bytes consumed as counts).
        let mut bus = SystemBus::new();

        // HDMA table in WRAM at $7E:2000: one non-repeat entry (bit7
        // clear) covering 2 lines with a single data byte 0xAA, then the
        // 0x00 end-of-table marker.
        bus.write_u8(0x7E2000, 0x02).unwrap(); // line-count=2, non-repeat
        bus.write_u8(0x7E2001, 0xAA).unwrap(); // the entry's single data byte
        bus.write_u8(0x7E2002, 0x00).unwrap(); // end of table

        // VRAM address 0, increment by 1 word after each low-byte write
        // (VMAIN default 0 already does this).
        bus.write_u8(0x002116, 0x00).unwrap();
        bus.write_u8(0x002117, 0x00).unwrap();

        // DMA channel 0: direct-addressing HDMA (DMAPx bit6=0), mode 0 (1
        // byte/line) into $2118 (VMDATAL), table starting at $7E:2000.
        bus.write_u8(0x004300, 0x00).unwrap();
        bus.write_u8(0x004301, 0x18).unwrap();
        bus.write_u8(0x004302, 0x00).unwrap();
        bus.write_u8(0x004303, 0x20).unwrap();
        bus.write_u8(0x004304, 0x7E).unwrap();

        // Arm HDMA for channel 0 via $420C -- during vblank, like real
        // games do (arming mid-frame runs the engine with uninitialized
        // channel state on real hardware too).
        tick_dots(&mut bus, 230 * 341); // into vblank (scanline 230)
        bus.write_u8(0x00420C, 0x01).unwrap();

        // Before any HDMA has run, VRAM must still be untouched.
        assert_eq!(bus.ppu_ref().vram_ref().read(0x0000), 0x00);

        // The frame cycle starts on the LAST internal scanline (hardware
        // V=0): crossing into it runs hdma_init, and its ~dot-276 HDMA
        // slot performs the fresh entry's first transfer.
        tick_dots(&mut bus, 31 * 341 + 100); // scanline 261, dot 100: init has run, transfer hasn't
        assert_eq!(bus.ppu_ref().vram_ref().read(0x0000), 0x00, "init alone must not transfer");
        tick_dots(&mut bus, 300); // cross dot 276 of scanline 261: the entry's one transfer

        assert_eq!(bus.ppu_ref().vram_ref().read(0x0000), 0xAA, "the entry's first line must write the table's data byte");

        tick_dots(&mut bus, 341); // cross scanline 0's HDMA slot: WAIT line (count 2->0 exhausts, reload reads 0x00 -> terminates)

        assert_eq!(
            bus.ppu_ref().vram_ref().read(0x0002),
            0x00,
            "a non-repeat entry's wait lines must not write the B-bus at all (the old engine re-wrote every line)"
        );

        tick_dots(&mut bus, 341); // cross scanline 1's HDMA slot: channel is terminated, must not transfer again

        assert_eq!(bus.ppu_ref().vram_ref().read(0x0002), 0x00, "a terminated channel must not keep transferring into subsequent scanlines");
        assert!(
            bus.dma_ref().channel(0).unwrap().hdma_terminated,
            "after the wait line, the reload must read the end-of-table marker (0x00) -- not the entry's own data byte -- and terminate"
        );
    }

    #[test]
    fn hdma_direct_mode_repeat_entry_streams_fresh_data_each_line() {
        let mut bus = SystemBus::new();

        // Repeat entry (bit7 SET, $81-$FF): transfers on EVERY line of the
        // entry, consuming fresh data bytes from the table each line --
        // line-count=2 with 2 lines' worth of distinct data (0x11, 0x22)
        // following, then end-of-table.
        bus.write_u8(0x7E3000, 0x82).unwrap(); // 0x80 | 2 = repeat, 2 lines
        bus.write_u8(0x7E3001, 0x11).unwrap();
        bus.write_u8(0x7E3002, 0x22).unwrap();
        bus.write_u8(0x7E3003, 0x00).unwrap();

        bus.write_u8(0x002116, 0x00).unwrap();
        bus.write_u8(0x002117, 0x00).unwrap();

        bus.write_u8(0x004300, 0x00).unwrap();
        bus.write_u8(0x004301, 0x18).unwrap();
        bus.write_u8(0x004302, 0x00).unwrap();
        bus.write_u8(0x004303, 0x30).unwrap();
        bus.write_u8(0x004304, 0x7E).unwrap();

        tick_dots(&mut bus, 230 * 341); // into vblank, then arm
        bus.write_u8(0x00420C, 0x01).unwrap();

        tick_dots(&mut bus, 32 * 341); // crosses init + the pre-visible line's HDMA slot (1st transfer)
        assert_eq!(bus.ppu_ref().vram_ref().read(0x0000), 0x11);

        tick_dots(&mut bus, 341); // scanline 0's HDMA slot: 2nd line of the repeat entry
        assert_eq!(bus.ppu_ref().vram_ref().read(0x0002), 0x22, "a repeat entry must advance to the next table byte for each line");
    }

    #[test]
    fn hdma_multi_byte_transfer_source_wraps_within_bank_not_into_next_bank() {
        // Mode 1 transfers 2 bytes/line (into $2118 then $2119). Set up a
        // "no-repeat" entry whose data bytes straddle the $7E/$7F bank
        // boundary: line-count at $7E:FFFE, first data byte at $7E:FFFF
        // (the last address in bank $7E), second data byte that MUST be
        // re-read from $7E:0000 (wrapping within the same bank, matching
        // real hardware and every other address-stepping path in this
        // file) rather than carrying into $7F:0000.
        let mut bus = SystemBus::new();

        bus.write_u8(0x7EFFFE, 0x81).unwrap(); // no-repeat, 1 line
        bus.write_u8(0x7EFFFF, 0x11).unwrap(); // 1st data byte (low, ->$2118)
        bus.write_u8(0x7E0000, 0x22).unwrap(); // 2nd data byte, correct same-bank wrap (->$2119)
        bus.write_u8(0x7F0000, 0xFF).unwrap(); // decoy: what the old 24-bit-carry bug would read instead

        // VMAIN = increment after the HIGH byte write, so both $2118 and
        // $2119 land on the SAME VRAM word (0) instead of the low-byte
        // write auto-advancing VMADD before the high byte is written.
        bus.write_u8(0x002115, 0x80).unwrap();
        bus.write_u8(0x002116, 0x00).unwrap();
        bus.write_u8(0x002117, 0x00).unwrap();

        bus.write_u8(0x004300, 0x01).unwrap(); // DMAPx: direct HDMA, mode 1 (2 bytes/line)
        bus.write_u8(0x004301, 0x18).unwrap(); // BBADx = $18 (VMDATAL/VMDATAH)
        bus.write_u8(0x004302, 0xFE).unwrap(); // A1T low = $FFFE
        bus.write_u8(0x004303, 0xFF).unwrap(); // A1T high
        bus.write_u8(0x004304, 0x7E).unwrap(); // A1B = bank $7E

        tick_dots(&mut bus, 230 * 341); // into vblank, then arm channel 0
        bus.write_u8(0x00420C, 0x01).unwrap();

        tick_dots(&mut bus, 32 * 341); // crosses init + the pre-visible line's HDMA slot: transfers both bytes

        assert_eq!(bus.ppu_ref().vram_ref().read(0x0000), 0x11, "1st byte, read from $7E:FFFF");
        assert_eq!(
            bus.ppu_ref().vram_ref().read(0x0001), 0x22,
            "2nd byte must wrap to $7E:0000 (same bank), not carry into $7F:0000 (which would read the 0xFF decoy)"
        );
    }

    // ========================================================================
    // Joypad input tests
    // ========================================================================

    /// Ticks the bus forward to land just inside vblank (NTSC: scanline 224
    /// of 262, 341 dots/line), which is what actually latches the
    /// auto-joypad-read result into $4218/$4219 (see `tick_ppu`). Must not
    /// overshoot past scanline 262 back into the next frame's active
    /// scanlines, since `tick_ppu` only compares vblank state once per
    /// call (before vs. after the whole batch), not per-scanline within it.
    fn tick_past_one_vblank_entry(bus: &mut SystemBus) {
        const DOTS_TO_MIDDLE_OF_VBLANK: u32 = 230 * 341; // scanline 230, safely within 224-261
        bus.tick_ppu(DOTS_TO_MIDDLE_OF_VBLANK / 2); // tick_ppu doubles cycles to dots
    }

    #[test]
    fn auto_joypad_read_reports_zero_before_being_enabled() {
        let mut bus = SystemBus::new();
        bus.set_joypad1_state(0xFFFF);
        tick_past_one_vblank_entry(&mut bus);

        // $4200 bit0 (auto-read enable) was never set, so the vblank-entry
        // latch in `tick_ppu` must not have copied the live state in.
        assert_eq!(bus.read_u8(0x004218).unwrap(), 0x00);
        assert_eq!(bus.read_u8(0x004219).unwrap(), 0x00);
    }

    #[test]
    fn auto_joypad_read_latches_live_state_at_vblank_entry() {
        let mut bus = SystemBus::new();
        bus.write_u8(0x004200, 0x01).unwrap(); // NMITIMEN bit0: enable auto-read

        // Press Start + Right (bit12 and bit8 in the SNES auto-read layout).
        bus.set_joypad1_state(0x1100);
        tick_past_one_vblank_entry(&mut bus);

        assert_eq!(bus.read_u8(0x004218).unwrap(), 0x00, "A/X/L/R byte: none of those pressed");
        assert_eq!(bus.read_u8(0x004219).unwrap(), 0x11, "Start (d4) and Right (d0) set");
    }

    #[test]
    fn auto_joypad_read_maps_every_button_to_its_documented_bit() {
        // Cross-checks the full $4218/$4219 bit layout against the
        // documented SNES auto-read format (wiki.superfamicom.org):
        // $4218 d7=A d6=X d5=L d4=R; $4219 d7=B d6=Y d5=Select d4=Start
        // d3=Up d2=Down d1=Left d0=Right.
        let cases: &[(u16, u16, u8)] = &[
            (0x8000, 0x4219, 0x80), // B
            (0x4000, 0x4219, 0x40), // Y
            (0x2000, 0x4219, 0x20), // Select
            (0x1000, 0x4219, 0x10), // Start
            (0x0800, 0x4219, 0x08), // Up
            (0x0400, 0x4219, 0x04), // Down
            (0x0200, 0x4219, 0x02), // Left
            (0x0100, 0x4219, 0x01), // Right
            (0x0080, 0x4218, 0x80), // A
            (0x0040, 0x4218, 0x40), // X
            (0x0020, 0x4218, 0x20), // L
            (0x0010, 0x4218, 0x10), // R
        ];

        for &(snes_bits, reg_addr, expected) in cases {
            let mut bus = SystemBus::new();
            bus.write_u8(0x004200, 0x01).unwrap();
            bus.set_joypad1_state(snes_bits);
            tick_past_one_vblank_entry(&mut bus);
            assert_eq!(
                bus.read_u8(0x000000 | reg_addr as u32).unwrap(),
                expected,
                "button bits {:#06X} must set exactly {:#04X} at ${:04X}",
                snes_bits, expected, reg_addr
            );
        }
    }

    #[test]
    fn auto_joypad_read_does_not_update_mid_frame() {
        // The latch only happens on the vblank-entry edge, not live on
        // every read -- pressing a button mid-frame (after the last latch)
        // must not be visible until the next vblank entry.
        let mut bus = SystemBus::new();
        bus.write_u8(0x004200, 0x01).unwrap();
        bus.set_joypad1_state(0x8000); // B held during the first vblank
        tick_past_one_vblank_entry(&mut bus);
        assert_eq!(bus.read_u8(0x004219).unwrap(), 0x80);

        // Change input mid-frame without crossing another vblank entry.
        bus.set_joypad1_state(0x0000);
        assert_eq!(
            bus.read_u8(0x004219).unwrap(),
            0x80,
            "must still report the last-latched value until the next vblank"
        );
    }

    #[test]
    fn manual_joypad_strobe_shifts_out_bits_msb_first() {
        // Real controllers shift out B,Y,Select,Start,Up,Down,Left,Right,
        // A,X,L,R,0,0,0,0 -- MSB (B) first -- via the $4016 strobe/serial
        // protocol, independent of the auto-read mechanism.
        let mut bus = SystemBus::new();
        // B (bit15) and A (bit7) pressed.
        bus.set_joypad1_state(0x8080);

        // Strobe high then low to latch the snapshot.
        bus.write_u8(0x004016, 0x01).unwrap();
        bus.write_u8(0x004016, 0x00).unwrap();

        let mut bits = Vec::new();
        for _ in 0..16 {
            bits.push(bus.read_u8(0x004016).unwrap() & 0x01);
        }

        let expected = [
            1, 0, 0, 0, 0, 0, 0, 0, // B,Y,Select,Start,Up,Down,Left,Right
            1, 0, 0, 0, 0, 0, 0, 0, // A,X,L,R,0,0,0,0
        ];
        assert_eq!(bits, expected, "bits must shift out MSB-first matching real controller order");

        // A standard controller with nothing chained behind it reports 1
        // (pulled high) for any further reads past the 16 real bits.
        assert_eq!(bus.read_u8(0x004016).unwrap() & 0x01, 1);
        assert_eq!(bus.read_u8(0x004016).unwrap() & 0x01, 1);
    }

    #[test]
    fn manual_joypad_read_while_strobe_high_always_reports_first_bit() {
        // While strobe is held high, the register continuously reflects
        // the live state's first bit (B) rather than shifting -- matching
        // real hardware, which keeps re-latching as long as strobe is 1.
        let mut bus = SystemBus::new();
        bus.write_u8(0x004016, 0x01).unwrap(); // strobe high

        bus.set_joypad1_state(0x8000); // B pressed
        assert_eq!(bus.read_u8(0x004016).unwrap() & 0x01, 1);
        assert_eq!(bus.read_u8(0x004016).unwrap() & 0x01, 1, "must not advance/shift while strobe is high");

        bus.set_joypad1_state(0x0000); // B released, still strobing
        assert_eq!(bus.read_u8(0x004016).unwrap() & 0x01, 0, "must reflect the live state, not a stale latch");
    }

    #[test]
    fn joyser1_reads_zero_before_any_strobe_regardless_of_controller_state() {
        // Un-strobed $4017 reads must keep the serial DATA bit (bit 0) at
        // the deliberately safe 0 -- an always-1 ("pulled high"/no
        // controller) stub was tried and caused a real boot-time
        // regression in the real ROM (see the $4017 read handler). The
        // hardwired bits 4-2 (always 1 on port 2) and the open-bus high
        // bits are real hardware behavior and stay.
        let mut bus = SystemBus::new();
        bus.set_joypad1_state(0xFFFF);
        bus.set_joypad2_state(0xFFFF);
        let value = bus.read_u8(0x004017).unwrap();
        assert_eq!(value & 0x03, 0x00, "the un-strobed data bits must read 0");
        assert_eq!(value & 0x1C, 0x1C, "port 2 hardwires bits 4-2 high");
    }

    #[test]
    fn joypad2_serial_read_shifts_out_its_own_snapshot_after_the_shared_strobe() {
        let mut bus = SystemBus::new();
        bus.set_joypad1_state(0x0000); // controller 1: nothing pressed
        bus.set_joypad2_state(0x8010); // controller 2: B (bit15) + R (bit4)

        // One strobe cycle on the shared $4016 line latches BOTH ports.
        bus.write_u8(0x004016, 0x01).unwrap();
        bus.write_u8(0x004016, 0x00).unwrap();

        let mut joy2_bits = Vec::new();
        for _ in 0..16 {
            joy2_bits.push(bus.read_u8(0x004017).unwrap() & 1);
        }
        let mut expected = vec![0u8; 16];
        expected[0] = 1; // B (bit15, shifted out first)
        expected[11] = 1; // R (bit4)
        assert_eq!(joy2_bits, expected, "$4017 must shift controller 2's own snapshot");
        // Controller 1's shift register must be untouched by $4017 reads.
        assert_eq!(bus.read_u8(0x004016).unwrap() & 1, 0, "controller 1 stream must be independent");
        // Past 16 bits, a connected controller reports 1 (no more data).
        assert_eq!(bus.read_u8(0x004017).unwrap() & 1, 1);
    }

    #[test]
    fn joypad2_auto_read_latches_at_vblank_into_421a_421b() {
        let mut bus = SystemBus::new();
        bus.write_u8(0x004200, 0x01).unwrap(); // auto-joypad-read enable
        bus.set_joypad2_state(0x8010); // B + R
        tick_dots(&mut bus, 225 * 341); // cross the vblank-entry edge
        assert_eq!(bus.read_u8(0x00421A).unwrap(), 0x10, "JOY2L must hold the low byte of the latch");
        assert_eq!(bus.read_u8(0x00421B).unwrap(), 0x80, "JOY2H must hold the high byte of the latch");
    }

    #[test]
    fn manual_joypad_strobe_snapshots_state_at_the_falling_edge_not_the_rising_edge() {
        // The latch must happen when strobe transitions from high to low,
        // using whatever the live state is AT THAT MOMENT -- previously it
        // snapshotted on the RISING edge instead, so a button pressed while
        // strobe was already held high (a common polling pattern) would be
        // missed entirely.
        let mut bus = SystemBus::new();
        bus.set_joypad1_state(0x0000);
        bus.write_u8(0x004016, 0x01).unwrap(); // strobe high; live state is 0 right now

        // Change the live state while strobe is still asserted.
        bus.set_joypad1_state(0x8000); // B pressed

        bus.write_u8(0x004016, 0x00).unwrap(); // falling edge: must snapshot THIS state

        assert_eq!(
            bus.read_u8(0x004016).unwrap() & 0x01,
            1,
            "falling-edge snapshot must reflect the state at the falling edge, not the rising edge"
        );
    }

    // ========================================================================
    // VRAM/OAM/CGRAM readback register tests
    // ========================================================================

    #[test]
    fn oam_read_via_2138_round_trips_after_write_and_auto_increments() {
        let mut bus = SystemBus::new();
        bus.write_u8(0x002102, 0x00).unwrap(); // OAMADDL = 0, resets toggle
        bus.write_u8(0x002103, 0x00).unwrap();
        bus.write_u8(0x002104, 0xAA).unwrap(); // sprite 0 Y
        bus.write_u8(0x002104, 0xBB).unwrap(); // sprite 0 X

        // Reset OAMADD/toggle back to the start to read back what was written.
        bus.write_u8(0x002102, 0x00).unwrap();
        bus.write_u8(0x002103, 0x00).unwrap();
        assert_eq!(bus.read_u8(0x002138).unwrap(), 0xAA, "low byte (Y) of sprite 0");
        assert_eq!(bus.read_u8(0x002138).unwrap(), 0xBB, "high byte (X), after auto-increment");
    }

    #[test]
    fn vram_read_via_2139_213a_round_trips_after_write_and_auto_increments() {
        let mut bus = SystemBus::new();
        bus.write_u8(0x002115, 0x80).unwrap(); // increment after high-byte access
        bus.write_u8(0x002116, 0x10).unwrap();
        bus.write_u8(0x002117, 0x00).unwrap();
        bus.write_u8(0x002118, 0xAB).unwrap();
        bus.write_u8(0x002119, 0xCD).unwrap();

        // Point VMADD back at word $0010 to read back what was written.
        bus.write_u8(0x002116, 0x10).unwrap();
        bus.write_u8(0x002117, 0x00).unwrap();
        assert_eq!(bus.read_u8(0x002139).unwrap(), 0xAB, "low byte read back");
        assert_eq!(bus.read_u8(0x00213A).unwrap(), 0xCD, "high byte read back");

        // VMADD must have auto-incremented to word $0011 after the
        // high-byte read (same VMAIN-driven timing as the write side).
        bus.write_u8(0x002118, 0x11).unwrap();
        bus.write_u8(0x002119, 0x22).unwrap();
        assert_eq!(bus.ppu_ref().vram_ref().read(0x0022), 0x11, "word $0011 low byte, confirming VMADD advanced");
        assert_eq!(bus.ppu_ref().vram_ref().read(0x0023), 0x22);
    }

    #[test]
    fn cgram_read_via_213b_round_trips_after_write_and_auto_increments() {
        let mut bus = SystemBus::new();
        bus.write_u8(0x002121, 0x05).unwrap(); // CGADD = color index 5
        bus.write_u8(0x002122, 0x34).unwrap(); // low byte of color 5
        bus.write_u8(0x002122, 0x12).unwrap(); // high byte of color 5, advances CGADD to 6

        bus.write_u8(0x002121, 0x05).unwrap(); // back to color 5, resets toggle
        assert_eq!(bus.read_u8(0x00213B).unwrap(), 0x34, "low byte of color 5");
        assert_eq!(bus.read_u8(0x00213B).unwrap(), 0x12, "high byte of color 5");

        // CGADD must have auto-incremented to color 6 (never written -> 0).
        assert_eq!(bus.read_u8(0x00213B).unwrap(), 0x00, "low byte of color 6");
    }

    // ========================================================================
    // DMA/HDMA real transfer-state flag tests
    // ========================================================================

    #[test]
    fn dma_transfer_sets_done_flag_and_clears_active_flag_when_complete() {
        // `Dma::is_active()`/`check_done()` used to never be touched by
        // `execute_dma_channel`, so they permanently reported "never
        // active, never done" no matter what transfers actually ran.
        let mut bus = SystemBus::new();
        bus.write_u8(0x7E0010, 0x5A).unwrap();

        bus.write_u8(0x004300, 0x08).unwrap(); // fixed source, mode 0
        bus.write_u8(0x004301, 0x18).unwrap();
        bus.write_u8(0x004302, 0x10).unwrap();
        bus.write_u8(0x004303, 0x00).unwrap();
        bus.write_u8(0x004304, 0x7E).unwrap();
        bus.write_u8(0x004305, 0x04).unwrap();
        bus.write_u8(0x004306, 0x00).unwrap();

        assert!(!bus.dma_ref().check_done(), "no transfer has run yet");

        bus.write_u8(0x00420B, 0x01).unwrap(); // fire channel 0

        assert!(bus.dma_ref().check_done(), "channel must report done once its transfer completes");
        assert!(!bus.dma_ref().is_active(), "dma_active must be cleared once the (synchronous) transfer finishes");
    }

    #[test]
    fn immediate_dma_transfer_advances_ppu_and_apu_by_bytes_times_eight_master_cycles() {
        // `execute_dma_channel` used to advance no CPU/PPU/APU cycle count
        // at all -- as if the whole multi-byte transfer took zero time.
        // Real hardware costs 8 MASTER cycles/byte plus a small per-channel
        // setup cost (~8 master cycles); at the exact 4-master-cycles-per-
        // dot rate that's 2 dots per byte. (An intermediate version charged
        // 8 *CPU cycles* per byte through the fixed 2-dots/cycle path -- 8x
        // the real dot cost.)
        let mut bus = SystemBus::new();
        bus.write_u8(0x7E0010, 0x5A).unwrap();

        bus.write_u8(0x004300, 0x08).unwrap(); // fixed source, mode 0
        bus.write_u8(0x004301, 0x18).unwrap();
        bus.write_u8(0x004302, 0x10).unwrap();
        bus.write_u8(0x004303, 0x00).unwrap();
        bus.write_u8(0x004304, 0x7E).unwrap();
        bus.write_u8(0x004305, 0x04).unwrap(); // DAS = 4 bytes
        bus.write_u8(0x004306, 0x00).unwrap();

        let h_before = bus.ppu_ref().h_counter();
        bus.write_u8(0x00420B, 0x01).unwrap(); // fire channel 0: 4 bytes

        // 18 master cycles of $420B CPU<->DMA clock sync (snes9x
        // `Timings.DMACPUSync`) + 8 per-channel setup + 4 bytes * 8.
        let expected_master = 18u32 + 8 + 4 * 8;
        let expected_dots = expected_master / 4; // 4 master cycles per dot
        let h_after = bus.ppu_ref().h_counter();
        assert_eq!(
            (h_after as u32 + 341 - h_before as u32) % 341,
            expected_dots % 341,
            "a 4-byte DMA transfer must advance the PPU by (18 + 8 + 4*8)/4 = {} dots, not zero",
            expected_dots
        );
    }

    #[test]
    fn bus_accesses_accumulate_real_per_region_master_cycle_costs() {
        let mut bus = SystemBus::new();
        bus.take_step_access_costs(); // clear

        let _ = bus.read_u8(0x7E0000).unwrap(); // WRAM: 8 (slow)
        let _ = bus.read_u8(0x002100); // PPU register: 6 (fast)
        let _ = bus.read_u8(0x004016).unwrap(); // joypad port: 12 (extra-slow)
        let _ = bus.read_u8(0x008000).unwrap(); // SlowROM lower bank: 8

        let (count, master) = bus.take_step_access_costs();
        assert_eq!(count, 4);
        assert_eq!(master, 8 + 6 + 12 + 8, "each region must bill its real access speed");

        // FastROM (MEMSEL bit 0) speeds up UPPER-bank ROM only.
        bus.write_u8(0x00420D, 0x01).unwrap();
        bus.take_step_access_costs();
        let _ = bus.read_u8(0x808000).unwrap(); // FastROM upper bank: 6
        let _ = bus.read_u8(0x008000).unwrap(); // lower bank stays slow: 8
        let (_, master_fast) = bus.take_step_access_costs();
        assert_eq!(master_fast, 6 + 8, "FastROM must apply to $80+ banks only");
    }

    #[test]
    fn tick_master_advances_dots_at_exactly_four_master_cycles_each() {
        let mut bus = SystemBus::new();
        let h0 = bus.ppu_ref().h_counter();
        bus.tick_master(6); // 1 dot + remainder 2
        assert_eq!(bus.ppu_ref().h_counter(), h0 + 1, "6 master cycles = 1 whole dot");
        bus.tick_master(2); // remainder 2 + 2 = 1 more dot, no loss to truncation
        assert_eq!(bus.ppu_ref().h_counter(), h0 + 2, "sub-dot remainders must carry across calls");
    }

    #[test]
    fn ppu_to_cpu_readback_dma_transfers_real_data_and_does_not_report_a_stale_done_flag() {
        // The PPU->CPU (B->A) readback direction (DMAPx bit 7 set) used to
        // `return` immediately -- before ever touching `dma_active`/`done`
        // for that transfer. Firing a readback DMA right after an unrelated
        // forward transfer on a DIFFERENT channel had already left
        // `check_done()` == true, so the readback appeared to "complete"
        // even though nothing about it had actually run yet, and it never
        // moved a single real byte.
        let mut bus = SystemBus::new();

        // Seed OAM with a known byte and set OAMADD so $2138 (OAMDATAREAD)
        // reads it back -- a real B-bus register, exercised the same way a
        // CPU-driven read would.
        bus.write_u8(0x002102, 0x00).unwrap();
        bus.write_u8(0x002103, 0x00).unwrap();
        bus.write_u8(0x002104, 0x77).unwrap(); // OAM byte 0 = 0x77 (low-table words
        bus.write_u8(0x002104, 0x00).unwrap(); // commit on the odd-byte write)
        bus.write_u8(0x002102, 0x00).unwrap(); // reset OAMADD/toggle for the DMA read
        bus.write_u8(0x002103, 0x00).unwrap();

        // Channel 0: an unrelated forward (CPU->PPU) transfer that
        // completes normally, leaving done=true, active=false -- the
        // "earlier, unrelated transfer" whose stale flag must not leak.
        bus.write_u8(0x7E0010, 0x5A).unwrap();
        bus.write_u8(0x004300, 0x08).unwrap();
        bus.write_u8(0x004301, 0x18).unwrap();
        bus.write_u8(0x004302, 0x10).unwrap();
        bus.write_u8(0x004303, 0x00).unwrap();
        bus.write_u8(0x004304, 0x7E).unwrap();
        bus.write_u8(0x004305, 0x01).unwrap();
        bus.write_u8(0x004306, 0x00).unwrap();
        bus.write_u8(0x00420B, 0x01).unwrap(); // fire channel 0
        assert!(bus.dma_ref().channel(0).unwrap().done, "channel 0's own transfer really did complete");

        // Channel 1: PPU->CPU readback, reading $2138 (OAMDATAREAD) into
        // WRAM at $7E:0020. Before this fires, channel 1 has never run --
        // its done flag must start false and only become true because THIS
        // transfer actually executed, not because of channel 0's leftover
        // state (`check_done()` checks across all channels).
        assert!(!bus.dma_ref().channel(1).unwrap().done, "channel 1 has not run yet");

        bus.write_u8(0x004310, 0x80).unwrap(); // DMAP1: bit7 = PPU->CPU readback, mode 0
        bus.write_u8(0x004311, 0x38).unwrap(); // BBAD1 = $38 (-> $2138 OAMDATAREAD)
        bus.write_u8(0x004312, 0x20).unwrap(); // A1T1 low = $0020 (destination in WRAM)
        bus.write_u8(0x004313, 0x00).unwrap();
        bus.write_u8(0x004314, 0x7E).unwrap(); // A1B1 = bank $7E
        bus.write_u8(0x004315, 0x01).unwrap(); // DAS1 = 1 byte
        bus.write_u8(0x004316, 0x00).unwrap();

        bus.write_u8(0x00420B, 0x02).unwrap(); // fire channel 1

        // The byte must have actually moved from the B-bus register to WRAM.
        assert_eq!(bus.read_u8(0x7E0020).unwrap(), 0x77, "readback DMA must copy the real OAM byte via $2138, not skip the transfer");

        // And channel 1's own flags must reflect ITS transfer, not leak
        // channel 0's stale state.
        assert!(bus.dma_ref().channel(1).unwrap().done, "channel 1 must report done because its own transfer ran");
        assert!(!bus.dma_ref().is_active(), "dma_active must be cleared once the readback transfer finishes");
    }

    #[test]
    fn dma_is_enabled_reflects_420c_hdmaen_mask_not_leftover_das_value() {
        // `is_enabled()` used to infer "enabled" from `das > 0`, but HDMA's
        // indirect-addressing mode repurposes DAS as the live indirect
        // address -- legitimately nonzero on a channel that was never
        // enabled via $420C at all.
        let mut bus = SystemBus::new();
        assert!(!bus.dma_ref().is_enabled());

        bus.write_u8(0x004305, 0x34).unwrap(); // DASxL -- nonzero, but channel not armed
        bus.write_u8(0x004306, 0x12).unwrap();
        assert!(!bus.dma_ref().is_enabled(), "a nonzero DAS alone must not report the channel enabled");

        bus.write_u8(0x00420C, 0x01).unwrap();
        assert!(bus.dma_ref().is_enabled(), "$420C HDMAEN must be the real source of truth");

        bus.write_u8(0x00420C, 0x00).unwrap();
        assert!(!bus.dma_ref().is_enabled());
    }

    #[test]
    fn hdma_pending_reflects_armed_channels_and_clears_once_table_is_exhausted() {
        let mut bus = SystemBus::new();

        // A "no-repeat" entry (bit7 set) so the table pointer advances past
        // its 1 data byte after the line runs, landing exactly on the
        // end-of-table marker below (a "repeat" entry's pointer would stay
        // parked on the data byte itself, which is an existing, unrelated
        // quirk in `hdma_load_next_entry`'s repeat handling -- not one of
        // the bugs this test targets).
        bus.write_u8(0x7E4000, 0x81).unwrap(); // 1 line, no-repeat
        bus.write_u8(0x7E4001, 0x99).unwrap();
        bus.write_u8(0x7E4002, 0x00).unwrap(); // end of table

        bus.write_u8(0x002116, 0x00).unwrap();
        bus.write_u8(0x002117, 0x00).unwrap();

        bus.write_u8(0x004300, 0x00).unwrap();
        bus.write_u8(0x004301, 0x18).unwrap();
        bus.write_u8(0x004302, 0x00).unwrap();
        bus.write_u8(0x004303, 0x40).unwrap();
        bus.write_u8(0x004304, 0x7E).unwrap();

        assert!(!bus.dma_ref().hdma_pending(), "no channel armed yet");

        tick_dots(&mut bus, 230 * 341); // into vblank, then arm channel 0
        bus.write_u8(0x00420C, 0x01).unwrap();

        tick_dots(&mut bus, 31 * 341 + 100); // scanline 261 dot 100: hdma_init has loaded the first entry
        assert!(bus.dma_ref().hdma_pending(), "channel 0 is armed and its table isn't exhausted yet");

        tick_dots(&mut bus, 300); // cross the line's HDMA slot: transfers the 1 line, reload hits end-of-table
        assert!(!bus.dma_ref().hdma_pending(), "table exhausted -- pending must clear");
    }

    #[test]
    fn hdma_raw_0x80_line_counter_is_a_128_line_non_repeat_entry() {
        // Raw NLTRx byte 0x80 (repeat bit set, 7-bit count 0): because
        // real hardware decrements the WHOLE raw byte each scanline, 0x80
        // behaves as a plain 128-line non-repeat entry -- one transfer on
        // the first line (0x80 -> 0x7F clears the repeat bit), then 127
        // wait lines before the next entry loads. An earlier version
        // special-cased this to "reload the next entry immediately", which
        // is not what the hardware does.
        let mut bus = SystemBus::new();

        bus.write_u8(0x7E5000, 0x80).unwrap(); // raw 0x80: 128 lines, transfer once
        bus.write_u8(0x7E5001, 0xAA).unwrap(); // that first line's data byte
        bus.write_u8(0x7E5002, 0x01).unwrap(); // next real entry: 1 line
        bus.write_u8(0x7E5003, 0xBB).unwrap();
        bus.write_u8(0x7E5004, 0x00).unwrap(); // end of table

        bus.write_u8(0x002116, 0x00).unwrap();
        bus.write_u8(0x002117, 0x00).unwrap();

        bus.write_u8(0x004300, 0x00).unwrap();
        bus.write_u8(0x004301, 0x18).unwrap();
        bus.write_u8(0x004302, 0x00).unwrap();
        bus.write_u8(0x004303, 0x50).unwrap();
        bus.write_u8(0x004304, 0x7E).unwrap();

        tick_dots(&mut bus, 230 * 341); // into vblank, then arm
        bus.write_u8(0x00420C, 0x01).unwrap();

        tick_dots(&mut bus, 32 * 341); // init + the pre-visible line's HDMA slot: the entry's single transfer

        assert_eq!(bus.ppu_ref().vram_ref().read(0x0000), 0xAA);
        assert_eq!(
            bus.dma_ref().channel(0).unwrap().hdma_line_counter,
            0x7F,
            "whole-byte decrement: 0x80 -> 0x7F (127 wait lines remain)"
        );

        // Walk the remaining 127 wait lines: no B-bus writes may happen.
        tick_dots(&mut bus, 127 * 341);
        assert_eq!(
            bus.ppu_ref().vram_ref().read(0x0002),
            0x00,
            "wait lines of the 128-line entry must not transfer anything"
        );

        // Line 128: the next real entry (1 line, 0xBB) loads and transfers.
        tick_dots(&mut bus, 341);
        assert_eq!(
            bus.ppu_ref().vram_ref().read(0x0002),
            0xBB,
            "after the 128 lines, the next real table entry must load and transfer its data"
        );
    }

    #[test]
    fn hdma_indirect_mode_second_address_byte_stays_within_table_bank_on_wraparound() {
        // Table entry straddles the bank boundary: line-count byte at
        // $7E:FFFE, indirect address low byte at $7E:FFFF (so `next_offset`
        // wraps to 0xFFFF), and the high byte must be read from $7E:0000
        // (wrapping within the SAME bank) rather than carrying into $7F:0000.
        let mut bus = SystemBus::new();

        bus.write_u8(0x7EFFFE, 0x01).unwrap(); // line-count = 1
        bus.write_u8(0x7EFFFF, 0x34).unwrap(); // indirect address low byte
        bus.write_u8(0x7E0000, 0x12).unwrap(); // indirect address high byte (correct, same-bank wrap)
        bus.write_u8(0x7F0000, 0xFF).unwrap(); // decoy: what the old bug would have read instead

        bus.write_u8(0x004300, 0x40).unwrap(); // DMAPx bit6 = indirect addressing
        bus.write_u8(0x004301, 0x18).unwrap();
        bus.write_u8(0x004302, 0xFE).unwrap(); // A1T low = $FFFE
        bus.write_u8(0x004303, 0xFF).unwrap(); // A1T high
        bus.write_u8(0x004304, 0x7E).unwrap(); // A1B = bank $7E

        tick_dots(&mut bus, 230 * 341); // into vblank, then arm channel 0
        bus.write_u8(0x00420C, 0x01).unwrap();

        // Land mid-way through the pre-visible line (hardware V=0):
        // hdma_init has loaded the first entry, but the line's HDMA slot
        // (~dot 276) hasn't transferred yet -- a transfer would advance
        // the indirect pointer past the value under test.
        tick_dots(&mut bus, 31 * 341 + 100);

        let indirect_addr = bus.dma_ref().channel(0).unwrap().das;
        assert_eq!(indirect_addr, 0x1234, "high byte must wrap within bank $7E, not carry into $7F");
    }

    #[test]
    fn snapshot_round_trips_cpu_bus_and_memory_state() {
        let mut cpu = crate::cpu::Cpu::new();
        let mut bus = SystemBus::new();
        bus.load_cartridge(vec![0x42; 0x80000]).unwrap();

        // Scatter distinctive state across subsystems.
        cpu.a = 0x1234;
        cpu.pc = 0xABCD;
        cpu.e = false;
        bus.write_u8(0x7E1234, 0x99).unwrap(); // WRAM
        bus.write_u8(0x002116, 0x34).unwrap(); // VMADD
        bus.write_u8(0x002117, 0x12).unwrap();
        bus.write_u8(0x002118, 0x77).unwrap(); // VRAM byte (also bumps VMADD)
        bus.write_u8(0x002105, 0x07).unwrap(); // BGMODE = 7
        bus.write_u8(0x004202, 0x10).unwrap();
        bus.write_u8(0x004203, 0x10).unwrap(); // RDMPY = 0x100
        bus.write_u8(0x002140, 0x5A).unwrap(); // CPU->APU port 0
        tick_dots(&mut bus, 5 * 340 + 123); // advance PPU counters

        let snapshot = crate::state::save_snapshot(&cpu, &bus);

        // Wreck everything, then restore.
        let mut cpu2 = crate::cpu::Cpu::new();
        let mut bus2 = SystemBus::new();
        bus2.load_cartridge(vec![0x42; 0x80000]).unwrap();
        crate::state::load_snapshot(&mut cpu2, &mut bus2, &snapshot).unwrap();

        assert_eq!(cpu2.a, 0x1234);
        assert_eq!(cpu2.pc, 0xABCD);
        assert!(!cpu2.e);
        assert_eq!(bus2.read_u8(0x7E1234).unwrap(), 0x99, "WRAM must round-trip");
        assert_eq!(bus2.ppu_ref().vram_ref().read(0x1234 * 2), 0x77, "VRAM must round-trip");
        assert_eq!(bus2.ppu_registers().bgmode, 0x07, "PPU registers must round-trip");
        assert_eq!(bus2.read_u8(0x004216).unwrap(), 0x00, "RDMPY low byte");
        assert_eq!(bus2.read_u8(0x004217).unwrap(), 0x01, "RDMPY high byte");
        assert_eq!(bus2.apu_ref().cpu_to_apu_port(0), 0x5A, "APU port latch must round-trip");
        assert_eq!(bus2.ppu_ref().scanline(), bus.ppu_ref().scanline(), "PPU timing must round-trip");
        assert_eq!(bus2.ppu_ref().h_counter(), bus.ppu_ref().h_counter());
    }

    #[test]
    fn snapshot_with_bad_magic_or_wrong_sram_size_is_rejected() {
        let mut cpu = crate::cpu::Cpu::new();
        let mut bus = SystemBus::new();
        bus.load_cartridge(vec![0x42; 0x80000]).unwrap();
        let mut snapshot = crate::state::save_snapshot(&cpu, &bus);

        // Bad magic.
        let mut corrupted = snapshot.clone();
        corrupted[0] = b'X';
        assert!(matches!(
            crate::state::load_snapshot(&mut cpu, &mut bus, &corrupted),
            Err(EmulationError::InvalidSaveState(_))
        ));

        // Truncation anywhere must error, not panic.
        snapshot.truncate(snapshot.len() / 2);
        assert!(matches!(
            crate::state::load_snapshot(&mut cpu, &mut bus, &snapshot),
            Err(EmulationError::InvalidSaveState(_))
        ));
    }

    #[test]
    fn slhv_latches_hv_counters_readable_at_213c_213d() {
        let mut bus = SystemBus::new();
        // Advance the PPU to a known position: 3 full 341-dot lines plus
        // 300 dots -> scanline = 3, h_counter = 300.
        tick_dots(&mut bus, 3 * 341 + 300);

        let _ = bus.read_u8(0x002137).unwrap(); // SLHV: latch now
        tick_dots(&mut bus, 123); // moving on must NOT change the latch

        // The high-byte reads only drive bit 0 (bits 7-1 are PPU2 open
        // bus), so mask like real games do.
        let h_lo = bus.read_u8(0x00213C).unwrap() as u16;
        let h_hi = (bus.read_u8(0x00213C).unwrap() & 0x01) as u16;
        let v_lo = bus.read_u8(0x00213D).unwrap() as u16;
        let v_hi = (bus.read_u8(0x00213D).unwrap() & 0x01) as u16;
        assert_eq!((h_hi << 8) | h_lo, 300, "OPHCT must report the latched dot position");
        // Hardware's V counter leads our internal scanline by one (the
        // picture is V=1..224), so internal scanline 3 latches as V=4.
        assert_eq!((v_hi << 8) | v_lo, 4, "OPVCT must report the latched hardware V position");

        // STAT78 must report the latch and clear it (and reset toggles).
        let stat = bus.read_u8(0x00213F).unwrap();
        assert_ne!(stat & 0x40, 0, "STAT78 bit 6 must be set after a latch");
        let stat2 = bus.read_u8(0x00213F).unwrap();
        assert_eq!(stat2 & 0x40, 0, "reading STAT78 must clear the latch flag");
    }

    #[test]
    fn wrio_bit7_falling_edge_latches_counters() {
        let mut bus = SystemBus::new();
        tick_dots(&mut bus, 250);
        bus.write_u8(0x004201, 0xFF).unwrap(); // bit 7 high (also the power-on state)
        bus.write_u8(0x004201, 0x7F).unwrap(); // falling edge -> latch
        let stat = bus.read_u8(0x00213F).unwrap();
        assert_ne!(stat & 0x40, 0, "WRIO bit-7 falling edge must latch the counters");
        let h_lo = bus.read_u8(0x00213C).unwrap() as u16;
        let h_hi = (bus.read_u8(0x00213C).unwrap() & 0x01) as u16; // bits 7-1 are PPU2 open bus
        assert_eq!((h_hi << 8) | h_lo, 250);
    }

    #[test]
    fn slhv_soft_latch_is_gated_by_wrio_bit7() {
        // The $2137 read-latch only works while WRIO ($4201) bit 7 drives
        // the latch pin high (snes9x `S9xLatchCounters` gates on
        // FillRAM[$4213] & 0x80).
        let mut bus = SystemBus::new();
        bus.write_u8(0x004201, 0x7F).unwrap(); // pin low (the falling edge itself latches once)
        let _ = bus.read_u8(0x00213F).unwrap(); // clear that latch flag
        tick_dots(&mut bus, 100);
        let _ = bus.read_u8(0x002137).unwrap();
        assert_eq!(
            bus.read_u8(0x00213F).unwrap() & 0x40,
            0,
            "$2137 must not latch while WRIO bit 7 is low"
        );
        bus.write_u8(0x004201, 0xFF).unwrap(); // pin high again (rising edge doesn't latch)
        let _ = bus.read_u8(0x002137).unwrap();
        assert_ne!(
            bus.read_u8(0x00213F).unwrap() & 0x40,
            0,
            "the $2137 soft latch works with WRIO bit 7 high"
        );
    }

    #[test]
    fn enabling_nmi_mid_vblank_with_the_flag_still_set_fires_immediately() {
        // Turning $4200 bit 7 on while RDNMI's flag is still set (i.e.
        // during vblank, before the game read $4210) must trigger an NMI
        // right away -- snes9x ppu.cpp $4200: "NMI can trigger immediately
        // during VBlank as long as NMI_read ($4210) wasn't cleared".
        let mut bus = SystemBus::new();
        bus.tick_master(225 * 341 * 4); // into vblank (flag set at the entry edge)
        assert!(!bus.take_pending_nmi(), "NMI disabled at the vblank edge: nothing pending");
        bus.write_u8(0x004200, 0x80).unwrap();
        assert!(bus.take_pending_nmi(), "enabling NMI while RDNMI bit 7 is set must fire immediately");

        // ...but not once the game already read (and cleared) $4210.
        let mut bus2 = SystemBus::new();
        bus2.tick_master(225 * 341 * 4);
        let _ = bus2.read_u8(0x004210).unwrap(); // clears the flag
        bus2.write_u8(0x004200, 0x80).unwrap();
        assert!(!bus2.take_pending_nmi(), "no immediate NMI after $4210 was read");
    }

    #[test]
    fn rdnmi_and_timeup_mix_in_cpu_open_bus_bits() {
        let mut bus = SystemBus::new();
        bus.write_u8(0x7E0000, 0xFF).unwrap(); // drive the open bus to 0xFF
        // $4210: bit 7 = flag (clear), bits 6-4 = open bus, bits 3-0 = CPU
        // version 2.
        assert_eq!(bus.read_u8(0x004210).unwrap(), 0x72);
        // $4211: bit 7 = IRQ flag (clear), bits 6-0 = open bus -- which is
        // now 0x72, the byte the $4210 read just drove onto the bus.
        assert_eq!(bus.read_u8(0x004211).unwrap(), 0x72);
    }

    #[test]
    fn hvbjoy_reports_auto_joypad_busy_for_the_first_two_vblank_lines() {
        let mut bus = SystemBus::new();
        bus.write_u8(0x004200, 0x01).unwrap(); // auto-joypad-read enable
        bus.tick_master((224 * 341 + 10) * 4); // scanline 224 dot 10, just inside vblank
        assert_eq!(
            bus.read_u8(0x004212).unwrap() & 0x81,
            0x81,
            "vblank flag + auto-joypad busy during the read window"
        );
        bus.tick_master(2 * 341 * 4); // scanline 226
        assert_eq!(
            bus.read_u8(0x004212).unwrap() & 0x81,
            0x80,
            "busy clears once the ~2-line auto-read window passes"
        );
    }

    #[test]
    fn wram_refresh_stalls_the_cpu_40_master_cycles_once_per_scanline() {
        let mut bus = SystemBus::new();
        bus.take_step_access_costs(); // clear
        bus.tick_master(120 * 4); // dots 0-120: before the refresh position (~dot 134)
        assert_eq!(bus.take_step_access_costs().1, 0, "no refresh charge before dot ~134");
        bus.tick_master(20 * 4); // cross dot 134
        assert_eq!(
            bus.take_step_access_costs().1,
            40,
            "crossing the per-line refresh position must charge 40 master cycles"
        );
        bus.tick_master(100 * 4); // later on the same line
        assert_eq!(bus.take_step_access_costs().1, 0, "the refresh happens once per line");
        bus.tick_master(341 * 4); // same position on the NEXT line
        assert_eq!(bus.take_step_access_costs().1, 40, "every scanline refreshes again");
    }

    #[test]
    fn vram_read_returns_the_prefetch_buffer_with_dummy_read_semantics() {
        // $2139/$213A return the prefetch buffer; the buffer refills from
        // the PRE-increment address on the increment-phase read. Net
        // effect: after setting $2116/$2117 the first TWO word reads both
        // return the addressed word -- which is exactly why real code does
        // one dummy read before consuming data.
        let mut bus = SystemBus::new();
        bus.write_u8(0x002115, 0x80).unwrap(); // word step on the high-byte access
        bus.write_u8(0x002116, 0x00).unwrap();
        bus.write_u8(0x002117, 0x00).unwrap();
        bus.write_u8(0x002118, 0x22).unwrap(); // word 0 = 0x1122
        bus.write_u8(0x002119, 0x11).unwrap();
        bus.write_u8(0x002118, 0x44).unwrap(); // word 1 = 0x3344
        bus.write_u8(0x002119, 0x33).unwrap();

        bus.write_u8(0x002116, 0x00).unwrap(); // point back at word 0 (primes the buffer)
        bus.write_u8(0x002117, 0x00).unwrap();
        assert_eq!(bus.read_u8(0x002139).unwrap(), 0x22, "1st read: the primed word 0");
        assert_eq!(bus.read_u8(0x00213A).unwrap(), 0x11);
        assert_eq!(bus.read_u8(0x002139).unwrap(), 0x22, "2nd read: STILL word 0 (refill was pre-increment)");
        assert_eq!(bus.read_u8(0x00213A).unwrap(), 0x11);
        assert_eq!(bus.read_u8(0x002139).unwrap(), 0x44, "3rd read: word 1's data finally streams out");
        assert_eq!(bus.read_u8(0x00213A).unwrap(), 0x33);
    }

    #[test]
    fn vmain_remap_mode_rotates_the_data_port_address() {
        // VMAIN bits 2-3 = 01 (8-bit rotate): word address aaaaaaaaBBBccccc
        // is accessed as aaaaaaaacccccBBB. Nominal word 0x0021 (BBB=001,
        // ccccc=00001) must land at physical word 0x0009 (00001_001).
        let mut bus = SystemBus::new();
        bus.write_u8(0x002115, 0x84).unwrap(); // increment on high + remap mode 1
        bus.write_u8(0x002116, 0x21).unwrap();
        bus.write_u8(0x002117, 0x00).unwrap();
        bus.write_u8(0x002118, 0xCD).unwrap();
        bus.write_u8(0x002119, 0xAB).unwrap();
        assert_eq!(
            bus.ppu_ref().vram_ref().read_word(0x0009 * 2),
            0xABCD,
            "the remap must rotate the low byte's bit groups"
        );
        assert_eq!(bus.ppu_ref().vram_ref().read_word(0x0021 * 2), 0x0000, "nothing lands at the nominal address");
    }

    #[test]
    fn oam_low_table_commits_word_at_a_time_through_the_write_latch() {
        let mut bus = SystemBus::new();
        bus.write_u8(0x002102, 0x00).unwrap();
        bus.write_u8(0x002103, 0x00).unwrap();
        bus.write_u8(0x002104, 0x55).unwrap(); // even byte: held in the latch
        assert_eq!(
            bus.ppu_ref().oam_ref().read(0),
            0x00,
            "the even byte must sit in the latch until the odd-byte write commits the word"
        );
        bus.write_u8(0x002104, 0x66).unwrap(); // odd byte: commits both
        assert_eq!(bus.ppu_ref().oam_ref().read(0), 0x55);
        assert_eq!(bus.ppu_ref().oam_ref().read(1), 0x66);

        // The high table ($200+) has no latch: each byte writes immediately.
        bus.write_u8(0x002102, 0x00).unwrap();
        bus.write_u8(0x002103, 0x01).unwrap(); // OAMADD word 0x100 -> byte 0x200
        bus.write_u8(0x002104, 0x77).unwrap();
        assert_eq!(bus.ppu_ref().oam_ref().read(512), 0x77, "high-table bytes commit immediately");
    }

    #[test]
    fn mpy_recomputes_on_m7a_writes_too() {
        // The mode-7 multiplier is combinational on M7A and M7B's high
        // byte: writing M7A alone must refresh MPY (snes9x recomputes on
        // both $211B and $211C writes).
        let mut bus = SystemBus::new();
        bus.write_u8(0x00211C, 0x03).unwrap(); // M7B byte = 3
        bus.write_u8(0x00211B, 0xFE).unwrap(); // M7A = -2, low then high
        bus.write_u8(0x00211B, 0xFF).unwrap();
        let lo = bus.read_u8(0x002134).unwrap() as u32;
        let mid = bus.read_u8(0x002135).unwrap() as u32;
        let hi = bus.read_u8(0x002136).unwrap() as u32;
        let result = (((lo | (mid << 8) | (hi << 16)) << 8) as i32) >> 8;
        assert_eq!(result, -6, "MPY must refresh from the M7A write: -2 * 3 = -6");
    }

    #[test]
    fn rdnmi_flag_expires_at_the_end_of_vblank_even_if_never_read() {
        // Hardware clears RDNMI's bit 7 at the end of the blanking period
        // whether or not the game read $4210 (snes9x resets FillRAM[$4210]
        // at the V-counter wrap). A poll outside vblank must see 0.
        let mut bus = SystemBus::new();
        bus.tick_master(230 * 341 * 4); // into vblank; the entry edge set the flag
        bus.tick_master(33 * 341 * 4); // cross the frame wrap into the next frame's line 1
        assert_eq!(
            bus.read_u8(0x004210).unwrap() & 0x80,
            0,
            "the vblank flag must not survive past the end of vblank"
        );
    }

    #[test]
    fn forced_blank_off_during_vblank_reloads_the_oam_address() {
        // The vblank-entry OAM-address reload is skipped in forced blank;
        // turning forced blank OFF while still inside vblank performs the
        // reload right then (snes9x's $2100 handler).
        let mut bus = SystemBus::new();
        bus.write_u8(0x002100, 0x8F).unwrap(); // forced blank ON
        bus.write_u8(0x002102, 0x00).unwrap();
        bus.write_u8(0x002103, 0x00).unwrap();
        bus.write_u8(0x002104, 0x11).unwrap(); // consume word 0
        bus.write_u8(0x002104, 0x22).unwrap();

        tick_dots(&mut bus, 230 * 341); // vblank entry happened while blanked: no reload
        bus.write_u8(0x002100, 0x0F).unwrap(); // un-blank DURING vblank -> reload now

        bus.write_u8(0x002104, 0xAA).unwrap();
        bus.write_u8(0x002104, 0xBB).unwrap();
        assert_eq!(bus.ppu_ref().oam_ref().read(0), 0xAA, "the un-blank write must have reloaded OAMADD to the latch");
        assert_eq!(bus.ppu_ref().oam_ref().read(1), 0xBB);
    }

    #[test]
    fn hdma_on_the_same_channel_kills_an_in_flight_dma() {
        // snes9x dma.cpp: "If HDMA triggers in the middle of DMA transfer
        // and it uses the same channel, it kills the DMA transfer
        // immediately. $43x2 and $43x5 stop updating." A different
        // channel's DMA must be unaffected.
        let build = |dma_channel: u8| -> SystemBus {
            let mut bus = SystemBus::new();
            // HDMA channel 0: an effectively endless repeat entry so the
            // channel stays active on every line of the frame.
            bus.write_u8(0x7E6000, 0xFF).unwrap(); // repeat, 127 lines
            bus.write_u8(0x004300, 0x00).unwrap(); // direct, mode 0
            bus.write_u8(0x004301, 0x22).unwrap(); // -> $2122 (CGRAM), away from the DMA's target
            bus.write_u8(0x004302, 0x00).unwrap();
            bus.write_u8(0x004303, 0x60).unwrap();
            bus.write_u8(0x004304, 0x7E).unwrap();
            tick_dots(&mut bus, 230 * 341); // into vblank, then arm
            bus.write_u8(0x00420C, 0x01).unwrap();
            tick_dots(&mut bus, 32 * 341); // init + pre-visible line; now at scanline 0, dot 0

            // General DMA on `dma_channel`: 1000 bytes into $2118. At 8
            // master cycles/byte it reaches scanline 0's HDMA slot
            // (~dot 276) after ~135 bytes.
            let base = 0x004300 + (dma_channel as u32) * 0x10;
            bus.write_u8(base, 0x08).unwrap(); // fixed source, mode 0
            bus.write_u8(base + 1, 0x18).unwrap();
            bus.write_u8(base + 2, 0x10).unwrap();
            bus.write_u8(base + 3, 0x00).unwrap();
            bus.write_u8(base + 4, 0x7E).unwrap();
            bus.write_u8(base + 5, 0xE8).unwrap(); // DAS = 1000
            bus.write_u8(base + 6, 0x03).unwrap();
            bus.write_u8(0x00420B, 1 << dma_channel).unwrap();
            bus
        };

        // Same channel: the mid-transfer HDMA kills the DMA -- DAS holds
        // the untransferred remainder instead of draining to 0.
        let mut bus = build(0);
        let das = (bus.read_u8(0x004305).unwrap() as u16)
            | ((bus.read_u8(0x004306).unwrap() as u16) << 8);
        assert!(
            das > 0 && das < 1000,
            "the same-channel HDMA must abort the DMA mid-transfer (DAS = {das}, expected 0 < DAS < 1000)"
        );

        // Different channel: the DMA runs to completion.
        let mut bus = build(1);
        let das1 = (bus.read_u8(0x004315).unwrap() as u16)
            | ((bus.read_u8(0x004316).unwrap() as u16) << 8);
        assert_eq!(das1, 0, "an HDMA on channel 0 must not kill a DMA on channel 1");
    }

    #[test]
    fn vram_writes_are_blocked_during_active_display() {
        // The PPU owns VRAM while drawing: data-port writes only land
        // during vblank or forced blank (snes9x BlockInvalidVRAMAccess /
        // CHECK_INBLANK); blocked writes don't advance VMADD either.
        let mut bus = SystemBus::new();
        bus.write_u8(0x002100, 0x0F).unwrap(); // screen ON, scanline 0 = active display
        bus.write_u8(0x002116, 0x00).unwrap();
        bus.write_u8(0x002117, 0x00).unwrap();
        bus.write_u8(0x002118, 0xAA).unwrap(); // active display: dropped
        assert_eq!(bus.ppu_ref().vram_ref().read(0), 0x00, "active-display VRAM writes must be dropped");

        tick_dots(&mut bus, 230 * 341); // into vblank
        bus.write_u8(0x002118, 0xBB).unwrap(); // now it lands -- at word 0 (no phantom increment)
        assert_eq!(bus.ppu_ref().vram_ref().read(0), 0xBB, "vblank writes land at the unmoved address");
    }

    #[test]
    fn overscan_moves_vblank_and_the_nmi_to_line_239() {
        let mut bus = SystemBus::new();
        bus.write_u8(0x002133, 0x04).unwrap(); // SETINI overscan
        bus.write_u8(0x004200, 0x80).unwrap(); // NMI enable
        bus.tick_master(230 * 341 * 4); // line 230: visible in overscan mode
        assert_eq!(bus.read_u8(0x004212).unwrap() & 0x80, 0, "line 230 is not vblank with overscan on");
        assert!(!bus.take_pending_nmi(), "no NMI before line 239 in overscan mode");
        bus.tick_master(10 * 341 * 4); // line 240
        assert_eq!(bus.read_u8(0x004212).unwrap() & 0x80, 0x80, "vblank starts at line 239 with overscan");
        assert!(bus.take_pending_nmi(), "the NMI fires at the overscan vblank boundary");
    }

    #[test]
    fn cgdata_high_byte_masks_bit15_and_213b_reads_it_back_with_open_bus_bit7() {
        let mut bus = SystemBus::new();
        bus.write_u8(0x002121, 0x00).unwrap();
        bus.write_u8(0x002122, 0xFF).unwrap();
        bus.write_u8(0x002122, 0xFF).unwrap(); // high byte: bit 7 doesn't exist in CGRAM
        assert_eq!(
            bus.ppu_ref().cgram_ref().read_color(0),
            0x7FFF,
            "CGRAM colors are 15-bit: the write must mask bit 15"
        );

        bus.write_u8(0x002121, 0x00).unwrap();
        assert_eq!(bus.read_u8(0x00213B).unwrap(), 0xFF, "low byte reads back whole");
        // High-byte read: bits 6-0 from CGRAM (0x7F), bit 7 from PPU2's
        // open bus -- which the low-byte read just set to 0xFF.
        assert_eq!(bus.read_u8(0x00213B).unwrap(), 0xFF);
    }

    #[test]
    fn mode7_multiplier_reports_signed_product_at_2134_2136() {
        let mut bus = SystemBus::new();
        // M7A = -2 (0xFFFE), written low-then-high through the M7 latch.
        bus.write_u8(0x00211B, 0xFE).unwrap();
        bus.write_u8(0x00211B, 0xFF).unwrap();
        // Writing M7B's byte triggers the multiply: -2 * 3 = -6.
        bus.write_u8(0x00211C, 0x03).unwrap();
        let lo = bus.read_u8(0x002134).unwrap() as u32;
        let mid = bus.read_u8(0x002135).unwrap() as u32;
        let hi = bus.read_u8(0x002136).unwrap() as u32;
        let raw = lo | (mid << 8) | (hi << 16);
        // Sign-extend the 24-bit result.
        let result = ((raw << 8) as i32) >> 8;
        assert_eq!(result, -6, "MPY must be the signed product M7A * M7B-byte");
    }

    #[test]
    fn mode7_matrix_registers_latch_low_then_high() {
        let mut bus = SystemBus::new();
        bus.write_u8(0x00211B, 0x34).unwrap(); // M7A low
        bus.write_u8(0x00211B, 0x12).unwrap(); // M7A high
        assert_eq!(bus.ppu_registers().m7a, 0x1234);

        // The latch is shared: an M7D pair written after M7A must not
        // inherit stale bytes.
        bus.write_u8(0x00211E, 0x78).unwrap();
        bus.write_u8(0x00211E, 0x56).unwrap();
        assert_eq!(bus.ppu_registers().m7d, 0x5678);

        // M7X is 13-bit.
        bus.write_u8(0x00211F, 0xFF).unwrap();
        bus.write_u8(0x00211F, 0xFF).unwrap();
        assert_eq!(bus.ppu_registers().m7x, 0x1FFF);

        // $210D doubles as M7HOFS through the M7 latch.
        bus.write_u8(0x00210D, 0xCD).unwrap();
        bus.write_u8(0x00210D, 0x0A).unwrap();
        assert_eq!(bus.ppu_registers().m7_hofs, 0x0ACD);
    }

    #[test]
    fn hardware_multiplier_produces_product_in_rdmpy() {
        let mut bus = SystemBus::new();
        bus.write_u8(0x004202, 0xA7).unwrap(); // WRMPYA = 167
        bus.write_u8(0x004203, 0x3B).unwrap(); // WRMPYB = 59 -> starts the multiply
        let lo = bus.read_u8(0x004216).unwrap() as u16;
        let hi = bus.read_u8(0x004217).unwrap() as u16;
        assert_eq!((hi << 8) | lo, 167 * 59, "RDMPY must hold the unsigned 8x8 product");
    }

    #[test]
    fn hardware_divider_produces_quotient_and_remainder() {
        let mut bus = SystemBus::new();
        bus.write_u8(0x004204, 0x39).unwrap(); // WRDIVL: dividend = 0x1239 = 4665
        bus.write_u8(0x004205, 0x12).unwrap(); // WRDIVH
        bus.write_u8(0x004206, 0x07).unwrap(); // divisor 7 -> starts the divide
        let q = (bus.read_u8(0x004215).unwrap() as u16) << 8 | bus.read_u8(0x004214).unwrap() as u16;
        let r = (bus.read_u8(0x004217).unwrap() as u16) << 8 | bus.read_u8(0x004216).unwrap() as u16;
        assert_eq!(q, 4665 / 7);
        assert_eq!(r, 4665 % 7);
    }

    #[test]
    fn hardware_divide_by_zero_yields_ffff_quotient_and_dividend_remainder() {
        let mut bus = SystemBus::new();
        bus.write_u8(0x004204, 0xCD).unwrap();
        bus.write_u8(0x004205, 0xAB).unwrap();
        bus.write_u8(0x004206, 0x00).unwrap();
        let q = (bus.read_u8(0x004215).unwrap() as u16) << 8 | bus.read_u8(0x004214).unwrap() as u16;
        let r = (bus.read_u8(0x004217).unwrap() as u16) << 8 | bus.read_u8(0x004216).unwrap() as u16;
        assert_eq!(q, 0xFFFF, "divide by zero must yield quotient 0xFFFF (real-hardware behavior)");
        assert_eq!(r, 0xABCD, "divide by zero must yield the dividend as remainder");
    }

    #[test]
    fn wrio_write_reads_back_at_rdio() {
        let mut bus = SystemBus::new();
        bus.write_u8(0x004201, 0x5A).unwrap();
        assert_eq!(bus.read_u8(0x004213).unwrap(), 0x5A, "RDIO must follow the WRIO output latch");
    }

    #[test]
    fn wram_data_port_writes_sequentially_through_wmadd() {
        let mut bus = SystemBus::new();
        // Point WMADD at $7E:4000 (17-bit address 0x04000).
        bus.write_u8(0x002181, 0x00).unwrap();
        bus.write_u8(0x002182, 0x40).unwrap();
        bus.write_u8(0x002183, 0x00).unwrap();
        bus.write_u8(0x002180, 0x11).unwrap();
        bus.write_u8(0x002180, 0x22).unwrap();
        bus.write_u8(0x002180, 0x33).unwrap();
        assert_eq!(bus.read_u8(0x7E4000).unwrap(), 0x11);
        assert_eq!(bus.read_u8(0x7E4001).unwrap(), 0x22);
        assert_eq!(bus.read_u8(0x7E4002).unwrap(), 0x33);
    }

    #[test]
    fn wram_data_port_reads_bank_7f_via_the_17th_address_bit() {
        let mut bus = SystemBus::new();
        bus.write_u8(0x7F0005, 0xEE).unwrap();
        // WMADD = 0x10005 -> $7F:0005.
        bus.write_u8(0x002181, 0x05).unwrap();
        bus.write_u8(0x002182, 0x00).unwrap();
        bus.write_u8(0x002183, 0x01).unwrap();
        assert_eq!(bus.read_u8(0x002180).unwrap(), 0xEE, "WMADDH bit 0 must reach the second 64KB (bank $7F)");
        // The read must have advanced the address.
        bus.write_u8(0x7F0006, 0xDD).unwrap();
        assert_eq!(bus.read_u8(0x002180).unwrap(), 0xDD);
    }

    #[test]
    fn dma_to_wram_data_port_fills_wram() {
        // The classic bulk-clear idiom: DMA channel with B-bus address $80
        // (-> $2180 WMDATA) streams bytes into WRAM through the port.
        let mut bus = SystemBus::new();
        // Source bytes in WRAM bank $7E at $2000.
        bus.write_u8(0x7E2000, 0xAA).unwrap();
        bus.write_u8(0x7E2001, 0xBB).unwrap();
        // Destination: WMADD = $7E:6000.
        bus.write_u8(0x002181, 0x00).unwrap();
        bus.write_u8(0x002182, 0x60).unwrap();
        bus.write_u8(0x002183, 0x00).unwrap();
        // Channel 0: mode 0 (single byte to BBAD), A-bus $7E:2000, 2 bytes.
        bus.write_u8(0x004300, 0x00).unwrap();
        bus.write_u8(0x004301, 0x80).unwrap(); // BBAD = $80 -> $2180
        bus.write_u8(0x004302, 0x00).unwrap();
        bus.write_u8(0x004303, 0x20).unwrap();
        bus.write_u8(0x004304, 0x7E).unwrap();
        bus.write_u8(0x004305, 0x02).unwrap();
        bus.write_u8(0x004306, 0x00).unwrap();
        bus.write_u8(0x00420B, 0x01).unwrap(); // trigger channel 0
        assert_eq!(bus.read_u8(0x7E6000).unwrap(), 0xAA);
        assert_eq!(bus.read_u8(0x7E6001).unwrap(), 0xBB);
    }
}
