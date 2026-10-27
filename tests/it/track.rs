//! The pYIN f0 track (#113) against librosa on the golden recipes.

use std::f64::consts::TAU;
use std::sync::OnceLock;

use serde_json::Value;
use sound_to_patch::audio::SampleRate;
use sound_to_patch::notes::{MODEL_RATE, Note, NoteList, PitchTrack, TRACK_HOP, TrackError};
use sound_to_patch::units::{MidiPitch, Seconds};

use crate::support::cast::round_to_usize;
use crate::support::golden::Golden;

fn rate() -> f64 {
    f64::from(MODEL_RATE.get())
}

fn hop_s() -> f64 {
    f64::from(u32::try_from(TRACK_HOP).unwrap()) / rate()
}

fn f(value: &Value) -> f64 {
    value.as_f64().unwrap()
}

fn sine_bank(recipe: &Value) -> Vec<f64> {
    let gap = vec![0.0; round_to_usize(f(&recipe["gap_s"]) * rate())];
    let tone = round_to_usize(f(&recipe["tone_s"]) * rate());
    let mut out = gap.clone();
    for frequency in recipe["frequencies_hz"].as_array().unwrap() {
        out.extend((0..tone).map(|n| {
            let t = f64::from(u32::try_from(n).unwrap()) / rate();
            f(&recipe["amplitude"]) * (TAU * f(frequency) * t).sin()
        }));
        out.extend(&gap);
    }
    out
}

fn glide(recipe: &Value, glide_ms: f64) -> Vec<f64> {
    let hold = round_to_usize(f(&recipe["hold_s"]) * rate());
    let glide = round_to_usize(glide_ms / 1000.0 * rate());
    let (start, semitones) = (f(&recipe["start_hz"]), f(&recipe["semitones"]));
    let step = |i: usize| -> f64 {
        if i < hold {
            0.0
        } else if i < hold + glide {
            f64::from(u32::try_from(i - hold).unwrap()) / f64::from(u32::try_from(glide).unwrap())
        } else {
            1.0
        }
    };
    let top = start * 2_f64.powf(semitones / 12.0);
    let harmonics: Vec<f64> = (1..=u32::MAX)
        .map(f64::from)
        .take_while(|h| h * top < rate() / 2.0)
        .collect();
    let mut phase = 0.0;
    (0..2 * hold + glide)
        .map(|i| {
            let x: f64 = harmonics.iter().map(|h| (TAU * h * phase).sin() / h).sum();
            phase += start * 2_f64.powf(step(i) * semitones / 12.0) / rate();
            0.5 * x
        })
        .collect()
}

/// The track of the golden sine bank, run once for every test that reads it.
fn bank_track() -> &'static PitchTrack {
    static TRACK: OnceLock<PitchTrack> = OnceLock::new();
    TRACK.get_or_init(|| {
        let golden = Golden::load("pitch-track");
        PitchTrack::of(&sine_bank(&golden.recipe()["sine_bank"]), MODEL_RATE).unwrap()
    })
}

/// The 10-90 % rise of a glide in milliseconds, read as the golden reads it.
fn rise_ms(track: &PitchTrack, recipe: &Value) -> f64 {
    let (start, semitones) = (f(&recipe["start_hz"]), f(&recipe["semitones"]));
    let first_above = |level: f64| {
        track
            .frames()
            .iter()
            .position(|frame| {
                frame
                    .f0
                    .is_some_and(|f0| 12.0 * (f0.get() / start).log2() >= level)
            })
            .unwrap()
    };
    let frames = first_above(0.9 * semitones) - first_above(0.1 * semitones);
    f64::from(u32::try_from(frames).unwrap()) * hop_s() * 1000.0
}

