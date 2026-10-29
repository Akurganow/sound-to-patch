//! The plug-in renderer on the retail Vital.
//!
//! Ignored in CI. Each test needs Vital in a standard VST3 folder and
//! `SOUND_TO_PATCH_HOST` set to a host binary that loads it: for an
//! `x86_64`-only Vital on Apple silicon, a universal or `x86_64` build of
//! `sound-to-patch-host`. Run them with `cargo nextest run --run-ignored
//! only render_vital`. The presets are generated from the minimal fixture.

use std::path::PathBuf;

use serde_json::Value;
use sound_to_patch::render::check::{LoadFault, render_checked};
use sound_to_patch::render::plugin::{HOST_ENV, PluginRenderer, find_plugin};
use sound_to_patch::render::state::{preset_state, readback_mismatches, saved_preset};
use sound_to_patch::render::{
    Control, ControlKind, Material, Oversampling, PresetState, Recipe, RenderError, RenderJob,
    Renderer,
};
use sound_to_patch::synth::Synth;
use sound_to_patch::units::{Bpm, Seconds};

use crate::support::render::check_renderer;

fn host() -> PathBuf {
    PathBuf::from(std::env::var_os(HOST_ENV).unwrap_or_else(|| {
        panic!(
            "set {HOST_ENV} to a sound-to-patch-host that loads Vital; build one with \
             `cargo build --release -p sound-to-patch-host --target x86_64-apple-darwin`"
        )
    }))
}

fn renderer() -> PluginRenderer {
    let plugin = find_plugin(Synth::Vital)
        .unwrap_or_else(|| panic!("no Vital.vst3 in the standard VST3 folders; install Vital"));
    PluginRenderer::start(Synth::Vital, &plugin, &host()).unwrap()
}

/// The minimal fixture named `Probe`, with `edit` applied to its controls.
fn state(edit: impl FnOnce(&mut serde_json::Map<String, Value>)) -> PresetState {
    preset(|preset| edit(preset["settings"].as_object_mut().unwrap()))
}

