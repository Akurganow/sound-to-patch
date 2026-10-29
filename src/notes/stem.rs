//! The notes step of a stem: one model run and one f0 track, the decode
//! settings, the decode and the cleanup, in that order.

use std::error::Error;
use std::fmt;

use basic_pitch::NoteParams;
use serde::Serialize;

use super::{
    DecodeSettings, Detection, MODEL_SHA256, ModelRun, NoteList, OctaveCheck, PitchTrack, Profile,
    QUIET_GATE, StemLevels, TrackError, TranscribeError, Transcriber, Voicing, check_octaves,
    detect, drop_quiet_with, join_splits_with, merge_overlaps,
};
use crate::audio::SampleRate;
use crate::units::Bpm;

/// The version of the transcription crate, pinned in `Cargo.toml`.
pub const TRANSCRIBER_VERSION: &str = "0.1.0";

/// The note counts of the cleanup, one per step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CleanupSteps {
    /// Notes from the decoder.
    pub decoded: usize,
    /// What the octave check did; it keeps the count.
    pub octaves: OctaveCheck,
    /// Notes after octave ghosts merged into their notes.
    pub merged: usize,
    /// Notes after split notes joined.
    pub joined: usize,
    /// Notes the quiet-note guard dropped.
    pub quiet: usize,
    /// Notes kept.
    pub kept: usize,
}

/// Cleans decoded notes in four steps, and counts the notes after each.
///
/// The steps are the octave check (a monophonic part only, since the
/// track reads a chord as one low period), the merge of notes of one key
/// that overlap, the join of split notes, and the quiet-note guard at
/// [`QUIET_GATE`]. `mono` is the stem at `rate`, as the guard reads it.
#[must_use]
pub fn clean_up(
    decoded: &NoteList,
    run: &ModelRun,
    track: &PitchTrack,
    voicing: Voicing,
    mono: &[f64],
    rate: SampleRate,
) -> (NoteList, CleanupSteps) {
    let (checked, octaves) = match voicing {
        Voicing::Monophonic => check_octaves(decoded, track),
        Voicing::Polyphonic => (
            decoded.clone(),
            OctaveCheck {
                unchecked: decoded.notes().len(),
                ..OctaveCheck::default()
            },
        ),
    };
    let merged = merge_overlaps(&checked);
    let levels = StemLevels::new(mono, rate);
    let joined = join_splits_with(&merged, run, &levels);
    let (kept, quiet) = drop_quiet_with(&joined, &levels, QUIET_GATE);
    let steps = CleanupSteps {
        decoded: decoded.notes().len(),
        octaves,
        merged: merged.notes().len(),
        joined: joined.notes().len(),
        quiet,
        kept: kept.notes().len(),
    };
    (kept, steps)
}

/// The transcriber's model and settings, as the `transcription` group of
/// `layers.json` records them.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TranscriptionRecord {
    /// The SHA-256 of the model weights, 64 lowercase hex digits.
    pub model_sha256: &'static str,
    /// The version of the transcription crate.
    pub crate_version: &'static str,
    /// The profile name, or `detected` when the stem chose the settings.
    pub profile: String,
    /// Every decoder parameter, by the crate's own names.
    pub note_params: NoteParams,
}

impl TranscriptionRecord {
    /// The record of a decode with `settings`, from a profile or, with
    /// `None`, from detection.
    #[must_use]
    pub fn new(profile: Option<Profile>, settings: &DecodeSettings) -> Self {
        Self {
            model_sha256: MODEL_SHA256,
            crate_version: TRANSCRIBER_VERSION,
            profile: profile.map_or("detected", Profile::name).to_owned(),
            note_params: settings.note_params(),
        }
    }
}

/// Everything the notes step of a stem found.
#[derive(Clone, Debug)]
pub struct Transcription {
    /// The model run, with its posteriorgrams.
    pub run: ModelRun,
    /// The f0 track.
    pub track: PitchTrack,
    /// What detection found. It runs with a profile as well.
    pub detection: Detection,
    /// The profile that overrode detection, if any.
    pub profile: Option<Profile>,
    /// The voicing the cleanup used: the profile's (chords for `pad`) or
    /// the detected one.
    pub voicing: Voicing,
    /// The decode settings used.
    pub settings: DecodeSettings,
    /// The notes as the decoder gave them.
    pub decoded: NoteList,
    /// The counts of each cleanup step.
    pub steps: CleanupSteps,
    /// The clean notes, with their f0 contours.
    pub notes: NoteList,
    /// The record of the model and the settings.
    pub record: TranscriptionRecord,
}

