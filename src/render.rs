//! Rendering a preset and its notes to audio through a synth.
//!
//! A [`Renderer`] plays a [`RenderJob`]: the plug-in state of a preset, the
//! notes and the [`Recipe`]. It returns the audio and a [`RenderRecord`]
//! that names everything the audio depends on, so a later comparison can
//! cite it. Calibration, compare, refine, the bench and the dataset all
//! render through this trait and add no recipe field of their own.
//!
//! Renders of one preset are not bit-identical: a synth starts its unison
//! voices at random phases. Compare two renders within the repeat-render
//! spread, never sample by sample.
//!
//! [`fake::FakeRenderer`] is a seeded additive synth, so the render code runs
//! in CI with no plug-in.

use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::audio::{AudioError, SampleRate};
use crate::cast;
use crate::notes::{NoteList, Velocity};
use crate::synth::Synth;
use crate::units::{Bpm, Seconds};

pub mod fake;
pub mod recipe;

pub use recipe::{
    ClipEnd, Material, Oversampling, PreRoll, ProcessMode, RECIPE_VERSION, Recipe, TailWatch,
};

/// The highest MIDI key.
const MAX_KEY: u8 = 127;
/// How far a pitch from a note list may lie from a whole key. The MIDI
/// reader gives whole keys; a transcribed pitch with a bend is no key.
const KEY_TOLERANCE: f64 = 1e-9;
/// The latest time a note may end: 10 minutes. A clip holds every sample
/// in memory, and a longer part than a stem takes splits into several
/// renders.
pub const MAX_NOTE_END: Seconds = Seconds::from_const(600.0);

/// One note to play: a MIDI key, its start and end in the clip and its
/// velocity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderNote {
    key: u8,
    onset: Seconds,
    offset: Seconds,
    velocity: Velocity,
}

impl RenderNote {
    /// Checks the key, that the note ends after it starts and that it ends
    /// by [`MAX_NOTE_END`].
    ///
    /// # Errors
    ///
    /// Returns a [`RenderError`] for a key above 127, an offset that is not
    /// after the onset or an offset after [`MAX_NOTE_END`].
    pub fn new(
        key: u8,
        onset: Seconds,
        offset: Seconds,
        velocity: Velocity,
    ) -> Result<Self, RenderError> {
        if key > MAX_KEY {
            return Err(RenderError::Note(format!(
                "key {key} is above {MAX_KEY}; pass keys 0 to {MAX_KEY}"
            )));
        }
        if offset <= onset {
            return Err(RenderError::Note(format!(
                "key {key} ends at {offset}, not after its start at {onset}; end each note after it starts"
            )));
        }
        if offset > MAX_NOTE_END {
            return Err(RenderError::Note(format!(
                "key {key} ends at {offset}, after the longest clip of {MAX_NOTE_END}; split the part into several renders"
            )));
        }
        Ok(Self {
            key,
            onset,
            offset,
            velocity,
        })
    }

    /// Converts a note list, such as the one the MIDI reader gives. A note
    /// without a velocity takes `default`.
    ///
    /// # Errors
    ///
    /// Returns a [`RenderError`] for a pitch that is not a whole key.
    pub fn from_list(list: &NoteList, default: Velocity) -> Result<Vec<Self>, RenderError> {
        list.notes()
            .iter()
            .map(|note| {
                let pitch = note.pitch().get();
                if (pitch - pitch.round()).abs() > KEY_TOLERANCE {
                    return Err(RenderError::Note(format!(
                        "pitch {pitch} at {} is not a whole MIDI key, so the list carries a transcribed or bent pitch; pass the notes of a MIDI file",
                        note.onset()
                    )));
                }
                let key = cast::round_to_usize(pitch)
                    .and_then(|key| u8::try_from(key).ok())
                    .ok_or_else(|| {
                        RenderError::Note(format!(
                            "pitch {pitch} at {} is outside the MIDI keys; pass keys 0 to {MAX_KEY}",
                            note.onset()
                        ))
                    })?;
                Self::new(
                    key,
                    note.onset(),
                    note.offset(),
                    note.velocity().unwrap_or(default),
                )
            })
            .collect()
    }

    /// Returns the MIDI key.
    #[must_use]
    pub fn key(self) -> u8 {
        self.key
    }

    /// Returns the start in the clip.
    #[must_use]
    pub fn onset(self) -> Seconds {
        self.onset
    }

    /// Returns the end in the clip.
    #[must_use]
    pub fn offset(self) -> Seconds {
        self.offset
    }

    /// Returns the velocity.
    #[must_use]
    pub fn velocity(self) -> Velocity {
        self.velocity
    }
}

/// Whether a note event starts or ends a note.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventKind {
    /// The note ends. It sorts first, so a key that ends and starts on one
    /// frame plays again.
    Off,
    /// The note starts.
    On,
}

