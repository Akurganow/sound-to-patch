//! The repeat-render spread (#254) on the fake renderer: the statistic
//! recomputed from stored renders, and a comparison within and beyond it.

use std::num::NonZeroUsize;

use sound_to_patch::audio::{decode, write_wav};
use sound_to_patch::render::fake::{FakePatch, FakeRenderer};
use sound_to_patch::render::session::{BoxedRenderer, RenderSession};
use sound_to_patch::render::spread::{
    ClipMetrics, ClipOutput, SPREAD_RUNS, SpreadError, clip_rows, compare, distinct, draw,
    envelope_distance,
};
use sound_to_patch::render::{Material, RenderJob, Rendered};
use sound_to_patch::synth::Synth;
use sound_to_patch::units::{Bpm, Seconds};

fn job(level: f64) -> RenderJob {
    let state = FakePatch {
        harmonics: vec![level, 0.5 * level, 0.25 * level],
        attack: Seconds::new(0.01).unwrap(),
        release: Seconds::new(0.3).unwrap(),
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
/// file and measures the decoded files.
fn stored_renders(job: &RenderJob, count: usize, seed: u64) -> Vec<ClipMetrics> {
    let dir = tempfile::tempdir().unwrap();
    draw(fake(seed), job, count)
        .unwrap()
        .iter()
        .enumerate()
        .map(|(index, rendered)| {
            let path = dir.path().join(format!("render-{index:02}.wav"));
            write_wav(&path, &rendered.audio).unwrap();
            ClipMetrics::of(&decode(&path).unwrap().into_audio(), "a render").unwrap()
        })
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
    let error = distinct(&renders).unwrap_err();
    assert!(
        matches!(
            error,
            SpreadError::Repeated {
                distinct: 2,
                renders: 4
            }
        ),
        "{error}"
    );
    assert!(
        error.to_string().starts_with("only 2 of 4 renders differ"),
        "{error}"
    );
}

#[test]
fn the_spread_is_recomputed_from_stored_renders() {
    assert_eq!(SPREAD_RUNS, 20);
    let clips = stored_renders(&job(0.5), SPREAD_RUNS, 1);
    let rows = clip_rows(&clips).unwrap();
    let names: Vec<&str> = rows.iter().map(|row| row.output.as_str()).collect();
    assert_eq!(
        names,
        ["rms_db", "peak_db", "duration_s", "envelope_distance_db"]
    );
    assert_eq!(pairs(clips.len()).count(), 190);
    let distance = |i: usize, j: usize| ClipOutput::RmsDb.distance(&clips[i], &clips[j]);
    let rms = p95(pairs(clips.len()).map(|(i, j)| distance(i, j)).collect());
    let medians = (0..clips.len())
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
    assert!((rows[0].tolerance - p95(medians)).abs() < 1e-12);
    assert!((rows[3].spread - envelope).abs() < 1e-12);
    // The fake's random phases and level jitter make the renders differ.
    assert!(rows[0].spread > 0.0 && rows[0].spread < 0.2, "{rows:?}");
    assert!(rows[0].tolerance < rows[0].spread, "{rows:?}");
    assert_eq!(rows[0].unit, "dB");
    assert_eq!(rows[2].unit, "s");
}

#[test]
fn a_render_of_the_preset_matches_and_a_louder_one_does_not() {
    let renders = stored_renders(&job(0.5), SPREAD_RUNS, 1);
    let rows = clip_rows(&renders).unwrap();
    let same = stored_renders(&job(0.5), 1, 99).remove(0);
    for comparison in compare(&same, &renders, &rows).unwrap() {
        assert!(comparison.within, "{comparison:?}");
        assert!(comparison.ratio <= 1.0, "{comparison:?}");
    }
    let louder = stored_renders(&job(0.6), 1, 99).remove(0);
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

const REPORTS: [(&str, f64, &str); 2] = [
    ("report.json", 0.0, "within"),
    ("report-gain.json", 2.16, "within_with_gain"),
];
const OUTPUTS: [&str; 4] = ["rms_db", "peak_db", "duration_s", "envelope_distance_db"];

fn report(file: &str) -> serde_json::Value {
    let path = format!("docs/src/development/reference-recordings/{file}");
    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap()
}

fn comparison<'a>(row: &'a serde_json::Value, output: &str) -> &'a serde_json::Value {
    row["comparisons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["output"] == output)
        .unwrap()
}

fn number(value: &serde_json::Value) -> f64 {
    value.as_f64().unwrap()
}

#[test]
fn the_reference_reports_hold_ids_hashes_and_consistent_verdicts() {
    for (file, gain, verdict) in REPORTS {
        let report = report(file);
        assert_eq!(report["recording_gain_db"], gain, "{file}");
        let rows = report["references"].as_array().unwrap();
        assert_eq!(rows.len(), 38, "{file}");
        let mut ids = std::collections::HashSet::new();
        for row in rows {
            let id = row["id"].as_str().unwrap();
            assert!(ids.insert(id), "{file}: {id} twice");
            assert!(id.starts_with(row["preset_id"].as_str().unwrap()), "{id}");
            let spread = &row["spread"];
            assert_eq!(spread["n"], 20, "{id}");
            assert_eq!(spread["statistic"], "p95_pairwise_distance", "{id}");
            assert_eq!(spread["tolerance"], "p95_leave_one_out_median", "{id}");
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
            let outputs: Vec<&str> = row["comparisons"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c["output"].as_str().unwrap())
                .collect();
            assert_eq!(outputs, OUTPUTS, "{id}");
            let mut all = true;
            for output in OUTPUTS {
                let c = comparison(row, output);
                let srow = spread["outputs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|r| r["output"] == output)
                    .unwrap();
                let limit = number(&srow["tolerance"]).max(number(&srow["floor"]));
                assert!((number(&c["limit"]) - limit).abs() < 1e-12, "{id} {output}");
                let within = number(&c["difference"]) <= limit;
                assert_eq!(c[verdict].as_bool().unwrap(), within, "{id} {output}");
                assert!(
                    c.get(if gain == 0.0 {
                        "within_with_gain"
                    } else {
                        "within"
                    })
                    .is_none()
                );
                all &= within;
            }
            assert_eq!(row[verdict].as_bool().unwrap(), all, "{id}");
        }
    }
}

type Rows = Vec<serde_json::Value>;

/// The counts and tables of the reference recordings page, recomputed from
/// the two reports.
fn page_lines() -> Vec<String> {
    let rows = |file: &str| report(file)["references"].as_array().unwrap().clone();
    let (raw, gain) = (rows(REPORTS[0].0), rows(REPORTS[1].0));
    let mut lines = count_lines(&raw, &gain);
    lines.extend(duration_lines(&raw, &gain));
    lines.extend(table_lines(&raw, &gain));
    lines
}

fn count_lines(raw: &Rows, gain: &Rows) -> Vec<String> {
    let mut lines = Vec::new();
    for (name, rows, verdict) in [
        ("As recorded", raw, "within"),
        (
            "With the recordings raised by 2.16 dB",
            gain,
            "within_with_gain",
        ),
    ] {
        let count = |output: &str| {
            rows.iter()
                .filter(|row| comparison(row, output)[verdict].as_bool().unwrap())
                .count()
        };
        let all = rows
            .iter()
            .filter(|row| row[verdict].as_bool().unwrap())
            .count();
        lines.push(format!(
            "- {name}, {all} of 38 recordings lie within the limit on all four outputs: the RMS level on {}, the peak on {}, the duration on {} and the envelope on {}.",
            count("rms_db"),
            count("peak_db"),
            count("duration_s"),
            count("envelope_distance_db"),
        ));
    }
    lines
}

/// A recording's duration minus the renders' median, in s, and its
/// verdict as recorded.
fn duration(row: &serde_json::Value) -> (f64, bool) {
    let c = comparison(row, "duration_s");
    (
        number(&c["clip"]) - number(&c["renders"]),
        c["within"].as_bool().unwrap(),
    )
}

fn duration_lines(raw: &Rows, gain: &Rows) -> Vec<String> {
    let mut lines = Vec::new();
    let earlier_rows: Vec<usize> = (0..raw.len())
        .filter(|at| {
            let (d, within) = duration(&raw[*at]);
            d < 0.0 && !within
        })
        .collect();
    let earlier: Vec<f64> = earlier_rows
        .iter()
        .map(|at| -duration(&raw[*at]).0)
        .collect();
    let within_with_gain = |long: bool| {
        earlier_rows
            .iter()
            .filter(|at| {
                (-duration(&raw[**at]).0 >= 0.1) == long
                    && comparison(&gain[**at], "duration_s")["within_with_gain"]
                        .as_bool()
                        .unwrap()
            })
            .count()
    };
    let later: Vec<(&str, f64)> = raw
        .iter()
        .filter_map(|row| {
            let (d, within) = duration(row);
            (d > 0.0 && !within).then(|| (row["id"].as_str().unwrap(), d))
        })
        .collect();
    let long: Vec<f64> = earlier.iter().copied().filter(|d| *d >= 0.1).collect();
    let short: Vec<f64> = earlier.iter().copied().filter(|d| *d < 0.1).collect();
    let range = |values: &[f64], scale: f64| {
        let min = values.iter().copied().fold(f64::INFINITY, f64::min) * scale;
        let max = values.iter().copied().fold(0.0, f64::max) * scale;
        (min, max)
    };
    let (long_min, long_max) = range(&long, 1.0);
    let (short_min, short_max) = range(&short, 1000.0);
    lines.push(format!(
        "- As recorded, {} recordings end earlier than the renders, beyond the limit, and {} end later.",
        earlier.len(),
        later.len()
    ));
    lines.push(format!(
        "- {} of the earlier ones end {long_min:.2} to {long_max:.2} s early. The other {} end {short_min:.1} to {short_max:.1} ms early.",
        long.len(),
        short.len()
    ));
    lines.push(format!(
        "- With the recordings raised by 2.16 dB, the duration lies within the limit on {} of the {} and on {} of the {}.",
        within_with_gain(true),
        long.len(),
        within_with_gain(false),
        short.len()
    ));
    lines.push(format!(
        "- The later ones end {} late.",
        later
            .iter()
            .map(|(id, d)| format!("`{id}` by {:.1} ms", d * 1000.0))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    lines
}

fn table_lines(raw: &Rows, gain: &Rows) -> Vec<String> {
    let mut lines = Vec::new();
    let ratios = |row: &serde_json::Value| {
        OUTPUTS
            .iter()
            .map(|output| format!("{:.2}", number(&comparison(row, output)["ratio"])))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    for (row, with_gain) in raw.iter().zip(gain) {
        assert_eq!(row["id"], with_gain["id"]);
        let id = row["id"].as_str().unwrap();
        lines.push(format!(
            "| `{id}` | {} | {:.2} | {:+.1} | {} | {} |",
            number(&row["spread"]["bpm"]),
            number(&comparison(row, "rms_db")["difference"]),
            duration(row).0 * 1000.0,
            ratios(row),
            ratios(with_gain),
        ));
    }
    lines
}

#[test]
fn the_reference_page_counts_follow_the_reports() {
    let page = std::fs::read_to_string("docs/src/development/reference-recordings.md").unwrap();
    let lines = page_lines();
    let missing: Vec<&String> = lines
        .iter()
        .filter(|line| !page.contains(line.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "the page lacks {} lines; the reports give:\n{}",
        missing.len(),
        lines.join("\n")
    );
}
