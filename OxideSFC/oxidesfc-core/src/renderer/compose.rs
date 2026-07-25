//! Frame composition: the public entry points, the per-scanline band
//! splitter, and the main/sub screen compositing pass that applies windows,
//! color math and brightness to produce RGBA8888 output.

use super::background::draw_bg_layer;
use super::color::{average_bgr555, bgr555_to_rgb8, color_math};
use super::mode7::draw_mode7_layer;
use super::sprites::{draw_sprites, evaluate_sprites, SpriteEval};
use super::window::window_line;
use super::{bg_depths, LAYER_BACKDROP, SCREEN_HEIGHT, SCREEN_WIDTH};
use crate::cgram::Cgram;
use crate::oam::Oam;
use crate::ppu::PpuRegisters;
use crate::vram::Vram;

/// Renders one full frame to an RGBA8888 buffer (`SCREEN_WIDTH *
/// SCREEN_HEIGHT * 4` bytes, row-major, opaque alpha) with a single
/// register state applied to every scanline. Use
/// `render_frame_per_scanline` when per-line register snapshots are
/// available -- games routinely change registers mid-frame (raster IRQ
/// splits, HDMA), which this whole-frame entry point cannot represent.
pub fn render_frame(vram: &Vram, cgram: &Cgram, oam: &Oam, regs: &PpuRegisters) -> Vec<u8> {
    let n = SCREEN_WIDTH * SCREEN_HEIGHT;
    let mut fb = vec![0u8; n * 4];
    let mut scratch = BandScratch::new();
    let _ = render_band(&mut fb, &mut scratch, 0, SCREEN_HEIGHT, vram, cgram, oam, regs);
    fb
}

/// Renders one full frame from PER-SCANLINE register snapshots
/// (`lines[y]` = the register state in effect on scanline `y`).
/// Consecutive scanlines with identical registers are rendered as one
/// band, so the common case (a handful of raster splits per frame -- e.g.
/// SMW's IRQ-driven status bar, where BG3 scroll changes at the bar's
/// bottom edge) costs barely more than a single whole-frame pass, while
/// per-line HDMA effects (scroll waves, COLDATA gradients) degrade
/// gracefully to per-line bands.
pub fn render_frame_per_scanline(
    vram: &Vram,
    cgram: &Cgram,
    oam: &Oam,
    lines: &[PpuRegisters],
) -> Vec<u8> {
    render_frame_per_scanline_with_status(vram, cgram, oam, lines).0
}

/// Like `render_frame_per_scanline`, but also returns the frame's STAT77
/// sprite-overflow flags (bit 6 = range over: more than 32 sprites on
/// some line; bit 7 = time over: more than 34 8-pixel sprite tiles on
/// some line), accumulated by the per-line sprite evaluation across every
/// rendered band. `SystemBus::render_frame` feeds these into $213E.
pub fn render_frame_per_scanline_with_status(
    vram: &Vram,
    cgram: &Cgram,
    oam: &Oam,
    lines: &[PpuRegisters],
) -> (Vec<u8>, u8) {
    render_lines(vram, oam, lines, |_| cgram)
}

/// Like `render_frame_per_scanline_with_status`, but with a PER-SCANLINE
/// CGRAM snapshot as well (`cgram_lines[y]` = the palette in effect on
/// scanline `y`). Games routinely rewrite palette entries mid-frame via
/// HDMA to $2121/$2122 -- Prince of Persia 2 repaints backdrop color 0
/// every line for its sky gradient, then restores it in vblank, so a
/// single end-of-frame CGRAM paints the whole sky one flat color. A band
/// split happens wherever the registers OR the palette change.
pub fn render_frame_per_scanline_with_cgram(
    vram: &Vram,
    cgram_lines: &[Cgram],
    oam: &Oam,
    lines: &[PpuRegisters],
) -> (Vec<u8>, u8) {
    assert_eq!(
        cgram_lines.len(),
        SCREEN_HEIGHT,
        "one CGRAM snapshot per visible scanline is required"
    );
    render_lines(vram, oam, lines, |y| &cgram_lines[y])
}