/// A note-on or note-off at a sample frame of the clip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoteEvent {
    /// The frame, `round(t × rate)`.
    pub frame: usize,
    /// Start or end.
    pub kind: EventKind,
    /// The MIDI key.
    pub key: u8,
    /// The velocity of the note.
    pub velocity: Velocity,
}

/// Turns notes into events sorted by frame, a note-off before a note-on on
/// the same frame.
///
/// A frame is the note time times the rate, rounded to the nearest sample.
/// A note shorter than half a frame still lasts one frame, so its note-off
/// never comes before its own note-on.
#[must_use]
pub fn note_events(notes: &[RenderNote], rate: SampleRate) -> Vec<NoteEvent> {
    let mut events: Vec<NoteEvent> = notes
        .iter()
        .flat_map(|note| {
            let on = frame_of(note.onset, rate);
            let off = frame_of(note.offset, rate).max(on + 1);
            [(on, EventKind::On), (off, EventKind::Off)].map(|(frame, kind)| NoteEvent {
                frame,
                kind,
                key: note.key,
                velocity: note.velocity,
            })
        })
        .collect();
    events.sort_by_key(|event| (event.frame, event.kind, event.key));
    events
}

/// The frame of a time: the time times the rate, rounded to the nearest
/// sample. A time too large for a frame index gives `usize::MAX`; note
/// times are at most [`MAX_NOTE_END`], so the fallback does not act on them.
pub(crate) fn frame_of(time: Seconds, rate: SampleRate) -> usize {
    cast::round_to_usize(time.get() * f64::from(rate.get())).unwrap_or(usize::MAX)
}

/// The plug-in state of a preset: the bytes a synth loads as its state, and
/// the synth they belong to. Each synth's render state adapter builds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresetState {
    synth: Synth,
    bytes: Vec<u8>,
}

impl PresetState {
    /// Wraps the state bytes of `synth`.
    #[must_use]
    pub fn new(synth: Synth, bytes: Vec<u8>) -> Self {
        Self { synth, bytes }
    }

    /// Returns the synth.
    #[must_use]
    pub fn synth(&self) -> Synth {
        self.synth
    }

    /// Returns the state bytes.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// One render: the preset state, the notes, the tempo and the recipe.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderJob {
    state: PresetState,
    notes: Vec<RenderNote>,
    bpm: Bpm,
    recipe: Recipe,
}

impl RenderJob {
    /// Builds a job with the current recipe.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::NoNotes`] for an empty note list.
    pub fn new(state: PresetState, notes: Vec<RenderNote>, bpm: Bpm) -> Result<Self, RenderError> {
        if notes.is_empty() {
            return Err(RenderError::NoNotes);
        }
        Ok(Self {
            state,
            notes,
            bpm,
            recipe: Recipe::current(),
        })
    }

    /// Builds a job that plays the recipe's notes for `material`.
    #[must_use]
    pub fn for_material(state: PresetState, material: Material, bpm: Bpm) -> Self {
        let recipe = Recipe::current();
        Self {
            state,
            notes: recipe.notes(material),
            bpm,
            recipe,
        }
    }

    /// Returns the preset state.
    #[must_use]
    pub fn state(&self) -> &PresetState {
        &self.state
    }

    /// Returns the notes.
    #[must_use]
    pub fn notes(&self) -> &[RenderNote] {
        &self.notes
    }

    /// Returns the tempo, which synced LFOs and delays follow.
    #[must_use]
    pub fn bpm(&self) -> Bpm {
        self.bpm
    }

    /// Returns the recipe.
    #[must_use]
    pub fn recipe(&self) -> &Recipe {
        &self.recipe
    }

    /// Returns the end of the last note.
    #[must_use]
    pub fn last_note_off(&self) -> Seconds {
        self.notes
            .iter()
            .map(|note| note.offset)
            .fold(Seconds::from_const(0.0), |a, b| if b > a { b } else { a })
    }
}

/// What renders: the synth and its build, the host and the architecture of
/// the process that loads the plug-in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostIdentity {
    /// The synth.
    pub synth: Synth,
    /// The synth's build, such as `1.0.7`.
    pub build: String,
    /// The host and its version, such as `vst3-host 0.10.0`.
    pub host: String,
    /// The architecture of the host process, such as `x86_64`.
    pub arch: String,
}

/// Everything a render depends on, so a consumer can cite it and analyze
/// the audio with the same notes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RenderRecord {
    /// What rendered.
    pub identity: HostIdentity,
    /// The recipe version.
    pub recipe_version: u32,
    /// The sample rate in samples per second.
    pub sample_rate: u32,
    /// The block size in samples.
    pub block_size: u32,
    /// How the host told the plug-in it processes.
    pub process_mode: ProcessMode,
    /// The settle time rendered in silence after the state load.
    pub settle: Seconds,
    /// The oversampling of a Vital render.
    pub oversampling: Oversampling,
    /// The tempo.
    pub bpm: Bpm,
    /// The end of the last note.
    pub last_note_off: Seconds,
    /// The clip length.
    pub length: Seconds,
    /// Whether the tail reached its cap before it went quiet.
    pub tail_capped: bool,
}

