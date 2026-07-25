//! Mode 7's affine transform, screen-over behavior and EXTBG split.

use super::common::write_mode7_tile;
use crate::cgram::Cgram;
use crate::oam::Oam;
use crate::ppu::PpuRegisters;
use crate::renderer::color::bgr555_to_rgb8;
use crate::renderer::compose::render_frame;
use crate::vram::Vram;

#[test]
fn mode7_identity_matrix_renders_the_field_one_to_one() {
    let mut vram = Vram::new();
    let mut cgram = Cgram::new();
    let oam = Oam::new();

    // Map entry (0,0) -> tile 1 (low byte of word 0). Everything else
    // stays tile 0 (all-transparent).
    vram.write(0, 0x01);
    write_mode7_tile(&mut vram, 1, 0x25); // tile 1: every pixel = color 0x25

    // CGRAM color 0x25 = pure green.
    cgram.write(0x25 * 2, 0xE0);
    cgram.write(0x25 * 2 + 1, 0x03);

    let regs = PpuRegisters {
        inidisp: 0x0F,
        bgmode: 7,
        tm: 0x01, // BG1 only
        m7a: 0x0100, // identity matrix (1.0 in 8.8 fixed point)
        m7b: 0x0000,
        m7c: 0x0000,
        m7d: 0x0100,
        ..Default::default()
    };

    let fb = render_frame(&vram, &cgram, &oam, &regs);
    let green = bgr555_to_rgb8(cgram.read_color(0x25));
    let backdrop = bgr555_to_rgb8(cgram.read_color(0));

    // Pixel (0,0) lands inside tile 1's 8x8 area.
    assert_eq!((fb[0], fb[1], fb[2]), green, "identity transform must map screen (0,0) to field (0,0)");
    // Pixel (8,0) is the next map entry (tile 0, transparent) -> backdrop.
    let idx = 8 * 4;
    assert_eq!((fb[idx], fb[idx + 1], fb[idx + 2]), backdrop, "field pixel (8,0) is a transparent tile");
}

#[test]
fn mode7_scaling_matrix_transforms_coordinates() {
    // A = 2.0 doubles the horizontal step: screen x=4 samples field
    // x=8, so tile 1 at map (0,0) (field x 0-7) must NOT cover screen
    // x=4 when scaled, while an unscaled render would.
    let mut vram = Vram::new();
    let mut cgram = Cgram::new();
    let oam = Oam::new();

    vram.write(0, 0x01);
    write_mode7_tile(&mut vram, 1, 0x25);
    cgram.write(0x25 * 2, 0xE0);
    cgram.write(0x25 * 2 + 1, 0x03);

    let regs = PpuRegisters {
        inidisp: 0x0F,
        bgmode: 7,
        tm: 0x01,
        m7a: 0x0200, // 2.0: horizontal zoom OUT (field moves 2px per screen px)
        m7b: 0,
        m7c: 0,
        m7d: 0x0100,
        ..Default::default()
    };

    let fb = render_frame(&vram, &cgram, &oam, &regs);
    let green = bgr555_to_rgb8(cgram.read_color(0x25));
    let backdrop = bgr555_to_rgb8(cgram.read_color(0));

    // Screen x=3 -> field x=6 (still inside tile 1's row 0).
    let inside = 3 * 4;
    assert_eq!((fb[inside], fb[inside + 1], fb[inside + 2]), green);
    // Screen x=4 -> field x=8 (tile 0, transparent -> backdrop).
    let outside = 4 * 4;
    assert_eq!((fb[outside], fb[outside + 1], fb[outside + 2]), backdrop,
        "M7A=2.0 must sample field x=8 at screen x=4");
}

#[test]
fn mode7_screen_over_transparent_vs_wrap() {
    // Point the transform far outside the 1024x1024 field via M7Y and
    // check M7SEL's screen-over modes: wrap (0) shows the field again,
    // transparent (2) shows the backdrop.
    let mut vram = Vram::new();
    let mut cgram = Cgram::new();
    let oam = Oam::new();

    // Fill the WHOLE map with tile 1 so any wrapped coordinate hits it.
    for entry in 0..(128u16 * 128) {
        vram.write(entry * 2, 0x01);
    }
    write_mode7_tile(&mut vram, 1, 0x25);
    cgram.write(0x25 * 2, 0xE0);
    cgram.write(0x25 * 2 + 1, 0x03);

    let mut regs = PpuRegisters {
        inidisp: 0x0F,
        bgmode: 7,
        tm: 0x01,
        m7a: 0x0100,
        m7b: 0,
        m7c: 0,
        m7d: 0x0100,
        ..Default::default()
    };
    // Scroll far negative: field y = -1024 + screen y, outside the field.
    regs.m7_vofs = (-1024i16 as u16) & 0x1FFF;

    // Screen-over 0: wrap -> still shows tile 1's color.
    regs.m7sel = 0x00;
    let fb = render_frame(&vram, &cgram, &oam, &regs);
    let green = bgr555_to_rgb8(cgram.read_color(0x25));
    assert_eq!((fb[0], fb[1], fb[2]), green, "screen-over 0 must wrap");

    // Screen-over 2: transparent -> backdrop.
    regs.m7sel = 0x80;
    let fb2 = render_frame(&vram, &cgram, &oam, &regs);
    let backdrop = bgr555_to_rgb8(cgram.read_color(0));
    assert_eq!((fb2[0], fb2[1], fb2[2]), backdrop, "screen-over 2 must render transparent");
}

#[test]
fn mode7_extbg_splits_bg2_by_pixel_priority_bit() {
    // With SETINI EXTBG, BG2 shows the mode-7 field split by pixel bit
    // 7: high-priority pixels (bit 7 set) draw in FRONT of BG1;
    // low-priority pixels draw behind it. Here BG1 and BG2 are both
    // enabled; the field pixel value 0xA5 has bit 7 set, so BG2's
    // interpretation (color 0x25) must beat BG1's (color 0xA5).
    let mut vram = Vram::new();
    let mut cgram = Cgram::new();
    let oam = Oam::new();

    vram.write(0, 0x01);
    write_mode7_tile(&mut vram, 1, 0xA5); // bit7=1, low 7 bits = 0x25
    cgram.write(0xA5u8 as u16 * 2, 0x1F); // BG1's color: red
    cgram.write(0xA5u8 as u16 * 2 + 1, 0x00);
    cgram.write(0x25 * 2, 0xE0); // BG2's color: green
    cgram.write(0x25 * 2 + 1, 0x03);

    let regs = PpuRegisters {
        inidisp: 0x0F,
        bgmode: 7,
        tm: 0x03, // BG1 + BG2
        setini: 0x40, // EXTBG
        m7a: 0x0100,
        m7b: 0,
        m7c: 0,
        m7d: 0x0100,
        ..Default::default()
    };

    let fb = render_frame(&vram, &cgram, &oam, &regs);
    let green = bgr555_to_rgb8(cgram.read_color(0x25));
    assert_eq!((fb[0], fb[1], fb[2]), green,
        "an EXTBG pixel with bit 7 set must draw its BG2 slot in front of BG1");
}
