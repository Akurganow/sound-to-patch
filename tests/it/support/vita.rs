//! Helpers for the checks that load written presets in the open Vital
//! engine through `tools/vita/load_check.py`.

use std::f32::consts::TAU;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;
use sound_to_patch::preset::format::{PresetFormat, PresetModel};
use sound_to_patch::preset::vital::{
    FORMAT, FRAME_SAMPLES, Keyframe, LfoShape, MAX_LFO_POINTS, ROUTE_SOURCES, VitalModel, Wavetable,
};
use sound_to_patch::synth::Synth;

use crate::support::apply::{THREE_EDITS, run, template, write};

/// The Python that has the `vita` package, from `VITA_PYTHON`.
pub(crate) fn python() -> String {
    std::env::var("VITA_PYTHON").unwrap_or_else(|_| {
        panic!(
            "set VITA_PYTHON to a Python with the vita package, such as a virtual \
             environment made with `pip install vita numpy` outside the repository"
        )
    })
}

/// One line of the load-check report.
pub(crate) fn load_check(files: &[PathBuf]) -> Vec<Value> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/vita/load_check.py");
    let output = Command::new(python())
        .arg("-I")
        .arg(script)
        .args(files)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "the load check failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn vital(model: &dyn PresetModel) -> VitalModel {
    model.as_any().downcast_ref::<VitalModel>().unwrap().clone()
}

fn save(dir: &Path, name: &str, model: &VitalModel) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, FORMAT.write(model).unwrap()).unwrap();
    path
}

fn edit_json(path: &Path, name: &str, change: impl FnOnce(&mut Value)) -> PathBuf {
    let mut root: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    change(&mut root);
    let out = path.with_file_name(name);
    fs::write(&out, serde_json::to_vec(&root).unwrap()).unwrap();
    out
}

/// One cycle of `FRAME_SAMPLES` samples of a sine with `harmonic` cycles.
fn sine(position: u16, harmonic: u16) -> Keyframe {
    let samples = (0..u16::try_from(FRAME_SAMPLES).unwrap())
        .map(|i| (TAU * f32::from(i) * f32::from(harmonic) / 2048.0).sin())
        .collect();
    Keyframe { position, samples }
}

/// The variants of the load check of #181, each one difference from the
/// written base file, by name.
pub(crate) fn variants(dir: &Path) -> Vec<(String, PathBuf)> {
    let edits = write(dir, "edits.json", THREE_EDITS);
    let base = dir.join("Bass.vital");
    run(&edits, &base, None).unwrap();
    let fresh = || vital(template(Synth::Vital).as_ref());
    let mut list = vec![
        ("template".to_owned(), save(dir, "template.vital", &fresh())),
        ("base".to_owned(), base.clone()),
        (
            "unknown keys".to_owned(),
            edit_json(&base, "unknown.vital", |root| {
                root["settings"]["zz_unknown"] = 1.0.into();
                root["zz_top"] = "x".into();
            }),
        ),
        (
            "no name fields".to_owned(),
            edit_json(&base, "nameless.vital", |root| {
                let root = root.as_object_mut().unwrap();
                root.remove("preset_name");
                root.remove("author");
            }),
        ),
        (
            "retail-only keys".to_owned(),
            edit_json(&base, "retail.vital", |root| {
                let settings = &mut root["settings"];
                settings["custom_warps"] = Value::Array(Vec::new());
                settings["random_values"] = serde_json::json!([{"seed": 1}]);
                for osc in 1..=3 {
                    settings[format!("osc_{osc}_spectral_morph_phase")] = 0.25.into();
                }
            }),
        ),
    ];
    for stamp in ["1.0.6", "1.0.9", "1.1.0", "1.5.5"] {
        list.push((
            format!("stamp {stamp}"),
            edit_json(&base, &format!("stamp-{stamp}.vital"), |root| {
                root["synth_version"] = stamp.into();
            }),
        ));
    }
    let mut routes = fresh();
    for source in ROUTE_SOURCES {
        routes
            .route(source, "filter_1_cutoff", 0.25, None, None)
            .unwrap();
    }
    list.push((
        "a route from every source".to_owned(),
        save(dir, "routes.vital", &routes),
    ));
    let mut lfo = fresh();
    let points: Vec<(f64, f64)> = (0..MAX_LFO_POINTS)
        .map(|i| {
            let x = f64::from(u32::try_from(i).unwrap()) / 99.0;
            (x, (x * 7.0).fract())
        })
        .collect();
    lfo.set_lfo_shape(
        0,
        &LfoShape {
            name: "Saw Steps".to_owned(),
            powers: vec![0.0; points.len()],
            points,
            smooth: false,
        },
    )
    .unwrap();
    list.push((
        "an LFO of 100 points".to_owned(),
        save(dir, "lfo.vital", &lfo),
    ));
    for (name, count) in [
        ("a wavetable of 1 keyframe", 1_u16),
        ("a wavetable of 257 keyframes", 257),
    ] {
        let mut model = fresh();
        model
            .set_wavetable(
                0,
                &Wavetable {
                    name: "Harmonics".to_owned(),
                    author: "sound-to-patch".to_owned(),
                    keyframes: (0..count).map(|i| sine(i, 1 + i % 16)).collect(),
                },
            )
            .unwrap();
        list.push((
            name.to_owned(),
            save(dir, &format!("table-{count}.vital"), &model),
        ));
    }
    list
}
