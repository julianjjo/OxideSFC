//! Color conversion, color math and the pseudo-hires main/sub blend.

use super::common::{make_2bpp_tile, oam_empty};
use crate::cgram::Cgram;
use crate::oam::Oam;
use crate::ppu::PpuRegisters;
use crate::renderer::color::bgr555_to_rgb8;
use crate::renderer::compose::render_frame;
use crate::renderer::SCREEN_WIDTH;
use crate::vram::Vram;

#[test]
fn bgr555_to_rgb8_matches_known_values() {
    // Pure red (R=31,G=0,B=0) -> (255,0,0) after 5->8 bit expansion.
    assert_eq!(bgr555_to_rgb8(0x001F), (255, 0, 0));
    // Pure green (G=31) -> (0,255,0).
    assert_eq!(bgr555_to_rgb8(0x03E0), (0, 255, 0));
    // Pure blue (B=31) -> (0,0,255).
    assert_eq!(bgr555_to_rgb8(0x7C00), (0, 0, 255));
    // Black.
    assert_eq!(bgr555_to_rgb8(0x0000), (0, 0, 0));
}

#[test]
fn color_math_add_half_with_fixed_color_blends_the_backdrop() {
    // The regression this whole subscreen/color-math path fixes:
    // layers that real hardware only shows blended (SMW's title
    // background) used to render as harsh, fully-opaque tile noise
    // because color math was ignored entirely. Here the backdrop
    // (CGRAM 0) is the only thing on screen, color math is enabled for
    // the backdrop layer (CGADSUB bit 5) in add+half mode against a
    // fixed COLDATA color, and the output pixel must be the blended
    // result, not the raw backdrop.
    let vram = Vram::new();
    let mut cgram = Cgram::new();
    let oam = Oam::new();

    // Backdrop (CGRAM index 0) = BGR555 (r=10, g=20, b=0) = 0x028A.
    let backdrop_color: u16 = 10 | (20 << 5);
    cgram.write(0, (backdrop_color & 0xFF) as u8);
    cgram.write(1, (backdrop_color >> 8) as u8);

    let mut regs = PpuRegisters {
        inidisp: 0x0F, // full brightness, not blanked
        bgmode: 1,
        tm: 0x00, // nothing on the main screen except the backdrop
        ..Default::default()
    };
    // Color math: enable on backdrop (bit5), add (bit7=0), half (bit6).
    regs.cgadsub = 0x20 | 0x40;
    regs.cgwsel = 0x00; // blend with fixed COLDATA, not the subscreen
    // Fixed color = BGR555 (r=6, g=4, b=0).
    regs.coldata = 6 | (4 << 5);

    // Expected: per channel (backdrop + fixed) >> 1, clamped 0..31.
    let er = (10 + 6) >> 1; // 8
    let eg = (20 + 4) >> 1; // 12
    let eb = 0 >> 1; // 0
    let expected = bgr555_to_rgb8((er as u16) | ((eg as u16) << 5) | ((eb as u16) << 10));

    let fb = render_frame(&vram, &cgram, &oam, &regs);
    assert_eq!((fb[0], fb[1], fb[2]), expected,
        "backdrop must be color-math-blended with the fixed color (add, half)");

    // With color math disabled, the same pixel must be the raw backdrop.
    regs.cgadsub = 0x00;
    let fb2 = render_frame(&vram, &cgram, &oam, &regs);
    let raw = bgr555_to_rgb8(backdrop_color);
    assert_eq!((fb2[0], fb2[1], fb2[2]), raw,
        "with color math off, the backdrop must render unmodified");
}

