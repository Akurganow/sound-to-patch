//! One model run per stem and its decodes (#57, #59).

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use basic_pitch::Posteriorgrams;
use basic_pitch::model::TractBackend;
use sound_to_patch::audio::SampleRate;
use sound_to_patch::notes::{
    Backend, DecodeSettings, MODEL_RATE, MODEL_SHA256, NoteList, PitchRange, Profile,
    TranscribeError, Transcriber, min_note_frames,
};
use sound_to_patch::units::{Bpm, MidiPitch};

use crate::support::cast::round_to_usize;
use crate::support::golden::Golden;
use crate::support::signals::sine;

/// The model engine with a count of the windows it ran.
#[derive(Clone)]
struct Counting {
    inner: TractBackend,
    windows: Arc<AtomicUsize>,
}

impl Backend for Counting {
    fn predict_window(&self, window: &[f32]) -> basic_pitch::Result<Posteriorgrams> {
        self.windows.fetch_add(1, Ordering::SeqCst);
        self.inner.predict_window(window)
    }
}

fn counting() -> (Transcriber, Arc<AtomicUsize>) {
    let windows = Arc::new(AtomicUsize::new(0));
    let backend = Counting {
        inner: TractBackend::new().unwrap(),
        windows: Arc::clone(&windows),
    };
    (Transcriber::with_backend(backend), windows)
}

/// The model's window count for `n` samples at its rate.
fn window_count(n: usize) -> usize {
    (n + 3840).div_ceil(36_164)
}

fn rate() -> f64 {
    f64::from(MODEL_RATE.get())
}

fn bpm(value: f64) -> Bpm {
    Bpm::new(value).unwrap()
}

/// Every pitch in a note list, in order.
fn pitches(list: &NoteList) -> Vec<f64> {
    list.notes().iter().map(|note| note.pitch().get()).collect()
}

/// `seconds` of a sine at half scale, between 0.25 s of silence on each
/// side.
fn tone(frequency: f64, seconds: f64) -> Vec<f64> {
    let pad = vec![0.0; 5512];
    let body = sine(frequency, 0.0, rate(), round_to_usize(seconds * rate()));
    [
        pad.as_slice(),
        &body.iter().map(|x| 0.5 * x).collect::<Vec<_>>(),
        &pad,
    ]
    .concat()
}

#[test]
fn the_thread_count_does_not_change_the_posteriorgrams() {
    let samples = tone(220.0, 4.0);
    let bits = |threads: usize| {
        let (transcriber, _) = counting();
        let run = transcriber
            .with_threads(NonZeroUsize::new(threads).unwrap())
            .run(&samples, MODEL_RATE)
            .unwrap();
        let p = run.posteriorgrams();
        [&p.note, &p.onset, &p.contour]
            .iter()
            .flat_map(|m| m.as_slice().iter().map(|x| x.to_bits()))
            .collect::<Vec<u32>>()
    };
    let one = bits(1);
    assert!(one.len() > 1000, "{} values", one.len());
    assert_eq!(one, bits(8));
}

#[test]
fn the_bundled_model_is_the_upstream_file() {
    use std::fmt::Write;

    use sha2::{Digest, Sha256};
    let upstream = "2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec";
    assert_eq!(MODEL_SHA256, upstream);
    let digest = Sha256::digest(basic_pitch::model::MODEL_ONNX);
    let hex = digest.iter().fold(String::new(), |mut hex, byte| {
        write!(hex, "{byte:02x}").unwrap();
        hex
    });
    assert_eq!(hex, upstream);
}

#[test]
fn three_profiles_decode_one_run() {
    let (transcriber, windows) = counting();
    let samples = tone(220.0, 3.5);
    let run = transcriber.run(&samples, MODEL_RATE).unwrap();
    let after_run = windows.load(Ordering::SeqCst);
    assert_eq!(after_run, window_count(samples.len()));
    assert_eq!(after_run, 3);
    for profile in Profile::ALL {
        let notes = run.decode(&profile.settings(bpm(120.0))).unwrap();
        assert_eq!(pitches(&notes), vec![57.0], "{profile}");
    }
    assert_eq!(windows.load(Ordering::SeqCst), after_run);
}

#[test]
fn a_one_second_440_hz_sine_gives_one_a4() {
    let (transcriber, _) = counting();
    let samples = tone(440.0, 1.0);
    let run = transcriber.run(&samples, MODEL_RATE).unwrap();
    let notes = run.decode(&Profile::Lead.settings(bpm(120.0))).unwrap();
    assert_eq!(pitches(&notes), vec![69.0]);
    let note = &notes.notes()[0];
    let (onset, offset) = (note.onset().get(), note.offset().get());
    let covered = (offset.min(1.25) - onset.max(0.25)) / 1.0;
    assert!(
        covered >= 0.9,
        "covers {covered} of the tone: {onset} to {offset}"
    );
    // Pinned from the first run: both ends fall within one frame and a
    // half of the tone's ends.
    assert!((onset - 0.25).abs() < 0.018, "onset {onset}");
    assert!((offset - 1.25).abs() < 0.018, "offset {offset}");
    assert!(note.velocity().is_some());
}

