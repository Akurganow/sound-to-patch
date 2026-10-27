//! Note matching by the `mir_eval` transcription rules.
//!
//! A reference note and an estimate match when the onsets lie within
//! 50 ms, the pitches within 50 cents and, when offsets count, the offsets
//! within the larger of 50 ms and 20 % of the reference duration. Times
//! are compared after rounding to 4 decimals, so exactly 50 ms is a hit.
//! The score uses a maximum bipartite matching, not a greedy one, so a
//! crowded run of repeated notes scores as `mir_eval` scores it.

/// One note: onset and offset in seconds, and a MIDI pitch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EvalNote {
    pub(crate) onset: f64,
    pub(crate) offset: f64,
    pub(crate) pitch: f64,
}

/// Precision, recall and F1 of one comparison.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Score {
    pub(crate) matched: usize,
    pub(crate) reference: usize,
    pub(crate) estimated: usize,
}

impl Score {
    /// Adds the counts of another comparison, for a score over many signals.
    pub(crate) fn add(self, other: Self) -> Self {
        Self {
            matched: self.matched + other.matched,
            reference: self.reference + other.reference,
            estimated: self.estimated + other.estimated,
        }
    }

    pub(crate) fn precision(self) -> f64 {
        ratio(self.matched, self.estimated)
    }

    pub(crate) fn recall(self) -> f64 {
        ratio(self.matched, self.reference)
    }

    /// `mir_eval` scores 0 when either list is empty.
    pub(crate) fn f1(self) -> f64 {
        let (p, r) = (self.precision(), self.recall());
        if p + r == 0.0 {
            0.0
        } else {
            2.0 * p * r / (p + r)
        }
    }
}

fn ratio(a: usize, b: usize) -> f64 {
    if b == 0 { 0.0 } else { count(a) / count(b) }
}

/// A count as `f64`; counts here stay far below 2^53.
#[expect(
    clippy::cast_precision_loss,
    reason = "note counts stay far below 2^53"
)]
pub(crate) fn count(n: usize) -> f64 {
    n as f64
}

const ONSET_TOLERANCE: f64 = 0.05;
const PITCH_TOLERANCE_CENTS: f64 = 50.0;
const OFFSET_RATIO: f64 = 0.2;
const OFFSET_MIN_TOLERANCE: f64 = 0.05;

/// `np.around(x, 4)`: round half to even at 4 decimals.
fn round4(x: f64) -> f64 {
    (x * 1e4).round_ties_even() / 1e4
}

/// Whether the reference and the estimate match, with the estimate's
/// pitch moved by `shift` semitones first.
fn hit(reference: &EvalNote, estimate: &EvalNote, offsets: bool, shift: f64) -> bool {
    let onset = round4((reference.onset - estimate.onset).abs()) <= ONSET_TOLERANCE;
    let pitch = (100.0 * (reference.pitch - estimate.pitch - shift)).abs() <= PITCH_TOLERANCE_CENTS;
    let offset = !offsets || {
        let tolerance =
            (OFFSET_RATIO * (reference.offset - reference.onset)).max(OFFSET_MIN_TOLERANCE);
        round4((reference.offset - estimate.offset).abs()) <= tolerance
    };
    onset && pitch && offset
}

/// The size of a maximum matching between references and estimates
/// under `hit`, by augmenting paths.
fn max_matching(
    reference: &[EvalNote],
    estimated: &[EvalNote],
    offsets: bool,
    shift: f64,
) -> usize {
    let edges: Vec<Vec<usize>> = reference
        .iter()
        .map(|r| {
            (0..estimated.len())
                .filter(|&j| hit(r, &estimated[j], offsets, shift))
                .collect()
        })
        .collect();
    let mut owner: Vec<Option<usize>> = vec![None; estimated.len()];
    let mut size = 0;
    for i in 0..reference.len() {
        let mut seen = vec![false; estimated.len()];
        if augment(i, &edges, &mut owner, &mut seen) {
            size += 1;
        }
    }
    size
}

