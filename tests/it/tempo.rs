//! Tempo refinement on generated drum, bass, lead and pad parts.

use sound_to_patch::audio::{SampleRate, Tempo};
use sound_to_patch::confidence::Level;
use sound_to_patch::units::Bpm;

use crate::support::cast::round_to_usize;
use crate::support::rng::uniform;
use crate::support::signals::polyblep_saw;

const RATE: u32 = 22_050;
/// Beats in each part: the 32 lags plus the overlap, plus one.
const BEATS: usize = 37;
const TEMPOS: [f64; 5] = [70.3, 97.55, 123.4, 147.81, 178.2];
const OFFSETS: [f64; 4] = [-0.03, -0.01, 0.01, 0.03];

#[derive(Clone, Copy, Debug)]
enum Part {
    Drums,
    Bass,
    Lead,
    Pad,
}

/// One note of a part: onset and length in beats, frequency, attack and
/// release in seconds.
struct Note {
    beat: f64,
    beats: f64,
    hz: f64,
    attack: f64,
    release: f64,
}

fn f(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap())
}

fn midi(key: f64) -> f64 {
    440.0 * 2_f64.powf((key - 69.0) / 12.0)
}

fn render_notes(notes: &[Note], bpm: f64, len: usize, gain: f64) -> Vec<f64> {
    let rate = f64::from(RATE);
    let beat = 60.0 / bpm;
    let mut out = vec![0.0; len];
    for note in notes {
        let start = round_to_usize(note.beat * beat * rate);
        let hold = note.beats * beat;
        let total = round_to_usize(((hold + note.release) * rate).ceil());
        let saw = polyblep_saw(note.hz, rate, total);
        for (i, s) in saw.iter().enumerate() {
            let t = f(i) / rate;
            let env = if t < note.attack {
                t / note.attack
            } else if t < hold {
                1.0
            } else {
                (1.0 - (t - hold) / note.release).max(0.0)
            };
            if let Some(slot) = out.get_mut(start + i) {
                *slot += gain * env * s;
            }
        }
    }
    out
}

fn part(kind: Part, bpm: f64) -> Vec<f64> {
    let rate = f64::from(RATE);
    let beat = 60.0 / bpm;
    let len = round_to_usize((f(BEATS) * beat * rate).ceil());
    match kind {
        Part::Drums => {
            let mut out = vec![0.0; len];
            let noise = uniform(7, len);
            for n in 0..BEATS * 2 {
                let start = round_to_usize(f(n) * beat / 2.0 * rate);
                for i in 0..round_to_usize(0.3 * rate) {
                    let Some(slot) = out.get_mut(start + i) else {
                        break;
                    };
                    let t = f(i) / rate;
                    *slot += if n % 2 == 0 {
                        // Kick: a falling sine.
                        let phase = 2.0
                            * std::f64::consts::PI
                            * (50.0 * t + 2.0 * (1.0 - (-t / 0.02).exp()));
                        0.8 * phase.sin() * (-t / 0.12).exp()
                    } else {
                        // Hat: a noise burst.
                        0.2 * (2.0 * noise[start + i] - 1.0) * (-t / 0.02).exp()
                    };
                }
            }
            out
        }
        Part::Bass => {
            let keys = [33.0, 33.0, 36.0, 33.0, 40.0, 33.0, 31.0, 33.0];
            let notes: Vec<Note> = (0..BEATS * 2)
                .map(|n| Note {
                    beat: f(n) / 2.0,
                    beats: 0.4,
                    hz: midi(keys[n % keys.len()]),
                    attack: 0.003,
                    release: 0.01,
                })
                .collect();
            render_notes(&notes, bpm, len, 0.3)
        }
        Part::Lead => {
            // Quarter, quarter, eighth, eighth, half: four beats per bar.
            let bar = [(0.0, 1.0), (1.0, 1.0), (2.0, 0.5), (2.5, 0.5), (3.0, 1.0)];
            let keys = [69.0, 72.0, 76.0, 74.0, 72.0, 71.0, 69.0, 67.0];
            let mut notes = Vec::new();
            for b in 0..=BEATS / 4 {
                for (i, (at, length)) in bar.iter().enumerate() {
                    notes.push(Note {
                        beat: f(b) * 4.0 + at,
                        beats: length * 0.9,
                        hz: midi(keys[(b * bar.len() + i) % keys.len()]),
                        attack: 0.01,
                        release: 0.05,
                    });
                }
            }
            render_notes(&notes, bpm, len, 0.25)
        }
        Part::Pad => {
            let chords = [
                [57.0, 60.0, 64.0],
                [53.0, 57.0, 60.0],
                [55.0, 59.0, 62.0],
                [52.0, 55.0, 59.0],
            ];
            let mut notes = Vec::new();
            for b in 0..=BEATS / 4 {
                for &key in &chords[b % chords.len()] {
                    notes.push(Note {
                        beat: f(b) * 4.0,
                        beats: 4.0,
                        hz: midi(key),
                        attack: 0.2,
                        release: 0.3,
                    });
                }
            }
            render_notes(&notes, bpm, len, 0.15)
        }
    }
}

