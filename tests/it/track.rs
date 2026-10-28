//! The pYIN f0 track (#113) against librosa on the golden recipes.

use std::f64::consts::TAU;

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

/// `seconds` of a sine at `level` between 0.1 s of silence on each side.
fn padded_tone(frequency: f64, seconds: f64, level: f64) -> Vec<f64> {
    let pad = vec![0.0; round_to_usize(0.1 * rate())];
    let mut out = pad.clone();
    out.extend((0..round_to_usize(seconds * rate())).map(|n| {
        let t = f64::from(u32::try_from(n).unwrap()) / rate();
        level * (TAU * frequency * t).sin()
    }));
    out.extend(&pad);
    out
}

#[test]
fn a_low_sine_bank_reads_within_20_cents_of_librosa_with_no_octave_error() {
    let golden = Golden::load("pitch-track");
    let bank = &golden.recipe()["sine_bank"];
    let track = PitchTrack::of(&sine_bank(bank), MODEL_RATE).unwrap();
    let (gap, tone) = (f(&bank["gap_s"]), f(&bank["tone_s"]));
    for (i, expected) in golden.values()["sine_bank"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let truth = f(&expected["frequency_hz"]);
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
        // Octave check: every voiced frame lies within 100 cents of its own
        // tone.
        for f0 in &voiced {
            let cents = 1200.0 * (f0 / truth).log2();
            assert!(cents.abs() <= 100.0, "tone {truth} Hz read {f0} Hz");
        }
        voiced.sort_by(f64::total_cmp);
        let median = voiced[voiced.len() / 2];
        let from_librosa = 1200.0 * (median / f(&expected["median_hz"])).log2();
        let from_truth = 1200.0 * (median / truth).log2();
        let fraction = f64::from(u32::try_from(voiced.len()).unwrap())
            / f64::from(u32::try_from(frames.len()).unwrap());
        println!("{truth} Hz: median {median:.3} Hz, {from_truth:+.1} cents, voiced {fraction:.3}");
        assert!(
            from_librosa.abs() <= 20.0,
            "{truth} Hz: {from_librosa} cents from librosa"
        );
        assert!(
            from_truth.abs() <= 20.0,
            "{truth} Hz: {from_truth} cents from the tone"
        );
        let expected_voiced = f(&expected["voiced"]);
        let ours = f64::from(u32::try_from(voiced.len()).unwrap());
        assert!(
            (ours - expected_voiced).abs() <= 0.02 * expected_voiced + 1.0,
            "{truth} Hz: {ours} voiced"
        );
        assert!(
            frames
                .len()
                .abs_diff(usize::try_from(expected["frames"].as_u64().unwrap()).unwrap())
                <= 1
        );
    }
}

/// The rise of one golden glide, against librosa and the truth.
fn check_glide(ms: f64) -> f64 {
    let golden = Golden::load("pitch-track");
    let recipe = &golden.recipe()["glides"];
    let track = PitchTrack::of(&glide(recipe, ms), MODEL_RATE).unwrap();
    let ours = rise_ms(&track, recipe);
    let expected = &golden.values()["glides"][format!("{ms:?}")];
    println!(
        "{ms} ms glide: rise {ours:.1} ms, librosa {:.1} ms",
        f(&expected["rise_ms"])
    );
    ours
}

#[test]
fn a_150_ms_glide_rises_as_librosa_reads_it() {
    let ours = check_glide(150.0);
    assert!(
        (ours - 121.905).abs() <= 2.0 * hop_s() * 1000.0 + 1e-9,
        "read {ours}"
    );
    assert!((ours - 120.0).abs() <= 0.05 * 120.0, "read {ours}");
}

#[test]
fn a_400_ms_glide_rises_as_librosa_reads_it() {
    let ours = check_glide(400.0);
    assert!(
        (ours - 319.274).abs() <= 2.0 * hop_s() * 1000.0 + 1e-9,
        "read {ours}"
    );
    assert!((ours - 320.0).abs() <= 0.05 * 320.0, "read {ours}");
}

#[test]
fn a_50_ms_glide_misses_the_librosa_reading() {
    // Glides under about 60 ms sit at the resolution of a 93 ms frame: the
    // two trackers disagree by more than the two hops the slower glides
    // meet. The Rust tracker reads 7 hops on this recipe.
    let ours = check_glide(50.0);
    assert!(
        (ours - 23.220).abs() > 2.0 * hop_s() * 1000.0,
        "read {ours}"
    );
    assert_eq!(round_to_usize(ours / (hop_s() * 1000.0)), 7);
}

#[test]
fn contours_hold_the_track_between_onset_and_offset() {
    let track = PitchTrack::of(&padded_tone(55.0, 0.4, 0.5), MODEL_RATE).unwrap();
    let s = |value| Seconds::new(value).unwrap();
    let notes = NoteList::new(vec![
        Note::new(s(0.15), s(0.45), MidiPitch::new(33.0).unwrap()).unwrap(),
        Note::new(s(0.0), s(0.03), MidiPitch::new(40.0).unwrap()).unwrap(),
    ]);
    let filled = track.with_contours(&notes);
    let contour = filled.notes()[1].contour().unwrap();
    let expected: Vec<_> = track.voiced_between(s(0.15), s(0.45)).collect();
    assert!(expected.len() > 40, "{} voiced frames", expected.len());
    assert_eq!(contour.points().len(), expected.len());
    for (point, (time, f0)) in contour.points().iter().zip(expected) {
        assert!((0.15..0.45).contains(&point.time.get()));
        assert_eq!((point.time, point.f0), (time, f0));
    }
    // The leading silence has no voiced frame, so that note keeps none.
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
    let track = PitchTrack::of(&padded_tone(41.2, 0.4, 0.5e-4), MODEL_RATE).unwrap();
    let s = |value| Seconds::new(value).unwrap();
    let middle = track
        .frames()
        .iter()
        .filter(|frame| (0.2..0.4).contains(&frame.time.get()))
        .count();
    let voiced = track.voiced_between(s(0.2), s(0.4)).count();
    assert!(
        voiced * 10 >= middle * 9,
        "{voiced} of {middle} frames voiced"
    );
    // Levels are relative to the loudest frame; a frame of digital
    // silence reads -200 dB.
    let levels: Vec<f64> = track
        .frames()
        .iter()
        .map(|frame| frame.level.get())
        .collect();
    let loudest = levels.iter().copied().fold(f64::MIN, f64::max);
    assert!(loudest.abs() < 1e-9, "loudest frame at {loudest} dB");
    assert!(
        (levels[0] + 200.0).abs() < 1e-9,
        "first frame at {} dB",
        levels[0]
    );
}
