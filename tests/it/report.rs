//! The analysis report (#56): `layers.json`, its schema, the spectrogram
//! and the `--notes` path.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use midly::num::{u4, u7, u15, u24, u28};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};
use serde_json::{Value, json};
use sound_to_patch::audio::{self, BeatGrid, SampleRate, Tempo};
use sound_to_patch::estimate::envelope::{EnvelopeInput, EnvelopeLayer};
use sound_to_patch::notes::{
    DEFAULT_VELOCITY, PitchTrack, Profile, Transcriber, Velocities, min_note_frames,
    write_named_midi,
};
use sound_to_patch::report::spectrogram::{PLOT_HEIGHT, PLOT_WIDTH, Spectrogram};
use sound_to_patch::report::{
    AnalyzeRequest, LAYERS_FILE, NOTES_FILE, NotesRequest, SPECTROGRAM_FILE, analyze, notes,
};
use sound_to_patch::synth::Synth;
use sound_to_patch::units::{Bpm, Seconds};

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

/// The notes of [`stem`] at 120 BPM. A report test passes them, so the
/// notes step runs only where a test needs it.
pub(crate) fn given(dir: &Path) -> PathBuf {
    midi(dir, 120.0)
}

fn request<'a>(stem: &'a Path, notes: Option<&'a Path>, out: &'a Path) -> AnalyzeRequest<'a> {
    AnalyzeRequest {
        stem,
        bpm: Bpm::new(120.0).unwrap(),
        bpm_exact: false,
        synth: Synth::Vital,
        notes,
        profile: None,
        out: Some(out),
        force: false,
    }
}

/// The transcriber with an empty f0 track. The pYIN track runs on every
/// stem, given notes or not, for the pitch layer, at about half a second
/// per second of audio in a debug build; `tests/it/pitch.rs` tests that
/// layer on committed tracks.
fn transcriber() -> Transcriber {
    untracked(Transcriber::new().unwrap())
}

/// `transcriber` with an empty f0 track.
fn untracked(transcriber: Transcriber) -> Transcriber {
    transcriber.with_tracker(|_, _| Ok(PitchTrack::default()))
}

fn run(stem: &Path, notes: Option<&Path>, out: &Path) -> Value {
    analyze(&request(stem, notes, out), &transcriber()).unwrap();
    serde_json::from_str(&fs::read_to_string(out.join(LAYERS_FILE)).unwrap()).unwrap()
}

