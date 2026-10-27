//! The analysis report (#56): `layers.json`, its schema, the spectrogram
//! and the `--notes` path.

use std::fs;
use std::path::{Path, PathBuf};

use midly::num::{u4, u7, u15, u24, u28};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};
use serde_json::{Value, json};
use sound_to_patch::audio::SampleRate;
use sound_to_patch::report::spectrogram::{PLOT_HEIGHT, PLOT_WIDTH, Spectrogram};
use sound_to_patch::report::{AnalyzeRequest, LAYERS_FILE, SPECTROGRAM_FILE, analyze};
use sound_to_patch::synth::Synth;
use sound_to_patch::units::Bpm;

use crate::support::signals::{sine, write_wav16};

const RATE: u32 = 48_000;
const SECOND: usize = 48_000;

/// A 3 s stereo stem: a 55 Hz tone with five harmonics from 0.25 to
/// 2.75 s, the right channel at 0.8 of the left.
pub(crate) fn stem(dir: &Path) -> PathBuf {
    let len = 3 * SECOND;
    let mut left = vec![0.0; len];
    for harmonic in 1..=5 {
        let h = f64::from(harmonic);
        for (sample, value) in left
            .iter_mut()
            .zip(sine(55.0 * h, 0.0, f64::from(RATE), len))
        {
            *sample += 0.2 * value / h;
        }
    }
    for (n, sample) in left.iter_mut().enumerate() {
        let t = f64::from(u32::try_from(n).unwrap()) / f64::from(RATE);
        if !(0.25..2.75).contains(&t) {
            *sample = 0.0;
        }
    }
    let right: Vec<f64> = left.iter().map(|v| 0.8 * v).collect();
    let path = dir.join("bass.wav");
    write_wav16(&path, RATE, &[left, right]);
    path
}

/// A MIDI file with one A1 (55 Hz) from 0.25 to 2.75 s at `bpm`, written
/// with a tempo event of `file_bpm`.
fn midi(dir: &Path, file_bpm: f64) -> PathBuf {
    let ticks_per_beat = 480.0;
    let tick = |seconds: f64| {
        let ticks = seconds * file_bpm / 60.0 * ticks_per_beat;
        u28::new(u32::try_from(crate::support::cast::round_to_usize(ticks)).unwrap())
    };
    let note = |on: bool| TrackEventKind::Midi {
        channel: u4::new(0),
        message: if on {
            MidiMessage::NoteOn {
                key: u7::new(33),
                vel: u7::new(100),
            }
        } else {
            MidiMessage::NoteOff {
                key: u7::new(33),
                vel: u7::new(0),
            }
        },
    };
    let us = u32::try_from(crate::support::cast::round_to_usize(60e6 / file_bpm)).unwrap();
    let track = vec![
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::new(us))),
        },
        TrackEvent {
            delta: tick(0.25),
            kind: note(true),
        },
        TrackEvent {
            delta: u28::new(tick(2.75).as_int() - tick(0.25).as_int()),
            kind: note(false),
        },
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        },
    ];
    let mut smf = Smf::new(Header::new(
        Format::SingleTrack,
        Timing::Metrical(u15::new(480)),
    ));
    smf.tracks.push(track);
    let path = dir.join("bass.mid");
    smf.save(&path).unwrap();
    path
}

fn request<'a>(stem: &'a Path, notes: Option<&'a Path>, out: &'a Path) -> AnalyzeRequest<'a> {
    AnalyzeRequest {
        stem,
        bpm: Bpm::new(120.0).unwrap(),
        synth: Synth::Vital,
        notes,
        out: Some(out),
    }
}

fn run(stem: &Path, notes: Option<&Path>, out: &Path) -> Value {
    analyze(&request(stem, notes, out)).unwrap();
    serde_json::from_str(&fs::read_to_string(out.join(LAYERS_FILE)).unwrap()).unwrap()
}

