//! `preset apply` and the edit applier against the Vital template.

use std::fs;

use serde_json::Value;
use sound_to_patch::preset::apply::apply;
use sound_to_patch::preset::edits::Resolved;
use sound_to_patch::preset::format::{Stored, format_for};
use sound_to_patch::preset::registry::{ParamKind, registry_for};
use sound_to_patch::preset::{ApplyRequest, apply_file, inspect};
use sound_to_patch::synth::{Synth, SynthArg};

use crate::support::apply::{TWO_EDITS, changed_keys, run, settings, template, write};
use crate::support::for_each_ready_synth;

#[test]
fn an_edit_equal_to_the_default_changes_nothing() {
    for_each_ready_synth(|synth| {
        let format = format_for(synth).unwrap();
        let registry = registry_for(synth).unwrap();
        let base = template(synth);
        let before = settings(base.as_ref(), synth);
        for (index, spec) in registry.iter().filter(|spec| !spec.legacy).enumerate() {
            let mut model = template(synth);
            let edit = Resolved::Param {
                key: spec.key.clone(),
                stored: spec.default,
                uncalibrated: false,
                reason: None,
            };
            let report = apply(format, registry, model.as_mut(), &[edit]).unwrap();
            assert_eq!(report.changes, Vec::new(), "{}", spec.key);
            assert_eq!(model.get(&spec.key), base.get(&spec.key), "{}", spec.key);
            // The whole tree is compared for every 50th key; a report
            // without changes means the model saw no write.
            if index % 50 == 0 {
                assert_eq!(settings(model.as_ref(), synth), before, "{}", spec.key);
            }
        }
    });
}

#[test]
fn one_parameter_edit_changes_one_key_and_a_route_changes_its_slot() {
    let synth = Synth::Vital;
    let format = format_for(synth).unwrap();
    let registry = registry_for(synth).unwrap();
    let before = settings(template(synth).as_ref(), synth);
    let mut model = template(synth);
    let edit = Resolved::Param {
        key: "filter_1_cutoff".into(),
        stored: 72.0,
        uncalibrated: false,
        reason: None,
    };
    apply(format, registry, model.as_mut(), &[edit]).unwrap();
    assert_eq!(
        changed_keys(&before, &settings(model.as_ref(), synth)),
        ["filter_1_cutoff"]
    );

    // A route changes the slot's entry and its amount and bipolar
    // controls; power, stereo and bypass are rewritten at their defaults.
    let mut model = template(synth);
    let route = Resolved::Route {
        source: "lfo_1".into(),
        destination: "filter_1_cutoff".into(),
        amount: 0.25,
        polarity: None,
        reason: None,
    };
    apply(format, registry, model.as_mut(), &[route]).unwrap();
    let mut changed = changed_keys(&before, &settings(model.as_ref(), synth));
    changed.sort();
    assert_eq!(
        changed,
        ["modulation_1_amount", "modulation_1_bipolar", "modulations"]
    );
}

#[test]
fn editing_a_route_keeps_its_slot_data() {
    let synth = Synth::Vital;
    let format = format_for(synth).unwrap();
    let registry = registry_for(synth).unwrap();
    // A slot as Vital saves it: a drawn curve in the entry, a curve power,
    // stereo on and a bipolar source.
    let mut root: Value =
        serde_json::from_slice(&format.write(template(synth).as_ref()).unwrap()).unwrap();
    let curve = serde_json::json!({"num_points": 2, "points": [0.0, 1.0, 1.0, 0.0], "powers": [0.0, 0.0], "smooth": false});
    root["settings"]["modulations"][0] = serde_json::json!({
        "source": "lfo_1", "destination": "filter_1_cutoff", "line_mapping": curve
    });
    for (key, value) in [
        ("modulation_1_amount", 0.25),
        ("modulation_1_bipolar", 1.0),
        ("modulation_1_power", -2.0),
        ("modulation_1_stereo", 1.0),
    ] {
        root["settings"][key] = value.into();
    }
    let mut model = format.read(&serde_json::to_vec(&root).unwrap()).unwrap();
    let route = |amount| Resolved::Route {
        source: "lfo_1".into(),
        destination: "filter_1_cutoff".into(),
        amount,
        polarity: None,
        reason: None,
    };
    let report = apply(format, registry, model.as_mut(), &[route(0.5)]).unwrap();
    assert_eq!(report.changes.len(), 1);
    let after = settings(model.as_ref(), synth);
    assert_eq!(after["modulations"][0]["line_mapping"], curve);
    assert_eq!(after["modulation_1_amount"], 0.5);
    assert_eq!(after["modulation_1_bipolar"], 1.0);
    assert_eq!(after["modulation_1_power"], -2.0);
    assert_eq!(after["modulation_1_stereo"], 1.0);
    // The same route again is skipped.
    let report = apply(format, registry, model.as_mut(), &[route(0.5)]).unwrap();
    assert_eq!(report.changes, []);
    assert_eq!(report.skipped[0].why, "equals the template");
    // A new route in a free slot starts from the defaults.
    let other = Resolved::Route {
        source: "env_2".into(),
        destination: "osc_1_level".into(),
        amount: 0.5,
        polarity: None,
        reason: None,
    };
    apply(format, registry, model.as_mut(), &[other]).unwrap();
    let after = settings(model.as_ref(), synth);
    assert!(after["modulations"][1].get("line_mapping").is_none());
    assert_eq!(after["modulation_2_power"], 0.0);
}

