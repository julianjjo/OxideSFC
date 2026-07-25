//! DMA and HDMA execution: immediate channel transfers ($420B) and the
//! per-scanline HDMA table walk ($420C), both billed at the hardware rate of
//! 8 master cycles per byte so interrupts and HDMA can fire mid-transfer.

use super::{bbus_pattern_for_mode, SystemBus};

impl SystemBus {
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
    pub(super) fn execute_dma_channel(&mut self, channel: usize) {
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
    pub(super) fn hdma_init(&mut self) {
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
    pub(super) fn update_hdma_pending(&mut self) {
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
    pub(super) fn hdma_load_next_entry(&mut self, channel: usize) {
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
    pub(super) fn hdma_run_scanline(&mut self) {
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
    pub(super) fn hdma_transfer_one_line(&mut self, channel: usize) {
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
}
