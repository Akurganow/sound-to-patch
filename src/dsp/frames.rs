//! Active frames and the robust statistics over them.

use super::StftParams;
use super::stft::padded_sample;
use crate::cast;
use crate::units::Decibels;

/// The gate of the project's convention: a frame is active when its RMS
/// lies within 40 dB of the loudest frame.
pub const ACTIVE_GATE_DB: Decibels = Decibels::from_const(40.0);

/// The fewest active frames a statistic over frames needs; with fewer it
/// gives no estimate.
///
/// The first and last two frames of a clip reach into the zero padding, so
/// a median over few frames follows the edges. On 48 kHz noise, sawtooth
/// and sine clips with 4096-point frames, the worst error of the centroid,
/// spread, rolloff and flatness medians against a 10 s run was 4 % at 10
/// frames, 3 % at 12, 2 % at 16 and up to 47 times the value at 8 or fewer.
pub const MIN_ACTIVE_FRAMES: usize = 16;

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
        Self::from_rms(&frame_rms(samples, params), ACTIVE_GATE_DB)
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
            median: quantile(&sorted, 0.5)?,
            iqr: quantile(&sorted, 0.75)? - quantile(&sorted, 0.25)?,
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

impl Summary {
    /// Carries the median and the IQR into a unit type.
    #[must_use]
    pub fn to_stats<T>(self, unit: impl Fn(f64) -> T) -> Stats<T> {
        Stats {
            median: unit(self.median),
            iqr: unit(self.iqr),
        }
    }
}

/// The median and the interquartile range of a value over active frames, in
/// its unit type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats<T> {
    median: T,
    iqr: T,
}

impl<T: Copy> Stats<T> {
    /// Returns the median.
    #[must_use]
    pub fn median(&self) -> T {
        self.median
    }

    /// Returns the interquartile range.
    #[must_use]
    pub fn iqr(&self) -> T {
        self.iqr
    }
}

/// The linear quantile of sorted values, the default of `numpy.quantile`.
/// Returns `None` for no values.
#[must_use]
pub fn quantile(sorted: &[f64], q: f64) -> Option<f64> {
    let last = sorted.len().checked_sub(1)?;
    let position = q * cast::usize_to_f64(last);
    let below = cast::floor_to_usize(position)?.min(last);
    let above = (below + 1).min(last);
    let fraction = position - cast::usize_to_f64(below);
    Some(sorted[below] + (sorted[above] - sorted[below]) * fraction)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_gate_constant_passes_the_unit_check() {
        assert_eq!(ACTIVE_GATE_DB, Decibels::new(40.0).unwrap());
    }

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