pub(crate) fn validator() -> jsonschema::Validator {
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
pub(crate) fn shape(value: &Value) -> Value {
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
    let report = run(
        &stem(dir.path()),
        Some(&given(dir.path())),
        &dir.path().join("out"),
    );
    assert_eq!(report["synth"], "vital");
    assert_eq!(report["schema"], "layers/1");
    insta::assert_snapshot!(serde_json::to_string_pretty(&shape(&report)).unwrap());
}

#[test]
fn every_stem_field_has_a_convention() {
    let dir = tempfile::tempdir().unwrap();
    let report = run(
        &stem(dir.path()),
        Some(&given(dir.path())),
        &dir.path().join("out"),
    );
    let conventions = report["conventions"].as_object().unwrap();
    for key in report["stem"].as_object().unwrap().keys() {
        assert!(conventions.contains_key(key), "{key} has no convention");
    }
    assert_eq!(conventions["frame_size"], 8192);
}

#[test]
fn a_join_has_a_stable_record_the_schema_accepts() {
    use sound_to_patch::notes::{Note, NoteList};
    use sound_to_patch::units::{MidiPitch, Seconds};

    let dir = tempfile::tempdir().unwrap();
    let note = |from: f64, to: f64| {
        Note::new(
            Seconds::clamped(from),
            Seconds::clamped(to),
            MidiPitch::new(33.0).unwrap(),
        )
        .unwrap()
    };
    let list = NoteList::new(vec![note(0.25, 1.5), note(1.5, 2.75)]);
    let bytes = write_named_midi(
        &list,
        Bpm::new(120.0).unwrap(),
        Velocities::Constant(DEFAULT_VELOCITY),
        "bass.wav",
    )
    .unwrap();
    let path = dir.path().join("bass.mid");
    fs::write(&path, bytes).unwrap();
    let report = run(&stem(dir.path()), Some(&path), &dir.path().join("out"));
    let joins = report["voices"]["values"]["joins"].as_array().unwrap();
    assert_eq!(joins.len(), 1, "{joins:?}");
    let schema = definition("join");
    assert!(schema.is_valid(&joins[0]), "{:?}", joins[0]);
    let no_estimate = serde_json::json!({
        "estimate": false,
        "reasons": [{ "code": "no_note_list", "value": true }],
    });
    let mut unread = joins[0].clone();
    unread["dip_db"] = serde_json::Value::Null;
    unread["legato"] = serde_json::Value::Null;
    assert!(!schema.is_valid(&unread), "a null verdict with an estimate");
    unread["confidence"] = no_estimate.clone();
    assert!(schema.is_valid(&unread), "{unread:?}");
    unread["legato"] = serde_json::Value::Bool(true);
    assert!(!schema.is_valid(&unread), "a verdict with no estimate");
    insta::assert_snapshot!(serde_json::to_string_pretty(&shape(&joins[0])).unwrap());
}

#[test]
fn the_schema_accepts_the_report_and_rejects_broken_ones() {
    let dir = tempfile::tempdir().unwrap();
    let report = run(
        &stem(dir.path()),
        Some(&given(dir.path())),
        &dir.path().join("out"),
    );
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
fn the_layer_group_shape_takes_edit_records_as_suggested_edits() {
    let group = definition("layer_group");
    let confidence = json!({"level": "medium", "reasons": [{"code": "active_frames", "value": 12}], "estimate": true});
    let valid = json!({
        "values": {"cutoff_hz": 410.0},
        "confidence": confidence,
        "flags": {"needs_review": false},
        "needs_render": true,
        "suggested_edits": [
            {"param": "filter_1_cutoff", "value": "410 Hz", "reason": "timbre"},
            {"route": {"source": "env_2", "dest": "osc_1_transpose", "amount": "7 st", "polarity": "unipolar"}, "reason": "pitch", "uncalibrated": true, "confidence": "low"},
            {"param": "polyphony", "value": 3.0, "reason": "voices", "uncalibrated": false, "confidence": "high"},
            {"param": "legato", "value": true, "reason": "voices"},
        ],
        "unrepresentable": [
            {"field": "glides.time_s", "wanted": {"value": 20.0, "unit": "s"}, "nearest": {"value": 16.0, "unit": "s"}, "reason": "above_range", "residual": -4.0},
            {"field": "glides.from_silence", "wanted": "true", "reason": "not_in_synth"},
        ],
    });
    assert!(group.is_valid(&valid));
    for (path, bad) in [
        ("/suggested_edits/0", json!({"param": "x", "value": "1 s"})),
        (
            "/suggested_edits/0",
            json!({"param": "x", "value": "1 s", "reason": "r", "extra": 1}),
        ),
        (
            "/suggested_edits/0",
            json!({"menu": "x", "choice": "y", "reason": "r"}),
        ),
        ("/suggested_edits/1/route/polarity", json!("both")),
        ("/suggested_edits/1/uncalibrated", json!("yes")),
        ("/suggested_edits/2/confidence", json!("huge")),
        ("/unrepresentable/0/reason", json!("too_high")),
        ("/unrepresentable/0/wanted", json!(20.0)),
        (
            "/unrepresentable/1",
            json!({"field": "f", "wanted": "a", "reason": "not_in_synth", "extra": 1}),
        ),
    ] {
        let mut wrong = valid.clone();
        *wrong.pointer_mut(path).unwrap() = bad;
        assert!(!group.is_valid(&wrong), "{path} accepted");
    }
    let mut no_misses = valid.clone();
    no_misses.as_object_mut().unwrap().remove("unrepresentable");
    assert!(!group.is_valid(&no_misses));
    let mut bare_number = valid.clone();
    bare_number["suggested_edits"] = json!([410]);
    assert!(!group.is_valid(&bare_number));
    let mut number_map = valid.clone();
    number_map["suggested_edits"] = json!({"filter1_cutoff_hz": 410});
    assert!(!group.is_valid(&number_map));
    let mut empty = valid.clone();
    empty["suggested_edits"] = json!([]);
    assert!(group.is_valid(&empty));
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
    let notes = given(dir.path());
    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    run(&path, Some(&notes), &a);
    run(&path, Some(&notes), &b);
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
    let notes = given(dir.path());
    let request = AnalyzeRequest {
        out: None,
        ..request(&path, Some(&notes), dir.path())
    };
    assert_eq!(request.out_dir().unwrap(), dir.path().join("bass"));
    let analysis = analyze(&request, &transcriber()).unwrap();
    assert_eq!(analysis.layers, dir.path().join("bass").join(LAYERS_FILE));
    assert_eq!(
        analysis.spectrogram,
        dir.path().join("bass").join(SPECTROGRAM_FILE)
    );
    assert_eq!(
        analysis.notes,
        Some(dir.path().join("bass").join(NOTES_FILE))
    );
    assert!(analysis.layers.is_file() && analysis.skipped.is_empty());
    // The temporary siblings are renamed away.
    let names: Vec<String> = fs::read_dir(dir.path().join("bass"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 3, "{names:?}");
}

/// A run whose note list is empty removes the `notes.mid` of an earlier
/// run of the same stem, so the folder holds no notes the report lacks.
#[test]
fn a_run_without_notes_removes_the_old_notes_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = stem(dir.path());
    let out = dir.path().join("out");
    let first = analyze(
        &request(&path, Some(&given(dir.path())), &out),
        &transcriber(),
    )
    .unwrap();
    assert!(first.notes.as_ref().is_some_and(|p| p.is_file()));
    // The same stem, now silent: the notes step finds no note. Its new
    // checksum needs `--force`.
    crate::support::signals::write_wav16(&path, 22_050, &[vec![0.0; 22_050]]);
    let forced = AnalyzeRequest {
        force: true,
        ..request(&path, None, &out)
    };
    let second = analyze(&forced, &transcriber()).unwrap();
    assert_eq!(second.notes, None);
    assert!(!out.join(NOTES_FILE).exists());
    let report: Value =
        serde_json::from_str(&fs::read_to_string(out.join(LAYERS_FILE)).unwrap()).unwrap();
    assert_eq!(report["notes"], Value::Null);
}

/// An output folder that is a file, given by `--out` or the default
/// folder of `bass.wav` with a file `bass` next to it, stops the run.
#[test]
fn an_output_folder_that_is_a_file_fails_with_a_fix() {
    let dir = tempfile::tempdir().unwrap();
    let wav = stem(dir.path());
    let taken = dir.path().join("taken");
    fs::write(&taken, b"x").unwrap();
    let default = dir.path().join("bass");
    fs::write(&default, b"x").unwrap();
    for (out, folder) in [(Some(taken.as_path()), &taken), (None, &default)] {
        let request = AnalyzeRequest {
            out,
            ..request(&wav, None, dir.path())
        };
        let message = analyze(&request, &transcriber()).unwrap_err().to_string();
        assert_eq!(
            message,
            format!(
                "the output folder {} is a file. Pass --out <dir>",
                folder.display()
            )
        );
        let notes_request = NotesRequest {
            out,
            ..notes_request(&wav, dir.path(), 120.0, true)
        };
        let message = notes(&notes_request, &untracked(transcriber()))
            .unwrap_err()
            .to_string();
        assert_eq!(
            message,
            format!(
                "the output folder {} is a file. Pass --out <dir>",
                folder.display()
            )
        );
    }
}

#[test]
fn a_report_of_another_stem_is_kept_unless_forced() {
    let dir = tempfile::tempdir().unwrap();
    let wav = stem(dir.path());
    let aiff = dir.path().join("bass.aiff");
    fs::copy(&wav, &aiff).unwrap();
    let out = dir.path().join("out");
    let notes = given(dir.path());
    let first = run(&wav, Some(&notes), &out);
    assert_eq!(first["source"], "bass.wav");
    // The same stem again replaces its own report.
    run(&wav, Some(&notes), &out);
    let message = analyze(&request(&aiff, Some(&notes), &out), &transcriber())
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("report of `bass.wav`, not of `bass.aiff`"),
        "{message}"
    );
    assert!(message.contains("--force"), "{message}");
    let forced = AnalyzeRequest {
        force: true,
        ..request(&aiff, Some(&notes), &out)
    };
    analyze(&forced, &transcriber()).unwrap();
    let report: Value =
        serde_json::from_str(&fs::read_to_string(out.join(LAYERS_FILE)).unwrap()).unwrap();
    assert_eq!(report["source"], "bass.aiff");
}

#[test]
fn the_png_is_1500_by_512() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    run(&stem(dir.path()), Some(&given(dir.path())), &out);
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
    let out = dir.path().join("out");
    let (counting, windows) = crate::transcribe::counting();
    let analysis = analyze(&request(&path, Some(&notes), &out), &untracked(counting)).unwrap();
    assert_eq!(windows.load(Ordering::SeqCst), 0);
    assert!(analysis.transcription.is_none());
    let report: Value =
        serde_json::from_str(&fs::read_to_string(out.join(LAYERS_FILE)).unwrap()).unwrap();
    let ratio = &report["stem"]["centroid_over_f0"];
    assert!(ratio["median"].as_f64().unwrap() > 1.0, "{ratio}");
    // A 55 Hz median f0 selects 8192-point frames, and the stereo phase
    // now has an f0.
    assert_eq!(report["conventions"]["frame_size"], 8192);
    assert!(report["stem"]["stereo"]["full"]["ipd_deg"].is_number());
    assert_eq!(report["transcription"], Value::Null);
    assert!(validator().is_valid(&report));
}

