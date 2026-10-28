//! The command-line definition.
//!
//! The binary parses it, and `cargo xtask` renders the man page and the shell
//! completions from the same tree, so the three never disagree. Each command
//! keeps its arguments in a module of its own.

use std::error::Error;

use clap::{Parser, Subcommand};

pub mod analyze;
pub mod notes;
pub mod preset;
pub mod render;

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
    /// The tempo is refined from the stem near --bpm, and the stem is
    /// transcribed unless --notes gives its notes. Each step prints what it
    /// found.
    ///
    /// Example: sound-to-patch analyze bass.wav --bpm 140
    Analyze(analyze::AnalyzeArgs),
    /// Transcribe a stem and write notes.mid.
    ///
    /// The pitch range, the voicing and the minimum note length come from
    /// the stem unless --profile sets them. Each step prints what it found.
    ///
    /// Example: sound-to-patch notes bass.wav --bpm 140
    Notes(notes::NotesArgs),
    /// Read, inspect and write synth presets.
    Preset(preset::PresetArgs),
    /// Render a preset and a MIDI file to a WAV file through the synth.
    ///
    /// The synth's plug-in loads headless in the plug-in host process. A
    /// render counts only after the load check: the clip sounds, the state
    /// the plug-in saves back holds the preset, and the clip differs from
    /// the plug-in's init patch. Each step prints what it found.
    ///
    /// Example: sound-to-patch render bass.vital --midi notes.mid --bpm 140 -o bass.wav
    Render(render::RenderArgs),
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
                let analysis = crate::report::analyze(
                    &crate::report::AnalyzeRequest {
                        stem: &args.stem,
                        bpm: args.bpm,
                        bpm_exact: args.bpm_exact,
                        synth: args.synth()?,
                        notes: args.notes.as_deref(),
                        profile: args.profile,
                        out: args.out.as_deref(),
                        force: args.force,
                    },
                    &crate::notes::Transcriber::new()?,
                )?;
                let line = |text: String| text + "\n";
                Ok(Output {
                    stdout: analysis
                        .summary()
                        .into_iter()
                        .chain(
                            [&analysis.layers, &analysis.spectrogram]
                                .iter()
                                .map(|path| format!("wrote {}", path.display())),
                        )
                        .map(line)
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
            Self::Notes(args) => {
                let report = crate::report::notes(
                    &crate::report::NotesRequest {
                        stem: &args.stem,
                        bpm: args.bpm,
                        bpm_exact: args.bpm_exact,
                        profile: args.profile,
                        out: args.out.as_deref(),
                        force: args.force,
                    },
                    &crate::notes::Transcriber::new()?,
                )?;
                let line = |text: String| text + "\n";
                Ok(Output {
                    stdout: report
                        .summary()
                        .into_iter()
                        .chain([format!("wrote {}", report.path.display())])
                        .map(line)
                        .collect(),
                    stderr: report
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
            Self::Render(args) => render(args),
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

fn render(args: &render::RenderArgs) -> Result<Output, Box<dyn Error + Send + Sync>> {
    let report = crate::render::file::render_file(&crate::render::file::RenderRequest {
        preset: &args.preset,
        midi: &args.midi,
        bpm: args.bpm,
        out: &args.out,
        synth: args.synth.resolve(Some(&args.preset))?,
        plugin: args.plugin.as_deref(),
        force: args.force,
        backend: args.backend,
    })?;
    Ok(report
        .summary()
        .into_iter()
        .map(|line| line + "\n")
        .collect::<String>()
        .into())
}
