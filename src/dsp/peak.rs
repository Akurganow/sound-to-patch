//! Spectral peak picking with parabolic interpolation on the log magnitude.

use super::StftParams;
use crate::audio::SampleRate;
use crate::cast;
use crate::units::{Decibels, Hz};

/// A spectral peak with its interpolated position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Peak {
    bin: usize,
    offset: f64,
    log_magnitude: f64,
    neighbour_ratio: Option<Decibels>,
}

impl Peak {
    /// Returns the bin of the local maximum.
    #[must_use]
    pub fn bin(self) -> usize {
        self.bin
    }

    /// Returns the interpolated position in bins, `k + delta`, with `delta`
    /// in -0.5 to 0.5.
    #[must_use]
    pub fn position(self) -> f64 {
        cast::usize_to_f64(self.bin) + self.offset
    }

    /// Returns the interpolated frequency, `(k + delta) fs / fft_len`.
    #[must_use]
    pub fn frequency(self, params: StftParams, rate: SampleRate) -> Hz {
        Hz::clamped(self.position() * params.bin_width(rate).get())
    }

    /// Returns the interpolated magnitude at the peak, the exponential of
    /// the parabola's top on `ln |X|`.
    #[must_use]
    pub fn magnitude(self) -> f64 {
        self.log_magnitude.exp()
    }

    /// Returns how far the peak stands above its strongest neighbouring
    /// peak within the search span: `20 log10(|X_peak| / |X_neighbour|)`.
    /// `None` when no other local maximum lies in the span.
    #[must_use]
    pub fn neighbour_ratio(self) -> Option<Decibels> {
        self.neighbour_ratio
    }
}

/// Interpolates the local maximum at bin `k` of a magnitude spectrum.
///
/// With `a, b, c = ln|X[k-1]|, ln|X[k]|, ln|X[k+1]|` the parabola's top
/// sits at `delta = 0.5 (a - c) / (a - 2b + c)`. Returns `None` when `k`
/// has no neighbour on both sides, is not a strict local maximum, or a
/// magnitude in the three bins is zero. `span` bins on each side are
/// searched for a neighbouring peak.
#[must_use]
pub fn interpolate(magnitudes: &[f64], bin: usize, span: usize) -> Option<Peak> {
    let left = magnitudes.get(bin.checked_sub(1)?)?;
    let centre = magnitudes.get(bin)?;
    let right = magnitudes.get(bin + 1)?;
    if !(centre > left && centre >= right) || *left <= 0.0 || *right <= 0.0 {
        return None;
    }
    let (a, b, c) = (left.ln(), centre.ln(), right.ln());
    let curvature = a - 2.0 * b + c;
    let offset = if curvature < 0.0 {
        (0.5 * (a - c) / curvature).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    let log_magnitude = b - 0.25 * (a - c) * offset;
    let lo = bin.saturating_sub(span);
    let hi = (bin + span).min(magnitudes.len().saturating_sub(1));
    let neighbour = (lo..=hi)
        .filter(|&k| k != bin && is_local_max(magnitudes, k))
        .map(|k| magnitudes[k])
        .fold(None, |best: Option<f64>, m| {
            Some(best.map_or(m, |b| b.max(m)))
        });
    let neighbour_ratio = neighbour
        .filter(|&m| m > 0.0)
        .and_then(|m| Decibels::from_amplitude_gain(centre / m).ok());
    Some(Peak {
        bin,
        offset,
        log_magnitude,
        neighbour_ratio,
    })
}

fn is_local_max(magnitudes: &[f64], bin: usize) -> bool {
    match (
        bin.checked_sub(1).and_then(|k| magnitudes.get(k)),
        magnitudes.get(bin),
        magnitudes.get(bin + 1),
    ) {
        (Some(left), Some(centre), Some(right)) => centre > left && centre >= right,
        _ => false,
    }
}

/// Returns the bins of the strict local maxima of a magnitude spectrum,
/// skipping the first and the last bin.
#[must_use]
pub fn local_maxima(magnitudes: &[f64]) -> Vec<usize> {
    (1..magnitudes.len().saturating_sub(1))
        .filter(|&bin| is_local_max(magnitudes, bin))
        .collect()
}

/// Returns the interpolated peak with the largest magnitude between bins
/// `lo` and `hi` inclusive, or `None` when the range holds no local
/// maximum.
#[must_use]
pub fn strongest_peak(magnitudes: &[f64], lo: usize, hi: usize, span: usize) -> Option<Peak> {
    let hi = hi.min(magnitudes.len().saturating_sub(2));
    (lo.max(1)..=hi)
        .filter(|&bin| is_local_max(magnitudes, bin))
        .max_by(|&a, &b| magnitudes[a].total_cmp(&magnitudes[b]))
        .and_then(|bin| interpolate(magnitudes, bin, span))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_symmetric_peak_has_no_offset() {
        let peak = interpolate(&[1.0, 4.0, 1.0], 1, 1).unwrap();
        assert_eq!(peak.position(), 1.0);
        assert!((peak.magnitude() - 4.0).abs() < 1e-12);
        assert_eq!(peak.neighbour_ratio(), None);
    }

    #[test]
    fn a_gaussian_peak_is_located_exactly() {
        // ln of a Gaussian is a parabola, so the fit is exact.
        let centre = 10.3;
        let magnitudes: Vec<f64> = (0..20)
            .map(|k| (-(f64::from(k) - centre).powi(2) / 8.0).exp())
            .collect();
        let peak = interpolate(&magnitudes, 10, 2).unwrap();
        assert!(
            (peak.position() - centre).abs() < 1e-12,
            "{}",
            peak.position()
        );
        assert!((peak.magnitude() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn non_maxima_edges_and_zeros_give_no_peak() {
        assert!(interpolate(&[1.0, 2.0, 3.0], 1, 1).is_none());
        assert!(interpolate(&[3.0, 2.0], 0, 1).is_none());
        assert!(interpolate(&[0.0, 2.0, 1.0], 1, 1).is_none());
    }

    #[test]
    fn neighbour_ratio_compares_with_the_next_peak_in_the_span() {
        let magnitudes = [0.1, 1.0, 0.1, 0.05, 0.1, 0.01];
        let peak = interpolate(&magnitudes, 1, 4).unwrap();
        assert!((peak.neighbour_ratio().unwrap().get() - 20.0).abs() < 1e-9);
        assert_eq!(local_maxima(&magnitudes), vec![1, 4]);
        assert_eq!(strongest_peak(&magnitudes, 2, 5, 4).unwrap().bin(), 4);
    }
}