#[test]
fn uncalibrated_and_raw_edits_are_written_and_flagged() {
    let synth = Synth::Vital;
    let format = format_for(synth).unwrap();
    let registry = registry_for(synth).unwrap();
    let mut model = template(synth);
    let edits = [
        Resolved::Param {
            key: "filter_1_resonance".into(),
            stored: 0.3,
            uncalibrated: true,
            reason: Some("a slight peak".into()),
        },
        Resolved::Raw {
            key: "zz_newer_build".into(),
            value: Stored::Float(2.0),
            reason: None,
        },
    ];
    let report = apply(format, registry, model.as_mut(), &edits).unwrap();
    assert!(report.changes[0].uncalibrated);
    assert!(report.changes[1].raw);
    assert_eq!(model.get("filter_1_resonance"), Some(Stored::Float(0.3)));
    assert_eq!(model.get("zz_newer_build"), Some(Stored::Float(2.0)));

    let wrong_wire = [Resolved::Raw {
        key: "zz_text".into(),
        value: Stored::Text("x".into()),
        reason: None,
    }];
    let message = apply(format, registry, model.as_mut(), &wrong_wire)
        .unwrap_err()
        .to_string();
    assert!(message.contains("a control holds a number"), "{message}");
}

#[test]
fn an_unverified_effect_order_is_refused() {
    let synth = Synth::Vital;
    let mut model = template(synth);
    let edit = [Resolved::Param {
        key: "effect_chain_order".into(),
        stored: 7.0,
        uncalibrated: false,
        reason: None,
    }];
    let message = apply(
        format_for(synth).unwrap(),
        registry_for(synth).unwrap(),
        model.as_mut(),
        &edit,
    )
    .unwrap_err()
    .to_string();
    assert!(message.contains("unverified"), "{message}");
}

#[test]
fn a_second_inspect_shows_exactly_the_two_edits() {
    let dir = tempfile::tempdir().unwrap();
    let edits = write(dir.path(), "edits.json", TWO_EDITS);
    let template = dir.path().join("Init.vital");
    fs::write(
        &template,
        format_for(Synth::Vital)
            .unwrap()
            .template()
            .builtin()
            .unwrap(),
    )
    .unwrap();
    let out = dir.path().join("Init copy.vital");
    run(&edits, &out, Some(&template)).unwrap();
    let full = sound_to_patch::preset::format::DescribeOptions {
        full: true,
        shown: 0,
    };
    let before = inspect(&template, &SynthArg::default(), &full).unwrap();
    let after = inspect(&out, &SynthArg::default(), &full).unwrap();
    let differ: Vec<(&str, &str)> = before
        .lines()
        .zip(after.lines())
        .filter(|(a, b)| a != b)
        .collect();
    let names: Vec<&str> = differ
        .iter()
        .map(|(_, after)| {
            after
                .split_whitespace()
                .next()
                .unwrap()
                .trim_end_matches(':')
        })
        .collect();
    // The two edits, plus the name the output file gives the preset.
    assert_eq!(
        names,
        ["name", "preset_name", "env_1_attack", "filter_1_cutoff"]
    );
}

#[test]
fn the_output_keeps_the_template_stamp() {
    let dir = tempfile::tempdir().unwrap();
    let edits = write(dir.path(), "edits.json", TWO_EDITS);
    let builtin = format_for(Synth::Vital)
        .unwrap()
        .template()
        .builtin()
        .unwrap();
    let text = String::from_utf8(builtin)
        .unwrap()
        .replacen("\"1.0.6\"", "\"1.5.5\"", 1);
    let template = write(dir.path(), "mine.vital", &text);
    let out = dir.path().join("Out.vital");
    let summary = run(&edits, &out, Some(&template)).unwrap();
    assert!(
        summary.contains("\n  build 1.5.5, kept from the template\n"),
        "{summary}"
    );
    assert!(
        summary
            .contains("warning: the template is stamped 1.5.5, newer than the stable build 1.0.7"),
        "{summary}"
    );
    let root: Value = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
    assert_eq!(root["synth_version"], "1.5.5");
    assert_eq!(root["preset_name"], "Out");
}

