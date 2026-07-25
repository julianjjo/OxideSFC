//! The 65816's addressing modes: each `addr_*` resolves an operand's
//! effective 24-bit address, plus the stack addressing that emulation mode
//! constrains to page 1.

use super::Cpu;
use crate::bus::{BusResult, MemoryBus};

impl Cpu {
    /// Direct Page + index effective-address computation shared by the
    /// dp,X / dp,Y / (dp,X) addressing modes. Reproduces the documented
    /// 65816 emulation-mode quirk (from "Programming the 65816" by Eyes &
    /// Lichty, inherited for 6502 compatibility): when the CPU is in
    /// emulation mode (E=1) AND the low byte of D is zero, the low byte of
    /// (offset + index) wraps within a single 256-byte page instead of
    /// carrying into D's high byte. In every other case (native mode, or
    /// emulation mode with DL != 0) this is a plain 16-bit wrapping add.
    pub(super) fn dp_indexed_address(&self, offset: u16, index: u16) -> u16 {
        if self.e && (self.d & 0xFF) == 0 {
            self.d | (offset.wrapping_add(index) & 0xFF)
        } else {
            self.d.wrapping_add(offset).wrapping_add(index)
        }
    }

    /// Direct Page Indexed,X: like Direct Page, plus X (bank 0, wraps within 16 bits)
    pub(super) fn addr_direct_page_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let offset = self.fetch_u8(bus)? as u16;
        let addr = self.dp_indexed_address(offset, self.x);
        Ok(addr as u32)
    }

    /// Effective bus address of the current stack pointer location.
    ///
    /// In emulation mode, the 65816 hardware forces the stack into page 1
    /// (the SP high byte is fixed at 0x01) regardless of what's actually in
    /// `self.sp`'s high byte. In native mode, SP is a full 16-bit register
    /// pointing anywhere in bank 0 -- forcing page 1 unconditionally here
    /// (as this code used to) silently corrupted any native-mode stack
    /// usage outside page 1, e.g. SMW's boot code sets SP=$1FFF via TCS.
    pub(super) fn stack_addr(&self) -> u32 {
        if self.e {
            0x0100 | (self.sp & 0xFF) as u32
        } else {
            self.sp as u32
        }
    }

    /// Pushes `value` onto the stack. The number of bytes pushed is decided
    /// solely by `is_16bit` -- callers already compute this correctly from
    /// the relevant M/X flag (or pass a hardcoded width for registers like
    /// D/PC that are always a fixed size). The previous version also forced
    /// a 2-byte push whenever `self.e` was true, which meant PHA/PHX/PHY/PHP
    /// (genuinely 8-bit operations in emulation mode, the SNES's default
    /// boot state) silently pushed an extra phantom byte and corrupted SP.
    pub(super) fn push_stack(&mut self, bus: &mut impl MemoryBus, value: u16, is_16bit: bool) -> BusResult<()> {
        #[cfg(feature = "stack_shadow_debug")]
        {
            let full_pc = ((self.pb as u32) << 16) | (self.pc as u32);
            self.shadow_stack.push((full_pc, if is_16bit { 2 } else { 1 }));
        }
        if is_16bit {
            bus.write_u8(self.stack_addr(), (value >> 8) as u8)?;
            self.sp = self.sp.wrapping_sub(1);
        }
        bus.write_u8(self.stack_addr(), (value & 0xFF) as u8)?;
        self.sp = self.sp.wrapping_sub(1);
        Ok(())
    }

    pub(super) fn pull_stack(&mut self, bus: &mut impl MemoryBus, is_16bit: bool) -> BusResult<u16> {
        #[cfg(feature = "stack_shadow_debug")]
        {
            let full_pc = ((self.pb as u32) << 16) | (self.pc as u32);
            let expected = if is_16bit { 2 } else { 1 };
            match self.shadow_stack.pop() {
                Some((push_pc, push_size)) if push_size != expected => {
                    if self.stack_mismatch.is_none() {
                        self.stack_mismatch = Some(format!(
                            "push at PC={:06X} pushed {} byte(s), but pull at PC={:06X} expects {} byte(s)",
                            push_pc, push_size, full_pc, expected
                        ));
                    }
                }
                None => {
                    if self.stack_mismatch.is_none() {
                        self.stack_mismatch = Some(format!("pull at PC={:06X} ({} bytes) with empty shadow stack", full_pc, expected));
                    }
                }
                _ => {}
            }
        }
        self.sp = self.sp.wrapping_add(1);
        let low = bus.read_u8(self.stack_addr())? as u16;

        if is_16bit {
            self.sp = self.sp.wrapping_add(1);
            let high = bus.read_u8(self.stack_addr())? as u16;
            Ok((high << 8) | low)
        } else {
            Ok(low)
        }
    }

    // ==================== Jump/Call ====================

    /// Immediate: lee operando del stream de instrucciones
    pub(super) fn addr_immediate(&mut self, bus: &mut impl MemoryBus, is_16bit: bool) -> BusResult<u16> {
        if is_16bit {
            self.fetch_u16(bus)
        } else {
            self.fetch_u8(bus).map(|b| b as u16)
        }
    }

    /// Absolute: lee dirección de 16 bits del stream y combina con DB
    pub(super) fn addr_absolute(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let addr = self.fetch_u16(bus)?;
        Ok(((self.db as u32) << 16) | (addr as u32))
    }

    /// Direct Page: suma D al operando de 8 bits (siempre en banco 0)
    pub(super) fn addr_direct_page(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let offset = self.fetch_u8(bus)? as u16;
        let addr = self.d.wrapping_add(offset);
        Ok(addr as u32)
    }

    /// Absolute Long: 3-byte little-endian operand, explicit bank (ignores DB)
    pub(super) fn addr_absolute_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let lo = self.fetch_u8(bus)? as u32;
        let mid = self.fetch_u8(bus)? as u32;
        let hi = self.fetch_u8(bus)? as u32;
        Ok((hi << 16) | (mid << 8) | lo)
    }

    /// Absolute Long Indexed,X: 24-bit base + X, wrapping within 24 bits
    /// (unlike plain Absolute,X, the carry is allowed to cross banks).
    pub(super) fn addr_absolute_long_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let base = self.addr_absolute_long(bus)?;
        Ok(base.wrapping_add(self.x as u32) & 0xFF_FFFF)
    }

    /// Direct Page Indirect Long: reads a 24-bit pointer (low, high, bank)
    /// stored at the direct-page address, used as-is.
    pub(super) fn addr_indirect_long(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let dp_addr = self.addr_direct_page(bus)?;
        let lo = bus.read_u8(dp_addr)? as u32;
        let mid = bus.read_u8(dp_addr.wrapping_add(1))? as u32;
        let hi = bus.read_u8(dp_addr.wrapping_add(2))? as u32;
        Ok((hi << 16) | (mid << 8) | lo)
    }

    /// Direct Page Indirect Indexed,Y -- "(dp),Y": a 16-bit pointer stored
    /// at the direct-page address, combined with the Data Bank register
    /// (NOT an explicit bank byte, unlike the "[dp],Y" long form), then
    /// indexed by Y. The Y addition wraps within 16 bits without carrying
    /// into the next bank (same convention as plain Absolute,X/Y).
    pub(super) fn addr_indirect_dp_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let dp_addr = self.addr_direct_page(bus)?;
        let lo = bus.read_u8(dp_addr)? as u16;
        let hi = bus.read_u8(dp_addr.wrapping_add(1))? as u16;
        let pointer = ((hi << 8) | lo).wrapping_add(self.y);
        Ok(((self.db as u32) << 16) | (pointer as u32))
    }

    /// Direct Page Indirect -- "(dp)": a 16-bit pointer stored at the
    /// direct-page address, combined with the Data Bank register, with no
    /// index applied.
    pub(super) fn addr_indirect_dp(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let dp_addr = self.addr_direct_page(bus)?;
        let lo = bus.read_u8(dp_addr)? as u16;
        let hi = bus.read_u8(dp_addr.wrapping_add(1))? as u16;
        let pointer = (hi << 8) | lo;
        Ok(((self.db as u32) << 16) | (pointer as u32))
    }

    /// Direct Page Indexed Indirect,X -- "(dp,X)": add X to the direct
    /// page address *before* dereferencing the 16-bit pointer, then
    /// combine with the Data Bank register.
    pub(super) fn addr_indirect_dp_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let offset = self.fetch_u8(bus)? as u16;
        let dp_addr = self.dp_indexed_address(offset, self.x) as u32;
        let lo = bus.read_u8(dp_addr)? as u16;
        let hi = bus.read_u8(dp_addr.wrapping_add(1))? as u16;
        let pointer = (hi << 8) | lo;
        Ok(((self.db as u32) << 16) | (pointer as u32))
    }

    /// Direct Page Indirect Long Indexed,Y: same 24-bit pointer, plus Y,
    /// wrapping within 24 bits (the carry may cross banks, like Absolute
    /// Long Indexed,X).
    pub(super) fn addr_indirect_long_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let base = self.addr_indirect_long(bus)?;
        Ok(base.wrapping_add(self.y as u32) & 0xFF_FFFF)
    }

    /// Stack Relative: an 8-bit offset added to SP, always within bank 0
    /// (the stack never leaves bank 0 on the 65816).
    pub(super) fn addr_stack_relative(&mut self, bus: &mut impl MemoryBus) -> BusResult<u16> {
        let offset = self.fetch_u8(bus)? as u16;
        Ok(self.sp.wrapping_add(offset))
    }

    /// Stack Relative Indirect Indexed,Y -- "(sr,S),Y": a 16-bit pointer
    /// stored at the stack-relative address (bank 0), combined with the
    /// Data Bank register and indexed by Y (wraps within 16 bits, no
    /// carry into DB, same convention as plain Absolute,X/Y).
    pub(super) fn addr_stack_relative_indirect_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let sr_addr = self.addr_stack_relative(bus)?;
        let lo = bus.read_u8(sr_addr as u32)? as u16;
        let hi = bus.read_u8(sr_addr.wrapping_add(1) as u32)? as u16;
        let pointer = ((hi << 8) | lo).wrapping_add(self.y);
        Ok(((self.db as u32) << 16) | (pointer as u32))
    }

    /// Direct Page Indexed,Y: like Direct Page, plus Y (used by STX/LDX dp,Y)
    pub(super) fn addr_direct_page_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let offset = self.fetch_u8(bus)? as u16;
        let addr = self.dp_indexed_address(offset, self.y);
        Ok(addr as u32)
    }

    /// Absolute Indexed,X: DBR:addr + X computed as a full 24-bit addition --
    /// a carry out of the 16-bit offset propagates into the bank byte
    /// (DBR effectively becomes DBR+1 for that access, wrapping $FF to $00).
    pub(super) fn addr_absolute_x(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let addr = self.fetch_u16(bus)?;
        let base = ((self.db as u32) << 16) | (addr as u32);
        Ok(base.wrapping_add(self.x as u32) & 0xFF_FFFF)
    }

    /// Absolute Indexed,Y: DBR:addr + Y computed as a full 24-bit addition --
    /// a carry out of the 16-bit offset propagates into the bank byte
    /// (DBR effectively becomes DBR+1 for that access, wrapping $FF to $00).
    pub(super) fn addr_absolute_y(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        let addr = self.fetch_u16(bus)?;
        let base = ((self.db as u32) << 16) | (addr as u32);
        Ok(base.wrapping_add(self.y as u32) & 0xFF_FFFF)
    }
}
