//! The repeat-render spread (#254) on the fake renderer: the statistic
//! recomputed from stored renders, and a comparison within and beyond it.

use std::num::NonZeroUsize;

use sound_to_patch::audio::{decode, write_wav};
use sound_to_patch::render::fake::{FakePatch, FakeRenderer};
use sound_to_patch::render::session::{BoxedRenderer, RenderSession};
use sound_to_patch::render::spread::{
    ClipMetrics, ENVELOPE_DISTANCE, SPREAD_RUNS, clip_rows, compare, envelope_distance,
};
use sound_to_patch::render::{Material, RenderJob};
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

/// Renders `job` `count` times on a session of two fakes, stores each clip
/// as a WAV file and measures the decoded files.
fn stored_renders(job: &RenderJob, count: usize, seed: u64) -> Vec<ClipMetrics> {
    let session = RenderSession::new(NonZeroUsize::new(2).unwrap(), move |worker| {
        let seed = seed + u64::try_from(worker).unwrap();
        Ok(Box::new(FakeRenderer::new(Synth::Vital, seed)) as BoxedRenderer)
    });
    let dir = tempfile::tempdir().unwrap();
    session
        .render_all(&vec![job.clone(); count])
        .iter()
        .enumerate()
        .map(|(index, outcome)| {
            let path = dir.path().join(format!("render-{index:02}.wav"));
            write_wav(&path, &outcome.rendered().unwrap().audio).unwrap();
            ClipMetrics::of(&decode(&path).unwrap().into_audio()).unwrap()
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

fn pairs(count: usize) -> impl Iterator<Item = (usize, usize)> {
    (0..count).flat_map(move |i| (i + 1..count).map(move |j| (i, j)))
}

#[test]
fn the_spread_is_recomputed_from_stored_renders() {
    assert_eq!(SPREAD_RUNS, 20);
    let clips = stored_renders(&job(0.5), SPREAD_RUNS, 1);
    let rows = clip_rows(&clips);
    let names: Vec<&str> = rows.iter().map(|row| row.output.as_str()).collect();
    assert_eq!(
        names,
        ["rms_db", "peak_db", "duration_s", ENVELOPE_DISTANCE]
    );
    assert_eq!(pairs(clips.len()).count(), 190);
    let rms = p95(pairs(clips.len())
        .map(|(i, j)| (clips[i].rms_db - clips[j].rms_db).abs())
        .collect());
    let envelope = p95(pairs(clips.len())
        .map(|(i, j)| envelope_distance(&clips[i], &clips[j]))
        .collect());
    assert!(
        (rows[0].spread - rms).abs() < 1e-12,
        "{} {rms}",
        rows[0].spread
    );
    assert!((rows[3].spread - envelope).abs() < 1e-12);
    // The fake's random phases and level jitter make the renders differ.
    assert!(rows[0].spread > 0.0 && rows[0].spread < 0.2, "{rows:?}");
    assert_eq!(rows[0].unit, "dB");
    assert_eq!(rows[2].unit, "s");
}

#[test]
fn a_render_of_the_preset_matches_and_a_louder_one_does_not() {
    let renders = stored_renders(&job(0.5), SPREAD_RUNS, 1);
    let rows = clip_rows(&renders);
    let same = stored_renders(&job(0.5), 1, 99).remove(0);
    for comparison in compare(&same, &renders, &rows) {
        assert!(comparison.within, "{comparison:?}");
    }
    let louder = stored_renders(&job(0.6), 1, 99).remove(0);
    let comparisons = compare(&louder, &renders, &rows);
    let rms = comparisons.iter().find(|c| c.output == "rms_db").unwrap();
    assert!(!rms.within, "{rms:?}");
    // 0.6 against 0.5 is 1.58 dB.
    assert!((rms.difference - 1.58).abs() < 0.1, "{rms:?}");
    let envelope = comparisons
        .iter()
        .find(|c| c.output == ENVELOPE_DISTANCE)
        .unwrap();
    assert!(!envelope.within, "{envelope:?}");
}

#[test]
fn the_reference_reports_hold_ids_hashes_and_metrics() {
    for (file, gain) in [("report.json", 0.0), ("report-gain.json", 2.16)] {
        let path = format!("docs/src/development/reference-recordings/{file}");
        let report: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(report["n"], 20, "{file}");
        assert_eq!(report["statistic"], "p95_abs_pairwise", "{file}");
        assert_eq!(report["recording_gain_db"], gain, "{file}");
        let rows = report["references"].as_array().unwrap();
        assert_eq!(rows.len(), 38, "{file}");
        let mut ids = std::collections::HashSet::new();
        for row in rows {
            let id = row["id"].as_str().unwrap();
            assert!(ids.insert(id), "{file}: {id} twice");
            assert!(id.starts_with(row["preset_id"].as_str().unwrap()), "{id}");
            for hash in ["preset_sha256", "midi_sha256", "recording_sha256"] {
                let hex = row[hash].as_str().unwrap();
                assert!(
                    hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()),
                    "{id}: {hash}"
                );
            }
            let outputs: Vec<&str> = row["comparisons"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| c["output"].as_str().unwrap())
                .collect();
            assert_eq!(
                outputs,
                ["rms_db", "peak_db", "duration_s", ENVELOPE_DISTANCE],
                "{id}"
            );
        }
    }
}
