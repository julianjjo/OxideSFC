//! BRR (Bit Rate Reduction) block decoding -- the SNES' 9-bytes-per-16-samples
//! ADPCM format, with the four prediction filters.


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
            // Each byte holds two samples played high nibble first, low
            // nibble second (H0,L0,H1,L1,... per fullsnes and bsnes's
            // decode_brr). An earlier version emitted all 8 low nibbles
            // then all 8 high nibbles, time-scrambling every 16-sample
            // block and running the prediction-filter history in the
            // wrong order.
            let byte = data[i >> 1];
            let nibble = if i & 1 == 0 { (byte >> 4) & 0x0F } else { byte & 0x0F };

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
