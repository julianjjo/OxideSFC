//! `Cpu` construction, the fetch/execute step loop, memory access, and the
//! register-width and flag bookkeeping every instruction goes through.

use super::{Cpu, CpuFlags};
use crate::bus::{BusResult, MemoryBus};

impl Cpu {
    pub fn new() -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            pc: 0,
            pb: 0,
            sp: 0x01FF,
            db: 0,
            d: 0,
            p: CpuFlags::IRQ_DISABLE | CpuFlags::MEMORY_8BIT | CpuFlags::INDEX_8BIT,
            e: true,
            cycles: 0,
            waiting_for_interrupt: false,
            pending_cycle_adjustment: 0,
            #[cfg(feature = "stack_shadow_debug")]
            shadow_stack: Vec::new(),
            #[cfg(feature = "stack_shadow_debug")]
            stack_mismatch: None,
            #[cfg(feature = "stack_shadow_debug")]
            instruction_trace: std::collections::VecDeque::new(),
        }
    }

    /// Serializes the complete architectural CPU state (registers, flags,
    /// emulation-mode bit, WAI latch) for save states. The
    /// `stack_shadow_debug` diagnostic fields are intentionally excluded.
    pub fn save_state(&self, out: &mut Vec<u8>) {
        crate::state::put_u16(out, self.a);
        crate::state::put_u16(out, self.x);
        crate::state::put_u16(out, self.y);
        crate::state::put_u16(out, self.pc);
        crate::state::put_u8(out, self.pb);
        crate::state::put_u16(out, self.sp);
        crate::state::put_u8(out, self.db);
        crate::state::put_u16(out, self.d);
        crate::state::put_u8(out, self.p.bits());
        crate::state::put_bool(out, self.e);
        crate::state::put_u64(out, self.cycles);
        crate::state::put_bool(out, self.waiting_for_interrupt);
    }

    /// Restores state produced by `save_state`.
    pub(crate) fn load_state(&mut self, r: &mut crate::state::StateReader) -> Result<(), crate::error::EmulationError> {
        self.a = r.u16()?;
        self.x = r.u16()?;
        self.y = r.u16()?;
        self.pc = r.u16()?;
        self.pb = r.u8()?;
        self.sp = r.u16()?;
        self.db = r.u8()?;
        self.d = r.u16()?;
        self.p = CpuFlags::from_bits_truncate(r.u8()?);
        self.e = r.bool()?;
        self.cycles = r.u64()?;
        self.waiting_for_interrupt = r.bool()?;
        self.pending_cycle_adjustment = 0;
        Ok(())
    }

    pub fn reset(&mut self, bus: &mut impl MemoryBus) -> BusResult<()> {
        // Load reset vector from $FFFC-$FFFD
        let pc_lo = bus.read_u8(0xFFFC)?;
        let pc_hi = bus.read_u8(0xFFFD)?;
        self.pc = ((pc_hi as u16) << 8) | (pc_lo as u16);

        // Reset state
        self.pb = 0;
        self.db = 0;
        self.d = 0;
        self.sp = (self.sp & 0x00FF) | 0x0100; // Preserve low byte, set high to 0x01
        self.p = CpuFlags::IRQ_DISABLE | CpuFlags::MEMORY_8BIT | CpuFlags::INDEX_8BIT;
        self.e = true;
        self.a = 0;
        self.x = 0;
        self.y = 0;
        self.waiting_for_interrupt = false;
        self.pending_cycle_adjustment = 0;

        Ok(())
    }

    /// Lee un byte de la dirección actual (PB:PC) y avanza PC
    pub fn fetch_u8(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        let addr = ((self.pb as u32) << 16) | (self.pc as u32);
        let byte = bus.read_u8(addr)?;
        self.pc = self.pc.wrapping_add(1);
        Ok(byte)
    }

    /// Lee un word de la dirección actual (little-endian) y avanza PC
    pub fn fetch_u16(&mut self, bus: &mut impl MemoryBus) -> BusResult<u16> {
        let lo = self.fetch_u8(bus)? as u16;
        let hi = self.fetch_u8(bus)? as u16;
        Ok((hi << 8) | lo)
    }

    /// Ejecuta un ciclo de instrucción
    ///
    /// Returns the number of cycles the executed instruction cost. This is
    /// `u32` rather than `u8` solely to accommodate `MVN`/`MVP` (0x54/0x44),
    /// which can move up to 65536 bytes at 7 cycles/byte -- up to 458,752
    /// cycles -- in a single call; every other opcode's cost still fits
    /// comfortably in a handful of bits.
    pub fn step(&mut self, bus: &mut impl MemoryBus) -> BusResult<u32> {
        if self.waiting_for_interrupt {
            // WAI/STP suspended fetch -- only `nmi()`, or `irq()`/
            // `wake_if_interrupt_pending()` on an asserted IRQ line, can
            // resume it.
            return Ok(1);
        }
        #[cfg(feature = "stack_shadow_debug")]
        let pc_before_full = ((self.pb as u32) << 16) | (self.pc as u32);
        let opcode = self.fetch_u8(bus)?;
        let result = self.execute(opcode, bus);
        #[cfg(feature = "stack_shadow_debug")]
        {
            let pc_after_full = ((self.pb as u32) << 16) | (self.pc as u32);
            self.instruction_trace.push_back((pc_before_full, opcode, pc_after_full));
            if self.instruction_trace.len() > 300 {
                self.instruction_trace.pop_front();
            }
        }
        result
    }

    /// Real 65816 hardware cannot represent 16-bit A/X/Y while the
    /// emulation-mode flag (E) is set -- E forces M and X to 1 (8-bit)
    /// unconditionally. `op_xce` already enforces this when E transitions
    /// to true, but any opcode that can otherwise rewrite P from an
    /// arbitrary value (REP clearing M/X to request 16-bit, or PLP/RTI
    /// restoring whatever flags were sitting on the stack) must re-apply
    /// the same constraint afterward, or the CPU ends up in a
    /// hardware-impossible state: emulation mode with 16-bit registers.
    /// Mirrors `op_xce`'s enforcement exactly, including truncating X/Y's
    /// high bytes to zero the moment INDEX_8BIT becomes forced on.
    pub(super) fn enforce_emulation_mode_register_widths(&mut self) {
        if self.e {
            self.p.insert(CpuFlags::MEMORY_8BIT);
            self.p.insert(CpuFlags::INDEX_8BIT);
            self.x &= 0x00FF;
            self.y &= 0x00FF;
        }
    }

    /// Lee memoria según tamaño (8 o 16 bits)
    pub(super) fn read_memory(&mut self, bus: &mut impl MemoryBus, addr: u32, is_16bit: bool) -> BusResult<u16> {
        if is_16bit {
            let lo = bus.read_u8(addr)? as u16;
            let hi = bus.read_u8(addr.wrapping_add(1))? as u16;
            Ok((hi << 8) | lo)
        } else {
            Ok(bus.read_u8(addr)? as u16)
        }
    }

    /// Escribe memoria según tamaño (8 o 16 bits)
    pub(super) fn write_memory(&mut self, bus: &mut impl MemoryBus, addr: u32, value: u16, is_16bit: bool) -> BusResult<()> {
        bus.write_u8(addr, (value & 0xFF) as u8)?;
        if is_16bit {
            bus.write_u8(addr.wrapping_add(1), (value >> 8) as u8)?;
        }
        Ok(())
    }

    /// Sets the accumulator from a loaded `value`. In 16-bit mode this
    /// replaces all of A. In 8-bit mode, only the low byte is the
    /// architectural accumulator -- the high byte is a "hidden" register
    /// (exposed via XBA) that 8-bit loads must NOT clobber. Several load
    /// opcodes (LDA in every addressing mode, TXA, TYA, PLA) used to do
    /// `self.a = value` unconditionally, zeroing the high byte even in
    /// 8-bit mode; real code that stages a byte in the high half via XBA
    /// before an 8-bit LDA/PLA/TXA/TYA (a common and legitimate pattern,
    /// e.g. Super Mario World's own SPC700 upload routine) would have that
    /// byte silently destroyed.
    pub(super) fn set_a(&mut self, value: u16, is_16bit: bool) {
        if is_16bit {
            self.a = value;
        } else {
            self.a = (self.a & 0xFF00) | (value & 0xFF);
        }
    }

    /// Update N/Z flags based on memory size (uses value for flags, not register)
    pub(super) fn update_nz_flags_mem(&mut self, value: u16, is_16bit: bool) {
        if is_16bit {
            self.update_nz_flags_16(value);
        } else {
            self.update_nz_flags_8(value as u8);
        }
    }

    // ==================== Flag Helpers ====================

    /// Update N/Z flags based on a 16-bit value (for 16-bit register operations)
    pub fn update_nz_flags_16(&mut self, value: u16) {
        if value == 0 {
            self.p.insert(CpuFlags::ZERO);
        } else {
            self.p.remove(CpuFlags::ZERO);
        }
        if (value & 0x8000) != 0 {
            self.p.insert(CpuFlags::NEGATIVE);
        } else {
            self.p.remove(CpuFlags::NEGATIVE);
        }
    }

    /// Update N/Z flags based on an 8-bit value (for 8-bit operations)
    pub fn update_nz_flags_8(&mut self, value: u8) {
        if value == 0 {
            self.p.insert(CpuFlags::ZERO);
        } else {
            self.p.remove(CpuFlags::ZERO);
        }
        if (value & 0x80) != 0 {
            self.p.insert(CpuFlags::NEGATIVE);
        } else {
            self.p.remove(CpuFlags::NEGATIVE);
        }
    }
}
