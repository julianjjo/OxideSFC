use super::*;

#[test]
fn ppu_default_is_ntsc() {
    let ppu = Ppu::new();
    assert_eq!(ppu.mode(), PpuMode::Ntsc);
    assert_eq!(ppu.scanlines_per_frame(), 262);
}

#[test]
fn ppu_pal_mode() {
    let ppu = Ppu::new_pal();
    assert_eq!(ppu.mode(), PpuMode::Pal);
    assert_eq!(ppu.scanlines_per_frame(), 312);
}

#[test]
fn ppu_initial_state() {
    let ppu = Ppu::new();
    assert_eq!(ppu.scanline(), 0);
    assert_eq!(ppu.h_counter(), 0);
    assert_eq!(ppu.frame(), 0);
    assert!(!ppu.is_frame_ready());
}

#[test]
fn ppu_tick() {
    let mut ppu = Ppu::new();
    
    // Tick a few times
    ppu.tick();
    assert_eq!(ppu.h_counter(), 1);
    
    ppu.tick();
    assert_eq!(ppu.h_counter(), 2);
}

#[test]
fn ppu_scanline_wrap() {
    let mut ppu = Ppu::new();
    
    // Advance past end of scanline
    for _ in 0..Ppu::pixels_per_line() {
        ppu.tick();
    }
    
    assert_eq!(ppu.h_counter(), 0);
    assert_eq!(ppu.scanline(), 1);
}

#[test]
fn ppu_frame_complete() {
    let mut ppu = Ppu::new();
    
    // Advance to end of frame (262 scanlines * 341 dots)
    let pixels_per_frame = 262u32 * 341;
    
    for _ in 0..pixels_per_frame {
        ppu.tick();
    }
    
    assert_eq!(ppu.scanline(), 0);
    assert_eq!(ppu.h_counter(), 0);
    assert_eq!(ppu.frame(), 1);
    assert!(ppu.is_frame_ready());
}

#[test]
fn frame_ready_latches_on_vblank_entry_not_at_the_frame_wrap() {
    // The frontend renders as soon as this flag appears, and
    // `SystemBus::render_frame` reads VRAM/OAM live. Latching at the
    // frame wrap therefore rendered with the tile and sprite data the
    // game uploaded during that vblank for the NEXT frame -- see
    // `Ppu::tick`.
    let mut ppu = Ppu::new();

    // One dot short of entering vblank: nothing ready yet.
    for _ in 0..(224 * Ppu::pixels_per_line() as u32 - 1) {
        ppu.tick();
    }
    assert!(!ppu.is_frame_ready(), "the picture is still being scanned out");

    ppu.tick();
    assert_eq!(ppu.scanline(), 224, "sanity: line 224 is NTSC vblank entry");
    assert!(
        ppu.is_frame_ready(),
        "the finished picture must be ready at vblank entry"
    );

    // The latch must fire exactly once per frame: nothing re-latches
    // through the rest of vblank, including the wrap itself.
    ppu.clear_frame_ready();
    for _ in 0..((262 - 224) * Ppu::pixels_per_line() as u32) {
        ppu.tick();
    }
    assert_eq!(ppu.scanline(), 0, "sanity: back at the top of the frame");
    assert!(
        !ppu.is_frame_ready(),
        "the frame wrap must not latch a second frame"
    );

    // And the next frame's vblank entry latches again.
    for _ in 0..(224 * Ppu::pixels_per_line() as u32) {
        ppu.tick();
    }
    assert!(ppu.is_frame_ready(), "the following frame must latch too");
}

#[test]
fn ppu_clear_frame_ready() {
    let mut ppu = Ppu::new();
    
    // Advance to end of frame
    for _ in 0..(262 * 341) {
        ppu.tick();
    }
    
    assert!(ppu.is_frame_ready());
    
    // Clear and verify
    ppu.clear_frame_ready();
    assert!(!ppu.is_frame_ready());
}

#[test]
fn ppu_vblank() {
    let mut ppu = Ppu::new();
    
    // Scanline 224 starts vblank for NTSC
    for _ in 0..(224 * 341) {
        ppu.tick();
    }
    
    assert!(ppu.in_vblank());
    assert_eq!(ppu.scanline(), 224);
}

#[test]
fn ppu_hblank() {
    let mut ppu = Ppu::new();

    // The HBlank flag window is dot 274 through dot 0 of the next
    // line (the PPU keeps fetching next-line data until ~274), NOT
    // dot 256 where the visible picture ends.
    assert!(ppu.in_hblank(), "dot 0 is still inside the previous line's hblank window");
    ppu.tick();
    assert!(!ppu.in_hblank(), "dot 1 leaves hblank");
    for _ in 1..256 {
        ppu.tick();
    }
    assert_eq!(ppu.h_counter(), 256);
    assert!(!ppu.in_hblank(), "dot 256 (end of picture) is not yet hblank");
    for _ in 256..274 {
        ppu.tick();
    }
    assert_eq!(ppu.h_counter(), 274);
    assert!(ppu.in_hblank(), "hblank begins at dot 274");
}

#[test]
fn ppu_reset() {
    let mut ppu = Ppu::new();
    
    // Advance to some state
    for _ in 0..1000 {
        ppu.tick();
    }
    
    // Write something to memory
    ppu.vram().write(0x1000, 0xAB);
    
    // Reset
    ppu.reset();
    
    assert_eq!(ppu.scanline(), 0);
    assert_eq!(ppu.h_counter(), 0);
    assert_eq!(ppu.frame(), 0);
    assert!(!ppu.is_frame_ready());
}

#[test]
fn ppu_mode_switch() {
    let mut ppu = Ppu::new();
    
    // Switch to PAL
    ppu.set_pal();
    assert_eq!(ppu.mode(), PpuMode::Pal);
    assert_eq!(ppu.scanlines_per_frame(), 312);
    
    // Switch back to NTSC
    ppu.set_ntsc();
    assert_eq!(ppu.mode(), PpuMode::Ntsc);
    assert_eq!(ppu.scanlines_per_frame(), 262);
}

#[test]
fn ppu_vram_access() {
    let mut ppu = Ppu::new();
    
    ppu.vram().write(0x1234, 0xAB);
    assert_eq!(ppu.vram_ref().read(0x1234), 0xAB);
}

#[test]
fn ppu_cgram_access() {
    let mut ppu = Ppu::new();
    
    ppu.cgram().write(0x00, 0xAB);
    assert_eq!(ppu.cgram_ref().read(0x00), 0xAB);
}

#[test]
fn ppu_oam_access() {
    let mut ppu = Ppu::new();
    
    ppu.oam().write(0x00, 0xAB);
    assert_eq!(ppu.oam_ref().read(0x00), 0xAB);
}

#[test]
fn ppu_tick_n() {
    let mut ppu = Ppu::new();
    
    ppu.tick_n(100);
    assert_eq!(ppu.h_counter(), 100);
}

#[test]
fn ppu_constants() {
    assert_eq!(Ppu::pixels_per_line(), 341);
}
