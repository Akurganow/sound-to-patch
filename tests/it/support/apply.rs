//! Helpers for the `preset apply` tests.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sound_to_patch::preset::format::{PresetModel, format_for};
use sound_to_patch::preset::{ApplyRequest, apply_file};
use sound_to_patch::synth::{Synth, SynthArg};

pub(crate) fn template(synth: Synth) -> Box<dyn PresetModel> {
    let format = format_for(synth).unwrap();
    format.read(&format.template().builtin().unwrap()).unwrap()
}

pub(crate) fn settings(model: &dyn PresetModel, synth: Synth) -> serde_json::Map<String, Value> {
    let bytes = format_for(synth).unwrap().write(model).unwrap();
    let root: Value = serde_json::from_slice(&bytes).unwrap();
    root["settings"].as_object().unwrap().clone()
}

pub(crate) fn changed_keys(
    before: &serde_json::Map<String, Value>,
    after: &serde_json::Map<String, Value>,
) -> Vec<String> {
    after
        .iter()
        .filter(|(key, value)| before.get(*key) != Some(value))
        .map(|(key, _)| key.clone())
        .chain(
            before
                .keys()
                .filter(|key| !after.contains_key(*key))
                .cloned(),
        )
        .collect()
}

pub(crate) fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, text).unwrap();
    path
}

pub(crate) const TWO_EDITS: &str = r#"{"synth":"vital","edits":[{"param":"filter_1_cutoff","value":"1046.502261 Hz","reason":"cutoff near C6"},{"param":"env_1_attack","value":"1 s","reason":"slow swell"}]}"#;

pub(crate) fn run(edits: &Path, out: &Path, template: Option<&Path>) -> Result<String, String> {
    apply_file(&ApplyRequest {
        edits,
        out,
        template,
        synth: &SynthArg::default(),
        name: None,
    })
    .map_err(|error| error.to_string())
}
