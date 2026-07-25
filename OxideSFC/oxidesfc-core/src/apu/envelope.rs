//! ADSR/GAIN envelope generation and the global rate counter that decides,
//! per rate, which samples an envelope advances on.


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
pub(super) const SIMPLE_COUNTER_RANGE: u32 = 2048 * 5 * 3;

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
pub(super) fn read_counter(counter: u32, rate: usize) -> u32 {
    counter.wrapping_add(COUNTER_OFFSETS[rate]) % COUNTER_RATES[rate]
}

impl Adsr {
    pub fn new() -> Self {
        Adsr { level: 0, mode: EnvMode::Release, hidden: 0 }
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
