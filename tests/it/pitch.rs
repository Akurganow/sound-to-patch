//! The pitch layer (#114, #66, #67, #148): the sub-band line, pitch drops,
//! glides and vibrato on generated signals.
//!
//! The f0 track takes about half a second per second of audio in a debug
//! build, which adds up over the generated signals, so the glide and
//! vibrato cases read committed fixtures: the notes and the
//! track that the transcriber and the f0 track give for each generated
//! signal. `pitch_fixtures_are_current` rebuilds them in a release build
//! and fails when one differs. It compares the printed floats byte for
//! byte, which holds on one platform and toolchain: another CPU or libm can
//! round a track frame differently.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sound_to_patch::audio::SampleRate;
use sound_to_patch::confidence::Level;
use sound_to_patch::estimate::pitch::{
    DROP_FIT_START, EXTENT_SHARE, PitchInput, PitchLayer, SubBandLine, band_onset, fit_drop,
};
use sound_to_patch::notes::{
    DEFAULT_VELOCITY, Note, NoteList, PitchTrack, TrackError, TrackFrame, Transcriber, Velocities,
    Voicing, transcribe_stem, write_midi,
};
use sound_to_patch::report::{Analysis, AnalyzeRequest, analyze};
use sound_to_patch::synth::Synth;
use sound_to_patch::units::{Bpm, Decibels, Hz, MidiPitch, Seconds};

use crate::report::{shape, validator};
use crate::support::cast::round_to_usize;
use crate::support::golden::{Golden, Tolerance, assert_close, float, floats};
use crate::support::pitch::{BassLine, Glide, Hits, PitchDrop, RcGlide, Trill, Vibrato};
use crate::support::rng::gaussian;
use crate::support::signals::write_wav16;

const RATE: u32 = 22_050;
/// The rate of the fixtures that check the unison glides at 44.1 kHz.
const RATE_44: u32 = 44_100;
const FIXTURES: &str = "tests/fixtures/pitch";

fn rate() -> SampleRate {
    SampleRate::new(RATE).unwrap()
}

fn s(value: f64) -> Seconds {
    Seconds::new(value).unwrap()
}

/// A generated signal with a fixture.
#[derive(Clone, Copy, Debug)]
enum Signal {
    Glide(Glide),
    /// A glide at 44.1 kHz.
    Glide44(Glide),
    /// A glide from silence: 0.2 s of silence, then the glide without its
    /// first hold.
    Scoop(Glide),
    Vibrato(Vibrato),
    Hits(Hits),
    Rc(RcGlide),
    Trill(Trill),
    /// A constant 0.3 for 1 s.
    Dc,
    Bass,
}

impl Signal {
    fn render(self) -> Vec<f64> {
        match self {
            Self::Glide(glide) => glide.render(f64::from(RATE)),
            Self::Glide44(glide) => glide.render(f64::from(RATE_44)),
            Self::Vibrato(vibrato) => vibrato.render(f64::from(RATE)),
            Self::Scoop(glide) => {
                let rate = f64::from(RATE);
                let mut out = vec![0.0; round_to_usize(0.2 * rate)];
                out.extend_from_slice(&glide.render(rate)[round_to_usize(glide.hold_s * rate)..]);
                out
            }
            Self::Hits(hits) => hits.render(f64::from(RATE)),
            Self::Rc(rc) => rc.render(f64::from(RATE)),
            Self::Trill(trill) => trill.render(f64::from(RATE)),
            Self::Dc => vec![0.3; round_to_usize(f64::from(RATE))],
            Self::Bass => BassLine::render(f64::from(RATE)),
        }
    }

    /// The sample rate the signal renders at, in hertz.
    fn hz(self) -> u32 {
        if matches!(self, Self::Glide44(_)) {
            RATE_44
        } else {
            RATE
        }
    }

    fn rate(self) -> SampleRate {
        SampleRate::new(self.hz()).unwrap()
    }

    fn recipe(self) -> Value {
        match self {
            Self::Bass => json!({
                "kind": "bass_line",
                "notes": BassLine::NOTES,
                "drop": BassLine::DROP,
                "glide": BassLine::GLIDE,
            }),
            Self::Glide(g) | Self::Glide44(g) => json!({
                "kind": "glide",
                "start_hz": g.start_hz,
                "semitones": g.semitones,
                "full_s": g.full_s,
                "p": g.p,
                "hold_s": g.hold_s,
                "voices": g.voices,
                "detune_ct": g.detune_ct,
                "in_phase": g.in_phase,
            }),
            Self::Scoop(g) => json!({
                "kind": "scoop",
                "lead_s": 0.2,
                "start_hz": g.start_hz,
                "semitones": g.semitones,
                "full_s": g.full_s,
                "p": g.p,
                "hold_s": g.hold_s,
            }),
            Self::Hits(h) => json!({
                "kind": "hits",
                "pitches": h.pitches,
                "hit_s": h.hit_s,
                "decay_s": h.decay_s,
                "gap_s": h.gap_s,
                "lead_s": h.lead_s,
                "drive": h.drive,
                "continuous": h.continuous,
            }),
            Self::Rc(r) => json!({
                "kind": "rc_glide",
                "start_hz": r.start_hz,
                "semitones": r.semitones,
                "tau_s": r.tau_s,
                "hold_s": r.hold_s,
                "tail_s": r.tail_s,
            }),
            Self::Trill(t) => json!({
                "kind": "trill",
                "f0_hz": t.f0_hz,
                "rate_hz": t.rate_hz,
                "length_s": t.length_s,
                "lead_s": t.lead_s,
            }),
            Self::Dc => json!({"kind": "dc", "value": 0.3, "length_s": 1.0}),
            Self::Vibrato(v) => json!({
                "kind": "vibrato",
                "f0_hz": v.f0_hz,
                "rate_hz": v.rate_hz,
                "extent_ct": v.extent_ct,
                "length_s": v.length_s,
                "lead_s": v.lead_s,
                "voices": v.voices,
                "detune_ct": v.detune_ct,
            }),
        }
    }
}

const GLIDE_TIMES: [f64; 3] = [0.05, 0.15, 0.4];
const CURVES: [f64; 2] = [1.0, 2.5];
const VIBRATO_RATES: [f64; 9] = [3.0, 4.0, 5.0, 5.5, 6.0, 7.0, 8.0, 10.0, 12.0];

/// Every signal with a fixture, by name.
fn cases() -> Vec<(String, Signal)> {
    let mut cases = Vec::new();
    for full in GLIDE_TIMES {
        for p in CURVES {
            cases.push((
                glide_name("mono", full, p),
                Signal::Glide(Glide::mono(full, p)),
            ));
            if full > 0.1 {
                cases.push((
                    glide_name("unison", full, p),
                    Signal::Glide(Glide::unison(full, p)),
                ));
            }
        }
    }
    cases.push((
        "glide-step".to_owned(),
        Signal::Glide(Glide::mono(0.001, 1.0)),
    ));
    for rate in VIBRATO_RATES {
        cases.push((
            vibrato_name("mono", rate, 35.0),
            Signal::Vibrato(Vibrato::mono(rate, 35.0)),
        ));
    }
    for (rate, extent) in [(5.5, 60.0), (2.0, 35.0), (15.0, 35.0), (5.5, 5.0)] {
        cases.push((
            vibrato_name("mono", rate, extent),
            Signal::Vibrato(Vibrato::mono(rate, extent)),
        ));
    }
    cases.push((
        vibrato_name("unison", 5.5, 35.0),
        Signal::Vibrato(Vibrato::unison(5.5, 35.0)),
    ));
    for (full, p) in [(0.15, 1.0), (0.15, 2.5), (0.2, 1.0), (0.4, 1.0), (0.4, 2.5)] {
        cases.push((
            format!("{}-44khz", glide_name("unison", full, p)),
            Signal::Glide44(Glide::unison(full, p)),
        ));
    }
    for (full, p) in [(0.2, 1.0), (0.3, 1.0), (0.3, 2.5)] {
        cases.push((
            format!("{}-44khz", glide_name("unison-in-phase", full, p)),
            Signal::Glide44(Glide {
                in_phase: true,
                ..Glide::unison(full, p)
            }),
        ));
    }
    cases.extend(review_cases());
    cases
}

const SAME_HITS: [f64; 5] = [29.0; 5];
const SAME_THEN_UP: [f64; 6] = [29.0, 29.0, 29.0, 29.0, 36.0, 43.0];
const STEPS_A: [f64; 6] = [36.0, 34.0, 41.0, 39.0, 39.0, 42.0];
const STEPS_B: [f64; 6] = [34.0, 39.0, 45.0, 40.0, 38.0, 43.0];
const SEMITONE_STEP: [f64; 5] = [35.0, 38.0, 43.0, 44.0, 44.0];

/// The signals behind the review of the first pitch layer: retriggered and
/// stepping 808s, short and unison glides away from 110 Hz, a fall into
/// the band of the line, glides from silence, a trill, a ±100-cent vibrato
/// and DC.
fn review_cases() -> Vec<(String, Signal)> {
    let mut cases = hit_cases();
    cases.extend(review_glides());
    cases.extend([
        (
            "trill-semitone-6hz".to_owned(),
            Signal::Trill(Trill::new(6.0)),
        ),
        (
            vibrato_name("mono", 3.0, 100.0),
            Signal::Vibrato(Vibrato::mono(3.0, 100.0)),
        ),
        ("dc".to_owned(), Signal::Dc),
    ]);
    cases
}

/// Retriggered 808 hits at one pitch and 808 lines that step.
fn hit_cases() -> Vec<(String, Signal)> {
    let legato = |pitches: &'static [f64]| Hits {
        hit_s: 0.5,
        decay_s: 0.45,
        ..Hits::new(pitches, 0.0)
    };
    vec![
        (
            "hits-29x5-gap0ms".to_owned(),
            Signal::Hits(Hits::new(&SAME_HITS, 0.0)),
        ),
        (
            "hits-29x5-gap50ms".to_owned(),
            Signal::Hits(Hits::new(&SAME_HITS, 0.05)),
        ),
        (
            "hits-29-36-43-legato".to_owned(),
            Signal::Hits(Hits {
                hit_s: 0.4,
                decay_s: 0.36,
                ..Hits::new(&SAME_THEN_UP, 0.0)
            }),
        ),
        (
            "steps-808-legato".to_owned(),
            Signal::Hits(legato(&STEPS_A)),
        ),
        (
            "steps-808-driven-legato".to_owned(),
            Signal::Hits(Hits {
                drive: 2.5,
                ..legato(&STEPS_A)
            }),
        ),
        (
            "steps-808-staccato".to_owned(),
            Signal::Hits(Hits {
                hit_s: 0.425,
                decay_s: 0.45,
                ..Hits::new(&STEPS_B, 0.075)
            }),
        ),
        (
            "steps-808-continuous-semitone".to_owned(),
            Signal::Hits(Hits {
                hit_s: 0.45,
                decay_s: 0.8,
                continuous: true,
                ..Hits::new(&SEMITONE_STEP, 0.0)
            }),
        ),
    ]
}

