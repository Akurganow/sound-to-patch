//! The casts of the test code. Rounding goes through the library's audited
//! helper; only the PCM encoder differs from the library's, on purpose.

use sound_to_patch::cast;

/// Rounds `x` times 32768 to the nearest 16-bit value, clamped to the range,
/// the inverse of the decoder's scale.
///
/// The library's [`cast::pcm16`] scales by 32767, so -1 and 1 map to
/// -32767 and 32767. A test builds WAV bytes for the decoder, which divides
/// by 32768, so this copy scales by 32768 to read back the sample it was
/// given, and -1 reaches -32768.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the value is rounded and clamped to the i16 range first"
)]
pub(crate) fn pcm16(x: f64) -> i16 {
    (x * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16
}

/// Rounds a count, such as seconds times a rate, to `usize`. A value above
/// -0.5 rounds to 0.
///
/// # Panics
///
/// Panics where [`cast::round_to_usize`] returns `None`: for NaN, an
/// infinity, a value of -0.5 or less, or one of `2^53` or more.
pub(crate) fn round_to_usize(x: f64) -> usize {
    cast::round_to_usize(x).unwrap_or_else(|| panic!("{x} is not a count"))
}

#[test]
fn round_to_usize_rounds_half_away_from_zero() {
    assert_eq!(round_to_usize(2.5), 3);
    assert_eq!(round_to_usize(32_000.0), 32_000);
}

#[test]
#[should_panic(expected = "-1 is not a count")]
fn round_to_usize_rejects_a_negative_count() {
    round_to_usize(-1.0);
}

#[test]
fn pcm16_rounds_and_clamps() {
    assert_eq!(pcm16(1.0), 32_767);
    assert_eq!(pcm16(-2.0), -32_768);
    assert_eq!(pcm16(0.5), 16_384);
    assert_eq!(pcm16(-1.0), -32_768);
}