#[test]
fn a_bad_note_file_names_the_file_and_the_cause() {
    let dir = tempfile::tempdir().unwrap();
    let path = stem(dir.path());
    let out = dir.path().join("out");
    let broken = dir.path().join("broken.mid");
    fs::write(&broken, b"MThd not really").unwrap();
    let message = analyze(&request(&path, Some(&broken), &out), &transcriber())
        .unwrap_err()
        .to_string();
    assert!(message.contains("broken.mid"), "{message}");
    assert!(message.contains("not a valid MIDI file"), "{message}");

    let missing = dir.path().join("absent.mid");
    let message = analyze(&request(&path, Some(&missing), &out), &transcriber())
        .unwrap_err()
        .to_string();
    assert!(message.contains("absent.mid"), "{message}");
    assert!(message.contains("cannot open the file"), "{message}");

    let fast = midi(dir.path(), 140.0);
    let message = analyze(&request(&path, Some(&fast), &out), &transcriber())
        .unwrap_err()
        .to_string();
    assert!(message.contains("140.000 BPM"), "{message}");
    assert!(message.contains("--bpm is 120"), "{message}");
    assert!(!out.exists(), "a failed run writes nothing");
}

/// Without a note list, `analyze` runs the notes step once: one model run
/// for the whole command, and the `transcription` group holds the model
/// hash, the profile and the decode settings.
#[test]
fn the_notes_step_runs_the_model_once_and_fills_the_transcription_group() {
    use sound_to_patch::notes::MODEL_SHA256;
    let dir = tempfile::tempdir().unwrap();
    let (path, truth) = crate::cli::saw_notes(dir.path());
    let out = dir.path().join("out");
    let (counting, windows) = crate::transcribe::counting();
    let analysis = analyze(&request(&path, None, &out), &untracked(counting)).unwrap();
    // 1.2 s at 22,050 Hz is one model window.
    assert_eq!(windows.load(Ordering::SeqCst), 1);
    let transcription = analysis.transcription.as_ref().unwrap();
    assert_eq!(transcription.notes.notes().len(), truth.len());
    let summary = analysis.summary();
    assert_eq!(
        summary[0],
        "tempo: 120 BPM as given; the stem holds 2 beats, too few to refine it"
    );
    for step in [
        "detected range: ",
        "settings: detected, ",
        "quiet-note guard: 3 notes, 0 dropped below 40 dB",
    ] {
        assert!(
            summary.iter().any(|line| line.starts_with(step)),
            "no `{step}` in {summary:?}"
        );
    }
    let report: Value =
        serde_json::from_str(&fs::read_to_string(&analysis.layers).unwrap()).unwrap();
    assert_eq!(report["transcription"]["model_sha256"], MODEL_SHA256);
    assert_eq!(report["transcription"]["profile"], "detected");
    assert_eq!(
        report["transcription"]["note_params"],
        serde_json::to_value(transcription.settings.note_params()).unwrap()
    );
    // The transcribed notes give the f0 that picks the frame and the ratio.
    assert!(report["stem"]["centroid_over_f0"]["median"].is_number());
    assert_eq!(report["stem"]["bpm_given"], 120.0);
    assert_eq!(report["stem"]["bpm_refined"]["value"], Value::Null);
    // Each final note carries the octave flag of its note in the list.
    let list = report["notes"]["list"].as_array().unwrap();
    assert_eq!(list.len(), truth.len());
    for (written, note) in list.iter().zip(transcription.notes.notes()) {
        assert_eq!(written["list_onset_s"], note.onset().get());
        assert_eq!(written["octave_uncertain"], json!(note.octave_uncertain()));
    }
    let errors: Vec<String> = validator()
        .iter_errors(&report)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
}

