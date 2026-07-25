//! DMA and HDMA behavior, including their real per-byte cycle cost and the
//! interaction between the two.

use super::common::tick_dots;
use crate::bus::{MemoryBus, SystemBus};

#[test]
fn dma_fixed_source_fill_writes_the_same_byte_across_vram() {
    // DMAP bits 4-3 are a 2-bit A-bus step FIELD: 01 (dmap=$08/$09)
    // means FIXED source. SMW clears its layer tilemaps with exactly
    // this (one constant byte streamed $1000 times to $2118/$2119) --
    // an earlier version misread bit 3 as "B->A direction" and
    // silently skipped these fills entirely, leaving stale garbage in
    // every tilemap the game thought it had cleared.
    let mut bus = SystemBus::new();
    bus.write_u8(0x7E0010, 0x5A).unwrap(); // the fill byte, in WRAM

    bus.write_u8(0x002115, 0x80).unwrap(); // VMAIN: word step on high byte
    bus.write_u8(0x002116, 0x00).unwrap(); // VMADD = word 0x0100
    bus.write_u8(0x002117, 0x01).unwrap();

    bus.write_u8(0x004300, 0x09).unwrap(); // DMAP0: fixed source, mode 1
    bus.write_u8(0x004301, 0x18).unwrap(); // BBAD0: $2118/$2119
    bus.write_u8(0x004302, 0x10).unwrap(); // A1T = $7E0010
    bus.write_u8(0x004303, 0x00).unwrap();
    bus.write_u8(0x004304, 0x7E).unwrap();
    bus.write_u8(0x004305, 0x08).unwrap(); // DAS = 8 bytes = 4 words
    bus.write_u8(0x004306, 0x00).unwrap();
    bus.write_u8(0x00420B, 0x01).unwrap(); // fire channel 0

    for word in 0x0100u16..0x0104 {
        assert_eq!(
            bus.ppu_ref().vram_ref().read_word(word.wrapping_mul(2)),
            0x5A5A,
            "fixed-source fill must write the same byte to every word (word {:#06X})",
            word
        );
    }
}

#[test]
fn dma_decrement_mode_streams_the_source_backwards() {
    // DMAP bits 4-3 = 10 (dmap=$10 | mode) means the A-bus address
    // DECREMENTS -- previously misread as "fixed".
    let mut bus = SystemBus::new();
    bus.write_u8(0x7E0020, 0x11).unwrap();
    bus.write_u8(0x7E001F, 0x22).unwrap();
    bus.write_u8(0x7E001E, 0x33).unwrap();
    bus.write_u8(0x7E001D, 0x44).unwrap();

    bus.write_u8(0x002115, 0x80).unwrap();
    bus.write_u8(0x002116, 0x00).unwrap();
    bus.write_u8(0x002117, 0x02).unwrap(); // VMADD = word 0x0200

    bus.write_u8(0x004300, 0x11).unwrap(); // DMAP0: decrement, mode 1
    bus.write_u8(0x004301, 0x18).unwrap();
    bus.write_u8(0x004302, 0x20).unwrap(); // A1T = $7E0020, walking down
    bus.write_u8(0x004303, 0x00).unwrap();
    bus.write_u8(0x004304, 0x7E).unwrap();
    bus.write_u8(0x004305, 0x04).unwrap();
    bus.write_u8(0x004306, 0x00).unwrap();
    bus.write_u8(0x00420B, 0x01).unwrap();

    assert_eq!(bus.ppu_ref().vram_ref().read_word(0x0200 * 2), 0x2211,
        "first word = bytes at $20 (low) then $1F (high)");
    assert_eq!(bus.ppu_ref().vram_ref().read_word(0x0201 * 2), 0x4433,
        "second word = bytes at $1E then $1D -- the source must walk backwards");
}

