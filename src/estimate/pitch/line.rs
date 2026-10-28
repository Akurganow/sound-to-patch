//! The sub-band Hilbert line: the instantaneous frequency of the
//! fundamental of a low note, one value per sample.
//!
//! An f0 tracker averages over its frame, 93 ms at 22,050 Hz, so it smears
//! a pitch drop that settles in 20 ms. The line keeps the fundamental
//! alone with a band-pass, takes the frequency of its analytic signal
//! sample by sample and removes the phase spikes with a short median.

use crate::audio::SampleRate;
use crate::cast;
use crate::dsp::{AnalyticSignal, FilterError, band_pass_zero_phase};
use crate::units::{Hz, Seconds};

/// The low edge of the band.
pub const LINE_LO: Hz = Hz::from_const(20.0);
/// The high edge of the band.
pub const LINE_HI: Hz = Hz::from_const(200.0);
/// The length of the median filter, about 1.4 ms: 31 samples at 22,050 Hz,
/// 63 at 44,100 Hz and 67 at 48,000 Hz.
pub const LINE_MEDIAN: Seconds = Seconds::from_const(0.0014);

/// The convention of the line, as `layers.json` names it.
pub const LINE_CONVENTION: &str = "fourth-order Butterworth high-pass at 20 Hz and low-pass at \
200 Hz, each forward and backward; the frequency of the analytic signal between consecutive \
samples, arg(z[n+1] conj(z[n])) fs / 2 pi; a centred median of round(1.4 ms fs) samples, made \
odd, shortened at the ends";

/// The instantaneous frequency of the 20 to 200 Hz band of a segment, with
/// the envelope of that band.
#[derive(Clone, Debug, PartialEq)]
pub struct SubBandLine {
    start: Seconds,
    rate: SampleRate,
    hz: Vec<f64>,
    envelope: Vec<f64>,
}

impl SubBandLine {
    /// Builds the line of `segment`, whose first sample lies `start` into
    /// the stem.
    ///
    /// The analytic signal is meant for note-sized segments. Each cut end
    /// carries an error that decays with the distance from it, so a caller
    /// keeps a margin of silence or signal around the span it reads.
    ///
    /// # Errors
    ///
    /// Returns a [`FilterError`] when the rate puts 200 Hz at or above the
    /// Nyquist frequency.
    pub fn of(segment: &[f64], rate: SampleRate, start: Seconds) -> Result<Self, FilterError> {
        let band = band_pass_zero_phase(segment, rate, LINE_LO, LINE_HI)?;
        // Padding as long as the segment keeps the circular wrap of the FFT
        // off the segment.
        let analytic = AnalyticSignal::new(&band, rate, band.len());
        let hz = median_filter(&analytic.instantaneous_frequency(), median_len(rate));
        Ok(Self {
            start,
            rate,
            hz,
            envelope: analytic.envelope(),
        })
    }

    /// Returns the frequency between samples `n` and `n + 1`, in hertz. A
    /// value can be negative where the band holds no signal.
    #[must_use]
    pub fn hz(&self) -> &[f64] {
        &self.hz
    }

    /// Returns the rate of the segment.
    #[must_use]
    pub fn sample_rate(&self) -> SampleRate {
        self.rate
    }

    /// Returns the envelope of the band, one value per sample.
    #[must_use]
    pub fn envelope(&self) -> &[f64] {
        &self.envelope
    }

    /// Returns the time of frequency value `n`: halfway between samples `n`
    /// and `n + 1`.
    #[must_use]
    pub fn time_of(&self, n: usize) -> Seconds {
        Seconds::clamped(self.start.get() + (cast::usize_to_f64(n) + 0.5) / self.rate.hz().get())
    }

    /// Returns the time of sample `n`.
    #[must_use]
    pub fn sample_time(&self, n: usize) -> Seconds {
        Seconds::clamped(self.start.get() + cast::usize_to_f64(n) / self.rate.hz().get())
    }

    /// Returns the index of the first sample at or after `time`.
    #[must_use]
    pub fn index_at(&self, time: Seconds) -> usize {
        let offset = (time.get() - self.start.get()) * self.rate.hz().get();
        cast::ceil_to_usize(offset.max(0.0)).unwrap_or(0)
    }
}

/// The median length at `rate`: `round(1.4 ms fs)`, plus one when even.
#[must_use]
pub fn median_len(rate: SampleRate) -> usize {
    let len = cast::round_to_usize(LINE_MEDIAN.get() * rate.hz().get()).unwrap_or(1);
    len.max(1) | 1
}

/// A centred running median of odd length `len`. Near an end the window
/// shrinks to the samples that exist, so the ends are not pulled towards a
/// padding value.
#[must_use]
pub fn median_filter(x: &[f64], len: usize) -> Vec<f64> {
    let half = len / 2;
    let mut window = Vec::with_capacity(len);
    (0..x.len())
        .map(|n| {
            window.clear();
            window.extend_from_slice(&x[n.saturating_sub(half)..(n + half + 1).min(x.len())]);
            window.sort_by(f64::total_cmp);
            let mid = window.len() / 2;
            if window.len() % 2 == 1 {
                window[mid]
            } else {
                f64::midpoint(window[mid - 1], window[mid])
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_median_length_is_odd_and_near_1_4_ms() {
        for (rate, len) in [(22_050, 31), (44_100, 63), (48_000, 67), (8_000, 11)] {
            assert_eq!(median_len(SampleRate::new(rate).unwrap()), len, "{rate} Hz");
        }
    }

    #[test]
    fn the_median_removes_a_spike_and_keeps_the_ends() {
        let x = [1.0, 1.0, 9.0, 1.0, 1.0, 2.0, 3.0];
        assert_eq!(median_filter(&x, 3), [1.0, 1.0, 1.0, 1.0, 1.0, 2.0, 2.5]);
        assert_eq!(median_filter(&[], 31), Vec::<f64>::new());
        assert_eq!(median_filter(&[4.0], 31), [4.0]);
    }

    #[test]
    fn a_steady_low_sine_reads_its_frequency() {
        let rate = SampleRate::new(22_050).unwrap();
        let x: Vec<f64> = (0..22_050)
            .map(|n| (std::f64::consts::TAU * 41.2 * cast::usize_to_f64(n) / 22_050.0).sin())
            .collect();
        let line = SubBandLine::of(&x, rate, Seconds::new(1.0).unwrap()).unwrap();
        assert_eq!(line.hz().len(), x.len() - 1);
        // Away from the cut ends the frequency ripples at twice the tone,
        // by an amount that falls as one over the distance from an end.
        let middle = &line.hz()[5_000..17_000];
        let mean = middle.iter().sum::<f64>() / cast::usize_to_f64(middle.len());
        assert!((mean - 41.2).abs() < 0.01, "{mean}");
        assert!(middle.iter().all(|f| (f - 41.2).abs() < 0.5));
        assert!((line.time_of(0).get() - (1.0 + 0.5 / 22_050.0)).abs() < 1e-12);
        assert_eq!(line.index_at(Seconds::new(1.5).unwrap()), 11_025);
        assert!((line.envelope()[11_025] - 1.0).abs() < 0.01);
    }
}