/// `--profile pad` makes a one-voice stem a polyphonic line when the notes
/// step runs, and the final notes and the full-band levels read that one
/// voicing. Given notes take no profile, so the line stays mono.
#[test]
fn the_pad_profile_makes_a_one_voice_stem_polyphonic_for_every_rule() {
    let dir = tempfile::tempdir().unwrap();
    let (path, _) = crate::cli::saw_notes(dir.path());
    let decoded = audio::decode(&path).unwrap().into_audio();
    let samples = decoded.mono();
    for (profile, mono) in [(None, true), (Some(Profile::Pad), false)] {
        let out = dir.path().join(format!("out-{mono}"));
        let request = AnalyzeRequest {
            profile,
            ..request(&path, None, &out)
        };
        let analysis = analyze(&request, &transcriber()).unwrap();
        assert!(analysis.voices.mono);
        assert_eq!(analysis.final_notes.as_ref().unwrap().mono, mono);
        let input = EnvelopeInput {
            mono: &samples,
            rate: decoded.sample_rate(),
            notes: &analysis.transcription.as_ref().unwrap().notes,
            grid: BeatGrid::new(analysis.tempo.used(), Seconds::clamped(0.0)),
            stereo: None,
        };
        let mut envelope = EnvelopeLayer::of(&input).unwrap();
        envelope.read_levels(&input, mono).unwrap();
        assert_eq!(envelope.levels, analysis.envelope.levels);
    }
    let stem = stem(dir.path());
    let notes = given(dir.path());
    let out = dir.path().join("given");
    let request = AnalyzeRequest {
        profile: Some(Profile::Pad),
        ..request(&stem, Some(&notes), &out)
    };
    let analysis = analyze(&request, &transcriber()).unwrap();
    assert!(analysis.voices.mono);
    assert!(analysis.final_notes.unwrap().mono);
}

/// A note file written at the refined tempo reads under the given one.
#[test]
fn a_note_file_at_the_refined_tempo_is_accepted() {
    use sound_to_patch::notes::read_midi_near;
    let dir = tempfile::tempdir().unwrap();
    let refined = midi(dir.path(), 119.62);
    let given = Bpm::new(120.0).unwrap();
    assert!(read_midi_near(&refined, given, Some(Bpm::new(119.62).unwrap())).is_ok());
    let message = read_midi_near(&refined, given, Some(Bpm::new(121.0).unwrap()))
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("--bpm is 120 and the tempo refined from the stem is 121"),
        "{message}"
    );
    // A refined tempo equal to --bpm adds nothing to the message.
    let message = read_midi_near(&refined, given, Some(given))
        .unwrap_err()
        .to_string();
    assert!(!message.contains("refined"), "{message}");
}

/// The record of a transcription fits the `transcription` group of the
/// schema, with the crate's parameter names.
#[test]
fn a_transcription_record_fits_the_schema() {
    use sound_to_patch::notes::{MODEL_SHA256, Profile, TranscriptionRecord};
    let bpm = Bpm::new(120.0).unwrap();
    for profile in [None, Some(Profile::Bass)] {
        let settings = profile.map_or(Profile::Lead.settings(bpm), |p| p.settings(bpm));
        let record = serde_json::to_value(TranscriptionRecord::new(profile, &settings)).unwrap();
        assert!(definition("transcription").is_valid(&record), "{record}");
        assert_eq!(record["model_sha256"], MODEL_SHA256);
        assert_eq!(record["profile"], profile.map_or("detected", Profile::name));
        assert_eq!(
            record["note_params"]["minimum_frequency"],
            settings.range.minimum_frequency().get()
        );
        assert_eq!(record["note_params"]["include_pitch_bends"], false);
        assert_eq!(record["note_params"].as_object().unwrap().len(), 10);
        let mut other = record.clone();
        other["profile"] = json!("drum");
        assert!(!definition("transcription").is_valid(&other));
    }
}

/// A generated bass line in eighths at `bpm`, `beats` long, as a mono WAV.
fn bass_line(dir: &Path, bpm: f64, beats: usize) -> PathBuf {
    let (rate, samples) = crate::tempo::bass_line(bpm, beats);
    let path = dir.join("line.wav");
    write_wav16(&path, rate, &[samples]);
    path
}

/// The tempo of the first tempo event of a MIDI file, in BPM.
fn file_bpm(bytes: &[u8]) -> f64 {
    let smf = Smf::parse(bytes).unwrap();
    let us = smf.tracks[0]
        .iter()
        .find_map(|event| match event.kind {
            TrackEventKind::Meta(MetaMessage::Tempo(us)) => Some(us.as_int()),
            _ => None,
        })
        .unwrap();
    60e6 / f64::from(us)
}

fn notes_request<'a>(stem: &'a Path, out: &'a Path, bpm: f64, exact: bool) -> NotesRequest<'a> {
    NotesRequest {
        stem,
        bpm: Bpm::new(bpm).unwrap(),
        bpm_exact: exact,
        profile: Some(Profile::Bass),
        out: Some(out),
        force: false,
    }
}

