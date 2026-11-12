//! Rhythm arithmetic: Euclidean patterns, swing and ticks.
//!
//! A bar has 16 sixteenth slots, numbered 0 to 15. Ticks count 480 per
//! quarter note, as in the MIDI files the tool writes.
//!
//! ```
//! use sound_to_patch::music::rhythm::{euclid, onsets, ticks_for_ms};
//!
//! assert_eq!(onsets(&euclid(3, 8)), [0, 3, 6]);
//! assert_eq!(ticks_for_ms(5.0, 120.0), Some(5));
//! ```

use crate::audio::{self, BeatGrid, Feel, NoteValue};
use crate::cast;
use crate::notes::{TICKS_PER_BEAT, ticks_per_second};
use crate::units::{Bpm, Seconds};

/// The Euclidean pattern of `k` onsets over `n` steps.
///
/// The grouping algorithm: start with `k` groups `1` and `n - k` groups `0`, pair each
/// leading group with one trailing group, keep the unpaired ones as the
/// remainder, and repeat on the pairs and the remainder until at most one
/// remainder group is left. At least one pairing step runs. `k` above `n`
/// is cut to `n`.
#[must_use]
pub fn euclid(k: usize, n: usize) -> Vec<bool> {
    let k = k.min(n);
    if k == 0 || k == n {
        return vec![k == n; n];
    }
    let mut leading: Vec<Vec<bool>> = vec![vec![true]; k];
    let mut trailing: Vec<Vec<bool>> = vec![vec![false]; n - k];
    loop {
        let pairs = leading.len().min(trailing.len());
        let remainder = if leading.len() > pairs {
            leading.split_off(pairs)
        } else {
            trailing.split_off(pairs)
        };
        for (group, tail) in leading.iter_mut().zip(&trailing) {
            group.extend(tail);
        }
        trailing = remainder;
        if trailing.len() <= 1 {
            break;
        }
    }
    leading.into_iter().chain(trailing).flatten().collect()
}

/// The steps that hold an onset.
#[must_use]
pub fn onsets(pattern: &[bool]) -> Vec<usize> {
    pattern
        .iter()
        .enumerate()
        .filter_map(|(step, onset)| onset.then_some(step))
        .collect()
}

/// The pattern with every onset moved `rotation` steps later, wrapping
/// around.
#[must_use]
pub fn rotate(pattern: &[bool], rotation: usize) -> Vec<bool> {
    let mut turned = pattern.to_vec();
    if let Some(steps) = rotation.checked_rem(turned.len()) {
        turned.rotate_right(steps);
    }
    turned
}

/// The delay of the second note of a swung pair, in sixteenths, for a pair
/// percent `pct` (50 is straight) and a pair of sixteenths.
///
/// Returns `None` where the delay is negative or not finite: for a pair
/// percent below 50, an infinity or NaN.
#[must_use]
pub fn swing_delay(pct: f64) -> Option<f64> {
    non_negative((pct / 100.0 - 0.5) * 2.0)
}

/// The long-to-short ratio of a pair percent.
///
/// Returns `None` where the ratio is negative or not finite: for a pair
/// percent below 0, of 100 or more, or NaN.
#[must_use]
pub fn swing_ratio(pct: f64) -> Option<f64> {
    non_negative(pct / (100.0 - pct))
}

/// The pair percent of a long-to-short ratio.
///
/// Returns `None` for a negative ratio, which no pair has, and where the
/// percent is not finite: for an infinity or NaN.
#[must_use]
pub fn swing_pct(ratio: f64) -> Option<f64> {
    if ratio < 0.0 {
        return None;
    }
    // Dividing first keeps a huge ratio finite: `100 * ratio` overflows
    // from about 1.8e306.
    non_negative(ratio / (ratio + 1.0) * 100.0)
}

/// The two notes of a swung pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pair {
    /// A pair of sixteenths, 240 ticks.
    Sixteenths,
    /// A pair of eighths, 480 ticks.
    Eighths,
}

impl Pair {
    /// The length of the pair in ticks.
    fn ticks(self) -> u32 {
        let note = match self {
            Self::Sixteenths => Division::Sixteenth,
            Self::Eighths => Division::Eighth,
        };
        2 * note.ticks()
    }
}

