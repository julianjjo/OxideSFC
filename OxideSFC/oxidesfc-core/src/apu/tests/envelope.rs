//! ADSR/GAIN envelope behavior and its rate gating.

use crate::apu::envelope::{Adsr, EnvMode};
use crate::apu::envelope::SIMPLE_COUNTER_RANGE;

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