/// Short glides away from 110 Hz, a fall into the band, unisons whose
/// voices start in phase, and glides from silence.
fn review_glides() -> Vec<(String, Signal)> {
    let short = |start_hz: f64, semitones: f64| Glide {
        start_hz,
        semitones,
        ..Glide::mono(0.05, 1.0)
    };
    let scoop = |full_s: f64, p: f64| Glide {
        start_hz: 82.4,
        semitones: 5.0,
        ..Glide::mono(full_s, p)
    };
    let mut cases = vec![
        (
            "glide-mono-82.4hz-5st-50ms-p1".to_owned(),
            Signal::Glide(short(82.4, 5.0)),
        ),
        (
            "glide-mono-220hz-down-4st-50ms-p1".to_owned(),
            Signal::Glide(short(220.0, -4.0)),
        ),
        (
            "glide-mono-220hz-down-4st-200ms-p2.5".to_owned(),
            Signal::Glide(Glide {
                start_hz: 220.0,
                semitones: -4.0,
                ..Glide::mono(0.2, 2.5)
            }),
        ),
        (
            glide_name("unison", 0.2, 1.0),
            Signal::Glide(Glide::unison(0.2, 1.0)),
        ),
        (
            "scoop-mono-5st-150ms-p1".to_owned(),
            Signal::Scoop(scoop(0.15, 1.0)),
        ),
        (
            "scoop-mono-5st-300ms-p2.5".to_owned(),
            Signal::Scoop(scoop(0.3, 2.5)),
        ),
    ];
    for (name, start_hz, semitones, tau) in [
        ("rc-glide-110hz-7st-tau50ms", 110.0, 7.0, 0.05),
        ("rc-glide-196hz-down-5st-tau80ms", 196.0, -5.0, 0.08),
        ("rc-glide-110hz-1.2st-tau30ms", 110.0, 1.2, 0.03),
    ] {
        cases.push((
            name.to_owned(),
            Signal::Rc(RcGlide::new(start_hz, semitones, tau)),
        ));
    }
    for (full, p) in [(0.2, 1.0), (0.2, 2.5), (0.3, 2.5), (0.3, 1.0)] {
        cases.push((
            glide_name("unison-in-phase", full, p),
            Signal::Glide(Glide {
                in_phase: true,
                ..Glide::unison(full, p)
            }),
        ));
    }
    cases
}

fn glide_name(voices: &str, full: f64, p: f64) -> String {
    format!("glide-{voices}-{:.0}ms-p{p}", 1000.0 * full)
}

fn vibrato_name(voices: &str, rate: f64, extent: f64) -> String {
    format!("vibrato-{voices}-{rate}hz-{extent}ct")
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(FIXTURES).join(format!("{name}.json"))
}

/// The fixture document of a transcription.
fn document(name: &str, signal: Signal, notes: &NoteList, track: &PitchTrack) -> Value {
    json!({
        "name": name,
        "generator": "tests/it/pitch.rs:pitch_fixtures_are_current",
        "recipe": signal.recipe(),
        "sample_rate": signal.hz(),
        "notes": notes.notes().iter().map(|n| json!([n.onset().get(), n.offset().get(), n.pitch().get()])).collect::<Vec<_>>(),
        "frames": track.frames().iter().map(|f| json!([f.time.get(), f.f0.map(Hz::get)])).collect::<Vec<_>>(),
    })
}

/// The notes and the track of a fixture.
fn fixture(name: &str) -> (NoteList, PitchTrack) {
    let text = fs::read_to_string(fixture_path(name)).unwrap();
    let doc: Value = serde_json::from_str(&text).unwrap();
    let notes = doc["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| {
            Note::new(
                s(n[0].as_f64().unwrap()),
                s(n[1].as_f64().unwrap()),
                MidiPitch::new(n[2].as_f64().unwrap()).unwrap(),
            )
            .unwrap()
        })
        .collect();
    let frames = doc["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| TrackFrame {
            time: s(f[0].as_f64().unwrap()),
            f0: f[1].as_f64().map(|hz| Hz::new(hz).unwrap()),
            level: Decibels::new(0.0).unwrap(),
        })
        .collect();
    (NoteList::new(notes), PitchTrack::from_frames(frames))
}

/// The pitch layer of a fixture, on the audio of its signal.
fn layer(name: &str, signal: Signal) -> PitchLayer {
    let (notes, track) = fixture(name);
    let mono = signal.render();
    PitchLayer::of(&PitchInput {
        mono: &mono,
        rate: signal.rate(),
        track: &track,
        notes: &notes,
        voicing: Voicing::Monophonic,
    })
    .unwrap()
}

fn case(name: &str) -> Signal {
    cases().into_iter().find(|(n, _)| n == name).unwrap().1
}

#[test]
#[ignore = "runs the model and the f0 track; refresh with UPDATE_PITCH_FIXTURES=1 cargo nextest run --release --run-ignored only -E 'test(pitch_fixtures_are_current)'"]
fn pitch_fixtures_are_current() {
    let transcriber = Transcriber::new().unwrap();
    let update = std::env::var_os("UPDATE_PITCH_FIXTURES").is_some();
    let mut stale = Vec::new();
    for (name, signal) in cases().into_iter().chain(std::iter::once(bass_case())) {
        let mono = signal.render();
        let t = transcribe_stem(
            &transcriber,
            &mono,
            signal.rate(),
            Bpm::new(120.0).unwrap(),
            None,
        )
        .unwrap();
        let mut text = serde_json::to_string(&document(&name, signal, &t.notes, &t.track)).unwrap();
        text.push('\n');
        let path = fixture_path(&name);
        if update {
            fs::create_dir_all(FIXTURES).unwrap();
            fs::write(&path, &text).unwrap();
        } else if fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            stale.push(name);
        }
    }
    assert!(stale.is_empty(), "stale fixtures: {stale:?}");
}

/// The bass line of the report test, with its fixture.
fn bass_case() -> (String, Signal) {
    (BASS.to_owned(), Signal::Bass)
}

const BASS: &str = "bass-line";

/// A tracker that returns the committed track of the bass line, whatever
/// it reads.
#[expect(clippy::unnecessary_wraps, reason = "the signature of a tracker")]
fn bass_track(_: &[f64], _: SampleRate) -> Result<PitchTrack, TrackError> {
    Ok(fixture(BASS).1)
}

/// Writes the bass line as a WAV file and its notes as a MIDI file.
fn bass_files(dir: &Path) -> (PathBuf, PathBuf) {
    let wav = dir.join("bass-line.wav");
    write_wav16(&wav, RATE, &[BassLine::render(f64::from(RATE))]);
    let notes = NoteList::new(
        BassLine::NOTES
            .iter()
            .map(|&(on, off, pitch)| {
                Note::new(s(on), s(off), MidiPitch::new(pitch).unwrap()).unwrap()
            })
            .collect(),
    );
    let mid = dir.join("bass-line.mid");
    let bytes = write_midi(
        &notes,
        Bpm::new(120.0).unwrap(),
        Velocities::Constant(DEFAULT_VELOCITY),
    )
    .unwrap();
    fs::write(&mid, bytes).unwrap();
    (wav, mid)
}

fn analyze_bass(wav: &Path, mid: &Path, out: &Path) -> (Analysis, Value) {
    let request = AnalyzeRequest {
        stem: wav,
        bpm: Bpm::new(120.0).unwrap(),
        bpm_exact: true,
        synth: Synth::Vital,
        notes: Some(mid),
        profile: None,
        out: Some(out),
        force: false,
    };
    let transcriber = Transcriber::new().unwrap().with_tracker(bass_track);
    let analysis = analyze(&request, &transcriber).unwrap();
    let report = serde_json::from_str(&fs::read_to_string(&analysis.layers).unwrap()).unwrap();
    (analysis, report)
}

#[test]
fn the_pitch_group_of_a_bass_line_holds_its_glide_and_drop() {
    let dir = tempfile::tempdir().unwrap();
    let (wav, mid) = bass_files(dir.path());
    let (analysis, report) = analyze_bass(&wav, &mid, &dir.path().join("a"));
    let pitch = &report["pitch"];
    let glides = pitch["values"]["glides"].as_array().unwrap();
    assert_eq!(glides.len(), 1, "{pitch:#}");
    let (start, semitones, full, p) = BassLine::GLIDE;
    let time = glides[0]["time_s"].as_f64().unwrap();
    let curve = glides[0]["curve"].as_f64().unwrap();
    assert!(
        (rise(time, curve) / rise(full, p) - 1.0).abs() <= 0.05,
        "{pitch:#}"
    );
    assert!(
        (glides[0]["start_s"].as_f64().unwrap() - start).abs() <= 0.01,
        "{pitch:#}"
    );
    assert!(
        (glides[0]["interval_st"].as_f64().unwrap() - semitones).abs() <= 0.15,
        "{pitch:#}"
    );
    let drops = pitch["values"]["drops"].as_array().unwrap();
    assert_eq!(drops.len(), 1, "{pitch:#}");
    let (f_end, df, tau) = BassLine::DROP;
    assert!(
        (drops[0]["f_end_hz"].as_f64().unwrap() - f_end).abs() <= 1.5,
        "{pitch:#}"
    );
    assert!(
        (drops[0]["df_hz"].as_f64().unwrap() / df - 1.0).abs() <= 0.25,
        "{pitch:#}"
    );
    assert!(
        (drops[0]["tau_s"].as_f64().unwrap() / tau - 1.0).abs() <= 0.25,
        "{pitch:#}"
    );
    assert_eq!(pitch["values"]["vibrato"], json!([]));
    assert_eq!(pitch["suggested_edits"], json!([]));
    let errors: Vec<String> = validator()
        .iter_errors(&report)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    insta::assert_snapshot!(serde_json::to_string_pretty(&shape(pitch)).unwrap());
    // The summary names each record.
    let summary = analysis.summary();
    assert!(
        summary
            .iter()
            .any(|line| line.starts_with("glides: 1, 0 too short: +")),
        "{summary:?}"
    );
    assert!(
        summary
            .iter()
            .any(|line| line.starts_with("pitch drops: 1 of 2 low strikes: ")),
        "{summary:?}"
    );
    assert!(
        summary
            .iter()
            .any(|line| line.starts_with("vibrato: 0 of ")),
        "{summary:?}"
    );
    // A second run writes the same bytes.
    let (again, _) = analyze_bass(&wav, &mid, &dir.path().join("b"));
    assert_eq!(
        fs::read(&analysis.layers).unwrap(),
        fs::read(again.layers).unwrap()
    );
}