#[test]
fn dma_mode1_transfer_uploads_real_rom_bytes_into_vram() {
    let mut bus = SystemBus::new();
    let mut rom = vec![0u8; 0x80000];
    // Distinctive payload at LoROM bank 0, $8000+ -- 4 bytes that must
    // end up in VRAM byte-for-byte if the transfer is wired correctly.
    rom[0x0000] = 0x11;
    rom[0x0001] = 0x22;
    rom[0x0002] = 0x33;
    rom[0x0003] = 0x44;
    bus.load_cartridge(rom).unwrap();

    // Target VRAM address $0000, increment by 1 word after high-byte write.
    bus.write_u8(0x002115, 0x80).unwrap();
    bus.write_u8(0x002116, 0x00).unwrap();
    bus.write_u8(0x002117, 0x00).unwrap();

    // DMA channel 0: mode 1 (word, alternates $2118/$2119), CPU->PPU,
    // dest BBAD=$18 (so $2118 then $2119), source = bank $00:$8000, 4 bytes.
    bus.write_u8(0x004300, 0x01).unwrap(); // DMAPx: mode 1, direction CPU->PPU
    bus.write_u8(0x004301, 0x18).unwrap(); // BBADx = $18 (VMDATAL)
    bus.write_u8(0x004302, 0x00).unwrap(); // A1Tx low
    bus.write_u8(0x004303, 0x80).unwrap(); // A1Tx high ($8000)
    bus.write_u8(0x004304, 0x00).unwrap(); // A1Bx = bank 0
    bus.write_u8(0x004305, 0x04).unwrap(); // DASx low = 4 bytes
    bus.write_u8(0x004306, 0x00).unwrap(); // DASx high

    bus.write_u8(0x00420B, 0x01).unwrap(); // MDMAEN: trigger channel 0

    assert_eq!(bus.ppu_ref().vram_ref().read(0x0000), 0x11);
    assert_eq!(bus.ppu_ref().vram_ref().read(0x0001), 0x22);
    assert_eq!(bus.ppu_ref().vram_ref().read(0x0002), 0x33);
    assert_eq!(bus.ppu_ref().vram_ref().read(0x0003), 0x44);

    // DAS must read back as 0 (transfer complete) per real hardware.
    assert_eq!(bus.read_u8(0x004305).unwrap(), 0x00);
    assert_eq!(bus.read_u8(0x004306).unwrap(), 0x00);
}

#[test]
fn dma_with_zero_das_transfers_a_full_64kb_block() {
    // Documented real-hardware behavior: DAS=0 means 0x10000 bytes,
    // not "nothing to transfer" -- games rely on this for full-VRAM
    // clears/fills using a single DMA trigger. Uses a fixed source
    // address (the real pattern for memory-clear/fill DMA) so the
    // 65536-byte transfer doesn't depend on how a single ROM bank's
    // address space is carved up between ROM and WRAM mirror/I-O.
    let mut bus = SystemBus::new();
    let rom = vec![0x7Eu8; 0x80000];
    bus.load_cartridge(rom).unwrap();

    bus.write_u8(0x002115, 0x80).unwrap();
    bus.write_u8(0x002116, 0x00).unwrap();
    bus.write_u8(0x002117, 0x00).unwrap();

    bus.write_u8(0x004300, 0x11).unwrap(); // mode 1, fixed source address
    bus.write_u8(0x004301, 0x18).unwrap();
    bus.write_u8(0x004302, 0x00).unwrap();
    bus.write_u8(0x004303, 0x80).unwrap();
    bus.write_u8(0x004304, 0x00).unwrap();
    bus.write_u8(0x004305, 0x00).unwrap(); // DAS = 0 -> 65536 bytes
    bus.write_u8(0x004306, 0x00).unwrap();

    bus.write_u8(0x00420B, 0x01).unwrap();

    // Every VRAM byte should now be 0x7E (all 65536 bytes transferred).
    assert_eq!(bus.ppu_ref().vram_ref().read(0x0000), 0x7E);
    assert_eq!(bus.ppu_ref().vram_ref().read(0xFFFF), 0x7E);
}

// ========================================================================
// HDMA tests
// ========================================================================

