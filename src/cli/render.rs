//! The arguments of `render`.

use std::path::PathBuf;

use clap::Args;

use super::analyze::parse_bpm;
use crate::render::file::Backend;
use crate::synth::SynthArg;
use crate::units::Bpm;

/// Arguments of `render`.
#[derive(Debug, Args)]
pub struct RenderArgs {
    /// The preset to render
    pub preset: PathBuf,
    /// The MIDI file with the notes, read as `analyze --notes` reads it
    #[arg(long, value_name = "FILE")]
    pub midi: PathBuf,
    /// The tempo of the MIDI file in beats per minute, 4 to 1000
    #[arg(long, value_name = "BPM", value_parser = parse_bpm, allow_negative_numbers = true)]
    pub bpm: Bpm,
    /// The WAV file to write; the render record goes next to it with .json
    #[arg(short, long, value_name = "WAV")]
    pub out: PathBuf,
    /// The synth's .vst3 bundle [default: the bundle in
    /// /Library/Audio/Plug-Ins/VST3, else in the user's VST3 folder]
    #[arg(long, value_name = "BUNDLE")]
    pub plugin: Option<PathBuf>,
    /// The target synth.
    #[command(flatten)]
    pub synth: SynthArg,
    /// Replace an existing WAV or record file
    #[arg(long)]
    pub force: bool,
    /// What renders; `fake` is for tests of the command
    #[arg(long, value_enum, default_value_t = Backend::Plugin, hide = true)]
    pub backend: Backend,
}
