//! The repeat-render spread (#254) on the fake renderer: the statistic
//! recomputed from stored renders, and a comparison within and beyond it.

use std::num::NonZeroUsize;

use sound_to_patch::audio::{Audio, decode, write_wav};
use sound_to_patch::notes::Velocity;
use sound_to_patch::render::fake::{FakePatch, FakeRenderer};
use sound_to_patch::render::session::{BoxedRenderer, RESTART_AFTER, RenderSession};
use sound_to_patch::render::spread::{
    ClipMetrics, ClipOutput, FittedComparison, MIN_RUNS, SPREAD_RUNS, SpreadError, clip_rows,
    compare, compare_fitted, distinct, draw, envelope_distance, tolerance_rank,
};
use sound_to_patch::render::{Material, RenderJob, RenderNote, Rendered};
use sound_to_patch::synth::Synth;
use sound_to_patch::units::{Bpm, Seconds};

fn job(level: f64) -> RenderJob {
    job_with(level, 0.3)
}

fn job_with(level: f64, release: f64) -> RenderJob {
    let state = FakePatch {
        harmonics: vec![level, 0.5 * level, 0.25 * level],
        attack: Seconds::new(0.01).unwrap(),
        release: Seconds::new(release).unwrap(),
    }
    .state(Synth::Vital)
    .unwrap();
    RenderJob::for_material(state, Material::Lead, Bpm::new(120.0).unwrap())
}

fn fake(
    seed: u64,
) -> impl Fn(usize) -> Result<BoxedRenderer, sound_to_patch::render::RenderError> + Send + Sync + 'static
{
    move |_| Ok(Box::new(FakeRenderer::new(Synth::Vital, seed)) as BoxedRenderer)
}

/// Draws `count` renders of `job` from a fake, stores each clip as a WAV
/// file and returns the decoded files.
fn stored_audio(job: &RenderJob, count: usize, seed: u64) -> Vec<Audio> {
    let dir = tempfile::tempdir().unwrap();
    draw(fake(seed), job, count)
        .unwrap()
        .iter()
        .enumerate()
        .map(|(index, rendered)| {
            let path = dir.path().join(format!("render-{index:02}.wav"));
            write_wav(&path, &rendered.audio).unwrap();
            decode(&path).unwrap().into_audio()
        })
        .collect()
}

/// [`stored_audio`], measured.
fn stored_renders(job: &RenderJob, count: usize, seed: u64) -> Vec<ClipMetrics> {
    stored_audio(job, count, seed)
        .iter()
        .map(|audio| ClipMetrics::of(audio, "a render").unwrap())
        .collect()
}

/// numpy's default quantile, written out apart from the library's.
fn p95(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let last = u32::try_from(values.len() - 1).unwrap();
    let position = 0.95 * f64::from(last);
    let below = (0..=last)
        .rev()
        .find(|i| f64::from(*i) <= position)
        .unwrap();
    let above = (below + 1).min(last);
    let at = |i: u32| values[usize::try_from(i).unwrap()];
    at(below) + (at(above) - at(below)) * (position - f64::from(below))
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    if values.len() % 2 == 1 {
        values[mid]
    } else {
        f64::midpoint(values[mid - 1], values[mid])
    }
}

fn pairs(count: usize) -> impl Iterator<Item = (usize, usize)> {
    (0..count).flat_map(move |i| (i + 1..count).map(move |j| (i, j)))
}

#[test]
fn the_draws_of_a_spread_are_distinct() {
    let renders = draw(fake(1), &job(0.5), SPREAD_RUNS).unwrap();
    assert_eq!(renders.len(), SPREAD_RUNS);
    distinct(&renders).unwrap();
}

