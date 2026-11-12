//! `cargo xtask eval --layer envelope`: the envelope layer on generated
//! parts with known envelopes and pumps.
//!
//! Every part renders from its recipe, runs the envelope layer with the
//! true note list, and is scored against the recipe. A part renders only
//! when its name is selected, and is dropped once scored. The tables print the
//! attack ladder, the envelope grid, the knees, the level spread and the
//! pump grid with its bias per depth and recovery. `--json` writes one
//! record per part for a record-by-record comparison between builds.
//! Nothing here is committed.

use std::collections::HashMap;
use std::f64::consts::TAU;
use std::fmt::Write as _;
use std::path::Path;

use serde_json::{Value, json};
use sound_to_patch::audio::{BeatGrid, SampleRate};
use sound_to_patch::estimate::envelope::{
    EnvelopeInput, EnvelopeLayer, PUMP_BACK, Pump, PumpLock, RAMP_FACTOR, RMS_PERIODS, lock_name,
    rms_window,
};
use sound_to_patch::estimate::voices::{VoicesInput, VoicesLayer};
use sound_to_patch::notes::{Note, NoteList};
use sound_to_patch::units::{Bpm, Hz, MidiPitch, Seconds};

use super::signals::{convolve, splitmix};

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
    /// A line of notes under one envelope, with the true onsets and
    /// note-offs.
    Envelope {
        adsr: Adsr,
        notes: Vec<(f64, f64)>,
        db_linear: bool,
    },
    /// A line whose knees should read the note-offs, with the true onsets
    /// and its tally row.
    Knees {
        notes: Vec<(f64, f64)>,
        row: KneeRow,
    },
    /// Plucks at these onsets that decay at one rate: no knee.
    Pluck { onsets: Vec<f64> },
    /// A line with a linear release, and a noise reverb when `rt60_s` is above 0.
    Tail { release_s: f64, rt60_s: f64 },
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

