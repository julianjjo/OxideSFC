//! Register-to-register transfers, XBA, and the MVN/MVP block moves.

use super::{Cpu, CpuFlags};
use crate::bus::{BusResult, MemoryBus};

impl Cpu {
    /// TAX - Transfer Accumulator to Index X (2 cycles)
    pub(super) fn op_tax(&mut self) -> BusResult<u8> {
        if self.p.contains(CpuFlags::INDEX_8BIT) {
            // 8-bit index mode: only transfer low byte
            self.x = (self.a & 0xFF) as u16;
            self.update_nz_flags_8(self.x as u8);
        } else {
            // 16-bit index mode: transfer full accumulator
            self.x = self.a;
            self.update_nz_flags_16(self.x);
        }
        Ok(2)
    }

    /// TXA - Transfer Index X to Accumulator (2 cycles)
    pub(super) fn op_txa(&mut self) -> BusResult<u8> {
        if self.p.contains(CpuFlags::MEMORY_8BIT) {
            // 8-bit memory mode: only transfer low byte, preserving A's high byte
            self.set_a(self.x, false);
            self.update_nz_flags_8(self.a as u8);
        } else {
            // 16-bit memory mode: transfer full X
            self.a = self.x;
            self.update_nz_flags_16(self.a);
        }
        Ok(2)
    }

    /// TAY - Transfer Accumulator to Index Y (2 cycles)
    pub(super) fn op_tay(&mut self) -> BusResult<u8> {
        if self.p.contains(CpuFlags::INDEX_8BIT) {
            // 8-bit index mode: only transfer low byte
            self.y = (self.a & 0xFF) as u16;
            self.update_nz_flags_8(self.y as u8);
        } else {
            // 16-bit index mode: transfer full accumulator
            self.y = self.a;
            self.update_nz_flags_16(self.y);
        }
        Ok(2)
    }

    /// TYA - Transfer Index Y to Accumulator (2 cycles)
    pub(super) fn op_tya(&mut self) -> BusResult<u8> {
        if self.p.contains(CpuFlags::MEMORY_8BIT) {
            // 8-bit memory mode: only transfer low byte, preserving A's high byte
            self.set_a(self.y, false);
            self.update_nz_flags_8(self.a as u8);
        } else {
            // 16-bit memory mode: transfer full Y
            self.a = self.y;
            self.update_nz_flags_16(self.a);
        }
        Ok(2)
    }

    /// TSX - Transfer Stack Pointer to Index X (2 cycles)
    pub(super) fn op_tsx(&mut self) -> BusResult<u8> {
        if self.p.contains(CpuFlags::INDEX_8BIT) {
            // 8-bit index mode: only transfer low byte of SP
            self.x = (self.sp & 0xFF) as u16;
            self.update_nz_flags_8(self.x as u8);
        } else {
            // 16-bit index mode: transfer full SP
            self.x = self.sp;
            self.update_nz_flags_16(self.x);
        }
        Ok(2)
    }

    /// TXY - Transfer X to Y (0x9B, 2 cycles)
    pub(super) fn op_txy(&mut self) -> BusResult<u8> {
        if self.p.contains(CpuFlags::INDEX_8BIT) {
            self.y = self.x & 0xFF;
            self.update_nz_flags_8(self.y as u8);
        } else {
            self.y = self.x;
            self.update_nz_flags_16(self.y);
        }
        Ok(2)
    }

    /// TYX - Transfer Y to X (0xBB, 2 cycles)
    pub(super) fn op_tyx(&mut self) -> BusResult<u8> {
        if self.p.contains(CpuFlags::INDEX_8BIT) {
            self.x = self.y & 0xFF;
            self.update_nz_flags_8(self.x as u8);
        } else {
            self.x = self.y;
            self.update_nz_flags_16(self.x);
        }
        Ok(2)
    }

    /// TXS - Transfer Index X to Stack Pointer (2 cycles)
    /// Note: TXS does NOT affect N/Z flags
    pub(super) fn op_txs(&mut self) -> BusResult<u8> {
        if self.p.contains(CpuFlags::INDEX_8BIT) {
            // 8-bit index mode: only low byte matters
            // In emulation mode, SP high byte stays at 0x01
            if self.e {
                self.sp = 0x0100 | (self.x & 0xFF);
            } else {
                self.sp = self.x & 0xFF;
            }
        } else {
            // 16-bit index mode: transfer full X
            // In emulation mode, still restricted to page 1
            if self.e {
                self.sp = 0x0100 | (self.x & 0xFF);
            } else {
                self.sp = self.x;
            }
        }
        Ok(2)
    }

    /// TCD - Transfer Accumulator (C) to Direct Page register (2 cycles)
    /// D is always a full 16-bit register regardless of the M flag.
    pub(super) fn op_tcd(&mut self) -> BusResult<u8> {
        self.d = self.a;
        self.update_nz_flags_16(self.d);
        Ok(2)
    }

