//! The audited numeric casts. Signal code turns sample indices into seconds
//! and frequencies, and frequencies back into bin indices. Every such cast
//! goes through one function here, so each lossy cast has one reviewed home
//! and one test.

/// The largest integer that `f64` holds exactly: `2^53`.
const EXACT_LIMIT: f64 = 9_007_199_254_740_992.0;

/// Converts a count or an index to `f64`. Exact below `2^53`, far beyond any
/// sample count the tool meets.
#[expect(
    clippy::cast_precision_loss,
    reason = "counts stay below 2^53, where f64 holds every integer exactly"
)]
pub(crate) fn usize_to_f64(value: usize) -> f64 {
    value as f64
}

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
        assert_eq!(usize_to_f64(1 << 53), EXACT_LIMIT);
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