fn refine(samples: &[f64], given: f64) -> Tempo {
    Tempo::refine(
        samples,
        SampleRate::new(RATE).unwrap(),
        Bpm::new(given).unwrap(),
    )
    .unwrap()
}

fn check(kind: Part, truth: f64) {
    {
        let samples = part(kind, truth);
        for offset in OFFSETS {
            let tempo = refine(&samples, truth * (1.0 + offset));
            match kind {
                Part::Pad => {}
                _ => assert!(
                    (tempo.used().get() - truth).abs() <= 0.05,
                    "{kind:?} at {truth} BPM, --bpm offset {offset}: {}",
                    tempo.summary()
                ),
            }
        }
    }
}

/// One test per part and tempo, so the grid runs in parallel.
macro_rules! grid {
    ($($name:ident: $kind:ident, $tempo:literal;)+) => {
        $(
            #[test]
            fn $name() {
                check(Part::$kind, TEMPOS[$tempo]);
            }
        )+
    };
}

grid! {
    drums_70: Drums, 0; drums_98: Drums, 1; drums_123: Drums, 2; drums_148: Drums, 3; drums_178: Drums, 4;
    bass_70: Bass, 0; bass_98: Bass, 1; bass_123: Bass, 2; bass_148: Bass, 3; bass_178: Bass, 4;
    lead_70: Lead, 0; lead_98: Lead, 1; lead_123: Lead, 2; lead_148: Lead, 3; lead_178: Lead, 4;
    pad_70: Pad, 0; pad_98: Pad, 1; pad_123: Pad, 2; pad_148: Pad, 3; pad_178: Pad, 4;
}

#[test]
fn a_part_without_onsets_keeps_the_given_tempo_at_low_confidence() {
    // A steady tone of ten harmonics whose period divides the beat, so any
    // ripple at the period would repeat on every beat.
    let rate = f64::from(RATE);
    for (hz, bpm) in [
        (110.0, 120.0),
        (55.0, 132.0),
        (41.25, 165.0),
        (220.0, 120.0),
    ] {
        let len = round_to_usize(40.0 * 60.0 / bpm * rate);
        let samples: Vec<f64> = (0..len)
            .map(|n| {
                let t = f(n) / rate;
                let onset = (t / 0.005).min(1.0);
                let tone: f64 = (1..=10)
                    .map(|h| {
                        let h = f64::from(h);
                        (2.0 * std::f64::consts::PI * h * hz * t).sin() / h
                    })
                    .sum();
                0.2 * onset * tone
            })
            .collect();
        let given = bpm * 1.01;
        let tempo = refine(&samples, given);
        assert_eq!(tempo.used().get(), given, "{hz} Hz: {}", tempo.summary());
        assert_ne!(tempo.refined().confidence().level(), Some(Level::High));
    }
}
