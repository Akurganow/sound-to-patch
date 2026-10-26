//! Development tasks for sound-to-patch. Run them with `cargo xtask`.
//!
//! Nothing here ships. The tasks generate release artifacts, run repository
//! checks and fetch pinned datasets.

mod architecture;
mod credits;
mod dist;
mod fetch;
mod pr;
mod version;
mod vital;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::fetch::Sha256Digest;

/// Development tasks for sound-to-patch.
#[derive(Debug, Parser)]
#[command(
    name = "xtask",
    bin_name = "cargo xtask",
    long_about = None,
    arg_required_else_help = true
)]
struct Xtask {
    #[command(subcommand)]
    task: Task,
}

#[derive(Debug, Subcommand)]
enum Task {
    /// Write the man page and the shell completions.
    Dist {
        /// Directory that receives `man/` and `completions/`.
        #[arg(long, default_value = "target/dist")]
        out_dir: PathBuf,
    },
    /// Download a file and keep it only if its SHA-256 matches.
    Fetch {
        /// The pinned URL.
        url: String,
        /// The expected SHA-256 as 64 hex digits.
        #[arg(long)]
        sha256: Sha256Digest,
        /// Where the verified file goes.
        #[arg(long)]
        out: PathBuf,
    },
    /// Check that `ARCHITECTURE.md` names every top-level module of `src/`.
    CheckArchitecture,
    /// Check that every cited source has a credit with a licence.
    CheckCredits,
    /// Check that the docs name the manifest's `rust-version`.
    CheckRustVersion,
    /// Check a pull request description and title, read from the `PR_BODY`
    /// and `PR_TITLE` variables.
    CheckPrBody,
    /// Write `src/preset/vital/registry.json` from a Vital source checkout, or
    /// with `--check` fail when the committed file differs.
    VitalRegistry {
        /// The root of a local Vital checkout. Its text is read, never run.
        #[arg(long)]
        source: PathBuf,
        /// Write nothing and fail when the committed file is stale.
        #[arg(long)]
        check: bool,
    },
    /// Write a release version into the workspace manifest and the lock file.
    SetVersion {
        /// The new version, without a `v` prefix.
        version: String,
    },
}

fn main() -> ExitCode {
    let result = match Xtask::parse().task {
        Task::Dist { out_dir } => dist::write(&out_dir),
        Task::Fetch { url, sha256, out } => fetch::fetch(&url, &sha256, &out),
        Task::CheckArchitecture => architecture::check(root()),
        Task::CheckCredits => credits::check(root()),
        Task::CheckRustVersion => version::check_rust_version(root()),
        Task::CheckPrBody => std::env::var("PR_BODY")
            .map_err(|_| "set PR_BODY to the pull request description".into())
            .and_then(|body| pr::check_body(&body))
            .and_then(|()| {
                std::env::var("PR_TITLE")
                    .map_err(|_| "set PR_TITLE to the pull request title".into())
            })
            .and_then(|title| pr::check_title(&title)),
        Task::VitalRegistry { source, check } => vital::run(root(), &source, check),
        Task::SetVersion { version } => version::set(root(), &version),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// The workspace root, whatever directory `cargo xtask` runs from.
fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

/// The error of any task: a message that says what failed and how to fix it.
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