impl RenderRecord {
    /// Fills the fields that come from the job and the recipe.
    #[must_use]
    pub fn new(identity: HostIdentity, job: &RenderJob, end: ClipEnd) -> Self {
        let recipe = job.recipe();
        let rate = recipe.sample_rate();
        Self {
            identity,
            recipe_version: recipe.version(),
            sample_rate: rate.get(),
            block_size: recipe.block_size(),
            process_mode: recipe.process_mode(),
            settle: recipe.settle(),
            oversampling: recipe.vital_oversampling(),
            bpm: job.bpm(),
            last_note_off: job.last_note_off(),
            length: rate.seconds(end.frames),
            tail_capped: end.capped,
        }
    }
}

/// The audio of a render and its record.
#[derive(Clone, Debug, PartialEq)]
pub struct Rendered {
    /// Stereo audio at the recipe's rate.
    pub audio: crate::audio::Audio,
    /// What the audio depends on.
    pub record: RenderRecord,
}

/// Plays a [`RenderJob`] through one synth and returns the audio.
///
/// A backend loads the state, plays the recipe's pre-roll (settle, warm-up
/// note, settle) and throws it away, then renders the clip. The clip ends
/// once the [`TailWatch`] says the tail is over.
///
/// Events go to the synth in the order of [`note_events`]. A note-on of a
/// key that already sounds is sent as a second note-on, and each note-off
/// of a key ends the oldest sounding note of that key, first in, first out,
/// as the MIDI reader pairs them. How the synth voices a repeated key is
/// the synth's own rule.
pub trait Renderer {
    /// Returns what renders.
    fn identity(&self) -> HostIdentity;

    /// Renders one job.
    ///
    /// # Errors
    ///
    /// Returns a [`RenderError`] when the state does not load, the plug-in
    /// fails or the audio is not valid.
    fn render(&mut self, job: &RenderJob) -> Result<Rendered, RenderError>;
}

/// A render that failed.
#[derive(Debug)]
pub enum RenderError {
    /// A note cannot be played.
    Note(String),
    /// The job holds no note.
    NoNotes,
    /// The synth did not take the state.
    State(String),
    /// The state belongs to another synth than the renderer's.
    WrongSynth {
        /// The synth of the state.
        state: Synth,
        /// The synth of the renderer.
        renderer: Synth,
    },
    /// The process that hosts the plug-in stopped.
    Crashed(String),
    /// The process that hosts the plug-in stopped answering.
    TimedOut(Seconds),
    /// The plug-in gave audio that is not valid.
    Audio(AudioError),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Note(cause) => write!(f, "cannot play a note: {cause}"),
            Self::NoNotes => write!(
                f,
                "cannot render: the job has no notes. Pass a MIDI file with at least one note"
            ),
            Self::State(cause) => write!(
                f,
                "the synth did not load the preset: {cause}. Check that the preset loads in the synth"
            ),
            Self::WrongSynth { state, renderer } => write!(
                f,
                "the preset is for {state}, but the renderer runs {renderer}. Render it with --synth {state}"
            ),
            Self::Crashed(cause) => write!(
                f,
                "the plug-in host stopped: {cause}. Render the preset again; if it stops again, load the preset in the synth and report it with this message"
            ),
            Self::TimedOut(after) => write!(
                f,
                "the plug-in host stopped answering after {after}. Render the preset again; if it stops again, load the preset in the synth and report it with this message"
            ),
            Self::Audio(source) => write!(
                f,
                "the plug-in gave audio that is not valid: {source}. Report it with the preset"
            ),
        }
    }
}

impl Error for RenderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Audio(source) => Some(source),
            _ => None,
        }
    }
}