/// Runs the notes step on a mono stem at any rate: one model run, one f0
/// track, detection, the decode with the detected settings or those of
/// `profile`, and [`clean_up`].
///
/// # Errors
///
/// Returns a [`NotesError`] when the model run, a decode or the f0 track
/// fails.
pub fn transcribe_stem(
    transcriber: &Transcriber,
    mono: &[f64],
    rate: SampleRate,
    bpm: Bpm,
    profile: Option<Profile>,
) -> Result<Transcription, NotesError> {
    let run = transcriber.run(mono, rate)?;
    let track = transcriber.track(mono, rate)?;
    let detection = detect(&run, &track, bpm)?;
    let settings = profile.map_or(detection.settings, |profile| profile.settings(bpm));
    let voicing = profile.map_or(detection.voicing, Profile::voicing);
    let decoded = run.decode(&settings)?;
    let (kept, steps) = clean_up(&decoded, &run, &track, voicing, mono, rate);
    let notes = track.with_contours(&kept);
    Ok(Transcription {
        record: TranscriptionRecord::new(profile, &settings),
        run,
        track,
        detection,
        profile,
        voicing,
        settings,
        decoded,
        steps,
        notes,
    })
}

/// A notes step that failed.
#[derive(Debug)]
pub enum NotesError {
    /// The model run or a decode failed.
    Transcribe(TranscribeError),
    /// The f0 track failed.
    Track(TrackError),
}

impl fmt::Display for NotesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transcribe(error) => write!(f, "{error}"),
            Self::Track(error) => write!(f, "{error}"),
        }
    }
}

impl Error for NotesError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Transcribe(error) => Some(error),
            Self::Track(error) => Some(error),
        }
    }
}

impl From<TranscribeError> for NotesError {
    fn from(error: TranscribeError) -> Self {
        Self::Transcribe(error)
    }
}

impl From<TrackError> for NotesError {
    fn from(error: TrackError) -> Self {
        Self::Track(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::{Note, TrackFrame, Velocity};
    use crate::units::{Decibels, Hz, MidiPitch, Seconds};
    use basic_pitch::{Posteriorgram, Posteriorgrams};

    fn heard(onset: f64, offset: f64, pitch: f64, velocity: u8) -> Note {
        Note::new(
            Seconds::new(onset).unwrap(),
            Seconds::new(offset).unwrap(),
            MidiPitch::new(pitch).unwrap(),
        )
        .unwrap()
        .with_velocity(Velocity::new(velocity).unwrap())
    }

    #[test]
    fn errors_name_their_cause_and_keep_their_source() {
        let model = TranscribeError::Model(basic_pitch::Error::NonFiniteAudio {
            index: 3,
            value: f32::NAN,
        });
        let message = "the transcription model failed: audio sample 3 is not finite (NaN)";
        assert_eq!(model.to_string(), message);
        assert!(model.source().is_some());
        let error = NotesError::from(model);
        assert_eq!(error.to_string(), message);
        assert!(error.source().is_some());
        let track = NotesError::Track(TrackError::NotFinite(7));
        assert_eq!(track.to_string(), TrackError::NotFinite(7).to_string());
        assert!(track.source().is_some());
    }

    /// A chord part skips the octave check: its notes stay where the
    /// decoder put them, every one unchecked.
    #[test]
    fn a_chord_part_keeps_its_octaves_unchecked() {
        let run = ModelRun::from_posteriorgrams(
            Posteriorgrams::new(
                Posteriorgram::zeros(100, 88),
                Posteriorgram::zeros(100, 88),
                Posteriorgram::zeros(100, 264),
            )
            .unwrap(),
        );
        let track = PitchTrack::from_frames(
            (0..200)
                .map(|i| TrackFrame {
                    time: Seconds::new(f64::from(i) * 0.005).unwrap(),
                    f0: Some(Hz::from(MidiPitch::new(45.0).unwrap())),
                    level: Decibels::new(-3.0).unwrap(),
                })
                .collect(),
        );
        let decoded = NoteList::new(vec![heard(0.2, 0.8, 45.0, 90), heard(0.2, 0.3, 57.0, 37)]);
        let rate = SampleRate::new(22_050).unwrap();
        let mono = vec![0.5; 22_050];
        for (voicing, pitches, unchecked) in [
            (Voicing::Polyphonic, vec![45.0, 57.0], 2),
            (Voicing::Monophonic, vec![45.0], 0),
        ] {
            let (kept, steps) = clean_up(&decoded, &run, &track, voicing, &mono, rate);
            let found: Vec<f64> = kept.notes().iter().map(|n| n.pitch().get()).collect();
            assert_eq!(found, pitches, "{voicing:?}");
            assert_eq!(steps.octaves.unchecked, unchecked);
            assert_eq!((steps.decoded, steps.kept), (2, pitches.len()));
        }
    }

    #[test]
    fn the_transcriber_version_matches_the_pin() {
        let manifest = include_str!("../../Cargo.toml");
        assert!(
            manifest.contains(&format!(
                "basic-pitch-rs = {{ version = \"={TRANSCRIBER_VERSION}\""
            )),
            "Cargo.toml pins another basic-pitch-rs"
        );
    }
}
