//! DMA (Direct Memory Access) and HDMA Controller for SNES
//!
//! The SNES DMA controller has 8 DMA channels (channels 0-7) that can transfer
//! data from main memory (A-bus) to PPU/APU registers (B-bus). DMA transfers
//! halt the CPU during transfer.
//!
//! HDMA (Horizontal DMA) runs at the end of each scanline for raster effects.

/// Number of DMA channels
pub const DMA_CHANNELS: usize = 8;

/// A single DMA channel
#[derive(Debug, Clone)]
pub struct DmaChannel {
    /// Transfer mode and settings (DMAPx). Real hardware layout:
    /// Bits 0-2: Transfer unit/mode (0-7, byte pattern relative to BBADx)
    /// Bit 3: Transfer direction (0=CPU->PPU, 1=PPU->CPU)
    /// Bit 4: Fixed transfer (1=fixed address, 0=increment) -- DMA only
    /// Bit 5: Address decrement (DMA only; unused by this emulator)
    /// Bit 6: HDMA addressing mode (0=direct table, 1=indirect table)
    /// Bit 7: unused
    ///
    /// Whether a channel actually *does* HDMA at all is controlled
    /// entirely by the separate, global $420C (HDMAEN) register -- there
    /// is no per-channel "HDMA enable" bit in DMAPx itself.
    pub(crate) dmape: u8,
    /// B-bus address (destination) (BBADx)
    pub(crate) bbad: u8,
    /// A-bus address (source) - 16-bit (A1TxL/A1TxH)
    pub(crate) a1t: u16,
    /// A-bus bank (source bank) (A1Bx)
    pub(crate) a1b: u8,
    /// Transfer size (DMA) - 16-bit (DASxL/DASxH). Doubles, on real
    /// hardware, as the *indirect transfer address* for HDMA channels in
    /// indirect addressing mode (DMAPx bit 6) -- reused here the same way.
    pub(crate) das: u16,
    /// Transfer size bank (DASBx) for DMA; doubles as the fixed indirect
    /// HDMA address bank, set by software before enabling HDMA and never
    /// modified by the HDMA engine itself.
    pub(crate) dasb: u8,
    /// A2AxL/A2AxH: HDMA table's *current* read address (low 16 bits --
    /// the bank stays fixed at `a1b` for the whole table, matching real
    /// hardware). Advances as line-count/data bytes are consumed from the
    /// table; re-initialized from `a1t` at HDMA init.
    pub(crate) a2a: u16,
    /// $43xB: unused/unmapped on real hardware -- independent storage, not
    /// aliased to any other register.
    unused_b: u8,
    /// $43xC: unused/unmapped on real hardware -- independent storage, not
    /// aliased to `unused_b` (previously both offsets wrongly shared one
    /// backing field, so writing $43xC clobbered what $43xB last read
    /// back).
    unused_c: u8,
    /// NLTRx ($43xA): the raw line-counter/repeat byte last read from the
    /// HDMA table, decremented as a WHOLE byte once per scanline (so the
    /// repeat bit is naturally consumed along with the count, matching
    /// real hardware -- e.g. a raw 0x80 behaves as "transfer once, then
    /// wait 127 lines"). Bit 7 set = "repeat" (transfer on every line of
    /// the entry); bit 7 clear = transfer only on the entry's FIRST line,
    /// with no B-bus writes at all on the remaining wait lines. A
    /// freshly-read value of 0 is the end-of-table marker.
    pub(crate) hdma_line_counter: u8,
    /// True once this channel's table has been read to its end-of-table
    /// (0x00) marker; the channel is inert for the rest of the frame.
    pub(crate) hdma_terminated: bool,
    /// Whether this channel performs a B-bus transfer on the current
    /// scanline: set when a table entry is (re)loaded (every entry's first
    /// line always transfers), then re-derived each line from the repeat
    /// bit -- the canonical hardware HDMA state machine (anomie/fullsnes).
    /// Without it, wait lines of non-repeat entries kept re-writing the
    /// B-bus every scanline.
    pub(crate) hdma_do_transfer: bool,
    /// $43xD: unused/unmapped on real hardware -- independent storage, not
    /// aliased onto A1TxL (previously writing this offset silently
    /// corrupted the channel's real A-bus source address).
    unused_d: u8,
    /// $43xE: unused/unmapped on real hardware -- independent storage, not
    /// aliased onto A1TxH (see `unused_d`).
    unused_e: u8,
    /// $43xF: unused/unmapped on real hardware -- independent storage, not
    /// aliased onto DASBx (previously writing this offset silently
    /// corrupted the channel's real transfer-size/indirect-bank byte).
    unused_f: u8,
    /// Transfer completed flag. `pub(crate)` so `SystemBus::execute_dma_channel`
    /// can set it to reflect a real transfer's start/end instead of it
    /// being permanently stuck at `false`.
    pub(crate) done: bool,
}

