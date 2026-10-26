//! The short-time Fourier transform with centred frames.

use std::error::Error;
use std::fmt;
use std::sync::Arc;

use realfft::num_complex::Complex64;
use realfft::{RealFftPlanner, RealToComplex};

use super::Window;
use crate::audio::SampleRate;
use crate::cast;
use crate::units::{Hz, Seconds};

/// How a centred frame continues past either end of the signal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Padding {
    /// Zeros. The convention for descriptors.
    Zero,
    /// The signal mirrored at its end sample, which is not repeated
    /// (`d c b | a b c d | c b a`).
    Reflect,
}

/// The frame layout of an STFT: frame length `N`, hop, window, padding and
/// FFT oversampling.
///
/// Frames are centred: frame `m` covers samples `m hop - N/2` to
/// `m hop - N/2 + N - 1`, so a signal of `len` samples gives
/// `1 + floor((len + 2 floor(N/2) - N) / hop)` frames, which is
/// `1 + floor(len / hop)` for an even `N`. The default hop is `N/4`.
///
/// Oversampling by a factor `p` zero-pads each windowed frame to `p N`
/// samples, so the bins sit `fs / (p N)` apart. It refines peak positions
/// and changes no frequency resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StftParams {
    frame_len: usize,
    hop: usize,
    window: Window,
    padding: Padding,
    oversample: usize,
}

impl StftParams {
    /// A periodic Hann window of `frame_len` samples, hop `frame_len / 4`,
    /// zero padding and no oversampling.
    ///
    /// # Errors
    ///
    /// Returns an [`StftError`] for a frame shorter than 4 samples.
    pub fn new(frame_len: usize) -> Result<Self, StftError> {
        if frame_len < 4 {
            return Err(StftError::FrameTooShort(frame_len));
        }
        Ok(Self {
            frame_len,
            hop: frame_len / 4,
            window: Window::Hann,
            padding: Padding::Zero,
            oversample: 1,
        })
    }

    /// Sets the hop in samples.
    ///
    /// # Errors
    ///
    /// Returns an [`StftError`] for a hop of zero.
    pub fn with_hop(self, hop: usize) -> Result<Self, StftError> {
        if hop == 0 {
            return Err(StftError::ZeroHop);
        }
        Ok(Self { hop, ..self })
    }

    /// Sets the window.
    #[must_use]
    pub fn with_window(self, window: Window) -> Self {
        Self { window, ..self }
    }

    /// Sets the padding at both ends of the signal.
    #[must_use]
    pub fn with_padding(self, padding: Padding) -> Self {
        Self { padding, ..self }
    }

    /// Sets the FFT oversampling factor.
    ///
    /// # Errors
    ///
    /// Returns an [`StftError`] for a factor of zero.
    pub fn with_oversample(self, factor: usize) -> Result<Self, StftError> {
        if factor == 0 {
            return Err(StftError::ZeroOversample);
        }
        Ok(Self {
            oversample: factor,
            ..self
        })
    }

    /// Returns the frame length `N` in samples.
    #[must_use]
    pub fn frame_len(self) -> usize {
        self.frame_len
    }

    /// Returns the hop in samples.
    #[must_use]
    pub fn hop(self) -> usize {
        self.hop
    }

    /// Returns the window.
    #[must_use]
    pub fn window(self) -> Window {
        self.window
    }

    /// Returns the padding.
    #[must_use]
    pub fn padding(self) -> Padding {
        self.padding
    }

    /// Returns the FFT length, `N` times the oversampling factor.
    #[must_use]
    pub fn fft_len(self) -> usize {
        self.frame_len * self.oversample
    }

    /// Returns the number of frequency bins, `fft_len / 2 + 1`.
    #[must_use]
    pub fn bins(self) -> usize {
        self.fft_len() / 2 + 1
    }

    /// Returns the number of centred frames for a signal of `len` samples.
    #[must_use]
    pub fn frame_count(self, len: usize) -> usize {
        let padded = len + 2 * (self.frame_len / 2);
        1 + padded.saturating_sub(self.frame_len) / self.hop
    }

