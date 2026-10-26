//! The preset seam: the registry and format contracts for every ready
//! synth, the contract suites themselves, and template lookup.

use std::fs;

use sound_to_patch::preset::format::format_for;
use sound_to_patch::preset::registry::registry_for;
use sound_to_patch::preset::template::{Origin, TemplateError, locate_template};
use sound_to_patch::synth::Synth;

use crate::support::memory::{Flaw, MemoryFormat, MemoryTemplate};
use crate::support::{for_each_ready_synth, format_contract, registry_contract};

const VITAL_MINIMAL: &[u8] = include_bytes!("../fixtures/vital/marks.vital");

#[test]
fn every_ready_registry_meets_the_contract() {
    for_each_ready_synth(|synth| {
        let registry = registry_for(synth).unwrap();
        assert_eq!(registry.synth(), synth);
        registry_contract(registry).unwrap();
    });
}

#[test]
fn every_ready_format_meets_the_contract() {
    for_each_ready_synth(|synth| {
        let format = format_for(synth).unwrap();
        assert_eq!(format.synth(), synth);
        assert_eq!(format.extension(), synth.extension());
        let template = format.template().builtin().unwrap();
        let samples: Vec<&[u8]> = match synth {
            Synth::Vital => vec![VITAL_MINIMAL, &template],
            Synth::Serum2 => unreachable!("Serum 2 is not ready"),
        };
        format_contract(format, &samples).unwrap();
    });
}

const MEMORY_SAMPLE: &[u8] = b"build=1\nlevel=0.5\nx_new=2.25\nend\n";

fn memory(flaw: Flaw) -> MemoryFormat {
    let mut format = MemoryFormat::new(flaw);
    format.template.builtin = Some(b"build=1\nlevel=1.0\nend\n".to_vec());
    format
}

#[test]
fn the_memory_adapter_meets_the_contract() {
    format_contract(&memory(Flaw::None), &[MEMORY_SAMPLE]).unwrap();
}

#[test]
fn each_broken_adapter_fails_the_contract() {
    let failure = |flaw| format_contract(&memory(flaw), &[MEMORY_SAMPLE]).unwrap_err();
    assert!(failure(Flaw::DropsUnknownKeys).contains("unknown keys changed"));
    assert!(failure(Flaw::ChangesValues).contains("does not give an equal model"));
    assert!(failure(Flaw::PanicsOnEmpty).contains("panicked"));
    let mut bad_template = memory(Flaw::None);
    bad_template.template.builtin = Some(b"build=1\nend\n".to_vec());
    assert!(
        format_contract(&bad_template, &[MEMORY_SAMPLE])
            .unwrap_err()
            .contains("fails its check")
    );
}

#[test]
fn template_lookup_follows_its_order() {
    let dir = tempfile::tempdir().unwrap();
    let cli = dir.path().join("cli.mem");
    let first = dir.path().join("first.mem");
    let second = dir.path().join("second.mem");
    let absent = dir.path().join("absent.mem");
    fs::write(&cli, "cli").unwrap();
    fs::write(&first, "first").unwrap();
    fs::write(&second, "second").unwrap();
    let source = MemoryTemplate {
        candidates: vec![absent.clone(), first.clone(), second],
        builtin: Some(b"builtin".to_vec()),
    };

    let template = locate_template(&source, Some(&cli), 1 << 20).unwrap();
    assert_eq!(template.bytes, b"cli");
    assert_eq!(template.origin, Origin::CommandLine(cli));

    let template = locate_template(&source, None, 1 << 20).unwrap();
    assert_eq!(template.bytes, b"first");
    assert_eq!(template.origin, Origin::Candidate(first.clone()));

    let error = locate_template(&source, Some(&absent), 1 << 20).unwrap_err();
    assert!(matches!(error, TemplateError::Unreadable { .. }));
    assert!(error.to_string().contains(&absent.display().to_string()));

    let builtin_only = MemoryTemplate {
        candidates: vec![absent.clone()],
        builtin: Some(b"builtin".to_vec()),
    };
    let template = locate_template(&builtin_only, None, 1 << 20).unwrap();
    assert_eq!(template.origin, Origin::Builtin);

    // A file over the cap is refused on the command line and passed over
    // as a candidate.
    let error = locate_template(&source, Some(&first), 2).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("larger than the limit of 2 bytes"),
        "{error}"
    );
    let template = locate_template(&source, None, 2).unwrap();
    assert_eq!(template.origin, Origin::Builtin);
}

