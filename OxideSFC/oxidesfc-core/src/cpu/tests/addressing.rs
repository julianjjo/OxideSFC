//! Loads and stores exercised through each addressing mode -- the direct-page
//! wrap rules, long/indexed bank carries, and data-bank selection.

use crate::bus::MemoryBus;
use crate::cpu::{Cpu, CpuFlags};
use crate::wram::Wram;

#[test]
fn lda_immediate_8bit() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.p.insert(CpuFlags::MEMORY_8BIT); // 8-bit mode
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;

    // LDA #$42
    wram.write_u8(0x7E0000, 0xA9).unwrap();
    wram.write_u8(0x7E0001, 0x42).unwrap();

    let cycles = cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.a, 0x42);
    assert_eq!(cycles, 2);
    assert!(!cpu.p.contains(CpuFlags::ZERO));
    assert!(!cpu.p.contains(CpuFlags::NEGATIVE));
}

#[test]
fn lda_immediate_16bit() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.p.remove(CpuFlags::MEMORY_8BIT); // 16-bit mode
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;

    // LDA #$1234
    wram.write_u8(0x7E0000, 0xA9).unwrap();
    wram.write_u8(0x7E0001, 0x34).unwrap();
    wram.write_u8(0x7E0002, 0x12).unwrap();

    let cycles = cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.a, 0x1234);
    assert_eq!(cycles, 3);
}

#[test]
fn lda_immediate_zero_flag() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.p.insert(CpuFlags::MEMORY_8BIT);
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;

    // LDA #$00
    wram.write_u8(0x7E0000, 0xA9).unwrap();
    wram.write_u8(0x7E0001, 0x00).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.a, 0x00);
    assert!(cpu.p.contains(CpuFlags::ZERO));
}

#[test]
fn lda_immediate_negative_flag() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.p.insert(CpuFlags::MEMORY_8BIT);
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;

    // LDA #$80
    wram.write_u8(0x7E0000, 0xA9).unwrap();
    wram.write_u8(0x7E0001, 0x80).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.a, 0x80);
    assert!(cpu.p.contains(CpuFlags::NEGATIVE));
}

#[test]
fn lda_absolute_8bit() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.p.insert(CpuFlags::MEMORY_8BIT);
    cpu.db = 0x7E;
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;

    // Pre-populate memory
    wram.write_u8(0x7E1234, 0xAB).unwrap();

    // LDA $1234
    wram.write_u8(0x7E0000, 0xAD).unwrap();
    wram.write_u8(0x7E0001, 0x34).unwrap();
    wram.write_u8(0x7E0002, 0x12).unwrap();

    let cycles = cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.a, 0xAB);
    assert_eq!(cycles, 4);
}

#[test]
fn lda_absolute_16bit() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.p.remove(CpuFlags::MEMORY_8BIT); // 16-bit mode
    cpu.db = 0x7E;
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;

    // Pre-populate memory (little-endian)
    wram.write_u8(0x7E1234, 0xCD).unwrap();
    wram.write_u8(0x7E1235, 0xAB).unwrap();

    // LDA $1234
    wram.write_u8(0x7E0000, 0xAD).unwrap();
    wram.write_u8(0x7E0001, 0x34).unwrap();
    wram.write_u8(0x7E0002, 0x12).unwrap();

    let cycles = cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.a, 0xABCD);
    assert_eq!(cycles, 5);
}

#[test]
fn lda_direct_page() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.d = 0x1000;
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;

    // Value at DP+10 = 0x1010
    wram.write_u8(0x001010, 0x55).unwrap();

    // LDA $10 (Direct Page)
    wram.write_u8(0x7E0000, 0xA5).unwrap();
    wram.write_u8(0x7E0001, 0x10).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.a, 0x55);
}

#[test]
fn lda_direct_page_with_offset() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.d = 0x10F0; // D low byte != 0
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;
    cpu.p.insert(CpuFlags::MEMORY_8BIT);

    // Value at DP+$10 = 0x10F0 + 0x10 = 0x1100 (wrapping)
    wram.write_u8(0x001100, 0x77).unwrap();

    // LDA $10
    wram.write_u8(0x7E0000, 0xA5).unwrap();
    wram.write_u8(0x7E0001, 0x10).unwrap();

    let cycles = cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.a, 0x77);
    // Extra cycle when D low byte != 0
    assert_eq!(cycles, 4);
}