/// The 10 to 90 % time of the curve `1 - (1 - x)^p` over a full time.
fn rise(full: f64, p: f64) -> f64 {
    let x = |q: f64| 1.0 - (1.0 - q).powf(1.0 / p);
    (x(0.9) - x(0.1)) * full
}

#[test]
fn mono_glides_of_150_ms_or_more_read_their_rise_within_5_percent() {
    for full in [0.15, 0.4] {
        for p in CURVES {
            let name = glide_name("mono", full, p);
            let read = layer(&name, case(&name));
            assert_eq!(read.glides.len(), 1, "{name}: {:?}", read.glides);
            let glide = &read.glides[0];
            let (time, curve) = (glide.value.time.get(), glide.value.curve);
            let error = rise(time, curve) / rise(full, p) - 1.0;
            assert!(
                error.abs() <= 0.05,
                "{name}: {glide:?}, rise off by {error:.3}"
            );
            if p > 1.0 {
                assert!(curve > 1.0, "{name}: {glide:?}");
            } else {
                assert!((curve - 1.0).abs() <= 0.2, "{name}: {glide:?}");
            }
            assert!(
                (glide.value.interval.get() - 7.0).abs() <= 0.15,
                "{name}: {glide:?}"
            );
            assert!(
                (glide.value.start.get() - 0.5).abs() <= 0.01,
                "{name}: {glide:?}"
            );
            assert!(!glide.value.from_silence);
            assert_eq!(glide.confidence.level(), Some(Level::High), "{name}");
            assert!(!read.flags.glide_too_short);
        }
    }
}

#[test]
fn a_50_ms_glide_reports_none_and_flags_it_too_short() {
    // The fitted time of the two glides away from 110 Hz read 61 and 43 ms
    // over detected spans of 331 and 99 ms.
    for name in [
        glide_name("mono", 0.05, 1.0),
        glide_name("mono", 0.05, 2.5),
        "glide-mono-82.4hz-5st-50ms-p1".to_owned(),
        "glide-mono-220hz-down-4st-50ms-p1".to_owned(),
    ] {
        let read = layer(&name, case(&name));
        assert!(read.glides.is_empty(), "{name}: {:?}", read.glides);
        assert!(read.flags.glide_too_short, "{name}");
        assert_eq!(read.counts.too_short, 1, "{name}");
    }
}

#[test]
fn a_step_reports_no_glide_and_no_flag() {
    let read = layer("glide-step", case("glide-step"));
    assert!(read.glides.is_empty(), "{:?}", read.glides);
    assert!(!read.flags.glide_too_short);
}

#[test]
fn unison_glides_read_within_their_known_error_at_low_confidence() {
    // At 400 ms the unison reads within 5 %; at 150 ms the measured error
    // is the known error of the method, and the confidence is low.
    for (full, p, bound) in [
        (0.4, 1.0, 0.05),
        (0.4, 2.5, 0.05),
        (0.15, 1.0, 0.08),
        (0.15, 2.5, 0.15),
    ] {
        let name = glide_name("unison", full, p);
        let read = layer(&name, case(&name));
        assert_eq!(read.glides.len(), 1, "{name}: {:?}", read.glides);
        let glide = &read.glides[0];
        let error = rise(glide.value.time.get(), glide.value.curve) / rise(full, p) - 1.0;
        assert!(
            error.abs() <= bound,
            "{name}: {glide:?}, rise off by {error:.3}"
        );
        assert_eq!(
            glide.confidence.level(),
            Some(Level::Low),
            "{name}: {glide:?}"
        );
    }
}

#[test]
fn unison_glides_never_read_above_low_confidence() {
    // Voices that start in phase beat hardest, and the track goes unvoiced
    // for up to 75 ms inside the glide. Bridged, the gap keeps the glide
    // whole; read from silence, it once gave 3.9 semitones of 7 at high.
    // The least unison jitter of these fixtures is 3.54 cents, 0.14 over
    // the cut.
    let mut read_any = 0;
    for (name, signal) in cases() {
        let (Signal::Glide(glide) | Signal::Glide44(glide)) = signal else {
            continue;
        };
        if glide.voices == 1 || glide.full_s < 0.1 {
            continue;
        }
        let read = layer(&name, signal);
        assert!(!read.flags.weak_fundamental, "{name}");
        for g in &read.glides {
            assert_eq!(g.confidence.level(), Some(Level::Low), "{name}: {g:?}");
            assert!(
                (g.value.interval.get() - 7.0).abs() <= 0.15,
                "{name}: {g:?}"
            );
            let error = rise(g.value.time.get(), g.value.curve) / rise(glide.full_s, glide.p) - 1.0;
            assert!(error.abs() <= 0.15, "{name}: {g:?}, rise off by {error:.3}");
            read_any += 1;
        }
    }
    assert!(read_any >= 8, "{read_any}");
    let read = layer(
        "glide-unison-in-phase-200ms-p1",
        case("glide-unison-in-phase-200ms-p1"),
    );
    assert_eq!(read.glides.len(), 1, "{:?}", read.glides);
}

#[test]
fn a_glide_from_silence_reads_into_its_note() {
    for (name, full) in [
        ("scoop-mono-5st-150ms-p1", 0.15),
        ("scoop-mono-5st-300ms-p2.5", 0.3),
    ] {
        let read = layer(name, case(name));
        assert_eq!(read.glides.len(), 1, "{name}: {:?}", read.glides);
        let glide = &read.glides[0];
        assert!(glide.value.from_silence, "{name}: {glide:?}");
        // The track's first frames of a tone read low, so a glide from
        // silence reads its interval and time short.
        assert!(
            (glide.value.interval.get() - 5.0).abs() <= 0.6,
            "{name}: {glide:?}"
        );
        assert!(
            (glide.value.time.get() / full - 1.0).abs() <= 0.2,
            "{name}: {glide:?}"
        );
        assert_ne!(glide.confidence.level(), Some(Level::High), "{name}");
    }
}

#[test]
fn an_rc_portamento_reads_on_the_curve_bound_at_low_confidence() {
    // 1 - exp(-t / tau) approaches its target slower than any power of the
    // curve family, so p sits on its bound of 5.
    for (name, semitones) in [
        ("rc-glide-110hz-7st-tau50ms", 7.0),
        ("rc-glide-196hz-down-5st-tau80ms", -5.0),
    ] {
        let read = layer(name, case(name));
        assert_eq!(read.glides.len(), 1, "{name}: {:?}", read.glides);
        let glide = &read.glides[0];
        assert!(
            glide.value.on_bound && glide.value.curve > 4.7,
            "{name}: {glide:?}"
        );
        assert!(
            (glide.value.interval.get() - semitones).abs() <= 0.15,
            "{name}: {glide:?}"
        );
        assert!(
            (glide.value.start.get() - 0.5).abs() <= 0.02,
            "{name}: {glide:?}"
        );
        assert_eq!(glide.confidence.level(), Some(Level::Low), "{name}");
    }
}

#[test]
fn drops_on_a_ringing_808_line_read_without_a_level_rise() {
    // Notes of 0.4 s with a drop of one octave on the second and fourth,
    // with or without a 40 ms release. The note before still rings, so the
    // level rises 9 dB at a 0.3 s decay, under 3 dB at 2 s.
    const LINE: [f64; 5] = [28.0, 31.0, 26.0, 28.0, 33.0];
    const DROPS: [(usize, f64, f64); 2] = [(1, 1.0, 0.025), (3, 1.0, 0.02)];
    for (decay, release) in [(0.3, 0.04), (0.6, 0.0), (1.0, 0.04), (2.0, 0.04)] {
        let hits = Hits {
            hit_s: 0.4,
            decay_s: decay,
            drops: &DROPS,
            release_s: release,
            ..Hits::new(&LINE, 0.0)
        };
        // The notes end at 90 % of each hit, as a MIDI file would give them.
        let notes = |hits: &Hits| -> NoteList {
            let list: Vec<(f64, f64, f64)> = hits
                .notes()
                .into_iter()
                .map(|(on, _, pitch)| (on, on + 0.36, pitch))
                .collect();
            note_list(&list)
        };
        let read = drop_layer(&hits.render(f64::from(RATE)), &notes(&hits));
        assert_eq!(read.drops.len(), 2, "{decay} {release}: {:?}", read.drops);
        for (drop, (index, ratio, tau)) in read.drops.iter().zip(DROPS) {
            let f = 440.0 * 2f64.powf((LINE[index] - 69.0) / 12.0);
            let fit = drop.value;
            assert!((fit.f_end.get() - f).abs() <= 1.5, "{decay}: {fit:?}");
            assert!(
                (fit.df_hz / (ratio * f) - 1.0).abs() <= 0.25,
                "{decay}: {fit:?}"
            );
            assert!(
                (fit.tau.get() / tau - 1.0).abs() <= 0.25,
                "{decay}: {fit:?}"
            );
        }
        // The same line with no drop, and a retriggered same-pitch line,
        // read none: the restart swing fits a tau under 10 ms and needs a
        // 6 dB strike.
        for pitches in [&LINE[..], &[28.0; 5][..]] {
            let plain = Hits {
                pitches,
                drops: &[],
                ..hits
            };
            let read = drop_layer(&plain.render(f64::from(RATE)), &notes(&plain));
            assert_eq!(read.drops, [], "{decay} {release} {pitches:?}");
        }
    }
}

#[test]
fn a_fast_or_slowly_attacked_drop_reads_below_high_confidence() {
    // tau 5 ms: the fit sees a quarter of the fall from 7 ms on.
    let fast = PitchDrop::row(45.0, 100.0, 0.005);
    let read = drop_layer(
        &fast.render(f64::from(RATE)),
        &note_list(&[(0.25, 1.2, 30.0)]),
    );
    assert_eq!(read.drops.len(), 1, "{:?}", read.drops);
    assert_eq!(read.drops[0].confidence.level(), Some(Level::Medium));
    // The pitch falls during a slow attack, before the onset, and df reads
    // low: medium for a 20 ms ramp, low past a 30 ms attack.
    for (attack_s, level) in [(0.02, Level::Medium), (0.04, Level::Low)] {
        let slow = PitchDrop {
            attack_s,
            ..PitchDrop::row(41.2, 41.2, 0.03)
        };
        let read = drop_layer(&slow.render(f64::from(RATE)), &drop_notes(0));
        assert_eq!(read.drops.len(), 1, "{:?}", read.drops);
        assert_eq!(read.drops[0].confidence.level(), Some(level), "{attack_s}");
    }
    // The rows of the drop table, with no attack ramp, read high.
    let sharp = PitchDrop::row(41.2, 41.2, 0.03);
    let read = drop_layer(&sharp.render(f64::from(RATE)), &drop_notes(0));
    assert_eq!(read.drops[0].confidence.level(), Some(Level::High));
}