#[test]
fn hdma_direct_mode_non_repeat_entry_writes_once_then_waits_without_touching_the_bbus() {
    // Regression guard for the DKC intro table-desync bug: a
    // non-repeat entry (bit7 CLEAR, count $01-$80) transfers on its
    // FIRST line only -- the remaining "wait" lines perform no B-bus
    // writes at all -- and the table pointer must end up past the
    // entry's inline data so the next line-count read lands on the
    // real next entry, not on data bytes. The old engine transferred
    // on every line and never advanced the pointer, so tables like
    // DKC's `7F 03 18 03 03 03 00` slid out of sync the moment the
    // first entry expired (count bytes written to the PPU as data,
    // data bytes consumed as counts).
    let mut bus = SystemBus::new();

    // HDMA table in WRAM at $7E:2000: one non-repeat entry (bit7
    // clear) covering 2 lines with a single data byte 0xAA, then the
    // 0x00 end-of-table marker.
    bus.write_u8(0x7E2000, 0x02).unwrap(); // line-count=2, non-repeat
    bus.write_u8(0x7E2001, 0xAA).unwrap(); // the entry's single data byte
    bus.write_u8(0x7E2002, 0x00).unwrap(); // end of table

    // VRAM address 0, increment by 1 word after each low-byte write
    // (VMAIN default 0 already does this).
    bus.write_u8(0x002116, 0x00).unwrap();
    bus.write_u8(0x002117, 0x00).unwrap();

    // DMA channel 0: direct-addressing HDMA (DMAPx bit6=0), mode 0 (1
    // byte/line) into $2118 (VMDATAL), table starting at $7E:2000.
    bus.write_u8(0x004300, 0x00).unwrap();
    bus.write_u8(0x004301, 0x18).unwrap();
    bus.write_u8(0x004302, 0x00).unwrap();
    bus.write_u8(0x004303, 0x20).unwrap();
    bus.write_u8(0x004304, 0x7E).unwrap();

    // Arm HDMA for channel 0 via $420C -- during vblank, like real
    // games do (arming mid-frame runs the engine with uninitialized
    // channel state on real hardware too).
    tick_dots(&mut bus, 230 * 341); // into vblank (scanline 230)
    bus.write_u8(0x00420C, 0x01).unwrap();

    // Before any HDMA has run, VRAM must still be untouched.
    assert_eq!(bus.ppu_ref().vram_ref().read(0x0000), 0x00);

    // The frame cycle starts on the LAST internal scanline (hardware
    // V=0): crossing into it runs hdma_init, and its ~dot-276 HDMA
    // slot performs the fresh entry's first transfer.
    tick_dots(&mut bus, 31 * 341 + 100); // scanline 261, dot 100: init has run, transfer hasn't
    assert_eq!(bus.ppu_ref().vram_ref().read(0x0000), 0x00, "init alone must not transfer");
    tick_dots(&mut bus, 300); // cross dot 276 of scanline 261: the entry's one transfer

    assert_eq!(bus.ppu_ref().vram_ref().read(0x0000), 0xAA, "the entry's first line must write the table's data byte");

    tick_dots(&mut bus, 341); // cross scanline 0's HDMA slot: WAIT line (count 2->0 exhausts, reload reads 0x00 -> terminates)

    assert_eq!(
        bus.ppu_ref().vram_ref().read(0x0002),
        0x00,
        "a non-repeat entry's wait lines must not write the B-bus at all (the old engine re-wrote every line)"
    );

    tick_dots(&mut bus, 341); // cross scanline 1's HDMA slot: channel is terminated, must not transfer again

    assert_eq!(bus.ppu_ref().vram_ref().read(0x0002), 0x00, "a terminated channel must not keep transferring into subsequent scanlines");
    assert!(
        bus.dma_ref().channel(0).unwrap().hdma_terminated,
        "after the wait line, the reload must read the end-of-table marker (0x00) -- not the entry's own data byte -- and terminate"
    );
}

#[test]
fn hdma_direct_mode_repeat_entry_streams_fresh_data_each_line() {
    let mut bus = SystemBus::new();

    // Repeat entry (bit7 SET, $81-$FF): transfers on EVERY line of the
    // entry, consuming fresh data bytes from the table each line --
    // line-count=2 with 2 lines' worth of distinct data (0x11, 0x22)
    // following, then end-of-table.
    bus.write_u8(0x7E3000, 0x82).unwrap(); // 0x80 | 2 = repeat, 2 lines
    bus.write_u8(0x7E3001, 0x11).unwrap();
    bus.write_u8(0x7E3002, 0x22).unwrap();
    bus.write_u8(0x7E3003, 0x00).unwrap();

    bus.write_u8(0x002116, 0x00).unwrap();
    bus.write_u8(0x002117, 0x00).unwrap();

    bus.write_u8(0x004300, 0x00).unwrap();
    bus.write_u8(0x004301, 0x18).unwrap();
    bus.write_u8(0x004302, 0x00).unwrap();
    bus.write_u8(0x004303, 0x30).unwrap();
    bus.write_u8(0x004304, 0x7E).unwrap();

    tick_dots(&mut bus, 230 * 341); // into vblank, then arm
    bus.write_u8(0x00420C, 0x01).unwrap();

    tick_dots(&mut bus, 32 * 341); // crosses init + the pre-visible line's HDMA slot (1st transfer)
    assert_eq!(bus.ppu_ref().vram_ref().read(0x0000), 0x11);

    tick_dots(&mut bus, 341); // scanline 0's HDMA slot: 2nd line of the repeat entry
    assert_eq!(bus.ppu_ref().vram_ref().read(0x0002), 0x22, "a repeat entry must advance to the next table byte for each line");
}

