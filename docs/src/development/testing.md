# Testing

Tests follow the usual Rust layout. Agents write most of the code, so the
compiler, the lints and the tests must catch their typical mistakes.

## Layers

L0 is the compiler. L1 to L11 are tests. Each epic picks the layers it
needs from the matrix below and names them in its issue.

| Layer | What | Where |
| :-- | :-- | :-- |
| L0 | rustc, Clippy pedantic, rustdoc lints, units in types | `[workspace.lints]` in `Cargo.toml`, `clippy.toml` |
| L1 | Unit tests | `#[cfg(test)] mod tests` next to the code |
| L2 | Integration tests | the one test binary in `tests/it/`, one module per feature |
| L3 | Command-line cases | declarative files in `tests/cmd/`, and the console blocks of the README, the guide and the CLI reference, run by `trycmd` |
| L4 | Doc tests and runnable examples | rustdoc examples, `examples/` |
| L5 | Snapshots of the shape of our own output | `--help`, report and explanation templates, with `insta` or `trycmd` |
| L6 | Property tests | round trips, unit conversions, invariants |
| L7 | Golden numbers with explicit tolerances | JSON vectors in `tests/fixtures/golden/` |
| L8 | Fuzzing | `fuzz/`, for code that reads untrusted bytes |
| L9 | Benchmarks | `benches/`; CI only compiles them |
| L10 | Test-quality reports | mutation testing and coverage, at epic close |
| L11 | Dataset evaluation and the per-synth bench | `xtask`, local only |

`●` needed, `○` optional, `–` not applicable.

| Epic kind | L1 | L2 | L3 | L4 | L5 | L6 | L7 | L8 | L9 | L10 | L11 |
| :-- | :-: | :-: | :-: | :-: | :-: | :-: | :-: | :-: | :-: | :-: | :-: |
| Foundation | ○ | – | ● | – | ○ | – | – | – | – | – | – |
| Types and units | ● | – | – | ● | – | ● | – | – | – | ● | – |
| `preset` | ● | ● | ● | ● | ● | ● | ○ | ● | ○ | ● | ○ |
| `analyze` | ● | ● | ● | ● | ● | ● | ● | – | ○ | ○ | ● |
| `notes` | ● | ● | ● | ● | ○ | ○ | ● | – | ● | ○ | ● |
| `compare` | ● | ● | ● | ● | ○ | ● | ● | – | ● | ● | ● |
| `render` | ○ | ○ | ● | ● | – | – | ○ | – | ○ | – | ● |
| `refine` | ● | ● | ● | ● | ○ | ○ | ○ | – | ● | ○ | ● |
| Test bench | ● | ○ | – | – | ○ | ○ | – | – | – | – | ● |
| Skill and docs | – | – | ● | ● | ○ | – | – | – | – | – | ○ |

## Benchmarks

`cargo bench --bench notes` runs the benchmark harness groups of the notes step on
one generated 60 s stem: the model load, the model run at one thread and
at all threads, the f0 track, the MIDI write and the whole step at both
thread counts. A run takes about 30 minutes, most of it in the f0 track.
CI compiles the benches and never runs them.

The notes budget is the median plus 25 %, per minute of audio. The median
is the benchmark's own `median` estimate, from a run with nothing else
on the machine:

| Threads | Median | Budget |
| :-- | :-- | :-- |
| 1 | 30.05 s | 37.6 s |
| All | 29.17 s | 36.5 s |

