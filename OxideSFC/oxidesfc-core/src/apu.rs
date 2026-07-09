//! APU (Audio Processing Unit) module for OxideSFC SNES emulator.
//!
//! The APU is an "independent console within the SNES" consisting of:
//! - Sony SPC700: 8-bit audio CPU (similar to 6502)
//! - DSP: 8 voice channels with BRR compression, ADSR envelopes, echo
//! - 64KB APU RAM: Isolated memory for SPC700 program and audio data
//! - Communication Ports: 4 ports ($2140-$2143) for communication with main CPU

use std::collections::VecDeque;
use std::sync::Mutex;
use std::sync::Arc;

// ============================================================================
// SPC700 CPU Core
// ============================================================================

/// SPC700 CPU registers and state
/// The SPC700 is a modified 6502-compatible processor with custom opcodes
pub struct Spc700 {
    /// Accumulator (A register)
    pub a: u8,
    /// X index register
    pub x: u8,
    /// Y index register
    pub y: u8,
    /// Stack pointer (0-255, relative to page 0x01)
    pub sp: u8,
    /// Program counter
    pub pc: u16,
    /// Processor status word
    pub psw: Psw,
    /// Cycles remaining for current instruction
    pub cycles_remaining: u32,
    /// RAM reference for memory operations
    pub ram: Arc<Mutex<[u8; 65536]>>,
    /// The CPU<->APU communication latches ($F4-$F7 on this side,
    /// $2140-$2143 on the main CPU's side), shared with `Apu` so this
    /// really-executing SPC700 code drives the same ports the main CPU
    /// reads/writes -- see `ApuPorts`.
    pub ports: Arc<Mutex<ApuPorts>>,
    /// Set when `step` encounters an opcode outside the validated subset
    /// this decoder implements (see `execute_opcode`'s doc comment). Once
    /// set, `step` stops advancing instead of corrupting state or
    /// panicking on an unknown encoding.
    pub halted: Option<u8>,

    /// Timer hardware ($F1 enable bits, $FA-$FC divisor targets, $FD-$FF
    /// read-only output counters). Verified against
    /// wiki.superfamicom.org/spc700-reference and snesmusic.org's SPC700
    /// docs: timers 0/1 tick their internal stage at 8KHz, timer 2 at
    /// 64KHz; each tick increments an 8-bit divider that resets and bumps
    /// the visible 4-bit counter when it reaches the target. Reading a
    /// counter resets it to 0. This is real, documented SPC700 hardware --
    /// without it, any driver that polls a timer counter to pace itself
    /// (a near-universal pattern) spins forever.
    timer_enable: [bool; 3],
    timer_target: [u8; 3],
    timer_divider: [u8; 3],
    timer_counter: [u8; 3],
    /// Stage-1 prescaler accumulator, in SPC700 instruction-steps (not
    /// true elapsed cycles -- ticking once per `step()` call is an
    /// approximation, but sufficient to make timers progress correctly
    /// relative to each other and eventually fire).
    timer_prescaler: [u32; 3],
    /// Raw value last written to $F1, for bits other than the timer
    /// enables (kept for completeness/inspection; not otherwise acted on).
    control: u8,
    /// The DSP, reached indirectly through $F2 (register-select port,
    /// stored in `dsp_addr`) and $F3 (register data port) -- see
    /// `read_mem`/`write_mem`. Shared with `Apu` (which needs it for
    /// `Apu::tick`'s sample generation and register readback) the same
    /// way `ram`/`ports` are.
    dsp: Arc<Mutex<Dsp>>,
    /// Last value written to $F2: which DSP register $F3 currently reads
    /// from / writes to. Real hardware also lets $F2 be read back as a
    /// plain register holding this same value.
    dsp_addr: u8,
}

/// Stage-1 prescaler divisor (in SPC700 steps) for each timer: timers 0/1
/// run at 8KHz, timer 2 at 64KHz -- an 8x faster base rate.
const TIMER_PRESCALER_DIVISOR: [u32; 3] = [128, 128, 16];

/// The 4+4 one-way communication latches between the main CPU and the
/// SPC700 ($2140-$2143 / $F4-$F7). Real hardware keeps these fully
/// independent per direction: the main CPU's writes are only ever read by
/// the SPC700, and the SPC700's writes are only ever read by the main CPU.
/// Shared via `Arc<Mutex<>>` between `Apu` (the main-CPU-facing side) and
/// `Spc700` (which reads/writes these at RAM addresses $F4-$F7) so real
/// SPC700 execution and the main CPU observe the same live state.
#[derive(Debug, Clone, Default)]
pub struct ApuPorts {
    /// Written by the main CPU, read by the SPC700.
    cpu_to_apu: [u8; 4],
    /// Written by the SPC700, read by the main CPU.
    apu_to_cpu: [u8; 4],
}

#[derive(Clone, Copy)]
/// Processor Status Word flags
pub struct Psw {
    /// Negative flag
    pub n: bool,
    /// Overflow flag
    pub v: bool,
    /// Polarity flag (SPC700 specific)
    pub p: bool,
    /// Zero flag
    pub z: bool,
    /// Carry flag
    pub c: bool,
    /// Interrupt disable
    pub i: bool,
    /// Half carry flag (BCD operations)
    pub h: bool,
    /// Break flag
    pub b: bool,
    /// Unused/always 1
    pub u: bool,
    /// Direct page flag (SPC700 specific)
    pub d: bool,
}

impl Psw {
    pub fn new() -> Self {
        Psw {
            n: false,
            v: false,
            p: false,
            z: false,
            c: false,
            i: false,
            h: false,
            b: false,
            u: true,
            d: false,
        }
    }

    /// Real SPC700 PSW byte layout (bit7..bit0): `N V P B H I Z C`. `i`
    /// (Interrupt enable) previously read from bit 0x10 -- which is
    /// actually `B`'s (Break) real position -- and `b` was never modeled
    /// at all (`from_byte` always forced it to `false`, discarding
    /// whatever a real PUSH PSW/POP PSW round trip or the SPC700's own
    /// BRK instruction had set). Fixed to read each flag from its real
    /// bit.
    pub fn from_byte(b: u8) -> Self {
        Psw {
            n: (b & 0x80) != 0,
            v: (b & 0x40) != 0,
            p: (b & 0x20) != 0,
            b: (b & 0x10) != 0,
            h: (b & 0x08) != 0,
            i: (b & 0x04) != 0,
            z: (b & 0x02) != 0,
            c: (b & 0x01) != 0,
            u: true,
            d: false,
        }
    }

    /// See `from_byte`'s doc comment for the real bit layout this must
    /// match. This used to also unconditionally OR in `0x20` after
    /// already encoding `p` at that same bit -- a leftover 6502-ism (that
    /// CPU's status byte always reads back with its unused bit 5 forced
    /// to 1) that doesn't apply here: the SPC700's bit 0x20 is `P`, a
    /// real, software-visible, independently-settable flag (SETP/CLRP),
    /// not a fixed 1. Forcing it meant `to_byte` could never actually
    /// report `p == false` no matter what SETP/CLRP had done.
    pub fn to_byte(&self) -> u8 {
        (if self.n { 0x80 } else { 0 })
            | (if self.v { 0x40 } else { 0 })
            | (if self.p { 0x20 } else { 0 })
            | (if self.b { 0x10 } else { 0 })
            | (if self.h { 0x08 } else { 0 })
            | (if self.i { 0x04 } else { 0 })
            | (if self.z { 0x02 } else { 0 })
            | (if self.c { 0x01 } else { 0 })
    }
}

impl Default for Psw {
    fn default() -> Self {
        Self::new()
    }
}

impl Spc700 {
    /// Create a new SPC700 CPU instance
    pub fn new(ram: Arc<Mutex<[u8; 65536]>>, ports: Arc<Mutex<ApuPorts>>, dsp: Arc<Mutex<Dsp>>) -> Self {
        Spc700 {
            a: 0,
            x: 0,
            y: 0,
            sp: 0xFF,
            pc: 0xFFC0, // Reset vector ($FFFE-$FFFF) points here once the IPL ROM is loaded
            psw: Psw::new(),
            cycles_remaining: 0,
            ram,
            ports,
            halted: None,
            timer_enable: [false; 3],
            timer_target: [0; 3],
            timer_divider: [0; 3],
            timer_counter: [0; 3],
            timer_prescaler: [0; 3],
            control: 0,
            dsp,
            dsp_addr: 0,
        }
    }

    /// Advances the timer hardware by one SPC700 instruction-step. See the
    /// `timer_*` fields' doc comment for the verified behavior being
    /// modeled.
    fn tick_timers(&mut self) {
        for i in 0..3 {
            if !self.timer_enable[i] {
                continue;
            }
            self.timer_prescaler[i] += 1;
            if self.timer_prescaler[i] >= TIMER_PRESCALER_DIVISOR[i] {
                self.timer_prescaler[i] = 0;
                self.timer_divider[i] = self.timer_divider[i].wrapping_add(1);
                if self.timer_divider[i] == self.timer_target[i] {
                    self.timer_divider[i] = 0;
                    self.timer_counter[i] = (self.timer_counter[i] + 1) & 0x0F;
                }
            }
        }
    }

    /// Reset the SPC700 to initial state
    pub fn reset(&mut self) {
        self.a = 0;
        self.x = 0;
        self.y = 0;
        self.sp = 0xFF;
        self.psw = Psw::new();
        self.halted = None;
        // Real hardware also resets the timer hardware (enables, targets,
        // dividers, output counters, prescalers) and the $F1 control byte
        // to 0 -- previously left at whatever they were before reset, so a
        // reset APU could still have live timers ticking from a previous
        // run, or a driver that expects timers disabled after reset could
        // read a stale nonzero counter.
        self.timer_enable = [false; 3];
        self.timer_target = [0; 3];
        self.timer_divider = [0; 3];
        self.timer_counter = [0; 3];
        self.timer_prescaler = [0; 3];
        self.control = 0;
        // The $F2 DSP-register-address latch also resets to 0 on real
        // hardware -- previously left pointing at whatever register was
        // last selected before reset, so a read of $F3 immediately after
        // reset (before any real `MOV $F2,#reg`) could return a stale
        // register's data instead of register $00's.
        self.dsp_addr = 0;
        // The SPC700's reset vector is at $FFFE-$FFFF (NOT $FFFC-$FFFD,
        // which is the unrelated 65816 convention -- a real bug here in an
        // earlier version of this code that didn't matter while RAM was
        // always all-zero, but would matter once real IPL ROM bytes are
        // loaded at $FFC0-$FFFF).
        let low = self.read_mem(0xFFFE) as u16;
        let high = self.read_mem(0xFFFF) as u16;
        self.pc = (high << 8) | low;
        self.cycles_remaining = 0;
    }

    /// Read from memory. $00F4-$00F7 are not real RAM -- on real hardware
    /// they're hardwired to the CPU communication ports (the SPC700-side
    /// view of $2140-$2143), so they're special-cased here too. $F3 is
    /// likewise not RAM: it's the DSP register *data* port, indirectly
    /// addressing whichever register $F2 last selected -- real DSP
    /// registers do not otherwise appear anywhere in the SPC700's normal
    /// $0000-$FFFF address space.
    pub fn read_mem(&mut self, addr: u16) -> u8 {
        if (0xF4..=0xF7).contains(&addr) {
            self.ports.lock().unwrap().cpu_to_apu[(addr - 0xF4) as usize]
        } else if addr == 0xF3 {
            self.dsp.lock().unwrap().read_reg(self.dsp_addr)
        } else if addr == 0xF2 {
            self.dsp_addr
        } else if (0xFD..=0xFF).contains(&addr) {
            // Timer output counters: reading resets the counter to 0.
            let i = (addr - 0xFD) as usize;
            let value = self.timer_counter[i];
            self.timer_counter[i] = 0;
            value
        } else if addr == 0xF1 {
            self.control
        } else {
            self.ram.lock().unwrap()[addr as usize]
        }
    }

    /// Write to memory. See `read_mem` for why $00F4-$00F7, $F2/$F3, and
    /// the timer registers ($F1, $FA-$FF) are special. Missing the $F2/$F3
    /// indirection used to be a real, silent bug here: every uploaded
    /// sound driver's `MOV $F2,#reg` / `MOV $F3,#value` pair (the *only*
    /// way real SPC700 code ever reaches the DSP) fell through to the
    /// plain-RAM branch instead, so KON/MVOL/every other DSP register
    /// stayed at their power-on value forever regardless of how much
    /// music-driver code genuinely executed -- the SPC700 CPU, its RAM,
    /// and the DSP's own synthesis math were all individually correct,
    /// but nothing ever connected the driver's register writes to the
    /// DSP that would act on them.
    pub fn write_mem(&mut self, addr: u16, value: u8) {
        if (0xF4..=0xF7).contains(&addr) {
            self.ports.lock().unwrap().apu_to_cpu[(addr - 0xF4) as usize] = value;
        } else if addr == 0xF2 {
            self.dsp_addr = value;
        } else if addr == 0xF3 {
            self.dsp.lock().unwrap().write_reg(self.dsp_addr, value);
        } else if (0xFA..=0xFC).contains(&addr) {
            self.timer_target[(addr - 0xFA) as usize] = value;
        } else if addr == 0xF1 {
            self.control = value;
            for i in 0..3 {
                self.timer_enable[i] = (value & (1 << i)) != 0;
            }
            // Real hardware also clears each timer's divider/counter when
            // its enable bit transitions 0->1; approximated here by
            // always resetting a timer's internal state while disabled,
            // so re-enabling always starts from a clean count.
            for i in 0..3 {
                if !self.timer_enable[i] {
                    self.timer_divider[i] = 0;
                    self.timer_prescaler[i] = 0;
                    self.timer_counter[i] = 0;
                }
            }
        } else {
            self.ram.lock().unwrap()[addr as usize] = value;
        }
    }

    /// Push byte onto stack
    pub fn push_stack(&mut self, value: u8) {
        self.write_mem(0x0100 | (self.sp as u16), value);
        self.sp = self.sp.wrapping_sub(1);
    }