/// Shared band-splitting core: renders consecutive scanlines as one band
/// while both the register snapshot and the line's CGRAM stay identical.
fn render_lines<'a>(
    vram: &Vram,
    oam: &Oam,
    lines: &[PpuRegisters],
    cgram_for_line: impl Fn(usize) -> &'a Cgram,
) -> (Vec<u8>, u8) {
    assert_eq!(
        lines.len(),
        SCREEN_HEIGHT,
        "one register snapshot per visible scanline is required"
    );
    let n = SCREEN_WIDTH * SCREEN_HEIGHT;
    let mut fb = vec![0u8; n * 4];
    let mut scratch = BandScratch::new();
    let mut range_time_over = 0u8;

    let mut band_start = 0usize;
    for y in 1..=SCREEN_HEIGHT {
        if y == SCREEN_HEIGHT
            || lines[y] != lines[band_start]
            || cgram_for_line(y).as_slice() != cgram_for_line(band_start).as_slice()
        {
            range_time_over |= render_band(
                &mut fb,
                &mut scratch,
                band_start,
                y,
                vram,
                cgram_for_line(band_start),
                oam,
                &lines[band_start],
            );
            band_start = y;
        }
    }
    (fb, range_time_over)
}

/// Full-screen-sized compositing buffers reused across bands so a
/// many-band frame doesn't reallocate per band.
struct BandScratch {
    main: Vec<u16>,
    main_layer: Vec<u8>,
    sub: Vec<u16>,
    sub_layer: Vec<u8>,
}

impl BandScratch {
    fn new() -> Self {
        let n = SCREEN_WIDTH * SCREEN_HEIGHT;
        BandScratch {
            main: vec![0; n],
            main_layer: vec![LAYER_BACKDROP; n],
            sub: vec![0; n],
            sub_layer: vec![LAYER_BACKDROP; n],
        }
    }
}

/// Composites scanlines `y0..y1` of the frame into `fb` using one
/// register state: main screen (TM layers) plus, when color math wants
/// it, the subscreen (TS layers), blended per CGADSUB/CGWSEL. All math is
/// done in native 5-bit-per-channel BGR555 space, matching the hardware,
/// then converted to RGB888 with the INIDISP master-brightness scale.
fn render_band(
    fb: &mut [u8],
    scratch: &mut BandScratch,
    y0: usize,
    y1: usize,
    vram: &Vram,
    cgram: &Cgram,
    oam: &Oam,
    regs: &PpuRegisters,
) -> u8 {
    if (regs.inidisp & 0x80) != 0 {
        // Forced blank: real hardware outputs solid black (and evaluates
        // no sprites, so no range/time-over flags either).
        for px in fb[y0 * SCREEN_WIDTH * 4..y1 * SCREEN_WIDTH * 4].chunks_exact_mut(4) {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
            px[3] = 0xFF;
        }
        return 0;
    }

    // Hardware sprite evaluation: which sprites survive the 32-per-line /
    // 34-tiles-per-line limits on each of this band's lines (shared by
    // the main- and subscreen passes -- it depends only on OAM and OBSEL).
    let sprite_eval = evaluate_sprites(oam, regs, y0, y1);

    let backdrop = cgram.read_color(0) & 0x7FFF;

    // Main screen: backdrop, then TM-enabled layers back-to-front. TMW
    // selects which layers get window-masked on the main screen.
    for i in y0 * SCREEN_WIDTH..y1 * SCREEN_WIDTH {
        scratch.main[i] = backdrop;
        scratch.main_layer[i] = LAYER_BACKDROP;
    }
    render_layers(&mut scratch.main, &mut scratch.main_layer, regs.tm, regs.tmw, vram, cgram, oam, regs, &sprite_eval, y0, y1);

    // Subscreen: needed when color math blends with it (CGWSEL bit 1) --
    // otherwise the fixed COLDATA color is the second operand -- and in
    // pseudo-hires mode (SETINI bit 3), where hardware interleaves
    // subscreen pixels on the even half-dots. TSW is the subscreen's
    // window-mask selector.
    let pseudo_hires = regs.setini & 0x08 != 0 && (regs.bgmode & 0x07) < 5;
    let use_subscreen = regs.cgwsel & 0x02 != 0;
    // The SUB screen's backdrop is the fixed COLDATA color, not CGRAM color
    // 0 -- only the MAIN screen's backdrop is CGRAM 0 (bsnes:
    // `belowColor = hires ? cgram[0] : io.col.fixedColor`). This was CGRAM 0
    // for both, so every subscreen-based translucency effect (water, glass,
    // fog, shadows, spotlights, HUD overlays) blended against the wrong
    // operand wherever TS left the subscreen empty -- which is most of the
    // screen in most such effects.
    let hires = pseudo_hires || matches!(regs.bgmode & 0x07, 5 | 6);
    let sub_backdrop = if hires { backdrop } else { regs.coldata & 0x7FFF };
    if use_subscreen || pseudo_hires {
        for i in y0 * SCREEN_WIDTH..y1 * SCREEN_WIDTH {
            scratch.sub[i] = sub_backdrop;
            scratch.sub_layer[i] = LAYER_BACKDROP;
        }
        render_layers(&mut scratch.sub, &mut scratch.sub_layer, regs.ts, regs.tsw, vram, cgram, oam, regs, &sprite_eval, y0, y1);
    }

    // Only the 6 per-layer enable bits participate in the layer gate --
    // bits 6/7 are the half/subtract mode flags, and masking them keeps
    // the LAYER_OBJ_PAL03 pseudo-layer (id 6) permanently math-exempt.
    let math_enable = regs.cgadsub & 0x3F;
    let subtract = regs.cgadsub & 0x80 != 0;
    let half = regs.cgadsub & 0x40 != 0;
    let fixed = regs.coldata & 0x7FFF;
    let brightness = (regs.inidisp & 0x0F) as u32;

    // The color window (CGWSEL bits 4-7): per-region "force main screen
    // black" and "allow color math" controls, both keyed off the layer-5
    // window shape (WOBJSEL's high nibble).
    let math_window = window_line(regs, 5);
    let clip_mode = (regs.cgwsel >> 6) & 0x03;
    let prevent_mode = (regs.cgwsel >> 4) & 0x03;

    for i in y0 * SCREEN_WIDTH..y1 * SCREEN_WIDTH {
        let inside = math_window[i % SCREEN_WIDTH];
        // Force-black region: 0=never, 1=outside the window, 2=inside,
        // 3=always. A clipped pixel becomes black BEFORE color math (this
        // is how games do window-shaped darkening/spotlights).
        let force_black = match clip_mode {
            0 => false,
            1 => !inside,
            2 => inside,
            _ => true,
        };
        // Math-allowed region: 0=always, 1=inside, 2=outside, 3=never.
        let math_allowed = match prevent_mode {
            0 => true,
            1 => inside,
            2 => !inside,
            _ => false,
        };
        let main_color = if force_black { 0 } else { scratch.main[i] };
        let mut color = main_color;
        // Color math applies only if enabled for this pixel's source layer.
        if math_allowed && math_enable & (1 << scratch.main_layer[i]) != 0 {
            let operand = if use_subscreen { scratch.sub[i] } else { fixed };
            // Hardware skips the halve in two cases that `half` alone
            // doesn't capture (bsnes `PPU::Line::pixel`: `io.col.halve &&
            // windowAbove[x] && below.source != Source::COL`): when the main
            // pixel was clipped to black by the color window, and when the
            // subscreen operand is the backdrop rather than a real layer.
            // Applying it unconditionally halved effects hardware leaves at
            // full intensity.
            let halve = half
                && !force_black
                && (!use_subscreen || scratch.sub_layer[i] != LAYER_BACKDROP);
            color = color_math(main_color, operand, subtract, halve);
        }
        // Pseudo-hires (SETINI bit 3): hardware outputs the subscreen on
        // even half-dots and the main screen on odd ones -- on this fixed
        // 256-wide raster that collapses to averaging the two, the same
        // way the true hi-res modes collapse their dot pairs.
        if pseudo_hires {
            color = average_bgr555(&[color, scratch.sub[i]]);
        }

        let (mut r, mut g, mut b) = bgr555_to_rgb8(color);
        if brightness < 15 {
            r = ((r as u32 * brightness) / 15) as u8;
            g = ((g as u32 * brightness) / 15) as u8;
            b = ((b as u32 * brightness) / 15) as u8;
        }
        let o = i * 4;
        fb[o] = r;
        fb[o + 1] = g;
        fb[o + 2] = b;
        fb[o + 3] = 0xFF;
    }

    sprite_eval.range_time_over
}

