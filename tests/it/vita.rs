//! Written presets and single-note renders in the open Vital engine, and
//! written presets in the retail app. All are ignored: the open-engine
//! tests need `vita`, and the CI job `open-engine` runs them; the last
//! needs a file the app saved.

use std::fs;

use sound_to_patch::preset::format::{PresetFormat, Stored};
use sound_to_patch::preset::vital::FORMAT;

use crate::support::apply::{run, write};
use crate::support::vita::{load_check, variants};

#[test]
#[ignore = "needs vita: set VITA_PYTHON to a Python with tools/vita/requirements.txt"]
fn written_presets_load_in_the_open_engine() {
    let dir = tempfile::tempdir().unwrap();
    let variants = variants(dir.path());
    let names: std::collections::HashSet<&str> =
        variants.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names.len(),
        variants.len(),
        "every variant has its own name"
    );
    let files: Vec<_> = variants.iter().map(|(_, path)| path.clone()).collect();
    let report = load_check(&files);
    assert_eq!(report.len(), variants.len());
    for ((name, _), line) in variants.iter().zip(&report) {
        println!("{name}: {line}");
        assert_eq!(line["loaded"], true, "{name}");
        assert_eq!(line["changed"], serde_json::json!([]), "{name}");
        // The engine drops only the keys it does not know, and adds none.
        let dropped = match name.as_str() {
            "unknown keys" => serde_json::json!(["zz_unknown"]),
            "retail-only keys" => serde_json::json!([
                "custom_warps",
                "random_values",
                "osc_1_spectral_morph_phase",
                "osc_2_spectral_morph_phase",
                "osc_3_spectral_morph_phase"
            ]),
            _ => serde_json::json!([]),
        };
        assert_eq!(line["dropped"], dropped, "{name}");
        assert_eq!(line["added"], serde_json::json!([]), "{name}");
        assert_eq!(line["finite"], true, "{name}");
        assert!(line["rms"].as_f64().unwrap() > 1e-3, "{name} is silent");
        assert!(line["peak"].as_f64().unwrap() < 4.0, "{name} clips hard");
    }
    let bands = |index: usize| -> Vec<f64> {
        report[index]["bands"]
            .as_array()
            .unwrap()
            .iter()
            .map(|band| band.as_f64().unwrap())
            .collect()
    };
    let moved = bands(0)
        .iter()
        .zip(bands(1))
        .map(|(template, base)| (template - base).abs())
        .fold(0.0, f64::max);
    assert!(
        moved > 1.0,
        "the edits did not change the sound: {moved} dB"
    );
}

#[test]
#[ignore = "needs Vital and a saved copy of the written preset in VITAL_SAVED_PRESET"]
fn a_preset_saved_by_vital_keeps_the_edits() {
    // The edits of the re-save checked on 1.0.7. A new re-save must start
    // from a file written from this list.
    const RESAVE_EDITS: &str = r#"{"synth":"vital","edits":[{"param":"filter_1_cutoff","value":"1046.502261 Hz","reason":"cutoff near C6"},{"param":"env_1_attack","value":"1 s","reason":"slow swell"}]}"#;
    let saved_path = std::env::var("VITAL_SAVED_PRESET").unwrap_or_else(|_| {
        panic!(
            "set VITAL_SAVED_PRESET to the file Vital saved after loading the preset \
             that `preset apply` writes to Bass.vital from the edits in RESAVE_EDITS"
        )
    });
    let saved =
        fs::read(&saved_path).unwrap_or_else(|error| panic!("cannot read {saved_path}: {error}"));
    let dir = tempfile::tempdir().unwrap();
    let edits = write(dir.path(), "edits.json", RESAVE_EDITS);
    let out = dir.path().join("Bass.vital");
    run(&edits, &out, None).unwrap();
    let written = FORMAT.read(&fs::read(&out).unwrap()).unwrap();
    let again = FORMAT.read(&saved).unwrap();
    let name = |model: &dyn sound_to_patch::preset::format::PresetModel| {
        model
            .header()
            .fields
            .iter()
            .find(|field| field.label == "name")
            .map(|field| field.value.clone())
    };
    // Vital names a preset after its file, so a re-save under a new name
    // carries that name.
    let saved_stem = std::path::Path::new(&saved_path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned());
    assert_eq!(name(again.as_ref()), saved_stem);
    assert_eq!(name(written.as_ref()).as_deref(), Some("Bass"));
    for key in ["filter_1_cutoff", "env_1_attack"] {
        let (Some(Stored::Float(want)), Some(Stored::Float(got))) =
            (written.get(key), again.get(key))
        else {
            panic!("{key} is missing or not a number");
        };
        #[expect(
            clippy::cast_possible_truncation,
            reason = "one f32 step is the tolerance"
        )]
        let narrow = (want as f32).abs();
        let step = f64::from(narrow.next_up()) - f64::from(narrow);
        assert!(
            (want - got).abs() <= (1e-6 * want.abs()).max(step),
            "{key}: wrote {want}, Vital saved {got}"
        );
    }
    let keys = |model: &dyn sound_to_patch::preset::format::PresetModel| -> Vec<String> {
        let bytes = FORMAT.write(model).unwrap();
        let root: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        root["settings"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    };
    let (ours, theirs) = (keys(written.as_ref()), keys(again.as_ref()));
    println!("key order of the saved file: {theirs:?}");
    println!(
        "keys Vital added: {:?}",
        theirs
            .iter()
            .filter(|key| !ours.contains(key))
            .collect::<Vec<_>>()
    );
}