    /// Pop byte from stack
    pub fn pop_stack(&mut self) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        self.read_mem(0x0100 | (self.sp as u16))
    }

    /// Set zero and negative flags based on value
    pub fn set_zn(&mut self, value: u8) {
        self.psw.z = value == 0;
        self.psw.n = (value & 0x80) != 0;
    }

    /// Execute one instruction
    pub fn step(&mut self) -> u32 {
        if self.halted.is_some() {
            return 2;
        }
        self.tick_timers();
        let opcode = self.read_mem(self.pc);
        self.pc = self.pc.wrapping_add(1);

        self.execute_opcode(opcode)
    }

    fn fetch_u8(&mut self) -> u8 {
        let value = self.read_mem(self.pc);
        self.pc = self.pc.wrapping_add(1);
        value
    }

    fn fetch_u16(&mut self) -> u16 {
        let lo = self.fetch_u8() as u16;
        let hi = self.fetch_u8() as u16;
        (hi << 8) | lo
    }

    /// Core 8-bit add-with-carry: computes `a + operand + C`, setting
    /// C/V/H/Z/N, and returns the result. Shared by the A-targeted ADC/SBC
    /// forms and the memory-targeted `ADC dp,dp` / `ADC dp,#imm` /
    /// `ADC (X),(Y)` family (which store the result back to memory rather
    /// than A). H (half-carry from bit 3) matters beyond PUSH/POP PSW
    /// fidelity: DAA/DAS read it to decimal-adjust correctly.
    fn adc_generic(&mut self, a: u8, operand: u8) -> u8 {
        let c = self.psw.c as u16;
        let result = (a as u16) + (operand as u16) + c;
        self.psw.c = result > 0xFF;
        self.psw.h = ((a & 0x0F) as u16 + (operand & 0x0F) as u16 + c) > 0x0F;
        let overflow = (!(a ^ operand) & (a ^ (result as u8)) & 0x80) != 0;
        self.psw.v = overflow;
        let result = result as u8;
        self.set_zn(result);
        result
    }

    /// Core 8-bit subtract-with-borrow -- ADC with the operand's one's
    /// complement, the standard 6502/SPC700 equivalence (H follows the
    /// internal add, the hardware convention).
    fn sbc_generic(&mut self, a: u8, operand: u8) -> u8 {
        self.adc_generic(a, !operand)
    }

    /// ADC A,operand (8-bit, sets C/V/H/Z/N)
    fn adc8(&mut self, operand: u8) {
        self.a = self.adc_generic(self.a, operand);
    }

    /// SBC A,operand (8-bit, sets C/V/H/Z/N)
    fn sbc8(&mut self, operand: u8) {
        self.a = self.sbc_generic(self.a, operand);
    }

    /// Fetches the 16-bit `m.b` (absolute-address.bit) operand used by the
    /// carry-bit instructions (OR1/AND1/EOR1/MOV1/NOT1): the low 13 bits
    /// are a plain absolute address, the high 3 bits select the bit.
    fn fetch_abs_bit(&mut self) -> (u16, u8) {
        let word = self.fetch_u16();
        (word & 0x1FFF, ((word >> 13) & 0x07) as u8)
    }

    // Shared operand fetchers for the remaining ALU addressing modes
    // (opcode values verified against wiki.superfamicom.org/spc700-reference).
    fn operand_indirect_x(&mut self) -> u8 {
        self.read_mem(self.dp_addr(self.x))
    }
    fn operand_dp_x(&mut self) -> u8 {
        let dp = self.fetch_u8().wrapping_add(self.x);
        self.read_mem(self.dp_addr(dp))
    }
    fn operand_abs_x(&mut self) -> u8 {
        let addr = self.fetch_u16().wrapping_add(self.x as u16);
        self.read_mem(addr)
    }
    fn operand_abs_y(&mut self) -> u8 {
        let addr = self.fetch_u16().wrapping_add(self.y as u16);
        self.read_mem(addr)
    }
    fn operand_indirect_dp_x(&mut self) -> u8 {
        // [dp+X]
        let dp = self.fetch_u8().wrapping_add(self.x);
        let lo = self.read_mem(self.dp_addr(dp)) as u16;
        let hi = self.read_mem(self.dp_addr(dp.wrapping_add(1))) as u16;
        self.read_mem((hi << 8) | lo)
    }
    fn operand_indirect_dp_y(&mut self) -> u8 {
        // [dp]+Y
        let dp = self.fetch_u8();
        let lo = self.read_mem(self.dp_addr(dp)) as u16;
        let hi = self.read_mem(self.dp_addr(dp.wrapping_add(1))) as u16;
        let base = (hi << 8) | lo;
        self.read_mem(base.wrapping_add(self.y as u16))
    }

    /// Computes the effective address of a direct-page-addressed operand:
    /// $0000-$00FF when the PSW P (direct page select) flag is clear, or
    /// $0100-$01FF when it's set (real hardware; toggled by the SETP/CLRP
    /// opcodes). Every direct-page addressing mode -- dp, dp+X, dp+Y, and
    /// (X)/(X)+ (which use the X register itself as the direct-page
    /// offset) -- must resolve through this, including the pointer bytes
    /// of the indirect [dp+X]/[dp]+Y modes (though NOT the final address
    /// those pointers resolve to, which is a full, page-independent
    /// 16-bit address). Previously every call site computed `dp as u16`
    /// directly, so SETP/CLRP silently had no effect on any real memory
    /// access -- P could be set and cleared, but no instruction ever
    /// looked at it.
    fn dp_addr(&self, dp: u8) -> u16 {
        (dp as u16) | if self.psw.p { 0x100 } else { 0 }
    }

    /// CMP-style comparison: sets N/Z/C as if computing `a - b` (unsigned,
    /// carry set when no borrow is needed i.e. a >= b) without storing the
    /// result -- both operands are left unchanged.
    fn cmp8(&mut self, a: u8, b: u8) {
        let result = a.wrapping_sub(b);
        self.psw.z = result == 0;
        self.psw.n = (result & 0x80) != 0;
        self.psw.c = a >= b;
    }

    /// Executes one real SPC700 opcode.
    ///
    /// This implements exactly the opcode subset used by the real,
    /// well-known 64-byte SPC700 IPL boot ROM (the one loaded by
    /// `Apu::new` at $FFC0-$FFFF) -- verified byte-for-byte against
    /// sneslab.net/wiki/SPC700/IPL_ROM and snes.nesdev.org/wiki/Booting_the_SPC700
    /// (fetched 2026-06-30), and cross-checked by confirming every computed
    /// branch target in the ROM lands exactly on one of its two named
    /// labels ("Trans"/"Start") -- this is not a guess at the encoding.
    ///
    /// This intentionally does NOT implement the full SPC700 instruction
    /// set (a much larger undertaking): any opcode outside this set halts
    /// the SPC700 (see `halted`) instead of executing wrong/undefined
    /// behavior. The original dispatch this replaced used 6502 opcode
    /// *values* with 6502 *semantics* -- since the SPC700's real encoding
    /// is a completely different, custom mapping, it could never have
    /// correctly executed real SPC700 machine code (including this exact
    /// IPL ROM) even though it looked like a complete CPU core.
    fn execute_opcode(&mut self, opcode: u8) -> u32 {
        match opcode {
            0xCD => {
                // MOV X,#imm
                self.x = self.fetch_u8();
                self.set_zn(self.x);
                2
            }
            0xBD => {
                // MOV SP,X (no flags)
                self.sp = self.x;
                2
            }
            0xE8 => {
                // MOV A,#imm
                self.a = self.fetch_u8();
                self.set_zn(self.a);
                2
            }
            0xC6 => {
                // MOV (X),A -- direct page address X (page 0)
                self.write_mem(self.dp_addr(self.x), self.a);
                4
            }
            0x1D => {
                // DEC X
                self.x = self.x.wrapping_sub(1);
                self.set_zn(self.x);
                2
            }
            0xFC => {
                // INC Y
                self.y = self.y.wrapping_add(1);
                self.set_zn(self.y);
                2
            }
            0xD0 => {
                // BNE rel
                let rel = self.fetch_u8() as i8;
                if !self.psw.z {
                    self.pc = self.pc.wrapping_add(rel as u16);
                }
                2
            }
            0x10 => {
                // BPL rel
                let rel = self.fetch_u8() as i8;
                if !self.psw.n {
                    self.pc = self.pc.wrapping_add(rel as u16);
                }
                2
            }
            0x2F => {
                // BRA rel (always taken)
                let rel = self.fetch_u8() as i8;
                self.pc = self.pc.wrapping_add(rel as u16);
                4
            }
            0x8F => {
                // MOV dp,#imm -- operand order is imm, then dp (confirmed
                // by the ROM's own "MOV $F4,#$AA" / "MOV $F5,#$BB" bytes)
                let imm = self.fetch_u8();
                let dp = self.fetch_u8();
                self.write_mem(self.dp_addr(dp), imm);
                5
            }
            0x78 => {
                // CMP dp,#imm -- same imm-then-dp operand order as 0x8F
                let imm = self.fetch_u8();
                let dp = self.fetch_u8();
                let value = self.read_mem(self.dp_addr(dp));
                self.cmp8(value, imm);
                4
            }
            0xEB => {
                // MOV Y,dp
                let dp = self.fetch_u8();
                self.y = self.read_mem(self.dp_addr(dp));
                self.set_zn(self.y);
                3
            }
            0x7E => {
                // CMP Y,dp
                let dp = self.fetch_u8();
                let value = self.read_mem(self.dp_addr(dp));
                self.cmp8(self.y, value);
                3
            }
            0xE4 => {
                // MOV A,dp
                let dp = self.fetch_u8();
                self.a = self.read_mem(self.dp_addr(dp));
                self.set_zn(self.a);
                3
            }
            0xCB => {
                // MOV dp,Y (no flags)
                let dp = self.fetch_u8();
                self.write_mem(self.dp_addr(dp), self.y);
                4
            }
            0xC4 => {
                // MOV dp,A (no flags)
                let dp = self.fetch_u8();
                self.write_mem(self.dp_addr(dp), self.a);
                4
            }
            0xAB => {
                // INC dp
                let dp = self.fetch_u8();
                let value = self.read_mem(self.dp_addr(dp)).wrapping_add(1);
                self.write_mem(self.dp_addr(dp), value);
                self.set_zn(value);
                4
            }
            0xD7 => {
                // MOV [dp]+Y,A -- indirect (24-bit-style 16-bit pointer at
                // dp/dp+1 in page 0) indexed by Y, used by the IPL to write
                // an uploaded byte into the destination address it was given
                let dp = self.fetch_u8();
                let ptr_lo = self.read_mem(self.dp_addr(dp)) as u16;
                let ptr_hi = self.read_mem(self.dp_addr(dp.wrapping_add(1))) as u16;
                let addr = (ptr_lo | (ptr_hi << 8)).wrapping_add(self.y as u16);
                self.write_mem(addr, self.a);
                6
            }
            0xBA => {
                // MOVW YA,dp -- 16-bit load: A = low byte at dp, Y = high
                // byte at dp+1; N/Z reflect the combined 16-bit value
                let dp = self.fetch_u8();
                let lo = self.read_mem(self.dp_addr(dp));
                let hi = self.read_mem(self.dp_addr(dp.wrapping_add(1)));
                self.a = lo;
                self.y = hi;
                let word = ((hi as u16) << 8) | (lo as u16);
                self.psw.z = word == 0;
                self.psw.n = (word & 0x8000) != 0;
                5
            }
            0xDA => {
                // MOVW dp,YA -- 16-bit store: low byte (A) at dp, high byte
                // (Y) at dp+1 (no flags)
                let dp = self.fetch_u8();
                self.write_mem(self.dp_addr(dp), self.a);
                self.write_mem(self.dp_addr(dp.wrapping_add(1)), self.y);
                5
            }
            0xDD => {
                // MOV A,Y
                self.a = self.y;
                self.set_zn(self.a);
                2
            }
            0x5D => {
                // MOV X,A
                self.x = self.a;
                self.set_zn(self.x);
                2
            }
            0x1F => {
                // JMP [!abs+X] -- double-indirect: read a 16-bit pointer
                // from (abs+X), then jump to the 16-bit value stored there.
                // The IPL uses this with abs=$0000 to jump through the
                // address the main CPU staged at RAM $00-$01.
                let lo = self.fetch_u8() as u16;
                let hi = self.fetch_u8() as u16;
                let ptr = (hi << 8 | lo).wrapping_add(self.x as u16);
                let target_lo = self.read_mem(ptr) as u16;
                let target_hi = self.read_mem(ptr.wrapping_add(1)) as u16;
                self.pc = (target_hi << 8) | target_lo;
                6
            }

            // ============================================================
            // Extended opcode set, added to run real uploaded SPC700 sound
            // driver code (beyond the IPL ROM itself). Every opcode value
            // below is taken directly from the verified, complete SPC700
            // instruction chart at wiki.superfamicom.org/spc700-reference
            // (cross-checked against the IPL ROM opcodes above, all of
            // which matched exactly) -- not inferred from context.
            // ============================================================

            0x00 => 2, // NOP
            0x5F => { self.pc = self.fetch_u16(); 3 } // JMP !abs
            0x09 => {
                // OR dp,dp -- same src-then-dst byte order as MOV dp,dp
                // (0xFA): result = dst | src, stored back to dst, flags
                // from the result.
                let src_dp = self.fetch_u8();
                let dst_dp = self.fetch_u8();
                let src_val = self.read_mem(self.dp_addr(src_dp));
                let dst_val = self.read_mem(self.dp_addr(dst_dp));
                let result = dst_val | src_val;
                self.write_mem(self.dp_addr(dst_dp), result);
                self.set_zn(result);
                6
            }

            // Flag operations
            0x60 => { self.psw.c = false; 2 } // CLRC
            0x80 => { self.psw.c = true; 2 } // SETC
            0xED => { self.psw.c = !self.psw.c; 2 } // NOTC
            0xE0 => { self.psw.v = false; self.psw.h = false; 2 } // CLRV (also clears H)
            0x20 => { self.psw.p = false; 2 } // CLRP
            0x40 => { self.psw.p = true; 2 } // SETP
            0xA0 => { self.psw.i = true; 2 } // EI
            0xC0 => { self.psw.i = false; 2 } // DI

            // Register-to-register MOV (no flags except where noted)
            0x7D => { self.a = self.x; self.set_zn(self.a); 2 } // MOV A,X
            0xFD => { self.y = self.a; self.set_zn(self.y); 2 } // MOV Y,A (wiki: sets flags)
            0x9D => { self.x = self.sp; self.set_zn(self.x); 2 } // MOV X,SP

            // MOV A,(X) / (X)+ / (X),A / (X)+,A
            0xE6 => { self.a = self.read_mem(self.dp_addr(self.x)); self.set_zn(self.a); 3 } // MOV A,(X)
            0xBF => { // MOV A,(X)+
                self.a = self.read_mem(self.dp_addr(self.x));
                self.x = self.x.wrapping_add(1);
                self.set_zn(self.a);
                4
            }
            0xAF => { // MOV (X)+,A
                self.write_mem(self.dp_addr(self.x), self.a);
                self.x = self.x.wrapping_add(1);
                4
            }
            0xE7 => {
                // MOV A,[d+X] -- 6502-style "(zp,X)": a 16-bit pointer
                // lives at direct-page (dp+X) (wrapping within page 0),
                // and A is loaded from the byte at that pointer. Found
                // missing when it halted the SPC700 mid-sound-engine-
                // upload, silently leaving the CPU spinning forever on a
                // $2140/$2141 handshake the dead SPC700 could never answer.
                let dp = self.fetch_u8().wrapping_add(self.x);
                let ptr_lo = self.read_mem(self.dp_addr(dp)) as u16;
                let ptr_hi = self.read_mem(self.dp_addr(dp.wrapping_add(1))) as u16;
                let addr = ptr_lo | (ptr_hi << 8);
                self.a = self.read_mem(addr);
                self.set_zn(self.a);
                6
            }
            0xF7 => {
                // MOV A,[d]+Y -- 6502-style "(zp),Y": a 16-bit pointer
                // lives at direct-page `dp` (unindexed), and A is loaded
                // from that pointer plus Y (the addition happens *after*
                // the pointer is read, unlike 0xE7 where X offsets the
                // direct-page fetch itself). Found missing the same way
                // 0xE7 was: it halted the SPC700 partway through the
                // uploaded sound engine's own driver code, right where
                // real hardware would start actually triggering notes --
                // the DSP's synthesis primitives (envelopes, BRR decode)
                // were already implemented and unit-tested, but nothing
                // ever reached them because the driver never got this far.
                let dp = self.fetch_u8();
                let ptr_lo = self.read_mem(self.dp_addr(dp)) as u16;
                let ptr_hi = self.read_mem(self.dp_addr(dp.wrapping_add(1))) as u16;
                let addr = (ptr_lo | (ptr_hi << 8)).wrapping_add(self.y as u16);
                self.a = self.read_mem(addr);
                self.set_zn(self.a);
                6
            }

            // MOV A,dp+X / dp / !abs / !abs+X / !abs+Y
            0xF4 => { let dp = self.fetch_u8().wrapping_add(self.x); self.a = self.read_mem(self.dp_addr(dp)); self.set_zn(self.a); 4 } // MOV A,dp+X
            0xE5 => { let addr = self.fetch_u16(); self.a = self.read_mem(addr); self.set_zn(self.a); 4 } // MOV A,!abs
            0xF5 => { let addr = self.fetch_u16().wrapping_add(self.x as u16); self.a = self.read_mem(addr); self.set_zn(self.a); 5 } // MOV A,!abs+X
            0xF6 => { let addr = self.fetch_u16().wrapping_add(self.y as u16); self.a = self.read_mem(addr); self.set_zn(self.a); 5 } // MOV A,!abs+Y

            // MOV dp+X,A / !abs,A / !abs+X,A / !abs+Y,A
            0xD4 => { let dp = self.fetch_u8().wrapping_add(self.x); self.write_mem(self.dp_addr(dp), self.a); 5 } // MOV dp+X,A
            0xC5 => { let addr = self.fetch_u16(); self.write_mem(addr, self.a); 5 } // MOV !abs,A
            0xD5 => { let addr = self.fetch_u16().wrapping_add(self.x as u16); self.write_mem(addr, self.a); 6 } // MOV !abs+X,A
            0xD6 => { let addr = self.fetch_u16().wrapping_add(self.y as u16); self.write_mem(addr, self.a); 6 } // MOV !abs+Y,A

            // MOV X/Y <-> dp/!abs
            0xF8 => { let dp = self.fetch_u8(); self.x = self.read_mem(self.dp_addr(dp)); self.set_zn(self.x); 3 } // MOV X,dp
            0xF9 => { let dp = self.fetch_u8().wrapping_add(self.y); self.x = self.read_mem(self.dp_addr(dp)); self.set_zn(self.x); 4 } // MOV X,dp+Y
            0xE9 => { let addr = self.fetch_u16(); self.x = self.read_mem(addr); self.set_zn(self.x); 4 } // MOV X,!abs
            0xD8 => { let dp = self.fetch_u8(); self.write_mem(self.dp_addr(dp), self.x); 4 } // MOV dp,X
            0xD9 => { let dp = self.fetch_u8().wrapping_add(self.y); self.write_mem(self.dp_addr(dp), self.x); 5 } // MOV dp+Y,X
            0xC9 => { let addr = self.fetch_u16(); self.write_mem(addr, self.x); 5 } // MOV !abs,X
            0xEC => { let addr = self.fetch_u16(); self.y = self.read_mem(addr); self.set_zn(self.y); 4 } // MOV Y,!abs
            0xDB => { let dp = self.fetch_u8().wrapping_add(self.x); self.write_mem(self.dp_addr(dp), self.y); 5 } // MOV dp+X,Y
            0xCC => { let addr = self.fetch_u16(); self.write_mem(addr, self.y); 5 } // MOV !abs,Y

            0x8D => { self.y = self.fetch_u8(); self.set_zn(self.y); 2 } // MOV Y,#imm
            0xFA => {
                // MOV dp,dp -- like every other direct-page opcode, both
                // the source and destination addresses must respect the
                // PSW.P flag (resolving to page $01xx instead of $00xx
                // when set), via `dp_addr`. This previously fetched the
                // raw operand bytes and cast them straight to u16, always
                // treating the direct page as $00xx regardless of P --
                // unlike every sibling dp-addressed opcode above/below,
                // which all go through `self.dp_addr(...)`.
                let src = self.fetch_u8();
                let dst = self.fetch_u8();
                let v = self.read_mem(self.dp_addr(src));
                self.write_mem(self.dp_addr(dst), v);
                5
            } // MOV dp,dp
            0xFB => { let dp = self.fetch_u8().wrapping_add(self.x); self.y = self.read_mem(self.dp_addr(dp)); self.set_zn(self.y); 4 } // MOV Y,dp+X

            // 8-bit ALU on A: #imm / dp / !abs (the most common addressing forms)
            0x88 => { let v = self.fetch_u8(); self.adc8(v); 2 } // ADC A,#imm
            0x84 => { let dp = self.fetch_u8(); let v = self.read_mem(self.dp_addr(dp)); self.adc8(v); 3 } // ADC A,dp
            0x85 => { let addr = self.fetch_u16(); let v = self.read_mem(addr); self.adc8(v); 4 } // ADC A,!abs

            0xA8 => { let v = self.fetch_u8(); self.sbc8(v); 2 } // SBC A,#imm
            0xA4 => { let dp = self.fetch_u8(); let v = self.read_mem(self.dp_addr(dp)); self.sbc8(v); 3 } // SBC A,dp
            0xA5 => { let addr = self.fetch_u16(); let v = self.read_mem(addr); self.sbc8(v); 4 } // SBC A,!abs

            0x28 => { let v = self.fetch_u8(); self.a &= v; self.set_zn(self.a); 2 } // AND A,#imm
            0x24 => { let dp = self.fetch_u8(); let v = self.read_mem(self.dp_addr(dp)); self.a &= v; self.set_zn(self.a); 3 } // AND A,dp
            0x25 => { let addr = self.fetch_u16(); let v = self.read_mem(addr); self.a &= v; self.set_zn(self.a); 4 } // AND A,!abs

            0x08 => { let v = self.fetch_u8(); self.a |= v; self.set_zn(self.a); 2 } // OR A,#imm
            0x04 => { let dp = self.fetch_u8(); let v = self.read_mem(self.dp_addr(dp)); self.a |= v; self.set_zn(self.a); 3 } // OR A,dp
            0x05 => { let addr = self.fetch_u16(); let v = self.read_mem(addr); self.a |= v; self.set_zn(self.a); 4 } // OR A,!abs

            0x48 => { let v = self.fetch_u8(); self.a ^= v; self.set_zn(self.a); 2 } // EOR A,#imm
            0x44 => { let dp = self.fetch_u8(); let v = self.read_mem(self.dp_addr(dp)); self.a ^= v; self.set_zn(self.a); 3 } // EOR A,dp
            0x45 => { let addr = self.fetch_u16(); let v = self.read_mem(addr); self.a ^= v; self.set_zn(self.a); 4 } // EOR A,!abs

            // Compares
            0x68 => { let v = self.fetch_u8(); self.cmp8(self.a, v); 2 } // CMP A,#imm
            0x64 => { let dp = self.fetch_u8(); let v = self.read_mem(self.dp_addr(dp)); self.cmp8(self.a, v); 3 } // CMP A,dp
            0x65 => { let addr = self.fetch_u16(); let v = self.read_mem(addr); self.cmp8(self.a, v); 4 } // CMP A,!abs
            0xC8 => { let v = self.fetch_u8(); self.cmp8(self.x, v); 2 } // CMP X,#imm
            0x3E => { let dp = self.fetch_u8(); let v = self.read_mem(self.dp_addr(dp)); self.cmp8(self.x, v); 3 } // CMP X,dp
            0xAD => { let v = self.fetch_u8(); self.cmp8(self.y, v); 2 } // CMP Y,#imm
            0x5E => { let addr = self.fetch_u16(); let v = self.read_mem(addr); self.cmp8(self.y, v); 4 } // CMP Y,!abs
            0x1E => { let addr = self.fetch_u16(); let v = self.read_mem(addr); self.cmp8(self.x, v); 4 } // CMP X,!abs

            // Branches
            0xF0 => { let rel = self.fetch_u8() as i8; if self.psw.z { self.pc = self.pc.wrapping_add(rel as u16); } 2 } // BEQ
            0xB0 => { let rel = self.fetch_u8() as i8; if self.psw.c { self.pc = self.pc.wrapping_add(rel as u16); } 2 } // BCS
            0x90 => { let rel = self.fetch_u8() as i8; if !self.psw.c { self.pc = self.pc.wrapping_add(rel as u16); } 2 } // BCC
            0x70 => { let rel = self.fetch_u8() as i8; if self.psw.v { self.pc = self.pc.wrapping_add(rel as u16); } 2 } // BVS
            0x50 => { let rel = self.fetch_u8() as i8; if !self.psw.v { self.pc = self.pc.wrapping_add(rel as u16); } 2 } // BVC
            0x30 => { let rel = self.fetch_u8() as i8; if self.psw.n { self.pc = self.pc.wrapping_add(rel as u16); } 2 } // BMI

            // Increment/Decrement
            0xBC => { self.a = self.a.wrapping_add(1); self.set_zn(self.a); 2 } // INC A
            0x9C => { self.a = self.a.wrapping_sub(1); self.set_zn(self.a); 2 } // DEC A
            0x3D => { self.x = self.x.wrapping_add(1); self.set_zn(self.x); 2 } // INC X
            0xDC => { self.y = self.y.wrapping_sub(1); self.set_zn(self.y); 2 } // DEC Y
            0xBB => { let dp = self.fetch_u8().wrapping_add(self.x); let v = self.read_mem(self.dp_addr(dp)).wrapping_add(1); self.write_mem(self.dp_addr(dp), v); self.set_zn(v); 5 } // INC dp+X
            0x8B => { let dp = self.fetch_u8(); let v = self.read_mem(self.dp_addr(dp)).wrapping_sub(1); self.write_mem(self.dp_addr(dp), v); self.set_zn(v); 4 } // DEC dp
            0x9B => { let dp = self.fetch_u8().wrapping_add(self.x); let v = self.read_mem(self.dp_addr(dp)).wrapping_sub(1); self.write_mem(self.dp_addr(dp), v); self.set_zn(v); 5 } // DEC dp+X
            0xAC => { let addr = self.fetch_u16(); let v = self.read_mem(addr).wrapping_add(1); self.write_mem(addr, v); self.set_zn(v); 5 } // INC !abs
            0x8C => { let addr = self.fetch_u16(); let v = self.read_mem(addr).wrapping_sub(1); self.write_mem(addr, v); self.set_zn(v); 5 } // DEC !abs

            // Shift/rotate on A
            0x1C => { let c = (self.a & 0x80) != 0; self.a = self.a.wrapping_shl(1); self.psw.c = c; self.set_zn(self.a); 2 } // ASL A
            0x5C => { let c = (self.a & 1) != 0; self.a >>= 1; self.psw.c = c; self.set_zn(self.a); 2 } // LSR A
            0x3C => { let c_in = self.psw.c; let c_out = (self.a & 0x80) != 0; self.a = (self.a << 1) | (c_in as u8); self.psw.c = c_out; self.set_zn(self.a); 2 } // ROL A
            0x7C => { let c_in = self.psw.c; let c_out = (self.a & 1) != 0; self.a = (self.a >> 1) | ((c_in as u8) << 7); self.psw.c = c_out; self.set_zn(self.a); 2 } // ROR A

            // Stack
            0x2D => { self.push_stack(self.a); 4 } // PUSH A
            0x4D => { self.push_stack(self.x); 4 } // PUSH X
            0x6D => { self.push_stack(self.y); 4 } // PUSH Y
            0x0D => { self.push_stack(self.psw.to_byte()); 4 } // PUSH PSW
            0xAE => { self.a = self.pop_stack(); 4 } // POP A
            0xCE => { self.x = self.pop_stack(); 4 } // POP X
            0xEE => { self.y = self.pop_stack(); 4 } // POP Y
            0x8E => { let v = self.pop_stack(); self.psw = Psw::from_byte(v); 4 } // POP PSW

            // Subroutines
            0x6F => { // RET
                let lo = self.pop_stack() as u16;
                let hi = self.pop_stack() as u16;
                self.pc = (hi << 8) | lo;
                5
            }
            0x7F => { // RETI
                let psw_byte = self.pop_stack();
                self.psw = Psw::from_byte(psw_byte);
                let lo = self.pop_stack() as u16;
                let hi = self.pop_stack() as u16;
                self.pc = (hi << 8) | lo;
                6
            }
            0x3F => { // CALL !abs
                let target = self.fetch_u16();
                let ret_addr = self.pc;
                self.push_stack((ret_addr >> 8) as u8);
                self.push_stack((ret_addr & 0xFF) as u8);
                self.pc = target;
                8
            }

            // 16-bit word ops on YA
            0x7A => { // ADDW YA,dp
                let dp = self.fetch_u8();
                let lo = self.read_mem(self.dp_addr(dp)) as u16;
                let hi = self.read_mem(self.dp_addr(dp.wrapping_add(1))) as u16;
                let operand = (hi << 8) | lo;
                let ya = ((self.y as u16) << 8) | (self.a as u16);
                let result = (ya as u32) + (operand as u32);
                self.psw.c = result > 0xFFFF;
                let result = result as u16;
                self.a = (result & 0xFF) as u8;
                self.y = (result >> 8) as u8;
                self.psw.z = result == 0;
                self.psw.n = (result & 0x8000) != 0;
                5
            }
            0x9A => { // SUBW YA,dp
                let dp = self.fetch_u8();
                let lo = self.read_mem(self.dp_addr(dp)) as u16;
                let hi = self.read_mem(self.dp_addr(dp.wrapping_add(1))) as u16;
                let operand = (hi << 8) | lo;
                let ya = ((self.y as u16) << 8) | (self.a as u16);
                let result = ya.wrapping_sub(operand);
                self.psw.c = ya >= operand;
                self.a = (result & 0xFF) as u8;
                self.y = (result >> 8) as u8;
                self.psw.z = result == 0;
                self.psw.n = (result & 0x8000) != 0;
                5
            }
            0x5A => { // CMPW YA,dp
                let dp = self.fetch_u8();
                let lo = self.read_mem(self.dp_addr(dp)) as u16;
                let hi = self.read_mem(self.dp_addr(dp.wrapping_add(1))) as u16;
                let operand = (hi << 8) | lo;
                let ya = ((self.y as u16) << 8) | (self.a as u16);
                let result = ya.wrapping_sub(operand);
                self.psw.c = ya >= operand;
                self.psw.z = result == 0;
                self.psw.n = (result & 0x8000) != 0;
                4
            }

            // Decrement-and-branch-if-not-zero
            0xFE => { // DBNZ Y,rel
                self.y = self.y.wrapping_sub(1);
                let rel = self.fetch_u8() as i8;
                if self.y != 0 {
                    self.pc = self.pc.wrapping_add(rel as u16);
                }
                4
            }
            0x6E => { // DBNZ dp,rel
                let dp = self.fetch_u8();
                let v = self.read_mem(self.dp_addr(dp)).wrapping_sub(1);
                self.write_mem(self.dp_addr(dp), v);
                let rel = self.fetch_u8() as i8;
                if v != 0 {
                    self.pc = self.pc.wrapping_add(rel as u16);
                }
                5
            }

            // Remaining ALU addressing modes: (X), dp+X, !abs+X, !abs+Y,
            // [dp+X], [dp]+Y -- for ADC, SBC, AND, OR, EOR, CMP A,operand.
            0x86 => { let v = self.operand_indirect_x(); self.adc8(v); 6 }
            0x94 => { let v = self.operand_dp_x(); self.adc8(v); 5 }
            0x95 => { let v = self.operand_abs_x(); self.adc8(v); 6 }
            0x96 => { let v = self.operand_abs_y(); self.adc8(v); 6 }
            0x87 => { let v = self.operand_indirect_dp_x(); self.adc8(v); 6 }
            0x97 => { let v = self.operand_indirect_dp_y(); self.adc8(v); 6 }

            0xA6 => { let v = self.operand_indirect_x(); self.sbc8(v); 6 }
            0xB4 => { let v = self.operand_dp_x(); self.sbc8(v); 5 }
            0xB5 => { let v = self.operand_abs_x(); self.sbc8(v); 6 }
            0xB6 => { let v = self.operand_abs_y(); self.sbc8(v); 6 }
            0xA7 => { let v = self.operand_indirect_dp_x(); self.sbc8(v); 6 }
            0xB7 => { let v = self.operand_indirect_dp_y(); self.sbc8(v); 6 }

            0x26 => { let v = self.operand_indirect_x(); self.a &= v; self.set_zn(self.a); 6 }
            0x34 => { let v = self.operand_dp_x(); self.a &= v; self.set_zn(self.a); 5 }
            0x35 => { let v = self.operand_abs_x(); self.a &= v; self.set_zn(self.a); 6 }
            0x36 => { let v = self.operand_abs_y(); self.a &= v; self.set_zn(self.a); 6 }
            0x27 => { let v = self.operand_indirect_dp_x(); self.a &= v; self.set_zn(self.a); 6 }
            0x37 => { let v = self.operand_indirect_dp_y(); self.a &= v; self.set_zn(self.a); 6 }

            0x06 => { let v = self.operand_indirect_x(); self.a |= v; self.set_zn(self.a); 6 }
            0x14 => { let v = self.operand_dp_x(); self.a |= v; self.set_zn(self.a); 5 }
            0x15 => { let v = self.operand_abs_x(); self.a |= v; self.set_zn(self.a); 6 }
            0x16 => { let v = self.operand_abs_y(); self.a |= v; self.set_zn(self.a); 6 }
            0x07 => { let v = self.operand_indirect_dp_x(); self.a |= v; self.set_zn(self.a); 6 }
            0x17 => { let v = self.operand_indirect_dp_y(); self.a |= v; self.set_zn(self.a); 6 }

            0x46 => { let v = self.operand_indirect_x(); self.a ^= v; self.set_zn(self.a); 6 }
            0x54 => { let v = self.operand_dp_x(); self.a ^= v; self.set_zn(self.a); 5 }
            0x55 => { let v = self.operand_abs_x(); self.a ^= v; self.set_zn(self.a); 6 }
            0x56 => { let v = self.operand_abs_y(); self.a ^= v; self.set_zn(self.a); 6 }
            0x47 => { let v = self.operand_indirect_dp_x(); self.a ^= v; self.set_zn(self.a); 6 }
            0x57 => { let v = self.operand_indirect_dp_y(); self.a ^= v; self.set_zn(self.a); 6 }

            0x66 => { let v = self.operand_indirect_x(); self.cmp8(self.a, v); 6 }
            0x74 => { let v = self.operand_dp_x(); self.cmp8(self.a, v); 5 }
            0x75 => { let v = self.operand_abs_x(); self.cmp8(self.a, v); 6 }
            0x76 => { let v = self.operand_abs_y(); self.cmp8(self.a, v); 6 }
            0x67 => { let v = self.operand_indirect_dp_x(); self.cmp8(self.a, v); 6 }
            0x77 => { let v = self.operand_indirect_dp_y(); self.cmp8(self.a, v); 6 }

            // Shift/rotate on dp/dp+X/!abs (the A-only forms 1C/5C/3C/7C
            // are implemented above; opcode values verified against the
            // same SPC700 instruction chart).
            0x0B => { let dp = self.fetch_u8(); let v = self.read_mem(self.dp_addr(dp)); let c = (v & 0x80) != 0; let r = v.wrapping_shl(1); self.write_mem(self.dp_addr(dp), r); self.psw.c = c; self.set_zn(r); 4 } // ASL dp
            0x1B => { let dp = self.fetch_u8().wrapping_add(self.x); let v = self.read_mem(self.dp_addr(dp)); let c = (v & 0x80) != 0; let r = v.wrapping_shl(1); self.write_mem(self.dp_addr(dp), r); self.psw.c = c; self.set_zn(r); 5 } // ASL dp+X
            0x0C => { let addr = self.fetch_u16(); let v = self.read_mem(addr); let c = (v & 0x80) != 0; let r = v.wrapping_shl(1); self.write_mem(addr, r); self.psw.c = c; self.set_zn(r); 5 } // ASL !abs
            0x4B => { let dp = self.fetch_u8(); let v = self.read_mem(self.dp_addr(dp)); let c = (v & 1) != 0; let r = v >> 1; self.write_mem(self.dp_addr(dp), r); self.psw.c = c; self.set_zn(r); 4 } // LSR dp
            0x5B => { let dp = self.fetch_u8().wrapping_add(self.x); let v = self.read_mem(self.dp_addr(dp)); let c = (v & 1) != 0; let r = v >> 1; self.write_mem(self.dp_addr(dp), r); self.psw.c = c; self.set_zn(r); 5 } // LSR dp+X
            0x4C => { let addr = self.fetch_u16(); let v = self.read_mem(addr); let c = (v & 1) != 0; let r = v >> 1; self.write_mem(addr, r); self.psw.c = c; self.set_zn(r); 5 } // LSR !abs
            0x2B => { let dp = self.fetch_u8(); let v = self.read_mem(self.dp_addr(dp)); let c_in = self.psw.c; let c_out = (v & 0x80) != 0; let r = (v << 1) | (c_in as u8); self.write_mem(self.dp_addr(dp), r); self.psw.c = c_out; self.set_zn(r); 4 } // ROL dp
            0x3B => { let dp = self.fetch_u8().wrapping_add(self.x); let v = self.read_mem(self.dp_addr(dp)); let c_in = self.psw.c; let c_out = (v & 0x80) != 0; let r = (v << 1) | (c_in as u8); self.write_mem(self.dp_addr(dp), r); self.psw.c = c_out; self.set_zn(r); 5 } // ROL dp+X
            0x2C => { let addr = self.fetch_u16(); let v = self.read_mem(addr); let c_in = self.psw.c; let c_out = (v & 0x80) != 0; let r = (v << 1) | (c_in as u8); self.write_mem(addr, r); self.psw.c = c_out; self.set_zn(r); 5 } // ROL !abs
            0x6B => { let dp = self.fetch_u8(); let v = self.read_mem(self.dp_addr(dp)); let c_in = self.psw.c; let c_out = (v & 1) != 0; let r = (v >> 1) | ((c_in as u8) << 7); self.write_mem(self.dp_addr(dp), r); self.psw.c = c_out; self.set_zn(r); 4 } // ROR dp
            0x7B => { let dp = self.fetch_u8().wrapping_add(self.x); let v = self.read_mem(self.dp_addr(dp)); let c_in = self.psw.c; let c_out = (v & 1) != 0; let r = (v >> 1) | ((c_in as u8) << 7); self.write_mem(self.dp_addr(dp), r); self.psw.c = c_out; self.set_zn(r); 5 } // ROR dp+X
            0x6C => { let addr = self.fetch_u16(); let v = self.read_mem(addr); let c_in = self.psw.c; let c_out = (v & 1) != 0; let r = (v >> 1) | ((c_in as u8) << 7); self.write_mem(addr, r); self.psw.c = c_out; self.set_zn(r); 5 } // ROR !abs

            0x9F => { self.a = (self.a >> 4) | (self.a << 4); self.set_zn(self.a); 5 } // XCN A (exchange nibbles)

            0x3A => { // INCW dp
                let dp = self.fetch_u8();
                let lo = self.read_mem(self.dp_addr(dp)) as u16;
                let hi = self.read_mem(self.dp_addr(dp.wrapping_add(1))) as u16;
                let word = ((hi << 8) | lo).wrapping_add(1);
                self.write_mem(self.dp_addr(dp), (word & 0xFF) as u8);
                self.write_mem(self.dp_addr(dp.wrapping_add(1)), (word >> 8) as u8);
                self.psw.z = word == 0;
                self.psw.n = (word & 0x8000) != 0;
                6
            }
            0x1A => { // DECW dp
                let dp = self.fetch_u8();
                let lo = self.read_mem(self.dp_addr(dp)) as u16;
                let hi = self.read_mem(self.dp_addr(dp.wrapping_add(1))) as u16;
                let word = ((hi << 8) | lo).wrapping_sub(1);
                self.write_mem(self.dp_addr(dp), (word & 0xFF) as u8);
                self.write_mem(self.dp_addr(dp.wrapping_add(1)), (word >> 8) as u8);
                self.psw.z = word == 0;
                self.psw.n = (word & 0x8000) != 0;
                6
            }

            0xDE => { // CBNE dp+X, rel
                let dp = self.fetch_u8().wrapping_add(self.x);
                let v = self.read_mem(self.dp_addr(dp));
                let rel = self.fetch_u8() as i8;
                if self.a != v {
                    self.pc = self.pc.wrapping_add(rel as u16);
                }
                7
            }
            0x2E => { // CBNE dp, rel
                let dp = self.fetch_u8();
                let v = self.read_mem(self.dp_addr(dp));
                let rel = self.fetch_u8() as i8;
                if self.a != v {
                    self.pc = self.pc.wrapping_add(rel as u16);
                }
                6
            }

            0x4F => { // PCALL upage -- call within page $FF
                let target_lo = self.fetch_u8();
                let ret_addr = self.pc;
                self.push_stack((ret_addr >> 8) as u8);
                self.push_stack((ret_addr & 0xFF) as u8);
                self.pc = 0xFF00 | (target_lo as u16);
                6
            }

            opcode if (opcode & 0x0F) == 0x01 => { // TCALL n (n = bits 4-7)
                // The full x1 column is TCALL 0-15 with vectors descending
                // from $FFDE. An earlier guard used `& 0x1F == 0x01`, which
                // only matched the even-n half (TCALL 1/3/5/... encode as
                // $11/$31/$51/..., whose low 5 bits are $11) -- the odd
                // TCALLs fell through to the halt arm.
                let n = ((opcode >> 4) & 0x0F) as u16;
                let vector_addr = 0xFFDEu16.wrapping_sub(2 * n);
                let target_lo = self.read_mem(vector_addr) as u16;
                let target_hi = self.read_mem(vector_addr.wrapping_add(1)) as u16;
                let ret_addr = self.pc;
                self.push_stack((ret_addr >> 8) as u8);
                self.push_stack((ret_addr & 0xFF) as u8);
                self.pc = (target_hi << 8) | target_lo;
                8
            }

            // SET1/CLR1 d.bit: opcode = base | (bit << 5), base=0x02 (SET1)
            // or 0x12 (CLR1). Verified against the instruction chart's
            // SET1 d.0..d.7 = 02,22,42,62,82,A2,C2,E2 and CLR1 = 12,32,...,F2.
            opcode if (opcode & 0x1F) == 0x02 => {
                let bit = (opcode >> 5) & 0x07;
                let dp = self.fetch_u8();
                let v = self.read_mem(self.dp_addr(dp)) | (1 << bit);
                self.write_mem(self.dp_addr(dp), v);
                4
            }
            opcode if (opcode & 0x1F) == 0x12 => {
                let bit = (opcode >> 5) & 0x07;
                let dp = self.fetch_u8();
                let v = self.read_mem(self.dp_addr(dp)) & !(1 << bit);
                self.write_mem(self.dp_addr(dp), v);
                4
            }

            // BBS/BBC d.bit,rel: branch if memory bit is set/clear.
            // BBS = 03,23,43,...,E3; BBC = 13,33,...,F3.
            opcode if (opcode & 0x1F) == 0x03 => {
                let bit = (opcode >> 5) & 0x07;
                let dp = self.fetch_u8();
                let v = self.read_mem(self.dp_addr(dp));
                let rel = self.fetch_u8() as i8;
                if (v & (1 << bit)) != 0 {
                    self.pc = self.pc.wrapping_add(rel as u16);
                }
                5
            }
            opcode if (opcode & 0x1F) == 0x13 => {
                let bit = (opcode >> 5) & 0x07;
                let dp = self.fetch_u8();
                let v = self.read_mem(self.dp_addr(dp));
                let rel = self.fetch_u8() as i8;
                if (v & (1 << bit)) == 0 {
                    self.pc = self.pc.wrapping_add(rel as u16);
                }
                5
            }

            0x0E => { // TSET1 !abs -- OR A into memory, test original against A
                let addr = self.fetch_u16();
                let v = self.read_mem(addr);
                self.cmp8(self.a, v);
                self.write_mem(addr, v | self.a);
                6
            }
            0x4E => { // TCLR1 !abs -- AND ~A into memory, test original against A
                let addr = self.fetch_u16();
                let v = self.read_mem(addr);
                self.cmp8(self.a, v);
                self.write_mem(addr, v & !self.a);
                6
            }

            0xEF => 2, // SLEEP (approximated as a no-op rather than halting the CPU clock)
            0xFF => { self.halted = Some(0xFF); 2 } // STOP -- genuinely halts; surface it rather than spin

            0xCF => { // MUL YA -- unsigned Y*A -> 16-bit result, Y=high, A=low
                let result = (self.y as u16) * (self.a as u16);
                self.a = (result & 0xFF) as u8;
                self.y = (result >> 8) as u8;
                self.set_zn(self.y);
                9
            }
            0x9E => { // DIV YA,X -- unsigned (Y:A)/X -> A=quotient, Y=remainder
                let ya = ((self.y as u16) << 8) | (self.a as u16);
                if self.x == 0 {
                    // Real hardware: division by zero leaves A/Y in a
                    // hardware-specific overflowed state; approximated
                    // here as quotient=0xFF, remainder=YA's high byte,
                    // with V set to flag the overflow condition.
                    self.a = 0xFF;
                    self.y = (ya >> 8) as u8;
                    self.psw.v = true;
                } else {
                    self.a = (ya / self.x as u16) as u8;
                    self.y = (ya % self.x as u16) as u8;
                    self.psw.v = false;
                }
                self.set_zn(self.a);
                12
            }

            // ============================================================
            // Final opcode group, completing the full 256-opcode SPC700
            // instruction set (values verified against the same
            // wiki.superfamicom.org/spc700-reference chart as above).
            // ============================================================

            // ALU dp,dp -- same src-then-dst operand order as OR dp,dp
            // (0x09) / MOV dp,dp (0xFA). Result stored back to dst
            // (except CMP, which only sets flags).
            0x29 | 0x49 | 0x69 | 0x89 | 0xA9 => {
                let src_dp = self.fetch_u8();
                let dst_dp = self.fetch_u8();
                let src = self.read_mem(self.dp_addr(src_dp));
                let dst = self.read_mem(self.dp_addr(dst_dp));
                match opcode {
                    0x29 => { let r = dst & src; self.write_mem(self.dp_addr(dst_dp), r); self.set_zn(r); } // AND dp,dp
                    0x49 => { let r = dst ^ src; self.write_mem(self.dp_addr(dst_dp), r); self.set_zn(r); } // EOR dp,dp
                    0x69 => { self.cmp8(dst, src); } // CMP dp,dp
                    0x89 => { let r = self.adc_generic(dst, src); self.write_mem(self.dp_addr(dst_dp), r); } // ADC dp,dp
                    _ => { let r = self.sbc_generic(dst, src); self.write_mem(self.dp_addr(dst_dp), r); } // SBC dp,dp
                }
                6
            }

            // ALU dp,#imm -- same imm-then-dp operand order as MOV dp,#imm
            // (0x8F) / CMP dp,#imm (0x78).
            0x18 | 0x38 | 0x58 | 0x98 | 0xB8 => {
                let imm = self.fetch_u8();
                let dp = self.fetch_u8();
                let dst = self.read_mem(self.dp_addr(dp));
                let r = match opcode {
                    0x18 => { let r = dst | imm; self.set_zn(r); r } // OR dp,#imm
                    0x38 => { let r = dst & imm; self.set_zn(r); r } // AND dp,#imm
                    0x58 => { let r = dst ^ imm; self.set_zn(r); r } // EOR dp,#imm
                    0x98 => self.adc_generic(dst, imm), // ADC dp,#imm
                    _ => self.sbc_generic(dst, imm), // SBC dp,#imm
                };
                self.write_mem(self.dp_addr(dp), r);
                5
            }

            // ALU (X),(Y) -- both operands come from direct-page addresses
            // held in X (destination) and Y (source); the result is stored
            // back through (X) (except CMP).
            0x19 | 0x39 | 0x59 | 0x79 | 0x99 | 0xB9 => {
                let dst = self.read_mem(self.dp_addr(self.x));
                let src = self.read_mem(self.dp_addr(self.y));
                match opcode {
                    0x19 => { let r = dst | src; self.write_mem(self.dp_addr(self.x), r); self.set_zn(r); } // OR (X),(Y)
                    0x39 => { let r = dst & src; self.write_mem(self.dp_addr(self.x), r); self.set_zn(r); } // AND (X),(Y)
                    0x59 => { let r = dst ^ src; self.write_mem(self.dp_addr(self.x), r); self.set_zn(r); } // EOR (X),(Y)
                    0x79 => { self.cmp8(dst, src); } // CMP (X),(Y)
                    0x99 => { let r = self.adc_generic(dst, src); self.write_mem(self.dp_addr(self.x), r); } // ADC (X),(Y)
                    _ => { let r = self.sbc_generic(dst, src); self.write_mem(self.dp_addr(self.x), r); } // SBC (X),(Y)
                }
                5
            }

            // Carry-bit <-> memory-bit instructions, all addressed by the
            // 13-bit-address + 3-bit-bit `m.b` operand (see `fetch_abs_bit`).
            0x0A => { // OR1 C, m.b
                let (addr, bit) = self.fetch_abs_bit();
                self.psw.c |= (self.read_mem(addr) >> bit) & 1 != 0;
                5
            }
            0x2A => { // OR1 C, /m.b
                let (addr, bit) = self.fetch_abs_bit();
                self.psw.c |= (self.read_mem(addr) >> bit) & 1 == 0;
                5
            }
            0x4A => { // AND1 C, m.b
                let (addr, bit) = self.fetch_abs_bit();
                self.psw.c &= (self.read_mem(addr) >> bit) & 1 != 0;
                4
            }
            0x6A => { // AND1 C, /m.b
                let (addr, bit) = self.fetch_abs_bit();
                self.psw.c &= (self.read_mem(addr) >> bit) & 1 == 0;
                4
            }
            0x8A => { // EOR1 C, m.b
                let (addr, bit) = self.fetch_abs_bit();
                self.psw.c ^= (self.read_mem(addr) >> bit) & 1 != 0;
                5
            }
            0xAA => { // MOV1 C, m.b
                let (addr, bit) = self.fetch_abs_bit();
                self.psw.c = (self.read_mem(addr) >> bit) & 1 != 0;
                4
            }
            0xCA => { // MOV1 m.b, C
                let (addr, bit) = self.fetch_abs_bit();
                let v = self.read_mem(addr);
                let v = if self.psw.c { v | (1 << bit) } else { v & !(1 << bit) };
                self.write_mem(addr, v);
                6
            }
            0xEA => { // NOT1 m.b
                let (addr, bit) = self.fetch_abs_bit();
                let v = self.read_mem(addr) ^ (1 << bit);
                self.write_mem(addr, v);
                5
            }

            0xDF => { // DAA A -- decimal adjust after addition
                if self.psw.c || self.a > 0x99 {
                    self.a = self.a.wrapping_add(0x60);
                    self.psw.c = true;
                }
                if self.psw.h || (self.a & 0x0F) > 0x09 {
                    self.a = self.a.wrapping_add(0x06);
                }
                self.set_zn(self.a);
                3
            }
            0xBE => { // DAS A -- decimal adjust after subtraction
                if !self.psw.c || self.a > 0x99 {
                    self.a = self.a.wrapping_sub(0x60);
                    self.psw.c = false;
                }
                if !self.psw.h || (self.a & 0x0F) > 0x09 {
                    self.a = self.a.wrapping_sub(0x06);
                }
                self.set_zn(self.a);
                3
            }

            0xC7 => { // MOV [dp+X],A -- store through the pointer at dp+X
                // (the store counterpart of MOV A,[dp+X], 0xE7).
                let dp = self.fetch_u8().wrapping_add(self.x);
                let ptr_lo = self.read_mem(self.dp_addr(dp)) as u16;
                let ptr_hi = self.read_mem(self.dp_addr(dp.wrapping_add(1))) as u16;
                let addr = ptr_lo | (ptr_hi << 8);
                self.write_mem(addr, self.a);
                7
            }

            0x0F => { // BRK -- push PC then PSW, set B, clear I, and jump
                // through the $FFDE vector (shared with TCALL 0).
                let ret_addr = self.pc;
                self.push_stack((ret_addr >> 8) as u8);
                self.push_stack((ret_addr & 0xFF) as u8);
                self.push_stack(self.psw.to_byte());
                self.psw.b = true;
                self.psw.i = false;
                let target_lo = self.read_mem(0xFFDE) as u16;
                let target_hi = self.read_mem(0xFFDF) as u16;
                self.pc = (target_hi << 8) | target_lo;
                8
            }

            other => {
                // All 256 opcodes are handled above, but the compiler
                // can't prove that through the `opcode if ...` guard arms
                // (TCALL/SET1/CLR1/BBS/BBC), so a fallback is still
                // required. Halt loudly if it's ever reached -- that would
                // mean one of the guard predicates regressed.
                self.halted = Some(other);
                self.pc = self.pc.wrapping_sub(1);
                2
            }
        }
    }

}