#[test]
fn sta_absolute_8bit() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.a = 0xAB;
    cpu.p.insert(CpuFlags::MEMORY_8BIT);
    cpu.db = 0x7E;
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;

    // STA $1234
    wram.write_u8(0x7E0000, 0x8D).unwrap();
    wram.write_u8(0x7E0001, 0x34).unwrap();
    wram.write_u8(0x7E0002, 0x12).unwrap();

    let cycles = cpu.step(&mut wram).unwrap();
    assert_eq!(wram.read_u8(0x7E1234).unwrap(), 0xAB);
    assert_eq!(cycles, 4);
}

#[test]
fn sta_absolute_16bit() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.a = 0xABCD;
    cpu.p.remove(CpuFlags::MEMORY_8BIT); // 16-bit mode
    cpu.db = 0x7E;
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;

    // STA $1234
    wram.write_u8(0x7E0000, 0x8D).unwrap();
    wram.write_u8(0x7E0001, 0x34).unwrap();
    wram.write_u8(0x7E0002, 0x12).unwrap();

    let cycles = cpu.step(&mut wram).unwrap();
    assert_eq!(wram.read_u8(0x7E1234).unwrap(), 0xCD); // Low byte
    assert_eq!(wram.read_u8(0x7E1235).unwrap(), 0xAB); // High byte
    assert_eq!(cycles, 5);
}

#[test]
fn sta_direct_page() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.a = 0x99;
    cpu.p.insert(CpuFlags::MEMORY_8BIT);
    // Direct Page addressing always targets bank 0; keep D + operand
    // within the real hardware's 8KB WRAM mirror ($0000-$1FFF) --
    // `Wram` itself now correctly rejects the rest of bank 0
    // ($2000-$7FFF is I/O, $8000-$FFFF is ROM), matching real
    // hardware rather than treating all of bank 0 as WRAM.
    cpu.d = 0x1000;
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;

    // STA $20 (Direct Page)
    wram.write_u8(0x7E0000, 0x85).unwrap();
    wram.write_u8(0x7E0001, 0x20).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(wram.read_u8(0x001020).unwrap(), 0x99);
}

#[test]
fn ldx_immediate_8bit() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.p.insert(CpuFlags::INDEX_8BIT);
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;

    // LDX #$77
    wram.write_u8(0x7E0000, 0xA2).unwrap();
    wram.write_u8(0x7E0001, 0x77).unwrap();

    let cycles = cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.x, 0x77);
    assert_eq!(cycles, 2);
}

#[test]
fn ldx_immediate_16bit() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.p.remove(CpuFlags::INDEX_8BIT);
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;

    // LDX #$BEEF
    wram.write_u8(0x7E0000, 0xA2).unwrap();
    wram.write_u8(0x7E0001, 0xEF).unwrap();
    wram.write_u8(0x7E0002, 0xBE).unwrap();

    let cycles = cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.x, 0xBEEF);
    assert_eq!(cycles, 3);
}

#[test]
fn ldx_absolute() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.p.insert(CpuFlags::INDEX_8BIT);
    cpu.db = 0x7E;
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;

    wram.write_u8(0x7E5678, 0x33).unwrap();

    // LDX $5678
    wram.write_u8(0x7E0000, 0xAE).unwrap();
    wram.write_u8(0x7E0001, 0x78).unwrap();
    wram.write_u8(0x7E0002, 0x56).unwrap();

    let cycles = cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.x, 0x33);
    assert_eq!(cycles, 4);
}

#[test]
fn ldy_immediate_8bit() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.p.insert(CpuFlags::INDEX_8BIT);
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;

    // LDY #$88
    wram.write_u8(0x7E0000, 0xA0).unwrap();
    wram.write_u8(0x7E0001, 0x88).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.y, 0x88);
    assert!(cpu.p.contains(CpuFlags::NEGATIVE));
}

#[test]
fn stx_absolute() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.x = 0xDE;
    cpu.p.insert(CpuFlags::INDEX_8BIT);
    cpu.db = 0x7E;
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;

    // STX $ABCD
    wram.write_u8(0x7E0000, 0x8E).unwrap();
    wram.write_u8(0x7E0001, 0xCD).unwrap();
    wram.write_u8(0x7E0002, 0xAB).unwrap();

    let cycles = cpu.step(&mut wram).unwrap();
    assert_eq!(wram.read_u8(0x7EABCD).unwrap(), 0xDE);
    assert_eq!(cycles, 4);
}

