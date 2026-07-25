//! The DSP: its 128-byte register file, the 8 voices, the noise LFSR, the
//! echo delay line with its 8-tap FIR, and the stereo mix.

use super::clamp16;
use super::envelope::{read_counter, EnvMode};
use super::voice::{Voice, VoiceConfig};
use super::SIMPLE_COUNTER_RANGE;

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
    pub(super) echo_ring: Vec<(i32, i32)>,
    pub(super) echo_pos: usize,
    /// The last 8 samples read out of the delay line (the FIR filter's
    /// input window -- 8 CONSECUTIVE samples, newest at `fir_pos`).
    pub(super) fir_hist: [(i32, i32); 8],
    pub(super) fir_pos: usize,
    
    /// Output mix
    pub(super) output_left: i16,
    pub(super) output_right: i16,

    /// Global envelope timing counter, decremented once per generated
    /// sample and wrapped at `SIMPLE_COUNTER_RANGE`. `read_counter` uses
    /// it to decide, per rate, which samples an envelope advances on --
    /// see `Adsr::run`.
    pub(super) counter: i32,

    /// The DSP's shared 15-bit noise LFSR, clocked at the rate in FLG
    /// ($6C) bits 0-4 and used as the sample source by every voice whose
    /// NON ($3D) bit is set. Previously absent entirely: NON-flagged voices
    /// played their (usually meaningless) BRR sample instead, so
    /// percussion -- hi-hats, snares, cymbals -- and noise-based SFX (wind,
    /// rain, explosions, engine hum) were wrong or silent in a large part
    /// of the library. Seeded to 0x4000 like hardware after reset.
    pub(super) noise: i32,
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
            noise: 0x4000,
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
        self.noise = 0x4000;
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
        put_i32(out, self.noise);
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
        // `sample()` indexes the ring unconditionally, so a zero-length
        // ring (only reachable from a corrupt state) must not survive.
        if self.echo_ring.is_empty() {
            self.echo_ring.push((0, 0));
        }
        self.echo_pos = (r.u32()? as usize).min(self.echo_ring.len() - 1);
        for h in self.fir_hist.iter_mut() {
            *h = (r.i32()?, r.i32()?);
        }
        self.fir_pos = (r.u32()? as usize) % 8;
        self.output_left = r.u16()? as i16;
        self.output_right = r.u16()? as i16;
        self.counter = r.i32()?;
        self.noise = r.i32()?;
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
            0x6C => {
                // FLG bit 7 is a DSP soft reset: hardware key-offs every
                // voice and re-seeds the noise LFSR. Sound drivers set it
                // while swapping sample banks, and without this the old
                // voices kept playing against the new bank's data.
                if value & 0x80 != 0 {
                    for voice in &mut self.voices {
                        voice.key_off();
                        voice.adsr.level = 0;
                    }
                    self.noise = 0x4000;
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
        let flg = self.regs[0x6C];

        // Clock the shared noise LFSR at the rate in FLG bits 0-4, using
        // the same per-rate counter gating as the envelopes (bsnes
        // `SPC_DSP::run_counters` / the noise block in `echo_22`).
        if read_counter(counter, (flg & 0x1F) as usize) == 0 {
            let feedback = (self.noise << 13) ^ (self.noise << 14);
            self.noise = (feedback & 0x4000) ^ (self.noise >> 1);
        }

        // Mix all voices, reading each one's live configuration from its
        // register block ($n0-$n7). Voices whose EON ($4D) bit is set also
        // feed the echo input -- ONLY those (real hardware; previously
        // every voice went into the echo unconditionally).
        let eon = self.regs[0x4D];
        let non = self.regs[0x3D];
        let pmon = self.regs[0x2D];
        let noise = self.noise;
        let mut mix_left: i32 = 0;
        let mut mix_right: i32 = 0;
        let mut echo_in_left: i32 = 0;
        let mut echo_in_right: i32 = 0;
        // Previous voice's post-envelope output, which pitch modulation
        // (PMON, $2D) feeds forward into the next voice's pitch. Voice 0
        // has no predecessor, so PMON bit 0 is unused on hardware.
        let mut prev_enveloped: i32 = 0;

        for i in 0..self.voices.len() {
            let base = i * 0x10;
            let vol_l = self.regs[base] as i8;
            let vol_r = self.regs[base + 1] as i8;
            let mut pitch =
                ((self.regs[base + 2] as u16) | ((self.regs[base + 3] as u16) << 8)) & 0x3FFF;
            // PMON: modulate this voice's pitch by the previous voice's
            // output (bsnes `voice_V3b`). Clamped into the 14-bit pitch
            // range so a large negative modulation can't run the read
            // position backwards through our unsigned accumulator.
            if i > 0 && pmon & (1 << i) != 0 {
                let modulated =
                    pitch as i32 + (((prev_enveloped >> 5) * pitch as i32) >> 10);
                pitch = modulated.clamp(0, 0x3FFF) as u16;
            }
            let cfg = VoiceConfig {
                vol_l,
                vol_r,
                pitch,
                srcn: self.regs[base + 4],
                adsr1: self.regs[base + 5],
                adsr2: self.regs[base + 6],
                gain: self.regs[base + 7],
                dir,
                counter,
                use_noise: non & (1 << i) != 0,
                noise,
            };
            let (left, right, enveloped) = self.voices[i].sample(ram, &cfg);
            prev_enveloped = enveloped;
            // Hardware exposes the running envelope level and the voice's
            // post-envelope output in ENVX ($x8) and OUTX ($x9). Sound
            // drivers poll these for fades and voice stealing; leaving them
            // at their power-on value made those drivers misbehave.
            self.regs[base + 8] = (self.voices[i].adsr.get_output() >> 4) as u8;
            self.regs[base + 9] = ((enveloped >> 8) as i8) as u8;
            // Hardware saturates the accumulator after EACH voice is added,
            // not once at the end of the mix, so an overdriven mix
            // compresses rather than wrapping or clipping late.
            mix_left = clamp16(mix_left + left);
            mix_right = clamp16(mix_right + right);
            if eon & (1 << i) != 0 {
                echo_in_left = clamp16(echo_in_left + left);
                echo_in_right = clamp16(echo_in_right + right);
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
        let want_delay = if edl == 0 { 1 } else { edl * 512 };

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
        let echo_write_disabled = flg & 0x20 != 0;
        if !echo_write_disabled {
            let wl = clamp16(echo_in_left + ((fir_l * echo_feedback) >> 7));
            let wr = clamp16(echo_in_right + ((fir_r * echo_feedback) >> 7));
            self.echo_ring[self.echo_pos] = (wl, wr);
        }
        // Advance the delay line, and latch an EDL ($7D) change only when
        // the buffer offset wraps -- what hardware does, since EDL is only
        // consulted to decide where the wrap happens. This used to
        // reallocate-and-zero the whole ring the instant EDL changed, so a
        // game switching echo length between songs got an abrupt echo
        // cutout and a click; `resize` here preserves the overlapping
        // history instead.
        self.echo_pos += 1;
        if self.echo_pos >= self.echo_ring.len() {
            self.echo_pos = 0;
            if self.echo_ring.len() != want_delay {
                self.echo_ring.resize(want_delay, (0, 0));
            }
        }

        // Add echo to output
        let echo_volume_left = (self.regs[0x2C] as i8) as i32;
        let echo_volume_right = (self.regs[0x3C] as i8) as i32;
        mix_left = clamp16(mix_left + ((fir_l * echo_volume_left) >> 7));
        mix_right = clamp16(mix_right + ((fir_r * echo_volume_right) >> 7));

        // FLG bit 6 mutes all DSP output. Ignoring it meant a game that
        // muted the DSP (common while uploading a new sound bank) kept
        // playing whatever the voices still held.
        if flg & 0x40 != 0 {
            mix_left = 0;
            mix_right = 0;
        }

        let out_left = mix_left as i16;
        let out_right = mix_right as i16;
        self.output_left = out_left;
        self.output_right = out_right;

        (out_left, out_right)
    }
}

impl Default for Dsp {
    fn default() -> Self {
        Self::new()
    }
}