A run above the budget names its step: the f0 track takes 29.31 s of the
30.05 s. `cargo run --release --example notes_memory -- <stage>` runs one
stage on the same stem, so a tool that reports a process's peak memory
measures each stage. The [pipeline page](../analysis/pipeline.md#time-budget)
gives every group.

Render timing needs the synth, so it is an `xtask` task, not a bench:
see [Render timing](#render-timing).

## Running the tests

```sh
cargo nextest run --workspace
cargo test --workspace --doc
```

nextest runs each test in its own process, so an in-process mutex does not
serialize tests; use a nextest test group instead. nextest does not run
doc tests, hence the second command. CI uses the `ci` profile from
`.config/nextest.toml`: no fail-fast and a slow-test timeout.

Test builds optimise our own crates at `opt-level = 1` (`[profile.test]` in
`Cargo.toml`), and the dependencies at `opt-level = 3`. The numeric tests
run FFTs and trackers over whole stems, so unoptimised code made the suite
take about 15 CPU-minutes. With the optimised build it takes about one,
and the doc tests reuse the same build. Debug assertions and overflow
checks stay on; `tests/it/profile.rs` fails if a change drops either from
the test profile of the root package. It does not check a
`[profile.test.package.<member>]` override. The weekly
coverage job builds with the test profile too. Optimised code can shift
the line attribution of a coverage report; that effect is not measured. Plain
`cargo build` and `cargo run` keep the unoptimised dev profile.

## Continuous integration

Three workflows run on a pull request.

| Workflow | Jobs | Starts on |
| :-- | :-- | :-- |
| `ci.yml` | `fmt`, `clippy`, `test`, `doc`, `licenses`, `docs`, `test-linux` | a pull request that is opened, pushed to or reopened, and a push to `main` |
| `commits.yml` | `commits` | the same events, and a change of the title or description |
| `codeql.yml` | `Analyze (rust)` | a pull request to `main`, a push to `main`, weekly, and by hand |

`commits` has its own workflow because it reads only the title and the
description. Editing them runs that one Linux job and none of the heavy
ones. The job has no `if:` and no path filter: a skipped job reports
success under its own name and would hide an earlier failure on the same
commit. The main ruleset requires `fmt`, `clippy`, `test`, `doc`,
`commits`, `licenses`, `docs` and `Analyze (rust)` by job name, so a job
keeps its name when it moves between workflows.

Each workflow cancels the run of an older push to the same pull request. A
push to `main` never cancels another run, because every merged commit
needs its checks and may need a release. The `release` and `pages` jobs
keep their own groups. They never cancel a running job, but a newer run
replaces an older job that still waits in the same group. A release run
covers every commit since the last tag, and `pages` deploys the newest
documentation, so a replaced job loses nothing.

A change of the base branch (`edited` with a new base) starts only
`commits.yml`. `codeql.yml` never ran on it. After retargeting a pull
request, reopen it or push a commit: both `ci.yml` and `codeql.yml` then run
against the new base. No trigger can tell a base change from a title edit
before a job starts, and a job-level `if:` that reacted to the retarget
would report skipped jobs, which hide earlier failures.

The build cache of the dependencies comes from `Swatinem/rust-cache`,
through `.github/actions/setup-rust`. A key holds the job, the lockfiles
and manifests, the compiler version and a hash of the `CARGO*`, `RUST*` and
`CC*` environment variables, so a new stable release starts a fresh cache
and the `env:` of `ci.yml` and `commits.yml` must stay equal. Only a run on `main` saves a cache. A pull request restores
the newest cache of `main` and compiles its own crates on top; it saves
nothing, so branches cannot fill the repository quota. The `docs`
and `commits` jobs share one Linux cache (`linux-xtask`), because they
build the same `xtask` with the same features. The `release` job uses no
cache, because it holds the deploy key. The `test` job builds
the dependencies twice, once for the tests and once for `xtask`, because
`cargo xtask` resolves fewer features than the test build. The cache holds
both builds.

## CI versus local

- A test that needs a synth (Vital or Serum 2), `vita` or a dataset
  carries `#[ignore = "needs <thing>"]`, such as `#[ignore = "needs Vital"]`.
  When its input is missing, it panics with a message that names the input
  and how to get it. Run such tests with `cargo nextest run --run-ignored only`.
- Never skip a test at run time with an early `return`. It reports a pass
  that never happened.
- Never gate a test behind a Cargo feature. CI builds with `--all-features`,
  so the gate would not hold, and a local run without the feature would
  silently drop the test.
- Datasets download through `cargo xtask fetch`, which keeps a file only
  when its SHA-256 matches the pin. Evaluations on datasets run through
  `xtask`, outside CI.

## Naming and assertions

- Name a test after the behaviour it checks, with no `test_` prefix:
  `hz_from_midi_69_is_440`.
- Do not use `#[should_panic]`. Return or match an `Err` and assert on it.
- `clippy::float_cmp` denies `==` on floats. Compare with a tolerance.
- Always pass both `epsilon` and `max_relative` to `approx`: its default
  tolerance is machine epsilon (`f32::EPSILON`, about `1.2e-7`, and
  `f64::EPSILON`, about `2.2e-16`), too tight for signal processing.
- Clippy allows `unwrap`, `expect` and `panic` inside `#[test]` functions and
  `#[cfg(test)]` modules only. A helper module of `tests/it/` is declared
  with `#[cfg(test)] mod support;` and its items are `pub(crate)`, because
  `unreachable_pub` denies `pub`.
- Examples and benches are not test code for Clippy. They return `Result`
  and use `?`.

## Test data

- Generated signals are the default input: sines, band-limited saws,
  detuned pairs, noise from a seeded generator written in the test code.
- A committed fixture is small and has a row in
  `tests/fixtures/PROVENANCE.md`: origin, version, license, copyright line,
  SHA-256, date and purpose. Its license text goes in the same file.
- Nothing licensed or private enters git: no factory presets, no renders of
  them, no user stems, no personal Init preset. No Git LFS.
- A Vital fixture is a file our code generated, or a file a maintainer
  saved from the app after loading a file we generated. The generated files
  come from `cargo xtask vital-fixtures`; after a change to the writers, run
  it and update the SHA-256 rows. `cargo xtask vital-fixtures --check`
  compares the committed files with the generator and checks every row,
  the size limit of 256 KiB, the licence section and the wavetable authors.
  `cargo xtask vital-fixtures --coverage` lists the registry keys no
  fixture holds.
- Expected values never come from an AI. They come from a formula in the
  facts, a file saved by the synth, a render or an independent reference.
- A snapshot accepts every diff as the new truth, so CI runs `insta` with
  `INSTA_UPDATE=no`. Snapshot the decoded values of a preset, never its
  bytes, and never a number that must match an outside reference. Review
  a changed snapshot like code; record one locally with
  `INSTA_UPDATE=always` or `TRYCMD=overwrite`.

A Markdown page whose commands write files keeps its inputs in a
`<page>.in/` folder and holds a `<page>.out/` folder. With the `.out` folder,
`trycmd` runs the commands in a copy of `.in`, so the files they write never
land in the repository. It compares only the files that `.out` holds, so a
copy of one input is enough.

## The open Vital engine

The open-engine tests run in the open Vital engine through the GPL-3.0
`vita` package, which runs only as a separate Python process. No crate of
ours depends on it. They need `VITA_PYTHON`, a Python with the packages of
`tools/vita/requirements.txt`, pinned with their hashes, made outside the
repository:

```sh
uv venv --python 3.12.14 /tmp/vita-env
uv pip install --python /tmp/vita-env/bin/python --require-hashes -r tools/vita/requirements.txt
VITA_PYTHON=/tmp/vita-env/bin/python cargo nextest run --test it --run-ignored only -E 'test(/open_engine/)'
```

The CI job `open-engine` runs the same commands on each pull request:

- `written_presets_load_in_the_open_engine` writes the load-check variants
  and loads each through `tools/vita/load_check.py`.
- `the_open_engine_passes_the_single_note_contract` runs the renderer
  contract suite on `render::vita::VitaRenderer`, with its single-note
  cases: the bass and lead notes, and a note at 1 s.
- Two more refuse a MIDI file with more than one note and pass the load
  check at 140 BPM.
- `the_open_engine_renders_a_job_identically_twice` pins that the open
  engine's spread is 0.
- `the_open_engine_renders_every_length_it_is_asked_for` renders 12
  onsets and lengths that are not whole milliseconds.
- `the_open_engine_script_renders_and_releases_at_the_frames_it_is_asked_for`
  sends render and note lengths straight to the script.
- `the_open_engine_refusal_of_a_preset_is_a_state_error` sends a preset
  that the engine does not load through the renderer, which reports a
  state error.
- `the_open_engine_prints_stay_off_the_reply` makes the engine write to
  file descriptor 1 as it starts, and the reply still parses.

The refusals of a job the engine cannot play, another synth's state, more
than one note, controller changes and a custom tuning, come before the
script runs. Unit tests of `render::vita` check them in every CI run.
Stub scripts, run with `python3`, check that exit code 3 maps to a state
error and any other failure to a host error, that a refused load leaves
no loaded state and no tempo, and that a clip of another length than
asked is a crash. Without `python3` the stub tests skip and say so.

`VitaRenderer` starts `tools/vita/render.py` once per request, with JSON
in and JSON out. The engine renders one note per call and takes no MIDI
file, so a job with more than one note or with controller changes is
refused. Each call starts with no sound left from the one before. The
renderer plays the recipe's warm-up note after the load and drops it, so
smoothed parameters settle. The engine has no call that renders silence,
so the recipe's settle steps add to that warm-up render's length. The
clip renders to the tail cap and ends where the recipe's tail watch ends
it. The engine reads no `tuning` from a preset, so a custom tuning is
refused. The script writes the job's tempo into `beats_per_minute`, as a
plug-in in a host takes its tempo from the host. Two renders of one
preset, each in its own process, come out identical, so the open engine's
spread is 0.

Probed on `vita` 0.1.0:

- Velocity 0 still sounds: a note at velocity 0 renders at an RMS of
  0.10. The renderer never sends one, since a note's velocity is 1 to
  127.
- `set_bpm` before the load leaves the saved `beats_per_minute` as the
  preset holds it: 2.0 after `set_bpm(140)`. After the load it saves the
  new tempo, 2.333 beats per second. The script calls it before the load
  and writes the tempo into the preset, so both agree.
- `load_init_preset` resets `beats_per_minute` to 2.0, 120 BPM, whatever
  `set_bpm` or an earlier preset set.
- The engine multiplies a length in seconds by the rate in float32 and
  truncates, so a length of n frames sent as n / rate can come back one
  frame short. At 400,000 to 520,000 frames it did for 16 of 121 lengths;
  that float32 product predicts each of them. The script passes
  `(n + 0.5) / rate` and checks the float32 product, and the renderer
  refuses a clip of another length than it asked for.
- The engine releases a note at the end of a block of 64 frames: a note
  of 64 k + 1 frames releases a block later than one of 64 k. With
  n / rate, a note of 64 k + 1 frames could truncate to 64 k and release
  a block early. `the_open_engine_script_renders_and_releases_at_the_frames_it_is_asked_for`
  sends render and note lengths that truncated that way straight to the
  script, so it fails if either goes back to n / rate.
- The engine's prints would mix into the JSON reply, so the script sends
  its standard output to standard error and writes the reply to the saved
  descriptor. A preset the engine does not load and samples that are not
  finite exit with code 3, which the renderer reports as a state error.
- The recipe's warm-up note changes the probe preset's clip by at most
  1.6e-7, about one float32 step, and a preset whose LFO retriggers on
  each note by as little. On a preset with an LFO in sync mode, which
  runs on the time the engine has rendered, it changes the clip by up to
  1.65, the LFO's whole phase. The renderer keeps the warm-up, so such a
  preset starts after the same pre-roll as on the plug-in.

The cross-engine check needs Vital, `SOUND_TO_PATCH_HOST` and
`VITA_PYTHON`: `render_vital::retail_and_vita_renders_compare_on_one_note`
renders the probe preset's lead note 20 times on retail, in sequence on
one instance, and once in the open engine. It compares the open render
with the retail ones against the retail tolerance and prints each
output. The levels must agree within coarse bounds, 0.1 dB on the RMS
level and the peak and 1.0 dB on the envelope distance, so a broken or
wrong render fails: an open render at -12 dB, or of the init patch,
fails by 12.0 dB and 4.0 dB. The engines differ in build, so a miss of
the retail limit is a finding about the engines, printed here, not a
failure of either.

`a_preset_saved_by_vital_keeps_the_edits` reads a file the retail app
saved after loading a written preset, from `VITAL_SAVED_PRESET`.

## Rendering

What runs in CI, with no plug-in:

- The renderer contract suite on the fake renderer, and two tests that show
  the suite fails a silent renderer and a renderer with one silent channel.
- The load check on the fake: a passing render, one case each for
  silence, an init fallback, a changed saved state and a plug-in fault,
  and the warning of an init-like render. A scripted host that crashes
  inside the load check gives the plug-in fault too.
- The `render` command's files on the fake: the `.wav` name in any case,
  an existing output without `--force`, a missing folder, a folder in
  place of an output, a read-only WAV file that `--force` replaces, and a
  read-only folder or a failed second write, which leave the existing
  files as they were.
- The render session on the fake: a crash, a hang or a panic tried once
  more on a new renderer with the same job, a render that fails twice
  giving the sentinel while the session goes on, a panic named with its
  job, a renderer that does not start, a wrong synth failing at once on
  the same renderer, a restart after 256 renders, one live renderer per
  worker, the flush before every render and the order of the outcomes. A
  stub host that exits on its first render is retried on a new host.
- The timing table on the fake: every row with a median and a 95th
  percentile, the build and the architecture, and the clip length of a
  long tail.
- The recipe, its snapshot and its hash, the note events, the tail watch,
  the MIDI reading with its hash, and the record against its schema.
- The wire protocol, the host's request loop, the architecture pick from
  Mach-O headers, and the WAV writer read back through the decoder.
- The `render` command cases in `tests/cmd/`, with the hidden
  `--backend fake`.
- Clippy on the host crate for `x86_64-apple-darwin`.

What needs Vital: the `render_vital` tests render generated presets
through the installed plug-in, in the plug-in host process. The host loads
the plug-in as a library and opens no window. They need:

- Vital in a standard VST3 folder: `/Library/Audio/Plug-Ins/VST3`, or the
  same folder under the user's `Library`.
- `SOUND_TO_PATCH_HOST`, the path of a `sound-to-patch-host` binary that
  loads Vital. Vital 1.0.7 holds x86_64 code only, so on Apple silicon the
  host needs an x86_64 slice and runs under Rosetta.
- `VITA_PYTHON` for one of them, the cross-engine check
  `retail_and_vita_renders_compare_on_one_note`, which also renders in the
  open engine. Without it that test fails at once and names the variable.

```sh
rustup target add x86_64-apple-darwin
cargo build --release -p sound-to-patch-host --target x86_64-apple-darwin
SOUND_TO_PATCH_HOST="$PWD/target/x86_64-apple-darwin/release/sound-to-patch-host" \
  cargo nextest run --run-ignored only -E 'test(render_vital)'
```

They run the contract suite, read the saved state back, check the forced
2x oversampling, and run the load check on a loaded preset, the init
patch, a slower attack on the init patch, a newer stamp, a preset with no
sound source, a dropped key and a bundle that is no plug-in. Two more send
a full pitch bend and a mod wheel move and measure their effect. A session
of two workers renders four presets of rising level in order, and the
timing table fills every row with two runs. Two fresh instances render the same first clip bit
for bit, and an instance's second render differs from its first.

What needs Serum 2: nothing yet. Its renderer follows in its own epic.

The failure paths of the host run in CI without a plug-in: unit tests in
`render::plugin` start shell scripts that stand in for the host. They
cover a host that exits, hangs, prints bytes that are not UTF-8 or a line
without its end, comes from another release, closes its pipes and
lingers, never answers `quit`, lingers after `quit`, or writes a clip that
does not decode. A unit test cuts plug-in output without line ends at
64 KB and reads a longer reply whole.

### Render timing

`cargo xtask render-timing --out "$TMPDIR/timing.json"` times the Vital
plug-in on this machine and writes the table to a local file, outside
the repository. Refine sets its render budget from it. The table stays
local: it describes the machine, so the repository holds the method and
no numbers. It takes the same
`SOUND_TO_PATCH_HOST` as the Vital tests, or `--host`. `--preset` names
the preset, by default the `routes.vital` fixture, and `--runs` the
measurements per row, by default 20.

Each row holds the median and the 95th percentile, linear between the
two nearest measurements:

- `plugin_load`: starting a renderer, the host process and the plug-in
  load included, in seconds.
- `state_load`: loading the preset state into the plug-in, timed in the
  host around the `vst3-host` state load, in seconds.
- `render`: one render of the recipe's lead note, MIDI 60 held 3 s, with
  the state load and the pre-roll, in seconds.
- `clip_length`: the length of that clip, at least 4 s and longer while
  the tail sounds.
- `renders_per_second`, for 1, 2, 4 and 8 workers: each measurement is
  one batch of one job per worker in a session whose workers have
  started. Its 95th percentile is the rate at the 95th percentile of the
  batch time, the slow end.

The table also names the synth, its build, the host, the architecture of
the host process, the recipe version and the number of runs. It measures
only: nothing in it tunes a render.

### Repeat-render spread

`cargo run --release -p xtask -- render-spread --out spread.json` renders
one preset 20 times and writes its spread, the noise floor every later
comparison cites. The
[render spread page](../reference/formats/render-spread.md) lists the
outputs, the statistic and the tolerance. `--preset`, `--midi` and
`--bpm` name what it renders, by default the `routes.vital` fixture and
`tests/cmd/data/three-notes.mid` at 120 BPM. The load check runs first on
a fresh renderer, so a spread never measures a preset the plug-in did
not load.

A fresh Vital instance renders bit-identically to any other fresh
instance, and each later render on one instance differs. So the 20
draws render in sequence on one fresh instance, through
`render::spread::draw`. A draw whose samples repeat another's, by their
SHA-256, fails the run: a repeat would count as a draw. `--runs` takes 3
to 256 renders, the most one instance renders before the session
restarts it. Each output's row counts the renders that lie within the
limit of the others, each held out in turn, and records two diagnostics
of the draws: the adjacent ratio and the lag-1 autocorrelation.

Each render is stored as a WAV file in a folder beside the output, named
after it with `-renders`, and measured from the stored file, as a
consumer reads it. `analyze` writes each render's layer report there,
with the MIDI file and the exact tempo. The file is local: the spread
depends on the synth's build and its quality setting. The repository
keeps no spread of record; the per-recording metrics are on the
[reference recordings](reference-recordings.md) page only.

On the fake, `render_spread` checks that a spread's 20 draws are
distinct, that a draw refuses fewer than 3 renders and more than 256,
and that the distinct check refuses the renders of two workers that
start alike. It recomputes the 95th percentile of the 190 pairs and the
largest leave-one-out median from 20 stored renders with its own
quantile and median. It checks that a render of the same preset matches
while a louder one does not. After the gain fit, a louder render and one
8 dB quieter match on the shape, the quieter one flagged beyond ±3 dB,
and a longer release does not match. Two more tests read the committed
reference report: one checks its ids, hashes, gains and verdicts, the
other recomputes every count, range and table of the reference page. On
Vital, `the_draws_of_a_spread_on_vital_are_distinct` draws 20 renders of
one lead note on the plug-in and finds all of them distinct.

### Reference recordings

`cargo run --release -p xtask -- render-reference --root DIR --manifest FILE --out report.json`
compares reference recordings with automatic renders. A reference
recording is a render of a generated preset and MIDI file made in a
standard DAW host, not by our host, so the comparison is not circular.
Each manifest line names the `preset_id`, the `preset_file`, the
`midi_file`, the `bpm` and the `render_file`, with paths from `--root`,
and a `state_file` when the DAW exported the state it played.

The task reads every manifest line first: the preset, the MIDI file, the
recording and the exported state. It refuses two lines with one id, the
preset id and the MIDI file's name, since both would write one renders
folder. Then, for each recording, it runs the load check, draws 20
renders of the preset with the same MIDI file and tempo, and measures
their spread. `--workers` recordings render at once, 4 by default, each
on its own fresh instance. The recording compares with the renders on
the clip outputs of the spread page, against the tolerance. An
exported state is read back against the preset as the load check does,
the tempo against the recording's tempo. The name and the tuning are
left out, because the DAW names the export and saves no tuning in a
preset file. A recording that differs beyond the tolerance is reported
with its numbers. Nothing is tuned to make it match.

Recordings and renders stay local. The repository holds the preset ids,
the hashes and the metrics of the comparison, on the
[reference recordings](reference-recordings.md) page.

The verdict comes after a gain fit, as the
[render spread page](../reference/formats/render-spread.md#comparing-with-the-spread)
describes: one gain per recording matches its level to the renders, and
the recording matches on the peak-to-RMS ratio, the duration and the
envelope. The report records each gain, flags one beyond ±3 dB, and
keeps the comparison as recorded on all five outputs for information.

## Fuzzing

The fuzz targets live in `fuzz/`, a workspace of its own pinned to a dated
nightly in `fuzz/rust-toolchain.toml`. CI lints them on stable, and a
scheduled job runs each one for five minutes. Run one locally from the
repository root, seeded from the fixtures:

```sh
mkdir -p fuzz/corpus/vital_read
cp tests/fixtures/vital/*.vital fuzz/corpus/vital_read/
cargo +nightly-2026-10-20 fuzz run vital_read -- -max_total_time=600
```

Seed `edits_apply`, which runs edit lists through `preset apply` on the
routes fixture, from `tests/cmd/data/*.json`. The corpus and the crash
artifacts stay out of git. A crash becomes a
unit test with the input bytes inline, next to the code it broke.

## Golden numbers

- The recipe of the test signal lives in code, in the JSON vector that the
  generator writes and the Rust test reads.
- The expected value comes from an independent reference: librosa, SciPy or
  a closed form, never our own code. The generator,
  `xtask/golden/generate.py`, computes it once. Its output is committed as
  JSON under `tests/fixtures/golden/` with the recipe, the generator, the
  tool versions, the values and the tolerance.
- `xtask/golden/.python-version` pins the interpreter and
  `xtask/golden/requirements.txt` pins every package with its hashes.
  `cargo xtask golden` installs them with uv into `target/golden-venv` and
  rewrites every vector. A second run with the same pins writes
  byte-identical files.
- `requirements.in` lists the direct packages. To change a pin, edit it and
  regenerate the lock from the repository root:

  ```bash
  uv pip compile xtask/golden/requirements.in --generate-hashes --python-version 3.14.7 --output-file xtask/golden/requirements.txt
  ```

  Add `--upgrade` to move every package to its latest release. The command
  also stands in the header of `requirements.txt`.
- CI never runs Python. The `test` job runs `cargo xtask golden-check`, which
  fails when a vector lacks a field, lacks the Python version in `tools`,
  names a version other than the pin, or when a prototype table has no
  recipe file. The same job runs `cargo xtask check-no-ort`, which fails
  when `Cargo.lock` holds the ONNX Runtime crate of the transcription
  model, and `cargo xtask check-fuzz-lock`, which fails when
  `fuzz/Cargo.lock` names another version of the crate than `Cargo.lock`.
- `cargo run --release -p xtask -- eval` scores the notes step on generated
  parts with known notes and prints its tables. It stays local, renders
  its audio on each run and commits nothing. Run it as a release build:
  the f0 tracker's generic code compiles at our crate's level, so a debug
  run is about 55 times slower. `--part TEXT` runs the matching parts and
  prints their notes, raw and clean. It fails when a golden of the
  cleanup breaks. The dry 110 Hz saw must show its octave copy at MIDI 57
  before the cleanup, and only true notes after it: 5 of its 6, since the
  model misses one onset. Its six notes through reverb must give the six
  true notes. Those stems take minutes in a debug build, so no test in CI
  runs them.
- Noise comes from SplitMix64, which the generator and
  `tests/it/support/rng.rs` implement bit for bit, so a noise recipe gives
  the same signal in both languages.
- Tests compare through `assert_close` in `tests/it/support/golden.rs` with
  the stored tolerance, `|actual - expected| <= abs + rel |expected|`, and
  name the source of the number in a comment.
- `tests/fixtures/golden/prototype/` holds the tables that task issues
  quote. Each is the issue's method run on a recipe given in the file; where
  the issue stated no recipe, the file's recipe defines it and its numbers
  replace the issue's.
- To change a reference: edit the generator or the pins, run
  `cargo xtask golden`, and commit the vectors with the change.

## Closing an epic

The epic issue holds a definition-of-done checklist. Two reports are read
at close. Both report and do not gate:

- Mutation testing on the new modules:
  `cargo mutants --test-tool nextest --in-place`. Do not set
  `RUSTFLAGS=-D warnings` for it: a deleted line trips `deny(unused)` and
  turns a mutant unviable.
- Coverage: `cargo llvm-cov nextest --workspace`, which needs the
  `llvm-tools-preview` component. The scheduled workflow publishes the same
  report every week. A coverage percentage is never a gate, because it
  rewards tests that run code without asserting.

Tests write only to temporary directories, because mutation testing runs
them against broken code.
