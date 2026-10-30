//! `cargo xtask eval --layer envelope`: the envelope layer on generated
//! parts with known envelopes and pumps.
//!
//! Every part renders from its recipe, runs the envelope layer with the
//! true note list, and is scored against the recipe. The tables print the
//! attack ladder, the envelope grid, the knees, the level spread and the
//! pump grid with its bias per depth and recovery. `--json` writes one
//! record per part for a record-by-record comparison between builds.
//! Nothing here is committed.

use std::f64::consts::TAU;
use std::fmt::Write as _;
use std::path::Path;

use serde_json::{Value, json};
use sound_to_patch::audio::{BeatGrid, SampleRate};
use sound_to_patch::estimate::envelope::{
    EnvelopeInput, EnvelopeLayer, Pump, PumpLock, RMS_PERIODS, lock_name, rms_window,
};
use sound_to_patch::notes::{Note, NoteList};
use sound_to_patch::units::{Bpm, Hz, MidiPitch, Seconds};

use super::signals::splitmix;

/// The rate the set renders at.
const RATE: u32 = 44_100;

/// One generated part and what it should read.
struct Part {
    name: String,
    audio: Vec<f64>,
    notes: NoteList,
    bpm: f64,
    truth: Truth,
}

/// What a part should read.
enum Truth {
    /// One note with a linear attack.
    Attack { f0: f64, attack_s: f64 },
    /// A line of notes under one envelope, with the true note-offs.
    Envelope {
        adsr: Adsr,
        offs: Vec<f64>,
        db_linear: bool,
    },
    /// Plucks that decay at one rate: no knee.
    Pluck,
    /// Notes at known levels: the spread of their peaks.
    Spread { spread_db: f64 },
    /// A duck on every beat.
    Pump {
        depth_db: f64,
        release_s: f64,
        beat_s: f64,
        shape: Shape,
    },
    /// A level motion that is no pump, and the lock it should read.
    Lock { lock: PumpLock },
}

/// An ADSR: a linear attack, a hold, a decay to the sustain level and a
/// linear release.
#[derive(Clone, Copy, Debug)]
struct Adsr {
    attack: f64,
    hold: f64,
    decay: f64,
    sustain_db: f64,
    release: f64,
}

impl Adsr {
    /// The gain `t` seconds after the note-on for a note-off at `off`. The
    /// decay is linear in dB when `db_linear`, else linear in amplitude.
    fn gain(self, t: f64, off: f64, db_linear: bool) -> f64 {
        let s = 10f64.powf(self.sustain_db / 20.0);
        let held = |t: f64| {
            if t < 0.0 {
                0.0
            } else if t < self.attack {
                t / self.attack
            } else if t < self.attack + self.hold {
                1.0
            } else if t < self.attack + self.hold + self.decay {
                let x = (t - self.attack - self.hold) / self.decay;
                if db_linear {
                    10f64.powf(self.sustain_db * x / 20.0)
                } else {
                    1.0 - (1.0 - s) * x
                }
            } else {
                s
            }
        };
        if t < off {
            held(t)
        } else {
            held(off) * (1.0 - (t - off) / self.release).max(0.0)
        }
    }
}

fn index(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

fn samples(seconds: f64) -> usize {
    let n = (seconds * f64::from(RATE)).round().max(0.0);
    // At most a few minutes of audio, so the cast is exact.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "checked"
    )]
    let n = n as usize;
    n
}

/// SplitMix64 values in `[0, 1)`.
fn uniform(seed: u64, count: usize) -> Vec<f64> {
    splitmix(seed, count)
        .into_iter()
        .map(|z| f64::from(u32::try_from(z >> 32).unwrap_or(0)) / 4_294_967_296.0)
        .collect()
}

/// Seeded Gaussian noise by Box-Muller.
fn gaussian(seed: u64, count: usize) -> Vec<f64> {
    let u = uniform(seed, count + 1);
    u.windows(2)
        .map(|w| (-2.0 * (1.0 - w[0]).ln()).sqrt() * (TAU * w[1]).cos())
        .collect()
}