impl DmaChannel {
    /// Creates a new DMA channel with default values
    fn new() -> Self {
        Self {
            dmape: 0,
            bbad: 0,
            a1t: 0,
            a1b: 0,
            das: 0,
            dasb: 0,
            a2a: 0,
            unused_b: 0,
            unused_c: 0,
            hdma_line_counter: 0,
            hdma_terminated: false,
            hdma_do_transfer: false,
            unused_d: 0,
            unused_e: 0,
            unused_f: 0,
            done: false,
        }
    }

    /// DMAPx bit 6: HDMA addressing mode (0 = direct table, 1 = indirect).
    pub(crate) fn hdma_indirect_mode(&self) -> bool {
        (self.dmape & 0x40) != 0
    }

    /// DMAPx bits 0-2: transfer unit/mode, shared by DMA and HDMA (which
    /// B-bus offsets successive bytes cycle through relative to `bbad`).
    pub(crate) fn transfer_mode(&self) -> u8 {
        self.dmape & 0x07
    }

    /// Gets the full HDMA table start address (24-bit): bank `a1b`, offset
    /// `a1t`. Only meaningful at HDMA init -- the live read position during
    /// the frame is `a2a` (same bank).
    pub(crate) fn source_address(&self) -> u32 {
        ((self.a1b as u32) << 16) | (self.a1t as u32)
    }

    /// Gets the HDMA table's current read address (24-bit): bank `a1b`
    /// (fixed for the channel's whole table), offset `a2a` (advances as
    /// the table is consumed).
    pub(crate) fn table_addr(&self) -> u32 {
        ((self.a1b as u32) << 16) | (self.a2a as u32)
    }
}

/// DMA Controller
///
/// This struct is deliberately just register storage (the $43x0-$43xF
/// channel config bytes, plus a couple of status flags) -- it does not
/// execute transfers itself. An earlier version had `start_dma`/
/// `perform_dma_transfer`/`write_to_bbus` methods that *looked* like a
/// working DMA engine but their B-bus write path was a stub that silently
/// discarded every byte (no VRAM/CGRAM/OAM/APU ever actually received
/// data). That's exactly the kind of silent failure this project needs to
/// avoid, so it was deleted rather than left around half-working. Real
/// transfer execution lives in `SystemBus::execute_dma_channel`, which has
/// simultaneous access to the cartridge/WRAM (source) and PPU/APU
/// (destination) needed to actually move bytes -- something this struct
/// alone can't do without that access.
#[derive(Debug)]
pub struct Dma {
    /// 8 DMA channels
    channels: [DmaChannel; 8],
    /// Whether DMA is currently active (CPU halted)
    pub dma_active: bool,
    /// HDMA pending flag
    hdma_pending: bool,
    /// Mirrors $420C (HDMAEN): which channels are currently armed. This is
    /// the real source of truth for `is_enabled()` -- previously that
    /// method guessed from whether a channel's DAS register happened to be
    /// nonzero, but HDMA legitimately repurposes DAS as the indirect
    /// address, which can be nonzero on a channel that was never actually
    /// enabled via $420C.
    enable_mask: u8,
}

