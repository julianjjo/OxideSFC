//! Window masks and the color window's force-black region.

use super::common::{oam_empty, solid_bg1_setup};
use crate::renderer::color::bgr555_to_rgb8;
use crate::renderer::compose::render_frame;

#[test]
fn window1_masks_bg1_inside_its_range_when_enabled_via_tmw() {
    let (vram, cgram, mut regs) = solid_bg1_setup();
    // Window 1 covers X 10-20; enable it for BG1 (W12SEL bit 1) and
    // apply on the main screen (TMW bit 0).
    regs.wh0 = 10;
    regs.wh1 = 20;
    regs.w12sel = 0x02;
    regs.tmw = 0x01;

    let fb = render_frame(&vram, &cgram, &oam_empty(), &regs);
    let red = bgr555_to_rgb8(cgram.read_color(1));
    let backdrop = bgr555_to_rgb8(cgram.read_color(0));

    let at = |x: usize| { let i = x * 4; (fb[i], fb[i + 1], fb[i + 2]) };
    assert_eq!(at(5), red, "outside the window the layer draws normally");
    assert_eq!(at(15), backdrop, "inside the window the layer must be masked out");
    assert_eq!(at(25), red, "past the window's right edge the layer draws again");
}

#[test]
fn window1_invert_masks_outside_instead() {
    let (vram, cgram, mut regs) = solid_bg1_setup();
    regs.wh0 = 10;
    regs.wh1 = 20;
    regs.w12sel = 0x03; // W1 enable + invert for BG1
    regs.tmw = 0x01;

    let fb = render_frame(&vram, &cgram, &oam_empty(), &regs);
    let red = bgr555_to_rgb8(cgram.read_color(1));
    let backdrop = bgr555_to_rgb8(cgram.read_color(0));

    let at = |x: usize| { let i = x * 4; (fb[i], fb[i + 1], fb[i + 2]) };
    assert_eq!(at(15), red, "inverted window leaves the in-range region visible");
    assert_eq!(at(5), backdrop, "inverted window masks everything out of range");
}

#[test]
fn two_windows_combine_with_or_logic() {
    let (vram, cgram, mut regs) = solid_bg1_setup();
    regs.wh0 = 10;
    regs.wh1 = 20; // W1: 10-20
    regs.wh2 = 30;
    regs.wh3 = 40; // W2: 30-40
    regs.w12sel = 0x0A; // W1 enable + W2 enable for BG1, no inverts
    regs.wbglog = 0x00; // OR
    regs.tmw = 0x01;

    let fb = render_frame(&vram, &cgram, &oam_empty(), &regs);
    let red = bgr555_to_rgb8(cgram.read_color(1));
    let backdrop = bgr555_to_rgb8(cgram.read_color(0));

    let at = |x: usize| { let i = x * 4; (fb[i], fb[i + 1], fb[i + 2]) };
    assert_eq!(at(15), backdrop, "inside W1 masked");
    assert_eq!(at(35), backdrop, "inside W2 masked");
    assert_eq!(at(25), red, "between the windows stays visible under OR");
}

#[test]
fn window_without_tmw_bit_does_not_mask() {
    let (vram, cgram, mut regs) = solid_bg1_setup();
    regs.wh0 = 10;
    regs.wh1 = 20;
    regs.w12sel = 0x02; // window enabled for BG1...
    regs.tmw = 0x00; // ...but not applied to the main screen

    let fb = render_frame(&vram, &cgram, &oam_empty(), &regs);
    let red = bgr555_to_rgb8(cgram.read_color(1));
    let i = 15 * 4;
    assert_eq!((fb[i], fb[i + 1], fb[i + 2]), red,
        "a window that isn't enabled in TMW must not mask the main screen");
}

#[test]
fn color_window_forces_main_screen_black_in_region() {
    let (vram, cgram, mut regs) = solid_bg1_setup();
    // Color window shape: W1 10-20 via WOBJSEL's high nibble (bit 5 =
    // W1 enable for the color window).
    regs.wh0 = 10;
    regs.wh1 = 20;
    regs.wobjsel = 0x20;
    regs.cgwsel = 0x80; // clip-to-black mode 2: inside the window

    let fb = render_frame(&vram, &cgram, &oam_empty(), &regs);
    let red = bgr555_to_rgb8(cgram.read_color(1));

    let at = |x: usize| { let i = x * 4; (fb[i], fb[i + 1], fb[i + 2]) };
    assert_eq!(at(5), red, "outside the color window the pixel is untouched");
    assert_eq!(at(15), (0, 0, 0), "inside the color window the main screen must be black");
}
