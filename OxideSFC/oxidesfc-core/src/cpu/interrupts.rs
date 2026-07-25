//! Interrupts and the instructions that raise or wait for them: NMI, IRQ,
//! BRK/COP software interrupts, RTI, and WAI.

use super::{Cpu, CpuFlags};
use crate::bus::{BusResult, MemoryBus};

impl Cpu {
    /// Services a non-maskable interrupt: pushes the return context onto
    /// the stack and jumps to the NMI vector. Mirrors `op_rti`'s pull
    /// order in reverse so the two stay symmetric -- emulation mode pushes
    /// only PC then P (no bank, matching the 6502-style 3-byte frame
    /// `op_rti` pulls), native mode additionally pushes PB first (4-byte
    /// frame). The interrupt vector is $FFEA/$FFEB in native mode and
    /// $FFFA/$FFFB in emulation mode. Real hardware doesn't check NMI mid
    /// instruction -- callers should only invoke this between `step()`
    /// calls.
    pub fn nmi(&mut self, bus: &mut impl MemoryBus) -> BusResult<()> {
        self.waiting_for_interrupt = false;
        if !self.e {
            self.push_stack(bus, self.pb as u16, false)?;
        }
        self.push_stack(bus, self.pc, true)?;
        self.push_stack(bus, self.p.bits() as u16, false)?;

        self.p.remove(CpuFlags::DECIMAL);
        self.p.insert(CpuFlags::IRQ_DISABLE);

        let vector = if self.e { 0xFFFA_u32 } else { 0xFFEA_u32 };
        let lo = bus.read_u8(vector)? as u16;
        let hi = bus.read_u8(vector.wrapping_add(1))? as u16;
        self.pc = (hi << 8) | lo;
        self.pb = 0;

        Ok(())
    }

    /// Services a maskable IRQ (e.g. the PPU H/V-timer interrupt SMW uses
    /// for its in-level status-bar raster split). Same push/vector shape
    /// as `nmi()` but through the IRQ vectors ($FFEE native, $FFFE
    /// emulation). The CALLER must check `CpuFlags::IRQ_DISABLE` first --
    /// the 65816 ignores the (level-triggered) IRQ line while I is set,
    /// and the line stays asserted in the bus until software acknowledges
    /// it (reading $4211), so dispatching while I is set would re-enter
    /// forever.
    pub fn irq(&mut self, bus: &mut impl MemoryBus) -> BusResult<()> {
        self.waiting_for_interrupt = false;
        if !self.e {
            self.push_stack(bus, self.pb as u16, false)?;
        }
        self.push_stack(bus, self.pc, true)?;
        self.push_stack(bus, self.p.bits() as u16, false)?;

        self.p.remove(CpuFlags::DECIMAL);
        self.p.insert(CpuFlags::IRQ_DISABLE);

        let vector = if self.e { 0xFFFE_u32 } else { 0xFFEE_u32 };
        let lo = bus.read_u8(vector)? as u16;
        let hi = bus.read_u8(vector.wrapping_add(1))? as u16;
        self.pc = (hi << 8) | lo;
        self.pb = 0;

        Ok(())
    }

    /// Wakes the CPU out of a WAI/STP-induced halt when `pending` is
    /// true, without dispatching an interrupt handler. Real 65816
    /// hardware resumes normal instruction fetch on ANY asserted
    /// interrupt line (NMI or IRQ), even while the I flag masks IRQ
    /// dispatch -- it just won't jump to a handler in that case. `nmi()`
    /// already clears `waiting_for_interrupt` whenever it actually runs
    /// (and NMI dispatch is never gated on I), so this method exists for
    /// the IRQ side: callers should invoke it with the bus's live
    /// interrupt-line state (e.g. `bus.irq_pending() || nmi_pending`)
    /// BEFORE the IRQ_DISABLE-gated call to `irq()`, so a WAI right
    /// before/around SEI doesn't hang forever waiting for a handler that
    /// will never be allowed to run.
    pub fn wake_if_interrupt_pending(&mut self, pending: bool) {
        if pending {
            self.waiting_for_interrupt = false;
        }
    }

