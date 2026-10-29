//! The load check (#250) on the fake renderer: a passing render, one case
//! per fault, each with its own message, and the init-like warning.

use sound_to_patch::render::check::{LoadFault, render_checked};
use sound_to_patch::render::fake::{FakePatch, FakeRenderer, Fault};
use sound_to_patch::render::{Material, RenderError, RenderJob, Renderer};
use sound_to_patch::synth::Synth;
use sound_to_patch::units::{Bpm, Seconds};

fn job(patch: &FakePatch) -> RenderJob {
    RenderJob::for_material(
        patch.state(Synth::Vital).unwrap(),
        Material::Lead,
        Bpm::new(120.0).unwrap(),
    )
}

fn dark() -> FakePatch {
    FakePatch {
        harmonics: vec![1.0],
        attack: Seconds::new(0.05).unwrap(),
        release: Seconds::new(0.3).unwrap(),
    }
}

/// The fault the check reports when the render with index 2, the one
/// after the two init renders, has `fault`.
fn fault_at_render(fault: Fault) -> LoadFault {
    let mut renderer = FakeRenderer::new(Synth::Vital, 7).with_fault(2, fault);
    render_checked(&mut renderer, &job(&dark())).unwrap_err()
}

#[test]
fn a_preset_unlike_init_passes() {
    let mut renderer = FakeRenderer::new(Synth::Vital, 7);
    let (output, check) = render_checked(&mut renderer, &job(&dark())).unwrap();
    assert!(check.init_distance_db > check.margin_db, "{check:?}");
    assert!(check.margin_db >= 0.1);
    assert_eq!(output.audio.channels().len(), 2);
    assert_eq!(renderer.renders(), 3);
}

#[test]
fn a_silent_render_is_flagged() {
    let fault = fault_at_render(Fault::Silence);
    assert!(matches!(fault, LoadFault::Silent { .. }), "{fault}");
    assert!(
        fault.to_string().starts_with("the render is silent"),
        "{fault}"
    );
}

#[test]
fn a_state_that_does_not_load_is_an_init_fallback() {
    let fault = fault_at_render(Fault::KeepInit);
    assert!(matches!(fault, LoadFault::InitFallback), "{fault}");
    assert!(
        fault.to_string().contains("saves its init patch"),
        "{fault}"
    );
}

#[test]
fn a_changed_state_is_named() {
    let fault = fault_at_render(Fault::Rename);
    assert!(matches!(fault, LoadFault::Readback(_)), "{fault}");
    assert!(
        fault
            .to_string()
            .contains("preset_name is \"renamed\", not \"fake\""),
        "{fault}"
    );
}

#[test]
fn a_plug_in_fault_keeps_its_own_message() {
    let fault = fault_at_render(Fault::Crash);
    assert!(
        matches!(fault, LoadFault::Render(RenderError::Crashed(_))),
        "{fault}"
    );
    assert!(
        fault.to_string().starts_with("the plug-in host stopped"),
        "{fault}"
    );
}

#[test]
fn a_render_of_the_init_patch_passes_with_a_warning() {
    let mut renderer = FakeRenderer::new(Synth::Vital, 7);
    let (_, check) = render_checked(&mut renderer, &job(&FakePatch::init())).unwrap();
    assert!(check.init_like, "{check:?}");
    let warning = check.warning().unwrap();
    assert!(warning.contains("within the margin"), "{warning}");
    assert!(warning.contains("the preset loaded"), "{warning}");
    assert!(!warning.contains("authorized"), "{warning}");
    let (_, check) =
        render_checked(&mut FakeRenderer::new(Synth::Vital, 7), &job(&dark())).unwrap();
    assert!(!check.init_like && check.warning().is_none(), "{check:?}");
}

#[test]
fn the_init_patch_is_gone_after_a_load() {
    let mut renderer = FakeRenderer::new(Synth::Vital, 7);
    renderer.render(&job(&dark())).unwrap();
    assert!(matches!(
        renderer.render_init(&job(&dark())),
        Err(RenderError::Host { .. })
    ));
}
