//! The audited numeric casts. Every `as` between number types in the
//! library goes through one function here, with the reason it is sound and
//! a test of its edge cases.

/// An integer count as `f64`. Exact up to 2^53, far above any count of
/// samples, keys or slots this crate handles.
#[expect(
    clippy::cast_precision_loss,
    reason = "counts stay far below 2^53, where f64 is exact"
)]
pub(crate) fn usize_to_f64(n: usize) -> f64 {
    n as f64
}

/// The top 53 bits of a random word as a value in `[0, 1)`, every value a
/// multiple of 2^-53.
#[expect(
    clippy::cast_precision_loss,
    reason = "after the shift the value has at most 53 bits, so it is exact"
)]
pub(crate) fn unit_interval(bits: u64) -> f64 {
    (bits >> 11) as f64 / (1_u64 << 53) as f64
}

/// The nearest `f32`, for file formats that store 32-bit floats.
#[expect(
    clippy::cast_possible_truncation,
    reason = "rounding to the nearest f32 is the intent"
)]
pub(crate) fn f64_to_f32(x: f64) -> f32 {
    x as f32
}

/// A sample in -1..=1 as 16-bit PCM scaled by 32767, rounded to the
/// nearest step. Values beyond ±1 clamp, and NaN gives 0.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the value is rounded and clamped to the i16 range first"
)]
pub(crate) fn pcm16(x: f64) -> i16 {
    (x.clamp(-1.0, 1.0) * 32767.0).round() as i16
}

/// The largest integer that `f64` holds exactly: `2^53`.
const EXACT_LIMIT: f64 = 9_007_199_254_740_992.0;

/// Converts a non-negative `f64` to an index, rounding toward zero.
/// Returns `None` for NaN, a negative value or a value of `2^53` or more.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the guard admits only finite values in 0..2^53, which fit usize on 64-bit targets"
)]
pub(crate) fn floor_to_usize(value: f64) -> Option<usize> {
    (0.0..EXACT_LIMIT)
        .contains(&value)
        .then(|| value.floor() as usize)
}

/// Converts a non-negative `f64` to the nearest index, ties away from zero.
/// Returns `None` where [`floor_to_usize`] does.
pub(crate) fn round_to_usize(value: f64) -> Option<usize> {
    floor_to_usize(value.round())
}

/// Converts a non-negative `f64` to the next index up. Returns `None` where
/// [`floor_to_usize`] does.
pub(crate) fn ceil_to_usize(value: f64) -> Option<usize> {
    floor_to_usize(value.ceil())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_convert_exactly() {
        assert_eq!(usize_to_f64(0), 0.0);
        assert_eq!(usize_to_f64(44_100), 44_100.0);
        assert_eq!(usize_to_f64(1 << 53), 9_007_199_254_740_992.0);
    }

    #[test]
    fn unit_interval_spans_zero_to_below_one() {
        assert_eq!(unit_interval(0), 0.0);
        assert_eq!(unit_interval(u64::MAX), 1.0 - 2f64.powi(-53));
        assert_eq!(unit_interval(1 << 63), 0.5);
    }

    #[test]
    fn f32_rounds_to_nearest() {
        assert_eq!(f64_to_f32(0.5), 0.5_f32);
        assert_eq!(f64_to_f32(0.1), 0.1_f32);
        assert!(f64_to_f32(f64::MAX).is_infinite());
    }

    #[test]
    fn pcm16_rounds_and_clamps() {
        assert_eq!(pcm16(0.0), 0);
        assert_eq!(pcm16(1.0), 32767);
        assert_eq!(pcm16(-1.0), -32767);
        assert_eq!(pcm16(2.0), 32767);
        assert_eq!(pcm16(-2.0), -32767);
        assert_eq!(pcm16(0.9), 29490);
        assert_eq!(pcm16(f64::NAN), 0);
    }

    #[test]
    fn floor_rounds_toward_zero_and_rejects_bad_values() {
        assert_eq!(floor_to_usize(0.0), Some(0));
        assert_eq!(floor_to_usize(2.999), Some(2));
        assert_eq!(floor_to_usize(-0.0), Some(0));
        assert_eq!(floor_to_usize(-0.5), None);
        assert_eq!(floor_to_usize(f64::NAN), None);
        assert_eq!(floor_to_usize(f64::INFINITY), None);
        assert_eq!(floor_to_usize(EXACT_LIMIT), None);
        assert_eq!(floor_to_usize(EXACT_LIMIT - 1.0), Some((1 << 53) - 1));
    }

    #[test]
    fn round_and_ceil_follow_their_rule() {
        assert_eq!(round_to_usize(2.5), Some(3));
        assert_eq!(round_to_usize(2.4), Some(2));
        assert_eq!(ceil_to_usize(2.01), Some(3));
        assert_eq!(ceil_to_usize(2.0), Some(2));
        assert_eq!(ceil_to_usize(-0.5), Some(0));
        assert_eq!(round_to_usize(-0.6), None);
    }
}
