//! The outputs of a stem: `layers.json` and `spectrogram.png` from
//! `analyze`, and `notes.mid` from `notes`.
//!
//! [`analyze`] decodes the stem, refines its tempo, reads the note list if
//! one is given or else runs the notes step, reads the pitch layer, runs
//! the stem estimators and the descriptors, and writes both files into the
//! output folder.
//! [`notes`] refines the tempo, transcribes the stem and writes its notes
//! into the same folder. The schema of `layers.json` is committed as
//! `schemas/layers.schema.json`.

use std::error::Error;
use std::fmt;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use crate::audio::{self, BeatGrid, DecodeError, SampleRate, SkippedPacket, Stem, Tempo};
use crate::confidence::ConfidenceError;
use crate::dsp::{StftParams, describe};
use crate::estimate::envelope::{EnvelopeError, EnvelopeInput, EnvelopeLayer};
use crate::estimate::pitch::{PitchError, PitchInput, PitchLayer};
use crate::notes::{
    DEFAULT_VELOCITY, MidiError, MidiWriteError, NoteList, NotesError, PitchTrack, Profile,
    QUIET_GATE, Transcriber, Transcription, Velocities, Voicing, midi_track_name, read_midi_near,
    transcribe_stem, voicing_of_notes, write_named_midi,
};
use crate::synth::Synth;
use crate::units::{Bpm, Seconds};

mod layers;
pub mod spectrogram;

pub use layers::{SCHEMA, layers_json};
pub use spectrogram::Spectrogram;

/// The report file name.
pub const LAYERS_FILE: &str = "layers.json";
/// The spectrogram file name.
pub const SPECTROGRAM_FILE: &str = "spectrogram.png";
/// The notes file name.
pub const NOTES_FILE: &str = "notes.mid";

/// The output folder of a stem: `out`, or `<stem folder>/<stem file stem>/`.
fn out_dir(stem: &Path, out: Option<&Path>) -> PathBuf {
    out.map_or_else(
        || {
            let name = stem.file_stem().unwrap_or(stem.as_os_str());
            stem.with_file_name(name)
        },
        Path::to_path_buf,
    )
}

/// The file name of a stem, as a report records it.
fn source_of(stem: &Path) -> String {
    stem.file_name()
        .unwrap_or(stem.as_os_str())
        .to_string_lossy()
        .into_owned()
}

/// What [`analyze`] reads and where it writes.
#[derive(Clone, Copy, Debug)]
pub struct AnalyzeRequest<'a> {
    /// The audio stem.
    pub stem: &'a Path,
    /// The tempo of the stem, refined from the stem unless `bpm_exact`.
    pub bpm: Bpm,
    /// Use `bpm` as it is.
    pub bpm_exact: bool,
    /// The target synth, written into the report.
    pub synth: Synth,
    /// A MIDI file with the stem's notes. Without it the notes step runs.
    pub notes: Option<&'a Path>,
    /// The profile that overrides detection in the notes step, if any. The
    /// notes step does not run when `notes` gives the notes, so the profile
    /// then has no effect; the command line rejects the two together.
    pub profile: Option<Profile>,
    /// The output folder; by default `<stem folder>/<stem file stem>/`.
    pub out: Option<&'a Path>,
    /// Replace a report of another stem in the output folder.
    pub force: bool,
}

impl AnalyzeRequest<'_> {
    /// Returns the output folder.
    #[must_use]
    pub fn out_dir(&self) -> PathBuf {
        out_dir(self.stem, self.out)
    }

    /// Returns the file name of the stem, as the report records it.
    #[must_use]
    pub fn source(&self) -> String {
        source_of(self.stem)
    }
}