// ============================================================================
// DSP (Digital Signal Processor)
// ============================================================================

/// SNES DSP envelope phases. Ordering is significant and matches the
/// real hardware / bsnes `SPC_DSP` convention (`Release` < `Attack` <
/// `Decay` < `Sustain`): the envelope step logic tests `mode >= Decay`
/// to mean "decay or sustain", so these discriminants must stay in this
/// order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum EnvMode {
    Release,
    Attack,
    Decay,
    Sustain,
}

/// Per-voice envelope generator, implementing the real SNES DSP envelope
/// timing rather than a linear-per-sample approximation.
///
/// The value that matters for audio, `level`, is the 11-bit (0..0x7FF)
/// envelope the DSP multiplies each decoded BRR sample by. Its motion is
/// gated by a global sample counter and a per-rate divisor table (see
/// `COUNTER_RATES`/`COUNTER_OFFSETS` and `read_counter`): a given rate
/// only advances the envelope once every N samples, which is what makes
/// notes actually sustain for their intended duration. The previous
/// implementation moved `level` by a large fixed delta *every* sample,
/// which collapsed attack/decay/release into a few milliseconds and made
/// music sound like sparse clicks even once samples were being triggered.
///
/// The step math (attack `+0x20`, exponential decay `env -= env>>8`,
/// sustain-level detection, GAIN modes, the release `-0x8`/sample) is
/// ported from the widely-used blargg/bsnes `SPC_DSP::run_envelope`.
#[derive(Clone)]
pub struct Adsr {
    /// The live 11-bit envelope value (0..0x7FF) used for voice mixing.
    pub level: i32,
    /// Which envelope phase this voice is in.
    pub mode: EnvMode,
    /// Internal pre-clamp envelope value; GAIN mode 7 ("bent line")
    /// changes slope based on whether this has crossed 0x600, so it must
    /// be tracked separately from the clamped `level`.
    pub hidden: i32,
}

