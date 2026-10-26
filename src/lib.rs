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
//! - [`notes`]: the note list that every layer reads, and the harmonic
//!   masks built from it.
//! - [`preset`]: preset files behind one adapter per synth: the format
//!   traits, the parameter registry with its laws, the template and the
//!   Vital adapter.
//! - [`synth`]: the target synth, the shared `--synth` argument and what
//!   each synth offers in structure.
//! - [`units`]: unit newtypes, checked once when built, with their
//!   conversions, serde and the unit-string parser.

pub mod audio;
mod cast;
pub mod cli;
pub mod confidence;
pub mod dsp;
pub mod notes;
pub mod preset;
pub mod synth;
pub mod units;