/// A band-limited saw: harmonic `h` at `1 / h` below 0.45 of the rate.
fn saw(f0: f64, length: usize, phase: f64) -> Vec<f64> {
    let rate = f64::from(RATE);
    let mut out = vec![0.0; length];
    let mut h = 1.0;
    while h * f0 < 0.45 * rate {
        let step = TAU * h * f0 / rate;
        let start = TAU * h * phase;
        for (n, x) in out.iter_mut().enumerate() {
            *x += (start + step * index(n)).sin() / h;
        }
        h += 1.0;
    }
    out
}

/// Saws at `pitches` summed at seeded phases, scaled to a peak of 0.5.
fn chord(pitches: &[f64], seconds: f64, seed: u64) -> Vec<f64> {
    let length = samples(seconds);
    let mut out = vec![0.0; length];
    for (&f0, phase) in pitches.iter().zip(uniform(seed, pitches.len())) {
        for (x, s) in out.iter_mut().zip(saw(f0, length, phase)) {
            *x += s;
        }
    }
    let peak = out.iter().fold(0.0_f64, |m, x| m.max(x.abs())).max(1e-12);
    out.iter().map(|x| 0.5 * x / peak).collect()
}

fn note_list(notes: &[(f64, f64, f64)]) -> NoteList {
    NoteList::new(
        notes
            .iter()
            .filter_map(|&(on, off, hz)| {
                let pitch = MidiPitch::new(69.0 + 12.0 * (hz / 440.0).log2()).ok()?;
                Note::new(Seconds::new(on).ok()?, Seconds::new(off).ok()?, pitch).ok()
            })
            .collect(),
    )
}

/// Saw notes `(onset, note-off, hz, level dB)` under one ADSR.
fn adsr_notes(
    notes: &[(f64, f64, f64, f64)],
    adsr: Adsr,
    db_linear: bool,
    seconds: f64,
) -> Vec<f64> {
    let rate = f64::from(RATE);
    let length = samples(seconds);
    let mut out = vec![0.0; length];
    for (&(on, off, hz, level_db), phase) in notes.iter().zip(uniform(31, notes.len())) {
        let start = samples(on);
        let stop = samples(off + adsr.release).min(length);
        let level = 0.3 * 10f64.powf(level_db / 20.0);
        for (k, v) in saw(hz, stop - start, phase).iter().enumerate() {
            out[start + k] += level * v * adsr.gain(index(k) / rate, off - on, db_linear);
        }
    }
    out
}

/// The transcriber's end of a note: its note-off, its release and 40 ms,
/// near the middle of the 25 to 60 ms that #70 measured on dry notes.
fn transcribed_end(off: f64, release: f64) -> f64 {
    off + release + 0.04
}

/// The parts of the attack ladder: one note per pitch and attack.
fn attack_parts() -> Vec<Part> {
    let mut parts = Vec::new();
    for f0 in [41.2, 55.0, 110.0, 220.0, 440.0] {
        for attack in [0.001, 0.005, 0.01, 0.02, 0.05, 0.1, 0.3] {
            let adsr = Adsr {
                attack,
                hold: 0.0,
                decay: 0.2,
                sustain_db: -6.0,
                release: 0.05,
            };
            let off = 0.2 + attack + 0.8;
            parts.push(Part {
                name: format!("attack {f0} Hz {:.0} ms", attack * 1e3),
                audio: adsr_notes(&[(0.2, off, f0, 0.0)], adsr, true, off + 0.5),
                notes: note_list(&[(0.2, transcribed_end(off, 0.05), f0)]),
                bpm: 120.0,
                truth: Truth::Attack {
                    f0,
                    attack_s: attack,
                },
            });
        }
    }
    parts
}

