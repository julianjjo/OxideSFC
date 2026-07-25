//! CPU-visible reads: the memory map plus the full memory-mapped register
//! space. One `match` on the address, so every handled range is visible in
//! one place next to its neighbours.

use super::{BusResult, MemoryBus, SystemBus};
use crate::error::EmulationError;

impl SystemBus {
    /// Read from the bus with SNES memory mapping
    pub(super) fn read_bus(&mut self, addr: u32) -> BusResult<u8> {
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
                let result = self.wram.read_u8(offset)?;
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
}
