//! The arguments of `preset`.

use std::path::PathBuf;

use clap::{Args, Subcommand};

use crate::preset::format::DescribeOptions;
use crate::synth::SynthArg;

/// Arguments of `preset`.
#[derive(Debug, Args)]
pub struct PresetArgs {
    /// The preset subcommand.
    #[command(subcommand)]
    pub command: PresetCommand,
}

/// The subcommands of `preset`.
#[derive(Debug, Subcommand)]
pub enum PresetCommand {
    /// Print the header fields and the decoded tree of a preset file.
    ///
    /// Example: sound-to-patch preset inspect bass.vital
    Inspect(InspectArgs),
    /// Apply an edit list to a template and write a preset.
    ///
    /// Values are written with their unit, such as "1.2 kHz", "800 ms",
    /// "-6 dB", "+12 st" or "25%".
    ///
    /// Example: sound-to-patch preset apply edits.json -o bass.vital
    Apply(ApplyArgs),
}

/// Arguments of `preset inspect`.
#[derive(Debug, Args)]
pub struct InspectArgs {
    /// The preset file, such as `bass.vital`.
    pub file: PathBuf,
    /// The target synth.
    #[command(flatten)]
    pub synth: SynthArg,
    /// Print every list element and every text in full.
    #[arg(long)]
    pub full: bool,
    /// List elements printed before the rest is counted.
    #[arg(long, value_name = "N", default_value_t = DescribeOptions::default().shown)]
    pub shown: usize,
}

impl InspectArgs {
    /// The describe options these arguments ask for.
    #[must_use]
    pub fn describe_options(&self) -> DescribeOptions {
        DescribeOptions {
            full: self.full,
            shown: self.shown,
        }
    }
}

/// Arguments of `preset apply`.
#[derive(Debug, Args)]
pub struct ApplyArgs {
    /// The edit list, a JSON file.
    pub edits: PathBuf,
    /// The preset to write, such as `bass.vital`.
    #[arg(short, long, value_name = "FILE")]
    pub out: PathBuf,
    /// A preset to start from instead of the synth's built-in template.
    #[arg(long, value_name = "FILE")]
    pub template: Option<PathBuf>,
    /// The preset name. Without it, the name of the output file.
    #[arg(long)]
    pub name: Option<String>,
    /// Replace the output file if it exists.
    #[arg(long)]
    pub force: bool,
    /// The target synth.
    #[command(flatten)]
    pub synth: SynthArg,
}