#[test]
fn hdma_multi_byte_transfer_source_wraps_within_bank_not_into_next_bank() {
    // Mode 1 transfers 2 bytes/line (into $2118 then $2119). Set up a
    // "no-repeat" entry whose data bytes straddle the $7E/$7F bank
    // boundary: line-count at $7E:FFFE, first data byte at $7E:FFFF
    // (the last address in bank $7E), second data byte that MUST be
    // re-read from $7E:0000 (wrapping within the same bank, matching
    // real hardware and every other address-stepping path in this
    // file) rather than carrying into $7F:0000.
    let mut bus = SystemBus::new();

    bus.write_u8(0x7EFFFE, 0x81).unwrap(); // no-repeat, 1 line
    bus.write_u8(0x7EFFFF, 0x11).unwrap(); // 1st data byte (low, ->$2118)
    bus.write_u8(0x7E0000, 0x22).unwrap(); // 2nd data byte, correct same-bank wrap (->$2119)
    bus.write_u8(0x7F0000, 0xFF).unwrap(); // decoy: what the old 24-bit-carry bug would read instead

    // VMAIN = increment after the HIGH byte write, so both $2118 and
    // $2119 land on the SAME VRAM word (0) instead of the low-byte
    // write auto-advancing VMADD before the high byte is written.
    bus.write_u8(0x002115, 0x80).unwrap();
    bus.write_u8(0x002116, 0x00).unwrap();
    bus.write_u8(0x002117, 0x00).unwrap();

    bus.write_u8(0x004300, 0x01).unwrap(); // DMAPx: direct HDMA, mode 1 (2 bytes/line)
    bus.write_u8(0x004301, 0x18).unwrap(); // BBADx = $18 (VMDATAL/VMDATAH)
    bus.write_u8(0x004302, 0xFE).unwrap(); // A1T low = $FFFE
    bus.write_u8(0x004303, 0xFF).unwrap(); // A1T high
    bus.write_u8(0x004304, 0x7E).unwrap(); // A1B = bank $7E

    tick_dots(&mut bus, 230 * 341); // into vblank, then arm channel 0
    bus.write_u8(0x00420C, 0x01).unwrap();

    tick_dots(&mut bus, 32 * 341); // crosses init + the pre-visible line's HDMA slot: transfers both bytes

    assert_eq!(bus.ppu_ref().vram_ref().read(0x0000), 0x11, "1st byte, read from $7E:FFFF");
    assert_eq!(
        bus.ppu_ref().vram_ref().read(0x0001), 0x22,
        "2nd byte must wrap to $7E:0000 (same bank), not carry into $7F:0000 (which would read the 0xFF decoy)"
    );
}

// ========================================================================
// Joypad input tests
// ========================================================================

#[test]
fn dma_transfer_sets_done_flag_and_clears_active_flag_when_complete() {
    // `Dma::is_active()`/`check_done()` used to never be touched by
    // `execute_dma_channel`, so they permanently reported "never
    // active, never done" no matter what transfers actually ran.
    let mut bus = SystemBus::new();
    bus.write_u8(0x7E0010, 0x5A).unwrap();

    bus.write_u8(0x004300, 0x08).unwrap(); // fixed source, mode 0
    bus.write_u8(0x004301, 0x18).unwrap();
    bus.write_u8(0x004302, 0x10).unwrap();
    bus.write_u8(0x004303, 0x00).unwrap();
    bus.write_u8(0x004304, 0x7E).unwrap();
    bus.write_u8(0x004305, 0x04).unwrap();
    bus.write_u8(0x004306, 0x00).unwrap();

    assert!(!bus.dma_ref().check_done(), "no transfer has run yet");

    bus.write_u8(0x00420B, 0x01).unwrap(); // fire channel 0

    assert!(bus.dma_ref().check_done(), "channel must report done once its transfer completes");
    assert!(!bus.dma_ref().is_active(), "dma_active must be cleared once the (synchronous) transfer finishes");
}

#[test]
fn immediate_dma_transfer_advances_ppu_and_apu_by_bytes_times_eight_master_cycles() {
    // `execute_dma_channel` used to advance no CPU/PPU/APU cycle count
    // at all -- as if the whole multi-byte transfer took zero time.
    // Real hardware costs 8 MASTER cycles/byte plus a small per-channel
    // setup cost (~8 master cycles); at the exact 4-master-cycles-per-
    // dot rate that's 2 dots per byte. (An intermediate version charged
    // 8 *CPU cycles* per byte through the fixed 2-dots/cycle path -- 8x
    // the real dot cost.)
    let mut bus = SystemBus::new();
    bus.write_u8(0x7E0010, 0x5A).unwrap();

    bus.write_u8(0x004300, 0x08).unwrap(); // fixed source, mode 0
    bus.write_u8(0x004301, 0x18).unwrap();
    bus.write_u8(0x004302, 0x10).unwrap();
    bus.write_u8(0x004303, 0x00).unwrap();
    bus.write_u8(0x004304, 0x7E).unwrap();
    bus.write_u8(0x004305, 0x04).unwrap(); // DAS = 4 bytes
    bus.write_u8(0x004306, 0x00).unwrap();

    let h_before = bus.ppu_ref().h_counter();
    bus.write_u8(0x00420B, 0x01).unwrap(); // fire channel 0: 4 bytes

    // 18 master cycles of $420B CPU<->DMA clock sync (snes9x
    // `Timings.DMACPUSync`) + 8 per-channel setup + 4 bytes * 8.
    let expected_master = 18u32 + 8 + 4 * 8;
    let expected_dots = expected_master / 4; // 4 master cycles per dot
    let h_after = bus.ppu_ref().h_counter();
    assert_eq!(
        (h_after as u32 + 341 - h_before as u32) % 341,
        expected_dots % 341,
        "a 4-byte DMA transfer must advance the PPU by (18 + 8 + 4*8)/4 = {} dots, not zero",
        expected_dots
    );
}

