//! DSP synthesis: the gaussian resampler, noise, pitch modulation, the
//! echo path, KON/KOFF/ENDX bookkeeping and save-state round-tripping.

use super::common::{configure_ramp_voice, ram_with_ramp_sample};
use crate::apu::dsp::Dsp;
use crate::apu::envelope::EnvMode;
use crate::apu::voice::GAUSS;

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
fn non_flagged_voice_plays_the_noise_lfsr_instead_of_its_brr_sample() {
    // Regression guard for a completely absent noise generator: the DSP
    // never read NON ($3D) or FLG's noise-rate bits and had no LFSR at
    // all, so voices flagged for noise played their (usually
    // meaningless) BRR data instead. Percussion -- hi-hats, snares,
    // cymbals -- and noise SFX (wind, rain, explosions) were wrong or
    // missing across a large part of the library.
    let ram = ram_with_ramp_sample();

    let mut noisy = Dsp::new();
    configure_ramp_voice(&mut noisy, 0x1000);
    noisy.write_reg(0x3D, 0x01); // NON: voice 0 uses noise
    noisy.write_reg(0x6C, 0x1F); // FLG: fastest noise clock, echo on

    let mut tonal = Dsp::new();
    configure_ramp_voice(&mut tonal, 0x1000);
    tonal.write_reg(0x6C, 0x1F);

    let mut noisy_out = Vec::new();
    let mut tonal_out = Vec::new();
    for _ in 0..400 {
        noisy_out.push(noisy.sample(&ram).0 as i32);
        tonal_out.push(tonal.sample(&ram).0 as i32);
    }

    assert_ne!(
        noisy_out, tonal_out,
        "a NON-flagged voice must not produce the same output as the BRR sample"
    );
    // The LFSR must actually be running: a stuck generator would emit a
    // constant (or DC-ish) value rather than a broadband signal.
    let distinct: std::collections::HashSet<i32> =
        noisy_out[100..].iter().copied().collect();
    assert!(
        distinct.len() > 20,
        "the noise LFSR must advance, giving many distinct values; got {}",
        distinct.len()
    );
}

#[test]
fn noise_clock_rate_zero_freezes_the_lfsr() {
    // FLG bits 0-4 = 0 selects the rate that never fires (see
    // `COUNTER_RATES[0]`), so the noise value must hold steady -- games
    // rely on this to hold a fixed "noise" DC level.
    let ram = ram_with_ramp_sample();
    let mut dsp = Dsp::new();
    configure_ramp_voice(&mut dsp, 0x1000);
    dsp.write_reg(0x3D, 0x01); // NON voice 0
    dsp.write_reg(0x6C, 0x00); // noise rate 0 -> never clocks

    let mut out = Vec::new();
    for _ in 0..200 {
        out.push(dsp.sample(&ram).0 as i32);
    }
    // Past the attack ramp the envelope is flat, so a frozen LFSR gives
    // a constant output.
    let tail = &out[100..];
    assert!(
        tail.iter().all(|&v| v == tail[0]),
        "with noise rate 0 the LFSR must not advance; got varying output"
    );
}

#[test]
fn pitch_modulation_alters_the_modulated_voices_output() {
    // Regression guard for absent PMON ($2D): voice N's pitch must be
    // modulated by voice N-1's post-envelope output. Games use this for
    // vibrato/growl effects, which played as static pitch before.
    let ram = ram_with_ramp_sample();

    let configure_pair = |dsp: &mut Dsp, pmon: u8| {
        dsp.write_reg(0x5D, 0x02); // DIR page 2
        for base in [0x00u8, 0x10u8] {
            dsp.write_reg(base, 0x7F); // VOL(L)
            dsp.write_reg(base + 1, 0x7F); // VOL(R)
            dsp.write_reg(base + 2, 0x00); // pitch lo
            dsp.write_reg(base + 3, 0x10); // pitch hi -> 0x1000
            dsp.write_reg(base + 4, 0x00); // SRCN 0
            dsp.write_reg(base + 5, 0x8F); // ADSR1 enabled, fast attack
            dsp.write_reg(base + 6, 0xE0); // ADSR2 max sustain
        }
        dsp.write_reg(0x0C, 0x7F); // MVOLL
        dsp.write_reg(0x1C, 0x7F); // MVOLR
        dsp.write_reg(0x2D, pmon);
        dsp.write_reg(0x4C, 0x03); // KON voices 0 and 1
    };

    let mut modulated = Dsp::new();
    configure_pair(&mut modulated, 0x02); // PMON on voice 1
    let mut plain = Dsp::new();
    configure_pair(&mut plain, 0x00);

    let mut a = Vec::new();
    let mut b = Vec::new();
    for _ in 0..600 {
        a.push(modulated.sample(&ram).0 as i32);
        b.push(plain.sample(&ram).0 as i32);
    }

    assert_ne!(
        a, b,
        "setting a voice's PMON bit must change its pitch, and so its output"
    );
}

#[test]
fn pmon_bit_for_voice_zero_is_ignored() {
    // Voice 0 has no predecessor, so hardware leaves PMON bit 0 unused.
    let ram = ram_with_ramp_sample();
    let mut with_bit = Dsp::new();
    configure_ramp_voice(&mut with_bit, 0x1000);
    with_bit.write_reg(0x2D, 0x01);
    let mut without = Dsp::new();
    configure_ramp_voice(&mut without, 0x1000);

    for _ in 0..200 {
        assert_eq!(with_bit.sample(&ram), without.sample(&ram));
    }
}

