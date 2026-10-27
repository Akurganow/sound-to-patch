//! The analysis report of a stem: `layers.json` and `spectrogram.png`.
//!
//! [`analyze`] decodes the stem, reads the note list if one is given, runs
//! the stem estimators and the descriptors, and writes both files into the
//! output folder. The schema of `layers.json` is committed as
//! `schemas/layers.schema.json`.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::audio::{self, BeatGrid, DecodeError, Stem};
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
}

/// Analyses a stem and writes `layers.json` and `spectrogram.png` into the
/// output folder, creating it.
///
/// Both files are overwritten; nothing else in the folder changes. Returns
/// the text to print: the written paths and any corrupt packets that
/// became silence.
///
/// # Errors
///
/// Returns an [`AnalyzeError`] when the note list or the stem cannot be
/// read, or an output file cannot be written.
pub fn analyze(request: &AnalyzeRequest<'_>) -> Result<String, AnalyzeError> {
    let notes = request
        .notes
        .map(|path| read_midi(path, request.bpm))
        .transpose()
        .map_err(AnalyzeError::Notes)?;
    let decoded = audio::decode(request.stem).map_err(AnalyzeError::Decode)?;
    let mut lines: Vec<String> = decoded
        .skipped()
        .iter()
        .map(|packet| format!("warning: packet {packet} did not decode and became silence"))
        .collect();
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
    let json = layers_json(&stem, &descriptors, request.synth)?;
    let png = Spectrogram::of(stem.mid(), stem.sample_rate()).to_png()?;

    let dir = request.out_dir();
    let write = |name: &str, bytes: &[u8]| {
        let path = dir.join(name);
        std::fs::write(&path, bytes)
            .map(|()| path.clone())
            .map_err(|source| AnalyzeError::Write { path, source })
    };
    std::fs::create_dir_all(&dir).map_err(|source| AnalyzeError::Write {
        path: dir.clone(),
        source,
    })?;
    for path in [
        write(LAYERS_FILE, json.as_bytes())?,
        write(SPECTROGRAM_FILE, &png)?,
    ] {
        lines.push(format!("wrote {}", path.display()));
    }
    Ok(lines.join("\n") + "\n")
}

/// An analysis that failed.
#[derive(Debug)]
pub enum AnalyzeError {
    /// The note list cannot be read.
    Notes(MidiError),
    /// The stem cannot be decoded.
    Decode(DecodeError),
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
            Self::Internal(_) => None,
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
