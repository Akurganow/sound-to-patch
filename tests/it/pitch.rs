//! The pitch layer (#114, #66, #67, #148): the sub-band line, pitch drops,
//! glides and vibrato on generated signals.
//!
//! The f0 track takes about 40 s per second of audio in a debug build, so
//! the glide and vibrato cases read committed fixtures: the notes and the
//! track that the transcriber and the f0 track give for each generated
//! signal. `pitch_fixtures_are_current` rebuilds them in a release build
//! and fails when one differs.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sound_to_patch::audio::SampleRate;
use sound_to_patch::confidence::Level;
use sound_to_patch::estimate::pitch::{
    EXTENT_SHARE, PitchInput, PitchLayer, SubBandLine, band_onset, fit_drop,
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
use crate::support::pitch::{BassLine, Glide, PitchDrop, Vibrato};
use crate::support::rng::gaussian;
use crate::support::signals::write_wav16;

const RATE: u32 = 22_050;
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
    Vibrato(Vibrato),
    Bass,
}

impl Signal {
    fn render(self) -> Vec<f64> {
        match self {
            Self::Glide(glide) => glide.render(f64::from(RATE)),
            Self::Vibrato(vibrato) => vibrato.render(f64::from(RATE)),
            Self::Bass => BassLine::render(f64::from(RATE)),
        }
    }

    fn recipe(self) -> Value {
        match self {
            Self::Bass => json!({
                "kind": "bass_line",
                "notes": BassLine::NOTES,
                "drop": BassLine::DROP,
                "glide": BassLine::GLIDE,
            }),
            Self::Glide(g) => json!({
                "kind": "glide",
                "start_hz": g.start_hz,
                "semitones": g.semitones,
                "full_s": g.full_s,
                "p": g.p,
                "hold_s": g.hold_s,
                "voices": g.voices,
                "detune_ct": g.detune_ct,
            }),
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
        "sample_rate": RATE,
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
        rate: rate(),
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
        let t =
            transcribe_stem(&transcriber, &mono, rate(), Bpm::new(120.0).unwrap(), None).unwrap();
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
    for p in CURVES {
        let name = glide_name("mono", 0.05, p);
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
        // line's swing between the two partials fails the residual rule.
        assert_eq!(fit.accepted(), name == "noise", "{name}: {fit:?}");
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