/// A `layers.json` next to `notes.mid` describes that file. `notes` after
/// `analyze` would replace the final notes with the first list, so it
/// stops and leaves both files; with `--force` it writes the first list
/// and removes the report that no longer describes it.
#[test]
fn notes_does_not_replace_the_final_notes_a_report_describes() {
    use sound_to_patch::report::AnalyzeError;
    let dir = tempfile::tempdir().unwrap();
    let stem = bass_line(dir.path(), 120.0, 9);
    let out = dir.path().join("out");
    let analysis = analyze(
        &AnalyzeRequest {
            bpm_exact: true,
            ..request(&stem, None, &out)
        },
        &untracked(transcriber()),
    )
    .unwrap();
    let written = (
        fs::read(&analysis.layers).unwrap(),
        fs::read(analysis.notes.as_ref().unwrap()).unwrap(),
    );
    let request = notes_request(&stem, &out, 120.0, true);
    let error = notes(&request, &untracked(transcriber())).unwrap_err();
    assert!(
        matches!(error, AnalyzeError::NotesDescribed { .. }),
        "{error}"
    );
    assert!(error.to_string().contains("--force"), "{error}");
    let kept = (
        fs::read(out.join(LAYERS_FILE)).unwrap(),
        fs::read(out.join(NOTES_FILE)).unwrap(),
    );
    assert_eq!(kept, written);
    let forced = NotesRequest {
        force: true,
        ..request
    };
    let report = notes(&forced, &untracked(transcriber())).unwrap();
    assert!(!out.join(LAYERS_FILE).exists());
    assert_ne!(fs::read(&report.path).unwrap(), written.1);
    assert!(out.join(SPECTROGRAM_FILE).exists());
    // With no report that describes it, `notes` replaces its own file.
    notes(&request, &untracked(transcriber())).unwrap();
}

/// `notes` decodes and writes at the refined tempo. 117 BPM keeps notes of
/// 6 frames or more and 120 BPM of 5, so the minimum note length shows
/// which tempo reached the decoder.
#[test]
fn the_refined_tempo_reaches_the_decoder_and_the_midi_file() {
    let dir = tempfile::tempdir().unwrap();
    let stem = bass_line(dir.path(), 117.0, 9);
    let out = dir.path().join("out");
    let report = notes(
        &notes_request(&stem, &out, 120.0, false),
        &untracked(transcriber()),
    )
    .unwrap();
    assert_eq!(report.tempo.trusted(), Some(Bpm::new(117.0).unwrap()));
    let bpm = file_bpm(&fs::read(&report.path).unwrap());
    assert!((bpm - 117.0).abs() <= 0.01, "{bpm}");
    let frames = report.transcription.settings.min_note_frames;
    assert_eq!(frames, min_note_frames(Bpm::new(117.0).unwrap()));
    assert_ne!(frames, min_note_frames(Bpm::new(120.0).unwrap()));
}

/// `--bpm-exact` writes the file at `--bpm`, byte for byte as the writer
/// gives it, on a stem whose tempo would otherwise refine.
#[test]
fn bpm_exact_writes_the_given_tempo_byte_for_byte() {
    let dir = tempfile::tempdir().unwrap();
    let stem = bass_line(dir.path(), 234.0, 10);
    let mono = sound_to_patch::audio::decode(&stem)
        .unwrap()
        .into_audio()
        .mono();
    let refined = Tempo::refine(
        &mono,
        SampleRate::new(22_050).unwrap(),
        Bpm::new(240.0).unwrap(),
    )
    .unwrap();
    assert_eq!(refined.trusted(), Some(Bpm::new(234.0).unwrap()));

    let out = dir.path().join("out");
    let report = notes(
        &notes_request(&stem, &out, 240.0, true),
        &untracked(transcriber()),
    )
    .unwrap();
    assert_eq!(report.tempo.used(), Bpm::new(240.0).unwrap());
    assert_eq!(
        report.tempo.summary(),
        "tempo: 240 BPM as given (--bpm-exact)"
    );
    let expected = write_named_midi(
        &report.transcription.notes,
        Bpm::new(240.0).unwrap(),
        Velocities::Constant(DEFAULT_VELOCITY),
        "line.wav",
    )
    .unwrap();
    assert_eq!(fs::read(&report.path).unwrap(), expected);
    assert!((file_bpm(&expected) - 240.0).abs() < 1e-9);
}

/// `analyze --notes` on a stem of 40 beats records the given, refined and
/// used tempo, and reads a note file at the refined tempo.
#[test]
fn the_report_records_the_given_refined_and_used_tempo() {
    let dir = tempfile::tempdir().unwrap();
    let stem = bass_line(dir.path(), 119.62, 40);
    let notes = midi(dir.path(), 119.62);
    let out = dir.path().join("out");
    let report = run(&stem, Some(&notes), &out);
    let group = &report["stem"];
    assert_eq!(group["bpm"], 119.62);
    assert_eq!(group["bpm_given"], 120.0);
    assert_eq!(group["bpm_refined"]["value"], 119.62);
    assert_eq!(group["bpm_refined"]["confidence"]["level"], "high");
    assert!(validator().is_valid(&report));
}

/// `analyze --bpm-exact` keeps `--bpm` and records why no tempo was
/// refined.
#[test]
fn bpm_exact_leaves_the_refined_tempo_empty() {
    let dir = tempfile::tempdir().unwrap();
    let stem = bass_line(dir.path(), 119.62, 40);
    let notes = midi(dir.path(), 120.0);
    let out = dir.path().join("out");
    let exact = AnalyzeRequest {
        bpm_exact: true,
        ..request(&stem, Some(&notes), &out)
    };
    let analysis = analyze(&exact, &transcriber()).unwrap();
    assert_eq!(analysis.tempo.used(), Bpm::new(120.0).unwrap());
    let report: Value =
        serde_json::from_str(&fs::read_to_string(&analysis.layers).unwrap()).unwrap();
    let group = &report["stem"];
    assert_eq!(group["bpm"], 120.0);
    assert_eq!(group["bpm_refined"]["value"], Value::Null);
    assert_eq!(
        group["bpm_refined"]["confidence"]["reasons"],
        json!([{"code": "bpm_exact", "value": true}])
    );
    // Nothing was refined, so a note file at another tempo names --bpm only.
    let refined = midi(dir.path(), 119.62);
    let exact = AnalyzeRequest {
        bpm_exact: true,
        ..request(&stem, Some(&refined), &out)
    };
    let message = analyze(&exact, &transcriber()).unwrap_err().to_string();
    assert!(message.contains("but --bpm is 120. Export"), "{message}");
}

