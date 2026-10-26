//! The `sound-to-patch` command. It parses the arguments, calls the
//! library and prints the result.

use std::io::{ErrorKind, Write as _};
use std::process::ExitCode;

use clap::Parser;
use sound_to_patch::cli::Cli;

fn main() -> ExitCode {
    match Cli::parse().command.run() {
        Ok(text) => {
            let mut stdout = std::io::stdout().lock();
            match stdout
                .write_all(text.as_bytes())
                .and_then(|()| stdout.flush())
            {
                // A closed pipe, as with `| head`, is not an error of the command.
                Err(error) if error.kind() != ErrorKind::BrokenPipe => {
                    eprintln!(
                        "error: cannot write the output: {error}. Check the disk or the pipe the output goes to"
                    );
                    ExitCode::FAILURE
                }
                _ => ExitCode::SUCCESS,
            }
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