#[test]
fn ppu_to_cpu_readback_dma_transfers_real_data_and_does_not_report_a_stale_done_flag() {
    // The PPU->CPU (B->A) readback direction (DMAPx bit 7 set) used to
    // `return` immediately -- before ever touching `dma_active`/`done`
    // for that transfer. Firing a readback DMA right after an unrelated
    // forward transfer on a DIFFERENT channel had already left
    // `check_done()` == true, so the readback appeared to "complete"
    // even though nothing about it had actually run yet, and it never
    // moved a single real byte.
    let mut bus = SystemBus::new();

    // Seed OAM with a known byte and set OAMADD so $2138 (OAMDATAREAD)
    // reads it back -- a real B-bus register, exercised the same way a
    // CPU-driven read would.
    bus.write_u8(0x002102, 0x00).unwrap();
    bus.write_u8(0x002103, 0x00).unwrap();
    bus.write_u8(0x002104, 0x77).unwrap(); // OAM byte 0 = 0x77 (low-table words
    bus.write_u8(0x002104, 0x00).unwrap(); // commit on the odd-byte write)
    bus.write_u8(0x002102, 0x00).unwrap(); // reset OAMADD/toggle for the DMA read
    bus.write_u8(0x002103, 0x00).unwrap();

    // Channel 0: an unrelated forward (CPU->PPU) transfer that
    // completes normally, leaving done=true, active=false -- the
    // "earlier, unrelated transfer" whose stale flag must not leak.
    bus.write_u8(0x7E0010, 0x5A).unwrap();
    bus.write_u8(0x004300, 0x08).unwrap();
    bus.write_u8(0x004301, 0x18).unwrap();
    bus.write_u8(0x004302, 0x10).unwrap();
    bus.write_u8(0x004303, 0x00).unwrap();
    bus.write_u8(0x004304, 0x7E).unwrap();
    bus.write_u8(0x004305, 0x01).unwrap();
    bus.write_u8(0x004306, 0x00).unwrap();
    bus.write_u8(0x00420B, 0x01).unwrap(); // fire channel 0
    assert!(bus.dma_ref().channel(0).unwrap().done, "channel 0's own transfer really did complete");

    // Channel 1: PPU->CPU readback, reading $2138 (OAMDATAREAD) into
    // WRAM at $7E:0020. Before this fires, channel 1 has never run --
    // its done flag must start false and only become true because THIS
    // transfer actually executed, not because of channel 0's leftover
    // state (`check_done()` checks across all channels).
    assert!(!bus.dma_ref().channel(1).unwrap().done, "channel 1 has not run yet");

    bus.write_u8(0x004310, 0x80).unwrap(); // DMAP1: bit7 = PPU->CPU readback, mode 0
    bus.write_u8(0x004311, 0x38).unwrap(); // BBAD1 = $38 (-> $2138 OAMDATAREAD)
    bus.write_u8(0x004312, 0x20).unwrap(); // A1T1 low = $0020 (destination in WRAM)
    bus.write_u8(0x004313, 0x00).unwrap();
    bus.write_u8(0x004314, 0x7E).unwrap(); // A1B1 = bank $7E
    bus.write_u8(0x004315, 0x01).unwrap(); // DAS1 = 1 byte
    bus.write_u8(0x004316, 0x00).unwrap();

    bus.write_u8(0x00420B, 0x02).unwrap(); // fire channel 1

    // The byte must have actually moved from the B-bus register to WRAM.
    assert_eq!(bus.read_u8(0x7E0020).unwrap(), 0x77, "readback DMA must copy the real OAM byte via $2138, not skip the transfer");

    // And channel 1's own flags must reflect ITS transfer, not leak
    // channel 0's stale state.
    assert!(bus.dma_ref().channel(1).unwrap().done, "channel 1 must report done because its own transfer ran");
    assert!(!bus.dma_ref().is_active(), "dma_active must be cleared once the readback transfer finishes");
}

#[test]
fn dma_is_enabled_reflects_420c_hdmaen_mask_not_leftover_das_value() {
    // `is_enabled()` used to infer "enabled" from `das > 0`, but HDMA's
    // indirect-addressing mode repurposes DAS as the live indirect
    // address -- legitimately nonzero on a channel that was never
    // enabled via $420C at all.
    let mut bus = SystemBus::new();
    assert!(!bus.dma_ref().is_enabled());

    bus.write_u8(0x004305, 0x34).unwrap(); // DASxL -- nonzero, but channel not armed
    bus.write_u8(0x004306, 0x12).unwrap();
    assert!(!bus.dma_ref().is_enabled(), "a nonzero DAS alone must not report the channel enabled");

    bus.write_u8(0x00420C, 0x01).unwrap();
    assert!(bus.dma_ref().is_enabled(), "$420C HDMAEN must be the real source of truth");

    bus.write_u8(0x00420C, 0x00).unwrap();
    assert!(!bus.dma_ref().is_enabled());
}

