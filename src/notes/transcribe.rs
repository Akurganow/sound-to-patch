//! The transcription model: one run per stem, then any number of decodes.
//!
//! The model reads mono `f32` samples at 22,050 Hz and returns three
//! activation matrices, the posteriorgrams, with one frame every 256
//! samples. Running it is the expensive step. Decoding notes from the
//! posteriorgrams is cheap, so every profile decodes the same run.

use std::error::Error;
use std::fmt;
use std::num::NonZeroUsize;

use basic_pitch::notes::{NoteEvent, frame_to_time, model_output_to_notes};
use basic_pitch::{BasicPitch, Posteriorgrams};

pub use basic_pitch::model::{Backend, MODEL_SHA256};

use super::{DecodeSettings, LOWEST_MODEL_PITCH, Note, NoteList, PitchTrack, TrackError, Velocity};
use crate::audio::{ResampleError, SampleRate, resample};
use crate::cast;
use crate::units::{MidiPitch, Seconds};

/// Samples between two model frames.
const MODEL_HOP: u16 = 256;

/// The input rate of the model.
pub const MODEL_RATE: SampleRate = SampleRate::from_const(22_050);

/// The f0 tracker that the notes step runs beside the model:
/// [`PitchTrack::of`] unless [`Transcriber::with_tracker`] names another.
pub type Tracker = fn(&[f64], SampleRate) -> Result<PitchTrack, TrackError>;

/// The transcription model with its inference engine, built once and used
/// for every stem, and the f0 tracker of the notes step.
#[derive(Debug)]
pub struct Transcriber {
    model: BasicPitch,
    tracker: Tracker,
}

impl Transcriber {
    /// Loads the bundled model on the pure-Rust engine and spreads the
    /// windows over every core.
    ///
    /// # Errors
    ///
    /// Returns a [`TranscribeError`] when the engine cannot load the model.
    pub fn new() -> Result<Self, TranscribeError> {
        let threads = std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
        Ok(Self {
            model: BasicPitch::new()
                .map_err(TranscribeError::Model)?
                .with_threads(threads),
            tracker: PitchTrack::of,
        })
    }

    /// Spreads the windows over this many threads. The thread count never
    /// changes the result.
    #[must_use]
    pub fn with_threads(self, threads: NonZeroUsize) -> Self {
        Self {
            model: self.model.with_threads(threads),
            ..self
        }
    }

    /// Runs the model on another engine, such as one that counts its calls
    /// in a test.
    #[must_use]
    pub fn with_backend(backend: impl Backend + 'static) -> Self {
        Self {
            model: BasicPitch::with_backend(backend),
            tracker: PitchTrack::of,
        }
    }

    /// Runs another f0 tracker in the notes step, such as one that skips
    /// the pYIN track in a test whose subject is not the track.
    #[must_use]
    pub fn with_tracker(self, tracker: Tracker) -> Self {
        Self { tracker, ..self }
    }

    /// Runs the f0 tracker on a mono stem at any rate.
    pub(crate) fn track(&self, mono: &[f64], rate: SampleRate) -> Result<PitchTrack, TrackError> {
        (self.tracker)(mono, rate)
    }

    /// Runs the model once on a mono signal at any rate. The signal is
    /// resampled to [`MODEL_RATE`] and cast to `f32`, with its gain
    /// untouched: the model output depends on the absolute level.
    ///
    /// # Errors
    ///
    /// Returns a [`TranscribeError`] when resampling fails, a sample is
    /// not finite or the engine fails.
    pub fn run(&self, mono: &[f64], rate: SampleRate) -> Result<ModelRun, TranscribeError> {
        let samples = resample(mono, rate, MODEL_RATE).map_err(TranscribeError::Resample)?;
        let input: Vec<f32> = samples.into_iter().map(cast::f64_to_f32).collect();
        let posteriorgrams = self.model.predict(&input).map_err(TranscribeError::Model)?;
        Ok(ModelRun { posteriorgrams })
    }
}

/// The posteriorgrams of one model run over a stem.
#[derive(Clone, Debug)]
pub struct ModelRun {
    posteriorgrams: Posteriorgrams,
}

impl ModelRun {
    /// Wraps hand-built posteriorgrams, as a test's.
    #[cfg(test)]
    pub(crate) fn from_posteriorgrams(posteriorgrams: Posteriorgrams) -> Self {
        Self { posteriorgrams }
    }

    /// Returns the note, onset and contour activations.
    #[must_use]
    pub fn posteriorgrams(&self) -> &Posteriorgrams {
        &self.posteriorgrams
    }

    /// Returns the strongest onset activation of a key, 0 to 1, over the
    /// frames that start within one frame of `time`, or `None` outside the
    /// run or the model's keys. The decoder starts a note on an onset peak,
    /// or grows one from the note activations when no onset fires.
    #[must_use]
    pub fn onset_strength(&self, time: Seconds, pitch: MidiPitch) -> Option<f64> {
        let onsets = &self.posteriorgrams.onset;
        let bin = cast::round_to_usize(pitch.get() - f64::from(LOWEST_MODEL_PITCH))
            .filter(|&bin| bin < onsets.n_bins())?;
        let hop = f64::from(MODEL_HOP) / f64::from(MODEL_RATE.get());
        (0..onsets.n_frames())
            .filter(|&frame| (frame_to_time(frame) - time.get()).abs() <= hop)
            .filter_map(|frame| onsets.get(frame, bin))
            .map(f64::from)
            .reduce(f64::max)
    }

    /// Decodes the notes with one set of settings, without running the
    /// model again.
    ///
    /// Each note takes its times from the decoder's seconds, never from
    /// frame indices, and its velocity from the decoder. That velocity is
    /// the mean activation, a confidence and not a loudness. A note whose
    /// end does not follow its start carries no sound and is left out.
    ///
    /// # Errors
    ///
    /// Returns a [`TranscribeError`] when the decoder rejects the settings.
    pub fn decode(&self, settings: &DecodeSettings) -> Result<NoteList, TranscribeError> {
        let events = model_output_to_notes(&self.posteriorgrams, &settings.note_params())
            .map_err(TranscribeError::Model)?;
        Ok(NoteList::new(events.iter().filter_map(note_of).collect()))
    }
}

fn note_of(event: &NoteEvent) -> Option<Note> {
    let note = Note::new(
        Seconds::clamped(event.start_s),
        Seconds::clamped(event.end_s),
        MidiPitch::clamped(f64::from(event.pitch)),
    )
    .ok()?;
    // MIDI reserves velocity 0 for a note-off, so the lowest confidence
    // reads as 1.
    Some(match Velocity::new(event.velocity().max(1)) {
        Ok(velocity) => note.with_velocity(velocity),
        Err(_) => note,
    })
}

/// A model run or a decode that failed.
#[derive(Debug)]
pub enum TranscribeError {
    /// The stem could not be resampled to the model rate.
    Resample(ResampleError),
    /// The model or its decoder failed, or a sample was not finite.
    Model(basic_pitch::Error),
}

impl fmt::Display for TranscribeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Resample(source) => {
                write!(
                    f,
                    "cannot prepare the stem for the transcription model: {source}"
                )
            }
            Self::Model(source) => write!(f, "the transcription model failed: {source}"),
        }
    }
}

impl Error for TranscribeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Resample(source) => Some(source),
            Self::Model(source) => Some(source),
        }
    }
}