/// `analyze` without `--notes` decodes at the refined tempo: 117 BPM keeps
/// notes of 6 frames or more and 120 BPM of 5.
#[test]
fn analyze_decodes_at_the_refined_tempo() {
    let dir = tempfile::tempdir().unwrap();
    let stem = bass_line(dir.path(), 117.0, 9);
    let out = dir.path().join("out");
    let request = AnalyzeRequest {
        profile: Some(Profile::Bass),
        ..request(&stem, None, &out)
    };
    let analysis = analyze(&request, &untracked(transcriber())).unwrap();
    assert_eq!(analysis.tempo.trusted(), Some(Bpm::new(117.0).unwrap()));
    let settings = &analysis.transcription.as_ref().unwrap().settings;
    assert_eq!(
        settings.min_note_frames,
        min_note_frames(Bpm::new(117.0).unwrap())
    );
    assert_ne!(
        settings.min_note_frames,
        min_note_frames(Bpm::new(120.0).unwrap())
    );
    let report: Value =
        serde_json::from_str(&fs::read_to_string(&analysis.layers).unwrap()).unwrap();
    assert_eq!(
        report["transcription"]["note_params"],
        serde_json::to_value(settings.note_params()).unwrap()
    );
}

/// A refined tempo that is not trusted is not one a note file may match:
/// a file at it fails, and the error names `--bpm` only.
#[test]
fn a_note_file_at_an_untrusted_refined_tempo_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let stem = dir.path().join("pad.wav");
    let samples = crate::tempo::lfo_pad(123.0, 1.0, 1, 37);
    write_wav16(&stem, 22_050, std::slice::from_ref(&samples));
    let mono = sound_to_patch::audio::decode(&stem)
        .unwrap()
        .into_audio()
        .mono();
    let tempo = Tempo::refine(
        &mono,
        SampleRate::new(22_050).unwrap(),
        Bpm::new(120.0).unwrap(),
    )
    .unwrap();
    assert_eq!(tempo.trusted(), None, "{}", tempo.summary());
    let refined = tempo.refined().value().unwrap().get();
    assert!((refined - 120.0).abs() > 0.5, "{refined}");
    let notes = midi(dir.path(), refined);
    let out = dir.path().join("out");
    let message = analyze(&request(&stem, Some(&notes), &out), &transcriber())
        .unwrap_err()
        .to_string();
    assert!(message.contains("but --bpm is 120. Export"), "{message}");
}

/// The names in a folder, sorted, hidden ones included.
fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// The bytes of the three report files, `None` for a missing file or a
/// folder.
fn report_files(dir: &Path) -> [Option<Vec<u8>>; 3] {
    [LAYERS_FILE, SPECTROGRAM_FILE, NOTES_FILE].map(|name| fs::read(dir.join(name)).ok())
}

/// A rename onto a folder fails, so a folder at the place of one file
/// fails the write at that file's rename: `notes.mid` first, then
/// `spectrogram.png`, then `layers.json`. Each failure leaves the files the
/// folder held before the run and no hidden temporary file.
#[test]
fn a_failed_rename_leaves_the_old_report_and_no_temporary_file() {
    let dir = tempfile::tempdir().unwrap();
    let wav = stem(dir.path());
    let notes = given(dir.path());
    for blocked in [NOTES_FILE, SPECTROGRAM_FILE, LAYERS_FILE] {
        let out = dir.path().join(format!("out-{blocked}"));
        run(&wav, Some(&notes), &out);
        fs::remove_file(out.join(blocked)).unwrap();
        fs::create_dir(out.join(blocked)).unwrap();
        fs::write(out.join(blocked).join("keep"), b"x").unwrap();
        let before = report_files(&out);
        let forced = AnalyzeRequest {
            force: true,
            ..request(&wav, Some(&notes), &out)
        };
        let error = analyze(&forced, &transcriber()).unwrap_err().to_string();
        assert!(error.starts_with("cannot write "), "{blocked}: {error}");
        assert_eq!(report_files(&out), before, "{blocked}");
        assert_eq!(
            entries(&out),
            [LAYERS_FILE, NOTES_FILE, SPECTROGRAM_FILE],
            "{blocked}"
        );
        assert!(out.join(blocked).join("keep").is_file(), "{blocked}");
    }
}

/// `notes --force` removes a report that describes the final notes only
/// when the new `notes.mid` lands: a failed rename restores the report.
#[test]
fn a_failed_notes_write_keeps_the_report_that_describes_the_notes() {
    let dir = tempfile::tempdir().unwrap();
    let stem = bass_line(dir.path(), 120.0, 9);
    let out = dir.path().join("out");
    analyze(
        &AnalyzeRequest {
            bpm_exact: true,
            ..request(&stem, None, &out)
        },
        &untracked(transcriber()),
    )
    .unwrap();
    fs::remove_file(out.join(NOTES_FILE)).unwrap();
    fs::create_dir(out.join(NOTES_FILE)).unwrap();
    fs::write(out.join(NOTES_FILE).join("keep"), b"x").unwrap();
    let before = report_files(&out);
    let forced = NotesRequest {
        force: true,
        ..notes_request(&stem, &out, 120.0, true)
    };
    let error = notes(&forced, &untracked(transcriber()))
        .unwrap_err()
        .to_string();
    assert!(error.starts_with("cannot write "), "{error}");
    assert_eq!(report_files(&out), before);
    assert_eq!(entries(&out), [LAYERS_FILE, NOTES_FILE, SPECTROGRAM_FILE]);
}

