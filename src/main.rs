//! The `sound-to-patch` command. It parses the arguments and calls the
//! library.

use clap::Parser;
use sound_to_patch::cli::Cli;

fn main() {
    let _cli = Cli::parse();
}
