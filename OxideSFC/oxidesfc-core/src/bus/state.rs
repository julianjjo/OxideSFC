//! Whole-bus save-state serialization: every component the bus owns, plus
//! its own register/latch state.

use super::SystemBus;
use crate::error::EmulationError;

impl SystemBus {
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
        // with the restored register state and palette (they re-capture
        // as the next frame's scanlines are ticked).
        self.scanline_regs.fill(self.ppu_regs);
        for snap in &mut self.scanline_cgram {
            snap.as_mut_slice().copy_from_slice(self.ppu.cgram_ref().as_slice());
        }
        Ok(())
    }
}
