//! Analysis windows, in their periodic form.

use std::f64::consts::TAU;

use crate::cast;

/// An analysis window. Every window is periodic: its `N` samples are the
/// first `N` of a symmetric window of length `N + 1`, as spectral analysis
/// expects. The symmetric form shifts every bin slightly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Window {
    /// Periodic Hann, `w[n] = 0.5 - 0.5 cos(2 pi n / N)`. Main lobe 4 bins
    /// null to null, -3 dB width 1.44 bins. The default for analysis.
    Hann,
    /// Periodic four-term Blackman-Harris, `0.35875 - 0.48829 cos(2 pi n /
    /// N) + 0.14128 cos(4 pi n / N) - 0.01168 cos(6 pi n / N)`. Lower side
    /// lobes than Hann, a wider main lobe.
    BlackmanHarris,
    /// All ones. Only as a baseline: its frequency estimates are an order of
    /// magnitude worse than Hann's.
    Rectangular,
}

impl Window {
    /// Returns the `N` window samples.
    #[must_use]
    pub fn samples(self, len: usize) -> Vec<f64> {
        let n = cast::usize_to_f64(len);
        (0..len)
            .map(|index| {
                let phase = TAU * cast::usize_to_f64(index) / n;
                match self {
                    Self::Hann => 0.5 - 0.5 * phase.cos(),
                    Self::BlackmanHarris => {
                        0.358_75 - 0.488_29 * phase.cos() + 0.141_28 * (2.0 * phase).cos()
                            - 0.011_68 * (3.0 * phase).cos()
                    }
                    Self::Rectangular => 1.0,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hann_starts_at_zero_and_peaks_at_the_centre() {
        let hann = Window::Hann.samples(8);
        assert_eq!(hann[0], 0.0);
        assert!((hann[4] - 1.0).abs() < 1e-15);
        // Periodic: the sample after the last would be the first again.
        assert!((hann[1] - hann[7]).abs() < 1e-15);
    }

    #[test]
    fn blackman_harris_coefficients_sum_to_one_at_the_centre() {
        let window = Window::BlackmanHarris.samples(16);
        assert!((window[8] - 1.0).abs() < 1e-12, "{}", window[8]);
        assert!((window[0] - 6.0e-5).abs() < 1e-12, "{}", window[0]);
    }

    #[test]
    fn rectangular_is_all_ones_and_empty_stays_empty() {
        assert_eq!(Window::Rectangular.samples(3), vec![1.0; 3]);
        assert_eq!(Window::Hann.samples(0), Vec::<f64>::new());
    }
}