#[test]
fn sty_absolute() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.y = 0x55;
    cpu.p.insert(CpuFlags::INDEX_8BIT);
    cpu.db = 0x7E;
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;

    // STY $3000
    wram.write_u8(0x7E0000, 0x8C).unwrap();
    wram.write_u8(0x7E0001, 0x00).unwrap();
    wram.write_u8(0x7E0002, 0x30).unwrap();

    let cycles = cpu.step(&mut wram).unwrap();
    assert_eq!(wram.read_u8(0x7E3000).unwrap(), 0x55);
    assert_eq!(cycles, 4);
}

#[test]
fn stx_direct_page() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.x = 0x42;
    cpu.p.insert(CpuFlags::INDEX_8BIT);
    cpu.d = 0x0000;
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;

    // STX $05
    wram.write_u8(0x7E0000, 0x86).unwrap();
    wram.write_u8(0x7E0001, 0x05).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(wram.read_u8(0x0005).unwrap(), 0x42);
}

#[test]
fn sty_direct_page() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.y = 0x33;
    cpu.p.insert(CpuFlags::INDEX_8BIT);
    cpu.d = 0x1000;
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;

    // STY $80
    wram.write_u8(0x7E0000, 0x84).unwrap();
    wram.write_u8(0x7E0001, 0x80).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(wram.read_u8(0x1080).unwrap(), 0x33);
}

// ==================== Direct Page indexed wrap quirk ====================
// Regression coverage for the documented 65816 emulation-mode quirk
// (Eyes & Lichty, "Programming the 65816", inherited for 6502
// compatibility): when E=1 and D's low byte is 0, dp,X / dp,Y / (dp,X)
// must wrap (offset + index) within a single 256-byte page instead of
// carrying into D's high byte.

// Test code lives at $7E3000, not $7E0000 -- addresses below $2000 in
// any bank alias the same underlying WRAM bytes as bank 0's Direct
// Page mirror ($7E0000-$7E1FFF == $000000-$001FFF, see `Wram`), so
// placing the opcode/operand there would collide with the small
// effective addresses ($0001, $0002, ...) these quirk cases target.

#[test]
fn lda_dp_x_wraps_within_page_in_emulation_mode_with_dl_zero() {
    let mut cpu = Cpu::new(); // e = true, d = 0 by default
    let mut wram = Wram::new();
    cpu.x = 0x02;
    cpu.p.insert(CpuFlags::MEMORY_8BIT);
    cpu.pb = 0x7E;
    cpu.pc = 0x3000;

    // 0xFF + 0x02 = 0x101 -> must wrap the low byte to $0001, not $0101
    wram.write_u8(0x000001, 0x42).unwrap();
    wram.write_u8(0x000101, 0x99).unwrap(); // decoy: what the old (wrong) code would read

    // LDA $FF,X
    wram.write_u8(0x7E3000, 0xB5).unwrap();
    wram.write_u8(0x7E3001, 0xFF).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.a, 0x42, "must honor the page-wrap quirk, not a plain 16-bit add");
}

#[test]
fn lda_dp_x_uses_full_16bit_add_in_native_mode() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.e = false; // native mode: quirk never applies, regardless of D
    cpu.d = 0x1000;
    cpu.x = 0x02;
    cpu.p.insert(CpuFlags::MEMORY_8BIT);
    cpu.pb = 0x7E;
    cpu.pc = 0x3000;

    // 0x1000 + 0xFF + 0x02 = 0x1101, full carry into D's high byte
    wram.write_u8(0x001101, 0x77).unwrap();

    // LDA $FF,X
    wram.write_u8(0x7E3000, 0xB5).unwrap();
    wram.write_u8(0x7E3001, 0xFF).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.a, 0x77);
}

#[test]
fn lda_dp_x_uses_full_16bit_add_in_emulation_mode_with_dl_nonzero() {
    let mut cpu = Cpu::new(); // e = true
    let mut wram = Wram::new();
    cpu.d = 0x0010; // DL != 0 disables the quirk even in emulation mode
    cpu.x = 0x02;
    cpu.p.insert(CpuFlags::MEMORY_8BIT);
    cpu.pb = 0x7E;
    cpu.pc = 0x3000;

    // 0x0010 + 0xFF + 0x02 = 0x0111
    wram.write_u8(0x000111, 0x88).unwrap();

    // LDA $FF,X
    wram.write_u8(0x7E3000, 0xB5).unwrap();
    wram.write_u8(0x7E3001, 0xFF).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.a, 0x88);
}