#[test]
#[ignore = "needs vita: set VITA_PYTHON to a Python with tools/vita/requirements.txt"]
fn the_open_engine_passes_the_single_note_contract() {
    crate::support::render::check_renderer(
        &mut crate::support::vita::renderer(),
        &crate::support::render::probe(|_| {}),
        true,
    );
}

#[test]
#[ignore = "needs vita: set VITA_PYTHON to a Python with tools/vita/requirements.txt"]
fn the_open_engine_refuses_a_midi_file_with_more_than_one_note() {
    use sound_to_patch::render::{RenderJob, Renderer};
    let job = RenderJob::from_midi(
        crate::support::render::probe(|_| {}),
        std::path::Path::new("tests/cmd/data/three-notes.mid"),
        sound_to_patch::units::Bpm::new(120.0).unwrap(),
    )
    .unwrap();
    let error = crate::support::vita::renderer()
        .render(&job)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("the open engine plays one note per render, and the job has 3"),
        "{error}"
    );
    assert!(
        error.contains("render a MIDI file with one note"),
        "{error}"
    );
}

#[test]
#[ignore = "needs vita: set VITA_PYTHON to a Python with tools/vita/requirements.txt"]
fn the_open_engine_passes_the_load_check_at_another_tempo() {
    use sound_to_patch::render::check::render_checked;
    use sound_to_patch::render::{Material, RenderJob};
    let sent = crate::support::render::probe(|preset| {
        preset["settings"]["beats_per_minute"] = 2.0.into();
        preset["settings"]["filter_1_on"] = 1.0.into();
        preset["settings"]["filter_1_cutoff"] = 50.0.into();
    });
    let job = RenderJob::for_material(
        sent,
        Material::Lead,
        sound_to_patch::units::Bpm::new(140.0).unwrap(),
    );
    let (_, check) = render_checked(&mut crate::support::vita::renderer(), &job).unwrap();
    assert!(!check.init_like, "{check:?}");
}

#[test]
#[ignore = "needs vita: set VITA_PYTHON to a Python with tools/vita/requirements.txt"]
fn the_open_engine_renders_a_job_identically_twice() {
    use sound_to_patch::render::{Material, RenderJob, Renderer};
    // Each render runs in its own process, so the open engine's spread is 0.
    let job = RenderJob::for_material(
        crate::support::render::probe(|_| {}),
        Material::Lead,
        sound_to_patch::units::Bpm::new(120.0).unwrap(),
    );
    let mut renderer = crate::support::vita::renderer();
    let first = renderer.render(&job).unwrap().audio;
    let second = renderer.render(&job).unwrap().audio;
    assert_eq!(first, second);
}

