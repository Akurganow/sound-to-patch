//! The arguments of `analyze`.

use std::path::PathBuf;

use clap::Args;

use crate::synth::{Synth, SynthArg, SynthError};
use crate::units::Bpm;

/// Arguments of `analyze`.
#[derive(Debug, Args)]
pub struct AnalyzeArgs {
    /// The audio stem: WAV, AIFF, FLAC, MP3, AAC or M4A.
    pub stem: PathBuf,
    /// The tempo of the stem in beats per minute, 4 to 1000.
    #[arg(long, value_name = "BPM", value_parser = parse_bpm)]
    pub bpm: Bpm,
    /// A MIDI file with the stem's notes. Its tempo must match --bpm.
    #[arg(long, value_name = "FILE")]
    pub notes: Option<PathBuf>,
    /// The output folder [default: a folder named after the stem, next to
    /// it]
    #[arg(short, long, value_name = "DIR")]
    pub out: Option<PathBuf>,
    /// The target synth that the report names [default: vital]
    #[arg(long, value_enum, value_name = "SYNTH")]
    pub synth: Option<Synth>,
}

impl AnalyzeArgs {
    /// Resolves the target synth: the flag, else Vital, and only a synth
    /// with an adapter.
    ///
    /// # Errors
    ///
    /// [`SynthError::NotBuilt`] for a synth without an adapter.
    pub fn synth(&self) -> Result<Synth, SynthError> {
        SynthArg { synth: self.synth }.resolve(None)
    }
}

fn parse_bpm(text: &str) -> Result<Bpm, String> {
    let value: f64 = text
        .trim()
        .parse()
        .map_err(|_| format!("`{text}` is not a number"))?;
    Bpm::new(value).map_err(|error| error.to_string())
}
