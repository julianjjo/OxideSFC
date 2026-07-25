//! The mode-7 multiplier and the CPU's hardware multiply/divide unit, plus
//! WRIO readback.

use crate::bus::{MemoryBus, SystemBus};

#[test]
fn mpy_recomputes_on_m7a_writes_too() {
    // The mode-7 multiplier is combinational on M7A and M7B's high
    // byte: writing M7A alone must refresh MPY (snes9x recomputes on
    // both $211B and $211C writes).
    let mut bus = SystemBus::new();
    bus.write_u8(0x00211C, 0x03).unwrap(); // M7B byte = 3
    bus.write_u8(0x00211B, 0xFE).unwrap(); // M7A = -2, low then high
    bus.write_u8(0x00211B, 0xFF).unwrap();
    let lo = bus.read_u8(0x002134).unwrap() as u32;
    let mid = bus.read_u8(0x002135).unwrap() as u32;
    let hi = bus.read_u8(0x002136).unwrap() as u32;
    let result = (((lo | (mid << 8) | (hi << 16)) << 8) as i32) >> 8;
    assert_eq!(result, -6, "MPY must refresh from the M7A write: -2 * 3 = -6");
}

#[test]
fn mode7_multiplier_reports_signed_product_at_2134_2136() {
    let mut bus = SystemBus::new();
    // M7A = -2 (0xFFFE), written low-then-high through the M7 latch.
    bus.write_u8(0x00211B, 0xFE).unwrap();
    bus.write_u8(0x00211B, 0xFF).unwrap();
    // Writing M7B's byte triggers the multiply: -2 * 3 = -6.
    bus.write_u8(0x00211C, 0x03).unwrap();
    let lo = bus.read_u8(0x002134).unwrap() as u32;
    let mid = bus.read_u8(0x002135).unwrap() as u32;
    let hi = bus.read_u8(0x002136).unwrap() as u32;
    let raw = lo | (mid << 8) | (hi << 16);
    // Sign-extend the 24-bit result.
    let result = ((raw << 8) as i32) >> 8;
    assert_eq!(result, -6, "MPY must be the signed product M7A * M7B-byte");
}

#[test]
fn mode7_matrix_registers_latch_low_then_high() {
    let mut bus = SystemBus::new();
    bus.write_u8(0x00211B, 0x34).unwrap(); // M7A low
    bus.write_u8(0x00211B, 0x12).unwrap(); // M7A high
    assert_eq!(bus.ppu_registers().m7a, 0x1234);

    // The latch is shared: an M7D pair written after M7A must not
    // inherit stale bytes.
    bus.write_u8(0x00211E, 0x78).unwrap();
    bus.write_u8(0x00211E, 0x56).unwrap();
    assert_eq!(bus.ppu_registers().m7d, 0x5678);

    // M7X is 13-bit.
    bus.write_u8(0x00211F, 0xFF).unwrap();
    bus.write_u8(0x00211F, 0xFF).unwrap();
    assert_eq!(bus.ppu_registers().m7x, 0x1FFF);

    // $210D doubles as M7HOFS through the M7 latch.
    bus.write_u8(0x00210D, 0xCD).unwrap();
    bus.write_u8(0x00210D, 0x0A).unwrap();
    assert_eq!(bus.ppu_registers().m7_hofs, 0x0ACD);
}

#[test]
fn hardware_multiplier_produces_product_in_rdmpy() {
    let mut bus = SystemBus::new();
    bus.write_u8(0x004202, 0xA7).unwrap(); // WRMPYA = 167
    bus.write_u8(0x004203, 0x3B).unwrap(); // WRMPYB = 59 -> starts the multiply
    let lo = bus.read_u8(0x004216).unwrap() as u16;
    let hi = bus.read_u8(0x004217).unwrap() as u16;
    assert_eq!((hi << 8) | lo, 167 * 59, "RDMPY must hold the unsigned 8x8 product");
}

#[test]
fn hardware_divider_produces_quotient_and_remainder() {
    let mut bus = SystemBus::new();
    bus.write_u8(0x004204, 0x39).unwrap(); // WRDIVL: dividend = 0x1239 = 4665
    bus.write_u8(0x004205, 0x12).unwrap(); // WRDIVH
    bus.write_u8(0x004206, 0x07).unwrap(); // divisor 7 -> starts the divide
    let q = (bus.read_u8(0x004215).unwrap() as u16) << 8 | bus.read_u8(0x004214).unwrap() as u16;
    let r = (bus.read_u8(0x004217).unwrap() as u16) << 8 | bus.read_u8(0x004216).unwrap() as u16;
    assert_eq!(q, 4665 / 7);
    assert_eq!(r, 4665 % 7);
}

#[test]
fn hardware_divide_by_zero_yields_ffff_quotient_and_dividend_remainder() {
    let mut bus = SystemBus::new();
    bus.write_u8(0x004204, 0xCD).unwrap();
    bus.write_u8(0x004205, 0xAB).unwrap();
    bus.write_u8(0x004206, 0x00).unwrap();
    let q = (bus.read_u8(0x004215).unwrap() as u16) << 8 | bus.read_u8(0x004214).unwrap() as u16;
    let r = (bus.read_u8(0x004217).unwrap() as u16) << 8 | bus.read_u8(0x004216).unwrap() as u16;
    assert_eq!(q, 0xFFFF, "divide by zero must yield quotient 0xFFFF (real-hardware behavior)");
    assert_eq!(r, 0xABCD, "divide by zero must yield the dividend as remainder");
}

#[test]
fn wrio_write_reads_back_at_rdio() {
    let mut bus = SystemBus::new();
    bus.write_u8(0x004201, 0x5A).unwrap();
    assert_eq!(bus.read_u8(0x004213).unwrap(), 0x5A, "RDIO must follow the WRIO output latch");
}