impl Dma {
    /// Creates a new DMA controller
    pub fn new() -> Self {
        Self {
            channels: std::array::from_fn(|_| DmaChannel::new()),
            dma_active: false,
            hdma_pending: false,
            enable_mask: 0,
        }
    }

    /// Resets the DMA controller to initial state
    pub fn reset(&mut self) {
        for channel in &mut self.channels {
            *channel = DmaChannel::new();
        }
        self.dma_active = false;
        self.hdma_pending = false;
        self.enable_mask = 0;
    }

    /// Reads a DMA register
    ///
    /// # Arguments
    /// * `addr` - Register address ($43x0-$43xF)
    ///
    /// # Returns
    /// The value of the register
    pub fn read_register(&self, addr: u8) -> u8 {
        let channel = (addr >> 4) as usize;
        let reg = addr & 0x0F;

        if channel >= DMA_CHANNELS {
            return 0;
        }

        let ch = &self.channels[channel];

        match reg {
            0x0 => ch.dmape,
            0x1 => ch.bbad,
            0x2 => (ch.a1t & 0xFF) as u8,
            0x3 => ((ch.a1t >> 8) & 0xFF) as u8,
            0x4 => ch.a1b,
            0x5 => (ch.das & 0xFF) as u8,
            0x6 => ((ch.das >> 8) & 0xFF) as u8,
            0x7 => ch.dasb,
            0x8 => (ch.a2a & 0xFF) as u8,
            0x9 => ((ch.a2a >> 8) & 0xFF) as u8,
            0xA => ch.hdma_line_counter,
            0xB => ch.unused_b, // Unused, independent storage -- see field doc comment
            0xC => ch.unused_c, // Unused, independent storage -- see field doc comment
            0xD => ch.unused_d, // Unused, independent storage -- see field doc comment
            0xE => ch.unused_e, // Unused, independent storage -- see field doc comment
            0xF => ch.unused_f, // Unused, independent storage -- see field doc comment
            _ => 0,
        }
    }

    /// Writes to a DMA register
    ///
    /// # Arguments
    /// * `addr` - Register address ($43x0-$43xF)
    /// * `value` - Value to write
    pub fn write_register(&mut self, addr: u8, value: u8) {
        let channel = (addr >> 4) as usize;
        let reg = addr & 0x0F;

        if channel >= DMA_CHANNELS {
            return;
        }

        let ch = &mut self.channels[channel];

        match reg {
            0x0 => ch.dmape = value,
            0x1 => ch.bbad = value,
            0x2 => ch.a1t = (ch.a1t & 0xFF00) | (value as u16),
            0x3 => ch.a1t = (ch.a1t & 0x00FF) | ((value as u16) << 8),
            0x4 => ch.a1b = value,
            0x5 => ch.das = (ch.das & 0xFF00) | (value as u16),
            0x6 => ch.das = (ch.das & 0x00FF) | ((value as u16) << 8),
            0x7 => ch.dasb = value,
            0x8 => ch.a2a = (ch.a2a & 0xFF00) | (value as u16),
            0x9 => ch.a2a = (ch.a2a & 0x00FF) | ((value as u16) << 8),
            0xA => ch.hdma_line_counter = value,
            0xB => ch.unused_b = value, // Unused/unmapped -- independent storage, no aliasing
            0xC => ch.unused_c = value, // Unused/unmapped -- independent storage, no aliasing
            0xD => ch.unused_d = value, // Unused/unmapped -- independent storage, no aliasing
            0xE => ch.unused_e = value, // Unused/unmapped -- independent storage, no aliasing
            0xF => ch.unused_f = value, // Unused/unmapped -- independent storage, no aliasing
            _ => {}
        }
    }

    /// Checks if any DMA channel has a pending transfer
    pub fn is_active(&self) -> bool {
        self.dma_active
    }