impl From<AudioError> for RenderError {
    fn from(source: AudioError) -> Self {
        Self::Audio(source)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::Note;
    use crate::units::MidiPitch;

    fn s(value: f64) -> Seconds {
        Seconds::new(value).unwrap()
    }

    fn v(value: u8) -> Velocity {
        Velocity::new(value).unwrap()
    }

    #[test]
    fn a_note_must_end_after_it_starts_and_fit_the_key_range() {
        assert!(RenderNote::new(60, s(1.0), s(1.0), v(100)).is_err());
        assert!(RenderNote::new(128, s(0.0), s(1.0), v(100)).is_err());
        assert!(RenderNote::new(127, s(0.0), s(1.0), v(100)).is_ok());
    }

    #[test]
    fn a_note_must_end_by_the_longest_clip() {
        assert_eq!(Seconds::new(MAX_NOTE_END.get()).ok(), Some(MAX_NOTE_END));
        assert!(RenderNote::new(60, s(0.0), MAX_NOTE_END, v(100)).is_ok());
        let error = RenderNote::new(60, s(0.0), s(600.001), v(100)).unwrap_err();
        assert!(
            error
                .to_string()
                .ends_with("split the part into several renders"),
            "{error}"
        );
        assert!(RenderNote::new(60, s(0.0), s(1e300), v(100)).is_err());
    }

    fn frames(notes: &[RenderNote]) -> Vec<(usize, EventKind)> {
        note_events(notes, SampleRate::new(44_100).unwrap())
            .iter()
            .map(|e| (e.frame, e.kind))
            .collect()
    }

    #[test]
    fn a_note_shorter_than_half_a_frame_still_lasts_one_frame() {
        let note = RenderNote::new(60, s(0.0), s(0.000_01), v(100)).unwrap();
        assert_eq!(frames(&[note]), [(0, EventKind::On), (1, EventKind::Off)]);
    }

    #[test]
    fn a_chain_of_one_key_keeps_every_off_after_its_on() {
        let notes = [
            RenderNote::new(60, s(0.0), s(1.0), v(100)).unwrap(),
            RenderNote::new(60, s(1.0), s(1.000_01), v(100)).unwrap(),
            RenderNote::new(60, s(1.000_01), s(2.0), v(100)).unwrap(),
        ];
        // The second note rounds to frame 44 100 at both ends and lasts one
        // frame; the third starts on frame 44 100 too.
        assert_eq!(
            frames(&notes),
            [
                (0, EventKind::On),
                (44_100, EventKind::Off),
                (44_100, EventKind::On),
                (44_100, EventKind::On),
                (44_101, EventKind::Off),
                (88_200, EventKind::Off),
            ]
        );
    }

    #[test]
    fn the_wrong_synth_error_names_the_flag_to_pass() {
        let error = RenderError::WrongSynth {
            state: Synth::Vital,
            renderer: Synth::Serum2,
        };
        assert!(error.to_string().ends_with("Render it with --synth vital"));
    }

    #[test]
    fn events_round_to_the_nearest_frame_and_put_offs_first() {
        let rate = SampleRate::new(44_100).unwrap();
        let notes = [
            RenderNote::new(60, s(0.5), s(1.0), v(100)).unwrap(),
            RenderNote::new(60, s(1.0), s(1.25), v(90)).unwrap(),
        ];
        let events = note_events(&notes, rate);
        let frames: Vec<(usize, EventKind)> = events.iter().map(|e| (e.frame, e.kind)).collect();
        assert_eq!(
            frames,
            [
                (22_050, EventKind::On),
                (44_100, EventKind::Off),
                (44_100, EventKind::On),
                (55_125, EventKind::Off),
            ]
        );
        // 1/3 s is 14 700 frames exactly; 0.1 ms is 4.41 frames and rounds to 4.
        let odd = [RenderNote::new(36, s(0.0001), s(1.0 / 3.0), v(1)).unwrap()];
        let frames: Vec<usize> = note_events(&odd, rate).iter().map(|e| e.frame).collect();
        assert_eq!(frames, [4, 14_700]);
    }

    #[test]
    fn a_note_list_converts_with_its_velocity_or_the_default() {
        let pitch = |p: f64| MidiPitch::new(p).unwrap();
        let list = NoteList::new(vec![
            Note::new(s(0.0), s(1.0), pitch(36.0)).unwrap(),
            Note::new(s(1.0), s(2.0), pitch(48.0))
                .unwrap()
                .with_velocity(v(64)),
        ]);
        let notes = RenderNote::from_list(&list, v(100)).unwrap();
        let keys: Vec<(u8, u8)> = notes
            .iter()
            .map(|n| (n.key(), n.velocity().get()))
            .collect();
        assert_eq!(keys, [(36, 100), (48, 64)]);
        let bent = NoteList::new(vec![Note::new(s(0.0), s(1.0), pitch(36.4)).unwrap()]);
        assert!(RenderNote::from_list(&bent, v(100)).is_err());
    }

    #[test]
    fn a_job_needs_a_note_and_knows_its_last_note_off() {
        let state = PresetState::new(Synth::Vital, Vec::new());
        let bpm = Bpm::new(120.0).unwrap();
        assert!(matches!(
            RenderJob::new(state.clone(), Vec::new(), bpm),
            Err(RenderError::NoNotes)
        ));
        let job = RenderJob::for_material(state, Material::Pad, bpm);
        assert_eq!(job.notes().len(), 3);
        assert_eq!(job.last_note_off(), s(3.0));
    }
}
