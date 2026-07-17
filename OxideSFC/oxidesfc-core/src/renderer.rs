//! Turns VRAM/CGRAM/OAM contents plus PPU registers into an actual RGBA8888
//! framebuffer. None of this existed before -- DMA could upload real
//! cartridge graphics data into PPU memory, but nothing ever read it back
//! out into pixels, so the emulator could run indefinitely with completely
//! correct internal state and still never produce a single visible pixel.
//!
//! Feature coverage:
//!   - Background modes 0-6 via a shared tile path (8x8 and, per BGMODE
//!     bits 4-7, 16x16 tiles), and mode 7's affine single-layer path
//!     (128x128 map of 8x8 8bpp tiles, the M7A-M7D matrix, M7SEL flips
//!     and screen-over behavior, and the SETINI EXTBG priority-split
//!     second layer).
//!   - Hi-res modes 5/6: BG pixels are sampled in their real 512-dot
//!     horizontal space (16-wide tiles) and collapsed into the fixed
//!     256x224 output raster by averaging each adjacent dot pair; with
//!     SETINI's interlace bit the two field lines are averaged the same
//!     way (the output raster itself stays 256x224 -- the collapse is
//!     the documented equivalent of hardware's dot/field interleave on
//!     a fixed-size framebuffer).
//!   - Windowing, mosaic, per-tile BG priority, per-mode sprite/BG
//!     priority interleaving, color math with subscreen/fixed-color
//!     operands, direct-color mode, both OBJ tile tables, and
//!     per-scanline register bands.

use crate::cgram::Cgram;
use crate::oam::Oam;
use crate::ppu::PpuRegisters;
use crate::vram::Vram;

pub const SCREEN_WIDTH: usize = 256;
pub const SCREEN_HEIGHT: usize = 224;

