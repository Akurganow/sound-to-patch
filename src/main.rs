//! The `sound-to-serum` command. It parses the arguments and calls the
//! library.

use clap::Parser;
use sound_to_serum::cli::Cli;

fn main() {
    let _cli = Cli::parse();
}