    /// RTI (0x40) - Return from Interrupt
    ///
    /// Emulation mode mirrors the 6502/65C02: the interrupt sequence pushed
    /// only P then PC (3 bytes total, no bank), so RTI pulls just those two.
    /// Native mode's interrupt sequence additionally pushes PB (4 bytes
    /// total: PB, PCH, PCL, P), so RTI must also pull PB back -- skipping
    /// this in native mode left PB stuck at whatever it was after the
    /// pull, silently corrupting the active bank on return from any
    /// interrupt taken while running in native mode (e.g. SMW's NMI
    /// handler, since the boot code switches to native mode via CLC/XCE
    /// before enabling NMI).
    pub(super) fn op_rti(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        // Pull status register (always a single byte)
        let status = self.pull_stack(bus, false)?;
        self.p = CpuFlags::from_bits_truncate(status as u8);
        self.enforce_emulation_mode_register_widths();

        // Pull PC (always a full 16-bit value)
        self.pc = self.pull_stack(bus, true)?;

        if !self.e {
            self.pb = self.pull_stack(bus, false)? as u8;
            Ok(7)
        } else {
            Ok(6)
        }
    }

    /// WAI (0xCB) - Wait for Interrupt: suspends fetch until `nmi()` wakes
    /// the CPU. STP (0xDB) is treated identically -- see the field comment
    /// on `waiting_for_interrupt`.
    pub(super) fn op_wai(&mut self) -> BusResult<u8> {
        self.waiting_for_interrupt = true;
        Ok(3)
    }

    /// BRK (0x00) - Software interrupt. Pushes the same return-context
    /// frame as `nmi()` (see its comment for the native/emulation
    /// push-count distinction) and jumps to the BRK/IRQ vector. The byte
    /// immediately after the opcode is a signature byte real hardware
    /// fetches but ignores -- consumed here only so PC lands correctly.
    pub(super) fn op_brk(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.fetch_u8(bus)?;
        if !self.e {
            self.push_stack(bus, self.pb as u16, false)?;
        }
        self.push_stack(bus, self.pc, true)?;
        self.push_stack(bus, self.p.bits() as u16, false)?;
        self.p.remove(CpuFlags::DECIMAL);
        self.p.insert(CpuFlags::IRQ_DISABLE);
        let vector = if self.e { 0xFFFE_u32 } else { 0xFFE6_u32 };
        let lo = bus.read_u8(vector)? as u16;
        let hi = bus.read_u8(vector.wrapping_add(1))? as u16;
        self.pc = (hi << 8) | lo;
        self.pb = 0;
        Ok(7)
    }

    /// COP (0x02) - Coprocessor software interrupt. Same push frame and
    /// flag updates as `op_brk` (see its comment for the native/emulation
    /// push-count distinction) -- the only difference is COP dispatches
    /// through its own vector pair instead of BRK/IRQ's, since real
    /// hardware gives COP a distinct entry point so a coprocessor trap
    /// handler doesn't collide with the BRK/IRQ handler. Vectors are
    /// $00FFE4 (native) / $00FFF4 (emulation), one step below the
    /// NMI ($FFEA/$FFFA) and IRQ/BRK ($FFEE/$FFFE) pairs already used by
    /// `nmi()`/`irq()` in this file. Like BRK, the byte immediately after
    /// the opcode is a signature byte real hardware fetches but ignores --
    /// consumed here only so PC lands correctly.
    pub(super) fn op_cop(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.fetch_u8(bus)?;
        if !self.e {
            self.push_stack(bus, self.pb as u16, false)?;
        }
        self.push_stack(bus, self.pc, true)?;
        self.push_stack(bus, self.p.bits() as u16, false)?;
        self.p.remove(CpuFlags::DECIMAL);
        self.p.insert(CpuFlags::IRQ_DISABLE);
        let vector = if self.e { 0xFFF4_u32 } else { 0xFFE4_u32 };
        let lo = bus.read_u8(vector)? as u16;
        let hi = bus.read_u8(vector.wrapping_add(1))? as u16;
        self.pc = (hi << 8) | lo;
        self.pb = 0;
        Ok(7)
    }

    /// WDM (0x42) - Reserved/undefined opcode. Real silicon fetches and
    /// discards one operand byte and otherwise behaves as a 2-cycle NOP.
    pub(super) fn op_wdm(&mut self, bus: &mut impl MemoryBus) -> BusResult<u8> {
        self.fetch_u8(bus)?;
        Ok(2)
    }
}