/// Per-pixel source-layer id, used to decide (via CGADSUB) whether color
/// math applies to a given main-screen pixel. Values line up with
/// CGADSUB's enable bits: BG1-4 = 0-3, sprites (OBJ) = 4, backdrop = 5.
const LAYER_BG1: u8 = 0;
const LAYER_OBJ: u8 = 4;
const LAYER_BACKDROP: u8 = 5;
/// Sprite pixels using OBJ palettes 0-3. Real hardware NEVER applies color
/// math to these -- CGADSUB bit 4 only enables math for sprites on palettes
/// 4-7 (fullsnes "Color Math"). This id is 6, and the math gate masks
/// CGADSUB to its 6 enable bits, so bit 6 (the half-color flag) can never
/// accidentally enable math for it.
const LAYER_OBJ_PAL03: u8 = 6;

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
        if y == SCREEN_HEIGHT || lines[y] != lines[band_start] {
            range_time_over |=
                render_band(&mut fb, &mut scratch, band_start, y, vram, cgram, oam, &lines[band_start]);
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

    // Subscreen: only needed when color math blends with it (CGWSEL bit 1);
    // otherwise the fixed COLDATA color is the second operand. TSW is the
    // subscreen's window-mask selector.
    let use_subscreen = regs.cgwsel & 0x02 != 0;
    if use_subscreen {
        for i in y0 * SCREEN_WIDTH..y1 * SCREEN_WIDTH {
            scratch.sub[i] = backdrop;
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
            color = color_math(main_color, operand, subtract, half);
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

/// Per-scanline sprite evaluation results for one band: bit N of
/// `masks[line - y0]` says sprite N survived the hardware's per-line
/// limits (at most 32 sprites in range, at most 34 8-pixel tiles) on that
/// line, plus the accumulated STAT77 flags (bit 6 = range over, bit 7 =
/// time over). Evaluation walks OAM starting at `PpuRegisters::
/// first_sprite` exactly like the hardware's priority scan (snes9x
/// gfx.cpp `SetupOBJ`).
struct SpriteEval {
    y0: usize,
    masks: Vec<u128>,
    range_time_over: u8,
}

fn evaluate_sprites(oam: &Oam, regs: &PpuRegisters, y0: usize, y1: usize) -> SpriteEval {
    let (small_size, large_size) = sprite_size_pair((regs.obsel >> 5) & 0x07);
    let first = regs.first_sprite & 0x7F;

    // Decode each sprite's screen rectangle once.
    let mut geom = [(0i32, 0i32, 0u32, 0u32); 128];
    for (s, slot) in geom.iter_mut().enumerate() {
        let base = (s * 4) as u16;
        let x_low = oam.read(base);
        let y_raw = oam.read(base + 1);
        let high_table_byte = oam.read(512 + (s as u16) / 4);
        let shift = ((s % 4) * 2) as u8;
        let x_high_bit = (high_table_byte >> shift) & 0x01;
        let size_bit = (high_table_byte >> (shift + 1)) & 0x01;
        let (w, h) = if size_bit != 0 { large_size } else { small_size };
        let x_full = ((x_high_bit as u16) << 8) | (x_low as u16);
        let x: i32 = if x_full & 0x100 != 0 { (x_full as i32) - 512 } else { x_full as i32 };
        let y: i32 = if y_raw >= 0xF0 { (y_raw as i32) - 256 } else { y_raw as i32 };
        *slot = (x, y, w, h);
    }

    let mut masks = vec![0u128; y1 - y0];
    let mut range_time_over = 0u8;
    for line in y0..y1 {
        let ly = line as i32;
        let mut in_range = 0u32;
        let mut tiles = 34i32;
        let mut mask = 0u128;
        for k in 0..128u8 {
            let s = (first.wrapping_add(k) & 0x7F) as usize;
            let (x, y, w, h) = geom[s];
            if ly < y || ly >= y + h as i32 {
                continue;
            }
            if x + (w as i32) <= 0 || x >= SCREEN_WIDTH as i32 {
                continue;
            }
            if in_range >= 32 {
                range_time_over |= 0x40; // range over: a 33rd sprite on this line
                continue;
            }
            in_range += 1;
            if tiles <= 0 {
                range_time_over |= 0x80; // no tile budget left: sprite dropped
                continue;
            }
            tiles -= (w / 8) as i32;
            if tiles < 0 {
                // Budget ran out inside this sprite: flag time-over. (Real
                // hardware truncates the sprite's trailing tiles; drawing
                // it whole keeps this simple and errs on the visible side.)
                range_time_over |= 0x80;
            }
            mask |= 1u128 << s;
        }
        masks[line - y0] = mask;
    }
    SpriteEval { y0, masks, range_time_over }
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

/// Computes one scanline's combined window membership for a maskable
/// layer: `layer` 0-3 = BG1-4, 4 = OBJ, 5 = the color/math window.
/// `true` at an X means "this pixel is inside the (combined) window
/// area". Windows are purely horizontal on real hardware, so one line
/// serves every scanline of a band. Enable/invert bits come from
/// W12SEL/W34SEL/WOBJSEL (2 bits per window per layer) and the two-window
/// combine operation from WBGLOG/WOBJLOG (OR/AND/XOR/XNOR). With neither
/// window enabled the result is all-false (nothing masked).
fn window_line(regs: &PpuRegisters, layer: usize) -> [bool; SCREEN_WIDTH] {
    let (sel, log) = match layer {
        0 => (regs.w12sel & 0x0F, regs.wbglog & 0x03),
        1 => (regs.w12sel >> 4, (regs.wbglog >> 2) & 0x03),
        2 => (regs.w34sel & 0x0F, (regs.wbglog >> 4) & 0x03),
        3 => (regs.w34sel >> 4, (regs.wbglog >> 6) & 0x03),
        4 => (regs.wobjsel & 0x0F, regs.wobjlog & 0x03),
        _ => (regs.wobjsel >> 4, (regs.wobjlog >> 2) & 0x03),
    };
    let w1_inv = sel & 0x01 != 0;
    let w1_en = sel & 0x02 != 0;
    let w2_inv = sel & 0x04 != 0;
    let w2_en = sel & 0x08 != 0;

    let mut out = [false; SCREEN_WIDTH];
    if !w1_en && !w2_en {
        return out;
    }
    for (x, slot) in out.iter_mut().enumerate() {
        let x = x as u8;
        // An empty range (left > right) contains nothing, matching
        // hardware's "window disabled by degenerate bounds" behavior.
        let mut in1 = x >= regs.wh0 && x <= regs.wh1;
        if w1_inv {
            in1 = !in1;
        }
        let mut in2 = x >= regs.wh2 && x <= regs.wh3;
        if w2_inv {
            in2 = !in2;
        }
        *slot = match (w1_en, w2_en) {
            (true, false) => in1,
            (false, true) => in2,
            _ => match log {
                0 => in1 | in2,
                1 => in1 & in2,
                2 => in1 ^ in2,
                _ => !(in1 ^ in2),
            },
        };
    }
    out
}

/// Sign-extends a 13-bit register value (M7X/M7Y/M7HOFS/M7VOFS) to i32.
fn sign13(v: u16) -> i32 {
    (((v << 3) as i16) >> 3) as i32
}

/// Samples the mode-7 playing field at screen position (`x`, `y`),
/// returning the raw 8-bit pixel value, or `None` when the transformed
/// coordinate falls outside the 1024x1024 field and M7SEL's screen-over
/// mode says "transparent". The field is a 128x128 map of 8x8 8bpp tiles
/// stored interleaved in VRAM: word N's LOW byte is map entry N (a tile
/// number), and word N's HIGH byte is tile-data byte N (tile*64 + row*8 +
/// column). Transform per fullsnes/bsnes: the per-scanline origin uses
/// the matrix against (scroll - center) -- each product truncated to
/// ~-64/+63 sub-pixel steps via `& !63` -- plus the center, then steps by
/// M7A/M7C per screen pixel; all math in 8.8 signed fixed point.
fn mode7_sample(vram: &Vram, regs: &PpuRegisters, x: usize, y: usize) -> Option<u8> {
    let a = regs.m7a as i16 as i32;
    let b = regs.m7b as i16 as i32;
    let c = regs.m7c as i16 as i32;
    let d = regs.m7d as i16 as i32;
    let cx = sign13(regs.m7x);
    let cy = sign13(regs.m7y);
    let hofs = sign13(regs.m7_hofs);
    let vofs = sign13(regs.m7_vofs);

    // The scroll-minus-center offsets are clipped to signed 11 bits
    // (documented hardware quirk of the mode-7 pipeline).
    fn clip(v: i32) -> i32 {
        if v & 0x2000 != 0 { v | !0x3FF } else { v & 0x3FF }
    }

    // M7SEL bits 0/1 flip the whole 256x224 screen before the transform.
    let sx = (if regs.m7sel & 0x01 != 0 { 255 - x } else { x }) as i32;
    let sy = (if regs.m7sel & 0x02 != 0 { 255 - y } else { y }) as i32;

    let ox = ((a * clip(hofs - cx)) & !63)
        + ((b * clip(vofs - cy)) & !63)
        + ((b * sy) & !63)
        + (cx << 8);
    let oy = ((c * clip(hofs - cx)) & !63)
        + ((d * clip(vofs - cy)) & !63)
        + ((d * sy) & !63)
        + (cy << 8);

    let px = (ox + a * sx) >> 8;
    let py = (oy + c * sx) >> 8;

    let out_of_field = ((px | py) as u32) & !0x3FF != 0;
    let screen_over = (regs.m7sel >> 6) & 0x03;
    if out_of_field && screen_over == 2 {
        return None; // outside the field renders transparent
    }

    let (fx, fy) = ((px & 0x3FF) as u16, (py & 0x3FF) as u16);
    let tile = if out_of_field && screen_over == 3 {
        0 // outside the field repeats tile 0
    } else {
        // Map entry: low byte of word (tile_y * 128 + tile_x).
        vram.read(((fy / 8) as u16 * 128 + (fx / 8) as u16) * 2)
    };
    // Pixel: high byte of word (tile * 64 + row * 8 + column).
    let pixel_word = (tile as u16) * 64 + (fy % 8) * 8 + (fx % 8);
    Some(vram.read(pixel_word * 2 + 1))
}

/// SNES direct-color mode (CGWSEL bit 0, 8bpp layers): the pixel byte is
/// its own BGR color -- bits 0-2 = red, 3-5 = green, 6-7 = blue -- with
/// the tilemap palette bits (zero in mode 7) contributing one extra low
/// bit per channel. Returns BGR555.
fn direct_color(pixel: u8, palette: u8) -> u16 {
    let r = (((pixel & 0x07) << 2) | ((palette & 0x01) << 1)) as u16;
    let g = ((((pixel >> 3) & 0x07) << 2) | (palette & 0x02)) as u16;
    let b = ((((pixel >> 6) & 0x03) << 3) | ((palette & 0x04) >> 1)) as u16;
    r | (g << 5) | (b << 10)
}

/// Draws mode 7's BG1 (`extbg == false`: all 8 pixel bits are color, no
/// priority) or its EXTBG BG2 (`extbg == true`: pixel bit 7 is a priority
/// bit, bits 0-6 the color; only pixels matching `want_priority` draw).
fn draw_mode7_layer(
    buf: &mut [u16],
    layer_buf: &mut [u8],
    vram: &Vram,
    cgram: &Cgram,
    regs: &PpuRegisters,
    extbg: bool,
    want_priority: u8,
    skip: &[bool; SCREEN_WIDTH],
    y0: usize,
    y1: usize,
) {
    let use_direct_color = regs.cgwsel & 0x01 != 0;
    // Mode 7 honors BG1's mosaic bit (and BG2's for the EXTBG layer) the
    // same way the tile-based path does: snap the sampled screen
    // coordinate to the block origin.
    let mosaic_bit = if extbg { 0x02 } else { 0x01 };
    let mosaic_size = if regs.mosaic & mosaic_bit != 0 {
        ((regs.mosaic >> 4) & 0x0F) as usize + 1
    } else {
        1
    };
    for py in y0..y1 {
        for px in 0..SCREEN_WIDTH {
            if skip[px] {
                continue; // window-masked
            }
            let Some(raw) = mode7_sample(vram, regs, px - px % mosaic_size, py - py % mosaic_size)
            else {
                continue;
            };
            let (color_index, layer) = if extbg {
                if (raw >> 7) != want_priority {
                    continue;
                }
                (raw & 0x7F, LAYER_BG1 + 1)
            } else {
                (raw, LAYER_BG1)
            };
            if color_index == 0 {
                continue; // transparent
            }
            let color = if use_direct_color && !extbg {
                direct_color(color_index, 0)
            } else {
                cgram.read_color(color_index) & 0x7FFF
            };
            let idx = py * SCREEN_WIDTH + px;
            buf[idx] = color;
            layer_buf[idx] = layer;
        }
    }
}

/// SNES color math on two BGR555 colors, per-channel in 5-bit space:
/// `main +/- operand`, optionally halved, clamped to 0..31 per channel.
/// Channel extraction/reassembly delegates to `Cgram`'s helpers rather
/// than reimplementing the same BGR555 mask/shift logic inline.
fn color_math(main: u16, operand: u16, subtract: bool, half: bool) -> u16 {
    let combine = |m: i32, s: i32| -> u8 {
        let mut v = if subtract { m - s } else { m + s };
        if half {
            v >>= 1;
        }
        v.clamp(0, 31) as u8
    };
    let r = combine(Cgram::extract_red(main) as i32, Cgram::extract_red(operand) as i32);
    let g = combine(Cgram::extract_green(main) as i32, Cgram::extract_green(operand) as i32);
    let b = combine(Cgram::extract_blue(main) as i32, Cgram::extract_blue(operand) as i32);
    Cgram::make_color(r, g, b)
}

/// BGR555 -> RGB888, expanding each 5-bit channel by replicating its top 3
/// bits into the low bits (the standard technique for even 0-255 coverage).
fn bgr555_to_rgb8(color: u16) -> (u8, u8, u8) {
    let r5 = (color & 0x1F) as u32;
    let g5 = ((color >> 5) & 0x1F) as u32;
    let b5 = ((color >> 10) & 0x1F) as u32;
    let expand = |c5: u32| ((c5 << 3) | (c5 >> 2)) as u8;
    (expand(r5), expand(g5), expand(b5))
}

/// Bits per pixel of each of the 4 BG layers for a given BGMODE (0-6);
/// `None` means the layer doesn't exist in this mode. Verified against
/// wiki.superfamicom.org/Backgrounds.
fn bg_depths(mode: u8) -> [Option<u8>; 4] {
    match mode {
        0 => [Some(2), Some(2), Some(2), Some(2)],
        1 => [Some(4), Some(4), Some(2), None],
        2 => [Some(4), Some(4), None, None],
        3 => [Some(8), Some(4), None, None],
        4 => [Some(8), Some(2), None, None],
        5 => [Some(4), Some(2), None, None],
        6 => [Some(4), None, None, None],
        _ => [None, None, None, None],
    }
}

/// Decodes one row (8 palette-index pixels, left to right) of a planar
/// tile. `depth` is 2, 4, or 8 bits/pixel; tiles are stored as
/// `depth/2` consecutive 16-byte "bitplane pairs". Within a pair, the two
/// bitplanes are interleaved ROW BY ROW: each of the 8 rows contributes 2
/// adjacent bytes (low plane, then high plane) -- byte layout
/// `[r0p0, r0p1, r1p0, r1p1, ... r7p0, r7p1]`. This matches real SNES
/// VRAM word organization (one word per tile row per pair, low plane in
/// the low byte). An earlier version read `[8 bytes of p0][8 bytes of
/// p1]` (the NES layout, planes NOT interleaved) -- decoding real
/// cartridge graphics into striped garbage: every tile on every screen
/// (BGs and sprites alike) rendered as a half-height double-struck smear,
/// which is exactly why Mario/enemies were unrecognizable in gameplay.
/// Verified against real SMW VRAM contents: the logo's letter tiles only
/// decode into coherent glyph shapes with the interleaved layout.
fn decode_tile_row(vram: &Vram, tile_data_base_word: u16, tile_index: u16, depth: u8, row: u8) -> [u8; 8] {
    let bytes_per_tile = (depth as u16) * 8;
    let tile_byte_addr = tile_data_base_word
        .wrapping_mul(2)
        .wrapping_add(tile_index.wrapping_mul(bytes_per_tile));

    let mut out = [0u8; 8];
    let plane_pairs = depth / 2;
    for pair in 0..plane_pairs {
        let pair_base = tile_byte_addr.wrapping_add((pair as u16) * 16);
        let lo = vram.read(pair_base.wrapping_add((row as u16) * 2));
        let hi = vram.read(pair_base.wrapping_add((row as u16) * 2 + 1));
        for x in 0..8u8 {
            let bit = 7 - x;
            let b0 = (lo >> bit) & 1;
            let b1 = (hi >> bit) & 1;
            out[x as usize] |= (b0 | (b1 << 1)) << (pair * 2);
        }
    }
    out
}

/// Averages 1-4 BGR555 colors per channel (used to collapse hi-res dot
/// pairs / interlace line pairs into the 256x224 output raster).
fn average_bgr555(colors: &[u16]) -> u16 {
    let n = colors.len() as u32;
    let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
    for &c in colors {
        r += (c & 0x1F) as u32;
        g += ((c >> 5) & 0x1F) as u32;
        b += ((c >> 10) & 0x1F) as u32;
    }
    ((r / n) as u16) | (((g / n) as u16) << 5) | (((b / n) as u16) << 10)
}

fn draw_bg_layer(
    buf: &mut [u16],
    layer_buf: &mut [u8],
    vram: &Vram,
    cgram: &Cgram,
    regs: &PpuRegisters,
    bg: usize,
    depth: u8,
    want_priority: u8,
    skip: &[bool; SCREEN_WIDTH],
    y0: usize,
    y1: usize,
) {
    let mode = regs.bgmode & 0x07;
    // Modes 5/6 are the hi-res modes: BG pixels exist in a 512-dot
    // horizontal space (tiles forced 16 wide), collapsed into this
    // renderer's 256-wide raster by averaging each adjacent dot pair.
    // With SETINI's interlace bit the vertical resolution doubles too
    // (448 half-lines), collapsed the same way by averaging the two
    // field lines each output row spans.
    let hires = mode == 5 || mode == 6;
    let interlaced = hires && (regs.setini & 0x01) != 0;

    let tilemap_base_word = ((regs.bg_sc[bg] >> 2) as u16) * 0x400;
    let screen_size = regs.bg_sc[bg] & 0x03; // 0=32x32, 1=64x32, 2=32x64, 3=64x64
    let nba = if bg < 2 { regs.bg12nba } else { regs.bg34nba };
    let nibble = if bg % 2 == 0 { nba & 0x0F } else { (nba >> 4) & 0x0F };
    let tile_data_base_word = (nibble as u16) * 0x1000;

    let hofs = regs.bg_hofs[bg];
    let vofs = regs.bg_vofs[bg];

    // BGMODE bits 4-7: 16x16 tiles for BG1-4. In modes 5/6 the tile is
    // always 16 wide (the hi-res fetch pattern); the size bit then only
    // selects 8- vs 16-pixel height.
    let size16 = regs.bgmode & (0x10 << bg) != 0;
    let tile_w: u32 = if hires || size16 { 16 } else { 8 };
    let tile_h: u32 = if size16 { 16 } else { 8 };

    // Mosaic (MOSAIC $2106): when enabled for this BG, every size x size
    // screen-space block repeats its top-left pixel -- implemented by
    // snapping the sampled coordinate down to the block origin while
    // still writing every screen pixel.
    let mosaic_size = if regs.mosaic & (1 << bg) != 0 {
        ((regs.mosaic >> 4) & 0x0F) as usize + 1
    } else {
        1
    };

    let (map_w_tiles, map_h_tiles): (u32, u32) = match screen_size {
        0 => (32, 32),
        1 => (64, 32),
        2 => (32, 64),
        _ => (64, 64),
    };

    // Samples this layer at a WORLD coordinate (scroll already applied),
    // returning the BGR555 color, or None when transparent / not part of
    // the current priority pass. Multi-cell (16-wide/-tall) tiles select
    // their 8x8 sub-cell the same way OBJ tiles do: +1 tile number per
    // horizontal cell, +16 per vertical cell, wrapping in the 10-bit
    // tile-number space; flips mirror across the WHOLE tile.
    let sample = |world_x: u32, world_y: u32| -> Option<u16> {
        let tile_col = (world_x / tile_w) % map_w_tiles;
        let tile_row = (world_y / tile_h) % map_h_tiles;

        // Sizes larger than 32x32 are 2-4 separate contiguous 32x32
        // (0x400-entry) maps in VRAM; resolve which one this tile is in.
        let (quad_col, local_col) = (tile_col / 32, tile_col % 32);
        let (quad_row, local_row) = (tile_row / 32, tile_row % 32);
        let quadrant: u32 = match screen_size {
            0 => 0,
            1 => quad_col,
            2 => quad_row,
            _ => quad_row * 2 + quad_col,
        };

        let map_entry_word = tilemap_base_word
            .wrapping_add((quadrant * 0x400) as u16)
            .wrapping_add((local_row * 32 + local_col) as u16);
        let entry = vram.read_word(map_entry_word.wrapping_mul(2));

        // Tilemap entry bit 13 is the per-tile priority bit. Only draw
        // tiles matching the priority pass currently being composited.
        let tile_priority = ((entry >> 13) & 0x01) as u8;
        if tile_priority != want_priority {
            return None;
        }

        let base_tile = entry & 0x3FF;
        let palette_num = (entry >> 10) & 0x07;
        let flip_h = (entry & 0x4000) != 0;
        let flip_v = (entry & 0x8000) != 0;

        let mut in_x = world_x % tile_w;
        let mut in_y = world_y % tile_h;
        if flip_h {
            in_x = tile_w - 1 - in_x;
        }
        if flip_v {
            in_y = tile_h - 1 - in_y;
        }
        let cell_x = (in_x / 8) as u16;
        let cell_y = (in_y / 8) as u16;
        let tile_index = base_tile.wrapping_add(cell_x).wrapping_add(cell_y * 0x10) & 0x3FF;

        let row_pixels = decode_tile_row(vram, tile_data_base_word, tile_index, depth, (in_y % 8) as u8);
        let pixel_value = row_pixels[(in_x % 8) as usize];
        if pixel_value == 0 {
            return None; // transparent -- leave whatever's already drawn beneath
        }

        let cgram_index = match depth {
            2 => (palette_num * 4 + pixel_value as u16) as u8,
            4 => (palette_num * 16 + pixel_value as u16) as u8,
            _ => pixel_value, // 8bpp: direct index, no palette grouping
        };
        // 8bpp layers honor CGWSEL's direct-color mode (the tilemap
        // palette bits feed the channels' extra low bits).
        Some(if depth == 8 && regs.cgwsel & 0x01 != 0 {
            direct_color(pixel_value, palette_num as u8)
        } else {
            cgram.read_color(cgram_index) & 0x7FFF
        })
    };

    // Offset-per-tile (modes 2/4, and 6 on hardware): BG3's tilemap
    // doubles as a table of per-8-pixel-column scroll overrides for
    // BG1/BG2. The first visible column always uses the normal scroll;
    // for screen column N >= 1 the entry fetched from BG3's tilemap at
    // world position ((N-1)*8 + (BG3HOFS & ~7), BG3VOFS) replaces the
    // horizontal offset (the BG's own fine scroll, HOFS & 7, still
    // applies), and in mode 2 the entry one tile-row below (BG3VOFS + 8)
    // replaces the vertical offset. Mode 4 fetches a single entry whose
    // bit 15 selects H or V. Entry bit 13 gates the override for BG1,
    // bit 14 for BG2 (fullsnes "OPT"; snes9x gfx.cpp
    // DrawBackgroundOffset). Hi-res mode 6's 512-dot variant is not
    // modeled.
    let opt_active = !hires && (mode == 2 || mode == 4) && bg < 2;
    let opt_valid_mask: u16 = 0x2000 << bg;
    let bg3_tilemap_base_word = ((regs.bg_sc[2] >> 2) as u16) * 0x400;
    let bg3_screen_size = regs.bg_sc[2] & 0x03;
    let bg3_hofs = regs.bg_hofs[2];
    let bg3_vofs = regs.bg_vofs[2];
    let bg3_entry = |world_x: u32, world_y: u32| -> u16 {
        let (map_w, map_h): (u32, u32) = match bg3_screen_size {
            0 => (32, 32),
            1 => (64, 32),
            2 => (32, 64),
            _ => (64, 64),
        };
        let tile_col = (world_x / 8) % map_w;
        let tile_row = (world_y / 8) % map_h;
        let (quad_col, local_col) = (tile_col / 32, tile_col % 32);
        let (quad_row, local_row) = (tile_row / 32, tile_row % 32);
        let quadrant: u32 = match bg3_screen_size {
            0 => 0,
            1 => quad_col,
            2 => quad_row,
            _ => quad_row * 2 + quad_col,
        };
        let word = bg3_tilemap_base_word
            .wrapping_add((quadrant * 0x400) as u16)
            .wrapping_add((local_row * 32 + local_col) as u16);
        vram.read_word(word.wrapping_mul(2))
    };

    for py in y0..y1 {
        let eff_py = py - py % mosaic_size;
        for px in 0..SCREEN_WIDTH {
            if skip[px] {
                continue; // window-masked
            }
            let eff_px = px - px % mosaic_size;

            let color = if hires {
                // Sample both dots of the pair (and both field lines when
                // interlaced), averaging whatever is opaque.
                let dots = [(2 * eff_px) as u32, (2 * eff_px + 1) as u32];
                let lines: &[u32] = if interlaced {
                    &[0, 1]
                } else {
                    &[0]
                };
                let mut samples = [0u16; 4];
                let mut count = 0;
                for &line in lines {
                    let world_y = if interlaced {
                        ((2 * eff_py) as u32 + line).wrapping_add(vofs as u32)
                    } else {
                        (eff_py as u32).wrapping_add(vofs as u32)
                    };
                    for &dot in &dots {
                        if let Some(c) = sample(dot.wrapping_add(hofs as u32), world_y) {
                            samples[count] = c;
                            count += 1;
                        }
                    }
                }
                if count == 0 {
                    continue;
                }
                average_bgr555(&samples[..count])
            } else {
                let (mut eff_hofs, mut eff_vofs) = (hofs, vofs);
                if opt_active {
                    let col = ((px as u32) + ((hofs as u32) & 7)) / 8;
                    if col > 0 {
                        let opt_x = ((col - 1) * 8).wrapping_add((bg3_hofs as u32) & !7u32);
                        let hentry = bg3_entry(opt_x, bg3_vofs as u32);
                        if mode == 4 {
                            if hentry & opt_valid_mask != 0 {
                                if hentry & 0x8000 != 0 {
                                    eff_vofs = hentry & 0x3FF;
                                } else {
                                    eff_hofs = (hentry & 0x3F8) | (hofs & 7);
                                }
                            }
                        } else {
                            let ventry = bg3_entry(opt_x, (bg3_vofs as u32).wrapping_add(8));
                            if hentry & opt_valid_mask != 0 {
                                eff_hofs = (hentry & 0x3F8) | (hofs & 7);
                            }
                            if ventry & opt_valid_mask != 0 {
                                eff_vofs = ventry & 0x3FF;
                            }
                        }
                    }
                }
                let world_x = (eff_px as u32).wrapping_add(eff_hofs as u32);
                let world_y = (eff_py as u32).wrapping_add(eff_vofs as u32);
                match sample(world_x, world_y) {
                    Some(c) => c,
                    None => continue,
                }
            };

            let idx = py * SCREEN_WIDTH + px;
            buf[idx] = color;
            layer_buf[idx] = LAYER_BG1 + bg as u8;
        }
    }
}

/// (width, height) in pixels for each of OBSEL's 8 size-pair codes,
/// returned as (small, large). Codes 6/7 are the two undocumented,
/// non-square pairs (16x32/32x64 and 16x32/32x32 respectively) --
/// cross-checked against the SNESdev wiki PPU registers page and
/// fullsnes's OBJSEL OBJ Size table, which agree exactly.
/// `size_code` is OBSEL bits 5-7 (already shifted down by the caller).
fn sprite_size_pair(size_code: u8) -> ((u32, u32), (u32, u32)) {
    match size_code & 0x07 {
        0 => ((8, 8), (16, 16)),
        1 => ((8, 8), (32, 32)),
        2 => ((8, 8), (64, 64)),
        3 => ((16, 16), (32, 32)),
        4 => ((16, 16), (64, 64)),
        5 => ((32, 32), (64, 64)),
        6 => ((16, 32), (32, 64)),
        _ => ((16, 32), (32, 32)),
    }
}

fn draw_sprites(
    buf: &mut [u16],
    layer_buf: &mut [u8],
    vram: &Vram,
    cgram: &Cgram,
    oam: &Oam,
    regs: &PpuRegisters,
    sprite_eval: &SpriteEval,
    want_priority: u8,
    skip: &[bool; SCREEN_WIDTH],
    y0: usize,
    y1: usize,
) {
    // OBSEL ($2101) layout is `sssnnbbb`: bits 0-2 = OBJ tile base (8K-word
    // steps), bits 3-4 = name select (gap to the second 256-tile table),
    // bits 5-7 = the size-pair code. An earlier version read the BASE from
    // bits 5-7 and the SIZE from bits 0-2 (exactly swapped) -- with SMW's
    // in-level OBSEL=$03 that decoded sprites from VRAM word 0 (background
    // tile data!) at 16x16/32x32 instead of the real sprite graphics at
    // word $6000 at 8x8/16x16, turning Mario and every enemy into
    // unrecognizable colored mush.
    let tile_data_base_word = ((regs.obsel & 0x07) as u16) * 0x2000;
    let name_select = ((regs.obsel >> 3) & 0x03) as u16;
    let (small_size, large_size) = sprite_size_pair((regs.obsel >> 5) & 0x07);

    // Iterate in reverse rotation order so that, within this priority
    // level, the sprite closest to FirstSprite ends up drawn last (on
    // top) -- hardware's overlap rule is "closest to FirstSprite in
    // evaluation order wins", which reduces to "lower OAM index wins"
    // when priority rotation is off (FirstSprite = 0). Only sprites whose
    // OAM priority (attr bits 4-5) equals `want_priority` are drawn in
    // this pass; the caller invokes the four priority levels in the
    // correct back-to-front slots for the current BG mode.
    let first_sprite = regs.first_sprite & 0x7F;
    for k in (0..128u8).rev() {
        let sprite_idx = first_sprite.wrapping_add(k) & 0x7F;
        let base = (sprite_idx as usize) * 4;
        // OAM entry layout per fullsnes: byte 0 = X (low 8 bits), byte 1 =
        // Y, byte 2 = tile, byte 3 = attributes. These first two used to
        // be read SWAPPED (byte 0 as Y), which transposed every sprite
        // around the screen diagonal -- subtle on near-diagonal scenes,
        // but it scattered SMW's walking enemies into vertical stacks and
        // painted a permanent garbage column at x=240 (sprites parked
        // offscreen with Y=$F0 came back as X=240 with Y = their stale X).
        let x_low = oam.read(base as u16);
        let y_raw = oam.read(base as u16 + 1);
        let tile_low = oam.read(base as u16 + 2) as u16;
        let attrs = oam.read(base as u16 + 3);

        // OAM attribute bits 4-5 = sprite priority (0-3).
        if (attrs >> 4) & 0x03 != want_priority {
            continue;
        }

        let high_table_byte = oam.read(512 + (sprite_idx as u16) / 4);
        let shift = (sprite_idx % 4) * 2;
        let x_high_bit = (high_table_byte >> shift) & 0x01;
        let size_bit = (high_table_byte >> (shift + 1)) & 0x01;

        let (w, h) = if size_bit != 0 { large_size } else { small_size };

        let x_full = ((x_high_bit as u16) << 8) | (x_low as u16);
        let x: i32 = if x_full & 0x100 != 0 { (x_full as i32) - 512 } else { x_full as i32 };
        let y: i32 = if y_raw >= 0xF0 { (y_raw as i32) - 256 } else { y_raw as i32 };

        if x + (w as i32) <= 0 || x >= SCREEN_WIDTH as i32 {
            continue;
        }
        if y + (h as i32) <= 0 || y >= SCREEN_HEIGHT as i32 {
            continue;
        }

        // OAM attribute byte layout is `vhoopppN`: bit 7 = v-flip, bit 6 =
        // h-flip, bits 5-4 = priority, bits 3-1 = palette, bit 0 = tile
        // number bit 8 (selects the second 256-tile table). An earlier
        // version read palette from bits 0-2 and flips from bits 5/6 --
        // every sprite got the wrong palette and priority-bit-contaminated
        // "flips", compounding the OBSEL swap above.
        let palette_num = ((attrs >> 1) & 0x07) as u16;
        let flip_h = (attrs & 0x40) != 0;
        let flip_v = (attrs & 0x80) != 0;
        let tile_base = tile_low | (((attrs & 0x01) as u16) << 8);

        for ty in 0..h {
            let screen_y = y + ty as i32;
            if screen_y < y0 as i32 || screen_y >= y1 as i32 {
                continue;
            }
            // Hardware per-line limits: skip lines where this sprite lost
            // the 32-sprites/34-tiles evaluation (see `evaluate_sprites`).
            if sprite_eval.masks[screen_y as usize - sprite_eval.y0] & (1u128 << sprite_idx) == 0 {
                continue;
            }
            let src_ty = if flip_v { h - 1 - ty } else { ty };
            let tile_row_idx = src_ty / 8;
            let pixel_row_in_tile = (src_ty % 8) as u8;

            for tx in 0..w {
                let screen_x = x + tx as i32;
                if screen_x < 0 || screen_x >= SCREEN_WIDTH as i32 {
                    continue;
                }
                if skip[screen_x as usize] {
                    continue; // window-masked
                }
                let src_tx = if flip_h { w - 1 - tx } else { tx };
                let tile_col_idx = src_tx / 8;
                let pixel_col_in_tile = (src_tx % 8) as u8;

                // Sprite tiles are laid out in a 16-tiles-wide grid in VRAM.
                // The column component must wrap WITHIN the tile number's
                // low nibble (real hardware behavior): a multi-tile sprite
                // whose base tile's low nibble plus the column offset
                // exceeds 15 wraps back into the same row rather than
                // carrying into the next row/table -- e.g. base tile 0x0A
                // with column offset 7 must land on tile 0x01, not 0x11.
                // The row component (already a multiple of 16) is added
                // separately on top of the untouched high bits and may
                // carry normally.
                let row_component = (tile_row_idx as u16).wrapping_mul(16);
                let col_component = tile_base.wrapping_add(tile_col_idx as u16) & 0x0F;
                let tile_index = (tile_base & 0xFFF0)
                    .wrapping_add(row_component)
                    .wrapping_add(col_component)
                    & 0x1FF;
                // Tiles 256-511 live in the second table, offset from the
                // base by (name_select + 1) * 0x1000 words per OBSEL.
                let (table_base_word, tile_in_table) = if tile_index >= 0x100 {
                    (
                        tile_data_base_word.wrapping_add((name_select + 1) * 0x1000),
                        tile_index & 0xFF,
                    )
                } else {
                    (tile_data_base_word, tile_index)
                };

                let row_pixels = decode_tile_row(vram, table_base_word, tile_in_table, 4, pixel_row_in_tile);
                let pixel_value = row_pixels[pixel_col_in_tile as usize];
                if pixel_value == 0 {
                    continue; // transparent
                }

                let cgram_index = 128 + (palette_num * 16 + pixel_value as u16) as u8;
                let idx = screen_y as usize * SCREEN_WIDTH + screen_x as usize;
                buf[idx] = cgram.read_color(cgram_index) & 0x7FFF;
                // Hardware rule: color math only ever applies to sprites
                // using OBJ palettes 4-7; palettes 0-3 are exempt even when
                // CGADSUB bit 4 is set.
                layer_buf[idx] = if palette_num < 4 { LAYER_OBJ_PAL03 } else { LAYER_OBJ };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encodes a tile in the REAL SNES row-interleaved bitplane-pair
    /// layout (`[r0p0, r0p1, r1p0, r1p1, ...]`). An earlier version of
    /// this helper wrote `[8 bytes p0][8 bytes p1]` -- the same wrong
    /// (NES-style) layout the production decoder had, so the tests
    /// self-consistently passed while every real cartridge tile rendered
    /// as garbage.
    fn make_2bpp_tile(rows: [[u8; 8]; 8]) -> [u8; 16] {
        let mut data = [0u8; 16];
        for (y, row) in rows.iter().enumerate() {
            let mut lo = 0u8;
            let mut hi = 0u8;
            for (x, &pixel) in row.iter().enumerate() {
                let bit = 7 - x;
                lo |= (pixel & 1) << bit;
                hi |= ((pixel >> 1) & 1) << bit;
            }
            data[y * 2] = lo;
            data[y * 2 + 1] = hi;
        }
        data
    }

    #[test]
    fn sprite_size_pair_codes_6_and_7_are_non_square() {
        // OBSEL size-pair codes 6 and 7 are the two undocumented,
        // non-square pairs. An earlier version approximated both as
        // square (16x16/32x32), which is wrong for the "large" size of
        // code 6 (real hardware: 32x64) and for the "small" size of both
        // codes (real hardware: 16x32, not 16x16). Values cross-checked
        // against the SNESdev wiki PPU registers page and fullsnes's
        // OBJSEL OBJ Size table, which agree exactly:
        //   code 6: small 16x32, large 32x64
        //   code 7: small 16x32, large 32x32
        let (small6, large6) = sprite_size_pair(6);
        assert_eq!(small6, (16, 32), "code 6 small size must be 16x32, not square 16x16");
        assert_eq!(large6, (32, 64), "code 6 large size must be 32x64, not square 32x32");

        let (small7, large7) = sprite_size_pair(7);
        assert_eq!(small7, (16, 32), "code 7 small size must be 16x32, not square 16x16");
        assert_eq!(large7, (32, 32), "code 7 large size is genuinely square 32x32");
    }

    #[test]
    fn tile_decode_uses_row_interleaved_snes_bitplane_layout() {
        // Pins the REAL SNES tile byte layout with hand-written raw bytes
        // (deliberately NOT built via make_2bpp_tile, so this test cannot
        // become self-consistently wrong alongside the encoder again).
        // 2bpp row-interleaved: byte 0 = row0 plane0, byte 1 = row0 plane1.
        let mut vram = Vram::new();
        vram.write(0, 0b1111_0000); // row 0, plane 0
        vram.write(1, 0b0000_1111); // row 0, plane 1
        vram.write(2, 0b1000_0001); // row 1, plane 0
        vram.write(3, 0b1000_0000); // row 1, plane 1

        let row0 = decode_tile_row(&vram, 0, 0, 2, 0);
        assert_eq!(row0, [1, 1, 1, 1, 2, 2, 2, 2],
            "row 0 must combine byte0 (plane0) and byte1 (plane1)");
        let row1 = decode_tile_row(&vram, 0, 0, 2, 1);
        assert_eq!(row1, [3, 0, 0, 0, 0, 0, 0, 1],
            "row 1 must come from bytes 2/3 (row-interleaved), not bytes 1/9 (planar)");

        // 4bpp: second bitplane pair starts 16 bytes in; its two planes
        // are also row-interleaved and contribute pixel bits 2/3.
        let mut vram4 = Vram::new();
        vram4.write(0, 0xFF); // row 0, plane 0
        vram4.write(16, 0xFF); // row 0, plane 2
        vram4.write(17, 0xFF); // row 0, plane 3
        let row0_4bpp = decode_tile_row(&vram4, 0, 0, 4, 0);
        assert_eq!(row0_4bpp, [0x0D; 8],
            "4bpp planes 0/2/3 set -> pixel value 0b1101 for every pixel of row 0");
    }

    #[test]
    fn forced_blank_renders_solid_black() {
        let vram = Vram::new();
        let cgram = Cgram::new();
        let oam = Oam::new();
        let mut regs = PpuRegisters::default();
        regs.inidisp = 0x80;

        let fb = render_frame(&vram, &cgram, &oam, &regs);
        assert_eq!(fb.len(), SCREEN_WIDTH * SCREEN_HEIGHT * 4);
        assert!(fb.chunks_exact(4).all(|px| px == [0, 0, 0, 0xFF]));
    }

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
    fn mode1_bg1_tile_renders_with_correct_palette_colors() {
        let mut vram = Vram::new();
        let mut cgram = Cgram::new();
        let oam = Oam::new();

        // A 4bpp tile needs 2 bitplane pairs (32 bytes); use only pixel
        // values 0 (transparent) and 1 so a single 2bpp pair suffices and
        // the second pair (all zero) contributes nothing.
        let mut tile_row0 = [0u8; 8];
        tile_row0[0] = 1; // first pixel uses palette index 1
        let tile = make_2bpp_tile([tile_row0, [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8]]);
        // Tile data base word 0 -> byte 0. Tile index 0.
        for (i, &b) in tile.iter().enumerate() {
            vram.write(i as u16, b);
        }

        // Tilemap base word 0x400 (so it doesn't overlap tile data).
        // Map entry 0 (top-left tile) = tile index 0, palette 0, no flip.
        vram.write_word(0x400 * 2, 0x0000);

        // CGRAM color for BG palette 0, pixel index 1 -> entry 1 -> pure
        // red.
        cgram.write(1 * 2, 0xFF);
        cgram.write(1 * 2 + 1, 0x7F); // low byte 0xFF, high byte 0x7F -> 0x7FFF -> R=31,G=31,B=31? recompute below

        let mut regs = PpuRegisters::default();
        regs.inidisp = 0x0F; // not forced blank, full brightness
        regs.bgmode = 1; // BG1 = 4bpp
        regs.bg_sc[0] = 0x04; // tilemap base = (0x04>>2)*0x400 = 0x400, size 32x32
        regs.bg12nba = 0x00; // BG1 tile data base = 0
        regs.tm = 0x01; // enable BG1 only

        let fb = render_frame(&vram, &cgram, &oam, &regs);
        let expected = bgr555_to_rgb8(cgram.read_color(1));
        let idx = 0; // pixel (0,0)
        assert_eq!((fb[idx], fb[idx + 1], fb[idx + 2]), expected);
        assert_eq!(fb[idx + 3], 0xFF);

        // A pixel using value 0 elsewhere in the same tile must show the
        // backdrop color (CGRAM index 0), not the tile's palette color.
        let backdrop = bgr555_to_rgb8(cgram.read_color(0));
        let idx2 = (1) * 4; // pixel (1,0), tile pixel value 0
        assert_eq!((fb[idx2], fb[idx2 + 1], fb[idx2 + 2]), backdrop);
    }

    #[test]
    fn sprite_renders_at_its_oam_position_with_object_palette() {
        let mut vram = Vram::new();
        let mut cgram = Cgram::new();
        let mut oam = Oam::new();

        // 4bpp tile, single pixel value 1 at (0,0), rest transparent.
        let mut tile_row0 = [0u8; 8];
        tile_row0[0] = 1;
        let tile = make_2bpp_tile([tile_row0, [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8]]);
        for (i, &b) in tile.iter().enumerate() {
            vram.write(i as u16, b);
        }

        // Sprite 0: X=20, Y=10, tile=0, palette=0, no flip, small size.
        // Raw OAM byte order per fullsnes: byte 0 = X, byte 1 = Y (an
        // earlier renderer read these swapped, and this test encoded the
        // same swapped order, so it kept passing).
        oam.write(0, 20); // X
        oam.write(1, 10); // Y
        oam.write(2, 0); // tile
        oam.write(3, 0x00); // attrs: palette 0
        oam.write(512, 0x00); // high table byte for sprites 0-3: X high bit 0, size 0

        // Object palette 0, pixel index 1 -> CGRAM index 128 + 1 = 129.
        cgram.write(129 * 2, 0xE0); // arbitrary nonzero BGR555 low byte
        cgram.write(129 * 2 + 1, 0x03);

        let mut regs = PpuRegisters::default();
        regs.inidisp = 0x0F;
        regs.obsel = 0x00; // size pair 0 (8x8/16x16), tile base word 0
        regs.tm = 0x10; // enable sprites only

        let fb = render_frame(&vram, &cgram, &oam, &regs);
        let expected = bgr555_to_rgb8(cgram.read_color(129));
        let idx = (10 * SCREEN_WIDTH + 20) * 4;
        assert_eq!((fb[idx], fb[idx + 1], fb[idx + 2]), expected);
    }

    #[test]
    fn obsel_and_oam_attributes_decode_per_real_hardware_bit_layout() {
        // Regression guard for two systematically-swapped decodes that
        // made every sprite unrecognizable mush: OBSEL's base address is
        // bits 0-2 (NOT 5-7, which are the size pair), and the OAM
        // attribute byte is `vhoopppN` (palette in bits 1-3, flips in
        // bits 6-7, tile bit 8 in bit 0 -- NOT palette 0-2 / flips 5-6).
        let mut vram = Vram::new();
        let mut cgram = Cgram::new();
        let mut oam = Oam::new();

        // OBJ tile base = OBSEL bits 0-2 = 1 -> word 0x2000 -> byte 0x4000.
        // Tile 0 there: pixel value 1 at (0,0), rest transparent.
        let mut tile_row0 = [0u8; 8];
        tile_row0[0] = 1;
        let tile = make_2bpp_tile([tile_row0, [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8]]);
        for (i, &b) in tile.iter().enumerate() {
            vram.write(0x4000 + i as u16, b);
        }
        // The SECOND tile table (tiles 256-511) for OBSEL base 1, name 0
        // sits at word 0x2000 + 0x1000 = 0x3000 -> byte 0x6000. Its tile 0
        // (= sprite tile number 0x100): pixel value 1 at (1,0).
        let mut tile2_row0 = [0u8; 8];
        tile2_row0[1] = 1;
        let tile2 = make_2bpp_tile([tile2_row0, [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8]]);
        for (i, &b) in tile2.iter().enumerate() {
            vram.write(0x6000 + i as u16, b);
        }

        // Sprite 0 at (20,10), tile 0, attrs = palette 1 (bits 1-3).
        // (raw byte order: byte 0 = X, byte 1 = Y)
        oam.write(0, 20);
        oam.write(1, 10);
        oam.write(2, 0);
        oam.write(3, 0b0000_0010); // palette 1
        oam.write(512, 0x00);
        // Sprite 1 at (40,10), tile bit8 set via attr bit 0 -> tile 0x100
        // from the second table, palette 0, H-FLIP via bit 6.
        oam.write(4, 40);
        oam.write(5, 10);
        oam.write(6, 0);
        oam.write(7, 0b0100_0001); // hflip + tile bit 8
        // (sprite 1 shares high-table byte 512; both X high bits 0, small size)

        // OBJ palette 1, index 1 -> CGRAM 128 + 16 + 1 = 145.
        cgram.write(145 * 2, 0x1F);
        cgram.write(145 * 2 + 1, 0x00);
        // OBJ palette 0, index 1 -> CGRAM 129.
        cgram.write(129 * 2, 0xE0);
        cgram.write(129 * 2 + 1, 0x03);

        let mut regs = PpuRegisters::default();
        regs.inidisp = 0x0F;
        regs.obsel = 0x01; // size pair 0 (8x8), name 0, BASE = word 0x2000
        regs.tm = 0x10;

        let fb = render_frame(&vram, &cgram, &oam, &regs);

        // Sprite 0's pixel must use OBJ palette 1 (CGRAM 145).
        let expected_pal1 = bgr555_to_rgb8(cgram.read_color(145));
        let i0 = (10 * SCREEN_WIDTH + 20) * 4;
        assert_eq!((fb[i0], fb[i0 + 1], fb[i0 + 2]), expected_pal1,
            "attr bits 1-3 must select the OBJ palette");

        // Sprite 1: tile 0x100 comes from the second table; its source
        // pixel at x=1 lands at x = 7-1 = 6 after the H-flip (attr bit 6).
        let expected_pal0 = bgr555_to_rgb8(cgram.read_color(129));
        let i1 = (10 * SCREEN_WIDTH + 40 + 6) * 4;
        assert_eq!((fb[i1], fb[i1 + 1], fb[i1 + 2]), expected_pal0,
            "attr bit 0 must select the second tile table and bit 6 must H-flip");
    }

    #[test]
    fn large_sprite_tile_column_wraps_within_low_nibble() {
        // Regression guard for the multi-tile sprite tile-number bug: the
        // column component must wrap WITHIN the tile number's low nibble,
        // not carry into the row/table bits. Base tile 0x0E plus a column
        // offset of 3 (the 4th 8x8 cell of a 32x32 sprite) must land on
        // tile (0x0E + 3) & 0x0F = 0x01, NOT the unwrapped 0x0E + 3 = 0x11.
        let mut vram = Vram::new();
        let mut cgram = Cgram::new();
        let mut oam = Oam::new();

        // Tile index 1 (the CORRECT wrapped target): pixel value 1 at (0,0).
        let mut tile1_row0 = [0u8; 8];
        tile1_row0[0] = 1;
        let tile1 = make_2bpp_tile([tile1_row0, [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8]]);
        for (i, &b) in tile1.iter().enumerate() {
            vram.write(1 * 32 + i as u16, b); // 4bpp tile 1 starts at byte 32
        }
        // Tile index 0x11 = 17 (the WRONG unwrapped target) is left all
        // zero/transparent, so if the bug regresses the sprite pixel
        // silently disappears instead of showing the wrong graphics.

        // Sprite 0 at (20,10), base tile 0x0E, palette 0, no flip.
        // (raw byte order: byte 0 = X, byte 1 = Y)
        oam.write(0, 20); // X
        oam.write(1, 10); // Y
        oam.write(2, 0x0E); // tile low byte
        oam.write(3, 0x00); // attrs: palette 0, no flip, tile bit 8 = 0
        // Secondary OAM byte for sprites 0-3: sprite 0's size bit (bit 1)
        // set -> large size; X high bit (bit 0) clear.
        oam.write(512, 0x02);

        // OBJ palette 0, pixel index 1 -> CGRAM 128 + 1 = 129.
        cgram.write(129 * 2, 0x1F);
        cgram.write(129 * 2 + 1, 0x00);

        let mut regs = PpuRegisters::default();
        regs.inidisp = 0x0F;
        regs.obsel = 0x20; // size-pair code 1 (8x8/32x32), tile base word 0
        regs.tm = 0x10; // enable sprites only

        let fb = render_frame(&vram, &cgram, &oam, &regs);

        // Screen pixel for sprite-local (tx=24, ty=0) -> tile_row_idx=0,
        // tile_col_idx=3 -> must read wrapped tile index 1.
        let expected = bgr555_to_rgb8(cgram.read_color(129));
        let idx = (10 * SCREEN_WIDTH + 20 + 24) * 4;
        assert_eq!((fb[idx], fb[idx + 1], fb[idx + 2]), expected,
            "column offset must wrap within the tile number's low nibble, not carry into higher bits");
    }

    #[test]
    fn per_scanline_register_bands_render_each_row_with_its_own_state() {
        // The banded renderer must apply each scanline's captured register
        // state to that scanline only -- this is what makes SMW's
        // IRQ-driven status-bar split (different BG3 scroll above/below
        // the bar) and HDMA effects renderable at all with a
        // snapshot-based renderer.
        let mut vram = Vram::new();
        let mut cgram = Cgram::new();
        let oam = Oam::new();

        // Solid 4bpp-compatible tile (value 1) as tile 1 (tile 0 stays
        // all-transparent, since the zero-filled tilemap references it
        // everywhere); only map entry (0,0) uses the solid tile, so it is
        // visible exactly when hofs/vofs = 0.
        let solid = make_2bpp_tile([[1u8; 8]; 8]);
        for (i, &b) in solid.iter().enumerate() {
            vram.write(32 + i as u16, b); // 4bpp tile 1 starts at byte 32
        }
        vram.write_word(0x400 * 2, 0x0001);
        cgram.write(2, 0x1F); // CGRAM 1 = red
        cgram.write(3, 0x00);

        let mut top = PpuRegisters::default();
        top.inidisp = 0x0F;
        top.bgmode = 1;
        top.bg_sc[0] = 0x04; // tilemap word 0x400, 32x32
        top.tm = 0x01;

        let mut bottom = top;
        bottom.bg_hofs[0] = 64; // scroll the tile out of view below the split

        let mut lines = vec![top; SCREEN_HEIGHT];
        for line in lines.iter_mut().skip(4) {
            *line = bottom;
        }

        let fb = render_frame_per_scanline(&vram, &cgram, &oam, &lines);
        let red = bgr555_to_rgb8(cgram.read_color(1));
        let backdrop = bgr555_to_rgb8(cgram.read_color(0));

        // Row 2 (above the split): tile pixel visible at x=0.
        let above = (2 * SCREEN_WIDTH) * 4;
        assert_eq!((fb[above], fb[above + 1], fb[above + 2]), red,
            "rows before the split must use the first band's scroll");
        // Row 6 (below the split): the tile is scrolled away, backdrop shows.
        let below = (6 * SCREEN_WIDTH) * 4;
        assert_eq!((fb[below], fb[below + 1], fb[below + 2]), backdrop,
            "rows after the split must use the second band's scroll");
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

        let mut regs = PpuRegisters::default();
        regs.inidisp = 0x0F; // full brightness, not blanked
        regs.bgmode = 1;
        regs.tm = 0x00; // nothing on the main screen except the backdrop
        // Color math: enable on backdrop (bit5), add (bit7=0), half (bit6).
        regs.cgadsub = 0x20 | 0x40;
        regs.cgwsel = 0x00; // blend with fixed COLDATA, not the subscreen
        // Fixed color = BGR555 (r=6, g=4, b=0).
        regs.coldata = 6 | (4 << 5);

        // Expected: per channel (backdrop + fixed) >> 1, clamped 0..31.
        let er = (10 + 6) >> 1; // 8
        let eg = (20 + 4) >> 1; // 12
        let eb = (0 + 0) >> 1; // 0
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
    fn oam_entry_byte_order_is_x_then_y_per_real_hardware() {
        // Pins the raw OAM byte order (fullsnes: byte 0 = X low 8 bits,
        // byte 1 = Y). The renderer used to read these swapped, which
        // transposed every sprite around the screen diagonal AND painted a
        // permanent garbage column at x=240: sprites parked offscreen with
        // Y=$F0 were drawn at X=240 with Y = whatever stale X they had.
        // Deliberately uses X != Y and an asymmetric assertion so a swap
        // cannot pass.
        let mut vram = Vram::new();
        for y in 0..8u16 {
            vram.write(y * 2, 0xFF); // tile 0: all pixels value 1
        }
        let mut cgram = Cgram::new();
        cgram.write(129 * 2, 0xFF);
        cgram.write(129 * 2 + 1, 0x7F);
        let mut oam = Oam::new();
        for s in 0..128u16 {
            oam.write(s * 4 + 1, 240); // park everything: Y byte = 240
        }
        // Sprite 0 raw bytes: [X=200, Y=50, tile=0, attr=0].
        oam.write(0, 200);
        oam.write(1, 50);
        oam.write(2, 0);
        oam.write(3, 0);
        oam.write(512, 0);

        let mut regs = PpuRegisters::default();
        regs.inidisp = 0x0F;
        regs.obsel = 0;
        regs.tm = 0x10;

        let fb = render_frame(&vram, &cgram, &oam, &regs);
        let at = |x: usize, y: usize| (fb[(y * SCREEN_WIDTH + x) * 4], fb[(y * SCREEN_WIDTH + x) * 4 + 1]) != (0, 0);
        assert!(
            at(200, 50),
            "raw OAM [200, 50] must place the sprite at X=200, Y=50"
        );
        assert!(
            !at(50, 200),
            "the transposed position must stay empty -- byte 0 is X, not Y"
        );
        // The parked sprites (Y byte = 240) must not paint anything at the
        // right edge -- the old swap drew them all in a column at x=240.
        for y in 0..SCREEN_HEIGHT {
            for x in 240..SCREEN_WIDTH {
                if y == 50 && x >= 200 {
                    continue; // (not reachable: sprite spans 200-207)
                }
                assert!(
                    !at(x, y),
                    "parked sprite leaked to ({}, {}) -- Y=240 must keep it offscreen",
                    x,
                    y
                );
            }
        }
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

        let mut regs = PpuRegisters::default();
        regs.inidisp = 0x0F;
        regs.obsel = 0x00;
        regs.tm = 0x10; // sprites only on main
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

    /// Writes mode-7 tile data: assigns `tile` to every map entry of the
    /// 128x128 field row `map_row`..(all rows if None isn't needed here),
    /// and fills the given tile's 64 pixels with `value`.
    fn write_mode7_tile(vram: &mut Vram, tile: u8, value: u8) {
        for i in 0..64u16 {
            vram.write(((tile as u16) * 64 + i) * 2 + 1, value);
        }
    }

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

        let mut regs = PpuRegisters::default();
        regs.inidisp = 0x0F;
        regs.bgmode = 7;
        regs.tm = 0x01; // BG1 only
        regs.m7a = 0x0100; // identity matrix (1.0 in 8.8 fixed point)
        regs.m7b = 0x0000;
        regs.m7c = 0x0000;
        regs.m7d = 0x0100;

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

        let mut regs = PpuRegisters::default();
        regs.inidisp = 0x0F;
        regs.bgmode = 7;
        regs.tm = 0x01;
        regs.m7a = 0x0200; // 2.0: horizontal zoom OUT (field moves 2px per screen px)
        regs.m7b = 0;
        regs.m7c = 0;
        regs.m7d = 0x0100;

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

        let mut regs = PpuRegisters::default();
        regs.inidisp = 0x0F;
        regs.bgmode = 7;
        regs.tm = 0x01;
        regs.m7a = 0x0100;
        regs.m7b = 0;
        regs.m7c = 0;
        regs.m7d = 0x0100;
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

        let mut regs = PpuRegisters::default();
        regs.inidisp = 0x0F;
        regs.bgmode = 7;
        regs.tm = 0x03; // BG1 + BG2
        regs.setini = 0x40; // EXTBG
        regs.m7a = 0x0100;
        regs.m7b = 0;
        regs.m7c = 0;
        regs.m7d = 0x0100;

        let fb = render_frame(&vram, &cgram, &oam, &regs);
        let green = bgr555_to_rgb8(cgram.read_color(0x25));
        assert_eq!((fb[0], fb[1], fb[2]), green,
            "an EXTBG pixel with bit 7 set must draw its BG2 slot in front of BG1");
    }

    /// A mode-1 setup with one solid red BG1 tile at the top-left, used by
    /// the window tests: returns (vram, cgram, regs) ready to render.
    fn solid_bg1_setup() -> (Vram, Cgram, PpuRegisters) {
        let mut vram = Vram::new();
        let mut cgram = Cgram::new();
        let solid = make_2bpp_tile([[1u8; 8]; 8]);
        for (i, &b) in solid.iter().enumerate() {
            vram.write(i as u16, b); // tile 0
        }
        // 32x32 tilemap at word 0x400, every entry -> tile 0.
        for entry in 0..(32u16 * 32) {
            vram.write_word((0x400 + entry) * 2, 0x0000);
        }
        cgram.write(2, 0x1F); // CGRAM 1 = red
        cgram.write(3, 0x00);

        let mut regs = PpuRegisters::default();
        regs.inidisp = 0x0F;
        regs.bgmode = 1;
        regs.bg_sc[0] = 0x04;
        regs.tm = 0x01;
        (vram, cgram, regs)
    }

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

    fn oam_empty() -> Oam {
        Oam::new()
    }

    /// Builds an OAM where each of `count` small sprites shows exactly ONE
    /// opaque pixel at its top-left corner (tile 0 must have pixel value 1
    /// at (0,0) only), sprite `i` at (x_step * i, 10). `large` also sets
    /// every sprite's size bit (OBSEL pair 0: 8x8 small / 16x16 large).
    fn oam_with_sprite_row(count: usize, x_step: u8, large: bool) -> Oam {
        let mut oam = Oam::new();
        for i in 0..count {
            let base = (i * 4) as u16;
            oam.write(base, (i as u8).wrapping_mul(x_step)); // X
            oam.write(base + 1, 10); // Y
            oam.write(base + 2, 0); // tile 0
            oam.write(base + 3, 0x00); // palette 0, priority 0
        }
        // Park every other sprite off-screen (Y = 0xF0 = -16 with an 8px
        // sprite ends at line -8, never visible).
        for i in count..128 {
            let base = (i * 4) as u16;
            oam.write(base + 1, 0xF0);
        }
        if large {
            for i in 0..count {
                let byte = 512 + (i as u16) / 4;
                let old = oam.read(byte);
                oam.write(byte, old | (0x02 << ((i % 4) * 2)));
            }
        }
        oam
    }

    fn single_pixel_sprite_setup() -> (Vram, Cgram, PpuRegisters) {
        let mut vram = Vram::new();
        let mut cgram = Cgram::new();
        // OBJ tile 0: pixel value 1 at (0,0), rest transparent.
        let mut tile_row0 = [0u8; 8];
        tile_row0[0] = 1;
        let tile = make_2bpp_tile([tile_row0, [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8], [0; 8]]);
        for (i, &b) in tile.iter().enumerate() {
            vram.write(i as u16, b);
        }
        cgram.write(129 * 2, 0xE0); // OBJ palette 0, pixel 1
        cgram.write(129 * 2 + 1, 0x03);
        let mut regs = PpuRegisters::default();
        regs.inidisp = 0x0F;
        regs.obsel = 0x00;
        regs.tm = 0x10; // sprites only
        (vram, cgram, regs)
    }

    #[test]
    fn sprite_range_limit_drops_the_33rd_sprite_on_a_line_and_flags_stat77() {
        // Hardware evaluates at most 32 sprites per scanline; a 33rd
        // in-range sprite is not drawn and sets STAT77 bit 6 (range over).
        let (vram, cgram, regs) = single_pixel_sprite_setup();
        let oam = oam_with_sprite_row(33, 7, false); // all 33 share lines 10-17
        let lines = vec![regs; SCREEN_HEIGHT];
        let (fb, flags) = render_frame_per_scanline_with_status(&vram, &cgram, &oam, &lines);

        let drawn = (10 * SCREEN_WIDTH) * 4; // sprite 0's pixel at (0,10)
        assert_ne!((fb[drawn], fb[drawn + 1], fb[drawn + 2]), (0, 0, 0), "sprite 0 must render");
        let dropped = (10 * SCREEN_WIDTH + 32 * 7) * 4; // sprite 32's pixel
        assert_eq!(
            (fb[dropped], fb[dropped + 1], fb[dropped + 2]),
            (0, 0, 0),
            "the 33rd sprite on the line must be dropped by the range limit"
        );
        assert_ne!(flags & 0x40, 0, "STAT77 range-over flag must be set");
        assert_eq!(flags & 0x80, 0, "33 8x8 sprites = 32 evaluated tiles: no time-over");
    }

    #[test]
    fn sprite_tile_budget_drops_sprites_past_34_tiles_and_flags_time_over() {
        // 18 sprites of 16x16 = 2 tiles each on one line: the first 17
        // consume the full 34-tile budget, the 18th is dropped and STAT77
        // bit 7 (time over) sets.
        let (vram, cgram, regs) = single_pixel_sprite_setup();
        let oam = oam_with_sprite_row(18, 14, true);
        let lines = vec![regs; SCREEN_HEIGHT];
        let (fb, flags) = render_frame_per_scanline_with_status(&vram, &cgram, &oam, &lines);

        let drawn = (10 * SCREEN_WIDTH) * 4;
        assert_ne!((fb[drawn], fb[drawn + 1], fb[drawn + 2]), (0, 0, 0), "sprite 0 must render");
        let dropped = (10 * SCREEN_WIDTH + 17 * 14) * 4; // sprite 17's top-left pixel
        assert_eq!(
            (fb[dropped], fb[dropped + 1], fb[dropped + 2]),
            (0, 0, 0),
            "the sprite past the 34-tile budget must be dropped"
        );
        assert_ne!(flags & 0x80, 0, "STAT77 time-over flag must be set");
    }

    #[test]
    fn first_sprite_rotation_changes_which_sprite_wins_overlaps() {
        // With $2103 bit 7 priority rotation, evaluation (and overlap
        // priority) starts at FirstSprite instead of sprite 0.
        let (vram, mut cgram, mut regs) = single_pixel_sprite_setup();
        // Sprite 1 uses OBJ palette 1 so the two sprites are colorimetrically
        // distinguishable (CGRAM 128 + 16 + 1 = 145).
        cgram.write(145 * 2, 0x1F);
        cgram.write(145 * 2 + 1, 0x00);

        let mut oam = oam_empty();
        for i in 0..2u16 {
            oam.write(i * 4, 20); // both at (20, 10)
            oam.write(i * 4 + 1, 10);
            oam.write(i * 4 + 2, 0);
            oam.write(i * 4 + 3, if i == 1 { 0b0000_0010 } else { 0 }); // sprite 1: palette 1
        }
        for i in 2..128u16 {
            oam.write(i * 4 + 1, 0xF0);
        }

        let idx = (10 * SCREEN_WIDTH + 20) * 4;
        let fb = render_frame(&vram, &cgram, &oam, &regs);
        let sprite0_color = bgr555_to_rgb8(cgram.read_color(129));
        assert_eq!((fb[idx], fb[idx + 1], fb[idx + 2]), sprite0_color, "FirstSprite 0: sprite 0 wins the overlap");

        regs.first_sprite = 1;
        let fb = render_frame(&vram, &cgram, &oam, &regs);
        let sprite1_color = bgr555_to_rgb8(cgram.read_color(145));
        assert_eq!((fb[idx], fb[idx + 1], fb[idx + 2]), sprite1_color, "FirstSprite 1: sprite 1 now has the highest priority");
    }

    #[test]
    fn offset_per_tile_mode2_overrides_bg1_h_scroll_per_column() {
        // Mode 2: BG3's tilemap supplies per-8-pixel-column scroll
        // overrides. Column 0 always uses the normal scroll; column 1's
        // override entry (BG3 map tile (0,0)) redirects BG1's horizontal
        // offset so the solid tile at world column 0 repeats there;
        // column 2 has no valid override and stays at the normal scroll.
        let mut vram = Vram::new();
        let mut cgram = Cgram::new();

        // BG1 4bpp tile 1: solid pixel value 1 (plane 0 = 0xFF each row).
        for row in 0..8u16 {
            vram.write(32 + row * 2, 0xFF);
        }
        // BG1 tilemap at word 0x400: tile 1 at map position (0,0) only.
        vram.write(0x800, 0x01);
        vram.write(0x801, 0x00);
        // BG3 tilemap at word 0x800: OPT entry for screen column 1 --
        // valid-for-BG1 (bit 13) + H offset 0x3F8 (walks the sample back
        // to world column 0: 8 + 1016 wraps to tile 0 of the 256-wide map).
        vram.write(0x1000, 0xF8);
        vram.write(0x1001, 0x23);

        cgram.write(1 * 2, 0xE0); // BG palette 0, pixel 1
        cgram.write(1 * 2 + 1, 0x03);

        let mut regs = PpuRegisters::default();
        regs.inidisp = 0x0F;
        regs.bgmode = 2;
        regs.tm = 0x01; // BG1 only
        regs.bg_sc[0] = 0x04; // BG1 tilemap base word 0x400
        regs.bg_sc[2] = 0x08; // BG3 tilemap base word 0x800 (the OPT table)

        let fb = render_frame(&vram, &cgram, &oam_empty(), &regs);
        let color = bgr555_to_rgb8(cgram.read_color(1));
        let px = |x: usize| {
            let i = (0 * SCREEN_WIDTH + x) * 4;
            (fb[i], fb[i + 1], fb[i + 2])
        };
        assert_eq!(px(0), color, "column 0 uses the normal (zero) scroll: tile 1 shows");
        assert_eq!(px(8), color, "column 1's OPT entry must override BG1's H offset");
        assert_eq!(px(16), (0, 0, 0), "column 2 has no valid OPT entry: backdrop");
    }

    #[test]
    fn bgmode_size_bit_selects_16x16_tiles_with_obj_style_cell_layout() {
        // Mode 1 with BGMODE bit 4 (BG1 16x16 tiles): the tile's four 8x8
        // cells are base, base+1, base+16, base+17 -- pixel (8,0) must
        // come from tile base+1 and pixel (0,8) from tile base+16.
        let mut vram = Vram::new();
        let mut cgram = Cgram::new();
        let oam = Oam::new();

        let solid = make_2bpp_tile([[1u8; 8]; 8]);
        // 4bpp tiles are 32 bytes each. Tile 2 (base+1) and tile 17
        // (base+16) are solid; base tile 1 and tile 18 stay transparent.
        for (i, &b) in solid.iter().enumerate() {
            vram.write(2 * 32 + i as u16, b);
            vram.write(17 * 32 + i as u16, b);
        }
        vram.write_word(0x400 * 2, 0x0001); // map (0,0) -> base tile 1
        cgram.write(2, 0x1F); // CGRAM 1 = red
        cgram.write(3, 0x00);

        let mut regs = PpuRegisters::default();
        regs.inidisp = 0x0F;
        regs.bgmode = 0x11; // mode 1 + BG1 16x16 tiles
        regs.bg_sc[0] = 0x04;
        regs.tm = 0x01;

        let fb = render_frame(&vram, &cgram, &oam, &regs);
        let red = bgr555_to_rgb8(cgram.read_color(1));
        let backdrop = bgr555_to_rgb8(cgram.read_color(0));

        let at = |x: usize, y: usize| {
            let i = (y * SCREEN_WIDTH + x) * 4;
            (fb[i], fb[i + 1], fb[i + 2])
        };
        assert_eq!(at(0, 0), backdrop, "cell (0,0) is the transparent base tile");
        assert_eq!(at(8, 0), red, "cell (1,0) must be tile base+1");
        assert_eq!(at(0, 8), red, "cell (0,1) must be tile base+16");
        assert_eq!(at(8, 8), backdrop, "cell (1,1) is the transparent base+17");
    }

    #[test]
    fn mode5_hires_maps_two_dots_per_output_pixel() {
        // Mode 5: tiles are 16 dots wide in a 512-dot space; each output
        // pixel covers two dots. With the LEFT 8x8 cell solid and the
        // right transparent, output pixels 0-3 (dots 0-7) show the color
        // and pixels 4-7 (dots 8-15) show the backdrop.
        let mut vram = Vram::new();
        let mut cgram = Cgram::new();
        let oam = Oam::new();

        let solid = make_2bpp_tile([[1u8; 8]; 8]);
        for (i, &b) in solid.iter().enumerate() {
            vram.write(1 * 32 + i as u16, b); // 4bpp tile 1 solid (left cell)
        }
        vram.write_word(0x400 * 2, 0x0001); // map (0,0) -> base tile 1 (cells 1,2)
        cgram.write(2, 0x1F);
        cgram.write(3, 0x00);

        let mut regs = PpuRegisters::default();
        regs.inidisp = 0x0F;
        regs.bgmode = 5; // BG1 = 4bpp, 16-wide tiles
        regs.bg_sc[0] = 0x04;
        regs.tm = 0x01;

        let fb = render_frame(&vram, &cgram, &oam, &regs);
        let red = bgr555_to_rgb8(cgram.read_color(1));
        let backdrop = bgr555_to_rgb8(cgram.read_color(0));
        let at = |x: usize| {
            let i = x * 4;
            (fb[i], fb[i + 1], fb[i + 2])
        };
        assert_eq!(at(0), red, "dots 0/1 (left cell) -> output pixel 0");
        assert_eq!(at(3), red, "dots 6/7 (left cell) -> output pixel 3");
        assert_eq!(at(4), backdrop, "dots 8/9 (transparent right cell) -> output pixel 4");
    }

    #[test]
    fn mode5_hires_averages_the_two_dots_of_each_output_pixel() {
        // A tile row alternating between two palette indices means every
        // output pixel spans one dot of each color -- the result must be
        // their per-channel average, not either dot alone.
        let mut vram = Vram::new();
        let mut cgram = Cgram::new();
        let oam = Oam::new();

        let tile = make_2bpp_tile([[1, 2, 1, 2, 1, 2, 1, 2]; 8]);
        for (i, &b) in tile.iter().enumerate() {
            vram.write(1 * 32 + i as u16, b);
        }
        vram.write_word(0x400 * 2, 0x0001);
        // Color 1 = pure red (r=31), color 2 = pure blue (b=31).
        cgram.write(2, 0x1F);
        cgram.write(3, 0x00);
        cgram.write(4, 0x00);
        cgram.write(5, 0x7C);

        let mut regs = PpuRegisters::default();
        regs.inidisp = 0x0F;
        regs.bgmode = 5;
        regs.bg_sc[0] = 0x04;
        regs.tm = 0x01;

        let fb = render_frame(&vram, &cgram, &oam, &regs);
        // Average of (31,0,0) and (0,0,31) in 5-bit space = (15,0,15).
        let expected = bgr555_to_rgb8(15 | (15 << 10));
        assert_eq!((fb[0], fb[1], fb[2]), expected,
            "output pixel 0 must average its red and blue dots");
    }

    #[test]
    fn mode5_interlace_averages_both_field_lines() {
        // With SETINI bit 0 (interlace) in mode 5, each output row spans
        // two half-lines (the two fields). A tile whose row 0 is red and
        // row 1 is blue must render output row 0 as their average; without
        // the interlace bit, row 0 is pure red.
        let mut vram = Vram::new();
        let mut cgram = Cgram::new();
        let oam = Oam::new();

        let mut rows = [[0u8; 8]; 8];
        rows[0] = [1; 8];
        rows[1] = [2; 8];
        let tile = make_2bpp_tile(rows);
        for (i, &b) in tile.iter().enumerate() {
            vram.write(1 * 32 + i as u16, b);
        }
        vram.write_word(0x400 * 2, 0x0001);
        cgram.write(2, 0x1F); // color 1 = red
        cgram.write(3, 0x00);
        cgram.write(4, 0x00); // color 2 = blue
        cgram.write(5, 0x7C);

        let mut regs = PpuRegisters::default();
        regs.inidisp = 0x0F;
        regs.bgmode = 5;
        regs.bg_sc[0] = 0x04;
        regs.tm = 0x01;

        let fb = render_frame(&vram, &cgram, &oam, &regs);
        let red = bgr555_to_rgb8(cgram.read_color(1));
        assert_eq!((fb[0], fb[1], fb[2]), red, "non-interlaced row 0 is the tile's row 0");

        regs.setini = 0x01; // interlace
        let fb2 = render_frame(&vram, &cgram, &oam, &regs);
        let expected = bgr555_to_rgb8(15 | (15 << 10)); // avg of red and blue
        assert_eq!((fb2[0], fb2[1], fb2[2]), expected,
            "interlaced row 0 must average the tile's rows 0 and 1 (the two fields)");
    }

    #[test]
    fn mosaic_repeats_each_blocks_top_left_pixel() {
        // BG1 with a single red tile at map (0,0): normally pixel (8,0) is
        // backdrop (tile 1 of the map is transparent). With an 8x8 mosaic
        // whose block origin (8,0) is transparent, nothing changes there --
        // but pixel (7,0)..(0,0) belong to block origin (0,0), so the whole
        // first block is red. More telling: with mosaic 16x16, pixel
        // (12, 12) samples block origin (0,0) -- INSIDE the tile -- so it
        // must be red even though the un-mosaicked tile only spans 8x8.
        let mut vram = Vram::new();
        let mut cgram = Cgram::new();
        let oam = Oam::new();

        let solid = make_2bpp_tile([[1u8; 8]; 8]);
        for (i, &b) in solid.iter().enumerate() {
            vram.write(32 + i as u16, b); // 4bpp tile 1 (bytes 32..)
        }
        vram.write_word(0x400 * 2, 0x0001); // map (0,0) -> tile 1
        cgram.write(2, 0x1F); // CGRAM 1 = red
        cgram.write(3, 0x00);

        let mut regs = PpuRegisters::default();
        regs.inidisp = 0x0F;
        regs.bgmode = 1;
        regs.bg_sc[0] = 0x04;
        regs.tm = 0x01;
        regs.mosaic = 0xF1; // size 16, enabled for BG1

        let fb = render_frame(&vram, &cgram, &oam, &regs);
        let red = bgr555_to_rgb8(cgram.read_color(1));
        let idx = (12 * SCREEN_WIDTH + 12) * 4;
        assert_eq!((fb[idx], fb[idx + 1], fb[idx + 2]), red,
            "pixel (12,12) must repeat block origin (0,0)'s red with 16x16 mosaic");

        // Without mosaic the same pixel is backdrop (outside the 8x8 tile).
        regs.mosaic = 0x00;
        let fb2 = render_frame(&vram, &cgram, &oam, &regs);
        let backdrop = bgr555_to_rgb8(cgram.read_color(0));
        assert_eq!((fb2[idx], fb2[idx + 1], fb2[idx + 2]), backdrop);
    }

    #[test]
    fn mode1_high_priority_bg3_tile_draws_in_front_of_bg1() {
        // Per-tile priority regression guard: in mode 1 with the BGMODE
        // bit-3 BG3-priority flag set, a BG3 tile whose tilemap priority
        // bit is set is the frontmost layer -- it must overwrite a BG1
        // pixel at the same location. The old renderer used a fixed
        // BG4<BG3<BG2<BG1 order and always drew BG1 on top, which is wrong
        // for this (very common in SMW) configuration.
        let mut vram = Vram::new();
        let mut cgram = Cgram::new();
        let oam = Oam::new();

        // Two distinct solid tiles: tile 0 (all pixel value 1) for BG1,
        // tile 1 (all pixel value 1) for BG3 -- same pixel value, different
        // palettes so we can tell which layer won.
        let solid = make_2bpp_tile([[1u8; 8]; 8]);
        for (i, &b) in solid.iter().enumerate() {
            vram.write(i as u16, b); // tile 0 at bytes 0..
        }

        // BG1 tilemap at word 0x1000, entry 0 -> tile 0, palette 0, priority 0.
        vram.write_word(0x1000 * 2, 0x0000);
        // BG3 tilemap at word 0x2000, entry 0 -> tile 0, palette 1, priority 1 (0x2000).
        vram.write_word(0x2000 * 2, 0x2000 | (1 << 10));

        // BG1 palette 0 index 1 -> CGRAM 1 = red-ish; BG3 palette 1 (2bpp)
        // index 1 -> CGRAM (1*4 + 1) = 5 = green-ish. Distinct colors.
        cgram.write(1 * 2, 0x1F); cgram.write(1 * 2 + 1, 0x00); // CGRAM1 = red
        cgram.write(5 * 2, 0xE0); cgram.write(5 * 2 + 1, 0x03); // CGRAM5 = green

        let mut regs = PpuRegisters::default();
        regs.inidisp = 0x0F;
        regs.bgmode = 0x09; // mode 1 + BG3 priority flag (bit 3)
        regs.bg_sc[0] = 0x10; // BG1 tilemap base word = (0x10>>2)*0x400 = 0x1000
        regs.bg_sc[2] = 0x20; // BG3 tilemap base word = (0x20>>2)*0x400 = 0x2000
        regs.bg12nba = 0x00; // BG1 tile data base word 0
        regs.bg34nba = 0x00; // BG3 tile data base word 0
        regs.tm = 0x05; // enable BG1 (bit0) + BG3 (bit2)

        let fb = render_frame(&vram, &cgram, &oam, &regs);
        let green = bgr555_to_rgb8(cgram.read_color(5));
        assert_eq!((fb[0], fb[1], fb[2]), green,
            "high-priority BG3 must render in front of BG1 in mode 1 with the BG3-priority flag set");
    }
}