/// The tally rows of [`Truth::Knees`].
#[derive(Clone, Copy)]
enum KneeRow {
    ShortSustain,
    Tremolo,
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

/// Samples `from..to` of a band-limited saw: harmonic `h` at `1 / h`
/// below 0.45 of the rate. Each sample sums its harmonics in the same
/// order, so a saw rendered in pieces equals one rendered at once.
fn saw_range(f0: f64, phase: f64, from: usize, to: usize) -> Vec<f64> {
    let rate = f64::from(RATE);
    let mut out = vec![0.0; to.saturating_sub(from)];
    let mut h = 1.0;
    while h * f0 < 0.45 * rate {
        let step = TAU * h * f0 / rate;
        let start = TAU * h * phase;
        for (n, x) in (from..).zip(out.iter_mut()) {
            *x += (start + step * index(n)).sin() / h;
        }
        h += 1.0;
    }
    out
}

/// The saws of a run by pitch and phase. The parts of a grid repeat the
/// same notes at other lengths, so each saw renders once, as long as the
/// longest part needs it.
#[derive(Default)]
struct Saws {
    cache: HashMap<(u64, u64), Vec<f64>>,
    /// The samples rendered so far.
    rendered: usize,
}

impl Saws {
    /// The first `length` samples of the saw at `f0` and `phase` cycles.
    fn get(&mut self, f0: f64, length: usize, phase: f64) -> &[f64] {
        let saw = self
            .cache
            .entry((f0.to_bits(), phase.to_bits()))
            .or_default();
        if saw.len() < length {
            let from = saw.len();
            saw.extend(saw_range(f0, phase, from, length));
            self.rendered += length - from;
        }
        &saw[..length]
    }
}

/// Saws at `pitches` summed at seeded phases, scaled to a peak of 0.5.
fn chord(saws: &mut Saws, pitches: &[f64], seconds: f64, seed: u64) -> Vec<f64> {
    let length = samples(seconds);
    let mut out = vec![0.0; length];
    for (&f0, phase) in pitches.iter().zip(uniform(seed, pitches.len())) {
        for (x, s) in out.iter_mut().zip(saws.get(f0, length, phase)) {
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
    saws: &mut Saws,
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
        for (k, v) in saws.get(hz, stop - start, phase).iter().enumerate() {
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

/// Takes the parts of a run: renders only the parts whose name holds
/// `only`, and hands each to `take` as soon as it is rendered, so a run
/// holds one part at a time.
struct Sink<'a> {
    only: Option<&'a str>,
    saws: Saws,
    taken: usize,
    take: &'a mut dyn FnMut(Part) -> crate::Result<()>,
}

impl Sink<'_> {
    fn wants(&self, name: &str) -> bool {
        self.only.is_none_or(|text| name.contains(text))
    }

    fn put(&mut self, part: Part) -> crate::Result<()> {
        self.taken += 1;
        (self.take)(part)
    }
}

/// Renders every selected part of the set, in report order.
fn all_parts(sink: &mut Sink) -> crate::Result<()> {
    attack_parts(sink)?;
    envelope_parts(sink)?;
    short_sustain_parts(sink)?;
    tremolo_parts(sink)?;
    pluck_parts(sink)?;
    tail_parts(sink)?;
    spread_parts(sink)?;
    pump_parts(sink)?;
    lock_parts(sink)
}

/// The parts of the attack ladder: one note per pitch and attack.
fn attack_parts(sink: &mut Sink) -> crate::Result<()> {
    for f0 in [41.2, 55.0, 110.0, 220.0, 440.0] {
        for attack in [0.001, 0.005, 0.01, 0.02, 0.05, 0.1, 0.3] {
            let name = format!("attack {f0} Hz {:.0} ms", attack * 1e3);
            if !sink.wants(&name) {
                continue;
            }
            let adsr = Adsr {
                attack,
                hold: 0.0,
                decay: 0.2,
                sustain_db: -6.0,
                release: 0.05,
            };
            let off = 0.2 + attack + 0.8;
            let audio = adsr_notes(
                &mut sink.saws,
                &[(0.2, off, f0, 0.0)],
                adsr,
                true,
                off + 0.5,
            );
            sink.put(Part {
                name,
                audio,
                notes: note_list(&[(0.2, transcribed_end(off, 0.05), f0)]),
                bpm: 120.0,
                truth: Truth::Attack {
                    f0,
                    attack_s: attack,
                },
            })?;
        }
    }
    Ok(())
}

/// The envelope grid: eight notes of one line per envelope.
fn envelope_parts(sink: &mut Sink) -> crate::Result<()> {
    let pitches = [110.0, 123.47, 130.81, 146.83];
    for db_linear in [true, false] {
        for decay in [0.05, 0.15, 0.3, 0.6] {
            for sustain_db in [0.0, -3.0, -6.0, -12.0, -24.0] {
                for hold in [0.0, 0.03] {
                    for release in [0.02, 0.05, 0.2] {
                        let name = format!(
                            "env {} D{:.0} S{sustain_db} H{:.0} R{:.0}",
                            if db_linear { "dB" } else { "lin" },
                            decay * 1e3,
                            hold * 1e3,
                            release * 1e3
                        );
                        if !sink.wants(&name) {
                            continue;
                        }
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
                        let audio = adsr_notes(&mut sink.saws, &notes, adsr, db_linear, end);
                        sink.put(Part {
                            name,
                            audio,
                            notes: note_list(
                                &notes
                                    .iter()
                                    .map(|n| (n.0, transcribed_end(n.1, release), n.2))
                                    .collect::<Vec<_>>(),
                            ),
                            bpm: 120.0,
                            truth: Truth::Envelope {
                                adsr,
                                notes: notes.iter().map(|n| (n.0, n.1)).collect(),
                                db_linear,
                            },
                        })?;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Four notes with a sustain shorter than their decay, each listed up to the next.
fn short_sustain_parts(sink: &mut Sink) -> crate::Result<()> {
    for decay in [0.2, 0.4] {
        for gap in [0.02, 0.05, 0.1] {
            for sustain_db in [-6.0, -12.0, -24.0] {
                for release in [0.05, 0.2] {
                    let name = format!(
                        "short D{:.0} G{:.0} S{sustain_db} R{:.0}",
                        decay * 1e3,
                        gap * 1e3,
                        release * 1e3
                    );
                    if !sink.wants(&name) {
                        continue;
                    }
                    let adsr = Adsr {
                        attack: 0.005,
                        hold: 0.1,
                        decay,
                        sustain_db,
                        release,
                    };
                    let length = 0.105 + decay + gap;
                    let step = length + release + 0.4;
                    let on = |i: usize| 0.1 + step * index(i);
                    let notes: Vec<(f64, f64, f64, f64)> = (0..4)
                        .map(|i| (on(i), on(i) + length, 220.0, 0.0))
                        .collect();
                    let list: Vec<(f64, f64, f64)> =
                        (0..4).map(|i| (on(i), on(i + 1) - 0.002, 220.0)).collect();
                    let audio = adsr_notes(&mut sink.saws, &notes, adsr, true, on(4));
                    sink.put(Part {
                        name,
                        audio,
                        notes: note_list(&list),
                        bpm: 120.0,
                        truth: Truth::Knees {
                            notes: notes.iter().map(|n| (n.0, n.1)).collect(),
                            row: KneeRow::ShortSustain,
                        },
                    })?;
                }
            }
        }
    }
    Ok(())
}

/// Four dry notes under a tremolo in dB and a release of 200 ms or more.
fn tremolo_parts(sink: &mut Sink) -> crate::Result<()> {
    let rate = f64::from(RATE);
    for hz in [2.0, 4.0, 8.0, 12.0] {
        for swing_db in [0.5, 1.0] {
            for release in [0.2, 0.4] {
                let name = format!("tremolo {hz} Hz {swing_db} dB R{:.0}", release * 1e3);
                if !sink.wants(&name) {
                    continue;
                }
                let adsr = Adsr {
                    attack: 0.005,
                    hold: 0.0,
                    decay: 0.01,
                    sustain_db: 0.0,
                    release,
                };
                let step = 1.2 + release + 0.4;
                let on = |i: usize| 0.1 + step * index(i);
                let notes: Vec<(f64, f64, f64, f64)> =
                    (0..4).map(|i| (on(i), on(i) + 1.2, 220.0, 0.0)).collect();
                let list: Vec<(f64, f64, f64)> =
                    (0..4).map(|i| (on(i), on(i + 1) - 0.002, 220.0)).collect();
                let audio = adsr_notes(&mut sink.saws, &notes, adsr, true, on(4))
                    .iter()
                    .enumerate()
                    .map(|(n, v)| {
                        let t = index(n) / rate;
                        v * 10f64.powf(swing_db * (TAU * hz * t).sin() / 20.0)
                    })
                    .collect();
                sink.put(Part {
                    name,
                    audio,
                    notes: note_list(&list),
                    bpm: 120.0,
                    truth: Truth::Knees {
                        notes: notes.iter().map(|n| (n.0, n.1)).collect(),
                        row: KneeRow::Tremolo,
                    },
                })?;
            }
        }
    }
    Ok(())
}

/// Plucks that decay exponentially and are cut by the next note.
fn pluck_parts(sink: &mut Sink) -> crate::Result<()> {
    let rate = f64::from(RATE);
    for tau in [0.15, 0.4, 1.0] {
        for f0 in [55.0, 220.0] {
            let name = format!("pluck {f0} Hz tau {:.0} ms", tau * 1e3);
            if !sink.wants(&name) {
                continue;
            }
            let every = 1.2;
            let seconds = 8.0 * every + 0.2;
            let audio = sink
                .saws
                .get(f0, samples(seconds), 0.0)
                .iter()
                .enumerate()
                .map(|(n, v)| 0.3 * v * (-(index(n) / rate).rem_euclid(every) / tau).exp())
                .collect();
            let notes: Vec<(f64, f64, f64)> = (0..8)
                .map(|i| (every * index(i), every * index(i + 1), f0))
                .collect();
            sink.put(Part {
                name,
                audio,
                notes: note_list(&notes),
                bpm: 120.0,
                truth: Truth::Pluck {
                    onsets: notes.iter().map(|n| n.0).collect(),
                },
            })?;
        }
    }
    Ok(())
}

/// Six notes under a release, each listed up to the next, dry or through a
/// noise reverb of unit energy times `wet`.
fn tail_parts(sink: &mut Sink) -> crate::Result<()> {
    let rate = f64::from(RATE);
    let pitches = [110.0, 130.81, 146.83, 164.81, 196.0, 123.47];
    for release_s in [0.02, 0.05, 0.2, 0.4] {
        for (rt60_s, wet) in [
            (0.0, 0.0),
            (0.6, 0.15),
            (1.5, 0.15),
            (3.0, 0.15),
            (0.6, 0.4),
            (1.5, 0.4),
            (3.0, 0.4),
        ] {
            let name = format!("tail R{:.0} T{rt60_s} W{wet}", release_s * 1e3);
            if !sink.wants(&name) {
                continue;
            }
            let adsr = Adsr {
                attack: 0.01,
                hold: 0.0,
                decay: 0.01,
                sustain_db: 0.0,
                release: release_s,
            };
            let step = 0.6 + release_s + f64::max(1.1 * rt60_s, 0.2);
            let on = |i: usize| 0.1 + step * index(i);
            let notes: Vec<(f64, f64, f64, f64)> = pitches
                .iter()
                .enumerate()
                .map(|(i, &f)| (on(i), on(i) + 0.6, f, 0.0))
                .collect();
            let dry = adsr_notes(&mut sink.saws, &notes, adsr, true, on(6));
            let audio = if rt60_s > 0.0 {
                let shaped: Vec<f64> = gaussian(70_000, samples(1.1 * rt60_s))
                    .iter()
                    .enumerate()
                    .map(|(n, v)| v * 10f64.powf(-3.0 * index(n) / rate / rt60_s))
                    .collect();
                let energy = shaped.iter().map(|v| v * v).sum::<f64>().sqrt();
                let ir: Vec<f64> = shaped.iter().map(|v| v * wet / energy).collect();
                dry.iter()
                    .zip(convolve(&dry, &ir))
                    .map(|(a, b)| a + b)
                    .collect()
            } else {
                dry
            };
            let list: Vec<(f64, f64, f64)> = pitches
                .iter()
                .enumerate()
                .map(|(i, &f)| (on(i), on(i + 1) - 0.002, f))
                .collect();
            sink.put(Part {
                name,
                audio,
                notes: note_list(&list),
                bpm: 120.0,
                truth: Truth::Tail { release_s, rt60_s },
            })?;
        }
    }
    Ok(())
}

/// Eight notes at levels that span a known range.
fn spread_parts(sink: &mut Sink) -> crate::Result<()> {
    let adsr = Adsr {
        attack: 0.01,
        hold: 0.0,
        decay: 0.2,
        sustain_db: -6.0,
        release: 0.05,
    };
    for spread in [0.0, 3.0, 6.0, 12.0] {
        let name = format!("spread {spread} dB");
        if !sink.wants(&name) {
            continue;
        }
        let levels: Vec<f64> = (0..8).map(|i| -spread * index(i % 4) / 3.0).collect();
        let notes: Vec<(f64, f64, f64, f64)> = levels
            .iter()
            .enumerate()
            .map(|(i, &l)| (0.2 + 1.2 * index(i), 1.1 + 1.2 * index(i), 110.0, l))
            .collect();
        let audio = adsr_notes(&mut sink.saws, &notes, adsr, true, 10.0);
        sink.put(Part {
            name,
            audio,
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
        })?;
    }
    Ok(())
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

/// The time a duck takes from its deepest point back to within
/// [`PUMP_BACK`], on a 0.1 ms grid.
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
        .find(|&t| t > low && db(t) >= -PUMP_BACK.get())
        .map(|t| t - low)
}

/// The pump grid: a held chord or a held sub under a duck, with noise,
/// and the four duck shapes on the noiseless chord at 128 BPM.
fn pump_parts(sink: &mut Sink) -> crate::Result<()> {
    let rate = f64::from(RATE);
    for bpm in [90.0, 128.0, 174.0] {
        let beat = 60.0 / bpm;
        let seconds = 16.0 * beat;
        for (voice, pitches) in [("chord", vec![110.0, 130.81, 164.81]), ("sub", vec![55.0])] {
            for seed in [1, 2] {
                // Rendered on the first selected part that needs them.
                let mut chord_audio: Option<Vec<f64>> = None;
                let notes = note_list(
                    &pitches
                        .iter()
                        .map(|&f| (0.0, seconds, f))
                        .collect::<Vec<_>>(),
                );
                for noise_db in [None, Some(-30.0), Some(-20.0)] {
                    let mut noise: Option<Vec<f64>> = None;
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
                                let name = format!(
                                    "pump {bpm} {voice} s{seed} n{} {} {depth}/{:.0}",
                                    noise_db.map_or("-".to_owned(), |d| format!("{d}")),
                                    shape.name(),
                                    release * 1e3
                                );
                                if !sink.wants(&name) {
                                    continue;
                                }
                                let x = chord_audio.get_or_insert_with(|| {
                                    chord(&mut sink.saws, &pitches, seconds, seed)
                                });
                                let noise = noise.get_or_insert_with(|| {
                                    noise_db.map_or_else(
                                        || vec![0.0; x.len()],
                                        |db| {
                                            let std = 0.5 * 10f64.powf(db / 20.0);
                                            gaussian(seed + 100, x.len())
                                                .into_iter()
                                                .map(|n| std * n)
                                                .collect()
                                        },
                                    )
                                });
                                let audio = x
                                    .iter()
                                    .zip(noise.iter())
                                    .enumerate()
                                    .map(|(n, (v, e))| {
                                        v * duck(index(n) / rate, depth, release, beat, shape) + e
                                    })
                                    .collect();
                                sink.put(Part {
                                    name,
                                    audio,
                                    notes: notes.clone(),
                                    bpm,
                                    truth: Truth::Pump {
                                        depth_db: depth,
                                        release_s: release,
                                        beat_s: beat,
                                        shape,
                                    },
                                })?;
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// A pad: one saw per bar with a 15 ms release into the next.
fn pad_part(sink: &mut Sink) -> crate::Result<()> {
    let rate = f64::from(RATE);
    let bpm = 128.0;
    let seconds = 16.0 * 60.0 / bpm;
    let bar = 4.0 * 60.0 / bpm;
    let pad_pitches = [110.0, 98.0, 87.31, 98.0];
    if !sink.wants("pad per bar") {
        return Ok(());
    }
    let mut audio = vec![0.0; samples(seconds)];
    for (b, &f0) in pad_pitches.iter().enumerate() {
        let tone = sink.saws.get(f0, audio.len(), 0.0);
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
    sink.put(Part {
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
    })
}

/// Level motion that is no pump: plucks, pads and tremolos.
fn lock_parts(sink: &mut Sink) -> crate::Result<()> {
    let rate = f64::from(RATE);
    let bpm = 128.0;
    let beat = 60.0 / bpm;
    let seconds = 16.0 * beat;
    for (name, every, tau) in [
        ("plucks on the beat", beat, 0.25),
        ("plucks on eighths", beat / 2.0, 0.25),
        ("plucks per bar", 4.0 * beat, 1.0),
    ] {
        if !sink.wants(name) {
            continue;
        }
        let audio = sink
            .saws
            .get(110.0, samples(seconds), 0.0)
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
        sink.put(Part {
            name: name.to_owned(),
            audio,
            notes: note_list(&notes),
            bpm,
            truth: Truth::Lock {
                lock: PumpLock::None,
            },
        })?;
    }
    pad_part(sink)?;
    let notes = note_list(&[
        (0.0, seconds, 110.0),
        (0.0, seconds, 130.81),
        (0.0, seconds, 164.81),
    ]);
    let mut chord_audio: Option<Vec<f64>> = None;
    for (rate_hz, swing, lock) in [
        (3.1, 3.0, PumpLock::Free),
        (5.3, 1.5, PumpLock::Free),
        (5.3, 3.0, PumpLock::Free),
        (7.7, 6.0, PumpLock::Free),
        (2.0 * bpm / 60.0, 3.0, PumpLock::None),
        (4.0 * bpm / 60.0, 3.0, PumpLock::None),
    ] {
        let name = format!("tremolo {rate_hz:.2} Hz {swing} dB");
        if !sink.wants(&name) {
            continue;
        }
        let x = chord_audio
            .get_or_insert_with(|| chord(&mut sink.saws, &[110.0, 130.81, 164.81], seconds, 1));
        let audio = x
            .iter()
            .enumerate()
            .map(|(n, v)| v * 10f64.powf(swing * (TAU * rate_hz * index(n) / rate).sin() / 20.0))
            .collect();
        sink.put(Part {
            name,
            audio,
            notes: notes.clone(),
            bpm,
            truth: Truth::Lock { lock },
        })?;
    }
    Ok(())
}

fn grid(bpm: f64) -> BeatGrid {
    BeatGrid::new(Bpm::clamped(bpm), Seconds::clamped(0.0))
}

/// Runs the set and prints its tables; with `json`, writes one record per
/// part there.
pub(crate) fn run(only: Option<&str>, json: Option<&Path>) -> crate::Result<()> {
    let rate = SampleRate::new(RATE)?;
    let mut records = Vec::new();
    let mut tally = Tally::default();
    let mut take = |part: Part| -> crate::Result<()> {
        let input = EnvelopeInput {
            stereo: None,
            mono: &part.audio,
            rate,
            notes: &part.notes,
            grid: grid(part.bpm),
        };
        let mut layer = EnvelopeLayer::of(&input)?;
        // As `analyze` reads them: the full-band levels take the voicing of
        // the voices layer, which reads the envelope first.
        let voices = VoicesLayer::of(&VoicesInput {
            mono: &part.audio,
            rate,
            notes: &part.notes,
            envelope: &layer,
        })?;
        layer.read_levels(&input, voices.mono)?;
        let record = score(&part, &layer, layer.pump.as_ref(), &mut tally);
        println!("{}", line(&record));
        records.push(record);
        Ok(())
    };
    let mut sink = Sink {
        only,
        saws: Saws::default(),
        taken: 0,
        take: &mut take,
    };
    all_parts(&mut sink)?;
    if sink.taken == 0 {
        return Err(format!("no part name holds {:?}", only.unwrap_or_default()).into());
    }
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
    knee_short: (usize, usize),
    knee_tremolo: (usize, usize),
    pluck: (usize, usize),
    spread: (usize, usize),
    pump_depth: (usize, usize),
    pump_back: (usize, usize),
    lock: (usize, usize),
    release: (usize, usize),
    release_read: (usize, usize),
    rt60: (usize, usize),
    rt60_read: (usize, usize),
    dry_rt60: (usize, usize),
    /// Read minus true sustain over the cases of the sustain row.
    sustain_error: Reads,
    /// Read minus true depth over the cases of the depth row.
    depth_error: Reads,
    /// Measured minus true depth, per shape, true depth and recovery.
    bias: std::collections::BTreeMap<(&'static str, i64, i64), Reads>,
}

/// The values read over some parts, and the count of parts that read none.
#[derive(Default)]
struct Reads {
    values: Vec<f64>,
    unread: usize,
}

impl Reads {
    fn add(&mut self, value: Option<f64>) {
        match value {
            Some(v) => self.values.push(v),
            None => self.unread += 1,
        }
    }

    /// The median and range of the values, and the counts read and unread.
    fn summary(&self) -> String {
        let counts = format!("{} read, {} unread", self.values.len(), self.unread);
        let low = self.values.iter().copied().reduce(f64::min);
        let high = self.values.iter().copied().reduce(f64::max);
        match (low, high) {
            (Some(low), Some(high)) => format!(
                "{:>6.2} ({low:.2} to {high:.2}), {counts}",
                median(&self.values)
            ),
            _ => format!("{:>6} {counts}", "-"),
        }
    }
}

fn tick(pair: &mut (usize, usize), pass: bool) {
    pair.1 += 1;
    if pass {
        pair.0 += 1;
    }
}

/// Scores a tail part from the release and the tail RT60 it read. A dry
/// part passes when it reads no RT60, and each "where read" row counts the
/// parts that read none.
fn tail_cases(
    tally: &mut Tally,
    release_s: f64,
    rt60_s: f64,
    release: Option<f64>,
    rt60: Option<f64>,
) {
    tick(&mut tally.release_read, release.is_some());
    if let Some(r) = release {
        tick(&mut tally.release, (r / release_s - 1.0).abs() <= 0.5);
    }
    if rt60_s > 0.0 {
        tick(&mut tally.rt60_read, rt60.is_some());
        if let Some(r) = rt60 {
            tick(&mut tally.rt60, (r / rt60_s - 1.0).abs() <= 0.25);
        }
    } else {
        tick(&mut tally.dry_rt60, rt60.is_none());
    }
}

impl Tally {
    fn rows(&self) -> Vec<String> {
        let mut rows = vec!["\nAcceptance (passed / cases)".to_owned()];
        for (name, (p, n)) in [
            ("attack 10-90 % within 15 % above 4 periods", self.attack),
            ("upper bound under 1.5 periods, none above 4", self.bound),
            ("pooled decay within 20 %", self.decay),
            ("pooled sustain within 2 dB", self.sustain),
            ("knee within 15 ms of the note-off", self.knee),
            ("same, sustain shorter than the decay", self.knee_short),
            (
                "same, dry tremolo with a release from 200 ms",
                self.knee_tremolo,
            ),
            ("pluck has no knee", self.pluck),
            ("level spread within 1 dB", self.spread),
            ("pump depth within 5 dB below the truth", self.pump_depth),
            ("pump recovery within 30 % of the truth", self.pump_back),
            ("lock as expected", self.lock),
            ("tail parts that write a release", self.release_read),
            ("written release within 50 %", self.release),
            ("reverb parts that read a tail RT60", self.rt60_read),
            ("tail RT60 within 25 % where read", self.rt60),
            ("dry tail parts that read no tail RT60", self.dry_rt60),
        ] {
            rows.push(format!("{name:<48} {p:>5} / {n}"));
        }
        // The rows that score only the parts that read a value.
        for (name, (p, n)) in [
            ("written release", self.release_read),
            ("tail RT60 within", self.rt60_read),
        ] {
            if let Some(row) = rows.iter_mut().find(|row| row.starts_with(name)) {
                let _ = write!(row, ", {} unread", n - p);
            }
        }
        rows.push("\nRead minus true, dB: median (min to max) over the cases of a row".to_owned());
        rows.push(format!(
            "{:<48} {}",
            "pooled sustain, the cases of its row",
            self.sustain_error.summary()
        ));
        rows.push(format!(
            "{:<48} {}",
            "pump depth, the cases of its row",
            self.depth_error.summary()
        ));
        rows.push(
            "\nPump depth bias, measured minus true dB: median (min to max) per shape, depth and recovery"
                .to_owned(),
        );
        for ((shape, depth, release), reads) in &self.bias {
            rows.push(format!(
                "{shape:<6} {depth:>3} dB {release:>4} ms: {}",
                reads.summary()
            ));
        }
        rows
    }

    fn print(&self) {
        for row in self.rows() {
            println!("{row}");
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
        "release_s": layer.release.value().map(|s| s.median().get()),
        "tail_rt60_s": layer.tail_rt60.value().map(|s| s.median().get()),
        "tail_weight": layer.tail_weight.value().map(|s| s.median().get()),
        "tail_votes": layer.tail_votes.count(),
    });
    let read: Vec<(f64, Option<f64>)> = layer
        .notes
        .iter()
        .map(|n| (n.onset.get(), n.knee.map(Seconds::get)))
        .collect();
    match &part.truth {
        Truth::Attack { f0, attack_s } => {
            let note = layer.notes.first();
            let rise = note.and_then(|n| n.attack).map(|a| a.get() / RAMP_FACTOR);
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
            notes,
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
            tally
                .sustain_error
                .add(sustain.map(|s| s - adsr.sustain_db));
            for hit in knee_hits(&read, notes) {
                tick(&mut tally.knee, hit);
            }
            let offs: Vec<f64> = notes.iter().map(|n| n.1).collect();
            record["truth"] = json!({"decay_s": adsr.decay, "sustain_db": adsr.sustain_db,
                "hold_s": adsr.hold, "release_s": adsr.release, "offs": offs, "db_linear": db_linear});
        }
        Truth::Knees { notes, row } => {
            let pair = match row {
                KneeRow::ShortSustain => &mut tally.knee_short,
                KneeRow::Tremolo => &mut tally.knee_tremolo,
            };
            for hit in knee_hits(&read, notes) {
                tick(pair, hit);
            }
            let offs: Vec<f64> = notes.iter().map(|n| n.1).collect();
            record["truth"] = json!({"offs": offs});
        }
        Truth::Pluck { onsets } => {
            for hit in pluck_hits(&read, onsets) {
                tick(&mut tally.pluck, hit);
            }
        }
        Truth::Tail { release_s, rt60_s } => {
            tail_cases(
                tally,
                *release_s,
                *rt60_s,
                layer.release.value().map(|s| s.median().get()),
                layer.tail_rt60.value().map(|s| s.median().get()),
            );
            record["truth"] = json!({"release_s": release_s, "rt60_s": rt60_s});
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
                tally.depth_error.add(depth.map(|d| d - depth_db));
                let right = match (back, expected_back) {
                    (Some(b), Some(e)) => (b / e - 1.0).abs() <= 0.3,
                    (None, None) => pump.is_some(),
                    _ => false,
                };
                tick(&mut tally.pump_back, right);
            }
            tick(&mut tally.lock, lock_right(*depth_db, pump.map(|p| p.lock)));
            if *depth_db > 0.0 {
                // Whole dB and ms keys keep the map ordered.
                #[expect(clippy::cast_possible_truncation, reason = "small grid values")]
                let key = (
                    shape.name(),
                    depth_db.round() as i64,
                    (release_s * 1e3).round() as i64,
                );
                tally
                    .bias
                    .entry(key)
                    .or_default()
                    .add(depth.map(|d| d - depth_db));
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

/// The median of `values`: the mean of the middle pair for an even count.
fn median(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(f64::total_cmp);
    let half = v.len() / 2;
    if v.len().is_multiple_of(2) {
        f64::midpoint(v[half - 1], v[half])
    } else {
        v[half]
    }
}

/// The envelope read for the true note at `onset`: the layer skips some
/// notes, so a position in its list names no true note.
fn envelope_at(read: &[(f64, Option<f64>)], onset: f64) -> Option<&(f64, Option<f64>)> {
    read.iter().find(|(on, _)| (on - onset).abs() < 1e-9)
}

/// Per true note `(onset, note-off)`, whether its envelope has a knee
/// within 15 ms of the note-off. `read` holds `(onset, knee)` per
/// envelope; a note the layer skipped fails.
fn knee_hits(read: &[(f64, Option<f64>)], truth: &[(f64, f64)]) -> Vec<bool> {
    truth
        .iter()
        .map(|&(onset, off)| {
            envelope_at(read, onset)
                .is_some_and(|(_, knee)| knee.is_some_and(|k| (k - off).abs() <= 0.015))
        })
        .collect()
}

/// Per true onset, whether its envelope shows no knee; a note the layer
/// skipped fails.
fn pluck_hits(read: &[(f64, Option<f64>)], onsets: &[f64]) -> Vec<bool> {
    onsets
        .iter()
        .map(|&onset| envelope_at(read, onset).is_some_and(|(_, knee)| knee.is_none()))
        .collect()
}

/// Whether the lock of a pump part is right; `pump` is the read lock, or
/// `None` when no pump was read, which reads as no lock. A 3 dB duck may
/// lock to the beat or read too shallow for a pump, but never free.
fn lock_right(depth_db: f64, pump: Option<PumpLock>) -> bool {
    let read = pump.unwrap_or(PumpLock::None);
    if depth_db >= 6.0 {
        read == PumpLock::Beat
    } else if depth_db == 0.0 {
        read == PumpLock::None
    } else {
        matches!(read, PumpLock::Beat | PumpLock::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_median_of_an_even_count_is_the_mean_of_the_middle_pair() {
        assert!((median(&[-1.0, -3.0]) + 2.0).abs() < 1e-12);
        assert!((median(&[3.0, 1.0, 2.0]) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn a_skipped_note_fails_its_knee_and_moves_no_other_note() {
        let truth = [(0.2, 0.7), (1.2, 1.7), (2.2, 2.7)];
        let read = [(1.2, Some(1.7)), (2.2, Some(2.705))];
        assert_eq!(knee_hits(&read, &truth), [false, true, true]);
    }

    #[test]
    fn a_skipped_pluck_is_a_failure() {
        let read = [(1.2, None)];
        assert_eq!(pluck_hits(&read, &[0.0, 1.2]), [false, true]);
    }

    #[test]
    fn the_lock_check_can_fail_at_3_db_and_reads_no_pump_as_no_lock() {
        assert!(!lock_right(3.0, Some(PumpLock::Free)));
        assert!(lock_right(3.0, Some(PumpLock::Beat)));
        assert!(lock_right(3.0, None));
        assert!(lock_right(0.0, None));
        assert!(!lock_right(6.0, None));
        assert!(!lock_right(0.0, Some(PumpLock::Beat)));
    }

    /// Renders the parts whose name holds `only`, and returns their names
    /// and lengths with the saw samples the run rendered.
    fn render(only: &str) -> (Vec<(String, usize)>, usize) {
        let mut parts = Vec::new();
        let mut take = |part: Part| -> crate::Result<()> {
            parts.push((part.name, part.audio.len()));
            Ok(())
        };
        let mut sink = Sink {
            only: Some(only),
            saws: Saws::default(),
            taken: 0,
            take: &mut take,
        };
        all_parts(&mut sink).unwrap();
        let rendered = sink.saws.rendered;
        (parts, rendered)
    }

    #[test]
    fn a_selected_part_renders_alone() {
        let (parts, rendered) = render("attack 110 Hz 10 ms");
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].0, "attack 110 Hz 10 ms");
        // One note of the part, and no saw of another part.
        assert!(rendered > 0 && rendered <= parts[0].1, "{rendered}");
        let (none, rendered) = render("no such part");
        assert_eq!(none.len(), 0);
        assert_eq!(rendered, 0);
    }

    #[test]
    fn a_saw_from_the_cache_equals_one_rendered_at_once() {
        let mut saws = Saws::default();
        let short = saws.get(123.47, 1000, 0.3).to_vec();
        let long = saws.get(123.47, 3000, 0.3).to_vec();
        let whole = saw_range(123.47, 0.3, 0, 3000);
        assert_eq!(long, whole);
        assert_eq!(short, whole[..1000]);
        assert_eq!(saws.rendered, 3000);
        saws.get(123.47, 2000, 0.3);
        assert_eq!(saws.rendered, 3000);
    }

    #[test]
    fn the_recovery_truth_uses_the_layer_threshold() {
        // A dB-linear duck is back within PUMP_BACK after
        // release ln(depth / PUMP_BACK).
        let (depth, release, beat) = (12.0, 0.15, 60.0 / 128.0);
        let expected = release * (depth / PUMP_BACK.get()).ln();
        let back = back_time(depth, release, beat, Shape::Db).unwrap();
        assert!((back - expected).abs() <= 2e-4, "{back} vs {expected}");
    }

    #[test]
    fn a_dry_tail_fails_on_a_false_rt60_and_unread_parts_are_counted() {
        let mut tally = Tally::default();
        tail_cases(&mut tally, 0.2, 0.0, Some(0.2), Some(3.0));
        tail_cases(&mut tally, 0.2, 0.0, Some(0.2), None);
        tail_cases(&mut tally, 0.2, 1.5, None, None);
        tail_cases(&mut tally, 0.2, 1.5, Some(0.21), Some(1.6));
        assert_eq!(tally.dry_rt60, (1, 2));
        assert_eq!(tally.rt60, (1, 1));
        assert_eq!(tally.rt60_read, (1, 2));
        assert_eq!(tally.release, (3, 3));
        let rows = tally.rows().join("\n");
        assert!(
            rows.contains("dry tail parts that read no tail RT60"),
            "{rows}"
        );
        assert!(rows.contains("1 / 1, 1 unread"), "{rows}");
    }

    #[test]
    fn a_bias_row_counts_the_parts_that_read_no_pump() {
        let mut reads = Reads::default();
        for value in [Some(-3.0), Some(-1.0), None] {
            reads.add(value);
        }
        assert_eq!(reads.summary(), " -2.00 (-3.00 to -1.00), 2 read, 1 unread");
    }
}