/// What [`analyze`] found and wrote.
#[derive(Clone, Debug)]
pub struct Analysis {
    /// The path of `layers.json`.
    pub layers: PathBuf,
    /// The path of `spectrogram.png`.
    pub spectrogram: PathBuf,
    /// The given, refined and used tempo.
    pub tempo: Tempo,
    /// The notes step, `None` when `--notes` gave the notes.
    pub transcription: Option<Transcription>,
    /// The pitch layer: glides, pitch drops and vibrato.
    pub pitch: PitchLayer,
    /// The envelope layer: the amplitude envelope and the pump.
    pub envelope: EnvelopeLayer,
    /// The packets that did not decode and became silence.
    pub skipped: Vec<SkippedPacket>,
}

impl Analysis {
    /// Returns the lines that tell what each step found, in order: the
    /// tempo, the notes step when it ran, the pitch layer, then the
    /// envelope layer.
    #[must_use]
    pub fn summary(&self) -> Vec<String> {
        std::iter::once(self.tempo.summary())
            .chain(self.transcription.iter().flat_map(transcription_summary))
            .chain(self.pitch.summary())
            .chain(self.envelope.summary())
            .collect()
    }
}

/// The f0 track for given notes: the pitch layer reads it, and skips a
/// list that is empty or plays chords without it.
fn given_track(
    transcriber: &Transcriber,
    mono: &[f64],
    rate: audio::SampleRate,
    notes: &NoteList,
) -> Result<PitchTrack, AnalyzeError> {
    if notes.is_empty() || voicing_of_notes(notes) == Voicing::Polyphonic {
        return Ok(PitchTrack::default());
    }
    transcriber
        .track(mono, rate)
        .map_err(|error| AnalyzeError::Transcription(NotesError::Track(error)))
}

/// Analyses a stem and writes `layers.json` and `spectrogram.png` into the
/// output folder, creating it.
///
/// The tempo is refined from the stem unless the request says `bpm_exact`.
/// Without a note list the stem runs through [`transcribe_stem`] once,
/// with `transcriber`; with one, the model never runs, but the f0 track
/// of `transcriber` still reads the stem for the pitch layer.
///
/// Both files are replaced; nothing else in the folder changes. Each file
/// is written to a temporary sibling and renamed into place, and
/// `layers.json` comes last: an old report is removed before the new
/// spectrogram lands, so a `layers.json` that exists always belongs to the
/// spectrogram next to it, and neither is ever cut short.
///
/// # Errors
///
/// Returns an [`AnalyzeError`] when the output folder is a file or holds
/// the report of another stem (without `force`), when the note list or the
/// stem cannot be read, when the notes step fails, or when an output file
/// cannot be written.
pub fn analyze(
    request: &AnalyzeRequest<'_>,
    transcriber: &Transcriber,
) -> Result<Analysis, AnalyzeError> {
    let dir = request.out_dir();
    let source = request.source();
    check_out_dir(&dir, &source, request.force, Guarded::Report)?;
    let decoded = audio::decode(request.stem).map_err(AnalyzeError::Decode)?;
    let skipped = decoded.skipped().to_vec();
    let stem_audio = decoded.into_audio();
    let mono = stem_audio.mono();
    let tempo = tempo_of(
        &mono,
        stem_audio.sample_rate(),
        request.bpm,
        request.bpm_exact,
    )?;
    let (notes, transcription, track) = if let Some(path) = request.notes {
        let notes =
            read_midi_near(path, tempo.given(), tempo.trusted()).map_err(AnalyzeError::Notes)?;
        let track = given_track(transcriber, &mono, stem_audio.sample_rate(), &notes)?;
        (Some(notes), None, Some(track))
    } else {
        let transcription = transcribe_stem(
            transcriber,
            &mono,
            stem_audio.sample_rate(),
            tempo.used(),
            request.profile,
        )
        .map_err(AnalyzeError::Transcription)?;
        // A stem without notes keeps the frame rule of a stem without a list.
        let notes = (!transcription.notes.is_empty()).then(|| transcription.notes.clone());
        (notes, Some(transcription), None)
    };
    let empty = NoteList::default();
    let note_list = notes.as_ref().unwrap_or(&empty);
    let grid = BeatGrid::new(tempo.used(), Seconds::clamped(0.0));
    let (pitch, envelope) = read_layers(
        &mono,
        stem_audio.sample_rate(),
        note_list,
        track
            .as_ref()
            .or(transcription.as_ref().map(|t| &t.track))
            .unwrap_or(&PitchTrack::default()),
        transcription
            .as_ref()
            .map_or_else(|| voicing_of_notes(note_list), |t| t.voicing),
        grid,
    )?;
    // The stem and descriptor steps read the channels, not the mono mix.
    drop(mono);
    let stem = Stem::new(stem_audio, grid, notes.as_ref())?;
    let params = StftParams::fixed(stem.frame_len());
    let descriptors = describe(
        stem.mid(),
        stem.sample_rate(),
        params,
        stem.median_f0(),
        stem.hard_limited(),
    )?;
    let record = transcription.as_ref().map(|t| &t.record);
    let json = layers_json(
        &stem,
        &descriptors,
        request.synth,
        &source,
        &tempo,
        record,
        (&pitch, &envelope),
    )?;
    let png = Spectrogram::of(stem.mid(), stem.sample_rate()).to_png()?;

    std::fs::create_dir_all(&dir).map_err(|source| AnalyzeError::Write {
        path: dir.clone(),
        source,
    })?;
    let layers = dir.join(LAYERS_FILE);
    let spectrogram = dir.join(SPECTROGRAM_FILE);
    let layers_tmp = stage(&dir, LAYERS_FILE, json.as_bytes())?;
    let spectrogram_tmp = stage(&dir, SPECTROGRAM_FILE, &png)?;
    match std::fs::remove_file(&layers) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => {
            return Err(AnalyzeError::Write {
                path: layers,
                source: error,
            });
        }
        _ => {}
    }
    for (from, to) in [(&spectrogram_tmp, &spectrogram), (&layers_tmp, &layers)] {
        std::fs::rename(from, to).map_err(|source| AnalyzeError::Write {
            path: to.clone(),
            source,
        })?;
    }
    Ok(Analysis {
        layers,
        spectrogram,
        tempo,
        transcription,
        pitch,
        envelope,
        skipped,
    })
}

