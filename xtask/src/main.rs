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
mod plugin;
mod plugins;
mod pr;
mod ranks;
mod reference;
mod spread;
mod timing;
mod version;
mod vital;
mod words;

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use sound_to_patch::render::session::RESTART_AFTER;
use sound_to_patch::render::spread::{MIN_RUNS, SPREAD_RUNS};
use sound_to_patch::units::Bpm;

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
    /// Score the notes step, or the envelope layer, on generated parts with
    /// known truth; local only.
    Eval {
        /// Run only the parts whose name holds this text, and print their
        /// notes.
        #[arg(long)]
        part: Option<String>,
        /// The layer to score: the notes step, or the envelope layer on
        /// parts with known envelopes and pumps.
        #[arg(long, value_enum, default_value_t = eval::Layer::Notes)]
        layer: eval::Layer,
        /// With `--layer envelope`, write one JSON record per part here.
        #[arg(long)]
        json: Option<PathBuf>,
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
    /// Check that `ARCHITECTURE.md` names every top-level module of `src/`
    /// and every workspace crate.
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
        #[command(flatten)]
        plugin: PluginArgs,
        /// How many measurements each row takes.
        #[arg(long, default_value_t = NonZeroUsize::new(20).unwrap_or(NonZeroUsize::MIN))]
        runs: NonZeroUsize,
        /// The JSON file to write.
        #[arg(long)]
        out: PathBuf,
    },
    /// Render a preset 20 times on the Vital plug-in and write the
    /// repeat-render spread of each output; local only.
    RenderSpread {
        /// The preset to render [default: `tests/fixtures/vital/routes.vital`].
        #[arg(long)]
        preset: Option<PathBuf>,
        /// The MIDI file [default: `tests/cmd/data/three-notes.mid`].
        #[arg(long)]
        midi: Option<PathBuf>,
        /// The tempo of the MIDI file in beats per minute.
        #[arg(long, default_value_t = 120.0)]
        bpm: f64,
        /// How many renders, drawn in sequence on one plug-in instance.
        #[arg(long, default_value_t = SPREAD_RUNS, value_parser = runs_parser())]
        runs: usize,
        #[command(flatten)]
        plugin: PluginArgs,
        /// The JSON file to write.
        #[arg(long)]
        out: PathBuf,
    },
    /// Compare reference recordings with renders of the same presets, MIDI
    /// files and tempos on the Vital plug-in, within the repeat-render
    /// spread; local only.
    RenderReference {
        /// The folder the manifest's paths start from.
        #[arg(long)]
        root: PathBuf,
        /// The manifest, one JSON object per recording.
        #[arg(long)]
        manifest: PathBuf,
        /// How many renders per recording.
        #[arg(long, default_value_t = SPREAD_RUNS, value_parser = runs_parser())]
        runs: usize,
        /// How many recordings render at once, each on its own plug-in
        /// instance.
        #[arg(long, default_value_t = WORKERS)]
        workers: NonZeroUsize,
        #[command(flatten)]
        plugin: PluginArgs,
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
        Task::Eval { part, layer, json } => match layer {
            eval::Layer::Notes if json.is_some() => {
                Err("--json works with --layer envelope only".into())
            }
            eval::Layer::Notes => eval::run(part.as_deref()),
            eval::Layer::Envelope => eval::envelope::run(part.as_deref(), json.as_deref()),
        },
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
            runs,
            out,
        } => plugin.find().and_then(|found| {
            let preset = preset.unwrap_or_else(timing::default_preset);
            timing::run(&preset, &found, runs, &out)
        }),
        Task::RenderSpread {
            preset,
            midi,
            bpm,
            runs,
            plugin,
            out,
        } => plugin.find().and_then(|found| {
            let preset = preset.unwrap_or_else(timing::default_preset);
            let midi = midi.unwrap_or_else(spread::default_midi);
            let run = spread::SpreadRun {
                preset: &preset,
                midi: &midi,
                bpm: Bpm::new(bpm)?,
                runs,
            };
            spread::run_spread(&run, &found, &out)
        }),
        Task::RenderReference {
            root,
            manifest,
            runs,
            workers,
            plugin,
            out,
        } => plugin.find().and_then(|found| {
            let run = spread::ReferenceRun {
                root: &root,
                manifest: &manifest,
                runs,
                workers,
            };
            spread::run_reference(&run, &found, &out)
        }),
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

/// The plug-in of a render task.
#[derive(Debug, clap::Args)]
struct PluginArgs {
    /// The Vital .vst3 bundle [default: the standard VST3 folders].
    #[arg(long)]
    plugin: Option<PathBuf>,
    /// The sound-to-patch-host binary [default: `SOUND_TO_PATCH_HOST`].
    #[arg(long)]
    host: Option<PathBuf>,
}

impl PluginArgs {
    fn find(&self) -> Result<plugin::Plugin> {
        plugin::Plugin::find(self.plugin.as_deref(), self.host.as_deref())
    }
}

/// The renders a spread may take: at least three for a leave-one-out
/// median of two distances, and no more than one plug-in instance renders
/// before the session restarts it.
fn runs_parser() -> clap::builder::RangedU64ValueParser<usize> {
    clap::builder::RangedU64ValueParser::new().range(MIN_RUNS_U64..=RESTART_AFTER)
}

/// [`MIN_RUNS`] for the range of [`runs_parser`].
#[allow(clippy::as_conversions, reason = "a const cast of a small constant")]
const MIN_RUNS_U64: u64 = MIN_RUNS as u64;
/// The recordings a reference run renders at once.
const WORKERS: NonZeroUsize = NonZeroUsize::new(4).unwrap();

/// The workspace root, whatever directory `cargo xtask` runs from.
fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

/// Writes `value` to `out` as pretty JSON with a final newline.
fn write_json(out: &Path, value: &serde_json::Value) -> Result<()> {
    let text = serde_json::to_string_pretty(value)? + "\n";
    std::fs::write(out, text)
        .map_err(|error| format!("cannot write {}: {error}", out.display()).into())
}

/// The error of any task: a message that says what failed and how to fix it.
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spread_takes_three_renders_up_to_the_restart() {
        for (runs, valid) in [("2", false), ("3", true), ("256", true), ("257", false)] {
            for args in [
                &["render-spread", "--out", "s.json"][..],
                &[
                    "render-reference",
                    "--root",
                    ".",
                    "--manifest",
                    "m",
                    "--out",
                    "r.json",
                ],
            ] {
                let parsed =
                    Xtask::try_parse_from(["xtask"].iter().chain(args).chain(&["--runs", runs]));
                assert_eq!(parsed.is_ok(), valid, "{args:?} --runs {runs}");
            }
        }
    }
}