#[test]
fn hdma_pending_reflects_armed_channels_and_clears_once_table_is_exhausted() {
    let mut bus = SystemBus::new();

    // A "no-repeat" entry (bit7 set) so the table pointer advances past
    // its 1 data byte after the line runs, landing exactly on the
    // end-of-table marker below (a "repeat" entry's pointer would stay
    // parked on the data byte itself, which is an existing, unrelated
    // quirk in `hdma_load_next_entry`'s repeat handling -- not one of
    // the bugs this test targets).
    bus.write_u8(0x7E4000, 0x81).unwrap(); // 1 line, no-repeat
    bus.write_u8(0x7E4001, 0x99).unwrap();
    bus.write_u8(0x7E4002, 0x00).unwrap(); // end of table

    bus.write_u8(0x002116, 0x00).unwrap();
    bus.write_u8(0x002117, 0x00).unwrap();

    bus.write_u8(0x004300, 0x00).unwrap();
    bus.write_u8(0x004301, 0x18).unwrap();
    bus.write_u8(0x004302, 0x00).unwrap();
    bus.write_u8(0x004303, 0x40).unwrap();
    bus.write_u8(0x004304, 0x7E).unwrap();

    assert!(!bus.dma_ref().hdma_pending(), "no channel armed yet");

    tick_dots(&mut bus, 230 * 341); // into vblank, then arm channel 0
    bus.write_u8(0x00420C, 0x01).unwrap();

    tick_dots(&mut bus, 31 * 341 + 100); // scanline 261 dot 100: hdma_init has loaded the first entry
    assert!(bus.dma_ref().hdma_pending(), "channel 0 is armed and its table isn't exhausted yet");

    tick_dots(&mut bus, 300); // cross the line's HDMA slot: transfers the 1 line, reload hits end-of-table
    assert!(!bus.dma_ref().hdma_pending(), "table exhausted -- pending must clear");
}

#[test]
fn hdma_raw_0x80_line_counter_is_a_128_line_non_repeat_entry() {
    // Raw NLTRx byte 0x80 (repeat bit set, 7-bit count 0): because
    // real hardware decrements the WHOLE raw byte each scanline, 0x80
    // behaves as a plain 128-line non-repeat entry -- one transfer on
    // the first line (0x80 -> 0x7F clears the repeat bit), then 127
    // wait lines before the next entry loads. An earlier version
    // special-cased this to "reload the next entry immediately", which
    // is not what the hardware does.
    let mut bus = SystemBus::new();

    bus.write_u8(0x7E5000, 0x80).unwrap(); // raw 0x80: 128 lines, transfer once
    bus.write_u8(0x7E5001, 0xAA).unwrap(); // that first line's data byte
    bus.write_u8(0x7E5002, 0x01).unwrap(); // next real entry: 1 line
    bus.write_u8(0x7E5003, 0xBB).unwrap();
    bus.write_u8(0x7E5004, 0x00).unwrap(); // end of table

    bus.write_u8(0x002116, 0x00).unwrap();
    bus.write_u8(0x002117, 0x00).unwrap();

    bus.write_u8(0x004300, 0x00).unwrap();
    bus.write_u8(0x004301, 0x18).unwrap();
    bus.write_u8(0x004302, 0x00).unwrap();
    bus.write_u8(0x004303, 0x50).unwrap();
    bus.write_u8(0x004304, 0x7E).unwrap();

    tick_dots(&mut bus, 230 * 341); // into vblank, then arm
    bus.write_u8(0x00420C, 0x01).unwrap();

    tick_dots(&mut bus, 32 * 341); // init + the pre-visible line's HDMA slot: the entry's single transfer

    assert_eq!(bus.ppu_ref().vram_ref().read(0x0000), 0xAA);
    assert_eq!(
        bus.dma_ref().channel(0).unwrap().hdma_line_counter,
        0x7F,
        "whole-byte decrement: 0x80 -> 0x7F (127 wait lines remain)"
    );

    // Walk the remaining 127 wait lines: no B-bus writes may happen.
    tick_dots(&mut bus, 127 * 341);
    assert_eq!(
        bus.ppu_ref().vram_ref().read(0x0002),
        0x00,
        "wait lines of the 128-line entry must not transfer anything"
    );

    // Line 128: the next real entry (1 line, 0xBB) loads and transfers.
    tick_dots(&mut bus, 341);
    assert_eq!(
        bus.ppu_ref().vram_ref().read(0x0002),
        0xBB,
        "after the 128 lines, the next real table entry must load and transfer its data"
    );
}