/// The global envelope counter's period (30720 = 2048*5*3). From bsnes
/// `SPC_DSP::simple_counter_range`.
const SIMPLE_COUNTER_RANGE: u32 = 2048 * 5 * 3;

/// Global-counter divisor per envelope rate (0-31). Rate 0 never fires
/// (`SIMPLE_COUNTER_RANGE + 1` never divides the counter evenly). Larger
/// rate index = smaller divisor = faster envelope. From bsnes
/// `SPC_DSP::counter_rates`.
const COUNTER_RATES: [u32; 32] = [
    SIMPLE_COUNTER_RANGE + 1, // 0: never fires
    2048, 1536,
    1280, 1024, 768,
    640, 512, 384,
    320, 256, 192,
    160, 128, 96,
    80, 64, 48,
    40, 32, 24,
    20, 16, 12,
    10, 8, 6,
    5, 4, 3,
    2,
    1,
];

/// Per-rate phase offset applied before the modulo in `read_counter`, so
/// different rates fire on different global-counter values. From bsnes
/// `SPC_DSP::counter_offsets`.
const COUNTER_OFFSETS: [u32; 32] = [
    1, 0, 1040,
    536, 0, 1040,
    536, 0, 1040,
    536, 0, 1040,
    536, 0, 1040,
    536, 0, 1040,
    536, 0, 1040,
    536, 0, 1040,
    536, 0, 1040,
    536, 0, 1040,
    0,
    0,
];

/// Returns 0 exactly on the samples where an envelope running at `rate`
/// should advance one step; non-zero otherwise. `counter` is the DSP's
/// global sample counter (see `Dsp::counter`). This per-rate gating is
/// what makes each envelope rate take its real, audibly-correct amount
/// of wall-clock time instead of completing in a few samples.
fn read_counter(counter: u32, rate: usize) -> u32 {
    counter.wrapping_add(COUNTER_OFFSETS[rate]) % COUNTER_RATES[rate]
}

impl Adsr {
    pub fn new() -> Self {
        Adsr { level: 0, mode: EnvMode::Release, hidden: 0 }
    }

    pub fn reset(&mut self) {
        self.level = 0;
        self.mode = EnvMode::Release;
        self.hidden = 0;
    }

    pub fn key_on(&mut self) {
        self.mode = EnvMode::Attack;
        self.level = 0;
        self.hidden = 0;
    }

    pub fn key_off(&mut self) {
        self.mode = EnvMode::Release;
    }

    /// Advance the envelope one sample, given this voice's raw ADSR1
    /// ($x5: bit7 = ADSR enable, bits4-6 = decay rate, bits0-3 = attack
    /// rate), ADSR2 ($x6: bits5-7 = sustain level, bits0-4 = sustain
    /// rate) and GAIN ($x7) register bytes plus the DSP's global sample
    /// counter. Faithful port of bsnes `SPC_DSP::run_envelope`.
    pub fn run(&mut self, adsr1: u8, adsr2: u8, gain: u8, counter: u32) {
        if self.mode == EnvMode::Release {
            // Key-off release is a fixed linear ramp, ungated by the
            // counter -- one of the few things that runs every sample.
            self.level -= 0x8;
            if self.level < 0 {
                self.level = 0;
            }
            return;
        }

        let mut env = self.level;
        let mut env_data = adsr2 as i32;
        let rate: usize;
        // Whether this call is running the ADSR state machine (bit7 of
        // ADSR1 set) as opposed to GAIN (direct envelope control) mode --
        // the same switch the branch immediately below already dispatches
        // on, kept here too so the sustain-level transition check further
        // down can respect it.
        let adsr_mode = adsr1 & 0x80 != 0;

        if adsr_mode {
            // ADSR mode
            if self.mode >= EnvMode::Decay {
                // decay or sustain: exponential decrease
                env -= 1;
                env -= env >> 8;
                rate = if self.mode == EnvMode::Decay {
                    (((adsr1 >> 3) & 0x0E) as usize) + 0x10 // decay rate
                } else {
                    (adsr2 & 0x1F) as usize // sustain rate
                };
            } else {
                // attack
                rate = ((adsr1 & 0x0F) as usize) * 2 + 1;
                env += if rate < 31 { 0x20 } else { 0x400 };
            }
        } else {
            // GAIN mode
            env_data = gain as i32;
            let mode = gain >> 5;
            if mode < 4 {
                // direct gain
                env = (gain as i32) * 0x10;
                rate = 31;
            } else {
                rate = (gain & 0x1F) as usize;
                match mode {
                    4 => env -= 0x20,                    // linear decrease
                    5 => { env -= 1; env -= env >> 8; }  // exponential decrease
                    _ => {
                        // 6, 7: increase
                        env += 0x20;
                        if mode > 6 && (self.hidden as u32) >= 0x600 {
                            env += 0x8 - 0x20; // 7: two-slope ("bent line")
                        }
                    }
                }
            }
        }

        // Sustain-level detection: once the envelope decays down to the
        // programmed sustain level (its top 3 bits of ADSR2), switch
        // decay->sustain. This is an ADSR-only concept -- `env_data` only
        // ever holds real sustain-level bits when `adsr_mode` is true (in
        // GAIN mode it holds the GAIN byte's mode-select/rate bits
        // instead, which do not represent a sustain level at all). The
        // voice's `mode` state can still read `Decay` while GAIN mode is
        // active (the attack->decay clamp transition below is unconditional
        // on ADSR/GAIN), so without this guard, `env_data >> 5` (really
        // GAIN's mode-select nibble) could spuriously happen to equal
        // `env >> 8` and incorrectly flip the voice to `Sustain` even
        // though no ADSR sustain level was ever programmed or reached.
        if adsr_mode && (env >> 8) == (env_data >> 5) && self.mode == EnvMode::Decay {
            self.mode = EnvMode::Sustain;
        }

        self.hidden = env;

        if (env as u32) > 0x7FF {
            env = if env < 0 { 0 } else { 0x7FF };
            if self.mode == EnvMode::Attack {
                self.mode = EnvMode::Decay;
            }
        }

        // Commit the new envelope value only on the samples the rate's
        // counter selects.
        if read_counter(counter, rate) == 0 {
            self.level = env;
        }
    }

    /// The 11-bit envelope value (0..0x7FF) a voice multiplies its
    /// decoded BRR sample by.
    pub fn get_output(&self) -> i32 {
        self.level
    }
}

impl Default for Adsr {
    fn default() -> Self {
        Self::new()
    }
}

/// BRR (Block Relocation Reform) decoder
#[derive(Clone)]
pub struct BrrDecoder {
    pub history: [i16; 2],
    pub filter: [i16; 2],
}

impl BrrDecoder {
    pub fn new() -> Self {
        BrrDecoder {
            history: [0; 2],
            filter: [0; 2],
        }
    }
    
    pub fn reset(&mut self) {
        self.history = [0; 2];
        self.filter = [0; 2];
    }
    
    /// Decode BRR block, producing 16 samples.
    ///
    /// Header bits (real hardware layout, byte = `ssssffle`): bits 4-7 =
    /// shift (0-12; 13-15 are invalid and hardware clamps to a fixed
    /// value instead of shifting), bits 2-3 = filter select, bit 1 =
    /// loop flag, bit 0 = end flag. This previously read the shift/filter
    /// bits from entirely the wrong positions (lower nibble instead of
    /// upper, plus a `.max(1)` that made a genuinely valid shift-0 block
    /// impossible) -- silently decoding every real sample's pitch/filter
    /// wrong, though it went unnoticed because nothing ever reached real
    /// note-triggering DSP register writes until the $F2/$F3 CPU<->DSP
    /// port routing bug (see `Spc700::write_mem`) was fixed.
    ///
    /// The shift/filter arithmetic itself (shift-then-halve, doubled
    /// output-history convention, exact multiply/shift sequences per
    /// filter) is ported from the widely-used blargg/bsnes `SPC_DSP`
    /// reference decoder (`decode_brr` in bsnes-emu/bsnes's
    /// `bsnes/sfc/dsp/SPC_DSP.cpp`) rather than the decimal-multiplier
    /// approximation this had before, which is also what caused the
    /// `i16` multiply overflow this fix replaces with `i32` intermediate
    /// arithmetic.
    pub fn decode(&mut self, header: u8, data: &[u8; 8], output: &mut [i16; 16]) {
        let shift = (header >> 4) as i32;
        let filter = (header & 0x0C) as i32;
        let _loop_flag = (header & 0x02) != 0;
        let _end_flag = (header & 0x01) != 0;

        for i in 0..16 {
            // Each byte contains 2 4-bit samples (nibble)
            let nibble = if i < 8 {
                data[i] & 0x0F
            } else {
                (data[i - 8] >> 4) & 0x0F
            };

            // Sign-extend 4-bit to i32.
            let mut s: i32 = if nibble >= 8 { (nibble as i32) - 16 } else { nibble as i32 };

            s = (s << shift) >> 1;
            if shift >= 0x0D {
                // Invalid shift range: real hardware clamps rather than
                // shifting by an out-of-range amount.
                s = if s < 0 { -0x800 } else { 0 };
            }

            let p1 = self.history[0] as i32;
            let p2 = (self.history[1] as i32) >> 1;

            if filter >= 8 {
                s += p1;
                s -= p2;
                if filter == 8 {
                    // s += p1 * 0.953125 - p2 * 0.46875
                    s += p2 >> 4;
                    s += (p1 * -3) >> 6;
                } else {
                    // s += p1 * 0.8984375 - p2 * 0.40625
                    s += (p1 * -13) >> 7;
                    s += (p2 * 3) >> 4;
                }
            } else if filter != 0 {
                // s += p1 * 0.46875
                s += p1 >> 1;
                s += (-p1) >> 5;
            }

            s = s.clamp(-32768, 32767);
            // Real hardware truncates this doubling to 16 bits (not
            // saturating) -- matches the reference decoder's `(int16_t)`
            // cast, which is why this is a wrapping `as i16`, not `.clamp`.
            let sample16 = (s * 2) as i16;

            output[i] = sample16;
            self.history[1] = self.history[0];
            self.history[0] = sample16;
        }
    }
}

impl Default for BrrDecoder {
    fn default() -> Self {
        Self::new()
    }
}

/// A single voice channel.
///
/// This holds only genuine per-voice *state*; all configuration (volume,
/// pitch, source number, ADSR/GAIN) is read from the DSP register file
/// (`Dsp::regs`) at sample time and passed into `sample()`. An earlier
/// version cached those into per-voice fields inside `write_reg`, but the
/// register->field mapping there was offset by one slot (source_number
/// read the VOL(L) register, volumes read the ADSR2/GAIN registers,
/// pitch/ADSR read the wrong bytes, GAIN was ignored) -- so every voice
/// played the wrong sample at the wrong pitch and volume. Reading
/// straight from `regs[base + offset]` removes that whole class of bug.
#[derive(Clone)]
pub struct Voice {
    pub adsr: Adsr,
    pub brr: BrrDecoder,
    /// Address (in APU RAM) of the 9-byte BRR block currently being played.
    pub brr_addr: u16,
    /// Loop-point block address, read from the sample directory at key-on.
    pub loop_addr: u16,
    /// The 16 decoded samples of the block at `brr_addr`.
    pub brr_buffer: [i16; 16],
    /// Which of those 16 samples is currently being output (0..15).
    pub brr_position: u8,
    /// Address whose block is currently decoded into `brr_buffer`, so a
    /// block is decoded exactly once (keeping BRR filter history
    /// continuous rather than corrupting it by re-decoding).
    pub decoded_addr: u16,
    pub decoded_valid: bool,
    /// 12-bit fractional pitch accumulator: each output sample adds the
    /// voice's PITCH; every time it overflows 0x1000 the BRR read position
    /// advances one sample, so PITCH=0x1000 plays at the native ~32kHz.
    pub pitch_counter: u16,
    /// The last four decoded source samples (oldest first), fed to the
    /// 4-point resampling interpolator. Real hardware runs a 4-tap
    /// gaussian filter here; we use Catmull-Rom cubic interpolation as a
    /// close stand-in -- the point is that ANY 4-point kernel removes the
    /// harsh zipper/aliasing noise the previous nearest-neighbor
    /// resampling produced on every non-native-pitch note (i.e. nearly
    /// all of them), which read as "sound plays but is not clear".
    pub hist: [i32; 4],
    pub active: bool,
    /// Set by `key_on`; the sample-directory lookup that resolves the
    /// start/loop addresses is deferred to the next `sample()` call
    /// because `write_reg` (where KON is handled) has no APU-RAM access.
    pub key_on_pending: bool,
    pub output_left: i32,
    pub output_right: i32,
    /// Mirrors real hardware's per-voice ENDX status bit ($7C): set when
    /// this voice's BRR playback reaches a block whose header has the
    /// "end" bit set (natural sample end, whether or not it then loops),
    /// or when the voice is force-terminated by KOFF. Cleared only by
    /// `Dsp`'s handling of a $7C write (see `Dsp::write_reg`) or `key_on`
    /// (real hardware also clears a voice's own ENDX bit on KON), never
    /// by this struct itself -- `Dsp::sample` reads and aggregates this
    /// into `regs[0x7C]` after every `Voice::sample` call.
    pub endx: bool,
}

impl Voice {
    pub fn new() -> Self {
        Voice {
            adsr: Adsr::new(),
            brr: BrrDecoder::new(),
            brr_addr: 0,
            loop_addr: 0,
            brr_buffer: [0; 16],
            brr_position: 0,
            decoded_addr: 0,
            decoded_valid: false,
            pitch_counter: 0,
            hist: [0; 4],
            active: false,
            key_on_pending: false,
            output_left: 0,
            output_right: 0,
            endx: false,
        }
    }

    pub fn reset(&mut self) {
        *self = Voice::new();
    }

    pub fn key_on(&mut self) {
        self.adsr.key_on();
        self.brr.reset();
        self.brr_position = 0;
        self.pitch_counter = 0;
        // Real hardware clears this voice's own ENDX bit on key-on.
        self.endx = false;
        self.hist = [0; 4];
        self.decoded_valid = false;
        self.active = true;
        self.key_on_pending = true;
    }

    pub fn key_off(&mut self) {
        self.adsr.key_off();
        // Real hardware sets this voice's ENDX bit when it is
        // force-terminated by KOFF, not only on a natural BRR end-block
        // (see the `endx` field's doc comment and the end-flag site in
        // `sample()` below for the other real trigger).
        self.endx = true;
    }

    /// Generate one sample for this voice, reading its live configuration
    /// straight from the DSP register bytes (`vol_l`/`vol_r` = $x0/$x1,
    /// `pitch` = $x2/$x3 as a 14-bit value, `srcn` = $x4, `adsr1`/`adsr2`
    /// = $x5/$x6, `gain` = $x7) plus the sample-directory page (`dir` =
    /// $5D) and the DSP's global envelope `counter`.
    pub fn sample(
        &mut self,
        ram: &[u8; 65536],
        vol_l: i8,
        vol_r: i8,
        pitch: u16,
        srcn: u8,
        adsr1: u8,
        adsr2: u8,
        gain: u8,
        dir: u8,
        counter: u32,
    ) -> (i32, i32) {
        // Resolve start/loop addresses from the sample directory on the
        // first sample after key-on. The directory lives at page `dir<<8`;
        // each source number selects a 4-byte entry (start lo/hi, loop
        // lo/hi). The old code skipped the directory entirely and used
        // `srcn << 8` as the start address, which only coincidentally
        // pointed at real sample data.
        if self.key_on_pending {
            let dir_base = (dir as u16) << 8;
            let entry = dir_base.wrapping_add((srcn as u16).wrapping_mul(4));
            let rd = |off: u16| ram[entry.wrapping_add(off) as usize] as u16;
            self.brr_addr = rd(0) | (rd(1) << 8);
            self.loop_addr = rd(2) | (rd(3) << 8);
            self.brr_position = 0;
            self.pitch_counter = 0;
            self.decoded_valid = false;
            self.key_on_pending = false;
        }

        if !self.active {
            self.output_left = 0;
            self.output_right = 0;
            return (0, 0);
        }

        // Advance the BRR read position by pitch (PITCH=0x1000 => native
        // ~32kHz playback), pushing every source sample crossed into the
        // 4-entry interpolation history. Crossing a block boundary
        // advances to the next block, following the just-finished block's
        // end/loop flags.
        self.pitch_counter = self.pitch_counter.wrapping_add(pitch & 0x3FFF);
        let steps = self.pitch_counter >> 12;
        self.pitch_counter &= 0x0FFF;
        for _ in 0..steps {
            self.brr_position += 1;
            if self.brr_position >= 16 {
                self.brr_position -= 16;
                let cur_header = ram[self.brr_addr as usize];
                let end = cur_header & 0x01 != 0;
                let looping = cur_header & 0x02 != 0;
                if end {
                    // Real hardware sets this voice's ENDX bit whenever a
                    // BRR block's "end" header bit is hit -- whether or
                    // not the sample then loops (looping just determines
                    // whether playback continues from `loop_addr` or the
                    // voice goes silent; ENDX fires either way).
                    self.endx = true;
                    if looping {
                        self.brr_addr = self.loop_addr;
                    } else {
                        self.active = false;
                        self.adsr.level = 0;
                        self.output_left = 0;
                        self.output_right = 0;
                        return (0, 0);
                    }
                } else {
                    self.brr_addr = self.brr_addr.wrapping_add(9);
                }
                self.decoded_valid = false;
            }
            // Decode the (possibly new) current block before sampling it,
            // keeping BRR filter history continuous across blocks.
            if !self.decoded_valid || self.decoded_addr != self.brr_addr {
                let header = ram[self.brr_addr as usize];
                let mut data = [0u8; 8];
                for i in 0..8u16 {
                    data[i as usize] = ram[self.brr_addr.wrapping_add(1 + i) as usize];
                }
                self.brr.decode(header, &data, &mut self.brr_buffer);
                self.decoded_addr = self.brr_addr;
                self.decoded_valid = true;
            }
            self.hist = [
                self.hist[1],
                self.hist[2],
                self.hist[3],
                self.brr_buffer[self.brr_position as usize] as i32,
            ];
        }

        // Decode for the steps==0 path (first call after key-on) so the
        // history has real data to start from.
        if !self.decoded_valid || self.decoded_addr != self.brr_addr {
            let header = ram[self.brr_addr as usize];
            let mut data = [0u8; 8];
            for i in 0..8u16 {
                data[i as usize] = ram[self.brr_addr.wrapping_add(1 + i) as usize];
            }
            self.brr.decode(header, &data, &mut self.brr_buffer);
            self.decoded_addr = self.brr_addr;
            self.decoded_valid = true;
            self.hist[3] = self.brr_buffer[self.brr_position as usize] as i32;
        }

        // Advance the envelope, then stop the voice once a key-off release
        // has fully decayed.
        self.adsr.run(adsr1, adsr2, gain, counter);
        let env = self.adsr.get_output(); // 0..0x7FF
        if env == 0 && self.adsr.mode == EnvMode::Release {
            self.active = false;
            self.output_left = 0;
            self.output_right = 0;
            return (0, 0);
        }

        // 4-point Catmull-Rom interpolation between hist[1] and hist[2]
        // at the fractional pitch position (see `hist`'s doc comment).
        let brr_sample = {
            let t = (self.pitch_counter as f32) * (1.0 / 4096.0);
            let p0 = self.hist[0] as f32;
            let p1 = self.hist[1] as f32;
            let p2 = self.hist[2] as f32;
            let p3 = self.hist[3] as f32;
            let a = -0.5 * p0 + 1.5 * p1 - 1.5 * p2 + 0.5 * p3;
            let b = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
            let c = -0.5 * p0 + 0.5 * p2;
            let out = ((a * t + b) * t + c) * t + p1;
            (out as i32).clamp(-32768, 32767)
        };
        // Envelope is 11-bit, so >> 11 to scale.
        let enveloped = (brr_sample * env) >> 11;

        // Per-voice volume is a signed 8-bit value; >> 7 normalizes it.
        self.output_left = (enveloped * (vol_l as i32)) >> 7;
        self.output_right = (enveloped * (vol_r as i32)) >> 7;
        (self.output_left, self.output_right)
    }
}

impl Default for Voice {
    fn default() -> Self {
        Self::new()
    }
}

/// DSP (Digital Signal Processor)
pub struct Dsp {
    /// DSP registers ($00-$7F)
    pub regs: [u8; 128],
    