    /// Checks if DMA or HDMA is armed for any channel, based on the real
    /// $420C (HDMAEN) enable mask mirrored here via `set_enable_mask` --
    /// not a heuristic over register contents. An earlier version checked
    /// `das > 0`, but HDMA's indirect-addressing mode repurposes DAS as the
    /// live indirect address, which can be nonzero on a channel that was
    /// never enabled via $420C at all.
    pub fn is_enabled(&self) -> bool {
        self.enable_mask != 0
    }

    /// Mirrors the current $420C (HDMAEN) channel-enable bitmask; called by
    /// `SystemBus` whenever that register is written, so `is_enabled()`
    /// reflects real per-channel enable state.
    pub(crate) fn set_enable_mask(&mut self, mask: u8) {
        self.enable_mask = mask;
    }

    /// Serializes the full DMA/HDMA controller state for save states.
    pub fn save_state(&self, out: &mut Vec<u8>) {
        use crate::state::{put_bool, put_u16, put_u8};
        for ch in &self.channels {
            put_u8(out, ch.dmape);
            put_u8(out, ch.bbad);
            put_u16(out, ch.a1t);
            put_u8(out, ch.a1b);
            put_u16(out, ch.das);
            put_u8(out, ch.dasb);
            put_u16(out, ch.a2a);
            put_u8(out, ch.unused_b);
            put_u8(out, ch.unused_c);
            put_u8(out, ch.hdma_line_counter);
            put_bool(out, ch.hdma_terminated);
            put_bool(out, ch.hdma_do_transfer);
            put_u8(out, ch.unused_d);
            put_u8(out, ch.unused_e);
            put_u8(out, ch.unused_f);
            put_bool(out, ch.done);
        }
        put_bool(out, self.dma_active);
        put_bool(out, self.hdma_pending);
        put_u8(out, self.enable_mask);
    }

    /// Restores state produced by `save_state`.
    pub(crate) fn load_state(&mut self, r: &mut crate::state::StateReader) -> Result<(), crate::error::EmulationError> {
        for ch in self.channels.iter_mut() {
            ch.dmape = r.u8()?;
            ch.bbad = r.u8()?;
            ch.a1t = r.u16()?;
            ch.a1b = r.u8()?;
            ch.das = r.u16()?;
            ch.dasb = r.u8()?;
            ch.a2a = r.u16()?;
            ch.unused_b = r.u8()?;
            ch.unused_c = r.u8()?;
            ch.hdma_line_counter = r.u8()?;
            ch.hdma_terminated = r.bool()?;
            ch.hdma_do_transfer = r.bool()?;
            ch.unused_d = r.u8()?;
            ch.unused_e = r.u8()?;
            ch.unused_f = r.u8()?;
            ch.done = r.bool()?;
        }
        self.dma_active = r.bool()?;
        self.hdma_pending = r.bool()?;
        self.enable_mask = r.u8()?;
        Ok(())
    }

    /// Gets a reference to a specific DMA channel
    pub fn channel(&self, index: usize) -> Option<&DmaChannel> {
        self.channels.get(index)
    }

    /// Gets a mutable reference to a specific DMA channel
    pub fn channel_mut(&mut self, index: usize) -> Option<&mut DmaChannel> {
        self.channels.get_mut(index)
    }

    /// Gets whether HDMA is pending
    pub fn hdma_pending(&self) -> bool {
        self.hdma_pending
    }

    /// Sets HDMA pending flag
    pub fn set_hdma_pending(&mut self, pending: bool) {
        self.hdma_pending = pending;
    }

    /// Checks if any DMA channel has completed its transfer
    pub fn check_done(&self) -> bool {
        for ch in &self.channels {
            if ch.done {
                return true;
            }
        }
        false
    }

    /// Clears the done flag for a channel
    pub fn clear_done(&mut self, channel: usize) {
        if channel < DMA_CHANNELS {
            self.channels[channel].done = false;
        }
    }

    /// Clears all done flags
    pub fn clear_all_done(&mut self) {
        for ch in &mut self.channels {
            ch.done = false;
        }
    }
}

impl Default for Dma {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