    /// Returns the frequency of bin `k`, `k fs / fft_len`.
    #[must_use]
    pub fn bin_hz(self, bin: usize, rate: SampleRate) -> Hz {
        Hz::clamped(cast::usize_to_f64(bin) * rate.hz().get() / cast::usize_to_f64(self.fft_len()))
    }

    /// Returns the bin spacing, `fs / fft_len`.
    #[must_use]
    pub fn bin_width(self, rate: SampleRate) -> Hz {
        self.bin_hz(1, rate)
    }

    /// Returns the time of the centre of frame `m`, `m hop / fs`.
    #[must_use]
    pub fn frame_time(self, frame: usize, rate: SampleRate) -> Seconds {
        rate.seconds(frame * self.hop)
    }

    /// Returns the first sample of frame `m`; negative indices lie in the
    /// padding before the signal.
    pub(crate) fn frame_start(self, frame: usize) -> isize {
        // A frame index times a hop stays far below isize::MAX for any
        // signal that fits in memory.
        let centre = isize::try_from(frame * self.hop).unwrap_or(isize::MAX);
        let half = isize::try_from(self.frame_len / 2).unwrap_or(isize::MAX);
        centre.saturating_sub(half)
    }
}

/// Reads sample `index` of `samples` under a padding rule, for an index
/// that may lie before or after the signal.
pub(crate) fn padded_sample(samples: &[f64], index: isize, padding: Padding) -> f64 {
    if let Ok(i) = usize::try_from(index)
        && let Some(&sample) = samples.get(i)
    {
        return sample;
    }
    match padding {
        Padding::Zero => 0.0,
        Padding::Reflect => {
            let len = samples.len();
            if len < 2 {
                return samples.first().copied().unwrap_or(0.0);
            }
            let period = 2 * (len - 1);
            // The mirror is symmetric about sample 0, so x[-i] = x[i].
            let wrapped = index.unsigned_abs() % period;
            samples[if wrapped >= len {
                period - wrapped
            } else {
                wrapped
            }]
        }
    }
}

/// A planned STFT. It computes one frame at a time into a buffer it owns,
/// so a long stem never holds its whole spectrogram in memory.
///
/// The transform is not normalised: a frame holds
/// `X[m, k] = sum_n w[n] x[m hop - N/2 + n] exp(-j 2 pi k n / (p N))`.
///
/// ```
/// use sound_to_patch::audio::SampleRate;
/// use sound_to_patch::dsp::{Stft, StftParams};
///
/// let rate = SampleRate::new(8_000)?;
/// let params = StftParams::new(256)?;
/// let mut stft = Stft::new(params);
/// let signal: Vec<f64> = (0..1_000).map(|n| (0.1 * f64::from(n)).sin()).collect();
/// assert_eq!(params.frame_count(signal.len()), 16);
/// let peak = stft
///     .frame(&signal, 8)
///     .iter()
///     .enumerate()
///     .max_by(|a, b| a.1.norm().total_cmp(&b.1.norm()))
///     .map(|(bin, _)| bin);
/// // 0.1 rad per sample is 127.3 Hz at 8 kHz: bin 4 of 31.25 Hz.
/// assert_eq!(peak, Some(4));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct Stft {
    params: StftParams,
    window: Vec<f64>,
    fft: Arc<dyn RealToComplex<f64>>,
    input: Vec<f64>,
    output: Vec<Complex64>,
    scratch: Vec<Complex64>,
}

impl Stft {
    /// Plans the FFT and the window for these parameters.
    #[must_use]
    pub fn new(params: StftParams) -> Self {
        let fft = RealFftPlanner::<f64>::new().plan_fft_forward(params.fft_len());
        Self {
            window: params.window().samples(params.frame_len()),
            input: fft.make_input_vec(),
            output: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            fft,
            params,
        }
    }

    /// Returns the parameters.
    #[must_use]
    pub fn params(&self) -> StftParams {
        self.params
    }

    /// Computes frame `m` of `samples` and returns its `bins()` complex
    /// values. A frame past the end holds only padding.
    pub fn frame(&mut self, samples: &[f64], frame: usize) -> &[Complex64] {
        let start = self.params.frame_start(frame);
        let padding = self.params.padding();
        self.input.fill(0.0);
        for (offset, (slot, weight)) in self.input.iter_mut().zip(&self.window).enumerate() {
            let index = start.saturating_add(isize::try_from(offset).unwrap_or(isize::MAX));
            *slot = weight * padded_sample(samples, index, padding);
        }
        let result =
            self.fft
                .process_with_scratch(&mut self.input, &mut self.output, &mut self.scratch);
        debug_assert!(result.is_ok(), "buffers come from the plan: {result:?}");
        &self.output
    }

