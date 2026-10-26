//! The one cast of the test code: a float sample to a 16-bit PCM value.

/// Rounds `x` times 32768 to the nearest 16-bit value, clamped to the range,
/// the inverse of the decoder's scale.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the value is rounded and clamped to the i16 range first"
)]
pub(crate) fn pcm16(x: f64) -> i16 {
    (x * 32_768.0).round().clamp(-32_768.0, 32_767.0) as i16
}

/// Rounds a non-negative count, such as seconds times a rate, to `usize`.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "test durations are small, finite and non-negative"
)]
pub(crate) fn round_to_usize(x: f64) -> usize {
    assert!(x.is_finite() && x >= 0.0, "{x}");
    x.round() as usize
}

#[test]
fn round_to_usize_rounds_half_away_from_zero() {
    assert_eq!(round_to_usize(2.5), 3);
    assert_eq!(round_to_usize(32_000.0), 32_000);
}

#[test]
fn pcm16_rounds_and_clamps() {
    assert_eq!(pcm16(1.0), 32_767);
    assert_eq!(pcm16(-2.0), -32_768);
    assert_eq!(pcm16(0.5), 16_384);
    assert_eq!(pcm16(-1.0), -32_768);
}
