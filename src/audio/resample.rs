//! Resampling of a mono signal for a consumer with a fixed input rate.

use std::error::Error;
use std::fmt;

use rubato::audioadapter_buffers::direct::SequentialSlice;
use rubato::{Fft, FixedSync, Resampler};

use super::SampleRate;

/// Frames per call inside the resampler. Only speed depends on it.
const CHUNK: usize = 1024;

/// Resamples a mono signal from one rate to another in `f64`.
///
/// The synchronous FFT resampler of rubato does the work: its band-limited
/// conversion keeps the passband and removes everything above the new
/// Nyquist frequency. The output has `ceil(len * to / from)` samples, with
/// the resampler's whole-sample delay trimmed, so sample `n` of the output
/// sits at the time of sample `n * from / to` of the input. Equal rates
/// return a copy.
///
/// rubato leaves a fractional delay that this trim cannot remove: for some
/// rate pairs, among them 16, 32, 48, 96 and 192 kHz to 22050 Hz and 48 to
/// 8 kHz, the output lags by half an output sample, 23 us at 22050 Hz.
/// Pairs such as 44.1 to 22.05 kHz, 48 to 44.1 kHz and 22.05 to 48 kHz
/// have no lag. Half a sample is far below any time this program reports.
///
/// Resampling to a lower rate removes the band above the new Nyquist
/// frequency. Never feed such output to a spectral descriptor.
///
/// # Errors
///
/// Returns a [`ResampleError`] when rubato rejects the rates or fails.
pub fn resample(
    samples: &[f64],
    from: SampleRate,
    to: SampleRate,
) -> Result<Vec<f64>, ResampleError> {
    if from == to || samples.is_empty() {
        return Ok(samples.to_vec());
    }
    let fail = |message: String| ResampleError { from, to, message };
    let rate = |rate: SampleRate| usize::try_from(rate.get()).map_err(|e| fail(e.to_string()));
    let mut resampler = Fft::<f64>::new(rate(from)?, rate(to)?, CHUNK, 1, FixedSync::Input)
        .map_err(|e| fail(e.to_string()))?;
    let input = SequentialSlice::new(samples, 1, samples.len()).map_err(|e| fail(e.to_string()))?;
    let output = resampler
        .process_all(&input, samples.len(), None)
        .map_err(|e| fail(e.to_string()))?;
    let mut output = output.take_data();
    // rubato can return one sample more when upsampling.
    let len = (u64::try_from(samples.len()).map_err(|e| fail(e.to_string()))?
        * u64::from(to.get()))
    .div_ceil(u64::from(from.get()));
    output.truncate(usize::try_from(len).map_err(|e| fail(e.to_string()))?);
    Ok(output)
}

/// A resampling that failed: both rates and rubato's message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResampleError {
    from: SampleRate,
    to: SampleRate,
    message: String,
}

impl fmt::Display for ResampleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cannot resample from {} to {}: {}",
            self.from, self.to, self.message
        )
    }
}

impl Error for ResampleError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn rate(hz: u32) -> SampleRate {
        SampleRate::new(hz).unwrap()
    }

    #[test]
    fn output_length_follows_the_ratio() {
        let input = vec![0.0; 48_000];
        let output = resample(&input, rate(48_000), rate(22_050)).unwrap();
        assert_eq!(output.len(), 22_050);
        let output = resample(&input[..1000], rate(48_000), rate(22_050)).unwrap();
        // ceil(1000 * 22050 / 48000) = ceil(459.375)
        assert_eq!(output.len(), 460);
    }

    #[test]
    fn upsampling_keeps_the_ceiling_length() {
        let input = vec![0.0; 44_100];
        let output = resample(&input, rate(22_050), rate(48_000)).unwrap();
        assert_eq!(output.len(), 96_000);
    }

    /// The lag of the output in output samples, from the phase of a 100 Hz
    /// sine over the middle half.
    fn lag(from: u32, to: u32) -> f64 {
        let tone = |n: usize, fs: u32| {
            std::f64::consts::TAU * 100.0 * f64::from(u32::try_from(n).unwrap()) / f64::from(fs)
        };
        let x: Vec<f64> = (0..2 * usize::try_from(from).unwrap())
            .map(|n| tone(n, from).sin())
            .collect();
        let y = resample(&x, rate(from), rate(to)).unwrap();
        let (mut s, mut c) = (0.0, 0.0);
        for (n, value) in y.iter().enumerate().take(y.len() * 3 / 4).skip(y.len() / 4) {
            s += value * tone(n, to).sin();
            c += value * tone(n, to).cos();
        }
        -c.atan2(s) / std::f64::consts::TAU / 100.0 * f64::from(to)
    }

    #[test]
    fn some_rate_pairs_lag_by_half_an_output_sample() {
        for from in [16_000, 32_000, 48_000, 96_000, 192_000] {
            assert!((lag(from, 22_050) - 0.5).abs() < 1e-3, "{from}");
        }
        for (from, to) in [(44_100, 22_050), (48_000, 44_100), (22_050, 48_000)] {
            assert!(lag(from, to).abs() < 1e-3, "{from} {to}");
        }
    }

    #[test]
    fn equal_rates_and_empty_input_return_a_copy() {
        let input = vec![0.25, -0.5];
        assert_eq!(resample(&input, rate(48_000), rate(48_000)).unwrap(), input);
        assert_eq!(
            resample(&[], rate(48_000), rate(22_050)).unwrap(),
            Vec::<f64>::new()
        );
    }
}
