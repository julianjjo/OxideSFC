//! The PPU's memory-access ports as the CPU sees them: VRAM ($2116-$2119
//! with its address remapping and read-prefetch latch), CGRAM ($2121-$2122)
//! and OAM ($2102-$2104), each with its own address/word-latch quirks.

use super::SystemBus;

impl SystemBus {
    /// Applies $2115 (VMAIN) bits 2-3's "full graphic" address remapping
    /// to a VRAM word address: the selected bit-groups rotate so that
    /// sequential data-port accesses walk a bitmap column-major within
    /// 8-line strips. Formulas match snes9x's `S9xUpdateVRAMReadBuffer` /
    /// REGISTER_2118 remap tables (Shift 5/6/7, IncCount 32/64/128):
    ///   01: aaaaaaaa BBBccccc -> aaaaaaaa cccccBBB
    ///   10: aaaaaaaB BBcccccc -> aaaaaaac cccccBBB
    ///   11: aaaaaaBB Bccccccc -> aaaaaccc ccccBBB
    pub(super) fn vram_remap(&self, word_addr: u16) -> u16 {
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
    pub(super) fn reload_vram_prefetch(&mut self) {
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
    pub(super) fn vram_write(&mut self, is_high_byte: bool, value: u8) {
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
    pub(super) fn cgram_write(&mut self, value: u8) {
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
    pub(super) fn refresh_first_sprite(&mut self) {
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
    pub(super) fn oam_write(&mut self, value: u8) {
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
    pub(super) fn vram_read(&mut self, is_high_byte: bool) -> u8 {
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
    pub(super) fn cgram_read(&mut self) -> u8 {
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
    pub(super) fn oam_read(&mut self) -> u8 {
        let byte_addr = self.oamadd.wrapping_mul(2).wrapping_add(if self.oam_high { 1 } else { 0 });
        let value = self.ppu.oam().read(byte_addr);
        if self.oam_high {
            self.oamadd = self.oamadd.wrapping_add(1);
        }
        self.oam_high = !self.oam_high;
        value
    }
}