/// One entry in a mode's back-to-front compositing order: either a BG
/// layer restricted to tiles of a given priority bit (0 or 1), or the
/// sprites of a given OBJ priority level (0-3).
#[derive(Clone, Copy)]
enum DrawOp {
    /// (bg index 0-3, tile priority bit 0 or 1)
    Bg(usize, u8),
    /// OBJ priority level 0-3
    Obj(u8),
}

/// The back-to-front draw order for a BG mode, honoring per-tile BG
/// priority and per-sprite OBJ priority. Verified against
/// wiki.superfamicom.org/backgrounds. Only the layers actually enabled in
/// the screen mask are drawn; a later (frontmost) opaque pixel overwrites
/// an earlier one, so listing back-to-front and overwriting gives the
/// correct front-to-back result.
fn composite_order(mode: u8, bg3_priority: bool) -> &'static [DrawOp] {
    use DrawOp::{Bg, Obj};
    match mode {
        // Mode 0: four BGs (BG1/BG2 vs BG3/BG4 priority pairs).
        0 => &[
            Bg(3, 0), Bg(2, 0), Obj(0), Bg(3, 1), Bg(2, 1), Obj(1),
            Bg(1, 0), Bg(0, 0), Obj(2), Bg(1, 1), Bg(0, 1), Obj(3),
        ],
        // Mode 1: three BGs, with BG3 either frontmost (bit3 set) or near
        // the back (bit3 clear). This is what SMW's title screen and most
        // of its gameplay use.
        1 => {
            if bg3_priority {
                &[
                    Bg(2, 0), Obj(0), Obj(1), Bg(1, 0), Bg(0, 0), Obj(2),
                    Bg(1, 1), Bg(0, 1), Obj(3), Bg(2, 1),
                ]
            } else {
                &[
                    Bg(2, 0), Obj(0), Bg(2, 1), Obj(1), Bg(1, 0), Bg(0, 0),
                    Obj(2), Bg(1, 1), Bg(0, 1), Obj(3),
                ]
            }
        }
        // Modes 2-6: two BGs. Generic priority-interleaved order (exact
        // per-mode tables vary slightly but this matches the common
        // BG1-over-BG2, sprites-interleaved-by-priority arrangement).
        _ => &[
            Bg(1, 0), Obj(0), Bg(0, 0), Obj(1), Bg(1, 1), Obj(2), Bg(0, 1), Obj(3),
        ],
    }
}