/// The envelope grid: eight notes of one line per envelope.
fn envelope_parts() -> Vec<Part> {
    let pitches = [110.0, 123.47, 130.81, 146.83];
    let mut parts = Vec::new();
    for db_linear in [true, false] {
        for decay in [0.05, 0.15, 0.3, 0.6] {
            for sustain_db in [0.0, -3.0, -6.0, -12.0, -24.0] {
                for hold in [0.0, 0.03] {
                    for release in [0.02, 0.05, 0.2] {
                        let adsr = Adsr {
                            attack: 0.01,
                            hold,
                            decay,
                            sustain_db,
                            release,
                        };
                        let length = 0.01 + hold + 3.5 * decay + 0.3;
                        let step = length + release + 0.3;
                        let notes: Vec<(f64, f64, f64, f64)> = (0..8)
                            .map(|i| {
                                let on = 0.2 + step * index(i);
                                (on, on + length, pitches[i % 4], 0.0)
                            })
                            .collect();
                        let end = 0.2 + step * 8.0 + 0.3;
                        parts.push(Part {
                            name: format!(
                                "env {} D{:.0} S{sustain_db} H{:.0} R{:.0}",
                                if db_linear { "dB" } else { "lin" },
                                decay * 1e3,
                                hold * 1e3,
                                release * 1e3
                            ),
                            audio: adsr_notes(&notes, adsr, db_linear, end),
                            notes: note_list(
                                &notes
                                    .iter()
                                    .map(|n| (n.0, transcribed_end(n.1, release), n.2))
                                    .collect::<Vec<_>>(),
                            ),
                            bpm: 120.0,
                            truth: Truth::Envelope {
                                adsr,
                                offs: notes.iter().map(|n| n.1).collect(),
                                db_linear,
                            },
                        });
                    }
                }
            }
        }
    }
    parts
}

/// Plucks that decay exponentially and are cut by the next note.
fn pluck_parts() -> Vec<Part> {
    let rate = f64::from(RATE);
    let mut parts = Vec::new();
    for tau in [0.15, 0.4, 1.0] {
        for f0 in [55.0, 220.0] {
            let every = 1.2;
            let seconds = 8.0 * every + 0.2;
            let tone = saw(f0, samples(seconds), 0.0);
            let audio = tone
                .iter()
                .enumerate()
                .map(|(n, v)| 0.3 * v * (-(index(n) / rate).rem_euclid(every) / tau).exp())
                .collect();
            let notes: Vec<(f64, f64, f64)> = (0..8)
                .map(|i| (every * index(i), every * index(i + 1), f0))
                .collect();
            parts.push(Part {
                name: format!("pluck {f0} Hz tau {:.0} ms", tau * 1e3),
                audio,
                notes: note_list(&notes),
                bpm: 120.0,
                truth: Truth::Pluck,
            });
        }
    }
    parts
}

/// Eight notes at levels that span a known range.
fn spread_parts() -> Vec<Part> {
    let adsr = Adsr {
        attack: 0.01,
        hold: 0.0,
        decay: 0.2,
        sustain_db: -6.0,
        release: 0.05,
    };
    [0.0, 3.0, 6.0, 12.0]
        .into_iter()
        .map(|spread| {
            let levels: Vec<f64> = (0..8).map(|i| -spread * index(i % 4) / 3.0).collect();
            let notes: Vec<(f64, f64, f64, f64)> = levels
                .iter()
                .enumerate()
                .map(|(i, &l)| (0.2 + 1.2 * index(i), 1.1 + 1.2 * index(i), 110.0, l))
                .collect();
            Part {
                name: format!("spread {spread} dB"),
                audio: adsr_notes(&notes, adsr, true, 10.0),
                notes: note_list(
                    &notes
                        .iter()
                        .map(|n| (n.0, transcribed_end(n.1, 0.05), n.2))
                        .collect::<Vec<_>>(),
                ),
                bpm: 120.0,
                // The 10th and 90th percentiles of four levels spread evenly
                // over the range, two notes each, fall on the lowest and the
                // highest.
                truth: Truth::Spread { spread_db: spread },
            }
        })
        .collect()
}

/// The shape of a duck on every beat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    /// Down to `-depth` at the beat, back as `1 - (1 - g0) exp(-t /
    /// release)` in linear gain: the model the duck fit assumes.
    Gain,
    /// Back as `-depth exp(-t / release)` in dB.
    Db,
    /// As `Gain`, after a linear fall over 5 ms.
    Attack,
    /// A half-cosine dip in dB, `release` long, as a drawn LFO curve.
    Cosine,
}

impl Shape {
    fn name(self) -> &'static str {
        match self {
            Self::Gain => "gain",
            Self::Db => "db",
            Self::Attack => "attack",
            Self::Cosine => "cosine",
        }
    }
}

