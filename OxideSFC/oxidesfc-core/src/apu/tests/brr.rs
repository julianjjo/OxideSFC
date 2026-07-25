//! BRR block decoding: header fields, nibble order, filter arithmetic.

use crate::apu::brr::BrrDecoder;

#[test]
fn test_brr_decoder() {
    let mut decoder = BrrDecoder::new();
    let header = 0x00; // No filter, 9 bytes (standard BRR)
    let data = [0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77];
    let mut output = [0i16; 16];

    decoder.decode(header, &data, &mut output);

    // The decoder should produce some output (may be all zeros for this test data)
    // Just verify it doesn't panic
    assert_eq!(output.len(), 16);
}

#[test]
fn brr_decode_does_not_overflow_across_many_blocks_with_extreme_history() {
    // Regression guard for a real `attempt to multiply with overflow`
    // panic hit after ~3M real-ROM steps once notes actually started
    // triggering (see the $F2/$F3 fix above -- this code path was
    // simply never exercised with real driver-triggered data before
    // that). Filter 3 has the largest multiply coefficients, and
    // maximal-magnitude nibbles/history are the adversarial case for
    // the old `i16` intermediate arithmetic.
    let mut decoder = BrrDecoder::new();
    let header = 0x0F; // shift=0, filter=3 (header & 0x0C == 0x0C -> filter 3 branch)
    let data = [0xFFu8; 8]; // every nibble = 0xF (the maximal-magnitude negative nibble)
    let mut output = [0i16; 16];

    for _ in 0..200 {
        decoder.decode(header, &data, &mut output);
    }
    // Must not panic (the real regression), and must stay within
    // valid i16 PCM range.
    for &s in &output {
        assert!((-32768..=32767).contains(&(s as i32)));
    }
}

#[test]
fn brr_decode_extracts_shift_and_filter_from_the_correct_header_bits() {
    // Regression guard: the header parser used to read shift from
    // bits 0-3 and filter from a derived value, when real hardware's
    // layout (byte = `ssssffle`) puts shift in bits 4-7 and filter in
    // bits 2-3. A shift-12 filter-0 header applied to nibble value 1
    // must produce `((1 << 12) >> 1) * 2 = 4096` on the first sample
    // (no filter, no history yet, and the decoder's final step always
    // doubles the clamped intermediate value -- see `decode`'s doc
    // comment) -- if shift/filter were still being read from the
    // wrong bits, this would instead be treated as shift=0xF (invalid,
    // clamped to 0) filter=(0x0F>>2)&3=3.
    let mut decoder = BrrDecoder::new();
    let header = 0xC0; // shift=12 (0xC), filter=0
    let mut data = [0u8; 8];
    data[0] = 0x10; // first nibble = HIGH nibble of byte 0 (see `decode`)
    let mut output = [0i16; 16];

    decoder.decode(header, &data, &mut output);

    assert_eq!(output[0], 4096, "shift=12 on nibble value 1 with no filter must give ((1<<12)>>1)*2 = 4096");
}

#[test]
fn brr_decode_plays_the_high_nibble_of_each_byte_before_its_low_nibble() {
    // Regression guard for a scrambled BRR nibble order: hardware plays
    // H0,L0,H1,L1,...,H7,L7 (fullsnes; bsnes `decode_brr`), but this
    // used to emit all eight LOW nibbles first and then all eight HIGH
    // nibbles. That time-scrambled every 16-sample block of every
    // sample in every game AND ran the prediction filters' history in
    // the wrong order, so decoded amplitudes were wrong too -- heard as
    // constant graininess on all sampled instruments.
    //
    // Nibbles 0..15 laid out in hardware order must decode to a
    // strictly increasing ramp; the old order produced
    // 1,3,5,...,15,0,2,...,14, which is NOT monotonic.
    let mut decoder = BrrDecoder::new();
    let header = 0xB0; // shift=11, filter=0 (no history feedback)
    let mut data = [0u8; 8];
    for (i, byte) in data.iter_mut().enumerate() {
        *byte = ((i as u8 * 2) << 4) | (i as u8 * 2 + 1);
    }
    let mut output = [0i16; 16];

    decoder.decode(header, &data, &mut output);

    // Nibbles 0..7 are positive and 8..15 are negative (4-bit signed),
    // so the ramp rises across the first half and, after wrapping to
    // the most negative value, rises again across the second half.
    for half in [&output[0..8], &output[8..16]] {
        for w in half.windows(2) {
            assert!(
                w[1] > w[0],
                "nibbles must decode in hardware order (high nibble first), \
                 giving a monotonic ramp; got {:?}",
                output
            );
        }
    }
    assert!(
        output[8] < output[7],
        "nibble 8 is the most negative 4-bit value, so it must drop below nibble 7"
    );
}