/// Draws every layer enabled in `mask` (TM for the main screen, TS for the
/// subscreen) into `buf` (BGR555 per pixel), in the mode's correct
/// back-to-front per-tile/per-sprite priority order, recording each
/// written pixel's source layer in `layer_buf`. Transparent (palette
/// index 0) pixels are skipped, leaving whatever is beneath.
fn render_layers(
    buf: &mut [u16],
    layer_buf: &mut [u8],
    mask: u8,
    window_mask: u8,
    vram: &Vram,
    cgram: &Cgram,
    oam: &Oam,
    regs: &PpuRegisters,
    sprite_eval: &SpriteEval,
    y0: usize,
    y1: usize,
) {
    // Per-layer window skip masks: a `true` at X means "don't draw this
    // layer's pixel there". Only layers selected in TMW/TSW (passed as
    // `window_mask`) are masked at all.
    let skip: [[bool; SCREEN_WIDTH]; 5] = std::array::from_fn(|layer| {
        if window_mask & (1 << layer) != 0 {
            window_line(regs, layer)
        } else {
            [false; SCREEN_WIDTH]
        }
    });

    let mode = regs.bgmode & 0x07;
    if mode == 7 {
        // Mode 7: BG1 is the affine layer; with SETINI's EXTBG bit, BG2
        // shows the same playing field split by pixel bit 7 into two
        // priority slots. Back-to-front order per fullsnes's mode-7
        // priority chart: BG2(lo), OBJ0, BG1, OBJ1, BG2(hi), OBJ2, OBJ3.
        let extbg = regs.setini & 0x40 != 0;
        if extbg && mask & 0x02 != 0 {
            draw_mode7_layer(buf, layer_buf, vram, cgram, regs, true, 0, &skip[1], y0, y1);
        }
        if mask & 0x10 != 0 {
            draw_sprites(buf, layer_buf, vram, cgram, oam, regs, sprite_eval, 0, &skip[4], y0, y1);
        }
        if mask & 0x01 != 0 {
            draw_mode7_layer(buf, layer_buf, vram, cgram, regs, false, 0, &skip[0], y0, y1);
        }
        if mask & 0x10 != 0 {
            draw_sprites(buf, layer_buf, vram, cgram, oam, regs, sprite_eval, 1, &skip[4], y0, y1);
        }
        if extbg && mask & 0x02 != 0 {
            draw_mode7_layer(buf, layer_buf, vram, cgram, regs, true, 1, &skip[1], y0, y1);
        }
        if mask & 0x10 != 0 {
            draw_sprites(buf, layer_buf, vram, cgram, oam, regs, sprite_eval, 2, &skip[4], y0, y1);
            draw_sprites(buf, layer_buf, vram, cgram, oam, regs, sprite_eval, 3, &skip[4], y0, y1);
        }
        return;
    }

    let depths = bg_depths(mode);
    let bg3_priority = regs.bgmode & 0x08 != 0;
    for op in composite_order(mode, bg3_priority) {
        match *op {
            DrawOp::Bg(bg, tile_priority) => {
                if let Some(depth) = depths[bg] {
                    if mask & (1 << bg) != 0 {
                        draw_bg_layer(buf, layer_buf, vram, cgram, regs, bg, depth, tile_priority, &skip[bg], y0, y1);
                    }
                }
            }
            DrawOp::Obj(prio) => {
                if mask & 0x10 != 0 {
                    draw_sprites(buf, layer_buf, vram, cgram, oam, regs, sprite_eval, prio, &skip[4], y0, y1);
                }
            }
        }
    }
}

