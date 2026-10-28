//! `sound-to-patch-host`: the process that loads a VST3 plug-in headless
//! and renders notes through it, for `sound-to-patch`.
//!
//! It reads one request per line on standard input and answers each with
//! one reply line on standard output; `sound_to_patch::render::wire` holds
//! the protocol. Before it loads a plug-in, it points file descriptor 1 at
//! standard error and keeps the real standard output for replies, so
//! nothing a plug-in prints can split or swallow a reply. It opens no
//! window and needs no run loop. Plug-ins load on macOS only; on other
//! systems every request fails with that reason.

use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use sound_to_patch::render::wire::{Reply, Request, reply_line};

#[cfg(target_os = "macos")]
mod session;

/// Answers the requests of one run.
trait Handler {
    /// Answers one request other than [`Request::Quit`].
    fn handle(&mut self, request: Request) -> Reply;
}

fn failed(cause: impl Into<String>, fix: impl Into<String>) -> Reply {
    Reply::Failed {
        cause: cause.into(),
        fix: fix.into(),
    }
}

/// Answers requests until [`Request::Quit`] or the end of the input.
fn serve(
    input: impl BufRead,
    output: &mut impl Write,
    handler: &mut impl Handler,
) -> io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let (reply, quit) = match serde_json::from_str::<Request>(&line) {
            Ok(Request::Quit) => (Reply::Bye, true),
            Ok(request) => (handler.handle(request), false),
            Err(error) => (
                failed(
                    format!("the request is not valid ({error})"),
                    "Send one request of the wire protocol per line",
                ),
                false,
            ),
        };
        let text = reply_line(&reply).map_err(io::Error::other)?;
        writeln!(output, "{text}")?;
        output.flush()?;
        if quit {
            break;
        }
    }
    Ok(())
}

/// The handler of a system where plug-ins do not load.
#[cfg(not(target_os = "macos"))]
struct Unsupported;

#[cfg(not(target_os = "macos"))]
impl Handler for Unsupported {
    fn handle(&mut self, _request: Request) -> Reply {
        failed("the plug-in host runs on macOS only", "Render on a Mac")
    }
}

/// Keeps the real standard output for replies and points file descriptor
/// 1, where a plug-in prints, at standard error.
#[cfg(target_os = "macos")]
fn reply_pipe() -> io::Result<io::BufWriter<std::fs::File>> {
    use std::os::fd::AsFd as _;
    let replies = io::stdout().as_fd().try_clone_to_owned()?;
    rustix::stdio::dup2_stdout(io::stderr().as_fd())?;
    Ok(io::BufWriter::new(std::fs::File::from(replies)))
}

#[cfg(target_os = "macos")]
fn run() -> io::Result<()> {
    let mut output = reply_pipe()?;
    serve(
        io::stdin().lock(),
        &mut output,
        &mut session::Sessions::default(),
    )
}

#[cfg(not(target_os = "macos"))]
fn run() -> io::Result<()> {
    serve(io::stdin().lock(), &mut io::stdout(), &mut Unsupported)
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("sound-to-patch-host: the request pipe failed: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Answers every request with the frame count 1.
    struct Echo(usize);

    impl Handler for Echo {
        fn handle(&mut self, _request: Request) -> Reply {
            self.0 += 1;
            Reply::Rendered {
                frames: self.0,
                capped: false,
            }
        }
    }

    fn replies(input: &str) -> Vec<Reply> {
        let mut output = Vec::new();
        serve(input.as_bytes(), &mut output, &mut Echo(0)).unwrap();
        String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| {
                sound_to_patch::render::wire::parse_reply(line)
                    .unwrap()
                    .1
                    .unwrap()
            })
            .collect()
    }

    #[test]
    fn each_request_gets_one_reply_and_quit_ends_the_run() {
        let input = "{\"op\":\"save_state\"}\n\n{\"op\":\"quit\"}\n{\"op\":\"save_state\"}\n";
        assert_eq!(
            replies(input),
            [
                Reply::Rendered {
                    frames: 1,
                    capped: false
                },
                Reply::Bye
            ]
        );
    }

    #[test]
    fn a_bad_line_fails_and_the_run_goes_on() {
        let replies = replies("not json\n{\"op\":\"save_state\"}\n");
        assert!(
            matches!(&replies[0], Reply::Failed { cause, .. } if cause.starts_with("the request is not valid")),
            "{replies:?}"
        );
        assert_eq!(replies.len(), 2);
    }
}
