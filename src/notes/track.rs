//! The f0 track of a stem: one probabilistic YIN (pYIN) run over the whole
//! stem, frame by frame, with a voiced flag per frame.
//!
//! The track follows the fundamental, so it serves the register detection,
//! the octave check of the notes and the f0 contour of each note. It runs
//! at the model rate with frames of 2048 samples, a hop of 128 samples
//! (5.8 ms) and pitch bins of a tenth of a semitone, over the whole stem
//! at once. The tracker is the in-repo port of pYIN in `pyin.rs`; its cost
//! grows with the length of the stem.

use std::error::Error;
use std::fmt;

use super::pyin::{Pyin, PyinError, Settings};
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
        let tracker = Pyin::new(Settings {
            fmin: TRACK_FMIN.get(),
            fmax: TRACK_FMAX.get(),
            rate: MODEL_RATE.get(),
            frame: TRACK_FRAME,
            hop: TRACK_HOP,
            bins_per_semitone: cast::round_to_usize(BINS_PER_SEMITONE).unwrap_or(1),
        })
        .map_err(|PyinError(reason)| TrackError::Tracker(reason))?;
        let f0s = tracker
            .run(&samples)
            .map_err(|PyinError(reason)| TrackError::Tracker(reason))?;
        // The same centred frames as the tracker's, so frame m lines up.
        let rms = frame_rms(
            &samples,
            // A hop above zero never fails; the test of the settings pins it.
            StftParams::fixed(TRACK_FRAME)
                .with_hop(TRACK_HOP)
                .unwrap_or(StftParams::fixed(TRACK_FRAME)),
        );
        let loudest = rms.iter().copied().fold(0.0, f64::max);
        let frames = f0s
            .into_iter()
            .enumerate()
            .map(|(m, f0)| TrackFrame {
                time: Seconds::clamped(
                    cast::usize_to_f64(m) * cast::usize_to_f64(TRACK_HOP)
                        / f64::from(MODEL_RATE.get()),
                ),
                f0: f0.filter(|hz| hz.is_finite() && *hz > 0.0).map(Hz::clamped),
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
        self.frames
            .iter()
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

/// An f0 track that could not be built.
#[derive(Debug)]
pub enum TrackError {
    /// The stem could not be resampled to the model rate.
    Resample(ResampleError),
    /// A sample at this index is NaN or infinite.
    NotFinite(usize),
    /// The tracker could not run, for the reason given.
    Tracker(&'static str),
}

impl fmt::Display for TrackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resample(source) => {
                write!(f, "cannot prepare the stem for the f0 track: {source}")
            }
            Self::NotFinite(index) => write!(f, "cannot track f0: sample {index} is not finite"),
            Self::Tracker(reason) => write!(f, "cannot track f0: {reason}"),
        }
    }
}

impl Error for TrackError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Resample(source) => Some(source),
            Self::NotFinite(_) | Self::Tracker(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cast;

    fn frame(time: f64, f0: Option<f64>) -> TrackFrame {
        TrackFrame {
            time: Seconds::new(time).unwrap(),
            f0: f0.map(|hz| Hz::new(hz).unwrap()),
            level: Decibels::new(0.0).unwrap(),
        }
    }

    #[test]
    fn voiced_frames_run_from_the_onset_up_to_the_offset() {
        let track = PitchTrack::from_frames(vec![
            frame(0.1, Some(110.0)),
            frame(0.2, None),
            frame(0.3, Some(220.0)),
            frame(0.4, Some(440.0)),
        ]);
        let s = |value| Seconds::new(value).unwrap();
        let times: Vec<f64> = track
            .voiced_between(s(0.1), s(0.4))
            .map(|(time, _)| time.get())
            .collect();
        assert_eq!(times, [0.1, 0.3]);
        let pitches: Vec<f64> = track
            .voiced_pitches()
            .iter()
            .map(|p| p.get().round())
            .collect();
        assert_eq!(pitches, [45.0, 57.0, 69.0]);
    }

    #[test]
    fn errors_name_their_cause_and_keep_their_source() {
        let not_finite = TrackError::NotFinite(7);
        assert_eq!(
            not_finite.to_string(),
            "cannot track f0: sample 7 is not finite"
        );
        assert!(not_finite.source().is_none());
        let native = SampleRate::new(48_000).unwrap();
        let resample = TrackError::Resample(resample(&[f64::NAN], native, MODEL_RATE).unwrap_err());
        assert!(
            resample
                .to_string()
                .starts_with("cannot prepare the stem for the f0 track: ")
        );
        assert!(resample.source().is_some());
    }

    /// The constructor of the tracker panics unless these hold; the
    /// settings are constants, so this test is their only check.
    #[test]
    fn the_settings_meet_every_rule_of_the_tracker() {
        let rate = f64::from(MODEL_RATE.get());
        let (fmin, fmax) = (TRACK_FMIN.get(), TRACK_FMAX.get());
        assert!(0.0 < fmin && fmin < fmax && fmax <= rate / 2.0);
        let window = TRACK_FRAME / 2;
        // The longest period, ceil(22050 / 25) = 882 lags, fits the frame,
        // so the frame does not raise fmin silently.
        let longest = cast::ceil_to_usize(rate / fmin).unwrap();
        assert!(longest < TRACK_FRAME - window - 1);
        let shortest = cast::floor_to_usize(rate / fmax).unwrap().max(1);
        assert!(longest - shortest >= 2);
        // Pitch bins against the widest transition of 35.92 octaves per
        // second.
        let bins =
            cast::floor_to_usize(12.0 * BINS_PER_SEMITONE * (fmax / fmin).log2()).unwrap() + 1;
        let per_frame =
            cast::round_to_usize(35.92 * 12.0 * cast::usize_to_f64(TRACK_HOP) / rate).unwrap();
        assert!(bins > per_frame * 10);
        assert!(TRACK_HOP > 0 && window > 0);
    }
}
