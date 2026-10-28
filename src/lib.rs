//! Turn an audio stem or a written sound description into a starting point
//! in a target synth, Vital or Serum 2.
//!
//! The library holds all logic. The `sound-to-patch` binary parses the
//! command line with [`cli::Cli`], calls the library and prints the result.
//! The library itself never prints.
//!
//! # Modules
//!
//! - [`audio`]: stem intake. Decodes a file at its native rate, mixes it to
//!   mono and resamples it for a consumer with a fixed rate.
//! - [`cli`]: the command-line definition, shared by the binary and by the
//!   man page and completion generators.
//! - [`confidence`]: how far a layer trusts an estimate, as a level with
//!   reasons and a spread, or no estimate.
//! - [`dsp`]: the signal core: windows, STFT, peak picking, active frames,
//!   the analytic signal and decay fits.
//! - [`music`]: the music dictionary for notes, embedded from
//!   `dictionary/music/`, with the key, note-name, numeral and chord-symbol
//!   parsers, the snap rule and the rhythm arithmetic.
//! - [`notes`]: the note list that every layer reads, the transcriber with
//!   its profiles and its register detection, the f0 track, the cleanup
//!   with the quiet-note guard, the notes step of a stem, the MIDI reader
//!   and writer, and the harmonic masks built from the list.
//! - [`preset`]: preset files behind one adapter per synth: the format
//!   traits, the parameter registry with its laws, the template, the edit
//!   list and its applier, the mapper trait that turns layer values into
//!   edits, and the Vital adapter.
//! - [`render`]: rendering a preset and its notes to audio: the recipe, the
//!   render job and record, the renderer trait, a fake renderer for tests,
//!   the plug-in state of a preset and the renderer that drives the plug-in
//!   host process.
//! - [`report`]: the outputs of a stem: `layers.json` and the spectrogram
//!   image with the `analyze` pipeline that writes them, and `notes.mid`
//!   with the `notes` command.
//! - [`synth`]: the target synth, the shared `--synth` argument and what
//!   each synth offers in structure.
//! - [`units`]: unit newtypes, checked once when built, with their
//!   conversions, serde and the unit-string parser.

pub mod audio;
mod cast;
pub mod cli;
pub mod confidence;
pub mod dsp;
pub mod music;
pub mod notes;
pub mod preset;
pub mod render;
pub mod report;
pub mod synth;
pub mod units;