#[test]
fn a_draw_takes_three_renders_up_to_the_restart() {
    let at = |count: usize| draw(fake(1), &job(0.5), count).map(|renders| renders.len());
    assert_eq!(at(3).unwrap(), 3);
    // The most one instance renders before the session restarts it, on a
    // short note so that 256 renders stay quick.
    let short = {
        let note = RenderNote::new(
            60,
            Seconds::new(0.0).unwrap(),
            Seconds::new(0.01).unwrap(),
            Velocity::new(100).unwrap(),
        )
        .unwrap();
        let state = FakePatch {
            harmonics: vec![0.5],
            attack: Seconds::new(0.001).unwrap(),
            release: Seconds::new(0.01).unwrap(),
        }
        .state(Synth::Vital)
        .unwrap();
        RenderJob::new(state, vec![note], Bpm::new(120.0).unwrap()).unwrap()
    };
    let most = usize::try_from(RESTART_AFTER).unwrap();
    assert_eq!(draw(fake(1), &short, most).unwrap().len(), most);
    for count in [0, 2, usize::try_from(RESTART_AFTER).unwrap() + 1] {
        let error = at(count).unwrap_err();
        assert!(
            matches!(error, SpreadError::Runs(n) if n == count),
            "{error}"
        );
        assert!(
            error
                .to_string()
                .starts_with(&format!("a spread takes 3 to 256 renders, not {count}")),
            "{error}"
        );
    }
}

#[test]
fn renders_of_identical_workers_fail_the_distinct_check() {
    // Two workers that start alike render alike, as fresh Vital instances
    // do: the n-th render of each is the same clip.
    let session = RenderSession::new(NonZeroUsize::new(2).unwrap(), fake(1));
    session.start_all().unwrap();
    let renders: Vec<Rendered> = session
        .render_all(&vec![job(0.5); 4])
        .into_iter()
        .map(|outcome| outcome.rendered().unwrap().clone())
        .collect();
    // How the jobs split between the workers sets how many repeat.
    let error = distinct(&renders).unwrap_err();
    assert!(
        matches!(
            error,
            SpreadError::Repeated {
                distinct,
                renders: 4
            } if distinct < 4
        ),
        "{error}"
    );
    assert!(error.to_string().contains("of 4 renders differ"), "{error}");
}

#[test]
fn the_spread_is_recomputed_from_stored_renders() {
    assert_eq!(SPREAD_RUNS, 20);
    let clips = stored_renders(&job(0.5), SPREAD_RUNS, 1);
    let rows = clip_rows(&clips).unwrap();
    let names: Vec<&str> = rows.iter().map(|row| row.output.as_str()).collect();
    assert_eq!(
        names,
        [
            "rms_db",
            "peak_db",
            "peak_to_rms_db",
            "duration_s",
            "envelope_distance_db"
        ]
    );
    assert_eq!(pairs(clips.len()).count(), 190);
    let distance = |i: usize, j: usize| ClipOutput::RmsDb.distance(&clips[i], &clips[j]);
    let rms = p95(pairs(clips.len()).map(|(i, j)| distance(i, j)).collect());
    let medians: Vec<f64> = (0..clips.len())
        .map(|i| {
            median(
                (0..clips.len())
                    .filter(|j| *j != i)
                    .map(|j| distance(i, j))
                    .collect(),
            )
        })
        .collect();
    let envelope = p95(pairs(clips.len())
        .map(|(i, j)| envelope_distance(&clips[i], &clips[j]))
        .collect());
    assert!(
        (rows[0].spread - rms).abs() < 1e-12,
        "{} {rms}",
        rows[0].spread
    );
    // The rank ⌈0.95 × 21⌉ of 20 medians is the largest.
    assert_eq!(tolerance_rank(SPREAD_RUNS), SPREAD_RUNS);
    let largest = medians.iter().copied().fold(0.0, f64::max);
    assert!((rows[0].tolerance - largest).abs() < 1e-12);
    assert!((rows[4].spread - envelope).abs() < 1e-12);
    // The fake's random phases and level jitter make the renders differ.
    assert!(rows[0].spread > 0.0 && rows[0].spread < 0.2, "{rows:?}");
    assert!(rows[0].tolerance < rows[0].spread, "{rows:?}");
    assert_eq!(rows[0].unit, "dB");
    assert_eq!(rows[3].unit, "s");
}

