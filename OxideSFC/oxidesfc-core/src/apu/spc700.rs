//! The Sony SPC700, the APU's 8-bit audio CPU: its registers and PSW
//! flags, the timer hardware, APU-RAM access (including the $F0-$FF I/O
//! page that reaches the DSP and the CPU communication ports), and the
//! instruction fetch/execute loop. The 256-opcode dispatch itself lives in
//! `super::opcodes`.

use super::{ApuPorts, Dsp, TIMER_PRESCALER_DIVISOR};
use std::sync::{Arc, Mutex};

/// SPC700 CPU registers and state
/// The SPC700 is a modified 6502-compatible processor with custom opcodes
pub(super) struct Spc700 {
    /// Accumulator (A register)
    pub(super) a: u8,
    /// X index register
    pub(super) x: u8,
    /// Y index register
    pub(super) y: u8,
    /// Stack pointer (0-255, relative to page 0x01)
    pub(super) sp: u8,
    /// Program counter
    pub(super) pc: u16,
    /// Processor status word
    pub(super) psw: Psw,
    /// Cycles remaining for current instruction
    pub(super) cycles_remaining: u32,
    /// RAM reference for memory operations
    pub(super) ram: Arc<Mutex<[u8; 65536]>>,
    /// The CPU<->APU communication latches ($F4-$F7 on this side,
    /// $2140-$2143 on the main CPU's side), shared with `Apu` so this
    /// really-executing SPC700 code drives the same ports the main CPU
    /// reads/writes -- see `ApuPorts`.
    pub(super) ports: Arc<Mutex<ApuPorts>>,
    /// Set when `step` encounters an opcode outside the validated subset
    /// this decoder implements (see `execute_opcode`'s doc comment). Once
    /// set, `step` stops advancing instead of corrupting state or
    /// panicking on an unknown encoding.
    pub(super) halted: Option<u8>,

    /// Timer hardware ($F1 enable bits, $FA-$FC divisor targets, $FD-$FF
    /// read-only output counters). Verified against
    /// wiki.superfamicom.org/spc700-reference and snesmusic.org's SPC700
    /// docs: timers 0/1 tick their internal stage at 8KHz, timer 2 at
    /// 64KHz; each tick increments an 8-bit divider that resets and bumps
    /// the visible 4-bit counter when it reaches the target. Reading a
    /// counter resets it to 0. This is real, documented SPC700 hardware --
    /// without it, any driver that polls a timer counter to pace itself
    /// (a near-universal pattern) spins forever.
    pub(super) timer_enable: [bool; 3],
    pub(super) timer_target: [u8; 3],
    pub(super) timer_divider: [u8; 3],
    pub(super) timer_counter: [u8; 3],
    /// Stage-1 prescaler accumulator, in SPC700 instruction-steps (not
    /// true elapsed cycles -- ticking once per `step()` call is an
    /// approximation, but sufficient to make timers progress correctly
    /// relative to each other and eventually fire).
    pub(super) timer_prescaler: [u32; 3],
    /// Raw value last written to $F1, for bits other than the timer
    /// enables (kept for completeness/inspection; not otherwise acted on).
    pub(super) control: u8,
    /// The DSP, reached indirectly through $F2 (register-select port,
    /// stored in `dsp_addr`) and $F3 (register data port) -- see
    /// `read_mem`/`write_mem`. Shared with `Apu` (which needs it for
    /// `Apu::tick`'s sample generation and register readback) the same
    /// way `ram`/`ports` are.
    pub(super) dsp: Arc<Mutex<Dsp>>,
    /// Last value written to $F2: which DSP register $F3 currently reads
    /// from / writes to. Real hardware also lets $F2 be read back as a
    /// plain register holding this same value.
    pub(super) dsp_addr: u8,
}

#[derive(Clone, Copy)]
/// Processor Status Word flags
pub(super) struct Psw {
    /// Negative flag
    pub(super) n: bool,
    /// Overflow flag
    pub(super) v: bool,
    /// Polarity flag (SPC700 specific)
    pub(super) p: bool,
    /// Zero flag
    pub(super) z: bool,
    /// Carry flag
    pub(super) c: bool,
    /// Interrupt disable
    pub(super) i: bool,
    /// Half carry flag (BCD operations)
    pub(super) h: bool,
    /// Break flag
    pub(super) b: bool,
}

impl Psw {
    pub(super) fn new() -> Self {
        Psw {
            n: false,
            v: false,
            p: false,
            z: false,
            c: false,
            i: false,
            h: false,
            b: false,
        }
    }

