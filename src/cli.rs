//! The command-line definition.
//!
//! The binary parses it, and `cargo xtask` renders the man page and the shell
//! completions from the same tree, so the three never disagree. Each command
//! keeps its arguments in a module of its own.

use std::error::Error;

use clap::{Parser, Subcommand};

pub mod analyze;
pub mod preset;

/// Arguments of the `sound-to-patch` command.
///
/// # Examples
///
/// ```
/// use clap::Parser;
/// use clap::error::ErrorKind;
/// use sound_to_patch::cli::Cli;
///
/// let err = Cli::try_parse_from(["sound-to-patch", "--version"]).unwrap_err();
/// assert_eq!(err.kind(), ErrorKind::DisplayVersion);
/// ```
#[derive(Debug, Parser)]
#[command(
    name = "sound-to-patch",
    version,
    about,
    long_about = None,
    arg_required_else_help = true
)]
pub struct Cli {
    /// The command to run.
    #[command(subcommand)]
    pub command: Command,
}

/// The commands, one variant per line.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Analyse a stem and write layers.json and spectrogram.png.
    ///
    /// Example: sound-to-patch analyze bass.wav --bpm 140
    Analyze(analyze::AnalyzeArgs),
    /// Read, inspect and write synth presets.
    Preset(preset::PresetArgs),
}

/// The text a command prints: the result to standard output, warnings to
/// standard error.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Output {
    /// The result.
    pub stdout: String,
    /// Warnings, one per line.
    pub stderr: String,
}

impl From<String> for Output {
    fn from(stdout: String) -> Self {
        Self {
            stdout,
            stderr: String::new(),
        }
    }
}

impl Command {
    /// Runs the command and returns the text to print.
    ///
    /// # Errors
    ///
    /// The error of the command; its message names what failed, the cause
    /// and the fix.
    pub fn run(&self) -> Result<Output, Box<dyn Error + Send + Sync>> {
        match self {
            Self::Analyze(args) => {
                let analysis = crate::report::analyze(&crate::report::AnalyzeRequest {
                    stem: &args.stem,
                    bpm: args.bpm,
                    synth: args.synth()?,
                    notes: args.notes.as_deref(),
                    out: args.out.as_deref(),
                    force: args.force,
                })?;
                let line = |text: String| text + "\n";
                Ok(Output {
                    stdout: [&analysis.layers, &analysis.spectrogram]
                        .iter()
                        .map(|path| line(format!("wrote {}", path.display())))
                        .collect(),
                    stderr: analysis
                        .skipped
                        .iter()
                        .map(|packet| {
                            line(format!(
                                "warning: {packet} did not decode and became silence"
                            ))
                        })
                        .collect(),
                })
            }
            Self::Preset(args) => match &args.command {
                preset::PresetCommand::Inspect(inspect) => Ok(crate::preset::inspect(
                    &inspect.file,
                    &inspect.synth,
                    &inspect.describe_options(),
                )?
                .into()),
                preset::PresetCommand::Apply(apply) => {
                    Ok(crate::preset::apply_file(&crate::preset::ApplyRequest {
                        edits: &apply.edits,
                        out: &apply.out,
                        template: apply.template.as_deref(),
                        synth: &apply.synth,
                        name: apply.name.as_deref(),
                        force: apply.force,
                    })?
                    .into())
                }
            },
        }
    }
}
