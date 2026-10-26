//! Turn an audio stem or a written sound description into a Serum 2 starting
//! point.
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
//! - [`units`]: unit newtypes, checked once when built, with their
//!   conversions, serde and the unit-string parser.

pub mod cli;
pub mod confidence;
pub mod units;