    /// TDC - Transfer Direct Page register to Accumulator (C) (2 cycles)
    pub(super) fn op_tdc(&mut self) -> BusResult<u8> {
        self.a = self.d;
        self.update_nz_flags_16(self.a);
        Ok(2)
    }

    /// TCS - Transfer Accumulator (C) to Stack Pointer (2 cycles)
    /// Does not affect N/Z flags. In emulation mode SP's high byte stays 0x01.
    pub(super) fn op_tcs(&mut self) -> BusResult<u8> {
        if self.e {
            self.sp = 0x0100 | (self.a & 0xFF);
        } else {
            self.sp = self.a;
        }
        Ok(2)
    }

    /// TSC (0x3B) - Transfer Stack Pointer to Accumulator (C) (2 cycles).
    /// Always transfers the full 16-bit S into the full 16-bit C
    /// regardless of the M flag, and sets N/Z from the 16-bit result
    /// (unlike its mirror `op_tcs`, which touches no flags).
    pub(super) fn op_tsc(&mut self) -> BusResult<u8> {
        self.a = self.sp;
        self.update_nz_flags_16(self.a);
        Ok(2)
    }

    /// XBA - Exchange the two bytes of the Accumulator (3 cycles).
    /// Always operates on the full 16-bit C register regardless of the M
    /// flag; N/Z are set from the new low byte only.
    pub(super) fn op_xba(&mut self) -> BusResult<u8> {
        self.a = self.a.rotate_left(8);
        self.update_nz_flags_8((self.a & 0xFF) as u8);
        Ok(3)
    }

    // ==================== Addressing Modes ====================

    /// MVN $src,$dest (0x54) - Move Memory Negative (incrementing
    /// addresses). Per spec, MVN/MVP always operate on the full 16-bit
    /// A/X/Y regardless of the M/X flags. Real hardware re-executes this
    /// opcode one byte at a time (so a large transfer can be interrupted
    /// mid-copy by NMI/IRQ); this performs the whole transfer in one step
    /// instead, which leaves A/X/Y/DB in exactly the architecturally
    /// specified end state -- the only difference being the move is atomic
    /// here rather than interruptible mid-transfer.
    ///
    /// Real hardware spends 7 cycles per byte moved, which for a large
    /// transfer (up to 65536 bytes) can total up to 458,752 cycles -- far
    /// more than fits in the `u8` this function still returns for its
    /// direct-call-site compatibility with every other opcode handler. The
    /// true total is instead stashed in `self.pending_cycle_adjustment`,
    /// which `execute()` folds into its widened `u32` result immediately
    /// after this call returns.
    pub(super) fn op_mvn(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        // Operand order per the 65816 spec (Eyes & Lichty, "Block Move
        // Instructions"): the byte after the opcode is the DESTINATION
        // bank, the next one is the SOURCE bank -- the reverse of the
        // assembler mnemonic's `MVN src,dst` operand order. These used to
        // be read swapped, which made every cross-bank block move copy
        // from the wrong bank into the wrong bank (e.g. SMW's overworld
        // loader `MVN $7E,$0C` -- ROM tile data into WRAM -- instead read
        // WRAM garbage and wrote it into read-only ROM, leaving the
        // overworld's Map16 buffer holding the previous level's tiles).
        let dest_bank = self.fetch_u8(bus)?;
        let src_bank = self.fetch_u8(bus)?;
        let initial_count = (self.a as u32).wrapping_add(1);
        let mut count = initial_count;
        while count > 0 {
            let byte = bus.read_u8(((src_bank as u32) << 16) | (self.x as u32))?;
            bus.write_u8(((dest_bank as u32) << 16) | (self.y as u32), byte)?;
            self.x = self.x.wrapping_add(1);
            self.y = self.y.wrapping_add(1);
            count -= 1;
        }
        self.a = 0xFFFF;
        self.db = dest_bank;
        self.pending_cycle_adjustment = initial_count * 7;
        Ok(0)
    }

    /// MVP $src,$dest (0x44) - Move Memory Positive (decrementing
    /// addresses), used for overlapping copies where the destination is
    /// above the source. See `op_mvn` for the atomic-transfer rationale
    /// and cycle-accounting note.
    pub(super) fn op_mvp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        // Destination bank first, then source bank -- see `op_mvn`.
        let dest_bank = self.fetch_u8(bus)?;
        let src_bank = self.fetch_u8(bus)?;
        let initial_count = (self.a as u32).wrapping_add(1);
        let mut count = initial_count;
        while count > 0 {
            let byte = bus.read_u8(((src_bank as u32) << 16) | (self.x as u32))?;
            bus.write_u8(((dest_bank as u32) << 16) | (self.y as u32), byte)?;
            self.x = self.x.wrapping_sub(1);
            self.y = self.y.wrapping_sub(1);
            count -= 1;
        }
        self.a = 0xFFFF;
        self.db = dest_bank;
        self.pending_cycle_adjustment = initial_count * 7;
        Ok(0)
    }

    // ==================== Misc control ====================
}
