//! The `sound-to-patch` command. It parses the arguments, calls the
//! library and prints the result.

use std::io::Write as _;
use std::process::ExitCode;

use clap::Parser;
use sound_to_patch::cli::Cli;

fn main() -> ExitCode {
    match Cli::parse().command.run() {
        Ok(text) => {
            // A closed pipe, as with `| head`, is not an error of the command.
            let _ = std::io::stdout().lock().write_all(text.as_bytes());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
