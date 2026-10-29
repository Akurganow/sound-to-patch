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
//! in CI with no plug-in. [`plugin::PluginRenderer`] renders through a real
//! plug-in in the `sound-to-patch-host` process, with the state that
//! [`state::preset_state`] builds.

use std::error::Error;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::audio::AudioError;
use crate::cast;
use crate::notes::{MidiError, NoteList, Velocity, read_midi_bytes};
use crate::preset::format::FormatError;
use crate::synth::{Synth, SynthError};
use crate::units::{Bpm, Seconds};

pub mod check;
pub mod fake;
pub mod file;
pub mod plugin;
pub mod session;
pub mod spread;
pub mod state;
pub mod timing;
pub mod vita;

#[doc(inline)]
pub use sound_to_patch_core::render::{
    ClipEnd, ControlKind, EventKind, MAX_CONTROLLER, MAX_NOTE_END, MAX_PITCH_BEND, Material,
    NoteEvent, Oversampling, PlayError, PreRoll, ProcessMode, RECIPE_VERSION, Recipe, RenderNote,
    TailWatch, frame_of, note_events, recipe, wire,
};

/// How far a pitch from a note list may lie from a whole key. The MIDI
/// reader gives whole keys; a transcribed pitch with a bend is no key.
const KEY_TOLERANCE: f64 = 1e-9;

/// The render notes of a note list, such as the one the MIDI reader
/// gives. A note without a velocity takes `default`.
///
/// # Errors
///
/// Returns a [`RenderError`] for a pitch that is not a whole key.
pub fn notes_of_list(list: &NoteList, default: Velocity) -> Result<Vec<RenderNote>, RenderError> {
    list.notes()
        .iter()
        .map(|note| {
            let pitch = note.pitch().get();
            // A MIDI pitch lies in 0 to 127, so only a fractional pitch
            // fails here.
            let key = cast::round_to_usize(pitch)
                .and_then(|key| u8::try_from(key).ok())
                .filter(|key| (pitch - f64::from(*key)).abs() <= KEY_TOLERANCE)
                .ok_or_else(|| {
                    RenderError::Play(PlayError::Note(format!(
                        "pitch {pitch} at {} is not a whole MIDI key, so the list carries a transcribed or bent pitch; pass the notes of a MIDI file",
                        note.onset()
                    )))
                })?;
            Ok(RenderNote::new(
                key,
                note.onset(),
                note.offset(),
                note.velocity().unwrap_or(default),
            )?)
        })
        .collect()
}

/// The SHA-256 of `bytes` in lower-case hex.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// A controller change and its time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Control {
    /// When the change happens, from the start of the clip.
    pub time: Seconds,
    /// The change.
    pub kind: ControlKind,
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
    controls: Vec<Control>,
    bpm: Bpm,
    recipe: Recipe,
    midi_sha256: Option<String>,
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
            controls: Vec::new(),
            bpm,
            recipe: Recipe::current(),
            midi_sha256: None,
        })
    }

    /// Builds a job that plays the notes of a MIDI file. The file is read
    /// as `analyze --notes` reads it, so every tempo in it must match
    /// `bpm`. Notes without a velocity take the recipe's velocity. The
    /// record carries the SHA-256 of the file.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Midi`] for a file the MIDI reader refuses, and
    /// the errors of [`notes_of_list`] and [`RenderJob::new`].
    pub fn from_midi(state: PresetState, path: &Path, bpm: Bpm) -> Result<Self, RenderError> {
        let (list, bytes) = read_midi_bytes(path, bpm).map_err(RenderError::Midi)?;
        let notes = notes_of_list(&list, Recipe::current().velocity())?;
        let mut job = Self::new(state, notes, bpm)?;
        job.midi_sha256 = Some(sha256_hex(&bytes));
        Ok(job)
    }

    /// Builds a job that plays the recipe's notes for `material`.
    #[must_use]
    pub fn for_material(state: PresetState, material: Material, bpm: Bpm) -> Self {
        let recipe = Recipe::current();
        Self {
            state,
            notes: recipe.notes(material),
            controls: Vec::new(),
            bpm,
            recipe,
            midi_sha256: None,
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

    /// Adds controller changes. The clip still ends by the notes and the
    /// tail, so a change after the clip's end has no effect.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Play`] for a value that does not fit its
    /// MIDI message, as [`ControlKind::checked`] does.
    pub fn with_controls(mut self, controls: Vec<Control>) -> Result<Self, RenderError> {
        for control in &controls {
            control.kind.checked()?;
        }
        self.controls = controls;
        Ok(self)
    }

    /// Returns the controller changes.
    #[must_use]
    pub fn controls(&self) -> &[Control] {
        &self.controls
    }

    /// Returns the SHA-256 of the MIDI file the notes came from, in hex.
    #[must_use]
    pub fn midi_sha256(&self) -> Option<&str> {
        self.midi_sha256.as_deref()
    }

    /// Returns the end of the last note.
    #[must_use]
    pub fn last_note_off(&self) -> Seconds {
        self.notes
            .iter()
            .map(|note| note.offset())
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
    /// The SHA-256 of the MIDI file the notes came from, in hex. Absent for
    /// notes that came from no file, such as the recipe's notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub midi_sha256: Option<String>,
    /// The load check of the render, when it ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_check: Option<check::LoadCheck>,
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
            midi_sha256: job.midi_sha256.clone(),
            load_check: None,
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

    /// Renders the job's notes with the plug-in's own init patch: the job's
    /// state is not loaded. Only a renderer that has loaded no state yet
    /// holds the init patch, so the load check calls this first.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Host`] once a state has loaded, and the
    /// errors of [`Renderer::render`].
    fn render_init(&mut self, job: &RenderJob) -> Result<Rendered, RenderError>;

    /// Returns the state the plug-in saves now.
    ///
    /// # Errors
    ///
    /// Returns a [`RenderError`] when the plug-in fails.
    fn save_state(&mut self) -> Result<Vec<u8>, RenderError>;

    /// Returns how long the state load of the last render took, when the
    /// backend measures it. A render of the init patch loads no state.
    fn last_state_load(&self) -> Option<Seconds> {
        None
    }
}

/// A render that failed.
#[derive(Debug)]
pub enum RenderError {
    /// A note or a controller change cannot be played.
    Play(PlayError),
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
    TimedOut {
        /// How long the renderer waited.
        after: Seconds,
        /// The host's last output.
        output: String,
    },
    /// The plug-in gave audio that is not valid.
    Audio(AudioError),
    /// The preset file is not a valid preset.
    Preset(FormatError),
    /// The synth has no renderer yet.
    Synth(SynthError),
    /// The MIDI file cannot be read.
    Midi(MidiError),
    /// The plug-in host cannot start, cannot load the plug-in or refused a
    /// request.
    Host {
        /// Why.
        cause: String,
        /// How to fix it.
        fix: String,
    },
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Play(source) => write!(f, "{source}"),
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
            Self::TimedOut { after, output } => write!(
                f,
                "the plug-in host stopped answering after {after}; its last output: {output}. Render the preset again; if it stops again, load the preset in the synth and report it with this message"
            ),
            Self::Audio(source) => write!(
                f,
                "the plug-in gave audio that is not valid: {source}. Report it with the preset"
            ),
            Self::Preset(source) => write!(f, "{source}"),
            Self::Synth(source) => write!(f, "{source}"),
            Self::Midi(source) => write!(f, "{source}"),
            Self::Host { cause, fix } => {
                write!(f, "cannot render through the plug-in host: {cause}. {fix}")
            }
        }
    }
}

