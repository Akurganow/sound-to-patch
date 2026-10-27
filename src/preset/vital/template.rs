//! The Vital template: a built-in default that our code makes, or a preset
//! the user saved from the app and passes with `--template`.
//!
//! The built-in template lists every non-legacy control at its registry
//! default, so the output does not depend on a default a retail build may
//! change. Its wavetables are one sawtooth cycle and its sample is seeded
//! white noise, both generated here. It is not the app's own Init patch.

use std::path::PathBuf;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Map, Value, json};

use crate::cast::{f64_to_f32, pcm16, unit_interval, usize_to_f64};
use crate::preset::format::PresetModel;
use crate::preset::registry::ParamRegistry;
use crate::preset::template::{Severity, TemplateCheck, TemplateHelp, TemplateSource};
use crate::preset::vital::codec::VitalModel;
use crate::preset::vital::writers::FRAME_SAMPLES;
use crate::preset::vital::{DEFAULT_STAMP, RETAIL_BUILD, RETAIL_ONLY_KEYS, VitalVersion, registry};

/// Samples in the default sample: one second at 44.1 kHz.
const NOISE_SAMPLES: usize = 44_100;

const NOISE_SEED: u64 = 0x005E_ED0F_0000_5A7C;

/// The template rules of Vital.
#[derive(Clone, Copy, Debug, Default)]
pub struct VitalTemplate;

impl TemplateSource for VitalTemplate {
    fn help(&self) -> TemplateHelp {
        TemplateHelp(
            "Save a preset from Vital and pass it with --template, or leave --template out to \
             use the built-in default. Vital keeps user presets in ~/Music/Vital/User/Presets \
             on macOS"
                .to_owned(),
        )
    }