fn augment(i: usize, edges: &[Vec<usize>], owner: &mut [Option<usize>], seen: &mut [bool]) -> bool {
    for &j in &edges[i] {
        if !seen[j] {
            seen[j] = true;
            if owner[j].is_none_or(|k| augment(k, edges, owner, seen)) {
                owner[j] = Some(i);
                return true;
            }
        }
    }
    false
}

fn score(reference: &[EvalNote], estimated: &[EvalNote], matched: usize) -> Score {
    Score {
        matched,
        reference: reference.len(),
        estimated: estimated.len(),
    }
}

/// Onsets and pitches only.
pub(crate) fn onset_score(reference: &[EvalNote], estimated: &[EvalNote]) -> Score {
    score(
        reference,
        estimated,
        max_matching(reference, estimated, false, 0.0),
    )
}

/// Onsets, pitches and offsets.
pub(crate) fn offset_score(reference: &[EvalNote], estimated: &[EvalNote]) -> Score {
    score(
        reference,
        estimated,
        max_matching(reference, estimated, true, 0.0),
    )
}

/// Onsets and pitch classes: every pitch folds into one octave.
pub(crate) fn pitch_class_score(reference: &[EvalNote], estimated: &[EvalNote]) -> Score {
    let fold = |notes: &[EvalNote]| -> Vec<EvalNote> {
        notes
            .iter()
            .map(|n| EvalNote {
                pitch: 60.0 + n.pitch.rem_euclid(12.0),
                ..*n
            })
            .collect()
    };
    let (reference, estimated) = (fold(reference), fold(estimated));
    score(
        &reference,
        &estimated,
        max_matching(&reference, &estimated, false, 0.0),
    )
}

/// Estimates an octave above and below their reference, by onset.
pub(crate) fn octave_errors(reference: &[EvalNote], estimated: &[EvalNote]) -> (usize, usize) {
    (
        max_matching(reference, estimated, false, -12.0),
        max_matching(reference, estimated, false, 12.0),
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::Value;

    use super::*;

    fn notes(value: &Value) -> Vec<EvalNote> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|n| EvalNote {
                onset: n[0].as_f64().unwrap(),
                offset: n[1].as_f64().unwrap(),
                pitch: n[2].as_f64().unwrap(),
            })
            .collect()
    }

    fn check(name: &str, what: &str, got: Score, expected: &Value) {
        for (field, value) in [
            ("precision", got.precision()),
            ("recall", got.recall()),
            ("f1", got.f1()),
        ] {
            let want = expected[field].as_f64().unwrap();
            assert!(
                (value - want).abs() <= 1e-12,
                "{name} {what} {field}: {value} vs {want}"
            );
        }
    }

    #[test]
    fn every_hand_built_case_scores_as_mir_eval() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/golden/notes-match.json");
        let golden: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let cases = golden["recipe"]["cases"].as_object().unwrap();
        assert_eq!(cases.len(), 12);
        for (name, case) in cases {
            let (reference, estimated) = (notes(&case[0]), notes(&case[1]));
            let expected = &golden["values"][name];
            check(
                name,
                "onset",
                onset_score(&reference, &estimated),
                &expected["onset"],
            );
            check(
                name,
                "offset",
                offset_score(&reference, &estimated),
                &expected["offset"],
            );
            check(
                name,
                "pitch class",
                pitch_class_score(&reference, &estimated),
                &expected["pitch_class"],
            );
            let (up, down) = octave_errors(&reference, &estimated);
            assert_eq!(
                u64::try_from(up).unwrap(),
                expected["octave_up"].as_u64().unwrap(),
                "{name}"
            );
            assert_eq!(
                u64::try_from(down).unwrap(),
                expected["octave_down"].as_u64().unwrap(),
                "{name}"
            );
        }
    }
}