    /// Real SPC700 PSW byte layout (bit7..bit0): `N V P B H I Z C`. `i`
    /// (Interrupt enable) previously read from bit 0x10 -- which is
    /// actually `B`'s (Break) real position -- and `b` was never modeled
    /// at all (`from_byte` always forced it to `false`, discarding
    /// whatever a real PUSH PSW/POP PSW round trip or the SPC700's own
    /// BRK instruction had set). Fixed to read each flag from its real
    /// bit.
    pub(super) fn from_byte(b: u8) -> Self {
        Psw {
            n: (b & 0x80) != 0,
            v: (b & 0x40) != 0,
            p: (b & 0x20) != 0,
            b: (b & 0x10) != 0,
            h: (b & 0x08) != 0,
            i: (b & 0x04) != 0,
            z: (b & 0x02) != 0,
            c: (b & 0x01) != 0,
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
    pub(super) fn to_byte(&self) -> u8 {
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
    pub(super) fn new(ram: Arc<Mutex<[u8; 65536]>>, ports: Arc<Mutex<ApuPorts>>, dsp: Arc<Mutex<Dsp>>) -> Self {
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

    /// Advances the timer hardware by one SPC700 *cycle*. See the `timer_*`
    /// fields' doc comment for the verified behavior being modeled.
    ///
    /// `Apu::tick` calls this once per cycle an executed instruction
    /// consumed. It used to be driven from `step()` instead, i.e. once per
    /// instruction -- which only kept the timers at their real 8kHz/64kHz
    /// rates because the caller also (incorrectly) ran exactly one
    /// instruction per SPC700 cycle.
    pub(super) fn tick_timers(&mut self) {
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
    pub(super) fn reset(&mut self) {
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
    pub(super) fn read_mem(&mut self, addr: u16) -> u8 {
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
    pub(super) fn write_mem(&mut self, addr: u16, value: u8) {
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
    pub(super) fn push_stack(&mut self, value: u8) {
        self.write_mem(0x0100 | (self.sp as u16), value);
        self.sp = self.sp.wrapping_sub(1);
    }

    /// Pop byte from stack
    pub(super) fn pop_stack(&mut self) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        self.read_mem(0x0100 | (self.sp as u16))
    }

    /// Set zero and negative flags based on value
    pub(super) fn set_zn(&mut self, value: u8) {
        self.psw.z = value == 0;
        self.psw.n = (value & 0x80) != 0;
    }

    /// Execute one instruction, returning the number of SPC700 cycles it
    /// consumed. The caller is responsible for advancing the timers by that
    /// many cycles (see `Apu::tick`) -- this used to tick them itself, once
    /// per instruction rather than once per cycle.
    pub(super) fn step(&mut self) -> u32 {
        if self.halted.is_some() {
            return 2;
        }
        let opcode = self.read_mem(self.pc);
        self.pc = self.pc.wrapping_add(1);

        self.execute_opcode(opcode)
    }

    pub(super) fn fetch_u8(&mut self) -> u8 {
        let value = self.read_mem(self.pc);
        self.pc = self.pc.wrapping_add(1);
        value
    }

    pub(super) fn fetch_u16(&mut self) -> u16 {
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
    pub(super) fn adc_generic(&mut self, a: u8, operand: u8) -> u8 {
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
    pub(super) fn sbc_generic(&mut self, a: u8, operand: u8) -> u8 {
        self.adc_generic(a, !operand)
    }

    /// ADC A,operand (8-bit, sets C/V/H/Z/N)
    pub(super) fn adc8(&mut self, operand: u8) {
        self.a = self.adc_generic(self.a, operand);
    }

    /// SBC A,operand (8-bit, sets C/V/H/Z/N)
    pub(super) fn sbc8(&mut self, operand: u8) {
        self.a = self.sbc_generic(self.a, operand);
    }

    /// Fetches the 16-bit `m.b` (absolute-address.bit) operand used by the
    /// carry-bit instructions (OR1/AND1/EOR1/MOV1/NOT1): the low 13 bits
    /// are a plain absolute address, the high 3 bits select the bit.
    pub(super) fn fetch_abs_bit(&mut self) -> (u16, u8) {
        let word = self.fetch_u16();
        (word & 0x1FFF, ((word >> 13) & 0x07) as u8)
    }

    // Shared operand fetchers for the remaining ALU addressing modes
    // (opcode values verified against wiki.superfamicom.org/spc700-reference).
    pub(super) fn operand_indirect_x(&mut self) -> u8 {
        self.read_mem(self.dp_addr(self.x))
    }
    pub(super) fn operand_dp_x(&mut self) -> u8 {
        let dp = self.fetch_u8().wrapping_add(self.x);
        self.read_mem(self.dp_addr(dp))
    }
    pub(super) fn operand_abs_x(&mut self) -> u8 {
        let addr = self.fetch_u16().wrapping_add(self.x as u16);
        self.read_mem(addr)
    }
    pub(super) fn operand_abs_y(&mut self) -> u8 {
        let addr = self.fetch_u16().wrapping_add(self.y as u16);
        self.read_mem(addr)
    }
    pub(super) fn operand_indirect_dp_x(&mut self) -> u8 {
        // [dp+X]
        let dp = self.fetch_u8().wrapping_add(self.x);
        let lo = self.read_mem(self.dp_addr(dp)) as u16;
        let hi = self.read_mem(self.dp_addr(dp.wrapping_add(1))) as u16;
        self.read_mem((hi << 8) | lo)
    }
    pub(super) fn operand_indirect_dp_y(&mut self) -> u8 {
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
    pub(super) fn dp_addr(&self, dp: u8) -> u16 {
        (dp as u16) | if self.psw.p { 0x100 } else { 0 }
    }

    /// CMP-style comparison: sets N/Z/C as if computing `a - b` (unsigned,
    /// carry set when no borrow is needed i.e. a >= b) without storing the
    /// result -- both operands are left unchanged.
    pub(super) fn cmp8(&mut self, a: u8, b: u8) {
        let result = a.wrapping_sub(b);
        self.psw.z = result == 0;
        self.psw.n = (result & 0x80) != 0;
        self.psw.c = a >= b;
    }
}
