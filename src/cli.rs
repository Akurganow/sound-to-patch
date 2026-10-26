//! The command-line definition.
//!
//! The binary parses it, and `cargo xtask` renders the man page and the shell
//! completions from the same tree, so the three never disagree. Each command
//! keeps its arguments in a module of its own.

use std::error::Error;

use clap::{Parser, Subcommand};

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
    /// Read, inspect and write synth presets.
    Preset(preset::PresetArgs),
}

impl Command {
    /// Runs the command and returns the text to print.
    ///
    /// # Errors
    ///
    /// The error of the command; its message names what failed, the cause
    /// and the fix.
    pub fn run(&self) -> Result<String, Box<dyn Error + Send + Sync>> {
        match self {
            Self::Preset(args) => match &args.command {
                preset::PresetCommand::Inspect(inspect) => Ok(crate::preset::inspect(
                    &inspect.file,
                    &inspect.synth,
                    &inspect.describe_options(),
                )?),
                preset::PresetCommand::Apply(apply) => {
                    Ok(crate::preset::apply_file(&crate::preset::ApplyRequest {
                        edits: &apply.edits,
                        out: &apply.out,
                        template: apply.template.as_deref(),
                        synth: &apply.synth,
                        name: apply.name.as_deref(),
                        force: apply.force,
                    })?)
                }
            },
        }
    }
}