    /// Computes every frame of `samples`. Holds the whole spectrogram, so
    /// use it for short signals; iterate with [`Stft::frame`] otherwise.
    pub fn frames(&mut self, samples: &[f64]) -> Vec<Vec<Complex64>> {
        (0..self.params.frame_count(samples.len()))
            .map(|frame| self.frame(samples, frame).to_vec())
            .collect()
    }
}

impl fmt::Debug for Stft {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Stft")
            .field("params", &self.params)
            .finish_non_exhaustive()
    }
}

/// STFT parameters that cannot work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StftError {
    /// A frame shorter than 4 samples.
    FrameTooShort(usize),
    /// A hop of zero samples.
    ZeroHop,
    /// An oversampling factor of zero.
    ZeroOversample,
}

impl fmt::Display for StftError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrameTooShort(len) => {
                write!(
                    f,
                    "an STFT frame of {len} samples is too short; use 4 or more"
                )
            }
            Self::ZeroHop => write!(f, "an STFT hop of 0 samples never advances; use 1 or more"),
            Self::ZeroOversample => write!(
                f,
                "an oversampling factor of 0 leaves no FFT; use 1 or more"
            ),
        }
    }
}

impl Error for StftError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_count_follows_the_centred_layout() {
        let params = StftParams::new(8).unwrap();
        assert_eq!(params.hop(), 2);
        assert_eq!(params.frame_count(0), 1);
        assert_eq!(params.frame_count(9), 5);
        assert_eq!(params.frame_count(10), 6);
        let odd = StftParams::new(9).unwrap();
        // padded 10 + 8 = 18, (18 - 9) / 2 = 4
        assert_eq!(odd.frame_count(10), 5);
    }

    #[test]
    fn bins_and_times_use_the_fft_length_and_the_hop() {
        let rate = SampleRate::new(8_000).unwrap();
        let params = StftParams::new(256).unwrap().with_oversample(4).unwrap();
        assert_eq!(params.fft_len(), 1024);
        assert_eq!(params.bins(), 513);
        assert_eq!(params.bin_width(rate).get(), 7.8125);
        assert_eq!(params.frame_time(2, rate).get(), 0.016);
    }

    #[test]
    fn reflect_padding_mirrors_without_repeating_the_end() {
        let x = [1.0, 2.0, 3.0, 4.0];
        let read = |i| padded_sample(&x, i, Padding::Reflect);
        assert_eq!([read(-3), read(-2), read(-1)], [4.0, 3.0, 2.0]);
        assert_eq!([read(4), read(5), read(6)], [3.0, 2.0, 1.0]);
        assert_eq!(read(7), 2.0);
        assert_eq!(padded_sample(&x, -1, Padding::Zero), 0.0);
        assert_eq!(padded_sample(&[5.0], 3, Padding::Reflect), 5.0);
        assert_eq!(padded_sample(&[], 3, Padding::Reflect), 0.0);
    }

    #[test]
    fn a_constant_signal_puts_the_window_sum_in_bin_zero() {
        let params = StftParams::new(16).unwrap();
        let mut stft = Stft::new(params);
        let frame = stft.frame(&[1.0; 64], 4).to_vec();
        let window_sum: f64 = Window::Hann.samples(16).iter().sum();
        assert!((frame[0].re - window_sum).abs() < 1e-12);
        // A periodic Hann leaks into bin 1 only.
        assert!(frame[2..].iter().all(|bin| bin.norm() < 1e-12));
    }

    #[test]
    fn bad_parameters_fail_with_a_fix() {
        assert_eq!(StftParams::new(2).unwrap_err(), StftError::FrameTooShort(2));
        let params = StftParams::new(8).unwrap();
        assert_eq!(
            params.with_hop(0).unwrap_err().to_string(),
            "an STFT hop of 0 samples never advances; use 1 or more"
        );
        assert!(params.with_oversample(0).is_err());
    }
}