#[test]
#[ignore = "needs vita: set VITA_PYTHON to a Python with tools/vita/requirements.txt"]
fn the_open_engine_renders_every_length_it_is_asked_for() {
    use sound_to_patch::notes::Velocity;
    use sound_to_patch::render::{RenderJob, RenderNote, Renderer};
    use sound_to_patch::units::{Bpm, Seconds};
    // Onsets and lengths that are not whole milliseconds. The renderer
    // refuses a clip of another length than it asked for.
    let mut renderer = crate::support::vita::renderer();
    for step in 0..12_u32 {
        let onset = f64::from(step) * 0.013_7;
        let length = 0.05 + f64::from(step) * 0.071_3;
        let note = RenderNote::new(
            60,
            Seconds::new(onset).unwrap(),
            Seconds::new(onset + length).unwrap(),
            Velocity::new(100).unwrap(),
        )
        .unwrap();
        let job = RenderJob::new(
            crate::support::render::probe(|_| {}),
            vec![note],
            Bpm::new(120.0).unwrap(),
        )
        .unwrap();
        renderer
            .render(&job)
            .unwrap_or_else(|error| panic!("onset {onset} s, length {length} s: {error}"));
    }
}

/// Runs `tools/vita/render.py` on one render request for the init patch,
/// and returns the reply and the clip it wrote.
fn render_script(note_frames: usize, render_frames: usize) -> (serde_json::Value, Vec<Vec<f64>>) {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("clip.wav");
    let request = serde_json::json!({
        "op": "render", "preset": null, "bpm": 120.0, "sample_rate": 44_100,
        "key": 60, "velocity": 100, "note_frames": note_frames,
        "render_frames": render_frames, "warm_up": null, "out": out,
    });
    let reply = crate::support::vita::script("render.py", &request);
    let audio = sound_to_patch::audio::decode(&out).unwrap().into_audio();
    (reply, audio.channels().to_vec())
}

#[test]
#[ignore = "needs vita: set VITA_PYTHON to a Python with tools/vita/requirements.txt"]
fn the_open_engine_script_renders_and_releases_at_the_frames_it_is_asked_for() {
    // The engine truncates a length in seconds to frames in float32. With
    // n / rate, each of these render lengths came back one frame short. The engine
    // releases a note at the end of a block of 64 frames, so a note of
    // 64 k + 1 frames releases a block later than one of 64 k; with
    // n / rate, each of these note lengths truncated to 64 k.
    for frames in [400_000, 409_970, 419_940, 429_910, 430_907, 439_880] {
        let (reply, channels) = render_script(2205, frames);
        assert_eq!(reply["frames"], frames, "render of {frames} frames");
        assert_eq!(channels[0].len(), frames, "clip of {frames} frames");
    }
    for frames in [400_193, 400_449, 400_705, 401_921] {
        let (_, at) = render_script(frames, frames + 22_050);
        let (_, before) = render_script(frames - 1, frames + 22_050);
        assert_ne!(
            at, before,
            "a note of {frames} frames released a block early"
        );
    }
}

#[test]
#[ignore = "needs vita: set VITA_PYTHON to a Python with tools/vita/requirements.txt"]
fn the_open_engine_refusal_of_a_preset_is_a_state_error() {
    use sound_to_patch::render::{Material, PresetState, RenderError, RenderJob, Renderer};
    use sound_to_patch::synth::Synth;
    // A preset JSON that the engine does not load: the script exits with
    // code 3, and the renderer reports a state error, not a broken Python.
    let empty = PresetState::new(Synth::Vital, br#"{"settings":{}}"#.to_vec());
    let job = RenderJob::for_material(
        empty,
        Material::Lead,
        sound_to_patch::units::Bpm::new(120.0).unwrap(),
    );
    let error = crate::support::vita::renderer().render(&job).unwrap_err();
    assert!(
        matches!(&error, RenderError::State(cause) if cause == "the open engine did not load the preset"),
        "{error:?}"
    );
}

#[test]
#[ignore = "needs vita: set VITA_PYTHON to a Python with tools/vita/requirements.txt"]
fn the_open_engine_prints_stay_off_the_reply() {
    // An engine that writes to file descriptor 1 when it starts, as the
    // engine's own code may: the script sends it to standard error, so the
    // reply still parses.
    let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/vita/render.py");
    let code = format!(
        "import os, runpy, sys, vita\n\
         engine = vita.Synth\n\
         def noisy(*args):\n\
         \x20   os.write(1, b'engine noise\\n')\n\
         \x20   return engine(*args)\n\
         vita.Synth = noisy\n\
         sys.argv = [{script:?}]\n\
         runpy.run_path({script:?}, run_name='__main__')\n",
        script = script.to_string_lossy()
    );
    let reply = crate::support::vita::script_code(
        &code,
        &serde_json::json!({ "op": "save", "preset": null, "bpm": 120.0, "sample_rate": 44_100 }),
    );
    assert!(reply["state"].is_string(), "{reply}");
}