fn validator() -> jsonschema::Validator {
    let schema: Value =
        serde_json::from_str(&fs::read_to_string("schemas/layers.schema.json").unwrap()).unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

/// The validator of one definition of the schema.
fn definition(name: &str) -> jsonschema::Validator {
    let schema: Value =
        serde_json::from_str(&fs::read_to_string("schemas/layers.schema.json").unwrap()).unwrap();
    jsonschema::validator_for(&json!({
        "$schema": schema["$schema"],
        "$defs": schema["$defs"],
        "$ref": format!("#/$defs/{name}"),
    }))
    .unwrap()
}

/// The report with every number replaced by its JSON type, so a snapshot
/// fixes keys, nesting, strings and flags but no measured value.
fn shape(value: &Value) -> Value {
    match value {
        Value::Number(n) if n.is_u64() || n.is_i64() => json!("<integer>"),
        Value::Number(_) => json!("<number>"),
        Value::Array(items) => Value::Array(items.iter().map(shape).collect()),
        Value::Object(map) => {
            Value::Object(map.iter().map(|(k, v)| (k.clone(), shape(v))).collect())
        }
        other => other.clone(),
    }
}

#[test]
fn the_report_has_a_stable_shape_with_the_synth() {
    let dir = tempfile::tempdir().unwrap();
    let report = run(&stem(dir.path()), None, &dir.path().join("out"));
    assert_eq!(report["synth"], "vital");
    assert_eq!(report["schema"], "layers/1");
    insta::assert_snapshot!(serde_json::to_string_pretty(&shape(&report)).unwrap());
}

#[test]
fn every_stem_field_has_a_convention() {
    let dir = tempfile::tempdir().unwrap();
    let report = run(&stem(dir.path()), None, &dir.path().join("out"));
    let conventions = report["conventions"].as_object().unwrap();
    for key in report["stem"].as_object().unwrap().keys() {
        assert!(conventions.contains_key(key), "{key} has no convention");
    }
    assert_eq!(conventions["frame_size"], 4096);
}

#[test]
fn the_schema_accepts_the_report_and_rejects_broken_ones() {
    let dir = tempfile::tempdir().unwrap();
    let report = run(&stem(dir.path()), None, &dir.path().join("out"));
    let validator = validator();
    let errors: Vec<String> = validator
        .iter_errors(&report)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "{errors:?}");

    let mut unknown_group = report.clone();
    unknown_group["melody"] = json!({});
    assert!(!validator.is_valid(&unknown_group));

    let mut no_level = report.clone();
    no_level["stem"]["centroid_hz"]["confidence"]
        .as_object_mut()
        .unwrap()
        .remove("level");
    assert_eq!(
        no_level["stem"]["centroid_hz"]["confidence"]["estimate"],
        true
    );
    assert!(!validator.is_valid(&no_level));

    let mut value_without_estimate = report;
    value_without_estimate["stem"]["band_edge_hz"]["confidence"] =
        json!({"reasons": [{"code": "silent_stem", "value": true}], "estimate": false});
    assert!(!validator.is_valid(&value_without_estimate));
}

#[test]
fn the_layer_group_shape_takes_numbers_as_suggested_edits() {
    let group = definition("layer_group");
    let confidence = json!({"level": "medium", "reasons": [{"code": "active_frames", "value": 12}], "estimate": true});
    let valid = json!({
        "values": {"cutoff_hz": 410.0},
        "confidence": confidence,
        "flags": {"needs_review": false},
        "needs_render": true,
        "suggested_edits": {"filter1_cutoff_hz": 410},
    });
    assert!(group.is_valid(&valid));
    let mut string_edit = valid.clone();
    string_edit["suggested_edits"]["filter1_cutoff_hz"] = json!("410");
    assert!(!group.is_valid(&string_edit));
    let mut thin = valid;
    thin["confidence"] =
        json!({"reasons": [{"code": "too_few_notes", "value": 1}], "estimate": false});
    assert!(!group.is_valid(&thin));
    thin["values"] = Value::Null;
    assert!(group.is_valid(&thin));
}

#[test]
fn two_runs_write_identical_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = stem(dir.path());
    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    run(&path, None, &a);
    run(&path, None, &b);
    for name in [LAYERS_FILE, SPECTROGRAM_FILE] {
        assert_eq!(
            fs::read(a.join(name)).unwrap(),
            fs::read(b.join(name)).unwrap(),
            "{name}"
        );
    }
}

#[test]
fn the_default_folder_is_named_after_the_stem() {
    let dir = tempfile::tempdir().unwrap();
    let path = stem(dir.path());
    let request = AnalyzeRequest {
        out: None,
        ..request(&path, None, dir.path())
    };
    assert_eq!(request.out_dir(), dir.path().join("bass"));
    let text = analyze(&request).unwrap();
    assert!(dir.path().join("bass").join(LAYERS_FILE).is_file());
    assert!(text.contains("spectrogram.png"), "{text}");
}

#[test]
fn the_png_is_1500_by_512() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    run(&stem(dir.path()), None, &out);
    let png = fs::read(out.join(SPECTROGRAM_FILE)).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    // IHDR: width and height, big-endian, after the chunk length and type.
    assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), 1500);
    assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), 512);
}

