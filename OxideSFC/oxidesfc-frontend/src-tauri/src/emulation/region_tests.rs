//! Region/video-standard selection, which decides how fast a game runs.

use super::snes::{target_fps, video_mode_for_region};
use oxidesfc_core::PpuMode;


#[test]
fn region_byte_selects_the_video_standard_bsnes_would() {
    // bsnes' `SuperFamicom::videoRegion` table. Getting this backwards
    // is very visible: a 50 Hz standard on an NTSC game runs it 17%
    // slow, and 60 Hz on a PAL game runs it 20% fast (the bug this
    // table fixes -- nothing selected PAL at all before).
    for code in [0x00u8, 0x01, 0x0B, 0x0D, 0x0F, 0x10] {
        assert_eq!(
            video_mode_for_region(code),
            PpuMode::Ntsc,
            "region 0x{:02X} is a 60 Hz territory",
            code
        );
    }
    for code in [0x02u8, 0x03, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0C, 0x11, 0x12] {
        assert_eq!(
            video_mode_for_region(code),
            PpuMode::Pal,
            "region 0x{:02X} is a 50 Hz territory",
            code
        );
    }
}

#[test]
fn each_video_standard_paces_at_its_own_frame_rate() {
    assert!((target_fps(PpuMode::Ntsc) - 60.0988).abs() < 0.001);
    assert!((target_fps(PpuMode::Pal) - 50.007).abs() < 0.001);
}