#[test]
fn steps_between_808_notes_report_no_glide_or_drop() {
    // The track smears each step over 100 to 450 ms; the fit finds the
    // motion short or on a bound of the curve power.
    for name in [
        "steps-808-legato",
        "steps-808-driven-legato",
        "steps-808-staccato",
    ] {
        let read = layer(name, case(name));
        assert_eq!(read.glides, [], "{name}");
        assert_eq!(read.drops, [], "{name}");
        assert_eq!(read.counts.drop_candidates, 6, "{name}");
    }
}

#[test]
fn a_semitone_step_on_one_808_oscillator_is_no_glide_but_a_semitone_slide_is() {
    // One sine keeps its phase through every step, and each note strikes
    // its envelope again. The track smears the step from 43 to 44 into a
    // fast curve that fits the upper bound of p within 5 cents, and its
    // slope stays up 6.2 times as long as the fitted motion.
    let name = "steps-808-continuous-semitone";
    let read = layer(name, case(name));
    assert_eq!(read.glides, []);
    assert_eq!(read.drops, []);
    // An RC slide of 1.2 semitones fits the same bound, its slope up 2.27
    // times as long as the fitted motion.
    let name = "rc-glide-110hz-1.2st-tau30ms";
    let read = layer(name, case(name));
    assert_eq!(read.glides.len(), 1, "{:?}", read.glides);
    let glide = &read.glides[0];
    assert!(glide.value.on_bound, "{glide:?}");
    assert!((glide.value.interval.get() - 1.2).abs() < 0.15, "{glide:?}");
    assert_eq!(glide.confidence.level(), Some(Level::Low));
}

#[test]
fn retriggered_808_hits_report_no_drop() {
    // Each hit restarts the sine's phase, and the line swings at the join.
    // With tau down to 0.5 ms and no cap, the fit read that swing as falls
    // of 141 and 152 semitones with a tau of 0.7 ms.
    for name in [
        "hits-29x5-gap0ms",
        "hits-29x5-gap50ms",
        "hits-29-36-43-legato",
    ] {
        let read = layer(name, case(name));
        assert!(read.counts.drop_candidates >= 1, "{name}");
        assert_eq!(read.drops, [], "{name}");
        assert_eq!(read.glides, [], "{name}");
    }
}

#[test]
fn a_fall_into_the_band_is_a_glide_and_no_drop() {
    // A saw falling from 220 Hz enters the 200 Hz band of the line, whose
    // envelope rises with no strike.
    let name = "glide-mono-220hz-down-4st-200ms-p2.5";
    let read = layer(name, case(name));
    assert_eq!(read.drops, []);
    assert_eq!(read.glides.len(), 1, "{:?}", read.glides);
    let glide = &read.glides[0].value;
    assert!((glide.interval.get() + 4.0).abs() <= 0.15, "{glide:?}");
    let error = rise(glide.time.get(), glide.curve) / rise(0.2, 2.5) - 1.0;
    assert!(error.abs() <= 0.05, "{glide:?}, rise off by {error:.3}");
    let short = layer(
        "glide-mono-220hz-down-4st-50ms-p1",
        case("glide-mono-220hz-down-4st-50ms-p1"),
    );
    assert_eq!(short.drops, []);
}

#[test]
fn a_track_off_the_notes_reads_the_layer_at_low_confidence() {
    // DC: the transcriber gives two notes at MIDI 22, and no frame inside
    // them is voiced.
    let dc = layer("dc", case("dc"));
    assert_eq!(dc.confidence.level(), Some(Level::Low));
    let name = glide_name("mono", 0.15, 1.0);
    assert_eq!(
        layer(&name, case(&name)).confidence.level(),
        Some(Level::Medium)
    );
}

#[test]
fn a_trill_reads_at_low_confidence_and_a_wide_slow_vibrato_as_glides() {
    let trill = layer("trill-semitone-6hz", case("trill-semitone-6hz"));
    for v in &trill.vibrato {
        assert_eq!(v.confidence.level(), Some(Level::Low), "{v:?}");
    }
    // A known limit: ±100 cents at 3 Hz holds each extreme long enough to
    // read as a stable pitch, so the swings read as glides of 1.9
    // semitones, each at low confidence.
    let name = vibrato_name("mono", 3.0, 100.0);
    let wide = layer(&name, case(&name));
    assert!(wide.vibrato.is_empty(), "{:?}", wide.vibrato);
    for g in &wide.glides {
        assert_eq!(g.confidence.level(), Some(Level::Low), "{g:?}");
    }
}

#[test]
fn vibrato_reads_its_rate_within_0_1_hz_and_its_extent_within_15_percent() {
    for (name, rate, extent) in [
        (vibrato_name("mono", 5.5, 35.0), 5.5, 35.0),
        (vibrato_name("unison", 5.5, 35.0), 5.5, 35.0),
        (vibrato_name("mono", 5.5, 60.0), 5.5, 60.0),
    ] {
        let read = layer(&name, case(&name));
        assert_eq!(read.vibrato.len(), 1, "{name}: {:?}", read.vibrato);
        let vibrato = &read.vibrato[0].value;
        assert!(
            (vibrato.rate.get() - rate).abs() <= 0.1,
            "{name}: {vibrato:?}"
        );
        let error = vibrato.extent().get() / extent - 1.0;
        assert!(
            error.abs() <= 0.15,
            "{name}: {vibrato:?}, extent off by {error:.3}"
        );
        assert!(read.glides.is_empty(), "{name}: {:?}", read.glides);
    }
}

#[test]
fn slow_fast_and_shallow_motion_reports_no_vibrato() {
    for (rate, extent) in [(2.0, 35.0), (15.0, 35.0), (5.5, 5.0)] {
        let name = vibrato_name("mono", rate, extent);
        let read = layer(&name, case(&name));
        assert!(read.vibrato.is_empty(), "{name}: {:?}", read.vibrato);
        assert!(read.glides.is_empty(), "{name}: {:?}", read.glides);
    }
}

#[test]
fn the_extent_share_table_is_what_the_track_reads() {
    let mut measured = Vec::new();
    for rate in VIBRATO_RATES {
        let name = vibrato_name("mono", rate, 35.0);
        let read = layer(&name, case(&name));
        assert_eq!(read.vibrato.len(), 1, "{name}: {:?}", read.vibrato);
        let raw = read.vibrato[0].value.extent_raw.get();
        measured.push((rate, raw / 35.0));
    }
    assert_eq!(EXTENT_SHARE.len(), measured.len(), "{measured:?}");
    for ((rate, share), (at, table)) in measured.iter().zip(EXTENT_SHARE) {
        assert!(
            (rate - at).abs() < 1e-12 && (share - table).abs() < 0.005,
            "{measured:?}"
        );
    }
}

/// The notes the transcriber gave for each drop row: it places the onset
/// of a falling note late, and once splits the start into its own note.
fn drop_notes(row: usize) -> NoteList {
    let list: &[(f64, f64, f64)] = match row {
        0 => &[(0.418, 1.091, 28.0)],
        1 => &[(0.476, 1.068, 28.0)],
        2 => &[(0.453, 1.091, 33.0)],
        _ => &[(0.464, 0.546, 30.0), (0.499, 1.091, 28.0)],
    };
    note_list(list)
}

fn note_list(list: &[(f64, f64, f64)]) -> NoteList {
    NoteList::new(
        list.iter()
            .map(|&(on, off, pitch)| {
                Note::new(s(on), s(off), MidiPitch::new(pitch).unwrap()).unwrap()
            })
            .collect(),
    )
}

fn drop_layer(mono: &[f64], notes: &NoteList) -> PitchLayer {
    PitchLayer::of(&PitchInput {
        mono,
        rate: rate(),
        track: &PitchTrack::default(),
        notes,
        voicing: Voicing::Monophonic,
    })
    .unwrap()
}

/// A sine at MIDI `pitch` that starts from 0.2 s of silence at `phase`
/// radians, with a second harmonic of `h2` at `h2_phase`, a linear attack
/// of `attack_s` and a 0.8 s decay, 1.2 s long.
fn first_note(pitch: f64, attack_s: f64, phase: f64, h2: f64, h2_phase: f64) -> Vec<f64> {
    let rate = f64::from(RATE);
    let f = 440.0 * 2f64.powf((pitch - 69.0) / 12.0);
    (0..round_to_usize(1.2 * rate))
        .map(|n| {
            let t = f64::from(u32::try_from(n).unwrap()) / rate - 0.2;
            if t < 0.0 {
                return 0.0;
            }
            let x = (std::f64::consts::TAU * f * t + phase).sin()
                + h2 * (2.0 * std::f64::consts::TAU * f * t + h2_phase).sin();
            0.5 * (t / attack_s).min(1.0) * x * (-t / 0.8).exp()
        })
        .collect()
}

#[test]
fn a_driven_808_drop_reads_through_its_harmonics() {
    // tanh(3 x) adds odd harmonics; the third lies inside the band and
    // swings the line at twice the note. The raw residual was 23 % of the
    // fall, over the 15 % limit.
    let drop = PitchDrop {
        decay_s: 1.0,
        ..PitchDrop::row(43.65, 48.85, 0.065)
    };
    let driven: Vec<f64> = drop
        .render(f64::from(RATE))
        .iter()
        .map(|x| (3.0 * x).tanh())
        .collect();
    let read = drop_layer(&driven, &note_list(&[(0.3, 1.2, 29.0)]));
    assert_eq!(read.drops.len(), 1, "{:?}", read.counts);
    let fit = &read.drops[0].value;
    assert!((fit.df().unwrap().get() - 13.0).abs() < 0.5, "{fit:?}");
    assert!((fit.tau.get() / 0.065 - 1.0).abs() < 0.05, "{fit:?}");
}