#[test]
fn color_math_exempts_sprites_on_obj_palettes_0_to_3() {
    // Hardware rule (fullsnes "Color Math"): CGADSUB bit 4 enables
    // math only for sprite pixels using OBJ palettes 4-7; palettes 0-3
    // are NEVER blended. Character sprites (Mario, enemies) live on
    // the low palettes precisely so a game can make pal-4-7 effect
    // sprites (bubbles, spotlights) translucent without washing out
    // the actors -- blending everything made characters look wrong
    // anywhere OBJ math was on (e.g. SMW's overworld, CGADSUB=$30).
    let mut vram = Vram::new();
    let mut cgram = Cgram::new();
    let mut oam = Oam::new();

    // One tile, all 64 pixels = value 1.
    let tile = make_2bpp_tile([[1; 8]; 8]);
    for (i, &b) in tile.iter().enumerate() {
        vram.write(i as u16, b);
    }

    // Sprite 0: palette 0 (math-exempt). Sprite 1: palette 4 (blended).
    // Both 8x8, at y=10, x=20 and x=40. (byte 0 = X, byte 1 = Y)
    oam.write(0, 20);
    oam.write(1, 10);
    oam.write(2, 0);
    oam.write(3, 0x00); // palette 0
    oam.write(4, 40);
    oam.write(5, 10);
    oam.write(6, 0);
    oam.write(7, 0x08); // attrs bit3-1 = 100 -> palette 4
    oam.write(512, 0x00);

    // Both palettes' color 1 = the same mid red (r=16).
    let sprite_color: u16 = 16;
    for pal in [0usize, 4] {
        let e = 128 + pal * 16 + 1;
        cgram.write((e * 2) as u16, (sprite_color & 0xFF) as u8);
        cgram.write((e * 2 + 1) as u16, (sprite_color >> 8) as u8);
    }

    let mut regs = PpuRegisters {
        inidisp: 0x0F,
        obsel: 0x00,
        tm: 0x10, // sprites only on main
        ..Default::default()
    };
    // Math: enable on OBJ (bit4), add, full. Operand = fixed color,
    // pure green (g=20) so the blended pixel visibly changes.
    regs.cgadsub = 0x10;
    regs.cgwsel = 0x00;
    regs.coldata = 20 << 5;

    let fb = render_frame(&vram, &cgram, &oam, &regs);

    let raw = bgr555_to_rgb8(sprite_color);
    let blended = bgr555_to_rgb8(sprite_color | (20 << 5));
    let pal0_idx = (10 * SCREEN_WIDTH + 20) * 4;
    let pal4_idx = (10 * SCREEN_WIDTH + 40) * 4;
    assert_eq!(
        (fb[pal0_idx], fb[pal0_idx + 1], fb[pal0_idx + 2]),
        raw,
        "a palette-0 sprite pixel must NOT be color-mathed even with CGADSUB bit 4 set"
    );
    assert_eq!(
        (fb[pal4_idx], fb[pal4_idx + 1], fb[pal4_idx + 2]),
        blended,
        "a palette-4 sprite pixel MUST be color-mathed when CGADSUB bit 4 is set"
    );
}

#[test]
fn pseudo_hires_averages_main_and_subscreen_pixels() {
    // SETINI bit 3 (pseudo-hires): hardware interleaves subscreen
    // pixels on even half-dots and main-screen pixels on odd ones; on
    // this fixed 256-wide raster that collapses to averaging the two
    // (the same collapse the true hi-res modes use).
    let mut vram = Vram::new();
    let mut cgram = Cgram::new();

    // 4bpp tile 1: solid pixel value 1.
    for row in 0..8u16 {
        vram.write(32 + row * 2, 0xFF);
    }
    // BG1 tilemap at word 0x400: tile 1, palette 0 -> color 1 (red).
    vram.write(0x800, 0x01);
    vram.write(0x801, 0x00);
    // BG2 tilemap at word 0x800: tile 1, palette 1 -> color 17 (blue).
    vram.write(0x1000, 0x01);
    vram.write(0x1001, 0x04);

    cgram.write(2, 0x1F); // color 1 = pure red (BGR555 0x001F)
    cgram.write(2 + 1, 0x00);
    cgram.write(17 * 2, 0x00); // color 17 = pure blue (BGR555 0x7C00)
    cgram.write(17 * 2 + 1, 0x7C);

    let mut regs = PpuRegisters {
        inidisp: 0x0F,
        bgmode: 1,
        tm: 0x01, // main screen: BG1 (red)
        ts: 0x02, // subscreen: BG2 (blue)
        setini: 0x08, // pseudo-hires
        ..Default::default()
    };
    regs.bg_sc[0] = 0x04; // BG1 map at word 0x400
    regs.bg_sc[1] = 0x08; // BG2 map at word 0x800

    let fb = render_frame(&vram, &cgram, &oam_empty(), &regs);
    // avg(red 0x001F, blue 0x7C00) = r 15, g 0, b 15 = 0x3C0F.
    let expected = bgr555_to_rgb8(0x3C0F);
    assert_eq!((fb[0], fb[1], fb[2]), expected, "pseudo-hires must blend main and subscreen");

    // Without the bit, only the main screen shows.
    regs.setini = 0x00;
    let fb = render_frame(&vram, &cgram, &oam_empty(), &regs);
    let expected = bgr555_to_rgb8(0x001F);
    assert_eq!((fb[0], fb[1], fb[2]), expected, "without SETINI bit 3 the subscreen stays hidden");
}
