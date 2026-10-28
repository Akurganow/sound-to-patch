//! The arguments of `notes`.

use std::path::PathBuf;

use clap::Args;

use super::analyze::parse_bpm;
use crate::notes::Profile;
use crate::units::Bpm;

/// Arguments of `notes`.
#[derive(Debug, Args)]
pub struct NotesArgs {
    /// The audio stem: WAV, AIFF, FLAC, MP3, AAC or M4A.
    pub stem: PathBuf,
    /// The tempo of the stem in beats per minute, 4 to 1000. It is refined
    /// from the stem, 2.91 % below to 3.09 % above it, unless --bpm-exact
    /// is given
    #[arg(long, value_name = "BPM", value_parser = parse_bpm, allow_negative_numbers = true)]
    pub bpm: Bpm,
    /// Use --bpm as it is, without refining it from the stem
    #[arg(long)]
    pub bpm_exact: bool,
    /// Decode with the settings of this kind of stem instead of the
    /// detected ones
    #[arg(long, value_enum, value_name = "PROFILE")]
    pub profile: Option<Profile>,
    /// The output folder [default: a folder named after the stem, next to
    /// it]
    #[arg(short, long, value_name = "DIR")]
    pub out: Option<PathBuf>,
    /// Write next to a report of another stem in the output folder
    #[arg(long)]
    pub force: bool,
}