#[test]
fn a_drop_far_below_the_loudest_passage_is_not_read() {
    // A drop 60 dB below a loud passage, 1 s after it, sits in the noise
    // floor of a real stem, under the 40 dB gate of active frames; 30 dB
    // below it counts.
    let rate = f64::from(RATE);
    let notes = note_list(&[(0.2, 0.9, 64.0), (2.418, 3.091, 28.0)]);
    for (down_db, drops) in [(60.0, 0), (30.0, 1)] {
        let mut mono: Vec<f64> = (0..round_to_usize(rate))
            .map(|n| {
                0.8 * (std::f64::consts::TAU * 330.0 * f64::from(u32::try_from(n).unwrap()) / rate)
                    .sin()
            })
            .collect();
        mono.extend(std::iter::repeat_n(0.0, round_to_usize(rate)));
        let gain = 10_f64.powf(-down_db / 20.0);
        mono.extend(
            PitchDrop::row(41.2, 41.2, 0.03)
                .render(rate)
                .iter()
                .map(|x| gain * x),
        );
        let read = drop_layer(&mono, &notes);
        assert_eq!(read.drops.len(), drops, "{down_db} dB");
    }
}

#[test]
fn a_low_first_note_from_silence_reads_no_drop() {
    // The 20 Hz high-pass raises the line for about 10 ms after a tone
    // below 40 Hz starts from silence: on the line alone, these read falls
    // of 10 to 14 semitones with a tau of 3 to 5 ms.
    for (pitch, attack, phase, h2, h2_phase) in [
        (24.0, 0.001, 2.705, 0.0, 0.0),
        (24.0, 0.006, 5.224, 0.0, 0.0),
        (26.0, 0.006, 4.337, 0.0, 0.0),
        (26.0, 0.006, 4.337, 0.15, 1.0),
        (26.0, 0.001, 1.965, 0.3, 4.0),
    ] {
        let notes = note_list(&[(0.2, 1.1, pitch)]);
        let read = drop_layer(&first_note(pitch, attack, phase, h2, h2_phase), &notes);
        assert_eq!(read.drops, [], "{pitch} {attack} {phase} {h2}");
    }
}

/// The pitch layer of a synthetic track that follows `cents(t)` from 0.2
/// to 1.4 s at the tracker's hop, over silence, with `notes`.
fn track_layer(cents: impl Fn(f64) -> f64, notes: &[(f64, f64, f64)]) -> PitchLayer {
    let mono = vec![0.0; round_to_usize(1.6 * f64::from(RATE))];
    audio_track_layer(&mono, |t| Some(cents(t)), notes)
}

/// The pitch layer of a synthetic track as [`track_layer`] reads it, voiced
/// where `cents(t)` gives a pitch, over `mono`.
fn audio_track_layer(
    mono: &[f64],
    cents: impl Fn(f64) -> Option<f64>,
    notes: &[(f64, f64, f64)],
) -> PitchLayer {
    let hop = 128.0 / f64::from(RATE);
    let frames = (0..round_to_usize(1.6 / hop))
        .map(|k| {
            let t = f64::from(u32::try_from(k).unwrap()) * hop;
            TrackFrame {
                time: s(t),
                f0: (0.2..1.4)
                    .contains(&t)
                    .then(|| cents(t))
                    .flatten()
                    .map(|c| Hz::new(440.0 * 2f64.powf((c - 6900.0) / 1200.0)).unwrap()),
                level: Decibels::new(0.0).unwrap(),
            }
        })
        .collect();
    PitchLayer::of(&PitchInput {
        mono,
        rate: rate(),
        track: &PitchTrack::from_frames(frames),
        notes: &note_list(notes),
        voicing: Voicing::Monophonic,
    })
    .unwrap()
}

#[test]
fn a_vibrato_below_100_hz_reads_low_and_a_step_reads_none() {
    let vibrato = |t: f64| 35.0 * (std::f64::consts::TAU * 5.5 * t).sin();
    // 5.5 Hz over about 1 s holds 5 cycles: high at A3, low at A1.
    for (pitch, level) in [(57.0, Level::High), (33.0, Level::Low)] {
        let read = track_layer(|t| 100.0 * pitch + vibrato(t), &[(0.2, 1.4, pitch)]);
        assert_eq!(read.vibrato.len(), 1, "{pitch}: {:?}", read.vibrato);
        assert_eq!(read.vibrato[0].confidence.level(), Some(level), "{pitch}");
    }
    // A semitone step at 0.8 s stays inside one steady span, and spoils
    // the sine's share.
    let step = |t: f64| 5700.0 + if t < 0.8 { 0.0 } else { 100.0 } + vibrato(t);
    let read = track_layer(step, &[(0.2, 0.8, 57.0), (0.8, 1.4, 58.0)]);
    assert_eq!(read.vibrato, [], "{:?}", read.vibrato);
}

#[test]
fn drops_read_f_end_df_and_tau_within_their_tolerances() {
    for (row, &(f_end, df, tau)) in PitchDrop::ROWS.iter().enumerate() {
        let truth = PitchDrop::row(f_end, df, tau);
        let read = drop_layer(&truth.render(f64::from(RATE)), &drop_notes(row));
        assert_eq!(read.drops.len(), 1, "row {row}: {read:?}");
        let fit = read.drops[0].value;
        assert!((fit.f_end.get() - f_end).abs() <= 1.5, "row {row}: {fit:?}");
        assert!((fit.df_hz / df - 1.0).abs() <= 0.25, "row {row}: {fit:?}");
        let bound = if tau <= 0.05 { 0.25 } else { 0.4 };
        assert!(
            (fit.tau.get() / tau - 1.0).abs() <= bound,
            "row {row}: {fit:?}"
        );
        assert!(
            (fit.onset.get() - truth.lead_s).abs() <= 0.002,
            "row {row}: {fit:?}"
        );
        assert!(
            read.glides.is_empty() && read.vibrato.is_empty(),
            "row {row}"
        );
    }
}

#[test]
fn the_fit_starts_after_the_onset_overshoot() {
    // A plain 41.2 Hz tone switched on from silence: the line overshoots
    // by 9.4 Hz at 3 ms, so a fit from there reads a drop, and by 0.9 Hz
    // at 7 ms.
    assert!((DROP_FIT_START.get() - 0.007).abs() < 1e-12);
    for df in [0.0, 4.0] {
        let truth = PitchDrop::row(41.2, df, 0.02);
        let line = SubBandLine::of(&truth.render(f64::from(RATE)), rate(), s(0.0)).unwrap();
        let onset = band_onset(&line, s(0.0), s(0.5)).unwrap();
        let at = |ms: f64| line.hz()[line.index_at(s(onset.get() + ms / 1000.0))];
        if df == 0.0 {
            assert!(at(3.0) - 41.2 > 5.0, "{}", at(3.0));
            assert!((at(7.0) - 41.2).abs() < 1.5, "{}", at(7.0));
        }
        let fit = fit_drop(&line, onset, s(1.0)).unwrap();
        assert!(!fit.accepted(), "{df}: {fit:?}");
    }
}

#[test]
fn noise_under_low_notes_flags_no_weak_fundamental() {
    // The band at twice the pitch is twice as wide, so white noise reads
    // it 1.41 times stronger, under the factor of 2.
    let noise: Vec<f64> = gaussian(3, round_to_usize(1.25 * f64::from(RATE)))
        .into_iter()
        .enumerate()
        .map(|(n, x)| {
            if n < round_to_usize(0.25 * f64::from(RATE)) {
                0.0
            } else {
                0.3 * x
            }
        })
        .collect();
    let read = drop_layer(&noise, &note_list(&[(0.25, 1.2, 28.0)]));
    assert_eq!(read.counts.drop_candidates, 1);
    assert!(!read.flags.weak_fundamental);
    assert_eq!(read.drops, []);
    assert_eq!(read.confidence.level(), Some(Level::Low));
}

#[test]
fn the_sub_band_line_settles_on_f_end() {
    let truth = PitchDrop::row(41.2, 41.2, 0.02);
    let mono = truth.render(f64::from(RATE));
    let line = SubBandLine::of(&mono, rate(), s(0.0)).unwrap();
    let settled: Vec<f64> = (line.index_at(s(truth.lead_s + 5.0 * truth.tau_s))
        ..line.index_at(s(truth.lead_s + 0.4)))
        .map(|n| line.hz()[n])
        .collect();
    let mean = settled.iter().sum::<f64>() / f64::from(u32::try_from(settled.len()).unwrap());
    assert!((mean - 41.2).abs() <= 1.5, "{mean}");
    let worst = settled.iter().fold(0.0_f64, |m, f| m.max((f - 41.2).abs()));
    assert!(worst <= 1.5, "{worst}");
}

#[test]
fn a_driven_or_noisy_drop_keeps_f_end_within_1_hz() {
    let truth = PitchDrop::row(41.2, 41.2, 0.02);
    let dry = truth.render(f64::from(RATE));
    let driven: Vec<f64> = dry.iter().map(|x| (4.0 * x).tanh()).collect();
    // White noise 30 dB below the tone's peak.
    let peak = dry.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
    let noisy: Vec<f64> = dry
        .iter()
        .zip(gaussian(5, dry.len()))
        .map(|(x, n)| x + peak * 10f64.powf(-30.0 / 20.0) * n)
        .collect();
    for (name, mono) in [("tanh", driven), ("noise", noisy)] {
        let line = SubBandLine::of(&mono, rate(), s(0.0)).unwrap();
        let onset = band_onset(&line, s(0.0), s(0.5)).unwrap();
        let fit = fit_drop(&line, onset, s(1.0)).unwrap();
        assert!((fit.f_end.get() - 41.2).abs() <= 1.0, "{name}: {fit:?}");
        // Drive puts the third harmonic, 124 Hz, inside the band, and the
        // line swings between the two partials; averaged over a period of
        // the note, the swing leaves the residual.
        assert!(fit.accepted(), "{name}: {fit:?}");
    }
}

#[test]
fn a_small_or_irregular_fall_reports_no_drop() {
    // 41.2 Hz plus 4 Hz is 1.6 semitones, under the 2-semitone floor.
    let small = PitchDrop::row(41.2, 4.0, 0.02).render(f64::from(RATE));
    let notes = drop_notes(0);
    let read = drop_layer(&small, &notes);
    assert!(read.drops.is_empty(), "{:?}", read.drops);
    assert_eq!(read.counts.drop_candidates, 1);
    // A fall whose frequency also swings by half the drop at 9 Hz leaves a
    // residual above 15 % of the drop.
    let swing = wobbly_drop();
    let line = SubBandLine::of(&swing, rate(), s(0.0)).unwrap();
    let onset = band_onset(&line, s(0.0), s(0.5)).unwrap();
    let fit = fit_drop(&line, onset, s(1.0)).unwrap();
    assert!(fit.residual > 0.15 && !fit.accepted(), "{fit:?}");
    assert_eq!(drop_layer(&swing, &notes).drops, []);
}

