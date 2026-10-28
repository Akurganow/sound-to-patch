//! The data and the examples of the reference plugin.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sound_to_patch::preset::catalogue::unread_keys;
use sound_to_patch::preset::format::format_for;
use sound_to_patch::preset::{ApplyRequest, apply_file};
use sound_to_patch::synth::{Synth, SynthArg};

const SKILL: &str = "plugins/sound-to-patch-reference/skills/synth-reference";

fn validator(schema: &str) -> jsonschema::Validator {
    let schema: Value = serde_json::from_str(&fs::read_to_string(schema).unwrap()).unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

fn word_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir("dictionary/words")
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    files
}

fn entries(path: &Path) -> Vec<Value> {
    serde_json::from_str::<Value>(&fs::read_to_string(path).unwrap())
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

#[test]
fn every_axis_has_a_word_file() {
    let stems: Vec<String> = word_files()
        .iter()
        .map(|path| path.file_stem().unwrap().to_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        stems,
        [
            "body",
            "envelope",
            "mood",
            "movement",
            "space",
            "structure",
            "texture",
            "tone"
        ]
    );
}

#[test]
fn the_dictionary_files_match_their_schemas() {
    let words = validator("schemas/words.schema.json");
    for path in word_files() {
        let value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        let errors: Vec<String> = words.iter_errors(&value).map(|e| e.to_string()).collect();
        assert!(errors.is_empty(), "{}: {errors:?}", path.display());
    }
    let sources: Value =
        serde_json::from_str(&fs::read_to_string("dictionary/sources.json").unwrap()).unwrap();
    assert!(validator("schemas/sources.schema.json").is_valid(&sources));
}

#[test]
fn word_entries_sit_on_their_axis_and_name_known_sources() {
    let sources: BTreeSet<String> = entries(Path::new("dictionary/sources.json"))
        .iter()
        .map(|source| source["id"].as_str().unwrap().to_owned())
        .collect();
    let mut ids = BTreeSet::new();
    let mut defaults: BTreeMap<String, usize> = BTreeMap::new();
    for path in word_files() {
        let axis = path.file_stem().unwrap().to_str().unwrap();
        for entry in entries(&path) {
            let id = entry["id"].as_str().unwrap();
            assert!(ids.insert(id.to_owned()), "{id} twice");
            assert_eq!(entry["axis"], axis, "{id}");
            let word = entry["word"].as_str().unwrap().to_owned();
            *defaults.entry(word).or_default() += usize::from(entry["default"] == true);
            for source in entry["sources"].as_array().unwrap() {
                assert!(sources.contains(source.as_str().unwrap()), "{id}: {source}");
            }
        }
    }
    for (word, count) in defaults {
        assert_eq!(count, 1, "{word} needs exactly one default sense");
    }
}

#[test]
fn a_measure_outside_the_fixed_list_fails() {
    let entry = json!([{
        "id": "bright.spectral", "word": "bright", "aliases": [], "axis": "tone",
        "sense": "spectral", "meaning": "More upper harmonics.", "roles": [], "conflicts": [],
        "default": true, "agreement": {"value": 0.34, "n": 19, "class": "medium"},
        "few_sample": false, "measure": {"key": "loudness", "direction": "up"},
        "tag": "crowd data", "sources": []
    }]);
    let words = validator("schemas/words.schema.json");
    assert!(!words.is_valid(&entry));
    let mut good = entry;
    good[0]["measure"]["key"] = json!("centroid_over_f0");
    assert!(words.is_valid(&good));
}

fn examples(synth: Synth) -> Vec<PathBuf> {
    let dir = Path::new(SKILL).join("examples").join(synth.as_str());
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(|entry| entry.unwrap().path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

#[test]
fn every_example_writes_a_preset_on_the_built_in_template() {
    let counts: Value = serde_json::from_str(
        &fs::read_to_string(Path::new(SKILL).join("examples/count.json")).unwrap(),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    for synth in Synth::READY {
        let files = examples(synth);
        assert_eq!(
            u64::try_from(files.len()).ok(),
            counts[synth.as_str()].as_u64(),
            "examples/count.json must list {} examples for {synth}; one was added or deleted",
            files.len()
        );
        let format = format_for(synth).unwrap();
        let template = dir.path().join(format!("template.{}", format.extension()));
        fs::write(&template, format.template().builtin().unwrap()).unwrap();
        for file in files {
            let out = dir
                .path()
                .join(file.file_stem().unwrap())
                .with_extension(format.extension());
            apply_file(&ApplyRequest {
                edits: &file,
                out: &out,
                template: Some(&template),
                synth: &SynthArg { synth: Some(synth) },
                name: None,
                force: false,
            })
            .unwrap_or_else(|error| panic!("{}: {error}", file.display()));
        }
    }
}

#[test]
#[ignore = "needs a Vital-saved preset: set VITAL_PROBE_PRESET to its path"]
fn the_catalogue_covers_every_key_of_a_saved_preset() {
    let path = std::env::var("VITAL_PROBE_PRESET")
        .expect("set VITAL_PROBE_PRESET to a preset saved by Vital");
    let bytes = fs::read(&path).unwrap();
    let model = format_for(Synth::Vital).unwrap().read(&bytes).unwrap();
    let listed = unread_keys(Synth::Vital).unwrap();
    let missing: Vec<String> = model
        .unknown_keys()
        .into_iter()
        .filter_map(|key| key.strip_prefix("settings.").map(str::to_owned))
        .filter(|key| !listed.contains(&key.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "keys with no catalogue row: {missing:?}"
    );
}