#[test]
fn an_old_template_is_refused_with_its_stamp() {
    let dir = tempfile::tempdir().unwrap();
    let edits = write(dir.path(), "edits.json", TWO_EDITS);
    let builtin = format_for(Synth::Vital)
        .unwrap()
        .template()
        .builtin()
        .unwrap();
    let text = String::from_utf8(builtin)
        .unwrap()
        .replacen("\"1.0.6\"", "\"0.9.0\"", 1);
    let template = write(dir.path(), "old.vital", &text);
    let message = run(&edits, &dir.path().join("Out.vital"), Some(&template)).unwrap_err();
    assert!(
        message.contains("0.9.0") && message.contains("older than 1.0.0"),
        "{message}"
    );
}

#[test]
fn the_template_and_the_edit_list_are_never_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let edits = write(dir.path(), "edits.json", TWO_EDITS);
    let builtin = format_for(Synth::Vital)
        .unwrap()
        .template()
        .builtin()
        .unwrap();
    let template = dir.path().join("Init.vital");
    fs::write(&template, &builtin).unwrap();
    let message = run(&edits, &template, Some(&template)).unwrap_err();
    assert!(message.contains("edit list or the template"), "{message}");
    assert_eq!(fs::read(&template).unwrap(), builtin);
    // Only the Factory folder inside the synth's data folder is refused.
    let own = dir.path().join("Factory");
    fs::create_dir(&own).unwrap();
    run(&edits, &own.join("x.vital"), None).unwrap();
    let factory = dir.path().join("vital").join("Factory");
    fs::create_dir_all(&factory).unwrap();
    let message = run(&edits, &factory.join("x.vital"), None).unwrap_err();
    assert!(
        message.contains("inside Vital/Factory, the factory folder of Vital"),
        "{message}"
    );
    // A link into the factory folder lands there too.
    #[cfg(unix)]
    {
        let link = dir.path().join("Mine");
        std::os::unix::fs::symlink(&factory, &link).unwrap();
        let message = run(&edits, &link.join("x.vital"), None).unwrap_err();
        assert!(message.contains("factory folder"), "{message}");
    }
    // A path that leaves the factory folder again is allowed.
    let out = factory.join("..").join("Back.vital");
    run(&edits, &out, None).unwrap();
    assert!(dir.path().join("vital").join("Back.vital").exists());
}

#[test]
fn an_existing_output_is_replaced_only_with_force() {
    let dir = tempfile::tempdir().unwrap();
    let edits = write(dir.path(), "edits.json", TWO_EDITS);
    let out = write(dir.path(), "Mine.vital", "keep me");
    let message = run(&edits, &out, None).unwrap_err();
    assert!(
        message.contains("the file exists. Pass --force to replace it"),
        "{message}"
    );
    assert_eq!(fs::read_to_string(&out).unwrap(), "keep me");
    apply_file(&ApplyRequest {
        edits: &edits,
        out: &out,
        template: None,
        synth: &SynthArg::default(),
        name: None,
        force: true,
    })
    .unwrap();
    assert!(
        fs::read_to_string(&out)
            .unwrap()
            .contains("\"synth_version\"")
    );
}

#[test]
fn a_failed_write_is_an_output_error_and_leaves_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let edits = write(dir.path(), "edits.json", TWO_EDITS);
    let out = dir.path().join("missing").join("Out.vital");
    let message = run(&edits, &out, None).unwrap_err();
    assert!(message.starts_with("cannot write "), "{message}");
    assert!(
        message.contains("Check that the folder exists"),
        "{message}"
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn a_failed_apply_leaves_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let edits = write(
        dir.path(),
        "edits.json",
        r#"{"synth":"vital","edits":[{"param":"filter_1_cuttoff","value":"1 kHz","reason":"r"}]}"#,
    );
    let out = dir.path().join("Out.vital");
    let message = run(&edits, &out, None).unwrap_err();
    assert!(
        message.contains("Close names: filter_1_cutoff"),
        "{message}"
    );
    assert!(!out.exists());
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn every_choice_label_resolves_to_its_index() {
    for_each_ready_synth(|synth| {
        let registry = registry_for(synth).unwrap();
        for spec in registry
            .iter()
            .filter(|spec| spec.kind == ParamKind::Choice && !spec.legacy)
        {
            for (index, label) in spec.choices.iter().enumerate() {
                if spec.choices.iter().filter(|other| *other == label).count() > 1 {
                    continue;
                }
                let text = format!(
                    r#"{{"synth":"{synth}","edits":[{{"menu":"{}","choice":"{label}","reason":"r"}}]}}"#,
                    spec.key
                );
                let list = sound_to_patch::preset::edits::parse(
                    &text,
                    sound_to_patch::preset::edits::Path::Agent,
                )
                .unwrap();
                let resolved = sound_to_patch::preset::edits::resolve(&list, registry).unwrap();
                let Resolved::Param { stored, .. } = resolved[0] else {
                    panic!()
                };
                assert!(
                    (stored - spec.stored_min - f64::from(u32::try_from(index).unwrap())).abs()
                        < 1e-12
                );
            }
        }
    });
}
