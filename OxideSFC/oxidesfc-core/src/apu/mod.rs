//! APU (Audio Processing Unit) module for OxideSFC SNES emulator.
//!
//! The APU is an "independent console within the SNES" consisting of:
//! - Sony SPC700: 8-bit audio CPU (similar to 6502)
//! - DSP: 8 voice channels with BRR compression, ADSR envelopes, echo
//! - 64KB APU RAM: Isolated memory for SPC700 program and audio data
//! - Communication Ports: 4 ports ($2140-$2143) for communication with main CPU
//!
//! Layout: `spc700` and its `opcodes` dispatch are the audio CPU; `dsp`
//! drives the eight `voice`s, each decoding `brr` blocks through an
//! `envelope`. This module owns `Apu` itself -- the piece the main CPU sees
//! -- which holds the shared RAM/ports/DSP handles and converts the bus's
//! pacing cycles into SPC700 cycles and DSP samples.

mod brr;
mod dsp;
mod envelope;
mod opcodes;
mod spc700;
mod voice;

#[cfg(test)]
mod tests;

use dsp::Dsp;
use envelope::SIMPLE_COUNTER_RANGE;
use spc700::{Psw, Spc700};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

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

/// Saturate to the signed 16-bit range, the DSP's native accumulator
/// width. Real hardware saturates after *each* accumulation step, not only
/// at the end of the mix -- see `Dsp::sample`.
#[inline]
fn clamp16(v: i32) -> i32 {
    v.clamp(-32768, 32767)
}

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

    /// Fractional SPC700-step remainder carried between `tick()` calls,
    /// scaled by the exact clock-ratio denominator (see `tick`). The
    /// SPC700 has its OWN 24.576MHz crystal and steps at 1.024MHz -- it is
    /// not an integer division of the main clock. An earlier version used
    /// `cycles / 3` of the ~2.6847MHz pacing unit (= 894.9kHz), which ran
    /// the SPC700 -- and therefore its 8kHz/64kHz timers, i.e. the music
    /// driver's tempo -- 12.6% slow. Carrying the remainder (rather than
    /// truncating per call) also matters: without it the "wait for SPC
    /// ready" handshake loops spun long enough for a second NMI to fire
    /// inside the previous handler and corrupt the stack.
    spc_cycle_debt: u32,

    /// Unspent SPC700 cycle budget. An instruction can't be split across
    /// `tick()` calls, so whichever instruction runs past the end of a
    /// tick's budget leaves this negative, and the next tick pays it off
    /// before running anything. See `tick`.
    spc_cycle_credit: i64,

    /// The machine's master clock in Hz, used to convert the master/8 pacing
    /// units `tick` receives into real SPC700 cycles. NTSC by default; PAL
    /// machines run a different crystal, so `SystemBus::set_video_mode`
    /// updates this alongside the PPU's mode.
    master_clock_hz: u32,
}

