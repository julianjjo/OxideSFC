//! Per-layer window masks: the two window ranges, their inversion bits and
//! the WBGLOG/WOBJLOG combination operators.

use super::SCREEN_WIDTH;
use crate::ppu::PpuRegisters;

/// Computes one scanline's combined window membership for a maskable
/// layer: `layer` 0-3 = BG1-4, 4 = OBJ, 5 = the color/math window.
/// `true` at an X means "this pixel is inside the (combined) window
/// area". Windows are purely horizontal on real hardware, so one line
/// serves every scanline of a band. Enable/invert bits come from
/// W12SEL/W34SEL/WOBJSEL (2 bits per window per layer) and the two-window
/// combine operation from WBGLOG/WOBJLOG (OR/AND/XOR/XNOR). With neither
/// window enabled the result is all-false (nothing masked).
pub(super) fn window_line(regs: &PpuRegisters, layer: usize) -> [bool; SCREEN_WIDTH] {
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

