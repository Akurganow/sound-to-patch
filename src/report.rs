//! The analysis report of a stem: `layers.json` and `spectrogram.png`.
//!
//! [`analyze`] decodes the stem, reads the note list if one is given, runs
//! the stem estimators and the descriptors, and writes both files into the
//! output folder. The schema of `layers.json` is committed as
//! `schemas/layers.schema.json`.

use std::error::Error;
use std::fmt;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use crate::audio::{self, BeatGrid, DecodeError, SkippedPacket, Stem};
use crate::confidence::ConfidenceError;
use crate::dsp::{StftParams, describe};
use crate::notes::{MidiError, read_midi};
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

/// What [`analyze`] reads and where it writes.
#[derive(Clone, Copy, Debug)]
pub struct AnalyzeRequest<'a> {
    /// The audio stem.
    pub stem: &'a Path,
    /// The tempo of the stem.
    pub bpm: Bpm,
    /// The target synth, written into the report.
    pub synth: Synth,
    /// A MIDI file with the stem's notes.
    pub notes: Option<&'a Path>,
    /// The output folder; by default `<stem folder>/<stem file stem>/`.
    pub out: Option<&'a Path>,
    /// Replace a report of another stem in the output folder.
    pub force: bool,
}

impl AnalyzeRequest<'_> {
    /// Returns the output folder.
    #[must_use]
    pub fn out_dir(&self) -> PathBuf {
        self.out.map_or_else(
            || {
                let name = self.stem.file_stem().unwrap_or(self.stem.as_os_str());
                self.stem.with_file_name(name)
            },
            Path::to_path_buf,
        )
    }

    /// Returns the file name of the stem, as the report records it.
    #[must_use]
    pub fn source(&self) -> String {
        self.stem
            .file_name()
            .unwrap_or(self.stem.as_os_str())
            .to_string_lossy()
            .into_owned()
    }
}

/// What [`analyze`] wrote.
#[derive(Clone, Debug, PartialEq)]
pub struct Analysis {
    /// The path of `layers.json`.
    pub layers: PathBuf,
    /// The path of `spectrogram.png`.
    pub spectrogram: PathBuf,
    /// The packets that did not decode and became silence.
    pub skipped: Vec<SkippedPacket>,
}

/// Analyses a stem and writes `layers.json` and `spectrogram.png` into the
/// output folder, creating it.
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
/// stem cannot be read, or when an output file cannot be written.
pub fn analyze(request: &AnalyzeRequest<'_>) -> Result<Analysis, AnalyzeError> {
    let dir = request.out_dir();
    let source = request.source();
    check_out_dir(&dir, &source, request.force)?;
    let notes = request
        .notes
        .map(|path| read_midi(path, request.bpm))
        .transpose()
        .map_err(AnalyzeError::Notes)?;
    let decoded = audio::decode(request.stem).map_err(AnalyzeError::Decode)?;
    let skipped = decoded.skipped().to_vec();
    let grid = BeatGrid::new(request.bpm, Seconds::clamped(0.0));
    let stem = Stem::new(decoded.into_audio(), grid, notes.as_ref())?;
    let params = StftParams::fixed(stem.frame_len());
    let descriptors = describe(
        stem.mid(),
        stem.sample_rate(),
        params,
        stem.median_f0(),
        stem.hard_limited(),
    )?;
    let json = layers_json(&stem, &descriptors, request.synth, &source)?;
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
        skipped,
    })
}

/// Fails when the output folder is a file, or holds a `layers.json` whose
/// source is not `source` and `force` is off.
fn check_out_dir(dir: &Path, source: &str, force: bool) -> Result<(), AnalyzeError> {
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
        })
    }
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
    /// The output folder holds the report of another stem.
    OtherSource {
        /// The existing `layers.json`.
        path: PathBuf,
        /// The stem it names, if it names one.
        recorded: Option<String>,
        /// The stem of this run.
        source: String,
    },
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
            } => {
                let other = recorded
                    .as_deref()
                    .map_or_else(|| "no named stem".to_owned(), |name| format!("`{name}`"));
                write!(
                    f,
                    "{} is the report of {other}, not of `{source}`. Pass --out <dir> for this stem, or --force to replace it",
                    path.display()
                )
            }
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
            Self::Write { source, .. } => Some(source),
            Self::OutIsFile { .. } | Self::OtherSource { .. } | Self::Internal(_) => None,
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