#[test]
fn a_render_of_the_preset_matches_and_a_louder_one_does_not() {
    let renders = stored_renders(&job(0.5), SPREAD_RUNS, 1);
    let rows = clip_rows(&renders).unwrap();
    let same = stored_renders(&job(0.5), MIN_RUNS, 99).remove(0);
    for comparison in compare(&same, &renders, &rows).unwrap() {
        assert!(comparison.within, "{comparison:?}");
        assert!(comparison.ratio <= 1.0, "{comparison:?}");
    }
    let louder = stored_renders(&job(0.6), MIN_RUNS, 99).remove(0);
    let comparisons = compare(&louder, &renders, &rows).unwrap();
    let rms = comparisons.iter().find(|c| c.output == "rms_db").unwrap();
    assert!(!rms.within && rms.ratio > 1.0, "{rms:?}");
    // 0.6 against 0.5 is 1.58 dB.
    assert!((rms.difference - 1.58).abs() < 0.1, "{rms:?}");
    let envelope = comparisons
        .iter()
        .find(|c| c.output == "envelope_distance_db")
        .unwrap();
    assert!(!envelope.within, "{envelope:?}");
}

#[test]
fn a_fitted_gain_takes_out_the_level_and_keeps_the_shape() {
    let renders = stored_renders(&job(0.5), SPREAD_RUNS, 1);
    let rows = clip_rows(&renders).unwrap();
    let recording = |job: RenderJob| stored_audio(&job, MIN_RUNS, 99).remove(0);
    let outputs = |fitted: &FittedComparison| -> Vec<String> {
        fitted
            .comparisons
            .iter()
            .map(|c| c.output.clone())
            .collect()
    };
    // 0.6 against 0.5 is 1.58 dB louder: the fit takes it out, and the
    // shape matches.
    let louder = compare_fitted(&recording(job(0.6)), "louder", &renders, &rows).unwrap();
    assert!((louder.gain_db + 1.58).abs() < 0.1, "{louder:?}");
    assert!(!louder.gain_beyond_range && louder.within, "{louder:?}");
    assert_eq!(
        outputs(&louder),
        ["peak_to_rms_db", "duration_s", "envelope_distance_db"]
    );
    // 0.2 against 0.5 is 7.96 dB quieter: a finding, and still a match.
    let quiet = compare_fitted(&recording(job(0.2)), "quiet", &renders, &rows).unwrap();
    assert!((quiet.gain_db - 7.96).abs() < 0.1, "{quiet:?}");
    assert!(quiet.gain_beyond_range && quiet.within, "{quiet:?}");
    // A longer release changes the shape, which no gain takes out.
    let longer = compare_fitted(&recording(job_with(0.5, 0.6)), "longer", &renders, &rows).unwrap();
    assert!(!longer.within, "{longer:?}");
    let duration = &longer.comparisons[1];
    assert!(!duration.within && duration.ratio > 1.0, "{duration:?}");
}

/// Every clip output, as the comparison as recorded reports them.
const OUTPUTS: [&str; 5] = [
    "rms_db",
    "peak_db",
    "peak_to_rms_db",
    "duration_s",
    "envelope_distance_db",
];
/// The outputs of the sound's shape, which the verdict compares after the
/// gain fit.
const SHAPE: [&str; 3] = ["peak_to_rms_db", "duration_s", "envelope_distance_db"];
/// The split between a recording that ends a little early and one that
/// ends early by a tail, in s.
const LONG_S: f64 = 0.1;

type Row = serde_json::Value;

fn references() -> Vec<Row> {
    let path = "docs/src/development/reference-recordings/report.json";
    let report: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    report["references"].as_array().unwrap().clone()
}

/// The comparison of `output`, after the gain fit or as recorded.
fn comparison<'a>(row: &'a Row, output: &str, fitted: bool) -> &'a serde_json::Value {
    let comparisons = if fitted {
        &row["comparisons"]
    } else {
        &row["as_recorded"]["comparisons"]
    };
    comparisons
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["output"] == output)
        .unwrap()
}

fn spread_row<'a>(row: &'a Row, output: &str) -> &'a serde_json::Value {
    row["spread"]["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["output"] == output)
        .unwrap()
}