#[test]
fn hdma_indirect_mode_second_address_byte_stays_within_table_bank_on_wraparound() {
    // Table entry straddles the bank boundary: line-count byte at
    // $7E:FFFE, indirect address low byte at $7E:FFFF (so `next_offset`
    // wraps to 0xFFFF), and the high byte must be read from $7E:0000
    // (wrapping within the SAME bank) rather than carrying into $7F:0000.
    let mut bus = SystemBus::new();

    bus.write_u8(0x7EFFFE, 0x01).unwrap(); // line-count = 1
    bus.write_u8(0x7EFFFF, 0x34).unwrap(); // indirect address low byte
    bus.write_u8(0x7E0000, 0x12).unwrap(); // indirect address high byte (correct, same-bank wrap)
    bus.write_u8(0x7F0000, 0xFF).unwrap(); // decoy: what the old bug would have read instead

    bus.write_u8(0x004300, 0x40).unwrap(); // DMAPx bit6 = indirect addressing
    bus.write_u8(0x004301, 0x18).unwrap();
    bus.write_u8(0x004302, 0xFE).unwrap(); // A1T low = $FFFE
    bus.write_u8(0x004303, 0xFF).unwrap(); // A1T high
    bus.write_u8(0x004304, 0x7E).unwrap(); // A1B = bank $7E

    tick_dots(&mut bus, 230 * 341); // into vblank, then arm channel 0
    bus.write_u8(0x00420C, 0x01).unwrap();

    // Land mid-way through the pre-visible line (hardware V=0):
    // hdma_init has loaded the first entry, but the line's HDMA slot
    // (~dot 276) hasn't transferred yet -- a transfer would advance
    // the indirect pointer past the value under test.
    tick_dots(&mut bus, 31 * 341 + 100);

    let indirect_addr = bus.dma_ref().channel(0).unwrap().das;
    assert_eq!(indirect_addr, 0x1234, "high byte must wrap within bank $7E, not carry into $7F");
}

#[test]
fn hdma_on_the_same_channel_kills_an_in_flight_dma() {
    // snes9x dma.cpp: "If HDMA triggers in the middle of DMA transfer
    // and it uses the same channel, it kills the DMA transfer
    // immediately. $43x2 and $43x5 stop updating." A different
    // channel's DMA must be unaffected.
    let build = |dma_channel: u8| -> SystemBus {
        let mut bus = SystemBus::new();
        // HDMA channel 0: an effectively endless repeat entry so the
        // channel stays active on every line of the frame.
        bus.write_u8(0x7E6000, 0xFF).unwrap(); // repeat, 127 lines
        bus.write_u8(0x004300, 0x00).unwrap(); // direct, mode 0
        bus.write_u8(0x004301, 0x22).unwrap(); // -> $2122 (CGRAM), away from the DMA's target
        bus.write_u8(0x004302, 0x00).unwrap();
        bus.write_u8(0x004303, 0x60).unwrap();
        bus.write_u8(0x004304, 0x7E).unwrap();
        tick_dots(&mut bus, 230 * 341); // into vblank, then arm
        bus.write_u8(0x00420C, 0x01).unwrap();
        tick_dots(&mut bus, 32 * 341); // init + pre-visible line; now at scanline 0, dot 0

        // General DMA on `dma_channel`: 1000 bytes into $2118. At 8
        // master cycles/byte it reaches scanline 0's HDMA slot
        // (~dot 276) after ~135 bytes.
        let base = 0x004300 + (dma_channel as u32) * 0x10;
        bus.write_u8(base, 0x08).unwrap(); // fixed source, mode 0
        bus.write_u8(base + 1, 0x18).unwrap();
        bus.write_u8(base + 2, 0x10).unwrap();
        bus.write_u8(base + 3, 0x00).unwrap();
        bus.write_u8(base + 4, 0x7E).unwrap();
        bus.write_u8(base + 5, 0xE8).unwrap(); // DAS = 1000
        bus.write_u8(base + 6, 0x03).unwrap();
        bus.write_u8(0x00420B, 1 << dma_channel).unwrap();
        bus
    };

    // Same channel: the mid-transfer HDMA kills the DMA -- DAS holds
    // the untransferred remainder instead of draining to 0.
    let mut bus = build(0);
    let das = (bus.read_u8(0x004305).unwrap() as u16)
        | ((bus.read_u8(0x004306).unwrap() as u16) << 8);
    assert!(
        das > 0 && das < 1000,
        "the same-channel HDMA must abort the DMA mid-transfer (DAS = {das}, expected 0 < DAS < 1000)"
    );

    // Different channel: the DMA runs to completion.
    let mut bus = build(1);
    let das1 = (bus.read_u8(0x004315).unwrap() as u16)
        | ((bus.read_u8(0x004316).unwrap() as u16) << 8);
    assert_eq!(das1, 0, "an HDMA on channel 0 must not kill a DMA on channel 1");
}

