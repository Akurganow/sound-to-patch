//! Development tasks for sound-to-patch. Run them with `cargo xtask`.
//!
//! Nothing here ships. The tasks generate release artifacts, run repository
//! checks and fetch pinned datasets.

mod agreement;
mod architecture;
mod credits;
mod dist;
mod eval;
mod fetch;
mod fixtures;
mod golden;
mod lock;
mod music;
mod plugins;
mod pr;
mod ranks;
mod reference;
mod timing;
mod version;
mod vital;
mod words;

use std::num::NonZeroUsize;
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
    /// Score the notes step on generated parts with known notes; local only.
    Eval {
        /// Run only the parts whose name holds this text, and print their
        /// notes.
        #[arg(long)]
        part: Option<String>,
    },
    /// Recompute the listener agreement of the producer words from the
    /// pinned listener data (needs uv and network).
    Agreement {
        /// The download folder for the listener tables, outside the
        /// repository.
        #[arg(long)]
        data: PathBuf,
    },
    /// Regenerate the golden vectors with the pinned Python tools (needs uv).
    Golden,
    /// Check the golden vectors against the pinned tool versions.
    GoldenCheck,
    /// Check that `ARCHITECTURE.md` names every top-level module of `src/`.
    CheckArchitecture,
    /// Check that `Cargo.lock` holds no ONNX Runtime crate.
    CheckNoOrt,
    /// Check that `fuzz/Cargo.lock` names the crate version of `Cargo.lock`.
    CheckFuzzLock,
    /// Check that every cited source has a credit with a licence.
    CheckCredits,
    /// Recompute the ranked list of describing words from the pinned preset
    /// banks, sound tags and captions; local only (needs uv and network,
    /// downloads about 2.4 GB).
    WordRanks {
        /// The download folder for the pinned inputs, outside the
        /// repository.
        #[arg(long)]
        data: PathBuf,
    },
    /// Check the agent plugins: manifests, the marketplace, skill sizes and
    /// frontmatter, reference files, banned phrases and probes.
    CheckPlugins,
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
    /// Write the generated files of the shared reference folder, or with
    /// `--check` fail when one differs.
    Reference {
        /// Write nothing and fail when a committed file is stale.
        #[arg(long)]
        check: bool,
    },
    /// Write the generated Vital fixtures of `tests/fixtures/vital/`.
    VitalFixtures {
        /// Compare the committed fixtures with the generator instead.
        #[arg(long)]
        check: bool,
        /// Write nothing and list the registry keys no fixture holds.
        #[arg(long, conflicts_with = "check")]
        coverage: bool,
    },
    /// Time plug-in loads, state loads, renders and the throughput of 1, 2,
    /// 4 and 8 workers on the Vital plug-in, and write the table; local
    /// only.
    RenderTiming {
        /// The preset to render [default: `tests/fixtures/vital/routes.vital`].
        #[arg(long)]
        preset: Option<PathBuf>,
        /// The Vital .vst3 bundle [default: the standard VST3 folders].
        #[arg(long)]
        plugin: Option<PathBuf>,
        /// The sound-to-patch-host binary [default: `SOUND_TO_PATCH_HOST`].
        #[arg(long)]
        host: Option<PathBuf>,
        /// How many measurements each row takes.
        #[arg(long, default_value_t = NonZeroUsize::new(20).unwrap_or(NonZeroUsize::MIN))]
        runs: NonZeroUsize,
        /// The JSON file to write.
        #[arg(long)]
        out: PathBuf,
    },
    /// Write a release version into the workspace manifest and the lock files.
    SetVersion {
        /// The new version, without a `v` prefix.
        version: String,
    },
}

fn main() -> ExitCode {
    let result = match Xtask::parse().task {
        Task::Dist { out_dir } => dist::write(&out_dir),
        Task::Fetch { url, sha256, out } => fetch::fetch(&url, &sha256, &out),
        Task::Eval { part } => eval::run(part.as_deref()),
        Task::Agreement { data } => agreement::run(root(), &data),
        Task::Golden => golden::generate(root()),
        Task::GoldenCheck => golden::check(root()),
        Task::CheckArchitecture => architecture::check(root()),
        Task::CheckNoOrt => lock::check_no_ort(root()),
        Task::CheckFuzzLock => lock::check_fuzz_lock(root()),
        Task::CheckCredits => credits::check(root()),
        Task::CheckPlugins => plugins::check(root()),
        Task::WordRanks { data } => ranks::run(root(), &data),
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
        Task::Reference { check } => reference::run(root(), check),
        Task::SetVersion { version } => version::set(root(), &version),
        Task::RenderTiming {
            preset,
            plugin,
            host,
            runs,
            out,
        } => {
            let preset = preset.unwrap_or_else(timing::default_preset);
            timing::run(&preset, plugin.as_deref(), host.as_deref(), runs, &out)
        }
        Task::VitalFixtures { check, coverage } => {
            if coverage {
                fixtures::coverage()
            } else {
                fixtures::run(root(), check)
            }
        }
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