/// Reads the pitch and envelope layers of the mono mix.
fn read_layers(
    mono: &[f64],
    rate: SampleRate,
    notes: &NoteList,
    track: &PitchTrack,
    voicing: Voicing,
    grid: BeatGrid,
) -> Result<(PitchLayer, EnvelopeLayer), AnalyzeError> {
    let pitch = PitchLayer::of(&PitchInput {
        mono,
        rate,
        track,
        notes,
        voicing,
    })
    .map_err(AnalyzeError::Pitch)?;
    let envelope = EnvelopeLayer::of(&EnvelopeInput {
        mono,
        rate,
        notes,
        grid,
    })
    .map_err(AnalyzeError::Envelope)?;
    Ok((pitch, envelope))
}

/// The tempo of a stem: `bpm` as given when `exact`, else refined from
/// the mono mix.
fn tempo_of(mono: &[f64], rate: SampleRate, bpm: Bpm, exact: bool) -> Result<Tempo, AnalyzeError> {
    Ok(if exact {
        Tempo::exact(bpm)?
    } else {
        Tempo::refine(mono, rate, bpm)?
    })
}

/// What [`notes`] reads and where it writes.
#[derive(Clone, Copy, Debug)]
pub struct NotesRequest<'a> {
    /// The audio stem.
    pub stem: &'a Path,
    /// The tempo of the stem, refined from the stem unless `bpm_exact`.
    pub bpm: Bpm,
    /// Use `bpm` as it is.
    pub bpm_exact: bool,
    /// The profile that overrides detection, if any.
    pub profile: Option<Profile>,
    /// The output folder; by default `<stem folder>/<stem file stem>/`.
    pub out: Option<&'a Path>,
    /// Write next to a report of another stem.
    pub force: bool,
}

