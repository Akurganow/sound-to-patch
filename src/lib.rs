//! Turn an audio stem or a written sound description into a starting point
//! in a target synth, Vital or Serum 2.
//!
//! The library holds all logic. The `sound-to-patch` binary parses the
//! command line with [`cli::Cli`], calls the library and prints the result.
//! The library itself never prints.
//!
//! # Modules
//!
//! - [`cli`]: the command-line definition, shared by the binary and by the
//!   man page and completion generators.
//! - [`confidence`]: how far a layer trusts an estimate, as a level with
//!   reasons and a spread, or no estimate.
//! - [`preset`]: preset files behind one adapter per synth: the format
//!   traits, the parameter registry with its laws, the template and the
//!   Vital adapter.
//! - [`synth`]: the target synth, the shared `--synth` argument and what
//!   each synth offers in structure.
//! - [`units`]: unit newtypes, checked once when built, with their
//!   conversions, serde and the unit-string parser.

mod cast;
pub mod cli;
pub mod confidence;
pub mod preset;
pub mod synth;
pub mod units;
