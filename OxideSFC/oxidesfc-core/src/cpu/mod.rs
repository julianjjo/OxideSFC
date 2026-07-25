//! The 65816 CPU.
//!
//! One `impl Cpu` split across files by instruction family, so a given
//! opcode's implementation sits next to its relatives rather than in a
//! 4000-line block:
//!
//! - `core` -- construction, the fetch/execute step loop, memory access, and
//!   the register-width/flag bookkeeping every instruction goes through.
//! - `dispatch` -- the 256-arm opcode `match`.
//! - `addressing` -- the addressing modes and stack addressing.
//! - `load_store`, `add_sub`, `compare`, `incdec`, `logic`, `bittest`,
//!   `shift`, `branch`, `stack`, `transfer`, `flags` -- the instructions
//!   themselves.
//! - `interrupts` -- NMI/IRQ, BRK/COP, RTI and WAI.
//!
//! Register widths are the thing to be careful about throughout: the M and X
//! flags switch the accumulator and index registers between 8 and 16 bits at
//! runtime, and emulation mode forces both to 8.

mod add_sub;
mod addressing;
mod bittest;
mod branch;
mod compare;
mod core;
mod dispatch;
mod flags;
mod incdec;
mod interrupts;
mod load_store;
mod logic;
mod shift;
mod stack;
mod transfer;

#[cfg(test)]
mod tests;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CpuFlags: u8 {
        const CARRY             = 0b0000_0001; // C
        const ZERO              = 0b0000_0010; // Z
        const IRQ_DISABLE       = 0b0000_0100; // I
        const DECIMAL           = 0b0000_1000; // D
        const INDEX_8BIT        = 0b0001_0000; // X (1=8-bit indices)
        const MEMORY_8BIT       = 0b0010_0000; // M (1=8-bit mem/acc)
        const OVERFLOW          = 0b0100_0000; // V
        const NEGATIVE          = 0b1000_0000; // N
    }
}

pub struct Cpu {
    pub a: u16,
    pub x: u16,
    pub y: u16,
    pub pc: u16,
    pub pb: u8,
    pub sp: u16,
    pub db: u8,
    pub d: u16,
    pub p: CpuFlags,
    pub e: bool,
    pub cycles: u64,
    /// Set by WAI (0xCB) or STP (0xDB): suspends instruction fetch until
    /// `nmi()` wakes the CPU back up (real STP technically only wakes on a
    /// full reset, but treating it the same as WAI is a harmless
    /// simplification -- neither opcode is expected in normal gameplay
    /// code, just defensive coverage so hitting one doesn't read as an
    /// unimplemented-opcode halt).
    pub waiting_for_interrupt: bool,
    /// Side channel used by `op_mvn`/`op_mvp` to report their true cycle
    /// cost. Those two opcodes move up to 65536 bytes at 7 cycles/byte
    /// (up to 458,752 cycles) in a single `step()` call, which doesn't fit
    /// in the `u8` every other opcode handler returns; they stash the
    /// overflow here and `execute()` folds it into the widened `u32`
    /// total immediately after dispatch, so it never survives past a
    /// single instruction.
    pending_cycle_adjustment: u32,
    #[cfg(feature = "stack_shadow_debug")]
    pub shadow_stack: Vec<(u32, u8)>,
    #[cfg(feature = "stack_shadow_debug")]
    pub stack_mismatch: Option<String>,
    #[cfg(feature = "stack_shadow_debug")]
    pub instruction_trace: std::collections::VecDeque<(u32, u8, u32)>,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}
