//! The arguments of `analyze`.

use std::path::PathBuf;

use clap::Args;

use crate::notes::Profile;
use crate::synth::{Synth, SynthArg, SynthError};
use crate::units::Bpm;

/// Arguments of `analyze`.
#[derive(Debug, Args)]
pub struct AnalyzeArgs {
    /// The audio stem: WAV, AIFF, FLAC, MP3, AAC or M4A.
    pub stem: PathBuf,
    /// The tempo of the stem in beats per minute, 4 to 1000. It is refined
    /// from the stem, 2.91 % below to 3.09 % above it, unless --bpm-exact
    /// is given
    #[arg(long, value_name = "BPM", value_parser = parse_bpm)]
    pub bpm: Bpm,
    /// Use --bpm as it is, without refining it from the stem
    #[arg(long)]
    pub bpm_exact: bool,
    /// A MIDI file with the stem's notes, instead of transcribing the stem;
    /// the f0 track still reads the stem unless the notes play chords. Its
    /// tempo must match --bpm or the refined tempo
    #[arg(long, value_name = "FILE")]
    pub notes: Option<PathBuf>,
    /// Decode with the settings of this kind of stem instead of the
    /// detected ones
    #[arg(long, value_enum, value_name = "PROFILE", conflicts_with = "notes")]
    pub profile: Option<Profile>,
    /// The output folder [default: a folder named after the stem, next to
    /// it]
    #[arg(short, long, value_name = "DIR")]
    pub out: Option<PathBuf>,
    /// Replace a report of another stem in the output folder
    #[arg(long)]
    pub force: bool,
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

/// Parses `--bpm`: a number from 4 to 1000.
pub(crate) fn parse_bpm(text: &str) -> Result<Bpm, String> {
    let value: f64 = text
        .trim()
        .parse()
        .map_err(|_| format!("`{text}` is not a number"))?;
    Bpm::new(value).map_err(|error| error.to_string())
}