#[test]
fn a_low_sine_bank_reads_within_20_cents_of_librosa_with_no_octave_error() {
    let golden = Golden::load("pitch-track");
    let bank = &golden.recipe()["sine_bank"];
    let track = bank_track();
    let (gap, tone) = (f(&bank["gap_s"]), f(&bank["tone_s"]));
    for (i, expected) in golden.values()["sine_bank"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let start = gap + f64::from(u32::try_from(i).unwrap()) * (tone + gap);
        let frames: Vec<_> = track
            .frames()
            .iter()
            .filter(|frame| (start + 0.1..=start + tone - 0.1).contains(&frame.time.get()))
            .collect();
        let mut voiced: Vec<f64> = frames
            .iter()
            .filter_map(|frame| frame.f0)
            .map(sound_to_patch::units::Hz::get)
            .collect();
        voiced.sort_by(f64::total_cmp);
        let median = voiced[voiced.len() / 2];
        let cents = 1200.0 * (median / f(&expected["median_hz"])).log2();
        assert!(
            cents.abs() <= 20.0,
            "tone {i}: {median} Hz, {cents} cents from librosa"
        );
        let truth = 1200.0 * (median / f(&expected["frequency_hz"])).log2();
        assert!(truth.abs() <= 20.0, "tone {i}: {truth} cents from the tone");
        let expected_voiced =
            f64::from(u32::try_from(expected["voiced"].as_u64().unwrap()).unwrap());
        let ours = f64::from(u32::try_from(voiced.len()).unwrap());
        assert!(
            (ours - expected_voiced).abs() <= 0.02 * expected_voiced + 1.0,
            "tone {i}: {ours} voiced"
        );
        assert!(
            frames
                .len()
                .abs_diff(usize::try_from(expected["frames"].as_u64().unwrap()).unwrap())
                <= 1
        );
    }
    // Octave errors: no voiced frame anywhere lies 6 or more semitones from
    // every tone.
    for frame in track.frames() {
        if let Some(f0) = frame.f0 {
            let nearest = [30.87_f64, 41.2, 55.0]
                .iter()
                .map(|tone| (12.0 * (f0.get() / tone).log2()).abs())
                .fold(f64::INFINITY, f64::min);
            assert!(nearest < 6.0, "{} Hz at {}", f0.get(), frame.time.get());
        }
    }
}

#[test]
fn glides_of_150_and_400_ms_rise_as_librosa_reads_them() {
    let golden = Golden::load("pitch-track");
    let recipe = &golden.recipe()["glides"];
    let mut read = Vec::new();
    for ms in [50.0, 150.0, 400.0] {
        let track = PitchTrack::of(&glide(recipe, ms), MODEL_RATE).unwrap();
        let ours = rise_ms(&track, recipe);
        let expected = &golden.values()["glides"][format!("{ms:?}")];
        if ms > 100.0 {
            assert!(
                (ours - f(&expected["rise_ms"])).abs() <= 2.0 * hop_s() * 1000.0 + 1e-9,
                "{ms} ms read {ours}"
            );
            assert!(
                (ours - f(&expected["true_rise_ms"])).abs() <= 0.05 * f(&expected["true_rise_ms"]),
                "{ms} ms read {ours}"
            );
        }
        read.push(ours);
    }
    // A 50 ms glide is too fast for the track: its rise misses the 5 %
    // bound that the slower glides meet.
    assert!(
        (read[0] - 40.0).abs() > 0.05 * 40.0,
        "50 ms read {}",
        read[0]
    );
}

#[test]
fn contours_hold_the_track_between_onset_and_offset() {
    let track = bank_track();
    let s = |value| Seconds::new(value).unwrap();
    let notes = NoteList::new(vec![
        Note::new(s(0.3), s(0.7), MidiPitch::new(23.0).unwrap()).unwrap(),
        Note::new(s(0.05), s(0.2), MidiPitch::new(40.0).unwrap()).unwrap(),
    ]);
    let filled = track.with_contours(&notes);
    let note = &filled.notes()[1];
    let contour = note.contour().unwrap();
    let expected: Vec<_> = track.voiced_between(s(0.3), s(0.7)).collect();
    assert_eq!(contour.points().len(), expected.len());
    for (point, (time, f0)) in contour.points().iter().zip(expected) {
        assert!((0.3..0.7).contains(&point.time.get()));
        assert_eq!((point.time, point.f0), (time, f0));
    }
    // The silent gap has no voiced frame, so that note keeps no contour.
    assert!(filled.notes()[0].contour().is_none());
}

#[test]
fn silence_has_no_voiced_frame_and_a_nan_is_an_error() {
    assert_eq!(
        PitchTrack::of(&vec![0.0; 4410], MODEL_RATE)
            .unwrap()
            .voiced_pitches(),
        Vec::new()
    );
    assert_eq!(PitchTrack::of(&[], MODEL_RATE).unwrap().frames(), &[]);
    let mut samples = vec![0.1; 4410];
    samples[7] = f64::NAN;
    assert!(matches!(
        PitchTrack::of(&samples, MODEL_RATE),
        Err(TrackError::NotFinite(7))
    ));
    let native = SampleRate::new(48_000).unwrap();
    assert!(matches!(
        PitchTrack::of(&samples, native),
        Err(TrackError::Resample(_))
    ));
}

#[test]
fn a_quiet_stem_still_reads_voiced() {
    let golden = Golden::load("pitch-track");
    let quiet: Vec<f64> = sine_bank(&golden.recipe()["sine_bank"])
        .iter()
        .map(|x| x * 1e-4)
        .collect();
    let track = PitchTrack::of(&quiet, MODEL_RATE).unwrap();
    let loud = bank_track().voiced_pitches().len();
    let quiet = track.voiced_pitches().len();
    assert!(quiet + loud / 50 >= loud, "{quiet} of {loud} frames voiced");
}