#[test]
fn flg_mute_bit_silences_the_output() {
    // FLG bit 6 mutes all DSP output. Drivers set it while swapping
    // sample banks; ignoring it let the old voices keep sounding.
    let ram = ram_with_ramp_sample();
    let mut dsp = Dsp::new();
    configure_ramp_voice(&mut dsp, 0x1000);

    // Establish that the voice is audible first, so the assertion below
    // can't pass just because nothing was playing.
    let mut audible = false;
    for _ in 0..200 {
        if dsp.sample(&ram).0 != 0 {
            audible = true;
        }
    }
    assert!(audible, "the voice must be producing sound before muting");

    dsp.write_reg(0x6C, 0x40); // FLG: mute
    for _ in 0..200 {
        assert_eq!(dsp.sample(&ram), (0, 0), "FLG bit 6 must mute the DSP");
    }
}

#[test]
fn flg_soft_reset_bit_key_offs_every_voice() {
    // FLG bit 7 is a DSP soft reset: hardware key-offs all voices.
    let ram = ram_with_ramp_sample();
    let mut dsp = Dsp::new();
    configure_ramp_voice(&mut dsp, 0x1000);
    for _ in 0..50 {
        dsp.sample(&ram);
    }
    assert!(dsp.voices[0].active, "voice must be running before the reset");

    dsp.write_reg(0x6C, 0x80);
    // The envelope is zeroed and put in release, so the next sample
    // retires the voice.
    dsp.sample(&ram);
    assert!(
        !dsp.voices[0].active,
        "FLG bit 7 must key-off every voice"
    );
}

#[test]
fn envx_and_outx_registers_track_the_running_voice() {
    // Regression guard: ENVX ($x8) and OUTX ($x9) were never written
    // back, so sound drivers that poll them (fade-outs, voice stealing)
    // read the power-on value forever.
    let ram = ram_with_ramp_sample();
    let mut dsp = Dsp::new();
    configure_ramp_voice(&mut dsp, 0x1000);

    for _ in 0..300 {
        dsp.sample(&ram);
    }

    assert_ne!(
        dsp.read_reg(0x08), 0,
        "ENVX must expose the running envelope level"
    );
    let mut outx_values = std::collections::HashSet::new();
    for _ in 0..200 {
        dsp.sample(&ram);
        outx_values.insert(dsp.read_reg(0x09));
    }
    assert!(
        outx_values.len() > 1,
        "OUTX must follow the voice's output, not hold one value"
    );
}

#[test]
fn changing_edl_preserves_echo_history_until_the_buffer_wraps() {
    // Regression guard: any $7D write that changed EDL used to
    // reallocate-and-zero the delay line immediately, so a game
    // switching echo length between songs got an abrupt echo cutout and
    // a click. Hardware only consults EDL to decide where the buffer
    // offset wraps, so a mid-buffer change must leave the existing
    // reflection audible until then.
    let ram = ram_with_ramp_sample();
    let mut dsp = Dsp::new();
    configure_ramp_voice(&mut dsp, 0x1000);
    dsp.write_reg(0x7D, 0x01); // EDL = 1 -> 512-sample ring
    dsp.write_reg(0x4D, 0x01); // EON voice 0
    dsp.write_reg(0x2C, 0x7F); // EVOL(L)
    dsp.write_reg(0x3C, 0x7F); // EVOL(R)
    dsp.write_reg(0x7F, 0x7F); // FIR coefficient 7 = newest tap

    // Fill the delay line and get past the first wrap so echo energy is
    // actually flowing.
    for _ in 0..700 {
        dsp.sample(&ram);
    }
    let before: Vec<i32> = (0..20).map(|_| dsp.sample(&ram).0 as i32).collect();

    // Grow the echo length mid-buffer; the next handful of samples must
    // still read the history already in the ring.
    dsp.write_reg(0x7D, 0x04); // EDL = 4 -> 2048-sample ring, at next wrap
    let after: Vec<i32> = (0..20).map(|_| dsp.sample(&ram).0 as i32).collect();

    assert!(
        after.iter().any(|&v| v != 0),
        "the echo must keep sounding across an EDL change, not cut out"
    );
    assert!(
        before.iter().any(|&v| v != 0),
        "sanity: echo must be audible before the EDL change"
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
fn gauss_table_matches_hardware_shape() {
    // Pins the transcription of the hardware gaussian kernel (see
    // `GAUSS`): a typo'd digit would silently detune/alias every voice.
    assert_eq!(GAUSS.len(), 512);
    assert_eq!(GAUSS[0], 0);
    assert_eq!(GAUSS[511], 1305, "the table's documented peak value");
    assert!(
        GAUSS.windows(2).all(|w| w[1] >= w[0]),
        "the kernel is monotonically non-decreasing"
    );
    // The four taps a given offset selects sum to 2048 +/- 1 (hardware
    // rounding), so interpolation has unity gain after the `>> 11`.
    for offset in 0..256usize {
        let sum = GAUSS[255 - offset] as i32
            + GAUSS[511 - offset] as i32
            + GAUSS[256 + offset] as i32
            + GAUSS[offset] as i32;
        assert!(
            (2047..=2049).contains(&sum),
            "taps for offset {} sum to {}, outside hardware's 2048 +/- 1",
            offset,
            sum
        );
    }
    // The classic offset-0 quadruple, quoted in the DSP documentation.
    assert_eq!(
        (GAUSS[255], GAUSS[511], GAUSS[256], GAUSS[0]),
        (370, 1305, 374, 0)
    );
}

// ========================================================================
// Regression tests for the six fixes below (stereo output, ENDX, MOV
// dp,dp's P-flag handling, KON/KOFF trigger cleanup, GAIN-mode sustain
// transition, and Spc700::reset's timer/DSP-latch clearing).
// ========================================================================

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