fn wobbly_drop() -> Vec<f64> {
    let rate = f64::from(RATE);
    let mut phase = 0.0;
    (0..round_to_usize(1.25 * rate))
        .map(|n| {
            let t = f64::from(u32::try_from(n).unwrap()) / rate - 0.25;
            if t < 0.0 {
                return 0.0;
            }
            let f =
                41.2 + 20.0 * (-t / 0.05).exp() + 10.0 * (std::f64::consts::TAU * 9.0 * t).sin();
            phase += f / rate;
            0.8 * (std::f64::consts::TAU * phase).sin() * (-t / 0.5).exp()
        })
        .collect()
}

#[test]
fn a_fundamental_weaker_than_its_second_harmonic_skips_the_drop() {
    let rate = f64::from(RATE);
    let truth = PitchDrop::row(41.2, 41.2, 0.02);
    // The same drop on the fundamental, plus its second harmonic at three
    // times the amplitude.
    let mono: Vec<f64> = (0..round_to_usize(truth.length_s * rate))
        .map(|n| {
            let t = f64::from(u32::try_from(n).unwrap()) / rate - truth.lead_s;
            if t < 0.0 {
                return 0.0;
            }
            let phase =
                truth.f_end_hz * t + truth.df_hz * truth.tau_s * (1.0 - (-t / truth.tau_s).exp());
            let tau = std::f64::consts::TAU;
            0.2 * ((tau * phase).sin() + 3.0 * (2.0 * tau * phase).sin())
                * (-t / truth.decay_s).exp()
        })
        .collect();
    let read = drop_layer(&mono, &drop_notes(0));
    assert!(read.drops.is_empty(), "{:?}", read.drops);
    assert!(read.flags.weak_fundamental);
}

#[test]
fn chords_and_a_stem_without_notes_report_no_values() {
    let mono = vec![0.0; 22_050];
    let notes = drop_notes(0);
    let chords = PitchLayer::of(&PitchInput {
        mono: &mono,
        rate: rate(),
        track: &PitchTrack::default(),
        notes: &notes,
        voicing: Voicing::Polyphonic,
    })
    .unwrap();
    assert!(chords.flags.chord_skipped && !chords.confidence.is_estimate());
    assert_eq!(
        chords.summary(),
        ["pitch: the line plays chords, so no glides, drops or vibrato"]
    );
    let empty = drop_layer(&mono, &NoteList::default());
    assert!(empty.flags.no_notes && !empty.confidence.is_estimate());
}

#[test]
fn the_sub_band_line_and_the_drop_fit_match_scipy() {
    let golden = Golden::load("sub-band-line");
    let recipe = golden.recipe();
    let rows = recipe["rows"].as_array().unwrap();
    let drop_of = |row: &Value| {
        let row = floats(row);
        PitchDrop {
            decay_s: float(&recipe["decay_s"]),
            lead_s: float(&recipe["lead_s"]),
            length_s: float(&recipe["length_s"]),
            ..PitchDrop::row(row[0], row[1], row[2])
        }
    };
    let line = SubBandLine::of(&drop_of(&rows[0]).render(f64::from(RATE)), rate(), s(0.0)).unwrap();
    let indices: Vec<usize> = golden.values()["line_indices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| usize::try_from(n.as_u64().unwrap()).unwrap())
        .collect();
    let ours: Vec<f64> = indices.iter().map(|&n| line.hz()[n]).collect();
    let line_tolerance = Tolerance {
        abs: golden.tolerance().abs,
        rel: 0.0,
    };
    assert_close(&ours, &floats(&golden.values()["line_hz"]), line_tolerance);
    let fit_tolerance = Tolerance {
        abs: 0.0,
        rel: golden.tolerance().rel,
    };
    for (row, expected) in rows
        .iter()
        .zip(golden.values()["drops"].as_array().unwrap())
    {
        let line = SubBandLine::of(&drop_of(row).render(f64::from(RATE)), rate(), s(0.0)).unwrap();
        let onset = band_onset(&line, s(0.0), s(0.5)).unwrap();
        let fit = fit_drop(&line, onset, s(1.0)).unwrap();
        assert_eq!(
            fit.points,
            usize::try_from(expected["points"].as_u64().unwrap()).unwrap()
        );
        assert!(
            (onset.get() - float(&expected["onset_s"])).abs() < 1e-9,
            "{row}: {onset:?}"
        );
        assert_close(
            &[fit.f_end.get(), fit.df_hz, fit.tau.get()],
            &[
                float(&expected["f_end_hz"]),
                float(&expected["df_hz"]),
                float(&expected["tau_s"]),
            ],
            fit_tolerance,
        );
    }
}

#[test]
fn a_saw_struck_from_silence_reports_no_drop() {
    // Several partials share the band at a saw's onset, and the line swings
    // into a fall that settles off the note.
    let rate = f64::from(RATE);
    let held = Glide {
        start_hz: 55.0,
        semitones: 0.0,
        full_s: 0.1,
        p: 1.0,
        hold_s: 0.5,
        voices: 1,
        detune_ct: 0.0,
        in_phase: false,
    };
    let mut mono = vec![0.0; round_to_usize(0.25 * rate)];
    mono.extend(held.render(rate));
    let notes = NoteList::new(vec![
        Note::new(s(0.25), s(1.35), MidiPitch::new(33.0).unwrap()).unwrap(),
    ]);
    let read = drop_layer(&mono, &notes);
    assert_eq!(read.counts.drop_candidates, 1);
    assert_eq!(read.drops, []);
}

#[test]
fn a_quiet_fall_under_a_louder_line_in_the_band_is_not_read() {
    // A line at 87.3 Hz, inside the band, plays for 1 s; after 1 s of
    // silence a drop follows 24 dB under it, as a fall under a louder line
    // in the band reads on a real stem, or 10 dB under it. The full-band
    // gate of 40 dB passes both.
    let rate = f64::from(RATE);
    let notes = note_list(&[(0.2, 0.9, 41.0), (2.418, 3.091, 28.0)]);
    for (down_db, drops) in [(24.0, 0), (10.0, 1)] {
        let mut mono: Vec<f64> = (0..round_to_usize(rate))
            .map(|n| {
                0.8 * (std::f64::consts::TAU * 87.3 * f64::from(u32::try_from(n).unwrap()) / rate)
                    .sin()
            })
            .collect();
        mono.extend(std::iter::repeat_n(0.0, round_to_usize(rate)));
        let gain = 10_f64.powf(-down_db / 20.0);
        mono.extend(
            PitchDrop::row(41.2, 41.2, 0.03)
                .render(rate)
                .iter()
                .map(|x| gain * x),
        );
        let read = drop_layer(&mono, &notes);
        assert_eq!(read.drops.len(), drops, "{down_db} dB: {:?}", read.counts);
        assert_eq!(read.counts.drop_skips.quiet, 1 - drops, "{down_db} dB");
    }
}

#[test]
fn a_fast_fall_inside_a_slow_attack_is_not_read() {
    // A fall with tau 5 ms ends inside a 60 ms attack, where the line
    // wanders on the rising level: a fall in a real stem's noise floor
    // read so, with an attack of 208 ms. The same fall struck sharply
    // reads.
    for (attack_s, drops) in [(0.06, 0), (0.0, 1)] {
        let drop = PitchDrop {
            attack_s,
            ..PitchDrop::row(45.0, 100.0, 0.005)
        };
        let read = drop_layer(
            &drop.render(f64::from(RATE)),
            &note_list(&[(0.25, 1.2, 30.0)]),
        );
        assert_eq!(read.drops.len(), drops, "{attack_s}: {:?}", read.counts);
        assert_eq!(read.counts.drop_skips.no_strike, 1 - drops, "{attack_s}");
    }
}

/// A sweep from 0.2 s of silence, `f_end + df exp(-t / tau)`, with a
/// second harmonic of `h2`, a 3 ms attack and a 1 s decay, `length_s`
/// long and followed by 0.3 s of silence.
fn sweep(
    f_end: f64,
    df: f64,
    tau: f64,
    harmonics: &[f64],
    attack_s: f64,
    length_s: f64,
) -> Vec<f64> {
    let rate = f64::from(RATE);
    (0..round_to_usize((0.5 + length_s) * rate))
        .map(|n| {
            let t = f64::from(u32::try_from(n).unwrap()) / rate - 0.2;
            if !(0.0..length_s).contains(&t) {
                return 0.0;
            }
            let phase = std::f64::consts::TAU * (f_end * t + df * tau * (1.0 - (-t / tau).exp()));
            let overtones: f64 = (2_u32..)
                .zip(harmonics)
                .map(|(k, a)| a * (f64::from(k) * phase + 1.0).sin())
                .sum();
            0.5 * (t / attack_s).min(1.0) * (phase.sin() + overtones) * (-t).exp()
        })
        .collect()
}

#[test]
fn a_slow_sweep_from_silence_into_a_short_low_note_reads_as_a_drop() {
    // From 80.7 Hz into C1, tau 90 ms, under a 350 ms note: five time
    // constants end after the note, and the fall crosses the band around
    // twice the note. A second harmonic at half the fundamental swings the
    // line once per period. A real sweep of this shape read as a glide
    // from silence.
    for h2 in [0.0, 0.5] {
        let read = drop_layer(
            &sweep(32.7, 48.0, 0.09, &[h2], 0.003, 0.6),
            &note_list(&[(0.2, 0.55, 24.0)]),
        );
        assert_eq!(read.drops.len(), 1, "{h2}: {:?}", read.counts);
        let fit = &read.drops[0].value;
        assert!((fit.f_end.get() - 32.7).abs() < 1.0, "{h2}: {fit:?}");
        assert!((fit.tau.get() / 0.09 - 1.0).abs() < 0.25, "{h2}: {fit:?}");
    }
}

#[test]
fn a_step_and_settle_across_a_gap_is_no_glide() {
    // A4 to F#4 with the track unvoiced from 0.8 to 0.83 s. A step lands
    // 500 cents down across the gap and settles 200 more after it: the
    // pitch keeps moving the way it jumped, yet the jump carries most of
    // the interval. A glide jumps 100 cents across the same gap.
    let notes = [(0.2, 0.8, 69.0), (0.8, 1.4, 62.0)];
    let gap = |t: f64| (0.8..0.83).contains(&t);
    let step = |t: f64| {
        (!gap(t)).then(|| {
            if t < 0.8 {
                6900.0
            } else {
                6200.0 + 200.0 * (-(t - 0.83) / 0.02).exp()
            }
        })
    };
    let glide = |t: f64| {
        (!gap(t)).then(|| {
            let x = ((t - 0.7) / 0.3).clamp(0.0, 1.0);
            let jump = if t >= 0.83 { 100.0 } else { 0.0 };
            6900.0 - jump - 600.0 * x.min(1.0)
        })
    };
    let silent = vec![0.0; round_to_usize(1.6 * f64::from(RATE))];
    let read = audio_track_layer(&silent, step, &notes);
    assert_eq!(read.glides, [], "{:?}", read.glides);
    let read = audio_track_layer(&silent, glide, &notes);
    assert_eq!(read.glides.len(), 1, "{:?}", read.glides);
    // A dip of 30 dB in the audio under the gap breaks the sound, and the
    // gap is not bridged.
    let rate = f64::from(RATE);
    let lag = 0.0232 - 0.5 / 400.0;
    let mono: Vec<f64> = (0..round_to_usize(1.6 * rate))
        .map(|n| {
            let t = f64::from(u32::try_from(n).unwrap()) / rate;
            let dip = if (0.805 - lag..0.825 - lag).contains(&t) {
                0.03
            } else {
                1.0
            };
            0.5 * dip * (std::f64::consts::TAU * 400.0 * t).sin()
        })
        .collect();
    let read = audio_track_layer(&mono, glide, &notes);
    assert!(
        read.glides.iter().all(|g| g.value.start.get() > 0.8),
        "{:?}",
        read.glides
    );
}

#[test]
fn a_low_sweep_with_strong_overtones_reads_on_the_note_line() {
    // From 68.7 Hz into C1 with a slow attack, and the second to fourth
    // harmonics inside the band. At 8, 6 and 3 dB under the fundamental
    // the 20 to 200 Hz line follows them: its fit settles near 62 Hz, a
    // fall of the wrong sign, and misses on its size. At 14, 12 and 7 dB
    // under it the fit there settles on the note and misses on a residual
    // of 0.16. The note line reads each fall.
    for (tau, harmonics) in [
        (0.06, [0.4, 0.5, 0.7]),
        (0.09, [0.4, 0.5, 0.7]),
        (0.06, [0.2, 0.25, 0.45]),
    ] {
        let read = drop_layer(
            &sweep(32.7, 36.0, tau, &harmonics, 0.08, 0.6),
            &note_list(&[(0.25, 0.75, 24.0)]),
        );
        assert_eq!(
            read.drops.len(),
            1,
            "{tau} {harmonics:?}: {:?}",
            read.counts
        );
        let fit = &read.drops[0].value;
        assert!((fit.f_end.get() - 32.7).abs() < 1.0, "{tau}: {fit:?}");
        assert!((fit.tau.get() / tau - 1.0).abs() < 0.2, "{tau}: {fit:?}");
    }
}

#[test]
fn sweeps_into_82_and_104_hz_with_a_strong_second_harmonic_read_on_the_note_line() {
    // Octave falls with a tau of 60 ms into 82.4 Hz, with a second harmonic
    // as strong as the fundamental, and into 103.8 Hz, with one 5.6 dB
    // stronger, under the 6 dB of a weak fundamental. The 20 to 200 Hz
    // line, pulled up by the harmonic, settles 120 and 130 cents sharp, off
    // the note; the note line, low-passed at 124 and 156 Hz, reads the
    // fall.
    for (pitch, h2) in [(40.0, 1.0), (44.0, 1.9)] {
        let f_end = 440.0 * 2f64.powf((pitch - 69.0) / 12.0);
        let read = drop_layer(
            &sweep(f_end, f_end, 0.06, &[h2, 0.2], 0.005, 0.6),
            &note_list(&[(0.25, 0.75, pitch)]),
        );
        assert_eq!(read.drops.len(), 1, "{pitch}: {:?}", read.counts);
        let fit = &read.drops[0].value;
        assert!(
            (fit.f_end.get() / f_end - 1.0).abs() < 0.01,
            "{pitch}: {fit:?}"
        );
        assert!((fit.tau.get() / 0.06 - 1.0).abs() < 0.1, "{pitch}: {fit:?}");
    }
}

/// Sines at MIDI `pitch` struck every `period` seconds from 0.2 s, each
/// `(velocity, phase, h2, h2_phase)` with a second harmonic of `h2`, a
/// 6 ms attack and a 0.6 s decay, ringing 1.8 s and summed.
fn retriggers(pitch: f64, period: f64, hits: &[(f64, f64, f64, f64)]) -> Vec<f64> {
    let rate = f64::from(RATE);
    let f = 440.0 * 2f64.powf((pitch - 69.0) / 12.0);
    let length = 0.5 + period * cast_len(hits.len()) + 1.8;
    (0..round_to_usize(length * rate))
        .map(|n| {
            let time = f64::from(u32::try_from(n).unwrap()) / rate - 0.2;
            (0_u32..)
                .zip(hits)
                .map(|(k, &(velocity, phase, h2, h2_phase))| {
                    let t = time - period * f64::from(k);
                    if !(0.0..1.8).contains(&t) {
                        return 0.0;
                    }
                    let ph = std::f64::consts::TAU * f * t + phase;
                    velocity
                        * (t / 0.006).min(1.0)
                        * (-t / 0.6).exp()
                        * (ph.sin() + h2 * (2.0 * ph + h2_phase).sin())
                })
                .sum::<f64>()
                * 0.25
        })
        .collect()
}

fn cast_len(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap())
}

