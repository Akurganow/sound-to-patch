//! Note masks and the energy outside them (#50).

use std::f64::consts::PI;

use sound_to_patch::audio::SampleRate;
use sound_to_patch::dsp::StftParams;
use sound_to_patch::notes::{
    Contour, ContourPoint, MaskParams, Note, NoteList, out_of_mask_fraction,
};
use sound_to_patch::units::{Hz, MidiPitch, Seconds};

use crate::support::signals::times;

const RATE: f64 = 44_100.0;
const SECONDS: f64 = 2.0;

fn rate() -> SampleRate {
    SampleRate::new(44_100).unwrap()
}

/// A 50 ms raised-cosine fade at both ends, so the start and the stop add
/// no broadband click.
fn fade(t: f64) -> f64 {
    let edge = 0.05;
    let ramp = |u: f64| 0.5 - 0.5 * (PI * u.clamp(0.0, 1.0)).cos();
    ramp(t / edge) * ramp((SECONDS - t) / edge)
}

/// Harmonics 1 to 10 at amplitude 1/h of a phase track `phase(t)` of the
/// fundamental.
fn harmonic_tone(phase: impl Fn(f64) -> f64) -> Vec<f64> {
    times(RATE, 88_200)
        .into_iter()
        .map(|t| {
            let sum: f64 = (1..=10)
                .map(f64::from)
                .map(|h| (h * phase(t)).sin() / h)
                .sum();
            fade(t) * sum
        })
        .collect()
}

fn steady(frequency: f64) -> Vec<f64> {
    harmonic_tone(|t| 2.0 * PI * frequency * t)
}

fn mix(a: &[f64], b: &[f64]) -> Vec<f64> {
    a.iter().zip(b).map(|(x, y)| x + y).collect()
}

fn whole(pitch: f64) -> Note {
    Note::new(
        Seconds::new(0.0).unwrap(),
        Seconds::new(SECONDS + 0.5).unwrap(),
        MidiPitch::new(pitch).unwrap(),
    )
    .unwrap()
}

fn fraction(signal: &[f64], notes: &NoteList) -> f64 {
    let stft = StftParams::new(4096).unwrap();
    out_of_mask_fraction(signal, rate(), stft, notes, MaskParams::new())
        .unwrap()
        .get()
}

fn power(x: &[f64]) -> f64 {
    x.iter().map(|v| v * v).sum()
}

#[test]
fn a_two_note_chord_leaves_under_one_percent_outside_its_masks() {
    // A2 and E3.
    let chord = mix(&steady(110.0), &steady(164.813_778_456_434_9));
    let notes = NoteList::new(vec![whole(45.0), whole(52.0)]);
    let outside = fraction(&chord, &notes);
    assert!(outside < 0.01, "{outside}");
}

#[test]
fn zero_padding_keeps_a_steady_tone_inside_its_mask() {
    // Padding narrows the bins but not the Hann lobe, so the default sigma
    // must not shrink with it.
    let tone = steady(110.0);
    let notes = NoteList::new(vec![whole(45.0)]);
    for factor in [1, 2, 4, 16] {
        let stft = StftParams::new(4096)
            .unwrap()
            .with_oversample(factor)
            .unwrap();
        let outside = out_of_mask_fraction(&tone, rate(), stft, &notes, MaskParams::new())
            .unwrap()
            .get();
        assert!(outside < 0.01, "x{factor}: {outside}");
    }
}

#[test]
fn an_inharmonic_tone_raises_the_fraction_by_its_energy_share() {
    let chord = mix(&steady(110.0), &steady(164.813_778_456_434_9));
    let notes = NoteList::new(vec![whole(45.0), whole(52.0)]);
    // 270 Hz lies 50 Hz from the nearest partial, outside every mask.
    let raw: Vec<f64> = times(RATE, 88_200)
        .into_iter()
        .map(|t| fade(t) * (2.0 * PI * 270.0 * t).sin())
        .collect();
    // 20 dB below the chord in power.
    let gain = (power(&chord) / power(&raw) / 100.0).sqrt();
    let tone: Vec<f64> = raw.iter().map(|v| gain * v).collect();
    let share = power(&tone) / power(&mix(&chord, &tone));
    let rise = fraction(&mix(&chord, &tone), &notes) - fraction(&chord, &notes);
    assert!(
        rise > share / 2.0 && rise < share * 2.0,
        "rise {rise}, share {share}"
    );
}

#[test]
fn a_glide_stays_under_a_mask_that_follows_its_contour() {
    // An exponential glide from 110 to 165 Hz over the whole signal; its
    // phase is the integral of f(t) = 110 r^(t / T).
    let ratio: f64 = 1.5;
    let f = |t: f64| 110.0 * ratio.powf(t / SECONDS);
    let glide = harmonic_tone(|t| {
        2.0 * PI * 110.0 * SECONDS / ratio.ln() * (ratio.powf(t / SECONDS) - 1.0)
    });
    let points = (0..=250)
        .map(|i| {
            let t = f64::from(i) * 0.01;
            ContourPoint {
                time: Seconds::new(t).unwrap(),
                f0: Hz::new(f(t)).unwrap(),
            }
        })
        .collect();
    let with_contour = NoteList::new(vec![
        whole(45.0).with_contour(Contour::new(points).unwrap()),
    ]);
    let constant = NoteList::new(vec![whole(45.0)]);
    let following = fraction(&glide, &with_contour);
    let fixed = fraction(&glide, &constant);
    assert!(following < 0.02, "{following}");
    assert!(fixed > 0.2, "{fixed}");
}
