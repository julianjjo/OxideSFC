//! Master-clock timing: what each memory region costs, how those cycles
//! turn into PPU dots and APU cycles, and the per-scanline work that rides
//! along with them (HDMA, NMI/IRQ edges, joypad auto-read, the per-line
//! register/palette snapshots the renderer draws from).

use super::SystemBus;

impl SystemBus {
    /// Master-cycle cost of one bus access, per the real SNES memory
    /// speed map: 6 ("fast") for most I/O registers and FastROM-enabled
    /// upper-bank ROM, 8 ("slow") for WRAM/cartridge/SlowROM, 12
    /// ("extra-slow") for the $4000-$41FF joypad region. Verified against
    /// fullsnes's "Memory Access Cycles" table.
    pub(super) fn access_master_cycles(&self, addr: u32) -> u32 {
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
    pub(super) fn latch_hv_counters(&mut self) {
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
    pub(super) fn tick_ppu_dots(&mut self, dots: u32) {
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
                self.scanline_cgram[line as usize]
                    .as_mut_slice()
                    .copy_from_slice(self.ppu.cgram_ref().as_slice());
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
}
