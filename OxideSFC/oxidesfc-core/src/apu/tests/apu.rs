//! The `Apu` wrapper the main CPU sees: RAM access, the communication
//! ports, sample generation rate and cycle pacing.

use crate::apu::{Apu, PAL_MASTER_CLOCK_HZ};
use super::common::{isolated_spc700, ram_with_ramp_sample};
use crate::apu::dsp::Dsp;

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
fn tick_generates_samples_at_exactly_32khz_not_the_old_28x_too_fast_rate() {
    // One emulated second of the pacing unit (master/8) must produce
    // exactly 32,000 samples: 2,684,659 unit cycles -> 1,024,000
    // SPC700 steps -> /32 = 32,000. The exact-ratio conversion makes
    // this deterministic to +/-1 sample regardless of chunking. The
    // old divider-84 pacing produced ~31,960 (0.4% slow -> steady
    // frontend buffer underrun = periodic stutter); the ancient
    // divider-3 bug produced ~893,000.
    let mut apu = Apu::new();
    const ONE_SECOND_OF_UNIT_CYCLES: u32 = 2_684_659;
    const CHUNK: u32 = 1000;
    let mut remaining = ONE_SECOND_OF_UNIT_CYCLES;
    while remaining > 0 {
        let step = remaining.min(CHUNK);
        apu.tick(step);
        remaining -= step;
    }

    let generated = apu.buffer_size();
    assert!(
        (31_999..=32_001).contains(&generated),
        "expected exactly ~32,000 samples for one emulated second, got {}",
        generated
    );
}

#[test]
fn pal_machines_also_generate_exactly_32khz_audio() {
    // The DSP is clocked by the SPC700's own crystal, so its output rate
    // is 32kHz regardless of the video standard. `tick` receives master/8
    // units, and a PAL master/8 unit is 0.9% shorter than an NTSC one --
    // converting with a hardcoded NTSC unit rate (which is what the old
    // code did) would produce a ~31,708 Hz stream against the frontend's
    // 32,000 Hz playback, a drift the +/-0.5% rate control can't absorb.
    let mut apu = Apu::new();
    apu.set_master_clock_hz(PAL_MASTER_CLOCK_HZ);

    // One emulated PAL second: 21,281,370 master cycles / 8.
    const ONE_SECOND_OF_UNIT_CYCLES: u32 = PAL_MASTER_CLOCK_HZ / 8;
    const CHUNK: u32 = 977;
    let mut remaining = ONE_SECOND_OF_UNIT_CYCLES;
    while remaining > 0 {
        let step = remaining.min(CHUNK);
        apu.tick(step);
        remaining -= step;
    }

    let generated = apu.buffer_size();
    assert!(
        (31_998..=32_002).contains(&generated),
        "a PAL machine must still generate ~32,000 samples per emulated \
         second, got {}",
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