#[test]
fn a_missing_template_lists_every_path_and_the_help() {
    let dir = tempfile::tempdir().unwrap();
    let paths = vec![dir.path().join("a.mem"), dir.path().join("b.mem")];
    let source = MemoryTemplate {
        candidates: paths.clone(),
        builtin: None,
    };
    let message = locate_template(&source, None, 1 << 20)
        .unwrap_err()
        .to_string();
    let lines: Vec<&str> = message.lines().collect();
    assert_eq!(lines[0], "no template found; tried:");
    for (line, path) in lines[1..].iter().zip(&paths) {
        assert_eq!(*line, format!("  {}", path.display()));
    }
    assert_eq!(lines.len(), paths.len() + 2, "{message}");
    assert!(
        lines[paths.len() + 1].starts_with("Write a build=1 file and pass it with --template"),
        "{message}"
    );
}

#[test]
fn a_vital_template_path_is_used_as_it_is() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mine.vital");
    fs::write(&path, VITAL_MINIMAL).unwrap();
    let format = format_for(Synth::Vital).unwrap();
    let template = locate_template(format.template(), Some(&path), format.max_bytes()).unwrap();
    assert_eq!(template.bytes, VITAL_MINIMAL);
    let missing = dir.path().join("missing.vital");
    let message = locate_template(format.template(), Some(&missing), format.max_bytes())
        .unwrap_err()
        .to_string();
    assert!(message.contains("missing.vital"), "{message}");
}

#[test]
fn the_builtin_vital_template_has_no_findings() {
    let format = format_for(Synth::Vital).unwrap();
    let template = locate_template(format.template(), None, format.max_bytes()).unwrap();
    assert_eq!(template.origin, Origin::Builtin);
    let model = format.read(&template.bytes).unwrap();
    assert_eq!(format.template().check(model.as_ref()), Vec::new());
}

/// Lists the keys whose presence or value differs between a preset saved by
/// the retail app and the built-in template.
#[test]
#[ignore = "needs a Vital default preset in VITAL_DEFAULT_PRESET"]
fn a_retail_default_preset_matches_the_builtin_template() {
    let path = std::env::var("VITAL_DEFAULT_PRESET").unwrap_or_else(|_| {
        panic!(
            "set VITAL_DEFAULT_PRESET to a preset saved from a fresh Vital instance \
             (Vital menu: Save Preset, then pass the file path)"
        )
    });
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let format = format_for(Synth::Vital).unwrap();
    let ours: serde_json::Value =
        serde_json::from_slice(&format.template().builtin().unwrap()).unwrap();
    let saved = saved["settings"].as_object().unwrap();
    let ours = ours["settings"].as_object().unwrap();
    let mut differences = Vec::new();
    for key in saved.keys().chain(ours.keys()) {
        let (a, b) = (saved.get(key), ours.get(key));
        let same = match (
            a.and_then(serde_json::Value::as_f64),
            b.and_then(serde_json::Value::as_f64),
        ) {
            // The app stores controls as 32-bit floats.
            (Some(a), Some(b)) => (a - b).abs() <= 1e-6 * a.abs().max(1.0),
            _ => a.is_some() == b.is_some(),
        };
        if !same && !differences.contains(key) {
            differences.push(key.clone());
        }
    }
    assert_eq!(differences, Vec::<String>::new(), "keys that differ");
}