#[test]
fn ldx_dp_y_wraps_within_page_in_emulation_mode_with_dl_zero() {
    let mut cpu = Cpu::new(); // e = true, d = 0 by default
    let mut wram = Wram::new();
    cpu.y = 0x02;
    cpu.p.insert(CpuFlags::INDEX_8BIT);
    cpu.pb = 0x7E;
    cpu.pc = 0x3000;

    wram.write_u8(0x000001, 0x11).unwrap();
    wram.write_u8(0x000101, 0xEE).unwrap(); // decoy

    // LDX $FF,Y
    wram.write_u8(0x7E3000, 0xB6).unwrap();
    wram.write_u8(0x7E3001, 0xFF).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.x, 0x11, "must honor the page-wrap quirk, not a plain 16-bit add");
}

#[test]
fn ldx_dp_y_uses_full_16bit_add_in_native_mode() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.e = false;
    cpu.d = 0x1000;
    cpu.y = 0x02;
    cpu.p.insert(CpuFlags::INDEX_8BIT);
    cpu.pb = 0x7E;
    cpu.pc = 0x3000;

    wram.write_u8(0x001101, 0x66).unwrap();

    // LDX $FF,Y
    wram.write_u8(0x7E3000, 0xB6).unwrap();
    wram.write_u8(0x7E3001, 0xFF).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.x, 0x66);
}

#[test]
fn ldx_dp_y_uses_full_16bit_add_in_emulation_mode_with_dl_nonzero() {
    let mut cpu = Cpu::new(); // e = true
    let mut wram = Wram::new();
    cpu.d = 0x0010; // DL != 0 disables the quirk even in emulation mode
    cpu.y = 0x02;
    cpu.p.insert(CpuFlags::INDEX_8BIT);
    cpu.pb = 0x7E;
    cpu.pc = 0x3000;

    // 0x0010 + 0xFF + 0x02 = 0x0111
    wram.write_u8(0x000111, 0x44).unwrap();

    // LDX $FF,Y
    wram.write_u8(0x7E3000, 0xB6).unwrap();
    wram.write_u8(0x7E3001, 0xFF).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.x, 0x44);
}

#[test]
fn lda_indirect_dp_x_wraps_pointer_lookup_within_page_in_emulation_mode_with_dl_zero() {
    let mut cpu = Cpu::new(); // e = true, d = 0 by default
    let mut wram = Wram::new();
    cpu.x = 0x02;
    cpu.db = 0x7E;
    cpu.p.insert(CpuFlags::MEMORY_8BIT);
    cpu.pb = 0x7E;
    cpu.pc = 0x3000;

    // Pointer must be read from wrapped dp address $0001, not $0101
    wram.write_u8(0x000001, 0x00).unwrap(); // pointer lo
    wram.write_u8(0x000002, 0x02).unwrap(); // pointer hi -> pointer = $0200
    wram.write_u8(0x7E0200, 0x5A).unwrap(); // target value

    // Decoy pointer at the unwrapped ($0101) address, pointing elsewhere
    wram.write_u8(0x000101, 0xAD).unwrap();
    wram.write_u8(0x000102, 0xDE).unwrap();

    // LDA ($FF,X)
    wram.write_u8(0x7E3000, 0xA1).unwrap();
    wram.write_u8(0x7E3001, 0xFF).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.a, 0x5A, "must dereference the page-wrapped dp pointer, not a plain 16-bit add");
}

#[test]
fn lda_indirect_dp_x_uses_full_16bit_add_in_native_mode() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.e = false;
    cpu.d = 0x1000;
    cpu.x = 0x02;
    cpu.db = 0x7E;
    cpu.p.insert(CpuFlags::MEMORY_8BIT);
    cpu.pb = 0x7E;
    cpu.pc = 0x3000;

    // dp_addr = 0x1000 + 0xFF + 0x02 = 0x1101
    wram.write_u8(0x001101, 0x00).unwrap(); // pointer lo
    wram.write_u8(0x001102, 0x03).unwrap(); // pointer hi -> pointer = $0300
    wram.write_u8(0x7E0300, 0x9C).unwrap();

    // LDA ($FF,X)
    wram.write_u8(0x7E3000, 0xA1).unwrap();
    wram.write_u8(0x7E3001, 0xFF).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.a, 0x9C);
}

