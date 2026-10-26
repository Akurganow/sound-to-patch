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
/// the resampler's delay trimmed, so sample `n` of the output sits at the
/// time of sample `n * from / to` of the input. Equal rates return a copy.
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
    Ok(output.take_data())
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
    fn equal_rates_and_empty_input_return_a_copy() {
        let input = vec![0.25, -0.5];
        assert_eq!(resample(&input, rate(48_000), rate(48_000)).unwrap(), input);
        assert_eq!(
            resample(&[], rate(48_000), rate(22_050)).unwrap(),
            Vec::<f64>::new()
        );
    }
}
