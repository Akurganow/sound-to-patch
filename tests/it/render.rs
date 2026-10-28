//! The render core on the fake renderer: the contract suite, repeat
//! renders, faults and the recipe version.

use sound_to_patch::render::fake::{FakePatch, FakeRenderer, Fault};
use sound_to_patch::render::{Material, Recipe, RenderError, RenderJob, Renderer};
use sound_to_patch::synth::Synth;
use sound_to_patch::units::{Bpm, Seconds};

use crate::support::render::check_renderer;

fn patch(release: f64) -> FakePatch {
    FakePatch {
        harmonics: vec![0.5, 0.25, 0.125],
        attack: Seconds::new(0.005).unwrap(),
        release: Seconds::new(release).unwrap(),
    }
}

fn job(release: f64) -> RenderJob {
    RenderJob::for_material(
        patch(release).state(Synth::Vital),
        Material::Lead,
        Bpm::new(120.0).unwrap(),
    )
}

#[test]
fn the_fake_passes_the_renderer_contract() {
    for synth in Synth::READY {
        let mut renderer = FakeRenderer::new(synth, 7);
        check_renderer(&mut renderer, &patch(0.3).state(synth), false);
    }
}

#[test]
fn one_seed_gives_bit_identical_renders_and_repeats_vary() {
    let first = FakeRenderer::new(Synth::Vital, 7)
        .render(&job(0.3))
        .unwrap();
    let mut renderer = FakeRenderer::new(Synth::Vital, 7);
    let again = renderer.render(&job(0.3)).unwrap();
    assert_eq!(first.audio, again.audio);
    let repeat = renderer.render(&job(0.3)).unwrap();
    assert_ne!(again.audio, repeat.audio);
    let other = FakeRenderer::new(Synth::Vital, 8)
        .render(&job(0.3))
        .unwrap();
    assert_ne!(first.audio, other.audio);
}

#[test]
fn a_tail_heavy_preset_is_not_cut() {
    // A 60 dB fall over 6 s reaches -96 dBFS after about 9.6 s past the
    // note-off at 3 s. The clip must hold all of it plus the quiet second.
    let rendered = FakeRenderer::new(Synth::Vital, 7)
        .render(&job(6.0))
        .unwrap();
    assert!(!rendered.record.tail_capped);
    assert!(
        rendered.record.length.get() > 12.0,
        "{}",
        rendered.record.length
    );
    assert!(
        rendered.record.length.get() < 15.0,
        "{}",
        rendered.record.length
    );
}

#[test]
fn faults_fail_the_render_they_hit_and_no_other() {
    let mut renderer = FakeRenderer::new(Synth::Vital, 7)
        .with_fault(0, Fault::Crash)
        .with_fault(1, Fault::Hang)
        .with_fault(2, Fault::Silence);
    assert!(matches!(
        renderer.render(&job(0.3)),
        Err(RenderError::Crashed(_))
    ));
    assert!(matches!(
        renderer.render(&job(0.3)),
        Err(RenderError::TimedOut(_))
    ));
    let silent = renderer.render(&job(0.3)).unwrap();
    assert!(silent.audio.channels()[0].iter().all(|s| *s == 0.0));
    assert!(renderer.render(&job(0.3)).is_ok());
    assert_eq!(renderer.renders(), 4);
}

#[test]
fn a_state_for_another_synth_or_not_a_patch_is_refused() {
    let mut renderer = FakeRenderer::new(Synth::Vital, 7);
    let other = RenderJob::for_material(
        patch(0.3).state(Synth::Serum2),
        Material::Lead,
        Bpm::new(120.0).unwrap(),
    );
    assert!(matches!(
        renderer.render(&other),
        Err(RenderError::State(_))
    ));
    let garbage = RenderJob::for_material(
        sound_to_patch::render::PresetState::new(Synth::Vital, b"{}".to_vec()),
        Material::Lead,
        Bpm::new(120.0).unwrap(),
    );
    assert!(matches!(
        renderer.render(&garbage),
        Err(RenderError::State(_))
    ));
}

/// Any change to a recipe field changes this snapshot. Raise
/// `RECIPE_VERSION` with it: the snapshot name carries the version, so the
/// old snapshot stays as the record of the old recipe.
#[test]
fn the_recipe_matches_the_snapshot_of_its_version() {
    let recipe = Recipe::current();
    insta::assert_snapshot!(
        format!("recipe_v{}", recipe.version()),
        serde_json::to_string_pretty(&recipe).unwrap()
    );
}