/// A stem whose name without the extension is `.` or `..` has no folder
/// name of its own: the default folder would be the stem's folder or its
/// parent. A name without an extension, such as `.wav` or `noext`, would
/// name the stem file itself. Both commands stop before they write and ask
/// for `--out`.
#[test]
fn a_stem_named_dot_wav_or_dot_dot_wav_is_refused_without_out() {
    let dir = tempfile::tempdir().unwrap();
    let wav = stem(dir.path());
    let inner = dir.path().join("outer").join("inner");
    fs::create_dir_all(&inner).unwrap();
    let no_extension = "gives no folder name: it has no extension to strip, so the folder would be the stem file itself. Pass --out <dir>";
    for (name, cause) in [
        (
            "..wav",
            "gives no folder name: without its extension it is `.`. Pass --out <dir>",
        ),
        (
            "...wav",
            "gives no folder name: without its extension it is `..`. Pass --out <dir>",
        ),
        (".wav", no_extension),
        ("noext", no_extension),
    ] {
        let odd = inner.join(name);
        fs::copy(&wav, &odd).unwrap();
        let request = AnalyzeRequest {
            out: None,
            ..request(&odd, None, dir.path())
        };
        let message = analyze(&request, &transcriber()).unwrap_err().to_string();
        assert_eq!(message, format!("`{name}` {cause}"));
        let notes_request = NotesRequest {
            out: None,
            ..notes_request(&odd, dir.path(), 120.0, true)
        };
        let message = notes(&notes_request, &untracked(transcriber()))
            .unwrap_err()
            .to_string();
        assert!(
            message.contains(&format!("`{name}` gives no folder name")),
            "{message}"
        );
        fs::remove_file(&odd).unwrap();
    }
    assert_eq!(entries(&inner), Vec::<String>::new());
    assert_eq!(entries(&dir.path().join("outer")), ["inner"]);
}

/// A `spectrogram.png` that no report in the folder owns is a file of the
/// user's: `analyze` stops unless `--force`.
#[test]
fn a_spectrogram_without_a_report_is_kept_unless_forced() {
    let dir = tempfile::tempdir().unwrap();
    let wav = stem(dir.path());
    let notes = given(dir.path());
    let out = dir.path().join("pictures");
    fs::create_dir(&out).unwrap();
    fs::write(out.join(SPECTROGRAM_FILE), b"a picture").unwrap();
    let message = analyze(&request(&wav, Some(&notes), &out), &transcriber())
        .unwrap_err()
        .to_string();
    assert_eq!(
        message,
        format!(
            "{} exists, and no report in the folder owns it. Pass --out <dir> for this stem, or --force to replace it",
            out.join(SPECTROGRAM_FILE).display()
        )
    );
    assert_eq!(fs::read(out.join(SPECTROGRAM_FILE)).unwrap(), b"a picture");
    assert_eq!(entries(&out), [SPECTROGRAM_FILE]);
    let forced = AnalyzeRequest {
        force: true,
        ..request(&wav, Some(&notes), &out)
    };
    analyze(&forced, &transcriber()).unwrap();
    assert_ne!(fs::read(out.join(SPECTROGRAM_FILE)).unwrap(), b"a picture");
}

/// A `layers.json` that is not JSON, or names no `source`, is not a report
/// of this tool, and a `notes.mid` with no track name holds no notes of
/// it. The refusal says so, not that the file is a report of no stem.
#[test]
fn a_file_that_is_not_a_report_is_named_as_such() {
    let dir = tempfile::tempdir().unwrap();
    let wav = stem(dir.path());
    let notes = given(dir.path());
    for text in ["{\"x\":1}", "[1"] {
        let out = dir.path().join("k1");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join(LAYERS_FILE), text).unwrap();
        let message = analyze(&request(&wav, Some(&notes), &out), &transcriber())
            .unwrap_err()
            .to_string();
        assert_eq!(
            message,
            format!(
                "{} exists but is not a report of this tool: it cannot be read, is not JSON or names no `source`. Pass --out <dir> for this stem, or --force to replace it",
                out.join(LAYERS_FILE).display()
            )
        );
        let message = notes_of(&wav, &out);
        assert!(
            message.ends_with("names no `source`. Pass --out <dir> for this stem, or --force to write notes.mid next to it"),
            "{message}"
        );
    }
    let out = dir.path().join("k2");
    fs::create_dir_all(&out).unwrap();
    fs::write(out.join(NOTES_FILE), b"not midi").unwrap();
    let message = analyze(&request(&wav, Some(&notes), &out), &transcriber())
        .unwrap_err()
        .to_string();
    assert_eq!(
        message,
        format!(
            "{} exists but holds no notes of this tool: it cannot be read as MIDI or has no track name. Pass --out <dir> for this stem, or --force to replace it",
            out.join(NOTES_FILE).display()
        )
    );
}

/// The refusal of `notes` for `stem` into `out`.
fn notes_of(stem: &Path, out: &Path) -> String {
    notes(
        &notes_request(stem, out, 120.0, true),
        &untracked(transcriber()),
    )
    .unwrap_err()
    .to_string()
}

/// The SHA-256 of the file that [`stem`] writes, from `shasum -a 256`.
const STEM_SHA256: &str = "02f9e88c315f89918d42c0dd5578da54c6e5f4c54a0aab377e0ae42f829a4bb9";