#[test]
fn lda_indirect_dp_x_uses_full_16bit_add_in_emulation_mode_with_dl_nonzero() {
    let mut cpu = Cpu::new(); // e = true
    let mut wram = Wram::new();
    cpu.d = 0x0010; // DL != 0 disables the quirk even in emulation mode
    cpu.x = 0x02;
    cpu.db = 0x7E;
    cpu.p.insert(CpuFlags::MEMORY_8BIT);
    cpu.pb = 0x7E;
    cpu.pc = 0x3000;

    // dp_addr = 0x0010 + 0xFF + 0x02 = 0x0111
    wram.write_u8(0x000111, 0x00).unwrap(); // pointer lo
    wram.write_u8(0x000112, 0x04).unwrap(); // pointer hi -> pointer = $0400
    wram.write_u8(0x7E0400, 0x13).unwrap();

    // LDA ($FF,X)
    wram.write_u8(0x7E3000, 0xA1).unwrap();
    wram.write_u8(0x7E3001, 0xFF).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.a, 0x13);
}

// ==================== New opcode tests ====================

#[test]
fn cpu_stz_abs_writes_zero_8bit() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.p.insert(CpuFlags::MEMORY_8BIT);
    cpu.db = 0x7E;
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;

    wram.write_u8(0x7E1234, 0xFF).unwrap(); // pre-fill with garbage
    wram.write_u8(0x7E0000, 0x9C).unwrap(); // STZ $1234
    wram.write_u8(0x7E0001, 0x34).unwrap();
    wram.write_u8(0x7E0002, 0x12).unwrap();

    let cycles = cpu.step(&mut wram).unwrap();
    assert_eq!(cycles, 4);
    assert_eq!(wram.read_u8(0x7E1234).unwrap(), 0x00);
}

#[test]
fn cpu_stz_dp_writes_zero_16bit() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.p.remove(CpuFlags::MEMORY_8BIT); // 16-bit accumulator/memory
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;
    cpu.d = 0x0000;

    wram.write_u8(0x7E0050, 0xAA).unwrap();
    wram.write_u8(0x7E0051, 0xBB).unwrap();
    wram.write_u8(0x7E0000, 0x64).unwrap(); // STZ $50
    wram.write_u8(0x7E0001, 0x50).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(wram.read_u8(0x7E0050).unwrap(), 0x00);
    assert_eq!(wram.read_u8(0x7E0051).unwrap(), 0x00);
}

#[test]
fn cpu_sta_long_uses_explicit_bank_ignoring_db() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.p.insert(CpuFlags::MEMORY_8BIT);
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;
    cpu.db = 0x00; // deliberately different from the long address's bank
    cpu.a = 0x42;

    // STA $7E2000 (Absolute Long): bytes are little-endian addr, then bank
    wram.write_u8(0x7E0000, 0x8F).unwrap();
    wram.write_u8(0x7E0001, 0x00).unwrap();
    wram.write_u8(0x7E0002, 0x20).unwrap();
    wram.write_u8(0x7E0003, 0x7E).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(wram.read_u8(0x7E2000).unwrap(), 0x42, "STA long must use its own embedded bank, not DB");
}

#[test]
fn cpu_lda_long_reads_explicit_bank() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.p.insert(CpuFlags::MEMORY_8BIT);
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;
    cpu.db = 0x00;

    wram.write_u8(0x7E3000, 0x99).unwrap();
    wram.write_u8(0x7E0000, 0xAF).unwrap(); // LDA $7E3000 (Absolute Long)
    wram.write_u8(0x7E0001, 0x00).unwrap();
    wram.write_u8(0x7E0002, 0x30).unwrap();
    wram.write_u8(0x7E0003, 0x7E).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.a, 0x99);
}

#[test]
fn cpu_sta_long_x_wraps_carry_into_bank() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.p.insert(CpuFlags::MEMORY_8BIT);
    cpu.p.insert(CpuFlags::INDEX_8BIT);
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;
    cpu.a = 0x55;
    cpu.x = 0x10;

    // STA $7EFFF8,X with X=0x10 -> effective address 0x7F0008 (carries into next bank)
    wram.write_u8(0x7E0000, 0x9F).unwrap();
    wram.write_u8(0x7E0001, 0xF8).unwrap();
    wram.write_u8(0x7E0002, 0xFF).unwrap();
    wram.write_u8(0x7E0003, 0x7E).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(wram.read_u8(0x7F0008).unwrap(), 0x55, "the carry from base+X must cross into the next bank");
}