#[test]
fn a_same_pitch_retrigger_that_barely_rises_reads_no_drop_on_the_note_line() {
    // The third hit lands while the second rings at twice its level, so
    // the full band rises 0.6 dB, and the fourth 1.6 dB. The phase restart
    // swings the 20 to 200 Hz line, and the fit there misses on its
    // residual; on the note line the swing smooths into a fall that
    // settles on the note. Without the 6 dB strike the note line read it
    // as a drop.
    let hits = [
        (0.5032, 4.1357, 0.3, 4.1103),
        (0.9138, 2.3518, 0.3, 1.8045),
        (0.5059, 0.513, 0.15, 0.6091),
        (0.368, 3.9966, 0.0, 0.0),
        (0.7987, 2.1059, 0.3, 4.3989),
        (0.5931, 3.5907, 0.15, 0.8879),
    ];
    let notes: Vec<(f64, f64, f64)> = (0..6)
        .map(|k| {
            let on = 0.2 + 0.5 * f64::from(k);
            (on, on + 0.475, 27.0)
        })
        .collect();
    let read = drop_layer(&retriggers(27.0, 0.5, &hits), &note_list(&notes));
    assert!(read.counts.drop_skips.residual >= 1, "{:?}", read.counts);
    assert_eq!(read.drops, [], "{:?}", read.counts);
}

#[test]
fn a_drop_settles_only_on_a_note_that_starts_near_its_strike() {
    // A fall into C1 struck at 0.2 s, found in the search of a note at
    // 82.4 Hz that starts with it. A C1 note that started 100 ms before
    // the strike takes the drop; one that started 200 ms before belongs
    // to an earlier sound, and the fall settles on no note of its own.
    let mono = sweep(32.7, 36.0, 0.06, &[0.2], 0.003, 0.6);
    for (on, drops) in [(0.1, 1), (0.0, 0)] {
        let read = drop_layer(&mono, &note_list(&[(on, 0.8, 24.0), (0.22, 0.6, 40.0)]));
        assert_eq!(read.drops.len(), drops, "{on}: {:?}", read.counts);
    }
}

#[test]
fn a_low_sweep_reads_up_to_a_restrike_inside_its_note() {
    // A fall into C1 whose harmonics swing the 20 to 200 Hz line, struck
    // again 380 ms later with its phase turned over, inside the same note.
    // Where the two cancel, the note line's frequency runs below 0 Hz, and
    // the fit ends there; over the whole 400 ms span the restrike spoilt
    // the fit. A real sweep of this shape read as a glide.
    let (f_end, df, tau, harmonics) = (32.7, 36.0, 0.06, [0.4, 0.5, 0.7]);
    let mut mono = sweep(f_end, df, tau, &harmonics, 0.08, 0.38);
    let rate = f64::from(RATE);
    mono.resize(round_to_usize(1.6 * rate), 0.0);
    let turned = std::f64::consts::TAU * (f_end * 0.38 + df * tau * (1.0 - (-0.38 / tau).exp()))
        + std::f64::consts::PI;
    for (n, x) in mono.iter_mut().enumerate() {
        let t = f64::from(u32::try_from(n).unwrap()) / rate - 0.2;
        if (0.38..1.2).contains(&t) {
            let phase = turned + std::f64::consts::TAU * f_end * (t - 0.38);
            let overtones: f64 = (2_u32..)
                .zip(harmonics)
                .map(|(k, a)| a * (f64::from(k) * phase + 1.0).sin())
                .sum();
            *x += f64::midpoint(phase.sin(), overtones) * (-t).exp();
        }
    }
    let read = drop_layer(&mono, &note_list(&[(0.25, 1.4, 24.0)]));
    assert_eq!(read.drops.len(), 1, "{:?}", read.counts);
    let fit = &read.drops[0].value;
    assert!((fit.f_end.get() - f_end).abs() < 1.0, "{fit:?}");
}

#[test]
fn a_tone_an_octave_above_the_note_reads_no_drop_on_the_note_line() {
    // A tone at 123.5 Hz struck at 0.2 s, and the note's 61.7 Hz entering
    // 70 to 75 ms later. The note line keeps the upper tone down, and its
    // line falls from it to the lower one as that enters: a step between
    // two tones, not a fall. Over that fall the note line lies 7.3 and
    // 6.6 dB under the band, and without the 6 dB cap each read as a drop.
    // A real tone of this shape read as one.
    let rate = f64::from(RATE);
    for (at, attack, level) in [(0.075, 0.03, 0.6), (0.07, 0.05, 0.7)] {
        let mono: Vec<f64> = (0..round_to_usize(1.2 * rate))
            .map(|n| {
                let t = f64::from(u32::try_from(n).unwrap()) / rate - 0.2;
                if t < 0.0 {
                    return 0.0;
                }
                let tone = |f: f64, at: f64, attack: f64, level: f64| {
                    let u = t - at;
                    if u < 0.0 {
                        0.0
                    } else {
                        level * (u / attack).min(1.0) * (std::f64::consts::TAU * f * u).sin()
                    }
                };
                0.4 * (-t / 0.8).exp()
                    * (tone(123.47, 0.0, 0.003, 1.0) + tone(61.74, at, attack, level))
            })
            .collect();
        let read = drop_layer(&mono, &note_list(&[(0.35, 0.6, 35.0)]));
        assert_eq!(read.drops, [], "{at}: {:?}", read.counts);
    }
}