    /// 8 voice channels
    pub voices: Vec<Voice>,

    // Master volume, echo feedback/volume, echo buffer address, and FIR
    // coefficients used to be cached here into separate fields at write
    // time (see `write_reg`). That caching is exactly what caused the
    // FIR-coefficient address-decode bug (see the FIR comment in
    // `write_reg`): a wrong mask silently routed voice GAIN writes into
    // this array instead of the real coefficients. `dir` ($5D) and `eon`
    // ($4D) were already read straight from `regs` at sample time instead
    // of being cached, and never had that class of bug -- so every
    // globally-cached register above is now read the same way, directly
    // from `regs[addr]` in `sample()`, removing the whole class of bug
    // rather than just this one instance of it.

    /// Echo delay line (stereo), sized `EDL * 512` samples per the $7D
    /// register, matching real hardware's EDL x 16ms at 32kHz. The
    /// previous implementation read back the sample written ONE position
    /// earlier (a fixed 1-sample "delay"), fed a FIR whose taps were 8
    /// samples apart instead of adjacent, ignored EON (which voices feed
    /// the echo) and the FLG echo-disable bit -- on an echo-heavy game
    /// like SMW that turned the echo path into a screen-wide comb filter
    /// smearing the whole mix ("sound plays but is not clear").
    echo_ring: Vec<(i32, i32)>,
    echo_pos: usize,
    /// The last 8 samples read out of the delay line (the FIR filter's
    /// input window -- 8 CONSECUTIVE samples, newest at `fir_pos`).
    fir_hist: [(i32, i32); 8],
    fir_pos: usize,
    
    /// Output mix
    output_left: i16,
    output_right: i16,

    /// Global envelope timing counter, decremented once per generated
    /// sample and wrapped at `SIMPLE_COUNTER_RANGE`. `read_counter` uses
    /// it to decide, per rate, which samples an envelope advances on --
    /// see `Adsr::run`.
    counter: i32,
}

impl Dsp {
    pub fn new() -> Self {
        Dsp {
            regs: [0; 128],
            voices: vec![Voice::new(); 8],
            echo_ring: vec![(0, 0); 512],
            echo_pos: 0,
            fir_hist: [(0, 0); 8],
            fir_pos: 0,
            output_left: 0,
            output_right: 0,
            counter: 0,
        }
    }

    pub fn reset(&mut self) {
        self.regs = [0; 128];
        for voice in &mut self.voices {
            voice.reset();
        }
        self.echo_ring = vec![(0, 0); 512];
        self.echo_pos = 0;
        self.fir_hist = [(0, 0); 8];
        self.fir_pos = 0;
        self.output_left = 0;
        self.output_right = 0;
        self.counter = 0;
    }

    /// Serializes the COMPLETE DSP: the 128-byte register file plus all
    /// transient synthesis state -- per-voice envelope phase/level, BRR
    /// decode cursors/history, pitch accumulators, resampler history, and
    /// the echo delay line with its FIR window. A restored state resumes
    /// mid-note bit-for-bit, rather than re-keying voices silent.
    pub(crate) fn save_state(&self, out: &mut Vec<u8>) {
        use crate::state::{put_bool, put_bytes, put_i32, put_u16, put_u32, put_u8};
        put_bytes(out, &self.regs);
        for v in &self.voices {
            put_i32(out, v.adsr.level);
            put_u8(out, match v.adsr.mode {
                EnvMode::Release => 0,
                EnvMode::Attack => 1,
                EnvMode::Decay => 2,
                EnvMode::Sustain => 3,
            });
            put_i32(out, v.adsr.hidden);
            for &h in &v.brr.history {
                put_u16(out, h as u16);
            }
            for &f in &v.brr.filter {
                put_u16(out, f as u16);
            }
            put_u16(out, v.brr_addr);
            put_u16(out, v.loop_addr);
            for &s in &v.brr_buffer {
                put_u16(out, s as u16);
            }
            put_u8(out, v.brr_position);
            put_u16(out, v.decoded_addr);
            put_bool(out, v.decoded_valid);
            put_u16(out, v.pitch_counter);
            for &h in &v.hist {
                put_i32(out, h);
            }
            put_bool(out, v.active);
            put_bool(out, v.key_on_pending);
            put_i32(out, v.output_left);
            put_i32(out, v.output_right);
            put_bool(out, v.endx);
        }
        put_u32(out, self.echo_ring.len() as u32);
        for &(l, r) in &self.echo_ring {
            put_i32(out, l);
            put_i32(out, r);
        }
        put_u32(out, self.echo_pos as u32);
        for &(l, r) in &self.fir_hist {
            put_i32(out, l);
            put_i32(out, r);
        }
        put_u32(out, self.fir_pos as u32);
        put_u16(out, self.output_left as u16);
        put_u16(out, self.output_right as u16);
        put_i32(out, self.counter);
    }

