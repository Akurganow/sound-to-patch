//! The f0 track of a stem: one probabilistic YIN (pYIN) run over the whole
//! stem, frame by frame, with a voiced flag per frame.
//!
//! The track follows the fundamental, so it serves the register detection,
//! the octave check of the notes and the f0 contour of each note. It runs
//! at the model rate with frames of 2048 samples, a hop of 128 samples
//! (5.8 ms) and pitch bins of a tenth of a semitone, over the whole stem
//! at once. Its cost grows with the stem: memory and time are reported in
//! the bench docs and not reduced.

use std::error::Error;
use std::fmt;

use pyin::{Framing, PYINExecutor, PadMode};

use super::{Contour, ContourPoint, MODEL_RATE, Note, NoteList};
use crate::audio::{ResampleError, SampleRate, resample};
use crate::cast;
use crate::dsp::{StftParams, frame_rms};
use crate::units::{Decibels, Hz, MidiPitch, Seconds};

/// The lowest f0 the track looks for, below B0 (30.87 Hz).
pub const TRACK_FMIN: Hz = Hz::from_const(25.0);
/// The highest f0 the track looks for, just above C8 (4186 Hz), the top of
/// the transcription model. A lower ceiling reads every tone above it as a
/// subharmonic.
pub const TRACK_FMAX: Hz = Hz::from_const(4200.0);
/// Samples per analysis frame at the model rate.
pub const TRACK_FRAME: usize = 2048;
/// Samples between frames at the model rate.
pub const TRACK_HOP: usize = 128;
/// Pitch bins per semitone.
const BINS_PER_SEMITONE: f64 = 10.0;
/// The level of a frame of digital silence.
const SILENT_LEVEL: Decibels = Decibels::from_const(-200.0);

/// One frame of the track.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackFrame {
    /// The centre of the frame from the start of the stem.
    pub time: Seconds,
    /// The f0, or `None` when the frame reads as unvoiced.
    pub f0: Option<Hz>,
    /// The RMS of the frame relative to the loudest frame of the stem:
    /// 0 dB or less, and -200 dB for digital silence.
    pub level: Decibels,
}

/// The f0 track of a stem.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PitchTrack {
    frames: Vec<TrackFrame>,
}

impl PitchTrack {
    /// Runs the tracker over a mono stem at any rate. The stem is
    /// resampled to the model rate, and the copy the tracker reads is
    /// scaled to a peak of 1, because the tracker treats a frame below a
    /// fixed energy as silence. A silent or empty stem gives a track with
    /// no voiced frame.
    ///
    /// # Errors
    ///
    /// Returns a [`TrackError`] when resampling fails or a sample is not
    /// finite.
    pub fn of(mono: &[f64], rate: SampleRate) -> Result<Self, TrackError> {
        check_params()?;
        let mut samples = resample(mono, rate, MODEL_RATE).map_err(TrackError::Resample)?;
        if let Some(index) = samples.iter().position(|x| !x.is_finite()) {
            return Err(TrackError::NotFinite(index));
        }
        let peak = samples.iter().fold(0.0_f64, |peak, x| peak.max(x.abs()));
        if samples.is_empty() || peak == 0.0 {
            return Ok(Self::default());
        }
        for x in &mut samples {
            *x /= peak;
        }
        let mut executor = PYINExecutor::new(
            TRACK_FMIN.get(),
            TRACK_FMAX.get(),
            MODEL_RATE.get(),
            TRACK_FRAME,
            None,
            Some(TRACK_HOP),
            Some(1.0 / BINS_PER_SEMITONE),
        );
        let (times, f0, voiced, _) =
            executor.pyin(&samples, f64::NAN, Framing::Center(PadMode::Constant(0.0)));
        // The same centred frames as the tracker's, so frame m lines up.
        let rms = frame_rms(
            &samples,
            StftParams::fixed(TRACK_FRAME)
                .with_hop(TRACK_HOP)
                .map_err(|_| TrackError::Settings)?,
        );
        let loudest = rms.iter().copied().fold(0.0, f64::max);
        let frames = times
            .into_iter()
            .zip(f0)
            .zip(voiced)
            .enumerate()
            .map(|(m, ((time, f0), voiced))| TrackFrame {
                time: Seconds::clamped(time),
                f0: (voiced && f0.is_finite() && f0 > 0.0).then(|| Hz::clamped(f0)),
                level: rms
                    .get(m)
                    .and_then(|&value| Decibels::from_amplitude_gain(value / loudest).ok())
                    .map_or(SILENT_LEVEL, |level| {
                        Decibels::clamped(level.get().max(SILENT_LEVEL.get()))
                    }),
            })
            .collect();
        Ok(Self { frames })
    }