/// The delay of the second note of a swung pair in ticks.
///
/// Returns `None` where the delay is negative or not finite: for a pair
/// percent below 50, an infinity or NaN.
#[must_use]
pub fn swing_ticks(pct: f64, pair: Pair) -> Option<f64> {
    non_negative((pct / 100.0 - 0.5) * f64::from(pair.ticks()))
}

/// The length of one tick in ms, `1000 / ticks per second`, the tick the
/// MIDI writer uses.
///
/// Returns `None` for a tempo outside the 4 to 1000 BPM of [`Bpm`].
#[must_use]
pub fn tick_ms(bpm: f64) -> Option<f64> {
    let bpm = Bpm::new(bpm).ok()?;
    Some(1000.0 / ticks_per_second(bpm))
}

/// The length of a sixteenth in ms: a quarter of the beat period of the
/// stem's beat grid.
///
/// Returns `None` for a tempo outside the 4 to 1000 BPM of [`Bpm`].
#[must_use]
pub fn sixteenth_ms(bpm: f64) -> Option<f64> {
    let grid = BeatGrid::new(Bpm::new(bpm).ok()?, Seconds::from_const(0.0));
    Some(grid.beat_period().get() / 4.0 * 1000.0)
}

/// `value` when it is finite and not negative, with `-0.0` stored as
/// `+0.0`.
fn non_negative(value: f64) -> Option<f64> {
    (value.is_finite() && value >= 0.0).then_some(value + 0.0)
}

/// The whole ticks that cover `ms` at a tempo, rounded up, so an overlap
/// is never shorter than asked.
///
/// Returns `None` for a tempo outside the 4 to 1000 BPM of [`Bpm`], a
/// length that is NaN or negative, or a tick count beyond `u32`.
#[must_use]
pub fn ticks_for_ms(ms: f64, bpm: f64) -> Option<u32> {
    let tick = tick_ms(bpm)?;
    if ms.is_nan() || ms < 0.0 {
        return None;
    }
    // The nanotick margin keeps a length of exactly n ticks at n.
    let ticks = cast::ceil_to_usize((ms / tick - 1e-9).max(0.0))?;
    u32::try_from(ticks).ok()
}

/// A note division and its length in ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Division {
    /// A thirty-second note.
    ThirtySecond,
    /// A sixteenth note.
    Sixteenth,
    /// A sixteenth-note triplet.
    SixteenthTriplet,
    /// An eighth note.
    Eighth,
    /// An eighth-note triplet.
    EighthTriplet,
    /// A dotted eighth note.
    DottedEighth,
    /// A quarter-note triplet.
    QuarterTriplet,
    /// A quarter note.
    Quarter,
    /// A dotted quarter note.
    DottedQuarter,
    /// A bar of 4/4.
    Bar,
}

impl Division {
    /// Every division, shortest first.
    pub const ALL: [Self; 10] = [
        Self::ThirtySecond,
        Self::SixteenthTriplet,
        Self::Sixteenth,
        Self::EighthTriplet,
        Self::Eighth,
        Self::QuarterTriplet,
        Self::DottedEighth,
        Self::Quarter,
        Self::DottedQuarter,
        Self::Bar,
    ];

    /// The division of the stem analysis with the same length. This list
    /// is the dictionary's named subset of [`audio::Division::ALL`], which
    /// holds the lengths.
    fn division(self) -> audio::Division {
        let (value, feel) = match self {
            Self::ThirtySecond => (NoteValue::ThirtySecond, Feel::Straight),
            Self::Sixteenth => (NoteValue::Sixteenth, Feel::Straight),
            Self::SixteenthTriplet => (NoteValue::Sixteenth, Feel::Triplet),
            Self::Eighth => (NoteValue::Eighth, Feel::Straight),
            Self::EighthTriplet => (NoteValue::Eighth, Feel::Triplet),
            Self::DottedEighth => (NoteValue::Eighth, Feel::Dotted),
            Self::QuarterTriplet => (NoteValue::Quarter, Feel::Triplet),
            Self::Quarter => (NoteValue::Quarter, Feel::Straight),
            Self::DottedQuarter => (NoteValue::Quarter, Feel::Dotted),
            Self::Bar => (NoteValue::Whole, Feel::Straight),
        };
        audio::Division { value, feel }
    }