/// The gain of a duck at `t`.
fn duck(t: f64, depth_db: f64, release: f64, beat: f64, shape: Shape) -> f64 {
    let g0 = 10f64.powf(-depth_db / 20.0);
    let t = t.rem_euclid(beat);
    match shape {
        Shape::Gain => 1.0 - (1.0 - g0) * (-t / release).exp(),
        Shape::Db => 10f64.powf(-depth_db * (-t / release).exp() / 20.0),
        Shape::Attack => {
            let fall = 0.005;
            if t < fall {
                1.0 - (1.0 - g0) * t / fall
            } else {
                1.0 - (1.0 - g0) * (-(t - fall) / release).exp()
            }
        }
        Shape::Cosine => {
            let x = (t / release).min(1.0);
            10f64.powf(-depth_db * 0.5 * (1.0 + (std::f64::consts::PI * x).cos()) / 20.0)
        }
    }
}

/// The time a duck takes from its deepest point back to within 1.5 dB,
/// on a 0.1 ms grid.
fn back_time(depth_db: f64, release: f64, beat: f64, shape: Shape) -> Option<f64> {
    let db = |t: f64| 20.0 * duck(t, depth_db, release, beat, shape).log10();
    let steps = (beat / 1e-4).floor();
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a beat of 1e-4 steps"
    )]
    let steps = steps as usize;
    let grid: Vec<f64> = (0..steps).map(|i| index(i) * 1e-4).collect();
    let low = grid
        .iter()
        .copied()
        .min_by(|a, b| db(*a).total_cmp(&db(*b)))?;
    grid.iter()
        .copied()
        .find(|&t| t > low && db(t) >= -1.5)
        .map(|t| t - low)
}

