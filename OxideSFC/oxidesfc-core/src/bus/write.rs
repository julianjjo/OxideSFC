//! CPU-visible writes: the mirror of `super::read`, dispatching the whole
//! memory-mapped register space to the components that own each register.

use super::{BusResult, MemoryBus, SystemBus};
use crate::error::EmulationError;

impl SystemBus {
    /// Write to the bus with SNES memory mapping
    pub(super) fn write_bus(&mut self, addr: u32, value: u8) -> BusResult<()> {
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
                return self.wram.write_u8(offset, value);
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
                if reg.is_multiple_of(2) {
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
