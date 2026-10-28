//! The analysis report (#56): `layers.json`, its schema, the spectrogram
//! and the `--notes` path.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

use midly::num::{u4, u7, u15, u24, u28};
use midly::{Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind};
use serde_json::{Value, json};
use sound_to_patch::audio::{SampleRate, Tempo};
use sound_to_patch::notes::{
    DEFAULT_VELOCITY, PitchTrack, Profile, Transcriber, Velocities, min_note_frames,
    write_named_midi,
};
use sound_to_patch::report::spectrogram::{PLOT_HEIGHT, PLOT_WIDTH, Spectrogram};
use sound_to_patch::report::{
    AnalyzeRequest, LAYERS_FILE, NotesRequest, SPECTROGRAM_FILE, analyze, notes,
};
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

/// The notes of [`stem`] at 120 BPM. A report test passes them, so the
/// notes step, whose f0 track is slow in a debug build, runs only where a
/// test needs it.
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

fn transcriber() -> Transcriber {
    Transcriber::new().unwrap()
}

/// `transcriber` with an empty f0 track. The pYIN track takes minutes per
/// stem in a debug build, and a test of the tempo or the notes step's
/// wiring does not read it.
fn untracked(transcriber: Transcriber) -> Transcriber {
    transcriber.with_tracker(|_, _| Ok(PitchTrack::default()))
}

fn run(stem: &Path, notes: Option<&Path>, out: &Path) -> Value {
    analyze(&request(stem, notes, out), &transcriber()).unwrap();
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
    assert_eq!(request.out_dir(), dir.path().join("bass"));
    let analysis = analyze(&request, &transcriber()).unwrap();
    assert_eq!(analysis.layers, dir.path().join("bass").join(LAYERS_FILE));
    assert_eq!(
        analysis.spectrogram,
        dir.path().join("bass").join(SPECTROGRAM_FILE)
    );
    assert!(analysis.layers.is_file() && analysis.skipped.is_empty());
    // The temporary siblings are renamed away.
    let names: Vec<String> = fs::read_dir(dir.path().join("bass"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 2, "{names:?}");
}

#[test]
fn an_output_folder_that_is_a_file_fails_with_a_fix() {
    let dir = tempfile::tempdir().unwrap();
    let wav = stem(dir.path());
    // A stem without an extension names its own default folder.
    let bare = dir.path().join("noext");
    fs::copy(&wav, &bare).unwrap();
    let file = dir.path().join("taken");
    fs::write(&file, b"x").unwrap();
    for (stem, out) in [(&bare, None), (&wav, Some(file.as_path()))] {
        let request = AnalyzeRequest {
            out,
            ..request(stem, None, dir.path())
        };
        let message = analyze(&request, &transcriber()).unwrap_err().to_string();
        let folder = out.map_or(bare.clone(), Path::to_path_buf);
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
    let analysis = analyze(&request(&path, Some(&notes), &out), &counting).unwrap();
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
    let errors: Vec<String> = validator()
        .iter_errors(&report)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
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