/// Whether a value is a SHA-256 in lower-case hex.
fn is_sha256(value: &Value) -> bool {
    value.as_str().is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// Two songs each with a `bass.wav`, analysed into one folder: the report
/// records the checksum of the stem file, so the second stem is refused
/// though its name matches, and `--force` replaces the report. The same
/// file again passes. A new export of one stem is the same case.
#[test]
fn a_stem_with_the_same_name_and_other_audio_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (d1, d2) = (dir.path().join("d1"), dir.path().join("d2"));
    fs::create_dir_all(&d1).unwrap();
    fs::create_dir_all(&d2).unwrap();
    let first = stem(&d1);
    let second = d2.join("bass.wav");
    write_wav16(
        &second,
        RATE,
        &[sine(110.0, 0.0, f64::from(RATE), 3 * SECOND)],
    );
    let notes = given(dir.path());
    let out = dir.path().join("common");
    let report = run(&first, Some(&notes), &out);
    assert_eq!(report["source_sha256"], STEM_SHA256);
    assert!(validator().is_valid(&report));
    run(&first, Some(&notes), &out);
    let before = report_files(&out);
    let message = analyze(&request(&second, Some(&notes), &out), &transcriber())
        .unwrap_err()
        .to_string();
    assert_eq!(
        message,
        format!(
            "{} is the report of `bass.wav` with other content: another stem of that name, or this stem changed since that run. Pass --out <dir> for this stem, or --force to replace it",
            out.join(LAYERS_FILE).display()
        )
    );
    assert_eq!(report_files(&out), before);
    // A report without notes, as of a stem without notes, does not stop
    // `notes` by itself, so the checksum does.
    let beside = dir.path().join("beside");
    fs::create_dir(&beside).unwrap();
    let mut no_notes = report.clone();
    no_notes["notes"] = Value::Null;
    fs::write(beside.join(LAYERS_FILE), no_notes.to_string()).unwrap();
    let message = notes_of(&second, &beside);
    assert!(
        message.starts_with(&format!(
            "{} is the report of `bass.wav` with other content",
            beside.join(LAYERS_FILE).display()
        )),
        "{message}"
    );
    assert_eq!(entries(&beside), [LAYERS_FILE]);
    let forced = AnalyzeRequest {
        force: true,
        ..request(&second, Some(&notes), &out)
    };
    analyze(&forced, &transcriber()).unwrap();
    let report: Value =
        serde_json::from_str(&fs::read_to_string(out.join(LAYERS_FILE)).unwrap()).unwrap();
    assert!(
        is_sha256(&report["source_sha256"]),
        "{}",
        report["source_sha256"]
    );
    assert_ne!(report["source_sha256"], STEM_SHA256);
}

/// A report from before the checksum names its stem only: the guard falls
/// back to the name, so a stem of that name replaces it.
#[test]
fn a_report_without_a_checksum_is_matched_by_name() {
    let dir = tempfile::tempdir().unwrap();
    let wav = stem(dir.path());
    let notes = given(dir.path());
    let out = dir.path().join("out");
    let mut report = run(&wav, Some(&notes), &out);
    report.as_object_mut().unwrap().remove("source_sha256");
    assert!(!validator().is_valid(&report));
    fs::write(out.join(LAYERS_FILE), report.to_string()).unwrap();
    let report = run(&wav, Some(&notes), &out);
    assert_eq!(report["source_sha256"], STEM_SHA256);
}

/// The report files get the mode that a new file gets in the folder, as
/// with `File::create`, not the owner-only mode of a temporary file.
#[cfg(unix)]
#[test]
fn the_report_files_get_the_mode_of_a_new_file() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().unwrap();
    let wav = stem(dir.path());
    let out = dir.path().join("out");
    run(&wav, Some(&given(dir.path())), &out);
    let plain = dir.path().join("plain");
    fs::write(&plain, b"x").unwrap();
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    for name in [LAYERS_FILE, SPECTROGRAM_FILE, NOTES_FILE] {
        assert_eq!(mode(&out.join(name)), mode(&plain), "{name}");
    }
}

/// A run killed during its renames can leave the earlier `layers.json` as
/// a hidden backup and no `layers.json`, here after the new `notes.mid`
/// landed and the old one became a backup too. The next run of either
/// command stops and names every backup, so `notes` does not replace the
/// `notes.mid` the report describes. A forced run removes the backups once
/// its files are in place, and a plain run of the stem then passes.
#[test]
fn a_backup_left_by_a_killed_run_is_named_before_any_write() {
    let dir = tempfile::tempdir().unwrap();
    let wav = stem(dir.path());
    let notes_file = given(dir.path());
    let out = dir.path().join("out");
    run(&wav, Some(&notes_file), &out);
    let report = out.join(".layers.json.a1B2c3.old");
    let old_notes = out.join(".notes.mid.q1W2e3.old");
    fs::rename(out.join(LAYERS_FILE), &report).unwrap();
    fs::copy(out.join(NOTES_FILE), &old_notes).unwrap();
    let before = (
        fs::read(&report).unwrap(),
        fs::read(&old_notes).unwrap(),
        report_files(&out),
    );
    let expected = format!(
        "the folder holds backups that a run left when it stopped before it ended: {} of layers.json, {} of notes.mid. Rename each backup to its file name to restore the earlier files, or pass --force to write new files and remove the backups",
        report.display(),
        old_notes.display()
    );
    let message = analyze(&request(&wav, Some(&notes_file), &out), &transcriber())
        .unwrap_err()
        .to_string();
    assert_eq!(message, expected);
    assert_eq!(notes_of(&wav, &out), expected);
    let after = (
        fs::read(&report).unwrap(),
        fs::read(&old_notes).unwrap(),
        report_files(&out),
    );
    assert_eq!(after, before);
    let forced = AnalyzeRequest {
        force: true,
        ..request(&wav, Some(&notes_file), &out)
    };
    analyze(&forced, &transcriber()).unwrap();
    assert_eq!(entries(&out), [LAYERS_FILE, NOTES_FILE, SPECTROGRAM_FILE]);
    run(&wav, Some(&notes_file), &out);
    assert_eq!(entries(&out), [LAYERS_FILE, NOTES_FILE, SPECTROGRAM_FILE]);
}