#[test]
fn a_vibrato_right_after_a_glide_on_the_bound_keeps_its_span() {
    // A rise of 180 cents over 122 ms with p on the upper bound, then a
    // vibrato of 20 cents at 5.9 Hz until the voice stops at 1.21 s. The
    // glide holds within 10 cents of its end from 54 ms on, and the
    // vibrato's span starts there. Held for the glide's full fitted time,
    // the span fell under the 0.5 s the search reads; a real vibrato of
    // this shape was lost that way.
    let cents = |t: f64| {
        (t < 1.21).then(|| {
            let x = ((t - 0.5) / 0.122).clamp(0.0, 1.0);
            let vibrato = if t > 0.56 {
                20.0 * (std::f64::consts::TAU * 5.9 * (t - 0.56)).sin()
            } else {
                0.0
            };
            6800.0 + 180.0 * (1.0 - (1.0 - x).powi(5)) + vibrato
        })
    };
    let mono = vec![0.0; round_to_usize(1.6 * f64::from(RATE))];
    let read = audio_track_layer(&mono, cents, &[(0.2, 0.5, 68.0), (0.5, 1.21, 70.0)]);
    assert_eq!(read.glides.len(), 1, "{:?}", read.glides);
    assert!(read.glides[0].value.on_bound, "{:?}", read.glides);
    assert_eq!(read.vibrato.len(), 1, "{:?}", read.counts);
}

#[test]
fn a_kick_drop_faster_than_a_period_reads_no_drop_on_the_note_line() {
    // A kick from 255 Hz into 55 Hz with a tau of 3 ms: the fall ends
    // within one period of the note. On a line low-passed at 1.5 times the
    // note the low-pass smears it into a fall of 5 ms that reads a fifth of
    // its size. Without the floor of 10 ms on its tau, the note line read
    // such kicks with a wrong fall.
    for (attack, length) in [(0.001, 0.35), (0.0005, 0.4)] {
        let read = drop_layer(
            &sweep(55.0, 200.0, 0.003, &[], attack, length),
            &note_list(&[(0.2, 0.55, 33.0)]),
        );
        assert_eq!(read.drops, [], "{attack}: {:?}", read.counts);
    }
}

#[test]
fn drops_struck_250_ms_apart_read_each() {
    // Three 808 hits of 0.25 s, each with an octave drop of tau 60 ms: each
    // strike comes 4.2 time constants after the one before, while that
    // drop still settles. Each is a strike of its own and reads a drop; a
    // skip of every onset inside the settling read two.
    const LINE: [f64; 3] = [28.0, 31.0, 26.0];
    const DROPS: [(usize, f64, f64); 3] = [(0, 1.0, 0.06), (1, 1.0, 0.06), (2, 1.0, 0.06)];
    let hits = Hits {
        hit_s: 0.25,
        drops: &DROPS,
        ..Hits::new(&LINE, 0.0)
    };
    let read = drop_layer(&hits.render(f64::from(RATE)), &note_list(&hits.notes()));
    assert_eq!(read.drops.len(), 3, "{:?}", read.counts);
    for (drop, pitch) in read.drops.iter().zip(LINE) {
        let cents = 1200.0 * (drop.value.f_end.get() / 440.0).log2() + 6900.0 - 100.0 * pitch;
        assert!(cents.abs() <= 100.0, "{cents}: {:?}", drop.value);
    }
}

#[test]
fn a_slow_strike_across_a_search_border_reads_one_drop() {
    // A fall into C1 struck at 0.2 s under two C1 notes. The first note's
    // search ends 5 to 70 ms into the strike's attack and finds it; the
    // second search starts there, inside the rise, and finds it again. A
    // second harmonic as loud as the fundamental beats with it and dips the
    // envelope by up to 12 dB in the rise; over two periods of the note the
    // beat averages out. A fixed 20 ms after the border read a second drop
    // where the 80 ms attack crossed the half-peak later.
    for (h2, attack_s, tau) in [
        (0.2, 0.03, 0.03),
        (0.2, 0.08, 0.03),
        (0.2, 0.08, 0.06),
        (1.0, 0.08, 0.06),
    ] {
        let mono = sweep(32.7, 36.0, tau, &[h2], attack_s, 0.6);
        for border in [0.005, 0.01, 0.015, 0.025, 0.035, 0.04, 0.055, 0.07] {
            let notes = note_list(&[(0.1 + border, 0.8, 24.0), (0.22 + border, 0.8, 24.0)]);
            let read = drop_layer(&mono, &notes);
            assert_eq!(
                read.drops.len(),
                1,
                "{h2} {attack_s} {tau} {border}: {:?}",
                read.counts
            );
        }
    }
}

#[test]
fn drops_struck_115_ms_apart_read_each() {
    // Six 808 hits of 105 ms with 10 ms gaps, the last three with drops of
    // tau 30 ms. Each strike lands 15 ms after the border of its search,
    // while the drop before still settles. The envelope dips in the gap
    // before each, so each is a strike of its own. A fixed 20 ms after the
    // border skipped one of them; an average over three periods of the
    // note smooths the dip of the fifth hit of the second line away.
    const LINES: [[f64; 6]; 2] = [
        [33.0, 36.0, 31.0, 33.0, 36.0, 31.0],
        [38.0, 33.0, 38.0, 33.0, 38.0, 33.0],
    ];
    const DROPS: [[(usize, f64, f64); 3]; 2] = [
        [(3, 0.5, 0.03), (4, 0.5, 0.03), (5, 0.5, 0.03)],
        [(3, 1.0, 0.03), (4, 1.0, 0.03), (5, 1.0, 0.03)],
    ];
    for (line, drops) in LINES.iter().zip(&DROPS) {
        let hits = Hits {
            hit_s: 0.105,
            gap_s: 0.01,
            drops,
            ..Hits::new(line, 0.0)
        };
        let notes = hits.notes();
        let read = drop_layer(&hits.render(f64::from(RATE)), &note_list(&notes));
        assert_eq!(
            read.counts.drop_skips.settling, 0,
            "{line:?}: {:?}",
            read.counts
        );
        assert_eq!(read.drops.len(), 3, "{line:?}: {:?}", read.counts);
        for (drop, &(on, _, _)) in read.drops.iter().zip(&notes[3..]) {
            assert!(
                (drop.value.onset.get() - on).abs() < 0.005,
                "{on}: {:?}",
                drop.value
            );
        }
    }
}

#[test]
fn a_fast_sweep_into_c1_reads_on_the_note_line_above_the_tau_floor() {
    // From 68.7 Hz into C1 with a tau of 12 ms and a second harmonic 4 dB
    // under the fundamental, struck sharply. The 20 to 200 Hz line reads
    // no drop; the note line reads one with a tau of 17 ms, above the
    // 10 ms floor, its low-pass slowing the fall it follows.
    let read = drop_layer(
        &sweep(32.7, 36.0, 0.012, &[0.6, 0.3, 0.2], 0.003, 0.6),
        &note_list(&[(0.2, 0.75, 24.0)]),
    );
    assert_eq!(read.drops.len(), 1, "{:?}", read.counts);
    let fit = &read.drops[0].value;
    assert!((fit.f_end.get() - 32.7).abs() < 1.0, "{fit:?}");
    assert!((0.012..0.02).contains(&fit.tau.get()), "{fit:?}");
}

#[test]
fn fast_sweeps_whose_band_fit_sits_on_the_tau_bound_read_on_the_note_line() {
    // Falls of an octave into A1 and of half an octave into C2, tau 12 ms,
    // with overtones 4 to 14 dB under the fundamental. The 20 to 200 Hz
    // line and the 10 Hz one fit them with tau on the 3.04 ms bound of the
    // search; the note line reads each with a tau of 13 to 15 ms. A refit
    // after a size or residual miss alone read neither.
    for (f_end, df, pitch) in [(55.0, 55.0, 33.0), (65.4, 32.7, 36.0)] {
        let read = drop_layer(
            &sweep(f_end, df, 0.012, &[0.6, 0.3, 0.2], 0.003, 0.6),
            &note_list(&[(0.2, 0.8, pitch)]),
        );
        assert_eq!(read.drops.len(), 1, "{f_end}: {:?}", read.counts);
        let fit = &read.drops[0].value;
        assert!((fit.f_end.get() - f_end).abs() < 1.0, "{fit:?}");
        assert!((0.012..0.02).contains(&fit.tau.get()), "{fit:?}");
    }
}

#[test]
fn a_drop_settles_on_a_note_that_sounds_once_it_has_settled() {
    // A fall into C1 struck at 0.2 s with tau 54 ms, found in the search of
    // a note at 82.4 Hz that starts with it and ends at 0.6 s. Its fall
    // has settled at 0.474 s. A C1 note that ends at 0.52 s takes the drop;
    // one that ends at 0.44 s does not, since it stops before the fall
    // settles. Ten time constants run past the end of the fit span, and
    // then neither note took it.
    let mono = sweep(32.7, 36.0, 0.06, &[0.2], 0.003, 0.6);
    for (off, drops) in [(0.52, 1), (0.44, 0)] {
        let read = drop_layer(&mono, &note_list(&[(0.06, off, 24.0), (0.22, 0.6, 40.0)]));
        assert_eq!(read.drops.len(), drops, "{off}: {:?}", read.counts);
    }
}

#[test]
fn a_vibrato_after_a_slow_drop_starts_once_the_drop_has_settled() {
    // An octave drop into A1 with tau 60 ms, then a vibrato of 35 cents at
    // 5.5 Hz from five time constants on. Its search starts there and reads
    // it. Started 100 ms after the onset, a fifth of the fall still lay in
    // the span, and the search read no vibrato.
    let tau = 0.06;
    let mono = sweep(55.0, 55.0, tau, &[0.2], 0.003, 1.2);
    let cents = |t: f64| {
        let x = t - 0.2;
        let vibrato = if x > 5.0 * tau {
            35.0 * (std::f64::consts::TAU * 5.5 * x).sin()
        } else {
            0.0
        };
        Some(3300.0 + 1200.0 * (1.0 + (-x / tau).exp()).log2() + vibrato)
    };
    let read = audio_track_layer(&mono, cents, &[(0.2, 1.4, 33.0)]);
    assert_eq!(read.drops.len(), 1, "{:?}", read.counts);
    assert_eq!(read.vibrato.len(), 1, "{:?}", read.counts);
    let vibrato = &read.vibrato[0].value;
    assert!((vibrato.rate.get() - 5.5).abs() <= 0.1, "{vibrato:?}");
    assert!(
        (vibrato.extent().get() / 35.0 - 1.0).abs() <= 0.15,
        "{vibrato:?}"
    );
}
