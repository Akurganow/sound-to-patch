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
