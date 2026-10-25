//! Turn an audio stem or a written sound description into a Serum 2 starting
//! point.
//!
//! The library holds all logic. The `sound-to-serum` binary parses the
//! command line with [`cli::Cli`], calls the library and prints the result.
//! The library itself never prints.
//!
//! # Modules
//!
//! - [`cli`]: the command-line definition, shared by the binary and by the
//!   man page and completion generators.

pub mod cli;