/// What [`notes`] found and wrote.
#[derive(Clone, Debug)]
pub struct NotesReport {
    /// The path of `notes.mid`.
    pub path: PathBuf,
    /// The given, refined and used tempo.
    pub tempo: Tempo,
    /// The notes step: the model run, the track, the settings and the
    /// counts of each cleanup step.
    pub transcription: Transcription,
    /// The packets that did not decode and became silence.
    pub skipped: Vec<SkippedPacket>,
}

impl NotesReport {
    /// Returns the lines that tell what each step found and kept, in
    /// order: the tempo, then the notes step.
    #[must_use]
    pub fn summary(&self) -> Vec<String> {
        std::iter::once(self.tempo.summary())
            .chain(transcription_summary(&self.transcription))
            .collect()
    }
}

/// The lines of the notes step: what detection found, the settings and the
/// count after each cleanup step.
fn transcription_summary(t: &Transcription) -> Vec<String> {
    let detection = &t.detection;
    let range = |range: crate::notes::PitchRange| {
        format!("MIDI {} to {}", range.lowest().get(), range.highest().get())
    };
    let settings = &t.settings;
    let steps = &t.steps;
    vec![
        format!(
            "detected range: {}, from the {}",
            range(detection.settings.range),
            detection.source.name()
        ),
        format!(
            "detected voicing: {}, chord share {:.2}",
            detection.voicing.name(),
            detection.chord_share
        ),
        format!("register window: {} dB", detection.window.get()),
        format!(
            "cleanup voicing: {}, {}",
            t.voicing.name(),
            t.profile.map_or_else(
                || "as detected".to_owned(),
                |p| format!("from --profile {p}")
            )
        ),
        format!(
            "settings: {}, {}, minimum note {} frames ({:.1} ms), melodia {}",
            t.profile
                .map_or_else(|| "detected".to_owned(), |p| format!("profile {p}")),
            range(settings.range),
            settings.min_note_frames,
            settings.note_params().minimum_note_length_ms,
            if settings.melodia { "on" } else { "off" }
        ),
        format!("decoded: {} notes", steps.decoded),
        format!(
            "octave check: {} notes, {} moved down an octave, {} flagged, {} unchecked",
            steps.decoded, steps.octaves.moved, steps.octaves.flagged, steps.octaves.unchecked
        ),
        format!("ghost merge: {} notes", steps.merged),
        format!("attack split: {} notes", steps.split),
        format!("split join: {} notes", steps.joined),
        format!(
            "quiet-note guard: {} notes, {} dropped below {} dB",
            steps.kept,
            steps.quiet,
            QUIET_GATE.get()
        ),
    ]
}