/// The pump grid: a held chord or a held sub under a duck, with noise,
/// and the four duck shapes on the noiseless chord at 128 BPM.
fn pump_parts() -> Vec<Part> {
    let rate = f64::from(RATE);
    let mut parts = Vec::new();
    for bpm in [90.0, 128.0, 174.0] {
        let beat = 60.0 / bpm;
        let seconds = 16.0 * beat;
        for (voice, pitches) in [("chord", vec![110.0, 130.81, 164.81]), ("sub", vec![55.0])] {
            for seed in [1, 2] {
                let x = chord(&pitches, seconds, seed);
                let notes = note_list(
                    &pitches
                        .iter()
                        .map(|&f| (0.0, seconds, f))
                        .collect::<Vec<_>>(),
                );
                for noise_db in [None, Some(-30.0), Some(-20.0)] {
                    let noise: Vec<f64> = noise_db.map_or_else(
                        || vec![0.0; x.len()],
                        |db| {
                            let std = 0.5 * 10f64.powf(db / 20.0);
                            gaussian(seed + 100, x.len())
                                .into_iter()
                                .map(|n| std * n)
                                .collect()
                        },
                    );
                    let all = (bpm - 128.0).abs() < 1e-9
                        && voice == "chord"
                        && seed == 1
                        && noise_db.is_none();
                    let shapes: &[Shape] = if all {
                        &[Shape::Gain, Shape::Db, Shape::Attack, Shape::Cosine]
                    } else {
                        &[Shape::Gain]
                    };
                    for &shape in shapes {
                        for depth in [0.0, 3.0, 6.0, 9.0, 12.0, 18.0] {
                            for release in [0.05, 0.08, 0.15, 0.3] {
                                if depth == 0.0 && (release > 0.05 || shape != Shape::Gain) {
                                    continue;
                                }
                                let audio = x
                                    .iter()
                                    .zip(&noise)
                                    .enumerate()
                                    .map(|(n, (v, e))| {
                                        v * duck(index(n) / rate, depth, release, beat, shape) + e
                                    })
                                    .collect();
                                parts.push(Part {
                                    name: format!(
                                        "pump {bpm} {voice} s{seed} n{} {} {depth}/{:.0}",
                                        noise_db.map_or("-".to_owned(), |d| format!("{d}")),
                                        shape.name(),
                                        release * 1e3
                                    ),
                                    audio,
                                    notes: notes.clone(),
                                    bpm,
                                    truth: Truth::Pump {
                                        depth_db: depth,
                                        release_s: release,
                                        beat_s: beat,
                                        shape,
                                    },
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    parts
}

/// Level motion that is no pump: plucks, pads and tremolos.
fn lock_parts() -> Vec<Part> {
    let rate = f64::from(RATE);
    let bpm = 128.0;
    let beat = 60.0 / bpm;
    let seconds = 16.0 * beat;
    let mut parts = Vec::new();
    for (name, every, tau) in [
        ("plucks on the beat", beat, 0.25),
        ("plucks on eighths", beat / 2.0, 0.25),
        ("plucks per bar", 4.0 * beat, 1.0),
    ] {
        let tone = saw(110.0, samples(seconds), 0.0);
        let audio = tone
            .iter()
            .enumerate()
            .map(|(n, v)| 0.3 * v * (-(index(n) / rate).rem_euclid(every) / tau).exp())
            .collect();
        let mut notes = Vec::new();
        let mut on = 0.0;
        while on + every <= seconds + 1e-9 {
            notes.push((on, on + every, 110.0));
            on += every;
        }
        parts.push(Part {
            name: name.to_owned(),
            audio,
            notes: note_list(&notes),
            bpm,
            truth: Truth::Lock {
                lock: PumpLock::None,
            },
        });
    }
    // A pad: one saw per bar with a 15 ms release into the next.
    let bar = 4.0 * beat;
    let pad_pitches = [110.0, 98.0, 87.31, 98.0];
    let mut audio = vec![0.0; samples(seconds)];
    for (b, &f0) in pad_pitches.iter().enumerate() {
        let tone = saw(f0, audio.len(), 0.0);
        for (i, v) in audio.iter_mut().enumerate() {
            let t = index(i) / rate - index(b) * bar;
            let g = if t < 0.0 {
                0.0
            } else if t < bar {
                1.0
            } else {
                (1.0 - (t - bar) / 0.015).max(0.0)
            };
            *v += 0.3 * g * tone[i];
        }
    }
    parts.push(Part {
        name: "pad per bar".to_owned(),
        audio,
        notes: note_list(
            &pad_pitches
                .iter()
                .enumerate()
                .map(|(b, &f)| (index(b) * bar, index(b + 1) * bar, f))
                .collect::<Vec<_>>(),
        ),
        bpm,
        truth: Truth::Lock {
            lock: PumpLock::None,
        },
    });
    let x = chord(&[110.0, 130.81, 164.81], seconds, 1);
    let notes = note_list(&[
        (0.0, seconds, 110.0),
        (0.0, seconds, 130.81),
        (0.0, seconds, 164.81),
    ]);
    for (rate_hz, swing, lock) in [
        (3.1, 3.0, PumpLock::Free),
        (5.3, 1.5, PumpLock::Free),
        (5.3, 3.0, PumpLock::Free),
        (7.7, 6.0, PumpLock::Free),
        (2.0 * bpm / 60.0, 3.0, PumpLock::None),
        (4.0 * bpm / 60.0, 3.0, PumpLock::None),
    ] {
        let audio = x
            .iter()
            .enumerate()
            .map(|(n, v)| v * 10f64.powf(swing * (TAU * rate_hz * index(n) / rate).sin() / 20.0))
            .collect();
        parts.push(Part {
            name: format!("tremolo {rate_hz:.2} Hz {swing} dB"),
            audio,
            notes: notes.clone(),
            bpm,
            truth: Truth::Lock { lock },
        });
    }
    parts
}

fn grid(bpm: f64) -> BeatGrid {
    BeatGrid::new(Bpm::clamped(bpm), Seconds::clamped(0.0))
}

/// Runs the set and prints its tables; with `json`, writes one record per
/// part there.
pub(crate) fn run(only: Option<&str>, json: Option<&Path>) -> crate::Result<()> {
    let rate = SampleRate::new(RATE)?;
    let parts: Vec<Part> = [
        attack_parts(),
        envelope_parts(),
        pluck_parts(),
        spread_parts(),
        pump_parts(),
        lock_parts(),
    ]
    .into_iter()
    .flatten()
    .filter(|p| only.is_none_or(|text| p.name.contains(text)))
    .collect();
    if parts.is_empty() {
        return Err(format!("no part name holds {:?}", only.unwrap_or_default()).into());
    }
    let mut records = Vec::new();
    let mut tally = Tally::default();
    let mut out = String::new();
    for part in &parts {
        let layer = EnvelopeLayer::of(&EnvelopeInput {
            mono: &part.audio,
            rate,
            notes: &part.notes,
            grid: grid(part.bpm),
        })?;
        let record = score(part, &layer, layer.pump.as_ref(), &mut tally);
        writeln!(out, "{}", line(&record))?;
        records.push(record);
    }
    print!("{out}");
    tally.print();
    if let Some(path) = json {
        let text = records.iter().fold(String::new(), |mut text, r| {
            let _ = writeln!(text, "{r}");
            text
        });
        std::fs::write(path, text)?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

/// Pass counts against the acceptance of #69 and #71.
#[derive(Default)]
struct Tally {
    attack: (usize, usize),
    bound: (usize, usize),
    decay: (usize, usize),
    sustain: (usize, usize),
    knee: (usize, usize),
    pluck: (usize, usize),
    spread: (usize, usize),
    pump_depth: (usize, usize),
    pump_back: (usize, usize),
    lock: (usize, usize),
    /// Measured minus true depth, per shape, true depth and recovery.
    bias: std::collections::BTreeMap<(&'static str, i64, i64), Vec<f64>>,
}

fn tick(pair: &mut (usize, usize), pass: bool) {
    pair.1 += 1;
    if pass {
        pair.0 += 1;
    }
}

impl Tally {
    fn print(&self) {
        println!("\nAcceptance (passed / cases)");
        for (name, (p, n)) in [
            ("attack 10-90 % within 15 % above 4 periods", self.attack),
            ("upper bound under 1.5 periods, none above 4", self.bound),
            ("pooled decay within 20 %", self.decay),
            ("pooled sustain within 2 dB", self.sustain),
            ("knee within 15 ms of the note-off", self.knee),
            ("pluck has no knee", self.pluck),
            ("level spread within 1 dB", self.spread),
            ("pump depth within 5 dB below the truth", self.pump_depth),
            ("pump recovery within 30 % of the truth", self.pump_back),
            ("lock as expected", self.lock),
        ] {
            println!("{name:<48} {p:>5} / {n}");
        }
        for (title, map) in [("measured", &self.bias)] {
            println!(
                "\nPump depth bias, {title} minus true dB: median (min to max) per shape, depth and recovery"
            );
            for ((shape, depth, release), values) in map {
                let mut v = values.clone();
                v.sort_by(f64::total_cmp);
                let median = v[v.len() / 2];
                println!(
                    "{shape:<6} {depth:>3} dB {release:>4} ms: {median:>6.2} ({:.2} to {:.2}), {} parts",
                    v[0],
                    v[v.len() - 1],
                    v.len()
                );
            }
        }
    }
}

fn seconds(v: Option<Seconds>) -> Value {
    v.map_or(Value::Null, |s| json!(s.get()))
}

/// Scores one part and returns its record.
#[expect(clippy::too_many_lines, reason = "one arm per kind of part")]
fn score(part: &Part, layer: &EnvelopeLayer, pump: Option<&Pump>, tally: &mut Tally) -> Value {
    let mut record = json!({
        "name": part.name,
        "attack_s": layer.attack.value().map(|s| s.median().get()),
        "decay_s": layer.decay.value().map(|s| s.median().get()),
        "sustain_db": layer.sustain.value().map(|s| s.median().get()),
        "hold_s": layer.hold.value().map(|s| s.median().get()),
        "upper_bound": layer.flags.attack_is_upper_bound,
        "knees": layer.notes.iter().map(|n| seconds(n.knee)).collect::<Vec<_>>(),
        "level_spread_db": layer.level_spread.value().map(|d| d.get()),
        "pump_depth_db": pump.map(|p| p.depth.get()),
        "pump_back_s": pump.and_then(|p| p.back).map(Seconds::get),
        "pump_lock": pump.map(|p| lock_name(p.lock)),
        "residual_db": layer.residual.map(sound_to_patch::units::Decibels::get),
    });
    match &part.truth {
        Truth::Attack { f0, attack_s } => {
            let note = layer.notes.first();
            let rise = note.and_then(|n| n.attack).map(|a| a.get() / 1.25);
            let periods = attack_s * f0;
            let bound = note.is_some_and(|n| n.attack_upper_bound);
            if periods > 4.0 {
                let pass = rise.is_some_and(|r| (r / (0.8 * attack_s) - 1.0).abs() <= 0.15);
                tick(&mut tally.attack, pass);
                tick(&mut tally.bound, !bound);
            } else if periods < 1.5 {
                tick(&mut tally.bound, bound);
            }
            record["truth"] = json!({"rise_s": 0.8 * attack_s, "periods": periods,
                "window_s": rms_window(Hz::clamped(*f0), RMS_PERIODS).get()});
            record["rise_s"] = json!(rise);
            record["bound"] = json!(bound);
        }
        Truth::Envelope {
            adsr,
            offs,
            db_linear,
        } => {
            if adsr.sustain_db < -1.0 {
                let decay = layer.decay.value().map(|s| s.median().get());
                tick(
                    &mut tally.decay,
                    decay.is_some_and(|d| (d / adsr.decay - 1.0).abs() <= 0.2),
                );
            }
            let sustain = layer.sustain.value().map(|s| s.median().get());
            tick(
                &mut tally.sustain,
                sustain.is_some_and(|s| (s - adsr.sustain_db).abs() <= 2.0),
            );
            for (note, off) in layer.notes.iter().zip(offs) {
                tick(
                    &mut tally.knee,
                    note.knee.is_some_and(|k| (k.get() - off).abs() <= 0.015),
                );
            }
            record["truth"] = json!({"decay_s": adsr.decay, "sustain_db": adsr.sustain_db,
                "hold_s": adsr.hold, "release_s": adsr.release, "offs": offs, "db_linear": db_linear});
        }
        Truth::Pluck => {
            for note in &layer.notes {
                tick(&mut tally.pluck, note.knee.is_none());
            }
        }
        Truth::Spread { spread_db } => {
            let read = layer.level_spread.value().map(|d| d.get());
            tick(
                &mut tally.spread,
                read.is_some_and(|r| (r - spread_db).abs() <= 1.0),
            );
            record["truth"] = json!({"spread_db": spread_db});
        }
        Truth::Pump {
            depth_db,
            release_s,
            beat_s,
            shape,
            ..
        } => {
            let depth = pump.map(|p| p.depth.get());
            let back = pump.and_then(|p| p.back).map(Seconds::get);
            let expected_back = back_time(*depth_db, *release_s, *beat_s, *shape);
            if *depth_db >= 6.0 && *shape == Shape::Gain {
                tick(
                    &mut tally.pump_depth,
                    depth.is_some_and(|d| d <= depth_db + 0.5 && d >= depth_db - 5.0),
                );
                let right = match (back, expected_back) {
                    (Some(b), Some(e)) => (b / e - 1.0).abs() <= 0.3,
                    (None, None) => pump.is_some(),
                    _ => false,
                };
                tick(&mut tally.pump_back, right);
            }
            let expected = if *depth_db >= 6.0 {
                PumpLock::Beat
            } else if *depth_db == 0.0 {
                PumpLock::None
            } else {
                pump.map_or(PumpLock::None, |p| p.lock)
            };
            tick(&mut tally.lock, pump.is_some_and(|p| p.lock == expected));
            if *depth_db > 0.0 {
                // Whole dB and ms keys keep the map ordered.
                #[expect(clippy::cast_possible_truncation, reason = "small grid values")]
                let key = (
                    shape.name(),
                    depth_db.round() as i64,
                    (release_s * 1e3).round() as i64,
                );
                if let Some(d) = depth {
                    tally.bias.entry(key).or_default().push(d - depth_db);
                }
            }
            record["truth"] = json!({"depth_db": depth_db, "release_s": release_s,
                "back_s": expected_back, "shape": shape.name()});
        }
        Truth::Lock { lock } => {
            let read = pump.map_or(PumpLock::None, |p| p.lock);
            tick(&mut tally.lock, read == *lock);
            record["truth"] = json!({"lock": lock_name(*lock)});
        }
    }
    record
}

/// One printed row of a record.
fn line(record: &Value) -> String {
    let num = |key: &str, scale: f64| {
        record[key]
            .as_f64()
            .map_or("-".to_owned(), |v| format!("{:.1}", v * scale))
    };
    format!(
        "{:<36} att {:>6} dec {:>6} sus {:>6} hold {:>5} bound {:<5} spread {:>5} pump {:>5} back {:>5} {:<5} knees {}",
        record["name"].as_str().unwrap_or(""),
        num("attack_s", 1e3),
        num("decay_s", 1e3),
        num("sustain_db", 1.0),
        num("hold_s", 1e3),
        record["upper_bound"],
        num("level_spread_db", 1.0),
        num("pump_depth_db", 1.0),
        num("pump_back_s", 1e3),
        record["pump_lock"].as_str().unwrap_or("-"),
        record["knees"]
            .as_array()
            .map_or(0, |k| k.iter().filter(|v| !v.is_null()).count()),
    )
}