/// NTSC master clock (Hz). 21,477,272 / (341 dots x 4 x 262 lines) = the
/// canonical 60.0988 fps.
pub const NTSC_MASTER_CLOCK_HZ: u32 = 21_477_272;
/// PAL master clock (Hz). 21,281,370 / (341 dots x 4 x 312 lines) = 50.007 fps.
pub const PAL_MASTER_CLOCK_HZ: u32 = 21_281_370;

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
            // One DSP sample every 32 SPC700 cycle-steps: the real DSP is
            // hard-wired to the SPC700's 1.024MHz clock and outputs at
            // exactly 1,024,000 / 32 = 32,000 Hz. This used to be 84 (a
            // "main cycles per sample" calibration against the pacing
            // unit), which produced ~31,866 effective Hz -- close, but the
            // constant 0.4% shortfall versus the frontend's 32kHz playback
            // meant the audio buffer drained slightly faster than it
            // filled, causing a periodic underrun (heard as stutter).
            // (Much earlier it was `3`, which generated samples ~28x too
            // fast.)
            sample_divider: 32,
            sample_counter: 0,
            spc_cycle_debt: 0,
            spc_cycle_credit: 0,
            master_clock_hz: NTSC_MASTER_CLOCK_HZ,
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

    /// Sets the machine's master clock, which `tick` uses to convert the
    /// master/8 pacing units it receives into real SPC700 cycles. Use
    /// `NTSC_MASTER_CLOCK_HZ` / `PAL_MASTER_CLOCK_HZ`; `SystemBus::set_video_mode`
    /// keeps this in step with the PPU's video standard.
    pub fn set_master_clock_hz(&mut self, hz: u32) {
        if hz > 0 {
            self.master_clock_hz = hz;
        }
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

        // Convert pacing-unit cycles (master/8 = 21,477,272/8 = 2,684,659
        // Hz -- the unit `SystemBus::tick_master` feeds this in) into
        // SPC700 cycle-steps at the SPC700's true 1.024MHz rate (its own
        // 24.576MHz crystal / 24), carrying the exact fractional
        // remainder across calls. The intermediate product is computed in
        // u64 because a large DMA can tick tens of thousands of unit
        // cycles at once.
        // Expressed against the machine's master clock rather than a
        // hardcoded NTSC-derived unit rate: `cycles` counts master/8 units,
        // so one unit is `8 / master_clock_hz` seconds and the SPC700 advances
        // `cycles * 8 * 1_024_000 / master_clock_hz` of its own cycles. With
        // the NTSC master clock this is exactly the old 2,684,659-unit
        // divisor; with PAL's 21,281,370 Hz clock the unit is 0.9% shorter,
        // which the old constant would have turned into a 0.9% slow SPC700 --
        // and therefore a 31,708 Hz sample stream feeding a 32,000 Hz player,
        // a drift too large for the frontend's +/-0.5% rate control to absorb.
        let debt = self.spc_cycle_debt as u64 + cycles as u64 * 8 * 1_024_000;
        let master = self.master_clock_hz as u64;
        let spc_cycles = (debt / master) as u32;
        self.spc_cycle_debt = (debt % master) as u32;

        // Spend that cycle budget on instructions, charging each one its
        // real cost. This used to run one *instruction* per SPC700 cycle and
        // throw the returned cycle count away, so the SPC700 executed at
        // roughly 3.5x its real throughput (the average instruction is ~3.5
        // cycles). Tempo and sample rate still came out right -- the timers
        // and the DSP divider were both calibrated in those instruction
        // units -- but any driver whose timing depends on how much work fits
        // between two timer ticks (streaming engines, tight IPC handshakes)
        // saw a machine 3.5x faster than hardware.
        //
        // An instruction can't be split across calls, so the overshoot is
        // carried as a negative credit and paid off by the next tick.
        self.spc_cycle_credit += spc_cycles as i64;
        while self.spc_cycle_credit > 0 {
            let used = self.spc700.step().max(1);
            for _ in 0..used {
                self.spc700.tick_timers();
            }
            self.spc_cycle_credit -= used as i64;
        }

        // The DSP is clocked by the SPC700: one stereo sample per 32
        // SPC700 cycles = exactly 32,000 samples per emulated second.
        self.sample_counter += spc_cycles;
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
        // Never positive (see the field), so it round-trips as a magnitude.
        put_u32(out, (-self.spc_cycle_credit).clamp(0, u32::MAX as i64) as u32);
        put_u32(out, self.master_clock_hz);
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
        self.spc_cycle_credit = -(r.u32()? as i64);
        // A zero here would divide by zero in `tick`.
        self.master_clock_hz = match r.u32()? {
            0 => NTSC_MASTER_CLOCK_HZ,
            hz => hz,
        };
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
    
    /// The SPC700, for this module's tests to inspect execution state.
    /// Scoped to `crate::apu` (not the crate's public API) because `Spc700`
    /// is internal to this module, and `#[cfg(test)]` because nothing in a
    /// release build has any business reaching in here.
    #[cfg(test)]
    pub(in crate::apu) fn spc700(&self) -> &Spc700 {
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
