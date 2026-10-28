//! The plug-in renderer on the retail Vital.
//!
//! Ignored in CI. Each test needs Vital in a standard VST3 folder and
//! `SOUND_TO_PATCH_HOST` set to a host binary that loads it: for an
//! `x86_64`-only Vital on Apple silicon, a universal or `x86_64` build of
//! `sound-to-patch-host`. Run them with `cargo nextest run --run-ignored
//! only render_vital`. The presets are generated from the minimal fixture.

use std::path::PathBuf;

use serde_json::Value;
use sound_to_patch::render::plugin::{HOST_ENV, PluginRenderer, find_plugin};
use sound_to_patch::render::state::{preset_state, readback_mismatches, saved_preset};
use sound_to_patch::render::{Material, Oversampling, PresetState, Recipe, RenderJob, Renderer};
use sound_to_patch::synth::Synth;
use sound_to_patch::units::Bpm;

use crate::support::render::check_renderer;

fn renderer() -> PluginRenderer {
    let plugin = find_plugin(Synth::Vital)
        .unwrap_or_else(|| panic!("no Vital.vst3 in the standard VST3 folders; install Vital"));
    let host = PathBuf::from(std::env::var_os(HOST_ENV).unwrap_or_else(|| {
        panic!(
            "set {HOST_ENV} to a sound-to-patch-host that loads Vital; build one with \
             `cargo build --release -p sound-to-patch-host --target x86_64-apple-darwin`"
        )
    }));
    PluginRenderer::start(Synth::Vital, &plugin, &host).unwrap()
}

/// The minimal fixture named `Probe`, with `edit` applied to its controls.
fn state(edit: impl FnOnce(&mut serde_json::Map<String, Value>)) -> PresetState {
    let mut preset: Value =
        serde_json::from_slice(&std::fs::read("tests/fixtures/vital/minimal.vital").unwrap())
            .unwrap();
    preset["preset_name"] = "Probe".into();
    edit(preset["settings"].as_object_mut().unwrap());
    preset_state(
        Synth::Vital,
        &serde_json::to_vec(&preset).unwrap(),
        &Recipe::current(),
    )
    .unwrap()
}

fn lead(state: PresetState) -> RenderJob {
    RenderJob::for_material(state, Material::Lead, Bpm::new(120.0).unwrap())
}

#[test]
#[ignore = "needs Vital and SOUND_TO_PATCH_HOST"]
fn vital_passes_the_renderer_contract() {
    check_renderer(&mut renderer(), &state(|_| {}), false);
}

#[test]
#[ignore = "needs Vital and SOUND_TO_PATCH_HOST"]
fn vital_saves_the_state_it_loaded() {
    let mut renderer = renderer();
    let sent = state(|settings| {
        settings.insert("filter_1_on".into(), 1.0.into());
        settings.insert("filter_1_cutoff".into(), 50.0.into());
        settings.insert("env_1_attack".into(), 0.3.into());
    });
    renderer.render(&lead(sent.clone())).unwrap();
    let saved = renderer.save_state().unwrap();
    assert_eq!(saved_preset(&saved).unwrap()["preset_name"], "Probe");
    let mismatches = readback_mismatches(&sent, &saved).unwrap();
    assert!(
        mismatches.is_empty(),
        "{}",
        mismatches
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    );
}

#[test]
#[ignore = "needs Vital and SOUND_TO_PATCH_HOST"]
fn a_preset_at_8x_renders_at_2x() {
    let mut renderer = renderer();
    let output = renderer
        .render(&lead(state(|settings| {
            settings.insert("oversampling".into(), 3.0.into());
        })))
        .unwrap();
    assert_eq!(output.record.oversampling, Oversampling::X2);
    let saved = saved_preset(&renderer.save_state().unwrap()).unwrap();
    assert_eq!(saved["settings"]["oversampling"], 1.0);
}

#[test]
#[ignore = "needs Vital and SOUND_TO_PATCH_HOST"]
fn the_record_names_the_build_and_the_host() {
    let identity = renderer().identity();
    assert_eq!(identity.synth, Synth::Vital);
    assert_ne!(identity.build, "");
    assert!(
        identity.host.starts_with("sound-to-patch-host "),
        "{}",
        identity.host
    );
    assert_eq!(identity.arch, "x86_64");
}