fn number(value: &serde_json::Value) -> f64 {
    value.as_f64().unwrap()
}

fn within(row: &Row, output: &str, fitted: bool) -> bool {
    comparison(row, output, fitted)["within"].as_bool().unwrap()
}

fn names(comparisons: &serde_json::Value) -> Vec<&str> {
    comparisons
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["output"].as_str().unwrap())
        .collect()
}

#[test]
fn the_reference_report_holds_ids_hashes_and_consistent_verdicts() {
    let rows = references();
    assert_eq!(rows.len(), 38);
    let mut ids = std::collections::HashSet::new();
    for row in &rows {
        let id = row["id"].as_str().unwrap();
        assert!(ids.insert(id), "{id} twice");
        assert!(id.starts_with(row["preset_id"].as_str().unwrap()), "{id}");
        let spread = &row["spread"];
        assert_eq!(spread["n"], 20, "{id}");
        assert_eq!(spread["statistic"], "p95_pairwise_distance", "{id}");
        assert_eq!(
            spread["tolerance"], "conformal_95_leave_one_out_median",
            "{id}"
        );
        assert!(
            (number(&spread["coverage"]) - 20.0 / 21.0).abs() < 1e-12,
            "{id}"
        );
        for hex in [
            &spread["preset_sha256"],
            &spread["midi_sha256"],
            &row["recording_sha256"],
        ] {
            let hex = hex.as_str().unwrap();
            assert!(
                hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()),
                "{id}: {hex}"
            );
        }
        let gain = number(&row["gain_db"]);
        assert_eq!(
            row["gain_beyond_range"].as_bool().unwrap(),
            gain.abs() > 3.0,
            "{id}"
        );
        assert_eq!(names(&row["comparisons"]), SHAPE, "{id}");
        assert_eq!(names(&row["as_recorded"]["comparisons"]), OUTPUTS, "{id}");
        for (fitted, outputs, verdict) in [
            (true, &SHAPE[..], &row["within"]),
            (false, &OUTPUTS[..], &row["as_recorded"]["within"]),
        ] {
            let mut all = true;
            for output in outputs {
                let c = comparison(row, output, fitted);
                let srow = spread_row(row, output);
                assert!(number(&srow["floor"]) > 0.0, "{id} {output}");
                assert!(
                    srow["held_out_within"].as_u64().unwrap() <= 20,
                    "{id} {output}"
                );
                let limit = number(&srow["tolerance"]).max(number(&srow["floor"]));
                assert!((number(&c["limit"]) - limit).abs() < 1e-12, "{id} {output}");
                let inside = number(&c["difference"]) <= limit;
                assert_eq!(c["within"].as_bool().unwrap(), inside, "{id} {output}");
                all &= inside;
            }
            assert_eq!(verdict.as_bool().unwrap(), all, "{id}");
        }
    }
}

/// The counts, ranges and the table of the reference recordings page,
/// recomputed from the report.
fn page_lines() -> Vec<String> {
    let rows = references();
    let mut lines = count_lines(&rows);
    lines.extend(gain_lines(&rows));
    lines.extend(duration_lines(&rows));
    lines.extend(miss_lines(&rows));
    lines.extend(draw_lines(&rows));
    lines.extend(table_lines(&rows));
    lines
}

/// "a, b and c".
fn join(parts: &[String]) -> String {
    match parts.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        Some((last, _)) => last.clone(),
        None => String::new(),
    }
}

fn label(output: &str) -> &'static str {
    match output {
        "rms_db" => "the RMS level",
        "peak_db" => "the peak",
        "peak_to_rms_db" => "the peak-to-RMS ratio",
        "duration_s" => "the duration",
        _ => "the envelope",
    }
}