/// Transcribes a stem and writes `notes.mid` into the output folder,
/// creating it.
///
/// The tempo is refined from the stem unless the request says
/// `bpm_exact`. The stem runs through [`transcribe_stem`] at that tempo
/// with the detected settings, or those of the profile, and the file
/// carries that tempo. The file is written to a temporary sibling
/// and renamed into place, and nothing else in the folder changes.
///
/// # Errors
///
/// Returns an [`AnalyzeError`] when the output folder is a file or holds
/// the report of another stem (without `force`), when the stem cannot be
/// decoded or transcribed, when it gives no note, or when the file cannot
/// be written.
pub fn notes(
    request: &NotesRequest<'_>,
    transcriber: &Transcriber,
) -> Result<NotesReport, AnalyzeError> {
    let dir = out_dir(request.stem, request.out);
    let source = source_of(request.stem);
    check_out_dir(&dir, &source, request.force, Guarded::ReportBeside)?;
    check_notes_file(&dir, &source, request.force)?;
    let decoded = audio::decode(request.stem).map_err(AnalyzeError::Decode)?;
    let skipped = decoded.skipped().to_vec();
    let stem_audio = decoded.into_audio();
    let mono = stem_audio.mono();
    let tempo = tempo_of(
        &mono,
        stem_audio.sample_rate(),
        request.bpm,
        request.bpm_exact,
    )?;
    let transcription = transcribe_stem(
        transcriber,
        &mono,
        stem_audio.sample_rate(),
        tempo.used(),
        request.profile,
    )
    .map_err(AnalyzeError::Transcription)?;
    if transcription.notes.is_empty() {
        return Err(AnalyzeError::NoNotes { source });
    }
    let bytes = write_named_midi(
        &transcription.notes,
        tempo.used(),
        Velocities::Constant(DEFAULT_VELOCITY),
        &source,
    )
    .map_err(AnalyzeError::Midi)?;
    std::fs::create_dir_all(&dir).map_err(|source| AnalyzeError::Write {
        path: dir.clone(),
        source,
    })?;
    let path = dir.join(NOTES_FILE);
    let staged = stage(&dir, NOTES_FILE, &bytes)?;
    if let Err(source) = std::fs::rename(&staged, &path) {
        // The staged copy is ours alone; a failed removal changes nothing
        // the error does not already report.
        let _ = std::fs::remove_file(&staged);
        return Err(AnalyzeError::Write { path, source });
    }
    Ok(NotesReport {
        path,
        tempo,
        transcription,
        skipped,
    })
}

/// Fails when the output folder is a file, or holds a `layers.json` whose
/// source is not `source` and `force` is off. `guarded` says what `--force`
/// does to that report.
fn check_out_dir(
    dir: &Path,
    source: &str,
    force: bool,
    guarded: Guarded,
) -> Result<(), AnalyzeError> {
    if dir.exists() && !dir.is_dir() {
        return Err(AnalyzeError::OutIsFile {
            path: dir.to_path_buf(),
        });
    }
    let existing = dir.join(LAYERS_FILE);
    if force || !existing.exists() {
        return Ok(());
    }
    let recorded = std::fs::read_to_string(&existing)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|report| report.get("source")?.as_str().map(str::to_owned));
    if recorded.as_deref() == Some(source) {
        Ok(())
    } else {
        Err(AnalyzeError::OtherSource {
            path: existing,
            recorded,
            source: source.to_owned(),
            guarded,
        })
    }
}

/// Fails when the output folder holds a `notes.mid` whose track name is
/// not `source` and `force` is off.
fn check_notes_file(dir: &Path, source: &str, force: bool) -> Result<(), AnalyzeError> {
    let existing = dir.join(NOTES_FILE);
    if force || !existing.exists() {
        return Ok(());
    }
    let recorded = std::fs::read(&existing)
        .ok()
        .and_then(|bytes| midi_track_name(&bytes));
    if recorded.as_deref() == Some(source) {
        Ok(())
    } else {
        Err(AnalyzeError::OtherSource {
            path: existing,
            recorded,
            source: source.to_owned(),
            guarded: Guarded::Notes,
        })
    }
}

/// The file of another stem that stopped a run, and what `--force` does
/// to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Guarded {
    /// A `layers.json` that `analyze --force` replaces.
    Report,
    /// A `layers.json` that `notes --force` leaves, writing `notes.mid`
    /// next to it.
    ReportBeside,
    /// A `notes.mid` that `notes --force` replaces.
    Notes,
}

