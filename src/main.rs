//! The `sound-to-patch` command. It parses the arguments, calls the
//! library and prints the result.

use std::io::{ErrorKind, Write as _};
use std::process::ExitCode;

use clap::Parser;
use sound_to_patch::cli::Cli;

fn main() -> ExitCode {
    match Cli::parse().command.run() {
        Ok(output) => {
            // A closed stderr is ignored like a closed stdout, so the exit code stays.
            let _ = write!(std::io::stderr(), "{}", output.stderr);
            let mut stdout = std::io::stdout().lock();
            match stdout
                .write_all(output.stdout.as_bytes())
                .and_then(|()| stdout.flush())
            {
                // A closed pipe, as with `| head`, is not an error of the command.
                Err(error) if error.kind() != ErrorKind::BrokenPipe => {
                    let _ = writeln!(
                        std::io::stderr(),
                        "error: cannot write the output: {error}. Check the disk or the pipe the output goes to"
                    );
                    ExitCode::FAILURE
                }
                _ => ExitCode::SUCCESS,
            }
        }
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "error: {error}");
            ExitCode::FAILURE
        }
    }
}
