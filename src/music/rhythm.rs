//! Rhythm arithmetic: Euclidean patterns, swing and ticks.
//!
//! A bar has 16 sixteenth slots, numbered 0 to 15. Ticks count 480 per
//! quarter note, as in the MIDI files the tool writes.
//!
//! ```
//! use sound_to_patch::music::rhythm::{euclid, onsets, ticks_for_ms};
//!
//! assert_eq!(onsets(&euclid(3, 8)), [0, 3, 6]);
//! assert_eq!(ticks_for_ms(5.0, 120.0), 5);
//! ```

use crate::notes::TICKS_PER_BEAT;

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
#[must_use]
pub fn swing_delay(pct: f64) -> f64 {
    (pct / 100.0 - 0.5) * 2.0
}

/// The long-to-short ratio of a pair percent.
#[must_use]
pub fn swing_ratio(pct: f64) -> f64 {
    pct / (100.0 - pct)
}

/// The pair percent of a long-to-short ratio.
#[must_use]
pub fn swing_pct(ratio: f64) -> f64 {
    100.0 * ratio / (ratio + 1.0)
}

/// The delay of the second note of a swung pair in ticks. `note` is 16 for
/// a pair of sixteenths (240 ticks) and 8 for a pair of eighths (480).
#[must_use]
pub fn swing_ticks(pct: f64, note: u8) -> f64 {
    let pair = if note == 8 { 2.0 } else { 1.0 } * f64::from(TICKS_PER_BEAT) / 2.0;
    (pct / 100.0 - 0.5) * pair
}

/// The length of one tick in ms.
#[must_use]
pub fn tick_ms(bpm: f64) -> f64 {
    60_000.0 / (bpm * f64::from(TICKS_PER_BEAT))
}

/// The length of a sixteenth in ms.
#[must_use]
pub fn sixteenth_ms(bpm: f64) -> f64 {
    60_000.0 / bpm / 4.0
}

/// The whole ticks that cover `ms` at a tempo, rounded up, so an overlap
/// is never shorter than asked.
#[must_use]
pub fn ticks_for_ms(ms: f64, bpm: f64) -> u32 {
    let ticks = (ms / tick_ms(bpm) - 1e-9).ceil().max(0.0);
    // At most a few thousand ticks for any musical length.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "non-negative and far below u32::MAX"
    )]
    let ticks = ticks as u32;
    ticks
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

    /// The length in ticks at 480 per quarter.
    #[must_use]
    pub fn ticks(self) -> u32 {
        let quarter = u32::from(TICKS_PER_BEAT);
        match self {
            Self::ThirtySecond => quarter / 8,
            Self::Sixteenth => quarter / 4,
            Self::SixteenthTriplet => quarter / 6,
            Self::Eighth => quarter / 2,
            Self::EighthTriplet => quarter / 3,
            Self::DottedEighth => quarter * 3 / 4,
            Self::QuarterTriplet => quarter * 2 / 3,
            Self::Quarter => quarter,
            Self::DottedQuarter => quarter * 3 / 2,
            Self::Bar => quarter * 4,
        }
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
#[must_use]
pub fn slot_ticks(grid: u8) -> u32 {
    Division::Bar.ticks() / u32::from(grid.max(1))
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
            assert!((swing_ratio(pct) - ratio).abs() < 0.006, "{pct}");
            assert!((swing_delay(pct) - delay).abs() < 0.006, "{pct}");
            assert!((swing_ticks(pct, 16) - ticks).abs() < 0.1, "{pct}");
            for (bpm, late) in [90.0, 120.0, 140.0].into_iter().zip(ms) {
                let computed = swing_ticks(pct, 16) * tick_ms(bpm);
                assert!((computed - late).abs() < 0.1, "{pct} at {bpm}: {computed}");
            }
            assert!((swing_pct(swing_ratio(pct)) - pct).abs() < 1e-9);
        }
        assert!(swing_delay(50.0).abs() < 1e-12);
        assert!((swing_ticks(66.7, 8) - 80.16).abs() < 0.01);
    }

    #[test]
    fn ticks_and_milliseconds_match_the_published_values() {
        let ticks: Vec<u32> = Division::ALL
            .iter()
            .map(|division| division.ticks())
            .collect();
        assert_eq!(ticks, [60, 80, 120, 160, 240, 320, 360, 480, 720, 1920]);
        for (bpm, ms) in [(120.0, 1.042), (140.0, 0.893), (174.0, 0.718)] {
            assert!((tick_ms(bpm) - ms).abs() < 0.001, "{bpm}");
        }
        for (bpm, ms) in [(90.0, 166.7), (120.0, 125.0), (140.0, 107.1), (174.0, 86.2)] {
            assert!((sixteenth_ms(bpm) - ms).abs() < 0.05, "{bpm}");
        }
        assert_eq!(ticks_for_ms(5.0, 120.0), 5);
        assert_eq!(ticks_for_ms(5.0, 174.0), 7);
        assert_eq!(
            [slot_ticks(12), slot_ticks(16), slot_ticks(24)],
            [160, 120, 80]
        );
        assert!([160, 120, 80].iter().all(|ticks| ticks % 20 == 0));
    }
}