/// Writes `bytes` to a hidden temporary sibling of `name` in `dir` and
/// flushes it to disk; returns its path.
fn stage(dir: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, AnalyzeError> {
    let path = dir.join(format!(".{name}.tmp"));
    let fail = |source| AnalyzeError::Write {
        path: path.clone(),
        source,
    };
    let mut file = std::fs::File::create(&path).map_err(fail)?;
    file.write_all(bytes).map_err(fail)?;
    file.sync_all().map_err(fail)?;
    Ok(path)
}

/// An analysis that failed.
#[derive(Debug)]
pub enum AnalyzeError {
    /// The note list cannot be read.
    Notes(MidiError),
    /// The stem cannot be decoded.
    Decode(DecodeError),
    /// The output folder is a file.
    OutIsFile {
        /// The path of the folder.
        path: PathBuf,
    },
    /// The output folder holds the report or the notes of another stem.
    OtherSource {
        /// The existing `layers.json` or `notes.mid`.
        path: PathBuf,
        /// The stem it names, if it names one.
        recorded: Option<String>,
        /// The stem of this run.
        source: String,
        /// What the file is, and what `--force` does to it.
        guarded: Guarded,
    },
    /// The model run, a decode or the f0 track failed.
    Transcription(NotesError),
    /// The pitch layer failed.
    Pitch(PitchError),
    /// The envelope layer failed.
    Envelope(EnvelopeError),
    /// The stem gave no note.
    NoNotes {
        /// The file name of the stem.
        source: String,
    },
    /// The notes cannot be stored as MIDI.
    Midi(MidiWriteError),
    /// An output file or folder cannot be written.
    Write {
        /// The path that failed.
        path: PathBuf,
        /// The cause.
        source: io::Error,
    },
    /// An internal step failed: a confidence that could not be built, or a
    /// JSON or PNG encoder error.
    Internal(String),
}

impl fmt::Display for AnalyzeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Notes(error) => write!(f, "{error}"),
            Self::Decode(error) => write!(f, "{error}"),
            Self::OutIsFile { path } => write!(
                f,
                "the output folder {} is a file. Pass --out <dir>",
                path.display()
            ),
            Self::OtherSource {
                path,
                recorded,
                source,
                guarded,
            } => {
                let other = recorded
                    .as_deref()
                    .map_or_else(|| "no named stem".to_owned(), |name| format!("`{name}`"));
                let (what, force) = match guarded {
                    Guarded::Report => ("is the report", "replace it"),
                    Guarded::ReportBeside => ("is the report", "write notes.mid next to it"),
                    Guarded::Notes => ("holds the notes", "replace it"),
                };
                write!(
                    f,
                    "{} {what} of {other}, not of `{source}`. Pass --out <dir> for this stem, or --force to {force}",
                    path.display()
                )
            }
            Self::Transcription(error) => write!(f, "{error}"),
            Self::Pitch(error) => write!(f, "{error}"),
            Self::Envelope(error) => write!(f, "{error}"),
            Self::NoNotes { source } => write!(
                f,
                "`{source}` gave no note, so nothing was written: it is silent, or no note sounds within {} dB of its loudest passage. Check the stem, or pass --profile bass|lead|pad",
                QUIET_GATE.get()
            ),
            Self::Midi(error) => write!(f, "{error}"),
            Self::Write { path, source } => write!(
                f,
                "cannot write {}: {source}. Check the folder and its permissions, or pass another --out",
                path.display()
            ),
            Self::Internal(message) => write!(
                f,
                "the analysis failed internally ({message}). Report this as a bug"
            ),
        }
    }
}

impl Error for AnalyzeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Notes(error) => Some(error),
            Self::Decode(error) => Some(error),
            Self::Transcription(error) => Some(error),
            Self::Pitch(error) => Some(error),
            Self::Envelope(error) => Some(error),
            Self::Midi(error) => Some(error),
            Self::Write { source, .. } => Some(source),
            Self::OutIsFile { .. }
            | Self::OtherSource { .. }
            | Self::NoNotes { .. }
            | Self::Internal(_) => None,
        }
    }
}

impl From<ConfidenceError> for AnalyzeError {
    fn from(error: ConfidenceError) -> Self {
        Self::Internal(error.to_string())
    }
}

impl From<serde_json::Error> for AnalyzeError {
    fn from(error: serde_json::Error) -> Self {
        Self::Internal(error.to_string())
    }
}

impl From<png::EncodingError> for AnalyzeError {
    fn from(error: png::EncodingError) -> Self {
        Self::Internal(error.to_string())
    }
}
