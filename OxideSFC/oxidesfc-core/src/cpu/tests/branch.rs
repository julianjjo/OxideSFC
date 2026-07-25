//! Jumps and subroutine calls, including which bank each indirect pointer is
//! fetched from.

use crate::bus::MemoryBus;
use crate::cpu::Cpu;
use crate::wram::Wram;

#[test]
fn cpu_jsr_rts() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.pb = 0x7E;
    cpu.sp = 0x01FF;
    cpu.pc = 0x0000;

    // JSR $0200
    wram.write_u8(0x7E0000, 0x20).unwrap();
    wram.write_u8(0x7E0001, 0x00).unwrap();
    wram.write_u8(0x7E0002, 0x02).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.pc, 0x0200);
    // In emulation mode, JSR pushes 2 bytes (PC-1)
}

#[test]
fn cpu_jsr_indexed_indirect_jumps_through_pb_relative_pointer_and_pushes_return() {
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.e = false;
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;
    cpu.sp = 0x1FFF;
    cpu.x = 0x0004;

    // JSR ($0010,X) -> pointer at $7E:0014 -> target $8042.
    wram.write_u8(0x7E0000, 0xFC).unwrap();
    wram.write_u8(0x7E0001, 0x10).unwrap();
    wram.write_u8(0x7E0002, 0x00).unwrap();
    wram.write_u8(0x7E0014, 0x42).unwrap();
    wram.write_u8(0x7E0015, 0x80).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.pc, 0x8042, "must jump through the X-indexed pointer in the program bank");
    assert_eq!(cpu.pb, 0x7E, "JSR (addr,X) never changes the program bank");
    assert_eq!(cpu.sp, 0x1FFD, "must push a 16-bit return address");
    // Return address = last byte of the 3-byte instruction ($0002),
    // so RTS (which adds 1) resumes at $0003.
    assert_eq!(wram.read_u8(0x7E1FFE).unwrap(), 0x02);
    assert_eq!(wram.read_u8(0x7E1FFF).unwrap(), 0x00);
}

#[test]
fn jmp_indirect_0x6c_reads_pointer_from_bank_0_not_db() {
    // Real 65816 hardware always fetches the JMP ($addr) pointer from
    // bank 0, regardless of DB -- a 6502-inherited quirk. Set DB to a
    // bank that isn't mapped in this test's Wram-only bus, so the old
    // (buggy) `db`-based address would hit an invalid-address error
    // instead of silently succeeding.
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;
    cpu.db = 0x01;

    wram.write_u8(0x7E0000, 0x6C).unwrap(); // JMP ($0010)
    wram.write_u8(0x7E0001, 0x10).unwrap();
    wram.write_u8(0x7E0002, 0x00).unwrap();
    // Pointer target lives at bank-0 $0010/$0011, which mirrors WRAM's
    // low 8KB -- i.e. the same bytes as $7E0010/$7E0011.
    wram.write_u8(0x7E0010, 0x34).unwrap();
    wram.write_u8(0x7E0011, 0x12).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.pc, 0x1234, "JMP ($addr) must read its pointer from bank 0, not DB");
}

#[test]
fn jmp_indirect_x_0x7c_reads_pointer_from_pb_not_db() {
    // JMP ($addr,X) is a same-bank computed jump: its pointer must be
    // fetched from the current Program Bank (PB), not DB. Set DB to a
    // bank that isn't mapped in this test's Wram-only bus, so the old
    // (buggy) `db`-based address would hit an invalid-address error
    // instead of silently succeeding.
    let mut cpu = Cpu::new();
    let mut wram = Wram::new();
    cpu.pb = 0x7E;
    cpu.pc = 0x0000;
    cpu.db = 0x01;
    cpu.x = 0x0005;

    wram.write_u8(0x7E0000, 0x7C).unwrap(); // JMP ($0010,X)
    wram.write_u8(0x7E0001, 0x10).unwrap();
    wram.write_u8(0x7E0002, 0x00).unwrap();
    // Effective pointer is $0010 + X ($0005) = $0015, read from PB ($7E).
    wram.write_u8(0x7E0015, 0x78).unwrap();
    wram.write_u8(0x7E0016, 0x56).unwrap();

    cpu.step(&mut wram).unwrap();
    assert_eq!(cpu.pc, 0x5678, "JMP ($addr,X) must read its pointer from PB, not DB");
}