    /// The length in ticks at 480 per quarter: the beats of the same
    /// division in [`audio::Division::ALL`] times 480.
    #[must_use]
    pub fn ticks(self) -> u32 {
        // Every division here lasts a whole number of ticks, and the
        // product is exact for each, as the tests pin; the fallback is
        // never taken.
        let ticks = self.division().beats_per_cycle() * f64::from(TICKS_PER_BEAT);
        cast::round_to_usize(ticks)
            .and_then(|ticks| u32::try_from(ticks).ok())
            .unwrap_or(u32::MAX)
    }

    /// The name for people.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::ThirtySecond => "1/32",
            Self::Sixteenth => "1/16",
            Self::SixteenthTriplet => "1/16T",
            Self::Eighth => "1/8",
            Self::EighthTriplet => "1/8T",
            Self::DottedEighth => "dotted 1/8",
            Self::QuarterTriplet => "1/4T",
            Self::Quarter => "1/4",
            Self::DottedQuarter => "dotted 1/4",
            Self::Bar => "bar",
        }
    }
}

/// The ticks of one slot of a bar of `grid` slots: 12, 16 or 24 slots give
/// 160, 120 or 80 ticks.
///
/// Returns `None` for 0 slots and for a grid whose slots do not last a
/// whole number of ticks, such as 7.
#[must_use]
pub fn slot_ticks(grid: u8) -> Option<u32> {
    let bar = Division::Bar.ticks();
    let grid = u32::from(grid);
    (bar.checked_rem(grid)? == 0).then(|| bar / grid)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The 20 patterns printed in the source paper, as onset steps.
    const PRINTED: [(usize, usize, &[usize]); 20] = [
        (2, 5, &[0, 2]),
        (3, 4, &[0, 2, 3]),
        (3, 5, &[0, 2, 4]),
        (3, 7, &[0, 2, 4]),
        (3, 8, &[0, 3, 6]),
        (4, 7, &[0, 2, 4, 6]),
        (4, 9, &[0, 2, 4, 6]),
        (4, 11, &[0, 3, 6, 9]),
        (5, 6, &[0, 2, 3, 4, 5]),
        (5, 7, &[0, 2, 3, 5, 6]),
        (5, 8, &[0, 2, 3, 5, 6]),
        (5, 9, &[0, 2, 4, 6, 8]),
        (5, 11, &[0, 2, 4, 6, 8]),
        (5, 12, &[0, 3, 5, 8, 10]),
        (5, 13, &[0, 3, 5, 8, 10]),
        (7, 8, &[0, 2, 3, 4, 5, 6, 7]),
        (7, 12, &[0, 2, 3, 5, 7, 8, 10]),
        (5, 16, &[0, 3, 6, 9, 12]),
        (7, 16, &[0, 3, 5, 7, 10, 12, 14]),
        (9, 16, &[0, 2, 3, 5, 7, 9, 10, 12, 14]),
    ];

    /// E(k, 16) for k = 2 to 11, the bar table.
    const BARS: [(usize, &[usize]); 10] = [
        (2, &[0, 8]),
        (3, &[0, 5, 10]),
        (4, &[0, 4, 8, 12]),
        (5, &[0, 3, 6, 9, 12]),
        (6, &[0, 3, 5, 8, 11, 13]),
        (7, &[0, 3, 5, 7, 10, 12, 14]),
        (8, &[0, 2, 4, 6, 8, 10, 12, 14]),
        (9, &[0, 2, 3, 5, 7, 9, 10, 12, 14]),
        (10, &[0, 2, 3, 5, 7, 8, 10, 11, 13, 15]),
        (11, &[0, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15]),
    ];

    fn floor_form(k: usize, n: usize) -> Vec<bool> {
        (0..n).map(|i| (i * k) % n < k).collect()
    }

    #[test]
    fn reproduces_every_printed_pattern() {
        for (k, n, steps) in PRINTED {
            let pattern = euclid(k, n);
            assert_eq!(pattern.len(), n);
            assert_eq!(onsets(&pattern), steps, "E({k},{n})");
        }
    }

    #[test]
    fn reproduces_the_bar_table() {
        for (k, steps) in BARS {
            assert_eq!(onsets(&euclid(k, 16)), steps, "E({k},16)");
        }
        assert_eq!(onsets(&euclid(4, 16)), [0, 4, 8, 12]);
    }

    #[test]
    fn the_two_lists_hold_27_distinct_patterns() {
        let mut all: Vec<(usize, usize)> = PRINTED.iter().map(|(k, n, _)| (*k, *n)).collect();
        all.extend(BARS.iter().map(|(k, _)| (*k, 16)));
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), 27);
    }

    #[test]
    fn the_floor_form_fails_exactly_nine_printed_patterns() {
        let failing: Vec<(usize, usize)> = PRINTED
            .iter()
            .filter(|(k, n, _)| floor_form(*k, *n) != euclid(*k, *n))
            .map(|(k, n, _)| (*k, *n))
            .collect();
        assert_eq!(
            failing,
            [
                (2, 5),
                (3, 7),
                (4, 9),
                (5, 8),
                (5, 11),
                (5, 13),
                (7, 12),
                (5, 16),
                (9, 16)
            ]
        );
    }

    #[test]
    fn rotation_moves_onsets_by_r_modulo_n() {
        for (k, n, _) in PRINTED {
            let pattern = euclid(k, n);
            for r in 0..n {
                let turned: Vec<usize> = onsets(&rotate(&pattern, r));
                let mut expected: Vec<usize> =
                    onsets(&pattern).iter().map(|s| (s + r) % n).collect();
                expected.sort_unstable();
                assert_eq!(turned, expected, "E({k},{n}) by {r}");
            }
        }
        assert_eq!(euclid(0, 4), [false; 4]);
        assert_eq!(euclid(4, 4), [true; 4]);
    }

    #[test]
    fn swing_matches_the_published_table() {
        let rows = [
            (54.0, 1.17, 0.08, 9.6, [13.3, 10.0, 8.6]),
            (58.0, 1.38, 0.16, 19.2, [26.7, 20.0, 17.1]),
            (62.0, 1.63, 0.24, 28.8, [40.0, 30.0, 25.7]),
            (66.7, 2.00, 0.33, 40.0, [55.6, 41.7, 35.7]),
            (75.0, 3.00, 0.50, 60.0, [83.3, 62.5, 53.6]),
        ];
        for (pct, ratio, delay, ticks, ms) in rows {
            assert!((swing_ratio(pct).unwrap() - ratio).abs() < 0.006, "{pct}");
            assert!((swing_delay(pct).unwrap() - delay).abs() < 0.006, "{pct}");
            assert!(
                (swing_ticks(pct, Pair::Sixteenths).unwrap() - ticks).abs() < 0.1,
                "{pct}"
            );
            for (bpm, late) in [90.0, 120.0, 140.0].into_iter().zip(ms) {
                let computed = swing_ticks(pct, Pair::Sixteenths).unwrap() * tick_ms(bpm).unwrap();
                assert!((computed - late).abs() < 0.1, "{pct} at {bpm}: {computed}");
            }
            assert!((swing_pct(swing_ratio(pct).unwrap()).unwrap() - pct).abs() < 1e-9);
        }
        assert_eq!(swing_delay(50.0), Some(0.0));
        assert!((swing_ticks(66.7, Pair::Eighths).unwrap() - 80.16).abs() < 0.01);
        assert_eq!(
            [Pair::Sixteenths.ticks(), Pair::Eighths.ticks()],
            [240, 480]
        );
    }

    #[test]
    fn swing_refuses_a_negative_or_infinite_result() {
        for pct in [49.9, 0.0, -10.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(swing_delay(pct), None, "{pct}");
            assert_eq!(swing_ticks(pct, Pair::Sixteenths), None, "{pct}");
            assert_eq!(swing_ticks(pct, Pair::Eighths), None, "{pct}");
        }
        for pct in [100.0, 150.0, -1.0, f64::NAN, f64::INFINITY] {
            assert_eq!(swing_ratio(pct), None, "{pct}");
        }
        assert_eq!(swing_ratio(0.0), Some(0.0));
        assert_eq!(swing_ratio(75.0), Some(3.0));
        for ratio in [-1.0, -2.0, -0.5, f64::NAN, f64::INFINITY] {
            assert_eq!(swing_pct(ratio), None, "{ratio}");
        }
        assert_eq!(swing_pct(0.0), Some(0.0));
        assert_eq!(swing_pct(3.0), Some(75.0));
        assert_eq!(swing_pct(1e300), Some(100.0));
        assert_eq!(swing_pct(1e307), Some(100.0));
        assert_eq!(swing_pct(f64::MAX), Some(100.0));
        for zero in [swing_ratio(-0.0), swing_pct(-0.0)] {
            assert!(zero.is_some_and(f64::is_sign_positive), "{zero:?}");
        }
    }

    #[test]
    fn tempo_lengths_refuse_a_tempo_outside_the_bpm_range() {
        for bpm in [0.0, -120.0, f64::NAN, f64::INFINITY, 3.9, 1000.5] {
            assert_eq!(tick_ms(bpm), None, "{bpm}");
            assert_eq!(sixteenth_ms(bpm), None, "{bpm}");
        }
        assert_eq!(tick_ms(120.0), Some(1000.0 / 960.0));
        assert_eq!(sixteenth_ms(120.0), Some(125.0));
        for bpm in [4.0, 1000.0] {
            assert!(tick_ms(bpm).is_some(), "{bpm}");
            assert!(sixteenth_ms(bpm).is_some(), "{bpm}");
        }
        assert_eq!(tick_ms(4.0), Some(31.25));
        assert_eq!(sixteenth_ms(1000.0), Some(15.0));
    }

    /// The tick and the sixteenth come from the MIDI writer and the beat
    /// grid. They stay within two ulp of the published formulas
    /// `60000 / (BPM * 480)` and `60000 / BPM / 4`.
    #[test]
    fn tempo_lengths_match_the_published_formulas_to_two_ulp() {
        let ulps = |a: f64, b: f64| a.to_bits().abs_diff(b.to_bits());
        for hundredths in 400_u32..=100_000 {
            let bpm = f64::from(hundredths) / 100.0;
            let tick = 60_000.0 / (bpm * f64::from(TICKS_PER_BEAT));
            assert!(ulps(tick_ms(bpm).unwrap(), tick) <= 2, "{bpm}");
            assert!(
                ulps(sixteenth_ms(bpm).unwrap(), 60_000.0 / bpm / 4.0) <= 2,
                "{bpm}"
            );
        }
    }

    #[test]
    fn ticks_for_ms_refuses_a_bad_tempo_or_length() {
        for bpm in [0.0, -120.0, f64::NAN, f64::INFINITY, 3.9, 1000.5] {
            assert_eq!(ticks_for_ms(5.0, bpm), None, "{bpm}");
        }
        for ms in [f64::NAN, -1.0, f64::INFINITY, 1e12] {
            assert_eq!(ticks_for_ms(ms, 120.0), None, "{ms}");
        }
        assert_eq!(ticks_for_ms(0.0, 120.0), Some(0));
        assert_eq!(ticks_for_ms(-0.0, 120.0), Some(0));
        assert_eq!(ticks_for_ms(1000.0, 120.0), Some(960));
        assert_eq!(ticks_for_ms(1000.0, 4.0), Some(32));
    }

    #[test]
    fn slot_ticks_refuses_a_grid_that_does_not_split_the_bar() {
        assert_eq!(slot_ticks(0), None);
        assert_eq!(slot_ticks(7), None);
        assert_eq!(slot_ticks(1), Some(1920));
        assert_eq!(slot_ticks(32), Some(60));
    }

    #[test]
    fn ticks_and_milliseconds_match_the_published_values() {
        let ticks: Vec<u32> = Division::ALL
            .iter()
            .map(|division| division.ticks())
            .collect();
        assert_eq!(ticks, [60, 80, 120, 160, 240, 320, 360, 480, 720, 1920]);
        for division in Division::ALL {
            let beats = division.division().beats_per_cycle();
            assert_eq!(
                beats * f64::from(TICKS_PER_BEAT),
                f64::from(division.ticks()),
                "{}",
                division.name()
            );
            assert!(audio::Division::ALL.contains(&division.division()));
        }
        for (bpm, ms) in [(120.0, 1.042), (140.0, 0.893), (174.0, 0.718)] {
            assert!((tick_ms(bpm).unwrap() - ms).abs() < 0.001, "{bpm}");
        }
        for (bpm, ms) in [(90.0, 166.7), (120.0, 125.0), (140.0, 107.1), (174.0, 86.2)] {
            assert!((sixteenth_ms(bpm).unwrap() - ms).abs() < 0.05, "{bpm}");
        }
        assert_eq!(ticks_for_ms(5.0, 120.0), Some(5));
        assert_eq!(ticks_for_ms(5.0, 174.0), Some(7));
        assert_eq!(
            [slot_ticks(12), slot_ticks(16), slot_ticks(24)],
            [Some(160), Some(120), Some(80)]
        );
        assert!([160, 120, 80].iter().all(|ticks| ticks % 20 == 0));
    }
}