#[test]
fn dma_to_wram_data_port_fills_wram() {
    // The classic bulk-clear idiom: DMA channel with B-bus address $80
    // (-> $2180 WMDATA) streams bytes into WRAM through the port.
    let mut bus = SystemBus::new();
    // Source bytes in WRAM bank $7E at $2000.
    bus.write_u8(0x7E2000, 0xAA).unwrap();
    bus.write_u8(0x7E2001, 0xBB).unwrap();
    // Destination: WMADD = $7E:6000.
    bus.write_u8(0x002181, 0x00).unwrap();
    bus.write_u8(0x002182, 0x60).unwrap();
    bus.write_u8(0x002183, 0x00).unwrap();
    // Channel 0: mode 0 (single byte to BBAD), A-bus $7E:2000, 2 bytes.
    bus.write_u8(0x004300, 0x00).unwrap();
    bus.write_u8(0x004301, 0x80).unwrap(); // BBAD = $80 -> $2180
    bus.write_u8(0x004302, 0x00).unwrap();
    bus.write_u8(0x004303, 0x20).unwrap();
    bus.write_u8(0x004304, 0x7E).unwrap();
    bus.write_u8(0x004305, 0x02).unwrap();
    bus.write_u8(0x004306, 0x00).unwrap();
    bus.write_u8(0x00420B, 0x01).unwrap(); // trigger channel 0
    assert_eq!(bus.read_u8(0x7E6000).unwrap(), 0xAA);
    assert_eq!(bus.read_u8(0x7E6001).unwrap(), 0xBB);
}

#[test]
fn hdma_palette_rewrite_renders_per_line_despite_vblank_restore() {
    // Prince of Persia 2's sky gradient: an HDMA channel with B-bus
    // $21 in mode 3 ($2121,$2121,$2122,$2122) rewrites CGRAM color 0
    // during the visible frame, and the game's NMI handler restores
    // the palette in vblank. Rendering from a single shared CGRAM
    // painted every row with the vblank value -- a flat sky and a
    // wrong status-bar backdrop. The per-scanline CGRAM snapshots
    // must preserve each row's mid-frame color.
    let mut bus = SystemBus::new();
    bus.write_u8(0x002100, 0x0F).unwrap(); // full brightness, screen on
    // Default regs leave every layer off (TM=0): backdrop-only frame.

    // Direct-mode HDMA table in WRAM at $7E:2000. Non-repeat entries
    // transfer once then hold: lines 0-95 red, lines 96-191 blue.
    let table: [u8; 11] = [
        0x60, 0x00, 0x00, 0x1F, 0x00, // 96 lines: CGADD=0, color=$001F
        0x60, 0x00, 0x00, 0x00, 0x7C, // 96 lines: CGADD=0, color=$7C00
        0x00, // end of table
    ];
    for (i, &b) in table.iter().enumerate() {
        bus.write_u8(0x7E2000 + i as u32, b).unwrap();
    }
    bus.write_u8(0x004320, 0x03).unwrap(); // DMAP2: direct table, mode 3
    bus.write_u8(0x004321, 0x21).unwrap(); // BBAD2: $2121
    bus.write_u8(0x004322, 0x00).unwrap(); // A1T2L
    bus.write_u8(0x004323, 0x20).unwrap(); // A1T2H
    bus.write_u8(0x004324, 0x7E).unwrap(); // A1B2
    bus.write_u8(0x00420C, 0x04).unwrap(); // HDMAEN: channel 2

    // Tick two full frames (so HDMA init has run at a frame boundary
    // and a complete visible frame was captured with the gradient),
    // then park inside vblank.
    let master_per_line = 341 * 4;
    let lines_per_frame = bus.ppu_ref().scanlines_per_frame() as u32;
    for _ in 0..(2 * lines_per_frame + 230) {
        bus.tick_master(master_per_line);
    }
    assert!(bus.ppu_ref().in_vblank(), "test setup: should be parked in vblank");

    // The game's vblank palette restore. This lands in the live CGRAM
    // only -- the visible rows were already captured.
    bus.write_u8(0x002121, 0x00).unwrap();
    bus.write_u8(0x002122, 0x4C).unwrap();
    bus.write_u8(0x002122, 0x3D).unwrap();

    let fb = bus.render_frame();
    let w = crate::renderer::SCREEN_WIDTH;
    let top = (10 * w + 100) * 4;
    assert_eq!(
        (fb[top], fb[top + 1], fb[top + 2]),
        (255, 0, 0),
        "rows covered by the first HDMA entry must keep its color, not the vblank restore"
    );
    let bottom = (150 * w + 100) * 4;
    assert_eq!(
        (fb[bottom], fb[bottom + 1], fb[bottom + 2]),
        (0, 0, 255),
        "rows covered by the second HDMA entry must keep its color, not the vblank restore"
    );
}
