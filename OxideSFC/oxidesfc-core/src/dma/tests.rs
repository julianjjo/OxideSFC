use super::*;

/// Test DMA controller creation
#[test]
fn test_dma_new() {
    let dma = Dma::new();
    assert!(!dma.is_active());
    assert!(!dma.hdma_pending());
}

/// Test DMA register read/write
#[test]
fn test_dma_register() {
    let mut dma = Dma::new();
    
    // Write to channel 0 DMAP0 register
    dma.write_register(0x00, 0x87); // HDMA enabled
    assert_eq!(dma.read_register(0x00), 0x87);
    
    // Write to BBAD0
    dma.write_register(0x01, 0x21);
    assert_eq!(dma.read_register(0x01), 0x21);
    
    // Write to A1T0 (source address low)
    dma.write_register(0x02, 0x34);
    assert_eq!(dma.read_register(0x02), 0x34);
    
    // Write to A1T0 (source address high)
    dma.write_register(0x03, 0x12);
    assert_eq!(dma.read_register(0x03), 0x12);
    
    // Write to A1B0 (source bank)
    dma.write_register(0x04, 0x80);
    assert_eq!(dma.read_register(0x04), 0x80);
    
    // Write to DAS0 (size low)
    dma.write_register(0x05, 0x00);
    assert_eq!(dma.read_register(0x05), 0x00);
    
    // Write to DAS0 (size high) - set size to 0x100
    dma.write_register(0x06, 0x01);
    assert_eq!(dma.read_register(0x06), 0x01);
}

/// Test DMA channel struct
#[test]
fn test_dma_channel() {
    let ch = DmaChannel::new();
    
    assert_eq!(ch.dmape, 0);
    assert_eq!(ch.bbad, 0);
    assert_eq!(ch.a1t, 0);
    assert_eq!(ch.a1b, 0);
    assert_eq!(ch.das, 0);
    assert!(!ch.done);
}

/// Test DMA reset
#[test]
fn test_dma_reset() {
    let mut dma = Dma::new();
    
    // Write some values
    dma.write_register(0x00, 0x80);
    dma.write_register(0x01, 0x21);
    dma.write_register(0x05, 0xFF);
    
    // Reset
    dma.reset();
    
    // Verify all values are reset
    assert_eq!(dma.read_register(0x00), 0);
    assert_eq!(dma.read_register(0x01), 0);
    assert_eq!(dma.read_register(0x05), 0);
}

/// Test multiple channels
#[test]
fn test_multiple_channels() {
    let mut dma = Dma::new();
    
    // Write to channel 0
    dma.write_register(0x00, 0x01);
    
    // Write to channel 1 (at address 0x10)
    dma.write_register(0x10, 0x02);
    
    // Write to channel 7 (at address 0x70)
    dma.write_register(0x70, 0x03);
    
    // Verify channels are independent
    assert_eq!(dma.read_register(0x00), 0x01);
    assert_eq!(dma.read_register(0x10), 0x02);
    assert_eq!(dma.read_register(0x70), 0x03);
}

/// Test source address calculation
#[test]
fn test_source_address() {
    let mut dma = Dma::new();
    
    // Set bank to 0x80, address to 0x1234
    dma.write_register(0x04, 0x80); // Bank
    dma.write_register(0x02, 0x34); // Address low
    dma.write_register(0x03, 0x12); // Address high
    
    let ch = &dma.channels[0];
    assert_eq!(ch.source_address(), 0x801234);
}

/// Test DMA done flag
#[test]
fn test_done_flag() {
    let mut dma = Dma::new();
    
    assert!(!dma.check_done());
    
    // Manually set done flag for testing
    dma.channels[0].done = true;
    
    assert!(dma.check_done());
    
    dma.clear_done(0);
    assert!(!dma.check_done());
}

/// Test channel enable/disable. `is_enabled()` used to (wrongly) infer
/// "enabled" from a channel's DAS register being nonzero -- but HDMA's
/// indirect-addressing mode repurposes DAS as the live indirect
/// address, which is legitimately nonzero on channels never enabled via
/// $420C. The real source of truth is the $420C (HDMAEN) mask, mirrored
/// here via `set_enable_mask`.
#[test]
fn test_channel_enable() {
    let mut dma = Dma::new();

    // No channel armed via HDMAEN yet.
    assert!(!dma.is_enabled());

    // A nonzero DAS value alone (e.g. an indirect address, or leftover
    // transfer-size bytes) must NOT be mistaken for "enabled".
    dma.write_register(0x05, 0x10);
    assert!(!dma.is_enabled());

    // Arm channel 0 (bit 0) the way $420C (HDMAEN) does.
    dma.set_enable_mask(0x01);
    assert!(dma.is_enabled());

    // Disable by resetting.
    dma.reset();
    assert!(!dma.is_enabled());
}

/// Regression test for two aliasing bugs: $43xB/$43xC used to share one
/// backing field (`dasl`), and $43xD/$43xE/$43xF used to silently write
/// through to A1TxL/A1TxH/DASBx respectively. On real hardware all five
/// offsets are unused/unmapped and must be fully independent of each
/// other AND of every real register.
#[test]
fn unused_dma_registers_43xb_thru_43xf_are_independent_and_do_not_alias_real_registers() {
    let mut dma = Dma::new();

    // Give the real registers distinctive values first.
    dma.write_register(0x02, 0x11); // A1TxL
    dma.write_register(0x03, 0x22); // A1TxH
    dma.write_register(0x07, 0x33); // DASBx

    // Writing to the unused offsets must not disturb those real
    // registers ($43xD/$43xE/$43xF used to alias A1TxL/A1TxH/DASBx).
    dma.write_register(0x0B, 0xAA);
    dma.write_register(0x0C, 0xBB);
    dma.write_register(0x0D, 0xCC);
    dma.write_register(0x0E, 0xDD);
    dma.write_register(0x0F, 0xEE);

    assert_eq!(dma.read_register(0x02), 0x11, "$43xD write must not alias A1TxL");
    assert_eq!(dma.read_register(0x03), 0x22, "$43xE write must not alias A1TxH");
    assert_eq!(dma.read_register(0x07), 0x33, "$43xF write must not alias DASBx");

    // Each unused offset must independently echo back whatever was
    // last written to it (not hardcoded 0, and not aliased to any of
    // the other unused offsets -- $43xB/$43xC used to share one field).
    assert_eq!(dma.read_register(0x0B), 0xAA);
    assert_eq!(dma.read_register(0x0C), 0xBB, "$43xC must not alias $43xB's storage");
    assert_eq!(dma.read_register(0x0D), 0xCC);
    assert_eq!(dma.read_register(0x0E), 0xDD);
    assert_eq!(dma.read_register(0x0F), 0xEE);
}
