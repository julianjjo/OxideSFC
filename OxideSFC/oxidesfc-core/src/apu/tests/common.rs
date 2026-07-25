//! Fixtures shared by the APU test modules.

use crate::apu::dsp::Dsp;
use crate::apu::spc700::Spc700;
use crate::apu::ApuPorts;
use std::sync::{Arc, Mutex};

pub(super) fn isolated_spc700() -> Spc700 {
    let ram = Arc::new(Mutex::new([0u8; 65536]));
    let ports = Arc::new(Mutex::new(ApuPorts::default()));
    let dsp = Arc::new(Mutex::new(Dsp::new()));
    Spc700::new(ram, ports, dsp)
}

/// Builds APU RAM with a sample directory at page 2 (dir=$02) whose
/// source 0 points at a BRR sample at $0300: a self-looping block whose
/// 16 nibbles are 0,1,2,...,15 in hardware play order (high nibble of
/// each byte first), decoding to a full-scale sawtooth.
///
/// The shift is 11 rather than 0 so the decoded samples span most of
/// the i16 range. With shift 0 the whole waveform fits in -8..+6, and
/// the DSP's real output path (gaussian taps scaled `>> 11`, then the
/// hardware's low-bit clear) quantizes a signal that small down to a
/// handful of distinct values -- which makes amplitude-sensitive
/// assertions like `resampling_interpolates_between_source_samples...`
/// measure quantization instead of the thing they mean to measure.
pub(super) fn ram_with_ramp_sample() -> Box<[u8; 65536]> {
    let mut ram: Box<[u8; 65536]> = vec![0u8; 65536].try_into().unwrap();
    // Directory entry 0 at $0200: start=$0300, loop=$0300.
    ram[0x0200] = 0x00;
    ram[0x0201] = 0x03;
    ram[0x0202] = 0x00;
    ram[0x0203] = 0x03;
    // One BRR block at $0300: header shift=11/filter=0, loop+end set so
    // it loops onto itself forever. Nibbles 0,1,2,...,15 -> a ramp.
    ram[0x0300] = 0xB3; // shift=11 (bits 4-7) + end(bit0) + loop(bit1)
    for i in 0..8usize {
        let hi = (i * 2) as u8;
        let lo = (i * 2 + 1) as u8;
        ram[0x0301 + i] = (hi << 4) | (lo & 0x0F);
    }
    ram
}

/// Configures DSP voice 0 to play source 0 (dir page $02) at the given
/// pitch with instant-attack ADSR and full volume.
pub(super) fn configure_ramp_voice(dsp: &mut Dsp, pitch: u16) {
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