fn count_lines(rows: &[Row]) -> Vec<String> {
    let counts = |outputs: &[&str], fitted: bool| -> Vec<String> {
        outputs
            .iter()
            .map(|output| {
                let count = rows
                    .iter()
                    .filter(|row| within(row, output, fitted))
                    .count();
                format!("{} on {count}", label(output))
            })
            .collect()
    };
    let fitted = rows
        .iter()
        .filter(|row| row["within"].as_bool().unwrap())
        .count();
    let recorded = rows
        .iter()
        .filter(|row| row["as_recorded"]["within"].as_bool().unwrap())
        .count();
    vec![
        format!(
            "- After the gain fit, {fitted} of 38 recordings match the renders on all three outputs of the sound's shape: {}.",
            join(&counts(&SHAPE, true))
        ),
        format!(
            "- As recorded, for information, {recorded} of 38 lie within the limit on all five outputs: {}.",
            join(&counts(&OUTPUTS, false))
        ),
    ]
}

fn range(values: &[f64]) -> (f64, f64) {
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    (min, max)
}

fn gain_lines(rows: &[Row]) -> Vec<String> {
    let gains: Vec<f64> = rows.iter().map(|row| number(&row["gain_db"])).collect();
    let (min, max) = range(&gains);
    let beyond: Vec<String> = rows
        .iter()
        .filter(|row| row["gain_beyond_range"].as_bool().unwrap())
        .map(|row| {
            format!(
                "`{}`, {:+.2} dB",
                row["id"].as_str().unwrap(),
                number(&row["gain_db"])
            )
        })
        .collect();
    let beyond = if beyond.is_empty() {
        "None lies beyond ±3 dB.".to_owned()
    } else {
        format!("{} lie beyond ±3 dB: {}.", beyond.len(), join(&beyond))
    };
    vec![format!(
        "- The fitted gains lie from {min:+.2} to {max:+.2} dB, with a median of {:+.2} dB. {beyond}",
        median(gains.clone())
    )]
}

/// A recording's duration after the gain minus the renders' median, in s,
/// and its verdict.
fn duration(row: &Row) -> (f64, bool) {
    let c = comparison(row, "duration_s", true);
    (
        number(&c["clip"]) - number(&c["renders"]),
        c["within"].as_bool().unwrap(),
    )
}

fn duration_lines(rows: &[Row]) -> Vec<String> {
    let earlier: Vec<f64> = rows
        .iter()
        .map(duration)
        .filter(|(d, within)| *d < 0.0 && !within)
        .map(|(d, _)| -d)
        .collect();
    let later: Vec<String> = rows
        .iter()
        .filter_map(|row| {
            let (d, within) = duration(row);
            (d > 0.0 && !within)
                .then(|| format!("{:.1} ms (`{}`)", d * 1000.0, row["id"].as_str().unwrap()))
        })
        .collect();
    let long: Vec<f64> = earlier.iter().copied().filter(|d| *d >= LONG_S).collect();
    let short: Vec<f64> = earlier.iter().copied().filter(|d| *d < LONG_S).collect();
    let (long_min, long_max) = range(&long);
    let (short_min, short_max) = range(&short);
    let mut lines = vec![
        format!(
            "  - {} recordings end earlier than the renders, beyond the limit, and {} end later.",
            earlier.len(),
            later.len()
        ),
        format!(
            "  - {} of the earlier ones end {LONG_S} s or more early, from {long_min:.2} to {long_max:.2} s. The other {} end {:.1} to {:.1} ms early.",
            long.len(),
            short.len(),
            short_min * 1000.0,
            short_max * 1000.0,
        ),
    ];
    if !later.is_empty() {
        lines.push(format!("  - The later ones end late by {}.", join(&later)));
    }
    lines
}