    /// Builds a track from frames, such as a test's.
    #[must_use]
    pub fn from_frames(frames: Vec<TrackFrame>) -> Self {
        Self { frames }
    }

    /// Returns every frame in time order.
    #[must_use]
    pub fn frames(&self) -> &[TrackFrame] {
        &self.frames
    }

    /// Returns the voiced frames from `onset` (inclusive) to `offset`
    /// (exclusive) as `(time, f0)`.
    pub fn voiced_between(
        &self,
        onset: Seconds,
        offset: Seconds,
    ) -> impl Iterator<Item = (Seconds, Hz)> + '_ {
        let start = self.frames.partition_point(|frame| frame.time < onset);
        self.frames[start..]
            .iter()
            .take_while(move |frame| frame.time < offset)
            .filter_map(|frame| frame.f0.map(|f0| (frame.time, f0)))
    }

    /// Returns the pitches of every voiced frame, as fractional MIDI
    /// pitches.
    #[must_use]
    pub fn voiced_pitches(&self) -> Vec<MidiPitch> {
        self.voiced_pitches_within(Decibels::clamped(f64::MAX))
    }

    /// Returns the pitches of the voiced frames whose level lies within
    /// `window` of the loudest frame, as fractional MIDI pitches.
    #[must_use]
    pub fn voiced_pitches_within(&self, window: Decibels) -> Vec<MidiPitch> {
        self.frames
            .iter()
            .filter(|frame| frame.level.get() >= -window.get().abs())
            .filter_map(|frame| frame.f0)
            .filter_map(|f0| MidiPitch::try_from(f0).ok())
            .collect()
    }

    /// Returns the f0 contour of a span: the voiced frames inside it, or
    /// `None` when none is voiced.
    #[must_use]
    pub fn contour(&self, onset: Seconds, offset: Seconds) -> Option<Contour> {
        let points: Vec<ContourPoint> = self
            .voiced_between(onset, offset)
            .map(|(time, f0)| ContourPoint { time, f0 })
            .collect();
        Contour::new(points).ok()
    }

    /// Sets the f0 contour of every note that has a voiced frame inside it.
    #[must_use]
    pub fn with_contours(&self, notes: &NoteList) -> NoteList {
        NoteList::new(
            notes
                .notes()
                .iter()
                .map(|note| match self.contour(note.onset(), note.offset()) {
                    Some(contour) => note.clone().with_contour(contour),
                    None => note.clone(),
                })
                .collect::<Vec<Note>>(),
        )
    }
}

/// Fails unless the tracker accepts the settings: it panics on bad ones,
/// and a frame too short for the lowest period silently raises `fmin`.
fn check_params() -> Result<(), TrackError> {
    let rate = f64::from(MODEL_RATE.get());
    let (fmin, fmax) = (TRACK_FMIN.get(), TRACK_FMAX.get());
    let window = TRACK_FRAME / 2;
    let longest_period = cast::ceil_to_usize(rate / fmin).unwrap_or(usize::MAX);
    let valid = 0.0 < fmin
        && fmin < fmax
        && fmax <= rate / 2.0
        && TRACK_HOP > 0
        && longest_period + window < TRACK_FRAME;
    if valid {
        Ok(())
    } else {
        Err(TrackError::Settings)
    }
}

/// An f0 track that could not be built.
#[derive(Debug)]
pub enum TrackError {
    /// The stem could not be resampled to the model rate.
    Resample(ResampleError),
    /// A sample at this index is NaN or infinite.
    NotFinite(usize),
    /// The tracker settings break its own rules.
    Settings,
}

impl fmt::Display for TrackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resample(source) => {
                write!(f, "cannot prepare the stem for the f0 track: {source}")
            }
            Self::NotFinite(index) => write!(f, "cannot track f0: sample {index} is not finite"),
            Self::Settings => write!(
                f,
                "the f0 track settings are invalid: the frame must hold the longest period plus the window"
            ),
        }
    }
}

impl Error for TrackError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Resample(source) => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_keep_the_lowest_period_inside_the_frame() {
        check_params().unwrap();
        assert_eq!(TRACK_FMIN, Hz::new(25.0).unwrap());
        assert_eq!(TRACK_FMAX, Hz::new(4200.0).unwrap());
    }
}