    fn candidates(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn builtin(&self) -> Option<Vec<u8>> {
        serde_json::to_vec(&builtin_root()).ok()
    }

    fn check(&self, model: &dyn PresetModel) -> Vec<TemplateCheck> {
        let Some(model) = model.as_any().downcast_ref::<VitalModel>() else {
            return vec![TemplateCheck {
                severity: Severity::Error,
                text: "the template is not a Vital preset".to_owned(),
                fix: "Pass a .vital file".to_owned(),
            }];
        };
        check_root(model.root())
    }
}

fn error(text: impl Into<String>, fix: impl Into<String>) -> TemplateCheck {
    TemplateCheck {
        severity: Severity::Error,
        text: text.into(),
        fix: fix.into(),
    }
}

fn warning(text: impl Into<String>, fix: impl Into<String>) -> TemplateCheck {
    TemplateCheck {
        severity: Severity::Warning,
        text: text.into(),
        fix: fix.into(),
    }
}

fn check_root(root: &Map<String, Value>) -> Vec<TemplateCheck> {
    let mut checks = Vec::new();
    let settings = root.get("settings").and_then(Value::as_object);
    if settings
        .and_then(|settings| settings.get("sample"))
        .is_none()
    {
        checks.push(error(
            "the template has no `settings.sample`",
            "Vital throws on load without it; save the template again from Vital",
        ));
    }
    let stamp = root.get("synth_version").and_then(Value::as_str);
    match stamp.map(str::parse::<VitalVersion>) {
        // The load gate compares only the major and minor (`load_save.cpp`
        // of 1.0.6), so 1.0.8 loads in 1.0.7 and gets no warning.
        Some(Ok(version))
            if RETAIL_BUILD
                .parse::<VitalVersion>()
                .is_ok_and(|retail| !version.accepted_by(&retail)) =>
        {
            checks.push(warning(
                format!("the template is stamped {version}, which the stable build {RETAIL_BUILD} refuses"),
                format!(
                    "The output keeps the stamp, and Vital {RETAIL_BUILD} refuses a newer major or minor; pass a template saved by the build that loads the file"
                ),
            ));
        }
        Some(Ok(version)) if version.major >= 1 => {}
        Some(Ok(version)) => checks.push(error(
            format!("the template was saved by Vital {version}, older than 1.0.0"),
            "Open it in a current Vital and save it again; an old stamp makes Vital rewrite values on load",
        )),
        Some(Err(cause)) => checks.push(error(
            format!("the template stamp {cause}"),
            "Save the template again from Vital",
        )),
        None => checks.push(error(
            "the template has no `synth_version` text",
            "Save the template again from Vital",
        )),
    }
    let Some(settings) = settings else {
        return checks;
    };
    if settings.contains_key("sub_octave") {
        checks.push(error(
            "the template holds `sub_octave`, a key of builds before 0.5.0",
            "Open it in a current Vital and save it again",
        ));
    }
    if let Ok(registry) = registry() {
        for (key, value) in settings {
            let Some(spec) = registry.get(key) else {
                continue;
            };
            match value.as_f64() {
                None => checks.push(error(
                    format!("control `{key}` holds {value}, not a number"),
                    "Vital throws on load; save the template again from Vital",
                )),
                // Vital loads such a value as it is, and factory presets
                // hold some; the edits leave it unless one names the key.
                Some(number) if !spec.accepts_saved(number) => checks.push(warning(
                    format!(
                        "control `{key}` holds {number}, outside {}",
                        spec.range_text()
                    ),
                    "Vital applies it unclamped; edit the control to bring it into the range",
                )),
                Some(_) => {}
            }
        }
    }
    let length = |key: &str| settings.get(key).and_then(Value::as_array).map(Vec::len);
    if length("wavetables").is_none_or(|n| n < 3) {
        checks.push(warning(
            "the template holds fewer than 3 wavetables",
            "Vital keeps the tables of the patch it had loaded; save a template with all three oscillators",
        ));
    }
    if length("lfos").is_none_or(|n| n < 8) {
        checks.push(warning(
            "the template holds fewer than 8 LFO shapes",
            "Vital keeps the shapes of the patch it had loaded; save the template again from Vital",
        ));
    }
    if !settings.contains_key("modulations") {
        checks.push(warning(
            "the template has no `modulations` list",
            "Vital clears every route on load; save the template again from Vital",
        ));
    }
    let retail: Vec<&str> = RETAIL_ONLY_KEYS
        .into_iter()
        .filter(|key| settings.contains_key(*key))
        .collect();
    if !retail.is_empty() {
        checks.push(warning(
            format!("the template holds retail-only keys: {}", retail.join(", ")),
            "Keep the stamp it came with; the output copies it, and the open engine cannot read these keys",
        ));
    }
    checks
}

/// The built-in item of a structured list, used to fill a short list.
pub(crate) fn default_item(list: &str) -> Value {
    match list {
        "wavetables" => saw_wavetable(),
        "lfos" => triangle_lfo(),
        _ => json!({"source": "", "destination": ""}),
    }
}

/// The built-in template as a JSON tree.
fn builtin_root() -> Value {
    let mut settings = Map::new();
    if let Ok(registry) = registry() {
        for spec in registry.iter().filter(|spec| !spec.legacy) {
            settings.insert(spec.key.clone(), json!(spec.default));
        }
    }
    settings.insert("sample".to_owned(), noise_sample());
    let empty_slot = json!({"source": "", "destination": ""});
    settings.insert("modulations".to_owned(), Value::Array(vec![empty_slot; 64]));
    settings.insert(
        "wavetables".to_owned(),
        Value::Array(vec![saw_wavetable(); 3]),
    );
    settings.insert("lfos".to_owned(), Value::Array(vec![triangle_lfo(); 8]));
    json!({
        "synth_version": DEFAULT_STAMP,
        "preset_name": "Init",
        "author": "sound-to-patch",
        "comments": "",
        "preset_style": "",
        "macro1": "MACRO 1",
        "macro2": "MACRO 2",
        "macro3": "MACRO 3",
        "macro4": "MACRO 4",
        "settings": settings,
    })
}

/// One rising sawtooth cycle from -1, as base64 of little-endian 32-bit
/// floats.
pub(crate) fn saw_frame() -> String {
    let bytes: Vec<u8> = (0..FRAME_SAMPLES)
        .flat_map(|i| {
            let x = 2.0 * usize_to_f64(i) / usize_to_f64(FRAME_SAMPLES) - 1.0;
            f64_to_f32(x).to_le_bytes()
        })
        .collect();
    STANDARD.encode(bytes)
}

fn saw_wavetable() -> Value {
    json!({
        "groups": [{
            "components": [{
                "keyframes": [{"position": 0, "wave_data": saw_frame()}],
                "type": "Wave Source",
                // Linear keyframe interpolation and frequency-domain
                // interpolation, the defaults of a new wave source.
                "interpolation_style": 1,
                "interpolation": 1,
            }],
        }],
        "name": "Saw",
        "author": "sound-to-patch",
        "version": DEFAULT_STAMP,
        "remove_all_dc": false,
        "full_normalize": false,
    })
}

/// A triangle that starts at the bottom, peaks halfway and returns. The app
/// draws `1 - y`, so `y = 1` is the bottom.
fn triangle_lfo() -> Value {
    json!({
        "num_points": 3,
        "points": [0.0, 1.0, 0.5, 0.0, 1.0, 1.0],
        "powers": [0.0, 0.0, 0.0],
        "name": "Triangle",
        "smooth": false,
    })
}

/// White noise in ±0.9 from a fixed seed, as base64 of mono 16-bit PCM.
pub(crate) fn noise_samples() -> String {
    let mut state = NOISE_SEED;
    let bytes: Vec<u8> = (0..NOISE_SAMPLES)
        .flat_map(|_| {
            // splitmix64
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            pcm16(0.9 * (2.0 * unit_interval(z) - 1.0)).to_le_bytes()
        })
        .collect();
    STANDARD.encode(bytes)
}

fn noise_sample() -> Value {
    json!({
        "name": "White Noise",
        "length": NOISE_SAMPLES,
        "sample_rate": 44_100,
        "samples": noise_samples(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::format::PresetFormat;
    use crate::preset::vital::FORMAT;

    fn builtin() -> Vec<u8> {
        VitalTemplate.builtin().unwrap()
    }

    #[test]
    fn the_builtin_reads_and_passes_its_check() {
        let model = FORMAT.read(&builtin()).unwrap();
        assert_eq!(VitalTemplate.check(model.as_ref()), Vec::new());
        assert_eq!(model.unknown_keys(), Vec::<String>::new());
    }

    #[test]
    fn the_builtin_holds_every_non_legacy_control() {
        let root: Value = serde_json::from_slice(&builtin()).unwrap();
        let settings = root["settings"].as_object().unwrap();
        let registry = registry().unwrap();
        let non_legacy = registry.iter().filter(|spec| !spec.legacy).count();
        assert_eq!(non_legacy, 772);
        assert_eq!(settings.len(), non_legacy + 4);
        for spec in registry.iter() {
            assert_eq!(
                settings.contains_key(&spec.key),
                !spec.legacy,
                "{}",
                spec.key
            );
        }
    }

    #[test]
    fn embedded_blobs_have_the_sizes_vital_reads() {
        let root: Value = serde_json::from_slice(&builtin()).unwrap();
        for table in root["settings"]["wavetables"].as_array().unwrap() {
            let data = table["groups"][0]["components"][0]["keyframes"][0]["wave_data"]
                .as_str()
                .unwrap();
            assert_eq!(data.len(), 10_924);
            assert_eq!(STANDARD.decode(data).unwrap().len(), 8192);
        }
        let samples = root["settings"]["sample"]["samples"].as_str().unwrap();
        assert_eq!(samples.len(), 117_600);
        let pcm = STANDARD.decode(samples).unwrap();
        assert_eq!(pcm.len(), 88_200);
        let peak = pcm
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| i16::from_le_bytes(*pair).unsigned_abs())
            .max()
            .unwrap();
        assert!(peak <= 29_490 && peak > 29_000, "{peak}");
    }

    #[test]
    fn the_saw_rises_from_minus_one() {
        let bytes = STANDARD.decode(saw_frame()).unwrap();
        let sample = |i: usize| f32::from_le_bytes(bytes[4 * i..4 * i + 4].try_into().unwrap());
        assert_eq!(sample(0), -1.0);
        assert_eq!(sample(1024), 0.0);
        assert_eq!(sample(2047), 1.0 - 2.0 / 2048.0);
    }

    #[test]
    fn two_runs_give_identical_bytes() {
        assert_eq!(builtin(), builtin());
    }

    #[test]
    fn check_reports_each_problem() {
        let checks = |text: &str| {
            let root: Value = serde_json::from_str(text).unwrap();
            check_root(root.as_object().unwrap())
        };
        let texts = |found: Vec<TemplateCheck>| -> Vec<(Severity, String)> {
            found.into_iter().map(|c| (c.severity, c.text)).collect()
        };
        let full_shapes =
            r#""modulations":[],"wavetables":[{},{},{}],"lfos":[{},{},{},{},{},{},{},{}]"#;
        let no_sample = format!(r#"{{"synth_version":"1.0.6","settings":{{{full_shapes}}}}}"#);
        assert_eq!(
            texts(checks(&no_sample)),
            [(
                Severity::Error,
                "the template has no `settings.sample`".to_owned()
            )]
        );
        let old =
            format!(r#"{{"synth_version":"0.9.9","settings":{{"sample":{{}},{full_shapes}}}}}"#);
        assert_eq!(checks(&old)[0].severity, Severity::Error);
        assert!(checks(&old)[0].text.contains("older than 1.0.0"));
        let sub = format!(
            r#"{{"synth_version":"1.0.6","settings":{{"sample":{{}},"sub_octave":1,{full_shapes}}}}}"#
        );
        assert!(checks(&sub)[0].text.contains("sub_octave"));
        let bool_control = format!(
            r#"{{"synth_version":"1.0.6","settings":{{"sample":{{}},"filter_1_on":true,{full_shapes}}}}}"#
        );
        assert!(checks(&bool_control)[0].text.contains("filter_1_on"));
        let outside = format!(
            r#"{{"synth_version":"1.0.6","settings":{{"sample":{{}},"filter_2_cutoff":140.0,{full_shapes}}}}}"#
        );
        assert_eq!(
            checks(&outside)[0].text,
            "control `filter_2_cutoff` holds 140, outside 8.0 to 136.0"
        );
        let short =
            r#"{"synth_version":"1.0.6","settings":{"sample":{},"wavetables":[{}],"lfos":[]}}"#;
        let found = texts(checks(short));
        assert_eq!(found.len(), 3, "{found:?}");
        assert!(
            found
                .iter()
                .all(|(severity, _)| *severity == Severity::Warning)
        );
        let retail = format!(
            r#"{{"synth_version":"1.5.5","settings":{{"sample":{{}},"custom_warps":[],{full_shapes}}}}}"#
        );
        assert_eq!(
            texts(checks(&retail)),
            [
                (
                    Severity::Warning,
                    "the template is stamped 1.5.5, which the stable build 1.0.7 refuses"
                        .to_owned()
                ),
                (
                    Severity::Warning,
                    "the template holds retail-only keys: custom_warps".to_owned()
                )
            ]
        );
        for stamp in ["1.0.7", "1.0.8", "1.0.99"] {
            let patch = format!(
                r#"{{"synth_version":"{stamp}","settings":{{"sample":{{}},{full_shapes}}}}}"#
            );
            assert_eq!(texts(checks(&patch)), [], "{stamp}");
        }
    }
}