/// Which outputs miss together after the gain fit.
fn miss_lines(rows: &[Row]) -> Vec<String> {
    let beyond = |row: &Row, output: &str| !within(row, output, true);
    let duration_only = rows
        .iter()
        .filter(|row| {
            beyond(row, "duration_s")
                && !beyond(row, "envelope_distance_db")
                && !beyond(row, "peak_to_rms_db")
        })
        .count();
    let envelope: Vec<&Row> = rows
        .iter()
        .filter(|row| beyond(row, "envelope_distance_db"))
        .collect();
    let both = envelope
        .iter()
        .filter(|row| beyond(row, "duration_s"))
        .count();
    let ratio = |row: &Row, output: &str| number(&comparison(row, output, true)["ratio"]);
    let (low, high) = range(
        &envelope
            .iter()
            .map(|row| ratio(row, "envelope_distance_db"))
            .collect::<Vec<_>>(),
    );
    let crest: Vec<String> = rows
        .iter()
        .filter(|row| beyond(row, "peak_to_rms_db"))
        .map(|row| {
            format!(
                "`{}`, at a ratio of {:.2}, and {:.2} on the envelope",
                row["id"].as_str().unwrap(),
                ratio(row, "peak_to_rms_db"),
                ratio(row, "envelope_distance_db")
            )
        })
        .collect();
    vec![
        format!("- {duration_only} recordings miss on the duration alone."),
        format!(
            "- {} recordings lie beyond the limit on the envelope, at ratios from {low:.2} to {high:.2}. {both} of them also lie beyond it on the duration.",
            envelope.len()
        ),
        format!(
            "- The peak-to-RMS ratio lies beyond the limit on {}: {}.",
            crest.len(),
            join(&crest)
        ),
    ]
}

/// The checks of the draws: the held-out coverage of the tolerance and the
/// two diagnostics, over every recording's renders.
fn draw_lines(rows: &[Row]) -> Vec<String> {
    let cases = rows.len() * 20;
    let held_out: Vec<String> = OUTPUTS
        .iter()
        .map(|output| {
            let within: u64 = rows
                .iter()
                .map(|row| spread_row(row, output)["held_out_within"].as_u64().unwrap())
                .sum();
            format!(
                "{:.1} % on {}",
                100.0 * f64::from(u32::try_from(within).unwrap())
                    / f64::from(u32::try_from(cases).unwrap()),
                label(output)
            )
        })
        .collect();
    let diagnostic = |field: &str, outputs: &[&str]| {
        join(
            &outputs
                .iter()
                .map(|output| {
                    let values: Vec<f64> = rows
                        .iter()
                        .filter_map(|row| spread_row(row, output)[field].as_f64())
                        .collect();
                    let (min, max) = range(&values);
                    format!(
                        "{:.2} on {} ({min:.2} to {max:.2})",
                        median(values.clone()),
                        label(output)
                    )
                })
                .collect::<Vec<_>>(),
        )
    };
    vec![
        format!(
            "- Each render, held out in turn and compared with the other 19 as a clip, lies within their limit at these rates over the {cases} cases: {}. The rank of 19 renders covers 19 in 20, 95.0 %.",
            join(&held_out)
        ),
        format!(
            "- The lag-1 autocorrelation of each set of 20 has a median of {}.",
            diagnostic("lag1_autocorrelation", &OUTPUTS[..4])
        ),
        format!(
            "- The adjacent ratio has a median of {}.",
            diagnostic("adjacent_ratio", &OUTPUTS)
        ),
    ]
}

fn table_lines(rows: &[Row]) -> Vec<String> {
    let ratios = |row: &Row, outputs: &[&str], fitted: bool| {
        outputs
            .iter()
            .map(|output| format!("{:.2}", number(&comparison(row, output, fitted)["ratio"])))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    rows.iter()
        .map(|row| {
            format!(
                "| `{}` | {} | {:+.2} | {:+.1} | {} | {} |",
                row["id"].as_str().unwrap(),
                number(&row["spread"]["bpm"]),
                number(&row["gain_db"]),
                duration(row).0 * 1000.0,
                ratios(row, &SHAPE, true),
                ratios(row, &OUTPUTS, false),
            )
        })
        .collect()
}

/// The text with every run of white space as one space, so a line of
/// prose matches however the page wraps it.
fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn the_reference_page_counts_follow_the_report() {
    let page =
        flat(&std::fs::read_to_string("docs/src/development/reference-recordings.md").unwrap());
    let lines = page_lines();
    let missing: Vec<&String> = lines
        .iter()
        .filter(|line| !page.contains(&flat(line)))
        .collect();
    assert!(
        missing.is_empty(),
        "the page lacks {} lines; the report gives:\n{}",
        missing.len(),
        lines.join("\n")
    );
}
