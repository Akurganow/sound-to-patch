//! Written presets in the open Vital engine and in the retail app. Both
//! tests are ignored: the first needs `vita`, the second a file the app
//! saved.

use std::fs;

use sound_to_patch::preset::format::{PresetFormat, Stored};
use sound_to_patch::preset::vital::FORMAT;

use crate::support::apply::{run, write};
use crate::support::vita::{load_check, variants};

#[test]
#[ignore = "needs vita: set VITA_PYTHON to a Python with the vita package"]
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
