//! Stem intake: decoding, mono, resampling and the [`Stem`].
//!
//! A file decodes at its native rate. A consumer that needs another rate
//! resamples. The [`Stem`] adds the beat grid, the band edge, the flat-top
//! flag and the stereo screen.
//!
//! Samples are `f64` from the decoder on. Every channel is kept, because the
//! stereo statistics need both. Mono is the channel mean, with no gain
//! normalisation. Only a consumer with a fixed input rate, such as the
//! transcription model, resamples; analysis stays at the native rate.
//!
//! ```no_run
//! use sound_to_patch::audio::{self, SampleRate};
//!
//! let stem = audio::decode("bass.wav".as_ref())?.into_audio();
//! let mono = stem.mono();
//! let model_rate = SampleRate::new(22_050)?;
//! let resampled = audio::resample(&mono, stem.sample_rate(), model_rate)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::error::Error;
use std::fmt;
use std::num::NonZeroU32;

use crate::cast;
use crate::units::{Hz, Seconds};

mod decode;
mod resample;
mod stem;
mod stereo;
mod tempo;
mod wav;

pub use decode::{DecodeError, Decoded, MAX_SKIPPED_PACKETS, SkippedPacket, decode};
pub use resample::{ResampleError, resample};
pub use stem::{
    BAND_EDGE_CONVENTION, BAND_EDGE_MARGIN_DB, BAND_EDGE_PLATEAU_DB, BAND_EDGE_STEEP_DROP_DB,
    BeatGrid, CEILING_TOLERANCE, CLIP_APPROACH, Division, FLAT_TOP_MIN_RUN_S, FLAT_TOP_MIN_SHARE,
    Feel, HARD_LIMITED_CONVENTION, LONG_FRAME_BELOW_HZ, NoteValue, Stem, band_edge, is_flat_topped,
};
pub use stereo::{
    Band, HIGH_EDGE_HZ, SCREEN_FRAME_LEN, SCREEN_HARMONICS, STEREO_CONVENTION, SUB_EDGE_HZ,
    StereoScreen, StereoStats, stereo_screen,
};
pub use tempo::{
    BPM_STEP, MAX_BEAT_LAGS, MIN_BEAT_LAGS, OVERLAP_BEATS, PEAK_RATIO_MIN, PERIODICITY_MIN,
    SEARCH_SHARE, SHARPNESS_MIN, SPARSE_ONSETS_MAX, SPARSE_PERIODICITY_MIN, TEMPO_CONVENTION,
    Tempo,
};
pub use wav::write_wav;

/// A sample rate in samples per second: a whole number above zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SampleRate(NonZeroU32);

impl SampleRate {
    /// Checks that the rate is above zero.
    ///
    /// # Errors
    ///
    /// Returns an [`AudioError`] for zero.
    pub fn new(rate: u32) -> Result<Self, AudioError> {
        NonZeroU32::new(rate)
            .map(Self)
            .ok_or(AudioError::ZeroSampleRate)
    }

    /// Builds a rate in a `const` item. Every caller passes a literal above
    /// zero; zero would read as 1.
    pub(crate) const fn from_const(rate: u32) -> Self {
        Self(match NonZeroU32::new(rate) {
            Some(rate) => rate,
            None => NonZeroU32::MIN,
        })
    }

    /// Returns the rate in samples per second.
    #[must_use]
    pub fn get(self) -> u32 {
        self.0.get()
    }

    /// Returns the rate as a frequency.
    #[must_use]
    pub fn hz(self) -> Hz {
        Hz::clamped(f64::from(self.get()))
    }

    /// Returns the Nyquist frequency, half the rate.
    #[must_use]
    pub fn nyquist(self) -> Hz {
        Hz::clamped(f64::from(self.get()) / 2.0)
    }

    /// Returns the time of a sample index.
    #[must_use]
    pub fn seconds(self, samples: usize) -> Seconds {
        Seconds::clamped(cast::usize_to_f64(samples) / f64::from(self.get()))
    }
}

impl fmt::Display for SampleRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} Hz", self.get())
    }
}

/// Decoded audio: one or more channels of equal length at one sample rate.
/// Every sample is finite.
#[derive(Clone, Debug, PartialEq)]
pub struct Audio {
    sample_rate: SampleRate,
    channels: Vec<Vec<f64>>,
}

impl Audio {
    /// Checks the channels: at least one, all of the same length, every
    /// sample finite.
    ///
    /// # Errors
    ///
    /// Returns an [`AudioError`] that names the first failed rule.
    pub fn new(sample_rate: SampleRate, channels: Vec<Vec<f64>>) -> Result<Self, AudioError> {
        let Some(first) = channels.first() else {
            return Err(AudioError::NoChannels);
        };
        let frames = first.len();
        if let Some((channel, other)) = channels
            .iter()
            .enumerate()
            .find(|(_, channel)| channel.len() != frames)
        {
            return Err(AudioError::UnequalChannels {
                channel,
                frames: other.len(),
                expected: frames,
            });
        }
        for (channel, samples) in channels.iter().enumerate() {
            if let Some(frame) = samples.iter().position(|sample| !sample.is_finite()) {
                return Err(AudioError::NonFinite { channel, frame });
            }
        }
        Ok(Self {
            sample_rate,
            channels,
        })
    }