#[test]
fn cpu_sta_abs_x_carries_into_next_bank() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.p.insert(CpuFlags::MEMORY_8BIT);
    cpu.p.insert(CpuFlags::INDEX_8BIT);
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;
    cpu.db = 0x7E;
    cpu.a = 0x77;
    cpu.x = 0x10;

    // STA $FFF8,X with X=0x10: DB:$FFF8 + X overflows the 16-bit offset,
    // so the carry propagates into the bank byte -- effective address is
    // $7F0008, not $7E0008 (real 65816 hardware behavior).
    wram.write_u8(0x7E0000, 0x9D).unwrap();
    wram.write_u8(0x7E0001, 0xF8).unwrap();
    wram.write_u8(0x7E0002, 0xFF).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(wram.read_u8(0x7F0008).unwrap(), 0x77, "plain Absolute,X must carry into the next bank on overflow");
}

#[test]
fn cpu_sta_abs_y_carries_into_next_bank() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.p.insert(CpuFlags::MEMORY_8BIT);
    cpu.p.insert(CpuFlags::INDEX_8BIT);
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;
    cpu.db = 0x7E;
    cpu.a = 0x77;
    cpu.y = 0x0A;

    // STA $FFFE,Y with Y=0x0A: DB:$FFFE + Y overflows the 16-bit offset,
    // so the carry propagates into the bank byte -- effective address is
    // $7F0008, not $7E0008.
    wram.write_u8(0x7E0000, 0x99).unwrap();
    wram.write_u8(0x7E0001, 0xFE).unwrap();
    wram.write_u8(0x7E0002, 0xFF).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(wram.read_u8(0x7F0008).unwrap(), 0x77, "plain Absolute,Y must carry into the next bank on overflow");
}

#[test]
fn cpu_lda_abs_x_carries_into_next_bank_wrapping_ff_to_00() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.p.insert(CpuFlags::MEMORY_8BIT);
    cpu.p.insert(CpuFlags::INDEX_8BIT);
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;
    cpu.db = 0xFF;
    cpu.x = 0x0A;

    // LDA $FFFE,X with DB=$FF, X=0x0A: the bank carry must wrap from
    // $FF to $00 (the canonical Eyes & Lichty example), landing at $000008.
    wram.write_u8(0x7E0008, 0x42).unwrap();
    wram.write_u8(0x7E0000, 0xBD).unwrap();
    wram.write_u8(0x7E0001, 0xFE).unwrap();
    wram.write_u8(0x7E0002, 0xFF).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.a, 0x42, "bank carry must wrap from $FF to $00");
}

#[test]
fn cpu_lda_abs_y_carries_into_next_bank_wrapping_ff_to_00() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.p.insert(CpuFlags::MEMORY_8BIT);
    cpu.p.insert(CpuFlags::INDEX_8BIT);
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;
    cpu.db = 0xFF;
    cpu.y = 0x0A;

    // LDA $FFFE,Y with DB=$FF, Y=0x0A: same wraparound as the ,X case.
    wram.write_u8(0x7E0008, 0x42).unwrap();
    wram.write_u8(0x7E0000, 0xB9).unwrap();
    wram.write_u8(0x7E0001, 0xFE).unwrap();
    wram.write_u8(0x7E0002, 0xFF).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.a, 0x42, "bank carry must wrap from $FF to $00");
}

#[test]
fn cpu_lda_abs_x_reads_from_data_bank() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.p.insert(CpuFlags::MEMORY_8BIT);
    cpu.p.insert(CpuFlags::INDEX_8BIT);
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;
    cpu.db = 0x7E;
    cpu.x = 0x05;

    wram.write_u8(0x7E2005, 0x66).unwrap();
    wram.write_u8(0x7E0000, 0xBD).unwrap(); // LDA $2000,X
    wram.write_u8(0x7E0001, 0x00).unwrap();
    wram.write_u8(0x7E0002, 0x20).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.a, 0x66);
}

// ==================== Bug-fix regression tests ====================

