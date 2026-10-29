//! Render and load timing (#255) on the fake renderer: every row holds a
//! median and a 95th percentile, and the table names the build and the
//! architecture.

use std::num::NonZeroUsize;

use sound_to_patch::render::fake::{FakePatch, FakeRenderer};
use sound_to_patch::render::session::BoxedRenderer;
use sound_to_patch::render::timing::{TimingKind, WORKER_COUNTS, measure};
use sound_to_patch::render::{PresetState, RECIPE_VERSION};
use sound_to_patch::synth::Synth;
use sound_to_patch::units::Seconds;

fn state(release: f64) -> PresetState {
    FakePatch {
        harmonics: vec![1.0],
        attack: Seconds::new(0.005).unwrap(),
        release: Seconds::new(release).unwrap(),
    }
    .state(Synth::Vital)
    .unwrap()
}

fn fake(worker: usize) -> BoxedRenderer {
    Box::new(FakeRenderer::new(
        Synth::Vital,
        u64::try_from(worker).unwrap(),
    ))
}

#[test]
fn every_row_holds_a_median_and_a_95th_percentile() {
    let runs = NonZeroUsize::new(3).unwrap();
    let table = measure(|worker| Ok(fake(worker)), &state(0.05), runs).unwrap();
    assert_eq!(table.identity.build, "fake");
    assert_eq!(table.identity.arch, "fake");
    assert_eq!(table.recipe_version, RECIPE_VERSION);
    assert_eq!(table.runs, 3);
    let kinds: Vec<(TimingKind, Option<usize>)> = table
        .rows
        .iter()
        .map(|row| (row.kind, row.workers))
        .collect();
    let mut expected = vec![
        (TimingKind::PluginLoad, None),
        (TimingKind::StateLoad, None),
        (TimingKind::Render, None),
        (TimingKind::ClipLength, None),
    ];
    expected.extend(
        WORKER_COUNTS
            .iter()
            .map(|workers| (TimingKind::RendersPerSecond, Some(*workers))),
    );
    assert_eq!(kinds, expected);
    for row in &table.rows {
        assert!(row.median.is_finite() && row.median >= 0.0, "{row:?}");
        assert!(row.p95.is_finite() && row.p95 >= 0.0, "{row:?}");
        if row.kind == TimingKind::RendersPerSecond {
            // A slower batch gives a lower rate.
            assert!(row.p95 <= row.median, "{row:?}");
        } else {
            assert!(row.median <= row.p95, "{row:?}");
        }
    }
    let json = serde_json::to_value(&table).unwrap();
    assert_eq!(json["rows"][4]["kind"], "renders_per_second");
    assert_eq!(json["rows"][4]["workers"], 1);
}

#[test]
fn the_clip_length_shows_a_long_tail() {
    let runs = NonZeroUsize::new(1).unwrap();
    let clip = |release| {
        let table = measure(|worker| Ok(fake(worker)), &state(release), runs).unwrap();
        let row = table
            .rows
            .iter()
            .find(|row| row.kind == TimingKind::ClipLength);
        row.unwrap().median
    };
    assert!((4.0..4.2).contains(&clip(0.05)), "{}", clip(0.05));
    assert!(clip(3.0) > 6.0, "{}", clip(3.0));
}
