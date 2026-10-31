//! Generated envelope material: saw chords under a sidechain duck or a
//! tremolo, plucks and pads.

use std::f64::consts::TAU;

use sound_to_patch::notes::{Note, NoteList};
use sound_to_patch::units::{MidiPitch, Seconds};

use super::rng::uniform;

/// The three-note saw chord of the pump table: A2, C3 and E3.
pub(crate) const CHORD: [f64; 3] = [110.0, 130.81, 164.81];

/// Harmonics of the saws stop here, which keeps debug builds quick; the
/// envelope tracks read the full band, and the top octaves add little.
const SAW_TOP: f64 = 6000.0;

/// A saw at `f0` of `length` samples: harmonic `h` at `1 / h` up to
/// [`SAW_TOP`], starting at phase `phase` cycles.
pub(crate) fn saw(f0: f64, rate: f64, length: usize, phase: f64) -> Vec<f64> {
    let mut out = vec![0.0; length];
    let top = (SAW_TOP.min(0.5 * rate) / f0).floor();
    let mut h = 1.0;
    while h <= top {
        let step = TAU * h * f0 / rate;
        let start = TAU * h * phase;
        for (n, x) in out.iter_mut().enumerate() {
            *x += (start + step * index(n)).sin() / h;
        }
        h += 1.0;
    }
    out
}

fn index(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap())
}

/// The saws of `pitches` summed, each at a seeded random phase, scaled to
/// a peak near 0.5.
pub(crate) fn chord(pitches: &[f64], rate: f64, seconds: f64, seed: u64) -> Vec<f64> {
    let length = samples(seconds, rate);
    let phases = uniform(seed, pitches.len());
    let mut out = vec![0.0; length];
    for (&f0, phase) in pitches.iter().zip(phases) {
        for (x, s) in out.iter_mut().zip(saw(f0, rate, length, phase)) {
            *x += s;
        }
    }
    let scale = 0.5 / out.iter().fold(0.0_f64, |m, x| m.max(x.abs())).max(1e-12);
    out.iter().map(|x| x * scale).collect()
}

/// The number of samples in `seconds`.
pub(crate) fn samples(seconds: f64, rate: f64) -> usize {
    let n = (seconds * rate).round();
    assert!((0.0..4e9).contains(&n));
    // In range by the assert above.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "checked"
    )]
    let n = n as usize;
    n
}

/// A sidechain duck on every beat: the gain drops to `-depth_db` at the
/// beat and recovers exponentially, `1 - (1 - g0) exp(-t / release_s)`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Duck {
    pub(crate) depth_db: f64,
    pub(crate) release_s: f64,
    pub(crate) bpm: f64,
}

impl Duck {
    /// The gain at time `t`.
    pub(crate) fn gain(self, t: f64) -> f64 {
        let beat = 60.0 / self.bpm;
        let g0 = 10f64.powf(-self.depth_db / 20.0);
        1.0 - (1.0 - g0) * (-(t.rem_euclid(beat)) / self.release_s).exp()
    }

    /// Applies the duck to `x`.
    pub(crate) fn apply(self, x: &[f64], rate: f64) -> Vec<f64> {
        x.iter()
            .enumerate()
            .map(|(n, v)| v * self.gain(index(n) / rate))
            .collect()
    }
}

/// A tremolo: the level swings by `swing_db` either way at `rate_hz`.
pub(crate) fn tremolo(x: &[f64], rate: f64, rate_hz: f64, swing_db: f64) -> Vec<f64> {
    x.iter()
        .enumerate()
        .map(|(n, v)| {
            let t = index(n) / rate;
            v * 10f64.powf(swing_db * (TAU * rate_hz * t).sin() / 20.0)
        })
        .collect()
}