#[test]
fn silence_and_an_empty_stem_give_no_notes() {
    let (transcriber, _) = counting();
    for samples in [vec![0.0; 22_050], Vec::new()] {
        let run = transcriber.run(&samples, MODEL_RATE).unwrap();
        for profile in Profile::ALL {
            assert!(
                run.decode(&profile.settings(bpm(120.0)))
                    .unwrap()
                    .is_empty()
            );
        }
    }
}

#[test]
fn a_nan_sample_is_rejected_before_the_model_runs() {
    let (transcriber, windows) = counting();
    let mut samples = tone(440.0, 0.5);
    samples[100] = f64::NAN;
    let error = transcriber.run(&samples, MODEL_RATE).unwrap_err();
    assert!(
        matches!(
            error,
            TranscribeError::Model(basic_pitch::Error::NonFiniteAudio { index: 100, .. })
        ),
        "{error:?}"
    );
    assert_eq!(windows.load(Ordering::SeqCst), 0);
    assert_eq!(
        error.to_string(),
        "the transcription model failed: audio sample 100 is not finite (NaN)"
    );
}

#[test]
fn a_non_finite_sample_at_another_rate_fails_before_the_model_runs() {
    let (transcriber, windows) = counting();
    let native = SampleRate::new(48_000).unwrap();
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut samples = vec![0.1; 48_000];
        samples[5] = bad;
        let error = transcriber.run(&samples, native).unwrap_err();
        assert!(matches!(error, TranscribeError::Resample(_)), "{error:?}");
        assert_eq!(
            error.to_string(),
            format!(
                "cannot prepare the stem for the transcription model: cannot resample from 48000 Hz to 22050 Hz: sample 5 is not finite ({bad})"
            )
        );
    }
    assert_eq!(windows.load(Ordering::SeqCst), 0);
}

#[test]
fn a_stem_at_another_rate_is_resampled_first() {
    let (transcriber, windows) = counting();
    let native = SampleRate::new(48_000).unwrap();
    let samples: Vec<f64> = sine(220.0, 0.0, 48_000.0, 48_000)
        .into_iter()
        .map(|x| 0.5 * x)
        .collect();
    let run = transcriber.run(&samples, native).unwrap();
    assert_eq!(windows.load(Ordering::SeqCst), window_count(22_050));
    let notes = run.decode(&Profile::Lead.settings(bpm(120.0))).unwrap();
    assert_eq!(pitches(&notes), vec![57.0]);
}

#[test]
fn short_sines_decode_within_one_frame_of_their_length() {
    let frame = 256.0 / rate();
    let (transcriber, _) = counting();
    let settings = DecodeSettings {
        range: PitchRange::new(
            MidiPitch::new(21.0).unwrap(),
            MidiPitch::new(108.0).unwrap(),
        )
        .unwrap(),
        min_note_frames: 0,
        melodia: false,
    };
    let mut lengths = Vec::new();
    for frames in [10.0, 11.0, 12.0, 13.0] {
        let run = transcriber
            .run(&tone(440.0, frames * frame), MODEL_RATE)
            .unwrap();
        let notes = run.decode(&settings).unwrap();
        assert_eq!(pitches(&notes), vec![69.0], "{frames} frames");
        let note = &notes.notes()[0];
        let decoded = (note.offset().get() - note.onset().get()) / frame;
        assert!(
            (decoded - frames).abs() <= 1.0 + 1e-9,
            "{frames} frames read {decoded}"
        );
        lengths.push(round_to_usize(decoded));
    }
    // Pinned from the first run: each note reads one frame short.
    assert_eq!(lengths, vec![9, 10, 11, 12]);
}

#[test]
fn the_bpm_rule_and_the_decoder_frames_match_the_59_table() {
    let golden = Golden::load("prototype/59-min-note-length");
    let values = golden.values();
    for (bpm_text, row) in values["rule"].as_object().unwrap() {
        let tempo = bpm(bpm_text.parse().unwrap());
        let frames = usize::try_from(row["frames"].as_u64().unwrap()).unwrap();
        assert_eq!(min_note_frames(tempo), frames, "{bpm_text} BPM");
    }
    for (ms_text, row) in values["settings"].as_object().unwrap() {
        let params = basic_pitch::NoteParams {
            minimum_note_length_ms: ms_text.parse().unwrap(),
            ..basic_pitch::NoteParams::default()
        };
        let frames = usize::try_from(row["frames"].as_u64().unwrap()).unwrap();
        assert_eq!(
            params.min_note_len_frames().unwrap(),
            frames,
            "{ms_text} ms"
        );
    }
}
