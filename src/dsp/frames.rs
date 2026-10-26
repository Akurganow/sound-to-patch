//! Active frames and the robust statistics over them.

use super::StftParams;
use super::stft::padded_sample;
use crate::cast;
use crate::units::Decibels;

/// The gate of the project's convention: a frame is active when its RMS
/// lies within 40 dB of the loudest frame.
pub const ACTIVE_GATE_DB: f64 = 40.0;

/// Returns the RMS of each centred frame of an STFT layout, over the raw
/// samples (no window) with the layout's padding. Frame `m` lines up with
/// STFT frame `m`.
#[must_use]
pub fn frame_rms(samples: &[f64], params: StftParams) -> Vec<f64> {
    let len = cast::usize_to_f64(params.frame_len());
    (0..params.frame_count(samples.len()))
        .map(|frame| {
            let start = params.frame_start(frame);
            let energy: f64 = (0..params.frame_len())
                .map(|offset| {
                    let index = start.saturating_add(isize::try_from(offset).unwrap_or(isize::MAX));
                    padded_sample(samples, index, params.padding()).powi(2)
                })
                .sum();
            (energy / len).sqrt()
        })
        .collect()
}

/// The frames that carry signal: frame RMS within a gate of the loudest
/// frame. Digital silence has no active frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveFrames {
    active: Vec<bool>,
}

impl ActiveFrames {
    /// Marks each frame whose RMS lies within `gate` decibels of the largest
    /// frame RMS. A frame of zero RMS is never active.
    #[must_use]
    pub fn from_rms(rms: &[f64], gate: Decibels) -> Self {
        let peak = rms.iter().copied().fold(0.0, f64::max);
        let threshold = peak
            * Decibels::clamped(-gate.get().abs())
                .to_amplitude_gain()
                .unwrap_or(0.0);
        Self {
            active: rms
                .iter()
                .map(|&value| value > 0.0 && value >= threshold)
                .collect(),
        }
    }

    /// The project's convention: frame RMS of an STFT layout, gated at
    /// [`ACTIVE_GATE_DB`].
    #[must_use]
    pub fn of(samples: &[f64], params: StftParams) -> Self {
        Self::from_rms(
            &frame_rms(samples, params),
            Decibels::clamped(ACTIVE_GATE_DB),
        )
    }

    /// Returns whether frame `m` is active; a frame past the end is not.
    #[must_use]
    pub fn is_active(&self, frame: usize) -> bool {
        self.active.get(frame).copied().unwrap_or(false)
    }

    /// Returns the indices of the active frames in order.
    pub fn indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.active
            .iter()
            .enumerate()
            .filter_map(|(frame, &active)| active.then_some(frame))
    }

    /// Returns the number of active frames.
    #[must_use]
    pub fn count(&self) -> usize {
        self.active.iter().filter(|&&active| active).count()
    }

    /// Returns the number of frames, active or not.
    #[must_use]
    pub fn len(&self) -> usize {
        self.active.len()
    }

    /// Returns whether no frame was given.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }
}

/// The median and the interquartile range of a set of values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Summary {
    median: f64,
    iqr: f64,
}

impl Summary {
    /// Summarises finite values. Quantiles use linear interpolation between
    /// the sorted values, at position `q (n - 1)`, the convention of the
    /// common numeric libraries. Returns `None` for no value or a NaN or
    /// infinite one.
    #[must_use]
    pub fn of(values: &[f64]) -> Option<Self> {
        if values.is_empty() || values.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let mut sorted = values.to_vec();
        sorted.sort_by(f64::total_cmp);
        Some(Self {
            median: quantile(&sorted, 0.5),
            iqr: quantile(&sorted, 0.75) - quantile(&sorted, 0.25),
        })
    }

    /// Returns the median.
    #[must_use]
    pub fn median(self) -> f64 {
        self.median
    }

    /// Returns the interquartile range, the 75th minus the 25th percentile.
    #[must_use]
    pub fn iqr(self) -> f64 {
        self.iqr
    }
}

/// Linear-interpolation quantile of sorted, non-empty values.
fn quantile(sorted: &[f64], q: f64) -> f64 {
    let position = q * cast::usize_to_f64(sorted.len() - 1);
    let lower = cast::floor_to_usize(position).unwrap_or(0);
    let upper = (lower + 1).min(sorted.len() - 1);
    let fraction = position - cast::usize_to_f64(lower);
    sorted[lower] + fraction * (sorted[upper] - sorted[lower])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantiles_interpolate_linearly() {
        let summary = Summary::of(&[4.0, 1.0, 3.0, 2.0]).unwrap();
        // Sorted 1 2 3 4: median at 1.5, quartiles at 0.75 and 2.25.
        assert_eq!(summary.median(), 2.5);
        assert_eq!(summary.iqr(), 3.25 - 1.75);
        let single = Summary::of(&[7.0]).unwrap();
        assert_eq!((single.median(), single.iqr()), (7.0, 0.0));
        assert!(Summary::of(&[]).is_none());
        assert!(Summary::of(&[1.0, f64::NAN]).is_none());
    }

    #[test]
    fn frames_within_the_gate_are_active() {
        let rms = [1.0, 0.011, 0.009, 0.0, 0.5];
        let active = ActiveFrames::from_rms(&rms, Decibels::new(40.0).unwrap());
        assert_eq!(active.indices().collect::<Vec<_>>(), vec![0, 1, 4]);
        assert_eq!(active.count(), 3);
        assert_eq!(active.len(), 5);
        assert!(!active.is_active(9));
    }

    #[test]
    fn silence_has_no_active_frame() {
        let params = StftParams::new(64).unwrap();
        let active = ActiveFrames::of(&[0.0; 1000], params);
        assert_eq!(active.count(), 0);
        assert_eq!(active.len(), params.frame_count(1000));
    }

    #[test]
    fn frame_rms_of_a_constant_counts_the_padding() {
        let params = StftParams::new(8).unwrap();
        let rms = frame_rms(&[1.0; 16], params);
        // Frame 0 covers 4 padded zeros and 4 ones.
        assert!((rms[0] - 0.5_f64.sqrt()).abs() < 1e-15);
        assert!((rms[4] - 1.0).abs() < 1e-15);
    }
}
