//! A single DSP voice: BRR playback with the hardware gaussian resampler,
//! its envelope, and per-voice volume.

use super::brr::BrrDecoder;
use super::envelope::{Adsr, EnvMode};
use super::clamp16;

/// The SNES DSP's 512-entry gaussian interpolation table, transcribed from
/// bsnes' `SPC_DSP.cpp` (`gauss`).
///
/// This is the real hardware kernel used to resample BRR samples to the
/// output rate, and it is a deliberately *lowpassing* filter -- BRR content
/// was authored expecting it. `Voice::sample` previously used Catmull-Rom
/// cubic interpolation as a stand-in, which is a brighter kernel: it passes
/// the BRR quantization noise the gaussian rolls off, and overshoots, so
/// every sampled instrument came out harsher and grainier than on hardware
/// (audibly "not the same quality as bsnes").
///
/// The table is not perfectly normalized -- the four taps a given offset
/// selects sum to 2048 +/- 1, matching hardware's rounding, which
/// `gauss_table_matches_hardware_shape` pins along with the classic
/// offset-0 quadruple (370, 1305, 374, 0).
pub(super) const GAUSS: [i16; 512] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2,
    2, 2, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 5, 5, 5, 5,
    6, 6, 6, 6, 7, 7, 7, 8, 8, 8, 9, 9, 9, 10, 10, 10,
    11, 11, 11, 12, 12, 13, 13, 14, 14, 15, 15, 15, 16, 16, 17, 17,
    18, 19, 19, 20, 20, 21, 21, 22, 23, 23, 24, 24, 25, 26, 27, 27,
    28, 29, 29, 30, 31, 32, 32, 33, 34, 35, 36, 36, 37, 38, 39, 40,
    41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56,
    58, 59, 60, 61, 62, 64, 65, 66, 67, 69, 70, 71, 73, 74, 76, 77,
    78, 80, 81, 83, 84, 86, 87, 89, 90, 92, 94, 95, 97, 99, 100, 102,
    104, 106, 107, 109, 111, 113, 115, 117, 118, 120, 122, 124, 126, 128, 130, 132,
    134, 137, 139, 141, 143, 145, 147, 150, 152, 154, 156, 159, 161, 163, 166, 168,
    171, 173, 175, 178, 180, 183, 186, 188, 191, 193, 196, 199, 201, 204, 207, 210,
    212, 215, 218, 221, 224, 227, 230, 233, 236, 239, 242, 245, 248, 251, 254, 257,
    260, 263, 267, 270, 273, 276, 280, 283, 286, 290, 293, 297, 300, 304, 307, 311,
    314, 318, 321, 325, 328, 332, 336, 339, 343, 347, 351, 354, 358, 362, 366, 370,
    374, 378, 381, 385, 389, 393, 397, 401, 405, 410, 414, 418, 422, 426, 430, 434,
    439, 443, 447, 451, 456, 460, 464, 469, 473, 477, 482, 486, 491, 495, 499, 504,
    508, 513, 517, 522, 527, 531, 536, 540, 545, 550, 554, 559, 563, 568, 573, 577,
    582, 587, 592, 596, 601, 606, 611, 615, 620, 625, 630, 635, 640, 644, 649, 654,
    659, 664, 669, 674, 678, 683, 688, 693, 698, 703, 708, 713, 718, 723, 728, 732,
    737, 742, 747, 752, 757, 762, 767, 772, 777, 782, 787, 792, 797, 802, 806, 811,
    816, 821, 826, 831, 836, 841, 846, 851, 855, 860, 865, 870, 875, 880, 884, 889,
    894, 899, 904, 908, 913, 918, 923, 927, 932, 937, 941, 946, 951, 955, 960, 965,
    969, 974, 978, 983, 988, 992, 997, 1001, 1005, 1010, 1014, 1019, 1023, 1027, 1032, 1036,
    1040, 1045, 1049, 1053, 1057, 1061, 1066, 1070, 1074, 1078, 1082, 1086, 1090, 1094, 1098, 1102,
    1106, 1109, 1113, 1117, 1121, 1125, 1128, 1132, 1136, 1139, 1143, 1146, 1150, 1153, 1157, 1160,
    1164, 1167, 1170, 1174, 1177, 1180, 1183, 1186, 1190, 1193, 1196, 1199, 1202, 1205, 1207, 1210,
    1213, 1216, 1219, 1221, 1224, 1227, 1229, 1232, 1234, 1237, 1239, 1241, 1244, 1246, 1248, 1251,
    1253, 1255, 1257, 1259, 1261, 1263, 1265, 1267, 1269, 1270, 1272, 1274, 1275, 1277, 1279, 1280,
    1282, 1283, 1284, 1286, 1287, 1288, 1290, 1291, 1292, 1293, 1294, 1295, 1296, 1297, 1297, 1298,
    1299, 1300, 1300, 1301, 1302, 1302, 1303, 1303, 1303, 1304, 1304, 1304, 1304, 1304, 1305, 1305,
];

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
    /// 4-point resampling interpolator -- real hardware's 4-tap gaussian
    /// filter, using the hardware kernel in `GAUSS`.
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
    ///
    /// `use_noise` is this voice's NON ($3D) bit: when set, the voice's
    /// source is the DSP's shared noise LFSR (`noise`) instead of its BRR
    /// sample, exactly as on hardware. BRR playback still advances
    /// underneath, so clearing NON resumes the sample where it would have
    /// been.
    ///
    /// Returns `(left, right, enveloped)`, where `enveloped` is the
    /// post-envelope, pre-volume output that hardware feeds to the next
    /// voice's pitch modulation and to this voice's OUTX register.
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
        use_noise: bool,
        noise: i32,
    ) -> (i32, i32, i32) {
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
            return (0, 0, 0);
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
                        return (0, 0, 0);
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
            return (0, 0, 0);
        }

        // 4-point gaussian interpolation, the real hardware kernel (see
        // `GAUSS`). The top 8 bits of the 12-bit fractional pitch position
        // select the tap set; each tap is scaled >> 11, and the third
        // accumulation is truncated to 16 bits *before* the fourth tap is
        // added -- a genuine hardware quirk (bsnes `SPC_DSP::interpolate`
        // does the same `(int16_t)` narrowing mid-sum), not an accident.
        let brr_sample = if use_noise {
            // NON: the shared noise LFSR replaces the sample source.
            (noise * 2) as i16 as i32
        } else {
            let offset = ((self.pitch_counter >> 4) & 0xFF) as usize;
            let fwd = 255 - offset;
            let rev = offset;
            let mut out = (GAUSS[fwd] as i32 * self.hist[0]) >> 11;
            out += (GAUSS[fwd + 256] as i32 * self.hist[1]) >> 11;
            out += (GAUSS[rev + 256] as i32 * self.hist[2]) >> 11;
            out = out as i16 as i32;
            out += (GAUSS[rev] as i32 * self.hist[3]) >> 11;
            // Hardware clears the low bit of the interpolated result.
            clamp16(out) & !1
        };
        // Envelope is 11-bit, so >> 11 to scale. Hardware also clears the
        // low bit of the post-envelope value.
        let enveloped = ((brr_sample * env) >> 11) & !1;

        // Per-voice volume is a signed 8-bit value; >> 7 normalizes it.
        self.output_left = (enveloped * (vol_l as i32)) >> 7;
        self.output_right = (enveloped * (vol_r as i32)) >> 7;
        (self.output_left, self.output_right, enveloped)
    }
}

impl Default for Voice {
    fn default() -> Self {
        Self::new()
    }
}