    /// Returns the sample rate.
    #[must_use]
    pub fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    /// Returns the channels in file order.
    #[must_use]
    pub fn channels(&self) -> &[Vec<f64>] {
        &self.channels
    }

    /// Returns the number of frames, one sample per channel each.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.channels.first().map_or(0, Vec::len)
    }

    /// Returns the length in seconds.
    #[must_use]
    pub fn duration(&self) -> Seconds {
        self.sample_rate.seconds(self.frames())
    }

    /// Returns the mono mix: the mean of the channels in `f64`.
    #[must_use]
    pub fn mono(&self) -> Vec<f64> {
        mono_mean(&self.channels)
    }
}

/// Mixes channels to mono as their mean. Mixing `L = x` with `R = -x`
/// gives silence, and `L = R = x` gives `x`.
///
/// The channels must have equal length; a longer channel's extra samples
/// are ignored.
#[must_use]
pub fn mono_mean(channels: &[Vec<f64>]) -> Vec<f64> {
    let frames = channels.iter().map(Vec::len).min().unwrap_or(0);
    let count = cast::usize_to_f64(channels.len());
    (0..frames)
        .map(|frame| channels.iter().map(|channel| channel[frame]).sum::<f64>() / count)
        .collect()
}

/// A rule that decoded audio breaks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AudioError {
    /// A sample rate of zero.
    ZeroSampleRate,
    /// Audio with no channel.
    NoChannels,
    /// A channel whose length differs from the first channel's.
    UnequalChannels {
        /// The index of the channel.
        channel: usize,
        /// Its length in frames.
        frames: usize,
        /// The length of the first channel.
        expected: usize,
    },
    /// A NaN or infinite sample.
    NonFinite {
        /// The index of the channel.
        channel: usize,
        /// The frame index of the sample.
        frame: usize,
    },
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroSampleRate => write!(f, "the sample rate is zero; it must be above zero"),
            Self::NoChannels => write!(f, "the audio has no channel; it needs at least one"),
            Self::UnequalChannels {
                channel,
                frames,
                expected,
            } => write!(
                f,
                "channel {channel} has {frames} frames, but channel 0 has {expected}; every channel needs the same length"
            ),
            Self::NonFinite { channel, frame } => write!(
                f,
                "the sample at frame {frame} of channel {channel} is NaN or infinite; every sample must be finite"
            ),
        }
    }
}

impl Error for AudioError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn rate(hz: u32) -> SampleRate {
        SampleRate::new(hz).unwrap()
    }

    #[test]
    fn opposite_channels_mix_to_silence_and_equal_ones_to_themselves() {
        let x: Vec<f64> = (0..64).map(|n| (f64::from(n) * 0.3).sin()).collect();
        let negated: Vec<f64> = x.iter().map(|v| -v).collect();
        assert!(mono_mean(&[x.clone(), negated]).iter().all(|&v| v == 0.0));
        assert_eq!(mono_mean(&[x.clone(), x.clone()]), x);
    }

    #[test]
    fn mono_keeps_f64_precision() {
        let mixed = mono_mean(&[vec![0.5], vec![0.5 + 1e-12]]);
        assert!((mixed[0] - (0.5 + 5e-13)).abs() <= 1e-15, "{}", mixed[0]);
    }

    #[test]
    fn audio_rejects_unequal_channels_and_non_finite_samples() {
        assert_eq!(
            Audio::new(rate(48_000), vec![]).unwrap_err(),
            AudioError::NoChannels
        );
        assert_eq!(
            Audio::new(rate(48_000), vec![vec![0.0; 3], vec![0.0; 2]]).unwrap_err(),
            AudioError::UnequalChannels {
                channel: 1,
                frames: 2,
                expected: 3
            }
        );
        let error =
            Audio::new(rate(48_000), vec![vec![0.0, 0.0], vec![0.0, f64::NAN]]).unwrap_err();
        assert_eq!(
            error.to_string(),
            "the sample at frame 1 of channel 1 is NaN or infinite; every sample must be finite"
        );
    }

    #[test]
    fn audio_reports_rate_frames_and_duration() {
        let audio = Audio::new(rate(48_000), vec![vec![0.25; 24_000], vec![0.75; 24_000]]).unwrap();
        assert_eq!(audio.frames(), 24_000);
        assert_eq!(audio.duration().get(), 0.5);
        assert_eq!(audio.mono(), vec![0.5; 24_000]);
        assert_eq!(audio.sample_rate().hz().get(), 48_000.0);
        assert_eq!(audio.sample_rate().nyquist().get(), 24_000.0);
    }

    #[test]
    fn sample_rate_rejects_zero() {
        assert_eq!(SampleRate::new(0).unwrap_err(), AudioError::ZeroSampleRate);
        assert_eq!(rate(44_100).to_string(), "44100 Hz");
    }
}