/// Plucks: a saw at `f0` struck every `every_s` from 0, each decaying
/// exponentially with time constant `decay_s` until the next. The notes
/// end at the next strike.
pub(crate) fn plucks(
    f0: f64,
    rate: f64,
    seconds: f64,
    every_s: f64,
    decay_s: f64,
) -> (Vec<f64>, NoteList) {
    let tone = saw(f0, rate, samples(seconds, rate), 0.0);
    let x = tone
        .iter()
        .enumerate()
        .map(|(n, v)| 0.5 * v * (-(index(n) / rate).rem_euclid(every_s) / decay_s).exp())
        .collect();
    let mut notes = Vec::new();
    let mut on = 0.0;
    while on + every_s <= seconds + 1e-9 {
        notes.push((on, on + every_s, f0));
        on += every_s;
    }
    (x, note_list(&notes))
}

/// A note list from `(onset, offset, hz)` triples.
pub(crate) fn note_list(notes: &[(f64, f64, f64)]) -> NoteList {
    NoteList::new(
        notes
            .iter()
            .map(|&(on, off, hz)| {
                Note::new(
                    Seconds::new(on).unwrap(),
                    Seconds::new(off).unwrap(),
                    MidiPitch::new(69.0 + 12.0 * (hz / 440.0).log2()).unwrap(),
                )
                .unwrap()
            })
            .collect(),
    )
}

/// An ADSR gain: a linear attack, a hold at the peak, a decay linear in dB
/// to the sustain level, and a linear release from the level at the
/// note-off to zero.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Adsr {
    pub(crate) attack_s: f64,
    pub(crate) hold_s: f64,
    pub(crate) decay_s: f64,
    pub(crate) sustain_db: f64,
    pub(crate) release_s: f64,
}

impl Adsr {
    /// The gain `t` seconds after the note-on, for a note-off at `off`.
    pub(crate) fn gain(self, t: f64, off: f64) -> f64 {
        let held = |t: f64| {
            if t < 0.0 {
                0.0
            } else if t < self.attack_s {
                t / self.attack_s
            } else if t < self.attack_s + self.hold_s {
                1.0
            } else if t < self.attack_s + self.hold_s + self.decay_s {
                let x = (t - self.attack_s - self.hold_s) / self.decay_s;
                10f64.powf(self.sustain_db * x / 20.0)
            } else {
                10f64.powf(self.sustain_db / 20.0)
            }
        };
        if t < off {
            held(t)
        } else {
            held(off) * (1.0 - (t - off) / self.release_s).max(0.0)
        }
    }
}

/// Saw notes `(onset, note-off, hz)` under one ADSR, summed into a buffer
/// of `seconds`, each at a seeded random phase and scaled by `level_db`.
pub(crate) fn adsr_notes(
    notes: &[(f64, f64, f64, f64)],
    adsr: Adsr,
    rate: f64,
    seconds: f64,
) -> Vec<f64> {
    let length = samples(seconds, rate);
    let mut out = vec![0.0; length];
    let phases = uniform(31, notes.len());
    for (&(on, off, hz, level_db), phase) in notes.iter().zip(phases) {
        let start = samples(on, rate);
        let stop = samples(off + adsr.release_s, rate).min(length);
        let tone = saw(hz, rate, stop - start, phase);
        let level = 0.3 * 10f64.powf(level_db / 20.0);
        for (k, v) in tone.iter().enumerate() {
            let t = index(k) / rate;
            out[start + k] += level * v * adsr.gain(t, off - on);
        }
    }
    out
}

/// A saw whose harmonics pass a low-pass that sweeps, under a level that
/// falls: harmonic `h` has amplitude `gain(t) / h / sqrt(1 + (h f0 /
/// cutoff(t))^4)`, a two-pole response.
pub(crate) fn swept_saw(
    f0: f64,
    rate: f64,
    seconds: f64,
    gain: impl Fn(f64) -> f64,
    cutoff: impl Fn(f64) -> f64,
) -> Vec<f64> {
    let length = samples(seconds, rate);
    let top = (SAW_TOP.min(0.5 * rate) / f0).floor();
    (0..length)
        .map(|n| {
            let time = index(n) / rate;
            let edge = cutoff(time);
            let mut sum = 0.0;
            let mut h = 1.0;
            while h <= top {
                sum += (TAU * h * f0 * time).sin() / h / (1.0 + (h * f0 / edge).powi(4)).sqrt();
                h += 1.0;
            }
            0.3 * gain(time) * sum
        })
        .collect()
}