#[test]
fn every_level_is_finite_before_the_colour_map() {
    let rate = SampleRate::new(RATE).unwrap();
    let tone = sine(1000.0, 0.0, f64::from(RATE), 2 * SECOND);
    let cases: [(&str, Vec<f64>); 4] = [
        ("silence", vec![0.0; SECOND]),
        ("tone", tone),
        ("one sample", vec![0.5]),
        ("empty", Vec::new()),
    ];
    for (name, samples) in cases {
        let spectrogram = Spectrogram::of(&samples, rate);
        assert_eq!(spectrogram.panels().len(), 2);
        for panel in spectrogram.panels() {
            let db = panel.db();
            assert_eq!(db.len(), PLOT_WIDTH * PLOT_HEIGHT, "{name}");
            assert!(
                db.iter()
                    .all(|v| v.is_finite() && (-80.0..=0.0).contains(v)),
                "{name}: a level is not finite or outside -80 to 0 dB"
            );
        }
        assert_eq!(spectrogram.to_png().unwrap()[..4], *b"\x89PNG", "{name}");
    }
}

#[test]
fn a_tone_reads_0_db_in_its_row() {
    let rate = SampleRate::new(RATE).unwrap();
    let spectrogram = Spectrogram::of(&sine(1000.0, 0.0, 48_000.0, SECOND), rate);
    let panel = &spectrogram.panels()[0];
    let loudest = panel.db().iter().copied().fold(f64::NEG_INFINITY, f64::max);
    assert!(loudest > -0.5, "{loudest}");
    // The zoom panel shares the scale and ends at 500 Hz, far below the
    // tone. The middle column holds a frame wholly inside the signal, whose
    // ends would leak a step into every row.
    let zoom = spectrogram.panels()[1].db();
    let middle = zoom.iter().skip(PLOT_WIDTH / 2).step_by(PLOT_WIDTH);
    assert!(middle.copied().all(|v| v < -60.0));
}

#[test]
fn the_passage_is_the_most_active_10_s() {
    let rate = SampleRate::new(8000).unwrap();
    let mut samples = vec![0.0; 30 * 8000];
    let tone = sine(440.0, 0.0, 8000.0, 10 * 8000);
    samples[15 * 8000..25 * 8000].copy_from_slice(&tone);
    let spectrogram = Spectrogram::of(&samples, rate);
    assert!((spectrogram.span().get() - 10.0).abs() < 1e-9);
    // The first frame that reaches into the tone is centred half a frame
    // before it: 2048 samples, 0.256 s.
    let start = spectrogram.start().get();
    assert!((14.7..=15.0).contains(&start), "{start}");
    let steady = Spectrogram::of(&sine(440.0, 0.0, 8000.0, 30 * 8000), rate);
    assert!(
        steady.start().get().abs() < 1e-12,
        "a tie goes to the earliest"
    );
    let short = Spectrogram::of(&vec![0.1; 4 * 8000], rate);
    assert!((short.span().get() - 4.0).abs() < 1e-9);
}

#[test]
fn given_notes_reach_the_report_and_run_no_transcriber() {
    let dir = tempfile::tempdir().unwrap();
    let path = stem(dir.path());
    let notes = midi(dir.path(), 120.0);
    let report = run(&path, Some(&notes), &dir.path().join("out"));
    let ratio = &report["stem"]["centroid_over_f0"];
    assert!(ratio["median"].as_f64().unwrap() > 1.0, "{ratio}");
    // A 55 Hz median f0 selects 8192-point frames, and the stereo phase
    // now has an f0.
    assert_eq!(report["conventions"]["frame_size"], 8192);
    assert!(report["stem"]["stereo"]["full"]["ipd_deg"].is_number());
    assert_eq!(report["transcription"], Value::Null);
    assert!(validator().is_valid(&report));
    let without = run(&path, None, &dir.path().join("plain"));
    assert_eq!(without["stem"]["centroid_over_f0"], Value::Null);
}

#[test]
fn a_bad_note_file_names_the_file_and_the_cause() {
    let dir = tempfile::tempdir().unwrap();
    let path = stem(dir.path());
    let out = dir.path().join("out");
    let broken = dir.path().join("broken.mid");
    fs::write(&broken, b"MThd not really").unwrap();
    let message = analyze(&request(&path, Some(&broken), &out))
        .unwrap_err()
        .to_string();
    assert!(message.contains("broken.mid"), "{message}");
    assert!(message.contains("not a valid MIDI file"), "{message}");

    let missing = dir.path().join("absent.mid");
    let message = analyze(&request(&path, Some(&missing), &out))
        .unwrap_err()
        .to_string();
    assert!(message.contains("absent.mid"), "{message}");
    assert!(message.contains("cannot open the file"), "{message}");

    let fast = midi(dir.path(), 140.0);
    let message = analyze(&request(&path, Some(&fast), &out))
        .unwrap_err()
        .to_string();
    assert!(message.contains("140.000 BPM"), "{message}");
    assert!(message.contains("--bpm is 120"), "{message}");
    assert!(!out.exists(), "a failed run writes nothing");
}