impl Error for RenderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Play(source) => source.source(),
            Self::Audio(source) => Some(source),
            Self::Preset(source) => Some(source),
            Self::Synth(source) => Some(source),
            Self::Midi(source) => Some(source),
            _ => None,
        }
    }
}

impl From<PlayError> for RenderError {
    fn from(source: PlayError) -> Self {
        Self::Play(source)
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

    #[test]
    fn a_play_error_prints_the_same_text_as_a_render_error() {
        for error in [
            PlayError::Note("key 128".to_owned()),
            PlayError::Control("bend 20000".to_owned()),
        ] {
            let text = error.to_string();
            let render = RenderError::from(error);
            assert_eq!(render.to_string(), text);
            assert!(Error::source(&render).is_none());
        }
    }

    fn s(value: f64) -> Seconds {
        Seconds::new(value).unwrap()
    }

    fn v(value: u8) -> Velocity {
        Velocity::new(value).unwrap()
    }

    #[test]
    fn a_controller_value_must_fit_its_midi_message() {
        let job = || {
            RenderJob::new(
                PresetState::new(Synth::Vital, Vec::new()),
                vec![RenderNote::new(60, s(0.0), s(1.0), v(100)).unwrap()],
                Bpm::new(120.0).unwrap(),
            )
            .unwrap()
        };
        let at = |kind| Control { time: s(0.5), kind };
        let error = job()
            .with_controls(vec![at(ControlKind::PitchBend(MAX_PITCH_BEND + 1))])
            .unwrap_err();
        assert!(error.to_string().contains("pass 0 to 16383"), "{error}");
        let error = job()
            .with_controls(vec![at(ControlKind::ModWheel(128))])
            .unwrap_err();
        assert!(error.to_string().contains("pass 0 to 127"), "{error}");
        let edges = vec![
            at(ControlKind::PitchBend(MAX_PITCH_BEND)),
            at(ControlKind::ModWheel(MAX_CONTROLLER)),
        ];
        assert_eq!(job().with_controls(edges).unwrap().controls().len(), 2);
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
    fn a_note_list_converts_with_its_velocity_or_the_default() {
        let pitch = |p: f64| MidiPitch::new(p).unwrap();
        let list = NoteList::new(vec![
            Note::new(s(0.0), s(1.0), pitch(36.0)).unwrap(),
            Note::new(s(1.0), s(2.0), pitch(48.0))
                .unwrap()
                .with_velocity(v(64)),
        ]);
        let notes = notes_of_list(&list, v(100)).unwrap();
        let keys: Vec<(u8, u8)> = notes
            .iter()
            .map(|n| (n.key(), n.velocity().get()))
            .collect();
        assert_eq!(keys, [(36, 100), (48, 64)]);
        let bent = NoteList::new(vec![Note::new(s(0.0), s(1.0), pitch(36.4)).unwrap()]);
        assert!(notes_of_list(&bent, v(100)).is_err());
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