    /// Restores state produced by `save_state`.
    pub(crate) fn load_state(&mut self, r: &mut crate::state::StateReader) -> Result<(), crate::error::EmulationError> {
        let regs = r.bytes(128)?;
        self.regs.copy_from_slice(regs);
        for v in self.voices.iter_mut() {
            v.adsr.level = r.i32()?;
            v.adsr.mode = match r.u8()? {
                1 => EnvMode::Attack,
                2 => EnvMode::Decay,
                3 => EnvMode::Sustain,
                _ => EnvMode::Release,
            };
            v.adsr.hidden = r.i32()?;
            for h in v.brr.history.iter_mut() {
                *h = r.u16()? as i16;
            }
            for f in v.brr.filter.iter_mut() {
                *f = r.u16()? as i16;
            }
            v.brr_addr = r.u16()?;
            v.loop_addr = r.u16()?;
            for s in v.brr_buffer.iter_mut() {
                *s = r.u16()? as i16;
            }
            v.brr_position = r.u8()?;
            v.decoded_addr = r.u16()?;
            v.decoded_valid = r.bool()?;
            v.pitch_counter = r.u16()?;
            for h in v.hist.iter_mut() {
                *h = r.i32()?;
            }
            v.active = r.bool()?;
            v.key_on_pending = r.bool()?;
            v.output_left = r.i32()?;
            v.output_right = r.i32()?;
            v.endx = r.bool()?;
        }
        let ring_len = r.u32()? as usize;
        if ring_len > 512 * 16 {
            return Err(crate::error::EmulationError::InvalidSaveState(
                "implausible echo ring length",
            ));
        }
        self.echo_ring = (0..ring_len)
            .map(|_| -> Result<(i32, i32), crate::error::EmulationError> {
                Ok((r.i32()?, r.i32()?))
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.echo_pos = (r.u32()? as usize).min(self.echo_ring.len().saturating_sub(1));
        for h in self.fir_hist.iter_mut() {
            *h = (r.i32()?, r.i32()?);
        }
        self.fir_pos = (r.u32()? as usize) % 8;
        self.output_left = r.u16()? as i16;
        self.output_right = r.u16()? as i16;
        self.counter = r.i32()?;
        Ok(())
    }

    /// Read DSP register
    pub fn read_reg(&self, addr: u8) -> u8 {
        if (addr as usize) < self.regs.len() {
            self.regs[addr as usize]
        } else {
            0
        }
    }
    
    /// Write DSP register
    pub fn write_reg(&mut self, addr: u8, value: u8) {
        if (addr as usize) >= self.regs.len() {
            return;
        }
        
        self.regs[addr as usize] = value;

        // Every register's live value now lives only in `self.regs` and is
        // read directly from there at sample time (`sample()` below), the
        // same way `dir` ($5D) and `eon` ($4D) always worked. This used to
        // also cache master/echo volume, echo feedback, and FIR
        // coefficients ($0C/$1C/$2C/$3C/$0D/$0F.../$7F) into separate
        // `Dsp` fields here -- and that caching is exactly what caused a
        // real bug: the FIR-coefficient address decode used a wrong mask
        // (spaced-8 instead of spaced-16), which silently routed voice
        // GAIN register writes (offset 7 of each $n0-$n7 voice block, e.g.
        // $17/$27/.../$47) into the FIR array instead of leaving them in
        // `regs` where GAIN belongs, while never reaching the real FIR
        // taps at $5F/$6F/$7F at all. Reading straight from `regs[addr]`
        // removes that whole class of bug rather than just this one
        // instance of it. KON/KOFF remain real event triggers (key-on/
        // key-off are edge-triggered actions, not register state
        // `sample()` can just read back later), and $7C (ENDX) is a
        // special case in the opposite direction: real hardware clears
        // ALL of its bits on any CPU write, regardless of the written
        // value, rather than storing the value written.
        match addr {
            0x4C => {
                // KON - Key On
                for i in 0..8 {
                    if (value & (1 << i)) != 0 {
                        if let Some(voice) = self.voices.get_mut(i) {
                            voice.key_on();
                            // `voice.key_on()` clears the voice's own
                            // internal `endx` latch, but the aggregated
                            // $7C register bit (set by a previous
                            // `sample()` call OR-ing it in) would
                            // otherwise stay set until the next explicit
                            // $7C write -- real hardware clears a voice's
                            // ENDX bit immediately on its own KON, not
                            // only via a $7C write, so mirror that here.
                            self.regs[0x7C] &= !(1 << i);
                        }
                    }
                }
            }
            0x5C => {
                // KOFF - Key Off
                for i in 0..8 {
                    if (value & (1 << i)) != 0 {
                        if let Some(voice) = self.voices.get_mut(i) {
                            voice.key_off();
                            // Real hardware sets a force-terminated
                            // voice's ENDX bit immediately, not only once
                            // the next `sample()` call happens to
                            // aggregate it -- mirrors KON's immediate
                            // clear above.
                            self.regs[0x7C] |= 1 << i;
                        }
                    }
                }
            }
            0x7C => {
                // ENDX -- real hardware: writing ANY value clears every
                // bit, both in the per-voice latches this aggregates from
                // and in the visible register byte itself (so a write
                // immediately followed by a read, with no intervening
                // `sample()` call, sees zero rather than a stale
                // aggregated value from before the clear).
                for voice in &mut self.voices {
                    voice.endx = false;
                }
                self.regs[0x7C] = 0;
            }
            _ => {}
        }
    }
    
    /// Generate one stereo sample
    pub fn sample(&mut self, ram: &[u8; 65536]) -> (i16, i16) {
        // Advance the global envelope counter once per generated sample
        // (decrement-and-wrap, matching bsnes `run_counters`).
        self.counter -= 1;
        if self.counter < 0 {
            self.counter = SIMPLE_COUNTER_RANGE as i32 - 1;
        }
        let counter = self.counter as u32;
        let dir = self.regs[0x5D];

        // Mix all voices, reading each one's live configuration from its
        // register block ($n0-$n7). Voices whose EON ($4D) bit is set also
        // feed the echo input -- ONLY those (real hardware; previously
        // every voice went into the echo unconditionally).
        let eon = self.regs[0x4D];
        let mut mix_left: i32 = 0;
        let mut mix_right: i32 = 0;
        let mut echo_in_left: i32 = 0;
        let mut echo_in_right: i32 = 0;

        for i in 0..self.voices.len() {
            let base = i * 0x10;
            let vol_l = self.regs[base] as i8;
            let vol_r = self.regs[base + 1] as i8;
            let pitch = ((self.regs[base + 2] as u16) | ((self.regs[base + 3] as u16) << 8)) & 0x3FFF;
            let srcn = self.regs[base + 4];
            let adsr1 = self.regs[base + 5];
            let adsr2 = self.regs[base + 6];
            let gain = self.regs[base + 7];
            let (left, right) = self.voices[i].sample(
                ram, vol_l, vol_r, pitch, srcn, adsr1, adsr2, gain, dir, counter,
            );
            mix_left += left;
            mix_right += right;
            if eon & (1 << i) != 0 {
                echo_in_left += left;
                echo_in_right += right;
            }
            // Aggregate this voice's live ENDX latch (see the `endx`
            // field's doc comment on `Voice`) into the visible $7C
            // register bit, so `read_reg(0x7C)` -- reached from real
            // SPC700 code via the $F2/$F3 port pair -- reflects real
            // per-voice end-of-sample status. Only ORs bits in; a CPU
            // write to $7C (handled in `write_reg`) is what clears them.
            if self.voices[i].endx {
                self.regs[0x7C] |= 1 << i;
            }
        }

        // Apply master volume, read straight from regs (see the comment
        // in `write_reg` about why these are no longer cached fields).
        let master_volume_left = (self.regs[0x0C] as i8) as i32;
        let master_volume_right = (self.regs[0x1C] as i8) as i32;
        mix_left = (mix_left * master_volume_left) >> 7;
        mix_right = (mix_right * master_volume_right) >> 7;

        // Echo: a real EDL-sized delay line ($7D, EDL x 512 samples at
        // 32kHz = EDL x 16ms), an 8-tap FIR over the last 8 CONSECUTIVE
        // delayed samples (coefficient 7 = newest, matching hardware), and
        // EFB feedback into the buffer. Buffer writes are gated on the FLG
        // ($6C) echo-disable bit like real hardware.
        let edl = (self.regs[0x7D] & 0x0F) as usize;
        let delay = if edl == 0 { 1 } else { edl * 512 };
        if self.echo_ring.len() != delay {
            self.echo_ring = vec![(0, 0); delay];
            self.echo_pos = 0;
        }

        let (delayed_l, delayed_r) = self.echo_ring[self.echo_pos];
        self.fir_pos = (self.fir_pos + 1) % 8;
        self.fir_hist[self.fir_pos] = (delayed_l, delayed_r);

        let mut fir_l: i32 = 0;
        let mut fir_r: i32 = 0;
        for i in 0..8 {
            // i=0 -> oldest sample with coefficient 0 ... i=7 -> newest
            // with coefficient 7 (hardware's alignment), each tap >> 6.
            // Coefficients live at $0F,$1F,$2F,...,$7F (spaced 16 apart --
            // see `write_reg`'s comment), read straight from regs rather
            // than a separately-cached array.
            let (hl, hr) = self.fir_hist[(self.fir_pos + 1 + i) % 8];
            let coeff = (self.regs[((i as u8) << 4 | 0x0F) as usize] as i8) as i32;
            fir_l += (hl * coeff) >> 6;
            fir_r += (hr * coeff) >> 6;
        }

        // Write input + feedback back into the delay line (clamped to the
        // 16-bit range the real echo RAM holds -- unbounded values would
        // compound through the feedback multiply and overflow i32).
        let echo_feedback = (self.regs[0x0D] as i8) as i32;
        let echo_write_disabled = self.regs[0x6C] & 0x20 != 0;
        if !echo_write_disabled {
            let wl = (echo_in_left + ((fir_l * echo_feedback) >> 7)).clamp(-32768, 32767);
            let wr = (echo_in_right + ((fir_r * echo_feedback) >> 7)).clamp(-32768, 32767);
            self.echo_ring[self.echo_pos] = (wl, wr);
        }
        self.echo_pos = (self.echo_pos + 1) % self.echo_ring.len();

        // Add echo to output
        let echo_volume_left = (self.regs[0x2C] as i8) as i32;
        let echo_volume_right = (self.regs[0x3C] as i8) as i32;
        mix_left += (fir_l * echo_volume_left) >> 7;
        mix_right += (fir_r * echo_volume_right) >> 7;

        // Clamp and output
        let out_left = mix_left.clamp(-32768, 32767) as i16;
        let out_right = mix_right.clamp(-32768, 32767) as i16;

        (out_left, out_right)
    }
}

impl Default for Dsp {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Main APU Structure
// ============================================================================

/// APU struct representing the Audio Processing Unit of the SNES.
/// 
/// Contains all internal state for the SPC700 CPU, DSP, and communication ports.
/// The real, well-known 64-byte SPC700 IPL boot ROM, mapped at $FFC0-$FFFF.
/// Verified byte-for-byte against sneslab.net/wiki/SPC700/IPL_ROM and
/// snes.nesdev.org/wiki/Booting_the_SPC700 (fetched 2026-06-30) -- this is
/// the actual program every real SNES runs to receive an uploaded sound
/// driver from the main CPU, not a placeholder.
const IPL_ROM: [u8; 64] = [
    0xCD, 0xEF, 0xBD, 0xE8, 0x00, 0xC6, 0x1D, 0xD0, 0xFC, 0x8F, 0xAA, 0xF4, 0x8F, 0xBB, 0xF5, 0x78,
    0xCC, 0xF4, 0xD0, 0xFB, 0x2F, 0x19, 0xEB, 0xF4, 0xD0, 0xFC, 0x7E, 0xF4, 0xD0, 0x0B, 0xE4, 0xF5,
    0xCB, 0xF4, 0xD7, 0x00, 0xFC, 0xD0, 0xF3, 0xAB, 0x01, 0x10, 0xEF, 0x7E, 0xF4, 0x10, 0xEB, 0xBA,
    0xF6, 0xDA, 0x00, 0xBA, 0xF4, 0xC4, 0xF4, 0xDD, 0x5D, 0xD0, 0xDB, 0x1F, 0x00, 0x00, 0xC0, 0xFF,
];

pub struct Apu {
    /// 64KB APU RAM, shared with `spc700` so real SPC700 execution and
    /// this struct's `read_ram`/`write_ram` see the same memory. An
    /// earlier version of this code had `Apu` hold a second, completely
    /// separate `[u8; 65536]` array from the one `Spc700` actually
    /// executed against -- meaning anything written via `write_ram` (or
    /// the old upload-handshake stub) was invisible to the CPU it was
    /// supposedly emulating.
    ram: Arc<Mutex<[u8; 65536]>>,

    /// SPC700 CPU, now actually executing the real IPL ROM (see
    /// `Spc700::execute_opcode`) rather than a non-functional placeholder.
    spc700: Spc700,

    /// DSP (Digital Signal Processor), shared with `spc700` (which reaches
    /// it indirectly through the $F2/$F3 register-select/data ports) the
    /// same way `ram`/`ports` are.
    dsp: Arc<Mutex<Dsp>>,

    /// The CPU<->APU communication latches, shared with `spc700` (which
    /// reads/writes them via RAM addresses $F4-$F7) so the main CPU and
    /// real SPC700 execution observe the same live state.
    ports: Arc<Mutex<ApuPorts>>,

    /// Frame counter for timing
    frame_counter: u8,

    /// Control registers
    control: u8,

    /// Sample buffer for audio output (stereo)
    /// Generated stereo samples awaiting the frontend. A `VecDeque` so the
    /// per-sample drain is O(1) -- the old `Vec::remove(0)` shifted the
    /// entire remaining buffer (up to 320k entries) on every single
    /// sample, easily starving the audio callback into stutter.
    pub sample_buffer: VecDeque<(i16, i16)>,

    /// Frame timing
    frame_cycles: u32,
    cycles_per_frame: u32,

    /// Sample rate divider (for ~32kHz output)
    sample_divider: u32,
    sample_counter: u32,

    /// Fractional main-CPU cycles not yet converted into an SPC700 step,
    /// carried over between `tick()` calls. Without this, converting via
    /// per-call integer division (`cycles / 3`) discards up to 2 of every
    /// 3 cycles on nearly every call, since most 65816 instructions take
    /// only 2-4 cycles -- starving the SPC700 relative to real hardware
    /// and making the main CPU's "wait for SPC ready" polling loops (e.g.
    /// the SPC upload handshake) spin for far longer than a real frame's
    /// worth of time, long enough for a second NMI to fire while still
    /// inside the previous NMI handler and corrupt the stack.
    spc_cycle_debt: u32,
}

impl Apu {
    /// Create a new APU with initialized RAM.
    pub fn new() -> Self {
        let ram = Arc::new(Mutex::new([0u8; 65536]));
        let ports = Arc::new(Mutex::new(ApuPorts::default()));
        let dsp = Arc::new(Mutex::new(Dsp::new()));
        let spc700 = Spc700::new(Arc::clone(&ram), Arc::clone(&ports), Arc::clone(&dsp));

        let mut apu = Apu {
            ram,
            spc700,
            dsp,
            ports,
            frame_counter: 0,
            control: 0,
            sample_buffer: VecDeque::new(),
            frame_cycles: 0,
            // SNES APU frame rate: ~60 Hz (actually 60.1 Hz for NTSC)
            // APU runs at ~24.576 MHz
            // 24576000 / 60.1 ≈ 409200 cycles per frame
            cycles_per_frame: 409200,
            // Sample at ~32kHz relative to the main CPU's assumed ~2.68MHz
            // SlowROM rate (the same clock `SystemBus::tick_ppu`'s "2
            // dots/cycle" ratio assumes): 2,680,000 / 32,000 ~= 84 main
            // cycles per audio sample. This used to be `3`, confusing the
            // *SPC700's* main-cycle ratio (used correctly a few lines
            // below for `spc_cycle_debt`) with the audio sample rate --
            // 1/3 of ~2.68MHz is ~893kHz, not 32kHz, so samples were being
            // generated ~28x too fast.
            sample_divider: 84,
            sample_counter: 0,
            spc_cycle_debt: 0,
        };

        apu.init_boot_rom();

        apu
    }

    /// Loads the real SPC700 IPL ROM at $FFC0-$FFFF and resets the SPC700
    /// to run it from its actual reset vector ($FFFE-$FFFF, which the ROM
    /// itself sets to $FFC0). This is real machine code execution, not a
    /// scripted approximation of the handshake protocol: the $AA/$BB
    /// ready signal, the $CC upload handshake, and the byte-transfer
    /// protocol all emerge from the SPC700 actually running this ROM.
    pub fn init_boot_rom(&mut self) {
        {
            let mut ram = self.ram.lock().unwrap();
            ram[0xFFC0..=0xFFFF].copy_from_slice(&IPL_ROM);
        }
        *self.ports.lock().unwrap() = ApuPorts::default();
        self.spc700.reset();

        self.frame_counter = 0;
        self.frame_cycles = 0;
    }

    /// Read from APU port ($2140-$2143), the main CPU's read side. Backed
    /// by the same `ApuPorts` the real, executing SPC700 writes to.
    ///
    /// # Arguments
    /// * `port` - Port index (0-3, maps to $2140-$2143)
    ///
    /// # Returns
    /// The value at the specified port, or 0 if port is out of range
    pub fn read_port(&self, port: u8) -> u8 {
        if port < 4 {
            self.ports.lock().unwrap().apu_to_cpu[port as usize]
        } else {
            0
        }
    }

    /// Write to APU port ($2140-$2143), the main CPU's write side. Lands
    /// in the same `ApuPorts` the real, executing SPC700 reads from at RAM
    /// addresses $F4-$F7 -- it does NOT affect what `read_port`
    /// subsequently returns; those are physically separate latches on
    /// real hardware, and only the (now actually running) SPC700 code
    /// decides when/whether to echo anything back.
    ///
    /// # Arguments
    /// * `port` - Port index (0-3, maps to $2140-$2143)
    /// * `value` - Value to write
    pub fn write_port(&mut self, port: u8, value: u8) {
        if port < 4 {
            self.ports.lock().unwrap().cpu_to_apu[port as usize] = value;
        }
    }

    /// What the main CPU last wrote to a given port (what the SPC700 sees
    /// as input at $F4-$F7). Exposed for testing/debugging.
    pub fn cpu_to_apu_port(&self, port: u8) -> u8 {
        if port < 4 {
            self.ports.lock().unwrap().cpu_to_apu[port as usize]
        } else {
            0
        }
    }

    /// True if the SPC700 hit an opcode outside the validated subset
    /// `execute_opcode` implements and stopped advancing. See
    /// `Spc700::halted`.
    pub fn spc700_halted(&self) -> Option<u8> {
        self.spc700.halted
    }

    /// Read from APU RAM.
    ///
    /// $00-$7F used to be special-cased here as DSP registers, but that
    /// was inconsistent with the real execution path: `Spc700::read_mem`/
    /// `write_mem` (what the actually-executing SPC700 uses for every
    /// memory access, including its zero page) treats $00-$7F as ordinary
    /// RAM -- real SPC700 code has no memory-mapped access to the DSP at
    /// all, only the indirect $F2 (select)/$F3 (data) port pair (see
    /// `Spc700::write_mem`'s doc comment). Diverting this accessor to DSP
    /// registers meant it silently disagreed with what the CPU it's
    /// supposedly inspecting actually sees at those addresses. This now
    /// always reads real RAM for the full 0-0xFFFF range, matching
    /// `read_mem`/`write_mem` (and making this what used to be called
    /// `raw_ram` -- that separate method is gone since it's now identical
    /// to this one).
    ///
    /// # Arguments
    /// * `addr` - Address in APU RAM (0x0000-0xFFFF)
    ///
    /// # Returns
    /// The value at the specified address
    pub fn read_ram(&self, addr: u16) -> u8 {
        self.ram.lock().unwrap()[addr as usize]
    }

    /// Write to APU RAM. See `read_ram`'s doc comment: $00-$7F is always
    /// plain RAM here, consistent with `Spc700::write_mem`, not diverted
    /// to DSP registers. Use `Dsp::write_reg`/`Apu::dsp_reg` (or a
    /// dedicated DSP-register accessor) if DSP register access by address
    /// is actually needed -- that is a semantically different operation
    /// from writing RAM and should not be aliased onto this method.
    ///
    /// # Arguments
    /// * `addr` - Address in APU RAM (0x0000-0xFFFF)
    /// * `value` - Value to write
    pub fn write_ram(&mut self, addr: u16, value: u8) {
        self.ram.lock().unwrap()[addr as usize] = value;
    }

    /// Advance APU timing by given cycles.
    /// 
    /// This simulates the SPC700 and DSP operation over time.
    /// For a stub, we just track frame timing and generate silence.
    /// 
    /// # Arguments
    /// * `cycles` - Number of APU cycles to advance
    pub fn tick(&mut self, cycles: u32) {
        self.frame_cycles += cycles;

        // Run SPC700 for these cycles (~1/3 the main CPU speed). Accumulate
        // the fractional remainder across calls instead of truncating it
        // away each time -- see `spc_cycle_debt`'s doc comment for why a
        // naive per-call `cycles / 3` starves the SPC700.
        self.spc_cycle_debt += cycles;
        let spc_cycles = self.spc_cycle_debt / 3;
        self.spc_cycle_debt %= 3;
        for _ in 0..spc_cycles {
            self.spc700.step();
        }
        
        // Generate audio samples
        // Sample rate is about 32kHz
        self.sample_counter += cycles;
        while self.sample_counter >= self.sample_divider {
            self.sample_counter -= self.sample_divider;
            
            // Generate DSP sample
            let (left, right) = {
                let ram = self.ram.lock().unwrap();
                self.dsp.lock().unwrap().sample(&ram)
            };
            self.sample_buffer.push_back((left, right));
        }
        
        // Check if we've completed a frame
        if self.frame_cycles >= self.cycles_per_frame {
            self.frame_cycles -= self.cycles_per_frame;
            self.frame_counter = self.frame_counter.wrapping_add(1);
            
            // Keep buffer at reasonable size (max ~10 seconds of audio)
            while self.sample_buffer.len() > 320000 {
                self.sample_buffer.pop_front();
            }
        }
    }

    /// Get audio sample if available (mono).
    /// 
    /// # Returns
    /// Some(sample) if a sample is available, None otherwise
    pub fn sample(&mut self) -> Option<i16> {
        self.sample_buffer
            .pop_front()
            .map(|(left, right)| ((left as i32 + right as i32) / 2) as i16)
    }
    
    /// Get stereo audio sample if available.
    /// 
    /// # Returns
    /// Some((left, right)) if a sample is available, None otherwise
    pub fn sample_stereo(&mut self) -> Option<(i16, i16)> {
        self.sample_buffer.pop_front()
    }

    /// Serializes the complete APU: 64KB RAM, the CPU<->APU port latches,
    /// the SPC700's registers/timers, the full DSP (register file AND all
    /// transient synthesis state -- see `Dsp::save_state`), and the
    /// sample-pacing counters. A restored state resumes mid-note.
    pub fn save_state(&self, out: &mut Vec<u8>) {
        use crate::state::{put_bool, put_bytes, put_u16, put_u32, put_u8};
        put_bytes(out, &self.ram.lock().unwrap()[..]);
        {
            let ports = self.ports.lock().unwrap();
            put_bytes(out, &ports.cpu_to_apu);
            put_bytes(out, &ports.apu_to_cpu);
        }
        // SPC700 registers and timer hardware.
        put_u8(out, self.spc700.a);
        put_u8(out, self.spc700.x);
        put_u8(out, self.spc700.y);
        put_u8(out, self.spc700.sp);
        put_u16(out, self.spc700.pc);
        put_u8(out, self.spc700.psw.to_byte());
        put_bool(out, self.spc700.halted.is_some());
        put_u8(out, self.spc700.halted.unwrap_or(0));
        for i in 0..3 {
            put_bool(out, self.spc700.timer_enable[i]);
            put_u8(out, self.spc700.timer_target[i]);
            put_u8(out, self.spc700.timer_divider[i]);
            put_u8(out, self.spc700.timer_counter[i]);
            put_u32(out, self.spc700.timer_prescaler[i]);
        }
        put_u8(out, self.spc700.control);
        put_u8(out, self.spc700.dsp_addr);
        // The complete DSP (registers + transient synthesis state).
        self.dsp.lock().unwrap().save_state(out);
        // APU-level pacing state.
        put_u8(out, self.frame_counter);
        put_u8(out, self.control);
        put_u32(out, self.frame_cycles);
        put_u32(out, self.sample_counter);
        put_u32(out, self.spc_cycle_debt);
    }

    /// Restores state produced by `save_state`.
    pub(crate) fn load_state(&mut self, r: &mut crate::state::StateReader) -> Result<(), crate::error::EmulationError> {
        {
            let bytes = r.bytes(65536)?;
            self.ram.lock().unwrap().copy_from_slice(bytes);
        }
        {
            let mut ports = self.ports.lock().unwrap();
            ports.cpu_to_apu.copy_from_slice(r.bytes(4)?);
            ports.apu_to_cpu.copy_from_slice(r.bytes(4)?);
        }
        self.spc700.a = r.u8()?;
        self.spc700.x = r.u8()?;
        self.spc700.y = r.u8()?;
        self.spc700.sp = r.u8()?;
        self.spc700.pc = r.u16()?;
        self.spc700.psw = Psw::from_byte(r.u8()?);
        let halted = r.bool()?;
        let halted_op = r.u8()?;
        self.spc700.halted = if halted { Some(halted_op) } else { None };
        for i in 0..3 {
            self.spc700.timer_enable[i] = r.bool()?;
            self.spc700.timer_target[i] = r.u8()?;
            self.spc700.timer_divider[i] = r.u8()?;
            self.spc700.timer_counter[i] = r.u8()?;
            self.spc700.timer_prescaler[i] = r.u32()?;
        }
        self.spc700.control = r.u8()?;
        self.spc700.dsp_addr = r.u8()?;
        self.dsp.lock().unwrap().load_state(r)?;
        self.frame_counter = r.u8()?;
        self.control = r.u8()?;
        self.frame_cycles = r.u32()?;
        self.sample_counter = r.u32()?;
        self.spc_cycle_debt = r.u32()?;
        self.sample_buffer.clear();
        Ok(())
    }

    /// Reset APU to initial state.
    pub fn reset(&mut self) {
        self.ram.lock().unwrap().fill(0);
        *self.ports.lock().unwrap() = ApuPorts::default();
        self.frame_counter = 0;
        self.control = 0;
        self.sample_buffer.clear();
        self.frame_cycles = 0;
        self.spc700.reset();
        self.dsp.lock().unwrap().reset();
        
        // Reinitialize boot ROM
        self.init_boot_rom();
    }

    /// Get the current frame counter.
    pub fn frame_counter(&self) -> u8 {
        self.frame_counter
    }

    /// Get the control register value.
    pub fn control(&self) -> u8 {
        self.control
    }

    /// Set the control register value.
    pub fn set_control(&mut self, value: u8) {
        self.control = value;
    }

    /// Get the number of cycles per frame.
    pub fn cycles_per_frame(&self) -> u32 {
        self.cycles_per_frame
    }

    /// Get the current frame cycle count.
    pub fn frame_cycles(&self) -> u32 {
        self.frame_cycles
    }

    /// Check if there's a sample available.
    pub fn has_sample(&self) -> bool {
        !self.sample_buffer.is_empty()
    }

    /// Clear the sample buffer.
    pub fn clear_buffer(&mut self) {
        self.sample_buffer.clear();
    }

    /// Get the current number of samples in the buffer.
    pub fn buffer_size(&self) -> usize {
        self.sample_buffer.len()
    }
    
    /// Get reference to SPC700 for debugging
    pub fn spc700(&self) -> &Spc700 {
        &self.spc700
    }
    
    /// Reads a DSP register directly, for debugging/diagnostics.
    pub fn dsp_reg(&self, addr: u8) -> u8 {
        self.dsp.lock().unwrap().read_reg(addr)
    }

}

impl Default for Apu {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_apu() {
        let apu = Apu::new();
        assert_eq!(apu.frame_counter(), 0);
        assert_eq!(apu.control(), 0);
        assert!(apu.buffer_size() == 0);
    }

    /// Regression guard: `sample_divider` used to be `3` (confusing the
    /// SPC700's own main-cycle stepping ratio with the DSP's audio sample
    /// rate), which generated samples about 28x too fast -- one main-CPU
    /// second's worth of ticking produced ~893,000 buffered samples
    /// instead of the ~32,000 a real 32kHz output actually needs. Ticking
    /// one assumed-clock-rate second's worth of main CPU cycles (~2.68MHz,
    /// the same rate `SystemBus::tick_ppu`'s dot-doubling assumes) must
    /// land close to 32,000 samples, not orders of magnitude off in either
    /// direction.
    #[test]
    fn tick_generates_samples_at_approximately_32khz_not_the_old_28x_too_fast_rate() {
        let mut apu = Apu::new();
        const ONE_SECOND_OF_MAIN_CPU_CYCLES: u32 = 2_680_000;
        const CHUNK: u32 = 1000;
        let mut remaining = ONE_SECOND_OF_MAIN_CPU_CYCLES;
        while remaining > 0 {
            let step = remaining.min(CHUNK);
            apu.tick(step);
            remaining -= step;
        }

        let generated = apu.buffer_size();
        assert!(
            (30_000..=34_000).contains(&generated),
            "expected ~32,000 samples for one second of main-CPU-cycle ticking, got {} \
             (a value near 893,000 would mean the old too-fast divider regressed)",
            generated
        );
    }

    #[test]
    fn test_port_write_does_not_affect_read_side() {
        // $2140-$2143 are two independent one-way latches on real
        // hardware: the CPU's writes go to the SPC700's *input*, and the
        // CPU's reads come from a value only the SPC700 can set. A
        // previous version of this code modeled them as a single
        // loopback array, which meant the CPU's own writes (e.g. while
        // blanket-clearing hardware registers during boot) would silently
        // clobber the handshake value it later reads back.
        let mut apu = Apu::new();
        let initial_port1 = apu.read_port(1);

        apu.write_port(1, 0xCD);
        apu.write_port(2, 0xEF);
        apu.write_port(3, 0x12);

        assert_eq!(apu.read_port(1), initial_port1, "a CPU write to port 1 must not change what the CPU reads back");

        assert_eq!(apu.cpu_to_apu_port(1), 0xCD, "but the write must still be recorded on the CPU->APU side");
        assert_eq!(apu.cpu_to_apu_port(2), 0xEF);
        assert_eq!(apu.cpu_to_apu_port(3), 0x12);

        // Test out of range port
        assert_eq!(apu.read_port(4), 0);
        assert_eq!(apu.cpu_to_apu_port(4), 0);
    }

    /// Enough APU cycles for the real SPC700 IPL ROM to clear its 239-byte
    /// page-0 RAM loop and reach the ready handshake (well under 100
    /// instructions); generous headroom for slower paths through it too.
    const ENOUGH_CYCLES_FOR_IPL_READY: u32 = 10_000;

    #[test]
    fn test_real_spc700_execution_reaches_the_ipl_ready_handshake() {
        // Unlike before, $AA/$BB are no longer hardcoded -- they only
        // appear once the real, verified 64-byte IPL ROM (see `IPL_ROM`)
        // actually executes far enough to write them via genuine SPC700
        // instructions (`MOV $F4,#$AA` / `MOV $F5,#$BB`).
        let mut apu = Apu::new();
        assert_eq!(apu.read_port(0), 0x00, "nothing has run yet right after construction");

        apu.tick(ENOUGH_CYCLES_FOR_IPL_READY);

        assert_eq!(apu.spc700_halted(), None, "must not hit an opcode outside the validated subset getting here");
        assert_eq!(apu.read_port(0), 0xAA);
        assert_eq!(apu.read_port(1), 0xBB);
    }

    #[test]
    fn test_real_spc700_ignores_stray_writes_before_seeing_the_cc_sentinel() {
        // The real IPL ROM's ready loop ("CMP $F4,#$CC / BNE -") only ever
        // reacts to the literal $CC value -- writes of anything else (e.g.
        // unrelated boot code touching $2140 while clearing hardware
        // registers) just fail the comparison and the loop keeps spinning,
        // never touching the ready signal.
        let mut apu = Apu::new();
        apu.tick(ENOUGH_CYCLES_FOR_IPL_READY);
        assert_eq!(apu.read_port(0), 0xAA);

        apu.write_port(0, 0x00); // looks like e.g. STZ $2140, not the real handshake
        apu.tick(1_000);

        assert_eq!(apu.read_port(0), 0xAA, "a stray write must not disturb the ready signal");
    }

    #[test]
    fn test_real_spc700_executes_the_first_upload_command_and_echoes_it() {
        // Drives the real, executing SPC700 through the verified handshake
        // sequence: address setup on APUIO2/APUIO3, a flag on APUIO1, then
        // the $CC sentinel on APUIO0 -- and confirms the IPL ROM's own
        // "Start:" code (MOVW YA,$F6 / MOVW $00,YA / MOVW YA,$F4 / MOV
        // $F4,A / ...) actually runs and echoes $CC back, proving real
        // instruction execution drives this, not a scripted response.
        let mut apu = Apu::new();
        apu.tick(ENOUGH_CYCLES_FOR_IPL_READY);
        assert_eq!(apu.read_port(0), 0xAA);

        apu.write_port(2, 0x34); // target address low
        apu.write_port(3, 0x12); // target address high
        apu.write_port(1, 0x01); // nonzero -> "upload starts here", not execute
        apu.write_port(0, 0xCC);
        apu.tick(200);

        assert_eq!(apu.spc700_halted(), None);
        assert_eq!(apu.read_port(0), 0xCC, "the real IPL code must echo the command back");
        assert_eq!(apu.read_ram(0x0000), 0x34, "MOVW $00,YA must have staged the address's low byte");
        assert_eq!(apu.read_ram(0x0001), 0x12, "...and its high byte");
    }

    #[test]
    fn test_real_spc700_executes_the_execute_command_and_jumps() {
        // flag=0 on APUIO1 means "jump to this address" rather than
        // "upload more data here" -- confirms the real IPL code's mode
        // check (MOV A,Y / MOV X,A / BNE Trans / JMP [$0000+X]) actually
        // transfers control to the requested address.
        let mut apu = Apu::new();
        apu.tick(ENOUGH_CYCLES_FOR_IPL_READY);

        apu.write_port(2, 0x00);
        apu.write_port(3, 0x03);
        apu.write_port(1, 0x00); // flag = 0 -> execute
        apu.write_port(0, 0xCC);

        // Tick in small increments and stop the instant PC reaches the
        // jump target -- the target address ($0300) is uninitialized RAM
        // (0x00 = NOP, a real valid opcode per the SPC700 instruction
        // chart, not garbage), so ticking further would just run NOPs and
        // advance PC past it.
        let mut reached_target = false;
        for _ in 0..50 {
            if apu.spc700().pc == 0x0300 {
                reached_target = true;
                break;
            }
            apu.tick(3);
        }

        assert!(reached_target, "the real IPL code must jump to the requested address; PC stuck at {:04X}", apu.spc700().pc);
        assert_eq!(apu.spc700_halted(), None);
    }

    #[test]
    fn test_ram_read_write() {
        let mut apu = Apu::new();
        
        // Write to RAM
        apu.write_ram(0x0000, 0x42);
        apu.write_ram(0xFFFF, 0x24);
        apu.write_ram(0x1234, 0xAB);
        
        // Read from RAM
        assert_eq!(apu.read_ram(0x0000), 0x42);
        assert_eq!(apu.read_ram(0xFFFF), 0x24);
        assert_eq!(apu.read_ram(0x1234), 0xAB);
    }
    
    #[test]
    fn ram_access_at_the_dsp_register_range_is_plain_ram_not_diverted_to_the_dsp() {
        // Regression guard: `read_ram`/`write_ram` used to special-case
        // $00-$7F as DSP registers, which disagreed with the real
        // execution path (`Spc700::read_mem`/`write_mem` -- what the
        // actually-executing SPC700 uses -- always treats $00-$7F as
        // ordinary RAM; the DSP is only reachable indirectly via the
        // $F2/$F3 port pair). Writes through `write_ram` at $00-$7F must
        // land in real RAM and read back from `read_ram` unchanged, and
        // must NOT be visible as DSP register state.
        let mut apu = Apu::new();

        apu.write_ram(0x00, 0x42);
        apu.write_ram(0x01, 0x34);
        apu.write_ram(0x02, 0x12);

        assert_eq!(apu.read_ram(0x00), 0x42);
        assert_eq!(apu.read_ram(0x01), 0x34);
        assert_eq!(apu.read_ram(0x02), 0x12);

        // The DSP's own register file must be untouched by those writes --
        // only `Dsp::write_reg` (reached from real code via $F2/$F3, see
        // `spc700_f2_f3_ports_actually_reach_the_dsp_register_file`) may
        // change it.
        assert_eq!(apu.dsp_reg(0x00), 0, "plain RAM writes at $00-$7F must not alias onto DSP registers");
        assert_eq!(apu.dsp_reg(0x01), 0);
        assert_eq!(apu.dsp_reg(0x02), 0);
    }

    #[test]
    fn test_tick_advances_timing() {
        let mut apu = Apu::new();
        
        let initial_cycles = apu.frame_cycles();
        
        // Tick by a small number of cycles
        apu.tick(1000);
        
        assert_eq!(apu.frame_cycles(), initial_cycles + 1000);
    }

    #[test]
    fn test_reset() {
        let mut apu = Apu::new();
        
        // Write some data
        apu.write_port(0, 0xFF);
        apu.write_ram(0x1234, 0xFF);
        
        // Tick to generate samples
        apu.tick(apu.cycles_per_frame() + 1);
        
        // Reset
        apu.reset();

        // Right after reset, nothing has executed yet (ports are zero
        // until the real, reset SPC700 actually runs far enough to write
        // $AA/$BB itself -- see `test_real_spc700_execution_reaches_the_ipl_ready_handshake`).
        assert_eq!(apu.read_port(0), 0);
        assert_eq!(apu.read_port(1), 0);
        assert_eq!(apu.read_ram(0x1234), 0);
        assert!(!apu.has_sample());
        assert_eq!(apu.frame_counter(), 0);

        apu.tick(10_000);
        assert_eq!(apu.read_port(0), 0xAA, "the reset SPC700 must be able to run again after reset");
        assert_eq!(apu.read_port(1), 0xBB);
    }

    #[test]
    fn test_adsr() {
        let mut adsr = Adsr::new();
        assert_eq!(adsr.mode, EnvMode::Release);

        // ADSR1 with bit7 set (ADSR enabled) and a fast attack (AR=0x0F ->
        // rate 31 -> jumps straight to max), ADSR2 with a mid sustain
        // level. Drive the envelope with a global counter that advances
        // each sample so the rate gate actually fires.
        let adsr1 = 0x8F; // enable + attack rate 15
        let adsr2 = 0x7F; // sustain level 3, sustain rate 31
        let gain = 0;

        adsr.key_on();
        assert_eq!(adsr.mode, EnvMode::Attack);

        let mut counter: i32 = 0;
        for _ in 0..2000 {
            counter -= 1;
            if counter < 0 {
                counter = SIMPLE_COUNTER_RANGE as i32 - 1;
            }
            adsr.run(adsr1, adsr2, gain, counter as u32);
        }

        // A fast attack must have driven the envelope well above zero and
        // moved past the attack phase.
        assert!(adsr.level > 0, "envelope must rise during/after attack, got {}", adsr.level);
        assert!(adsr.mode >= EnvMode::Decay, "must have left the attack phase, mode = {:?}", adsr.mode);

        // Key-off must ramp the envelope down to zero.
        adsr.key_off();
        assert_eq!(adsr.mode, EnvMode::Release);
        for _ in 0..2000 {
            adsr.run(adsr1, adsr2, gain, 0);
        }
        assert_eq!(adsr.level, 0, "release must decay the envelope to silence");
    }

    #[test]
    fn adsr_rate_gating_makes_a_slow_attack_take_far_longer_than_a_fast_one() {
        // The whole point of the rate-table timing: a low attack rate must
        // take many more samples to reach full scale than a high one. The
        // old linear-per-sample model moved the envelope by a big fixed
        // delta every sample regardless of rate, collapsing all envelopes
        // to roughly the same (too-short) duration -- which is what made
        // notes sound like clicks. `run` a fast-attack and a slow-attack
        // envelope and confirm the slow one is still far from full scale
        // when the fast one has already saturated.
        let steps_to_run = 4000;

        let mut fast = Adsr::new();
        fast.key_on();
        let mut slow = Adsr::new();
        slow.key_on();

        let mut counter: i32 = 0;
        for _ in 0..steps_to_run {
            counter -= 1;
            if counter < 0 {
                counter = SIMPLE_COUNTER_RANGE as i32 - 1;
            }
            let c = counter as u32;
            fast.run(0x8F, 0x00, 0, c); // attack rate 15 (very fast)
            slow.run(0x82, 0x00, 0, c); // attack rate 2 (slow)
        }

        assert!(fast.level > slow.level,
            "a faster attack rate must reach a higher envelope in the same time (fast={}, slow={})",
            fast.level, slow.level);
    }
    
    #[test]
    fn test_brr_decoder() {
        let mut decoder = BrrDecoder::new();
        let header = 0x00; // No filter, 9 bytes (standard BRR)
        let data = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77];
        let mut output = [0i16; 16];

        decoder.decode(header, &data, &mut output);

        // The decoder should produce some output (may be all zeros for this test data)
        // Just verify it doesn't panic
        assert_eq!(output.len(), 16);
    }

    fn isolated_spc700() -> Spc700 {
        let ram = Arc::new(Mutex::new([0u8; 65536]));
        let ports = Arc::new(Mutex::new(ApuPorts::default()));
        let dsp = Arc::new(Mutex::new(Dsp::new()));
        Spc700::new(ram, ports, dsp)
    }

    #[test]
    fn mov_a_indirect_dp_plus_y_reads_the_pointer_at_dp_then_indexes_by_y() {
        // Opcode 0xF7: MOV A,[dp]+Y. Unlike 0xE7 (MOV A,[dp+X], which
        // indexes the *direct-page fetch* by X before reading the
        // pointer), this reads the 16-bit pointer straight from `dp` and
        // adds Y to the *resulting address* afterward -- the SPC700
        // analogue of 6502/65816 "(dp),Y". Missing until now: it halted
        // the real uploaded sound engine's driver partway through, right
        // where hardware would start actually triggering notes.
        let mut spc = isolated_spc700();

        spc.write_mem(0x0200, 0xF7); // MOV A,[$10]+Y
        spc.write_mem(0x0201, 0x10);
        spc.write_mem(0x0010, 0x00); // pointer at dp $10/$11 = $3000
        spc.write_mem(0x0011, 0x30);
        spc.write_mem(0x3005, 0x99); // $3000 + Y(5) = $3005

        spc.pc = 0x0200;
        spc.y = 5;
        spc.step();

        assert_eq!(spc.a, 0x99);
        assert_eq!(spc.pc, 0x0202);
        assert_eq!(spc.halted, None);
    }

    #[test]
    fn mov_y_dp_plus_x_reads_direct_page_indexed_by_x() {
        // Opcode 0xFB: MOV Y,dp+X. Missing until now for the same reason
        // as 0xF7 -- found by running the real ROM's SPC700 driver far
        // enough to reach it.
        let mut spc = isolated_spc700();

        spc.write_mem(0x0200, 0xFB); // MOV Y,$10+X
        spc.write_mem(0x0201, 0x10);
        spc.write_mem(0x0015, 0x77); // dp $10 + X(5) = $15

        spc.pc = 0x0200;
        spc.x = 5;
        spc.step();

        assert_eq!(spc.y, 0x77);
        assert_eq!(spc.pc, 0x0202);
        assert_eq!(spc.halted, None);
    }

    /// Builds APU RAM with a sample directory at page 2 (dir=$02) whose
    /// source 0 points at a BRR sample at $0300: a self-looping run of
    /// blocks whose decoded samples ramp upward (shift 0, filter 0, raw
    /// nibbles 0..7 -> decoded values 0..7 repeating). Returns the RAM.
    fn ram_with_ramp_sample() -> Box<[u8; 65536]> {
        let mut ram: Box<[u8; 65536]> = vec![0u8; 65536].try_into().unwrap();
        // Directory entry 0 at $0200: start=$0300, loop=$0300.
        ram[0x0200] = 0x00;
        ram[0x0201] = 0x03;
        ram[0x0202] = 0x00;
        ram[0x0203] = 0x03;
        // One BRR block at $0300: header shift=0/filter=0, loop+end set so
        // it loops onto itself forever. Nibbles 0,1,2,...,15 -> a ramp.
        ram[0x0300] = 0x03; // end(bit0) + loop(bit1)
        for i in 0..8usize {
            let hi = (i * 2) as u8;
            let lo = (i * 2 + 1) as u8;
            ram[0x0301 + i] = (hi << 4) | (lo & 0x0F);
        }
        ram
    }

    /// Configures DSP voice 0 to play source 0 (dir page $02) at the given
    /// pitch with instant-attack ADSR and full volume.
    fn configure_ramp_voice(dsp: &mut Dsp, pitch: u16) {
        dsp.write_reg(0x5D, 0x02); // DIR = page 2
        dsp.write_reg(0x00, 0x7F); // VOL(L)
        dsp.write_reg(0x01, 0x7F); // VOL(R)
        dsp.write_reg(0x02, (pitch & 0xFF) as u8);
        dsp.write_reg(0x03, (pitch >> 8) as u8);
        dsp.write_reg(0x04, 0x00); // SRCN 0
        dsp.write_reg(0x05, 0x8F); // ADSR1: enabled, fastest attack
        dsp.write_reg(0x06, 0xE0); // ADSR2: max sustain level
        dsp.write_reg(0x0C, 0x7F); // MVOLL
        dsp.write_reg(0x1C, 0x7F); // MVOLR
        dsp.write_reg(0x4C, 0x01); // KON voice 0
    }

    #[test]
    fn resampling_interpolates_between_source_samples_instead_of_stair_stepping() {
        // Regression guard for the nearest-neighbor resampler: at half
        // pitch (0x0800), nearest-neighbor emits every source sample
        // exactly twice (a stair-step, heard as harsh aliasing on almost
        // every note). A 4-point interpolator must instead produce
        // strictly intermediate values on the half-way positions while a
        // monotonic ramp is playing.
        let ram = ram_with_ramp_sample();
        let mut dsp = Dsp::new();
        configure_ramp_voice(&mut dsp, 0x0800);

        let mut out = Vec::new();
        for _ in 0..2000 {
            let (l, _r) = dsp.sample(&ram);
            out.push(l as i32);
        }

        // Ignore the attack/warm-up, then measure stair-stepping: count
        // adjacent equal pairs among nonzero samples. Nearest-neighbor at
        // half pitch makes ~100% of adjacent pairs equal; interpolation
        // makes most of them distinct.
        let tail: Vec<i32> = out[600..].iter().copied().filter(|&v| v != 0).collect();
        assert!(tail.len() > 200, "the looping ramp must keep producing nonzero samples");
        let equal_pairs = tail.windows(2).filter(|w| w[0] == w[1]).count();
        let ratio = equal_pairs as f64 / (tail.len() - 1) as f64;
        assert!(
            ratio < 0.6,
            "at half pitch, adjacent output samples must mostly be interpolated (distinct), \
             not duplicated stair-steps; got {:.0}% equal pairs",
            ratio * 100.0
        );
    }

    #[test]
    fn echo_uses_the_real_edl_delay_not_one_sample() {
        // Regression guard for the echo path: with EDL=1 the first echo
        // reflection must arrive ~512 samples (16ms) after the dry signal,
        // not 1 sample after (the old implementation read back the sample
        // written one position earlier, turning the echo into a comb
        // filter over the whole mix). Voice 0 plays the ramp with EON=0
        // (not fed to echo) for the first probe, then EON=1.
        let ram = ram_with_ramp_sample();
        let mut dsp = Dsp::new();
        configure_ramp_voice(&mut dsp, 0x1000);
        dsp.write_reg(0x7D, 0x01); // EDL = 1 -> 512-sample delay
        dsp.write_reg(0x4D, 0x01); // EON: voice 0 feeds the echo
        dsp.write_reg(0x2C, 0x7F); // EVOL(L) max
        dsp.write_reg(0x3C, 0x7F); // EVOL(R) max
        dsp.write_reg(0x0D, 0x00); // no feedback
        dsp.write_reg(0x6C, 0x00); // FLG: echo writes enabled
        // FIR: identity (coefficient 7 = 127, the "newest sample" tap).
        // Coefficient 7 lives at $7F (C0-C7 are spaced 16 apart -- see
        // `Dsp::write_reg`'s FIR comment), NOT $47 (which is just voice
        // 4's ordinary GAIN register).
        dsp.write_reg(0x0F, 0x00);
        dsp.write_reg(0x7F, 0x7F);

        // The echo contribution is (delayed voice output). For the first
        // 500 samples the delay line still holds zeros, so output ==
        // dry-only; once the 512-sample delay elapses the echo starts
        // adding energy. Compare mean |output| of the two windows against
        // a dsp with echo volume muted.
        let mut with_echo = Vec::new();
        for _ in 0..1600 {
            let (l, _r) = dsp.sample(&ram);
            with_echo.push(l as i32);
        }

        let mut dsp_dry = Dsp::new();
        configure_ramp_voice(&mut dsp_dry, 0x1000);
        dsp_dry.write_reg(0x7D, 0x01);
        dsp_dry.write_reg(0x4D, 0x01);
        dsp_dry.write_reg(0x0D, 0x00);
        dsp_dry.write_reg(0x6C, 0x00);
        dsp_dry.write_reg(0x0F, 0x00);
        dsp_dry.write_reg(0x7F, 0x7F);
        // echo volume left at 0 -> pure dry reference
        let mut dry = Vec::new();
        for _ in 0..1600 {
            let (l, _r) = dsp_dry.sample(&ram);
            dry.push(l as i32);
        }

        // Window A: samples 100..450 (before the 512-sample echo delay
        // elapses) must match the dry signal exactly -- any difference
        // there means echo is arriving too early.
        assert_eq!(
            &with_echo[100..450],
            &dry[100..450],
            "echo energy must NOT appear before the EDL delay elapses"
        );
        // Window B: after the delay, the echo-enabled output must diverge
        // from the dry signal (the reflection has arrived).
        let diverged = (700..1500).any(|i| with_echo[i] != dry[i]);
        assert!(diverged, "echo energy must appear after the EDL delay elapses");
    }

    #[test]
    fn spc700_f2_f3_ports_actually_reach_the_dsp_register_file() {
        // Regression guard for the root cause of "audio is permanently
        // silent despite a fully-implemented DSP and a driver that
        // genuinely runs": real SPC700 code has no other way to touch the
        // DSP except `MOV $F2,#reg` (select) then `MOV $F3,#value`
        // (read/write that register's data) -- there is no direct
        // memory-mapped access to DSP registers anywhere else in the
        // address space. `write_mem`/`read_mem` used to fall through to
        // plain RAM for both $F2 and $F3, so every real driver's register
        // writes (KON, MVOLL/MVOLR, per-voice volume/pitch/ADSR, ...)
        // silently landed in ordinary zero-page RAM instead -- the SPC700
        // CPU, its RAM, and the DSP's own synthesis math were all
        // individually correct and testable in isolation, but nothing
        // ever connected them during real execution.
        let mut spc = isolated_spc700();

        // Select DSP register $0C (MVOLL) via $F2, write $7F via $F3.
        spc.write_mem(0xF2, 0x0C);
        spc.write_mem(0xF3, 0x7F);
        assert_eq!(
            spc.dsp.lock().unwrap().read_reg(0x0C),
            0x7F,
            "MOV $F2,#$0C / MOV $F3,#$7F must reach the DSP's MVOLL register, not plain RAM"
        );

        // Reading $F3 back must reflect the DSP's live value for whatever
        // $F2 last selected, and $F2 itself must read back as the
        // selected address.
        assert_eq!(spc.read_mem(0xF2), 0x0C);
        assert_eq!(spc.read_mem(0xF3), 0x7F);

        // Selecting a different register and reading must return THAT
        // register's value, not a stale copy of the previous one.
        spc.dsp.lock().unwrap().write_reg(0x4C, 0x03); // KON, set directly for this check
        spc.write_mem(0xF2, 0x4C);
        assert_eq!(spc.read_mem(0xF3), 0x03);
    }

    #[test]
    fn adsr_envelope_output_keeps_full_precision_for_voice_mixing() {
        // Regression guard: `get_output()` must return the full 11-bit
        // envelope value (0..0x7FF) that `Voice::sample` scales samples by
        // via `(brr_sample * env) >> 11`. An earlier version returned
        // `level >> 8` (real hardware's ENVX *status register* truncation,
        // for CPU readback) and fed that into the mixer, scaling every
        // voice's output down by ~256x -- technically non-zero (easy to
        // miss) but effectively inaudible.
        let mut adsr = Adsr::new();
        adsr.level = 0x7FF;
        assert_eq!(adsr.get_output(), 0x7FF);

        adsr.level = 0x400;
        assert_eq!(adsr.get_output(), 0x400, "must not be divided down (e.g. by 256) before use as a mixing envelope");
    }

    #[test]
    fn brr_decode_does_not_overflow_across_many_blocks_with_extreme_history() {
        // Regression guard for a real `attempt to multiply with overflow`
        // panic hit after ~3M real-ROM steps once notes actually started
        // triggering (see the $F2/$F3 fix above -- this code path was
        // simply never exercised with real driver-triggered data before
        // that). Filter 3 has the largest multiply coefficients, and
        // maximal-magnitude nibbles/history are the adversarial case for
        // the old `i16` intermediate arithmetic.
        let mut decoder = BrrDecoder::new();
        let header = 0x0F; // shift=0, filter=3 (header & 0x0C == 0x0C -> filter 3 branch)
        let data = [0xFFu8; 8]; // every nibble = 0xF (the maximal-magnitude negative nibble)
        let mut output = [0i16; 16];

        for _ in 0..200 {
            decoder.decode(header, &data, &mut output);
        }
        // Must not panic (the real regression), and must stay within
        // valid i16 PCM range.
        for &s in &output {
            assert!((-32768..=32767).contains(&(s as i32)));
        }
    }

    #[test]
    fn brr_decode_extracts_shift_and_filter_from_the_correct_header_bits() {
        // Regression guard: the header parser used to read shift from
        // bits 0-3 and filter from a derived value, when real hardware's
        // layout (byte = `ssssffle`) puts shift in bits 4-7 and filter in
        // bits 2-3. A shift-12 filter-0 header applied to nibble value 1
        // must produce `((1 << 12) >> 1) * 2 = 4096` on the first sample
        // (no filter, no history yet, and the decoder's final step always
        // doubles the clamped intermediate value -- see `decode`'s doc
        // comment) -- if shift/filter were still being read from the
        // wrong bits, this would instead be treated as shift=0xF (invalid,
        // clamped to 0) filter=(0x0F>>2)&3=3.
        let mut decoder = BrrDecoder::new();
        let header = 0xC0; // shift=12 (0xC), filter=0
        let mut data = [0u8; 8];
        data[0] = 0x01; // first nibble (low nibble of byte 0) = 1
        let mut output = [0i16; 16];

        decoder.decode(header, &data, &mut output);

        assert_eq!(output[0], 4096, "shift=12 on nibble value 1 with no filter must give ((1<<12)>>1)*2 = 4096");
    }

    // ========================================================================
    // Regression tests for the six fixes below (stereo output, ENDX, MOV
    // dp,dp's P-flag handling, KON/KOFF trigger cleanup, GAIN-mode sustain
    // transition, and Spc700::reset's timer/DSP-latch clearing).
    // ========================================================================

    #[test]
    fn apu_sample_stereo_preserves_independent_left_right_panning_that_mono_sample_averages_away() {
        // Regression guard for fix #1: `Apu::sample()` (mono) collapses the
        // DSP's real independent per-voice-panned stereo output into a
        // single averaged value, which is fine for that accessor's own
        // documented purpose, but `Apu::sample_stereo()` -- what the
        // frontend's `Snes::get_audio_samples` now uses -- must keep left
        // and right genuinely distinct rather than silently averaging too.
        // A voice panned hard left (VOL(R) = 0) must produce zero energy
        // on the right channel while still producing real energy on the
        // left, which an averaged mono value could never reveal.
        let ram = ram_with_ramp_sample();
        let mut dsp = Dsp::new();
        dsp.write_reg(0x5D, 0x02); // DIR = page 2
        dsp.write_reg(0x00, 0x7F); // VOL(L) = max
        dsp.write_reg(0x01, 0x00); // VOL(R) = zero -- hard left pan
        dsp.write_reg(0x02, 0x00);
        dsp.write_reg(0x03, 0x10); // pitch 0x1000 (native rate)
        dsp.write_reg(0x04, 0x00); // SRCN 0
        dsp.write_reg(0x05, 0x8F); // ADSR1: enabled, fastest attack
        dsp.write_reg(0x06, 0xE0); // ADSR2: max sustain level
        dsp.write_reg(0x0C, 0x7F); // MVOLL
        dsp.write_reg(0x1C, 0x7F); // MVOLR
        dsp.write_reg(0x4C, 0x01); // KON voice 0

        let mut saw_nonzero_left = false;
        let mut saw_nonzero_right = false;
        for _ in 0..500 {
            let (l, r) = dsp.sample(&ram);
            if l != 0 {
                saw_nonzero_left = true;
            }
            if r != 0 {
                saw_nonzero_right = true;
            }
        }

        assert!(saw_nonzero_left, "a hard-left-panned voice must still produce left-channel energy");
        assert!(!saw_nonzero_right, "a hard-left-panned voice (VOL(R)=0) must produce exactly zero right-channel energy");

        // Now confirm the APU-level stereo accessor actually surfaces this
        // real separation end-to-end (through `Apu::sample_stereo`, not
        // just the `Dsp::sample` it wraps), while the mono accessor
        // predictably destroys it by averaging.
        let mut apu = Apu::new();
        // `Apu::new()` owns its own separate RAM (not the local `ram`
        // built above for the `Dsp`-only half of this test) -- copy the
        // same ramp-sample bytes into it via the real `write_ram` path.
        for addr in 0..ram.len() {
            if ram[addr] != 0 {
                apu.write_ram(addr as u16, ram[addr]);
            }
        }
        // Directly drive the APU's DSP the same way, bypassing SPC700
        // execution (which isn't needed to exercise the accessor).
        {
            let dsp_lock = apu.dsp.clone();
            let mut d = dsp_lock.lock().unwrap();
            d.write_reg(0x5D, 0x02);
            d.write_reg(0x00, 0x7F);
            d.write_reg(0x01, 0x00);
            d.write_reg(0x02, 0x00);
            d.write_reg(0x03, 0x10);
            d.write_reg(0x04, 0x00);
            d.write_reg(0x05, 0x8F);
            d.write_reg(0x06, 0xE0);
            d.write_reg(0x0C, 0x7F);
            d.write_reg(0x1C, 0x7F);
            d.write_reg(0x4C, 0x01);
        }
        apu.sample_buffer.clear();
        {
            let ram = apu.ram.clone();
            let ram = ram.lock().unwrap();
            let mut d = apu.dsp.lock().unwrap();
            for _ in 0..500 {
                let (l, r) = d.sample(&ram);
                apu.sample_buffer.push_back((l, r));
            }
        }

        let mut stereo_saw_nonzero_left = false;
        let mut stereo_saw_nonzero_right = false;
        let mut mono_all_matched_left_when_right_was_zero = true;
        while let Some((l, r)) = apu.sample_stereo() {
            if l != 0 {
                stereo_saw_nonzero_left = true;
            }
            if r != 0 {
                stereo_saw_nonzero_right = true;
                mono_all_matched_left_when_right_was_zero = false;
            }
        }
        assert!(stereo_saw_nonzero_left, "sample_stereo must surface real left-channel energy");
        assert!(!stereo_saw_nonzero_right, "sample_stereo must keep the right channel silent for a hard-left pan");
        assert!(mono_all_matched_left_when_right_was_zero, "sanity: right channel must have stayed zero throughout");
    }

    #[test]
    fn endx_sets_the_bit_on_natural_brr_end_and_on_koff_and_clears_on_any_write() {
        // Regression guard for fix #2: real hardware's ENDX ($7C) latches
        // a per-voice bit when that voice's BRR playback hits a block with
        // the "end" header bit set (natural sample end), or when the voice
        // is force-terminated by KOFF -- and a CPU write of ANY value to
        // $7C clears every bit, rather than storing the written value.
        let ram = ram_with_ramp_sample();
        let mut dsp = Dsp::new();
        configure_ramp_voice(&mut dsp, 0x1000);

        assert_eq!(dsp.read_reg(0x7C), 0x00, "ENDX must start clear");

        // The configured sample at $0300 has end+loop set on its single
        // block, so voice 0 must set ENDX bit 0 the first time it wraps
        // around (after 16 native-rate samples).
        for _ in 0..20 {
            dsp.sample(&ram);
        }
        assert_eq!(dsp.read_reg(0x7C) & 0x01, 0x01, "voice 0 must set its ENDX bit on hitting the BRR block's end flag");

        // A write of ANY value (not just 0x00) must clear every bit.
        dsp.write_reg(0x7C, 0xFF);
        assert_eq!(dsp.read_reg(0x7C), 0x00, "writing ENDX (with any value) must clear all bits, not store the written value");

        // KOFF force-termination must also set the voice's ENDX bit, even
        // for a fresh voice that never reached a natural BRR end.
        let mut dsp2 = Dsp::new();
        let ram2 = ram_with_ramp_sample();
        configure_ramp_voice(&mut dsp2, 0x1000);
        dsp2.write_reg(0x7C, 0xFF); // clear any bit set incidentally by configure/KON
        dsp2.sample(&ram2); // one sample so KON's key-on-pending resolves; not enough to hit block end
        dsp2.write_reg(0x5C, 0x01); // KOFF voice 0
        assert_eq!(dsp2.read_reg(0x7C) & 0x01, 0x01, "KOFF must set the force-terminated voice's ENDX bit");

        // Only the targeted voice's bit must be affected -- other voices'
        // bits must stay clear.
        assert_eq!(dsp2.read_reg(0x7C) & !0x01, 0, "KOFF on voice 0 must not set other voices' ENDX bits");
    }

    #[test]
    fn endx_bit_clears_on_key_on() {
        // Real hardware also clears a voice's own ENDX bit when it is
        // freshly key-on'd (a new note shouldn't immediately report the
        // previous note's end-of-sample status).
        let ram = ram_with_ramp_sample();
        let mut dsp = Dsp::new();
        configure_ramp_voice(&mut dsp, 0x1000);
        for _ in 0..20 {
            dsp.sample(&ram);
        }
        assert_eq!(dsp.read_reg(0x7C) & 0x01, 0x01, "must have set ENDX from the natural block end");

        // Key the voice back on -- ENDX for voice 0 must clear immediately
        // (before any further natural end is hit).
        dsp.write_reg(0x4C, 0x01);
        assert_eq!(dsp.read_reg(0x7C) & 0x01, 0x00, "KON must clear the voice's own ENDX bit");
    }

    #[test]
    fn mov_dp_dp_respects_the_p_flag_for_both_source_and_destination() {
        // Regression guard for fix #3: opcode 0xFA (MOV dp,dp) used to
        // cast its two direct-page operand bytes straight to u16 and
        // access page $00xx unconditionally, ignoring PSW.P -- unlike
        // every sibling direct-page opcode (see `dp_addr`'s doc comment).
        // With P set (SETP), both the source and destination must resolve
        // to page $01xx instead of $00xx.
        let mut spc = isolated_spc700();

        // Program: SETP; MOV $10,$20  (0xFA fetches src then dst, per the
        // existing implementation's own operand order).
        spc.write_mem(0x0200, 0x40); // SETP
        spc.write_mem(0x0201, 0xFA); // MOV dp,dp
        spc.write_mem(0x0202, 0x20); // src dp = $20
        spc.write_mem(0x0203, 0x10); // dst dp = $10

        // Seed page $01 (P=1 effective addresses) with a distinct value at
        // the source, and page $00 with a different value at the same
        // nominal offset, so the test fails loudly if P is ignored.
        spc.write_mem(0x0120, 0x77); // real source once P=1: $0120
        spc.write_mem(0x0020, 0x99); // decoy: what a P-ignoring read would see

        spc.pc = 0x0200;
        spc.step(); // SETP
        assert!(spc.psw.p, "SETP must have set P");
        spc.step(); // MOV dp,dp

        assert_eq!(spc.halted, None);
        assert_eq!(spc.read_mem(0x0110), 0x77, "with P=1, the destination must land at $0110 (page $01xx), carrying the value read from the real ($0120) source");
        assert_eq!(spc.read_mem(0x0010), 0x00, "page $00xx's destination slot must be untouched when P=1");
    }

    #[test]
    fn mov_dp_dp_still_uses_page_zero_when_p_is_clear() {
        // Complementary case: with P clear (the default), behavior must be
        // unchanged from before -- both operands resolve to page $00xx.
        let mut spc = isolated_spc700();
        spc.write_mem(0x0200, 0xFA); // MOV dp,dp (P clear by default)
        spc.write_mem(0x0201, 0x20); // src dp = $20
        spc.write_mem(0x0202, 0x10); // dst dp = $10
        spc.write_mem(0x0020, 0x55);

        spc.pc = 0x0200;
        assert!(!spc.psw.p);
        spc.step();

        assert_eq!(spc.halted, None);
        assert_eq!(spc.read_mem(0x0010), 0x55);
    }

    #[test]
    fn kon_koff_apply_immediately_without_relying_on_removed_trigger_fields() {
        // Regression guard for fix #4: `Dsp` used to carry `key_on_trigger`
        // (written by the KON handler but never read anywhere) and
        // `key_off_trigger` (declared and reset, but never actually
        // written by the KOFF handler at all) -- dead state that mirrored,
        // but never gated or deferred, what `write_reg` already applies
        // directly and immediately to each `Voice` via `key_on()`/
        // `key_off()`. This confirms KON/KOFF still take effect the same
        // real way (immediately, per-bit) now that those vestigial fields
        // are gone: a KON bit must activate its voice's envelope in the
        // very next `sample()` call, and a KOFF bit must move it to
        // Release the same way.
        let ram = ram_with_ramp_sample();
        let mut dsp = Dsp::new();
        configure_ramp_voice(&mut dsp, 0x1000);

        assert!(dsp.voices[0].active, "KON must have activated voice 0 immediately");
        assert_eq!(dsp.voices[0].adsr.mode, EnvMode::Attack, "a freshly key-on'd voice must be in Attack");

        dsp.sample(&ram);

        dsp.write_reg(0x5C, 0x01); // KOFF voice 0
        assert_eq!(dsp.voices[0].adsr.mode, EnvMode::Release, "KOFF must move the voice to Release immediately");

        // Only the targeted voice must be affected.
        for i in 1..8 {
            assert!(!dsp.voices[i].active, "KON was never issued for voice {}, it must stay inactive", i);
        }
    }

    #[test]
    fn gain_mode_sustain_transition_is_not_spuriously_triggered_by_gain_bits() {
        // Regression guard for fix #5: the decay->sustain transition check
        // must only apply while the voice is actually running the ADSR
        // state machine (ADSR1 bit7 set). A voice using GAIN mode's
        // increase submode (mode 6/7) can still have its internal `mode`
        // read `Decay` (the attack->decay clamp transition is unconditional
        // on ADSR/GAIN -- see `run`'s doc comment), and in that state,
        // `env_data` legitimately holds the GAIN byte, not an ADSR sustain
        // level. Force that exact state and confirm the voice does NOT
        // spuriously flip to Sustain purely because the envelope's top
        // bits happened to numerically match the GAIN byte's mode-select
        // bits -- it must keep behaving as a GAIN-mode envelope (able to
        // freely exceed what would have been a "sustain" plateau in ADSR
        // mode) since GAIN mode has no sustain-level concept at all.
        let mut adsr = Adsr::new();
        adsr.key_on();
        assert_eq!(adsr.mode, EnvMode::Attack);

        // GAIN byte: mode=7 (111, "bent line" increase) with a fast
        // nonzero rate (0x1F, the fastest) so `self.level` actually
        // advances each commit instead of a rate=0 field (which
        // `read_counter` never fires for -- see `COUNTER_RATES[0]`) --
        // gain>>5 == 7, so env_data>>5 == 7 once in GAIN mode.
        let gain: u8 = 0xFF; // mode=7 (0xFF >> 5 == 0b111), rate=0x1F
        let adsr1: u8 = 0x00; // ADSR1 bit7 clear -> GAIN mode
        let adsr2: u8 = 0x00; // irrelevant in GAIN mode

        let mut counter: i32 = 0;
        let mut reached_decay = false;
        for _ in 0..SIMPLE_COUNTER_RANGE {
            counter -= 1;
            if counter < 0 {
                counter = SIMPLE_COUNTER_RANGE as i32 - 1;
            }
            adsr.run(adsr1, adsr2, gain, counter as u32);
            if adsr.mode == EnvMode::Decay {
                reached_decay = true;
            }
            if adsr.mode == EnvMode::Sustain {
                break;
            }
        }

        assert!(reached_decay, "GAIN mode's increase submode must still be able to overflow past 0x7FF and trip the (ADSR/GAIN-agnostic) attack->decay clamp, reaching Decay");
        assert_ne!(adsr.mode, EnvMode::Sustain, "a GAIN-mode voice must never transition to Sustain -- that is an ADSR-only concept, and env_data>>5 in GAIN mode is the GAIN mode-select field, not a sustain level");
    }

    #[test]
    fn spc700_reset_clears_timers_and_dsp_address_latch() {
        // Regression guard for fix #6: `Spc700::reset()` restored
        // registers/PC/PSW but left the timer hardware (enable bits,
        // targets, dividers, output counters) and the $F2 DSP-register-
        // address latch at their pre-reset values -- real hardware zeroes
        // both on reset.
        let mut spc = isolated_spc700();

        // Arm all three timers with nonzero targets and let them run long
        // enough to accumulate nonzero divider/counter state.
        spc.write_mem(0xFA, 0x01); // timer 0 target
        spc.write_mem(0xFB, 0x01); // timer 1 target
        spc.write_mem(0xFC, 0x01); // timer 2 target
        spc.write_mem(0xF1, 0x07); // enable all three timers

        for _ in 0..2000 {
            spc.tick_timers();
        }
        // Sanity: at least the fast timer 2 (8x prescaler) must have
        // produced a nonzero readable counter by now.
        assert_ne!(spc.read_mem(0xFF), 0, "timer 2's counter must have advanced before reset (sanity check)");

        // Re-arm afterward since reading $FD-$FF above resets that
        // specific counter to 0 as a side effect -- set the DSP address
        // latch and re-enable timers with fresh nonzero state to confirm
        // reset (not an incidental read) is what clears them.
        spc.write_mem(0xF2, 0x0C); // select DSP register $0C (MVOLL)
        assert_eq!(spc.read_mem(0xF2), 0x0C, "sanity: latch must hold what was just written");
        spc.write_mem(0xF1, 0x07);
        for _ in 0..2000 {
            spc.tick_timers();
        }

        spc.reset();

        assert_eq!(spc.read_mem(0xF1), 0x00, "reset must clear the timer control byte");
        assert_eq!(spc.read_mem(0xFD), 0x00, "reset must clear timer 0's output counter");
        assert_eq!(spc.read_mem(0xFE), 0x00, "reset must clear timer 1's output counter");
        assert_eq!(spc.read_mem(0xFF), 0x00, "reset must clear timer 2's output counter");
        assert_eq!(spc.read_mem(0xF2), 0x00, "reset must clear the DSP register-address latch");

        // And timers must stay at zero afterward (disabled), not silently
        // resume ticking from leftover prescaler/divider state.
        for _ in 0..2000 {
            spc.tick_timers();
        }
        assert_eq!(spc.read_mem(0xFF), 0x00, "a disabled-by-reset timer must not resume advancing on its own");
    }

    #[test]
    fn dsp_save_state_round_trips_mid_note_transient_state() {
        // A voice paused mid-envelope, mid-BRR-block, with a live echo
        // ring must come back bit-for-bit -- save states used to reset
        // this transient state (voices restarted silent until re-keyed).
        let mut dsp = Dsp::new();
        dsp.regs[0x0C] = 0x7F; // MVOLL
        dsp.voices[3].active = true;
        dsp.voices[3].adsr.level = 0x345;
        dsp.voices[3].adsr.mode = EnvMode::Decay;
        dsp.voices[3].adsr.hidden = 0x612;
        dsp.voices[3].brr_addr = 0x4321;
        dsp.voices[3].loop_addr = 0x4300;
        dsp.voices[3].brr_position = 11;
        dsp.voices[3].brr_buffer[11] = -12345;
        dsp.voices[3].decoded_addr = 0x4321;
        dsp.voices[3].decoded_valid = true;
        dsp.voices[3].pitch_counter = 0x0ABC;
        dsp.voices[3].hist = [-5, 17, -300, 4000];
        dsp.voices[3].brr.history = [-100, 250];
        dsp.voices[3].endx = true;
        dsp.echo_ring = vec![(7, -9); 1024];
        dsp.echo_pos = 513;
        dsp.fir_hist[2] = (55, -66);
        dsp.fir_pos = 5;
        dsp.counter = 0x1234;

        let mut buf = Vec::new();
        dsp.save_state(&mut buf);
        let mut restored = Dsp::new();
        restored.load_state(&mut crate::state::StateReader::new(&buf)).unwrap();

        assert_eq!(restored.regs[0x0C], 0x7F);
        let v = &restored.voices[3];
        assert!(v.active);
        assert_eq!(v.adsr.level, 0x345, "envelope level must survive");
        assert_eq!(v.adsr.mode, EnvMode::Decay, "envelope phase must survive");
        assert_eq!(v.adsr.hidden, 0x612);
        assert_eq!(v.brr_addr, 0x4321);
        assert_eq!(v.loop_addr, 0x4300);
        assert_eq!(v.brr_position, 11);
        assert_eq!(v.brr_buffer[11], -12345, "decoded BRR samples must survive");
        assert!(v.decoded_valid);
        assert_eq!(v.pitch_counter, 0x0ABC);
        assert_eq!(v.hist, [-5, 17, -300, 4000], "resampler history must survive");
        assert_eq!(v.brr.history, [-100, 250], "BRR filter history must survive");
        assert!(v.endx);
        assert_eq!(restored.echo_ring.len(), 1024, "echo ring length must survive");
        assert_eq!(restored.echo_ring[100], (7, -9));
        assert_eq!(restored.echo_pos, 513);
        assert_eq!(restored.fir_hist[2], (55, -66));
        assert_eq!(restored.fir_pos, 5);
        assert_eq!(restored.counter, 0x1234);
    }

    #[test]
    fn every_spc700_opcode_executes_without_halting_except_stop() {
        // Full-instruction-set coverage pin: with all-zero RAM (so every
        // operand byte is 0x00), stepping a fresh SPC700 onto each of the
        // 256 opcodes must execute it -- the only opcode allowed to set
        // `halted` is 0xFF (STOP), which genuinely halts real hardware.
        // If any dispatch arm (or guard predicate) is removed or broken,
        // that opcode falls through to the defensive halt arm and this
        // test names it exactly.
        for opcode in 0..=255u8 {
            let mut spc = isolated_spc700();
            spc.write_mem(0x0200, opcode);
            spc.pc = 0x0200;
            spc.step();
            if opcode == 0xFF {
                assert_eq!(spc.halted, Some(0xFF), "STOP must halt");
            } else {
                assert_eq!(
                    spc.halted, None,
                    "opcode 0x{:02X} must execute without halting",
                    opcode
                );
            }
        }
    }

    #[test]
    fn odd_numbered_tcalls_jump_through_their_descending_vectors() {
        // Regression guard: the TCALL guard used to match `& 0x1F == 0x01`,
        // which silently missed TCALL 1/3/5/7/9/11/13/15 (opcodes
        // $11/$31/.../$F1). TCALL 1's vector is $FFDC ($FFDE - 2*1).
        let mut spc = isolated_spc700();
        spc.write_mem(0xFFDC, 0x34);
        spc.write_mem(0xFFDD, 0x12);
        spc.write_mem(0x0200, 0x11); // TCALL 1
        spc.pc = 0x0200;
        spc.sp = 0xFF;
        spc.step();
        assert_eq!(spc.halted, None);
        assert_eq!(spc.pc, 0x1234, "TCALL 1 must jump through the $FFDC vector");
        // Return address ($0201) pushed high-then-low.
        assert_eq!(spc.read_mem(0x01FF), 0x02);
        assert_eq!(spc.read_mem(0x01FE), 0x01);
    }

    #[test]
    fn alu_dp_dp_and_dp_imm_and_ix_iy_store_results_with_flags() {
        // ADC dp,dp: src-then-dst operand order, result to dst.
        let mut spc = isolated_spc700();
        spc.write_mem(0x0010, 0x22); // src
        spc.write_mem(0x0011, 0x33); // dst
        spc.write_mem(0x0200, 0x89); // ADC dp,dp
        spc.write_mem(0x0201, 0x10); // src dp
        spc.write_mem(0x0202, 0x11); // dst dp
        spc.pc = 0x0200;
        spc.psw.c = false;
        spc.step();
        assert_eq!(spc.read_mem(0x0011), 0x55, "ADC dp,dp must store dst+src into dst");
        assert!(!spc.psw.c);

        // OR dp,#imm: imm-then-dp operand order, result to dp.
        let mut spc = isolated_spc700();
        spc.write_mem(0x0020, 0x0F);
        spc.write_mem(0x0200, 0x18); // OR dp,#imm
        spc.write_mem(0x0201, 0xF0); // imm
        spc.write_mem(0x0202, 0x20); // dp
        spc.pc = 0x0200;
        spc.step();
        assert_eq!(spc.read_mem(0x0020), 0xFF);
        assert!(spc.psw.n, "N must reflect the stored result");

        // CMP (X),(Y): flags only, no store.
        let mut spc = isolated_spc700();
        spc.write_mem(0x0030, 0x40); // (X)
        spc.write_mem(0x0031, 0x50); // (Y)
        spc.write_mem(0x0200, 0x79); // CMP (X),(Y)
        spc.pc = 0x0200;
        spc.x = 0x30;
        spc.y = 0x31;
        spc.step();
        assert_eq!(spc.read_mem(0x0030), 0x40, "CMP must not store");
        assert!(!spc.psw.c, "0x40 < 0x50 must clear carry (borrow needed)");

        // SBC (X),(Y): result stored through (X).
        let mut spc = isolated_spc700();
        spc.write_mem(0x0030, 0x50);
        spc.write_mem(0x0031, 0x20);
        spc.write_mem(0x0200, 0xB9); // SBC (X),(Y)
        spc.pc = 0x0200;
        spc.x = 0x30;
        spc.y = 0x31;
        spc.psw.c = true; // no incoming borrow
        spc.step();
        assert_eq!(spc.read_mem(0x0030), 0x30, "SBC (X),(Y) must store dst-src into (X)");
        assert!(spc.psw.c, "no borrow must leave carry set");
    }

    #[test]
    fn carry_bit_instructions_use_13_bit_address_and_3_bit_bit_operand() {
        // MOV1 C, m.b: address $0123, bit 5 -> operand word $0123 | (5<<13).
        let mut spc = isolated_spc700();
        spc.write_mem(0x0123, 1 << 5);
        let operand: u16 = 0x0123 | (5 << 13);
        spc.write_mem(0x0200, 0xAA); // MOV1 C,m.b
        spc.write_mem(0x0201, (operand & 0xFF) as u8);
        spc.write_mem(0x0202, (operand >> 8) as u8);
        spc.pc = 0x0200;
        spc.psw.c = false;
        spc.step();
        assert!(spc.psw.c, "MOV1 C,m.b must load the addressed bit into carry");

        // MOV1 m.b, C writes the carry back into the addressed bit.
        let mut spc = isolated_spc700();
        let operand: u16 = 0x0040 | (3 << 13);
        spc.write_mem(0x0200, 0xCA); // MOV1 m.b,C
        spc.write_mem(0x0201, (operand & 0xFF) as u8);
        spc.write_mem(0x0202, (operand >> 8) as u8);
        spc.pc = 0x0200;
        spc.psw.c = true;
        spc.step();
        assert_eq!(spc.read_mem(0x0040), 1 << 3, "MOV1 m.b,C must set exactly the addressed bit");

        // NOT1 m.b toggles the addressed bit in place.
        let mut spc = isolated_spc700();
        spc.write_mem(0x0055, 0xFF);
        let operand: u16 = 0x0055 | (7 << 13);
        spc.write_mem(0x0200, 0xEA); // NOT1 m.b
        spc.write_mem(0x0201, (operand & 0xFF) as u8);
        spc.write_mem(0x0202, (operand >> 8) as u8);
        spc.pc = 0x0200;
        spc.step();
        assert_eq!(spc.read_mem(0x0055), 0x7F, "NOT1 must flip only the addressed bit");
    }

    #[test]
    fn daa_and_das_decimal_adjust_the_accumulator() {
        // 0x19 + 0x28 = 0x41 binary; DAA must correct it to 0x47 (19+28=47 BCD).
        let mut spc = isolated_spc700();
        spc.a = 0x19;
        spc.write_mem(0x0200, 0x88); // ADC A,#imm
        spc.write_mem(0x0201, 0x28);
        spc.write_mem(0x0202, 0xDF); // DAA A
        spc.pc = 0x0200;
        spc.psw.c = false;
        spc.step();
        assert_eq!(spc.a, 0x41, "sanity: binary add result before adjustment");
        assert!(spc.psw.h, "half-carry from bit 3 (9+8=17) must set H");
        spc.step();
        assert_eq!(spc.a, 0x47, "DAA must produce the BCD sum 47");

        // 0x42 - 0x15 = 0x2D binary; DAS must correct it to 0x27 (42-15=27 BCD).
        let mut spc = isolated_spc700();
        spc.a = 0x42;
        spc.write_mem(0x0200, 0xA8); // SBC A,#imm
        spc.write_mem(0x0201, 0x15);
        spc.write_mem(0x0202, 0xBE); // DAS A
        spc.pc = 0x0200;
        spc.psw.c = true; // no incoming borrow
        spc.step();
        assert_eq!(spc.a, 0x2D, "sanity: binary subtract result before adjustment");
        spc.step();
        assert_eq!(spc.a, 0x27, "DAS must produce the BCD difference 27");
    }

    #[test]
    fn mov_indirect_dp_x_store_writes_through_the_pointer() {
        // MOV [dp+X],A (0xC7): pointer at dp+X, store A through it.
        let mut spc = isolated_spc700();
        spc.write_mem(0x0014, 0x00); // pointer low  (at $10 + X=4)
        spc.write_mem(0x0015, 0x03); // pointer high -> $0300
        spc.write_mem(0x0200, 0xC7);
        spc.write_mem(0x0201, 0x10);
        spc.pc = 0x0200;
        spc.x = 0x04;
        spc.a = 0x99;
        spc.step();
        assert_eq!(spc.read_mem(0x0300), 0x99, "MOV [dp+X],A must store through the pointer at dp+X");
    }

    #[test]
    fn brk_pushes_pc_and_psw_then_jumps_through_ffde() {
        let mut spc = isolated_spc700();
        spc.write_mem(0xFFDE, 0x00);
        spc.write_mem(0xFFDF, 0x40); // vector -> $4000
        spc.write_mem(0x0200, 0x0F); // BRK
        spc.pc = 0x0200;
        spc.sp = 0xFF;
        spc.psw.i = true;
        spc.step();
        assert_eq!(spc.pc, 0x4000, "BRK must jump through the $FFDE vector");
        assert!(spc.psw.b, "BRK must set the Break flag");
        assert!(!spc.psw.i, "BRK must clear the Interrupt-enable flag");
        assert_eq!(spc.read_mem(0x01FF), 0x02, "pushed return PC high byte");
        assert_eq!(spc.read_mem(0x01FE), 0x01, "pushed return PC low byte");
        // Pushed PSW: I was set, B not yet set at push time.
        let pushed_psw = spc.read_mem(0x01FD);
        assert_ne!(pushed_psw & 0x04, 0, "pushed PSW must have I as it was before BRK");
    }
}
