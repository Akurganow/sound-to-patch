//! The command-line definition.
//!
//! The binary parses it, and `cargo xtask` renders the man page and the shell
//! completions from the same tree, so the three never disagree.

use clap::Parser;

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
pub struct Cli {}