/// The minimal fixture named `Probe`, with `edit` applied to the whole
/// preset.
fn preset(edit: impl FnOnce(&mut Value)) -> PresetState {
    let mut preset: Value =
        serde_json::from_slice(&std::fs::read("tests/fixtures/vital/minimal.vital").unwrap())
            .unwrap();
    preset["preset_name"] = "Probe".into();
    edit(&mut preset);
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
    let mismatches = readback_mismatches(&sent, &saved, Bpm::new(120.0).unwrap()).unwrap();
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

/// The probe preset: a slow attack and a closed filter, unlike init.
fn probe() -> PresetState {
    state(|settings| {
        settings.insert("filter_1_on".into(), 1.0.into());
        settings.insert("filter_1_cutoff".into(), 50.0.into());
        settings.insert("env_1_attack".into(), 0.3.into());
    })
}

#[test]
#[ignore = "needs Vital and SOUND_TO_PATCH_HOST"]
fn a_loaded_preset_passes_the_load_check() {
    let (_, check) = render_checked(&mut renderer(), &lead(probe())).unwrap();
    assert!(check.init_distance_db > check.margin_db, "{check:?}");
}

#[test]
#[ignore = "needs Vital and SOUND_TO_PATCH_HOST"]
fn the_init_patch_is_init_like() {
    let (_, check) = render_checked(&mut renderer(), &lead(state(|_| {}))).unwrap();
    assert!(check.init_like, "{check:?}");
}

/// A slower attack changes the sound little in mean band levels, so it
/// may lie within the margin; the render passes either way, because the
/// read-back matched.
#[test]
#[ignore = "needs Vital and SOUND_TO_PATCH_HOST"]
fn an_envelope_change_passes_the_load_check() {
    let attack = |seconds: f64| {
        state(move |settings| {
            settings.insert("env_1_attack".into(), seconds.into());
        })
    };
    for seconds in [0.4, 0.45] {
        let result = render_checked(&mut renderer(), &lead(attack(seconds)));
        assert!(result.is_ok(), "attack {seconds}: {}", result.unwrap_err());
    }
}

#[test]
#[ignore = "needs Vital and SOUND_TO_PATCH_HOST"]
fn a_newer_stamp_falls_back_to_init() {
    let sent = probe();
    let mut root = saved_preset(sent.bytes()).unwrap();
    root.insert("synth_version".into(), "1.7.0".into());
    let mut bytes = serde_json::to_vec(&root).unwrap();
    bytes.push(0);
    let fault = render_checked(
        &mut renderer(),
        &lead(PresetState::new(Synth::Vital, bytes)),
    )
    .unwrap_err();
    assert!(matches!(fault, LoadFault::InitFallback), "{fault}");
}

#[test]
#[ignore = "needs Vital and SOUND_TO_PATCH_HOST"]
fn a_preset_with_no_sound_source_is_flagged_silent() {
    let silent = state(|settings| {
        settings.insert("osc_1_on".into(), 0.0.into());
    });
    let fault = render_checked(&mut renderer(), &lead(silent)).unwrap_err();
    match fault {
        LoadFault::Silent { reasons } => {
            assert_eq!(reasons, ["no oscillator and no sample is on"]);
        }
        other => panic!("not flagged silent: {other}"),
    }
}

#[test]
#[ignore = "needs Vital and SOUND_TO_PATCH_HOST"]
fn a_key_vital_drops_is_a_readback_mismatch() {
    let unknown = state(|settings| {
        settings.insert("filter_1_on".into(), 1.0.into());
        settings.insert("filter_1_cutoff".into(), 50.0.into());
        settings.insert("not_a_vital_control".into(), 1.0.into());
    });
    let fault = render_checked(&mut renderer(), &lead(unknown)).unwrap_err();
    assert!(matches!(fault, LoadFault::Readback(_)), "{fault}");
    assert!(
        fault
            .to_string()
            .contains("settings.not_a_vital_control is missing"),
        "{fault}"
    );
}

#[test]
#[ignore = "needs Vital and SOUND_TO_PATCH_HOST"]
fn a_bundle_that_is_no_plug_in_is_a_plug_in_fault() {
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("Broken.vst3/Contents/MacOS");
    std::fs::create_dir_all(&executable).unwrap();
    std::fs::copy("/usr/bin/true", executable.join("Broken")).unwrap();
    std::fs::write(
        dir.path().join("Broken.vst3/Contents/Info.plist"),
        "<dict><key>CFBundleExecutable</key><string>Broken</string></dict>",
    )
    .unwrap();
    let error =
        PluginRenderer::start(Synth::Vital, &dir.path().join("Broken.vst3"), &host()).unwrap_err();
    assert!(matches!(error, RenderError::Host { .. }), "{error}");
    assert!(
        error.to_string().contains("loading the plug-in failed"),
        "{error}"
    );
}

/// The magnitude of `hz` in the left channel from 1 to 2 s.
fn magnitude(audio: &sound_to_patch::audio::Audio, hz: f64) -> f64 {
    let left = &audio.channels()[0][44_100..88_200];
    let (re, im) = left
        .iter()
        .enumerate()
        .fold((0.0, 0.0), |(re, im), (n, x)| {
            let phase =
                std::f64::consts::TAU * hz * f64::from(u32::try_from(n).unwrap()) / 44_100.0;
            (re + x * phase.cos(), im - x * phase.sin())
        });
    (re * re + im * im).sqrt() / 44_100.0
}

#[test]
#[ignore = "needs Vital and SOUND_TO_PATCH_HOST"]
fn a_tuning_does_not_carry_over_to_the_next_preset() {
    let up_two = preset(|preset| {
        preset["tuning"] = serde_json::json!({
            "scale_start_midi_note": 60,
            "reference_midi_note": 2,
            "tuning_name": "Up two",
            "mapping_name": "",
            "default": false,
            "scale": [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        });
    });
    let mut renderer = renderer();
    let tuned = renderer.render(&lead(up_two.clone())).unwrap();
    assert!(magnitude(&tuned.audio, 293.66) > 10.0 * magnitude(&tuned.audio, 261.63));
    let saved = renderer.save_state().unwrap();
    assert_eq!(
        readback_mismatches(&up_two, &saved, Bpm::new(120.0).unwrap()).unwrap(),
        []
    );
    // A preset without a tuning plays in the default tuning, not the last one.
    let plain = renderer.render(&lead(state(|_| {}))).unwrap();
    assert!(
        magnitude(&plain.audio, 261.63) > 10.0 * magnitude(&plain.audio, 293.66),
        "{} at 261.63 Hz, {} at 293.66 Hz",
        magnitude(&plain.audio, 261.63),
        magnitude(&plain.audio, 293.66)
    );
}

#[test]
#[ignore = "needs Vital and SOUND_TO_PATCH_HOST"]
fn a_full_pitch_bend_raises_the_note_two_semitones() {
    let mut renderer = renderer();
    let at = |kind| Control {
        time: Seconds::new(0.0).unwrap(),
        kind,
    };
    let plain = renderer.render(&lead(state(|_| {}))).unwrap();
    let bent = renderer
        .render(
            &lead(state(|_| {}))
                .with_controls(vec![at(ControlKind::PitchBend(16_383))])
                .unwrap(),
        )
        .unwrap();
    // MIDI 60 is 261.63 Hz; two semitones up is 293.66 Hz.
    assert!(magnitude(&plain.audio, 261.63) > 10.0 * magnitude(&plain.audio, 293.66));
    assert!(magnitude(&bent.audio, 293.66) > 10.0 * magnitude(&bent.audio, 261.63));
    // The pre-roll centres the bend again for the next render.
    let after = renderer.render(&lead(state(|_| {}))).unwrap();
    assert!(magnitude(&after.audio, 261.63) > 10.0 * magnitude(&after.audio, 293.66));
}

fn rms(audio: &sound_to_patch::audio::Audio) -> f64 {
    let left = &audio.channels()[0][44_100..88_200];
    (left.iter().map(|x| x * x).sum::<f64>() / 44_100.0).sqrt()
}

#[test]
#[ignore = "needs Vital and SOUND_TO_PATCH_HOST"]
fn the_mod_wheel_reaches_a_route() {
    let routed = || {
        state(|settings| {
            settings.insert(
                "modulations".into(),
                serde_json::json!([{ "source": "mod_wheel", "destination": "osc_1_level" }]),
            );
            settings.insert("modulation_1_amount".into(), (-1.0).into());
        })
    };
    let mut renderer = renderer();
    let still = renderer.render(&lead(routed())).unwrap();
    let wheel = Control {
        time: Seconds::new(0.0).unwrap(),
        kind: ControlKind::ModWheel(127),
    };
    let moved = renderer
        .render(&lead(routed()).with_controls(vec![wheel]).unwrap())
        .unwrap();
    assert!(
        rms(&moved.audio) < 0.5 * rms(&still.audio),
        "{} against {}",
        rms(&moved.audio),
        rms(&still.audio)
    );
}

#[test]
#[ignore = "needs Vital and SOUND_TO_PATCH_HOST"]
fn a_render_at_another_tempo_passes_the_load_check() {
    // The preset holds 120 BPM; the render plays at 140 BPM.
    let sent = state(|settings| {
        settings.insert("beats_per_minute".into(), 2.0.into());
    });
    let mut renderer = renderer();
    let job = RenderJob::for_material(sent, Material::Lead, Bpm::new(140.0).unwrap());
    render_checked(&mut renderer, &job).unwrap();
    let saved = saved_preset(&renderer.save_state().unwrap()).unwrap();
    let tempo = saved["settings"]["beats_per_minute"].as_f64().unwrap();
    assert!((tempo - 140.0 / 60.0).abs() < 1e-6, "{tempo}");
}
