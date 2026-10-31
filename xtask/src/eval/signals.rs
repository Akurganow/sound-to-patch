//! The generated evaluation set: parts with known notes on an exact BPM
//! grid, rendered with our own oscillators. Nothing here is committed as
//! audio; every run renders the set again from these recipes.

use std::f64::consts::TAU;

use realfft::RealFftPlanner;

use super::matcher::{EvalNote, count};

/// The rate the set renders at. The stem intake resamples it for the model.
pub(crate) const RATE: u32 = 44_100;

/// The part class whose manual profile is the right one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Class {
    Bass,
    Lead,
    Pad,
    /// Silence: every setting must give no note.
    Silence,
}

impl Class {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Bass => "bass",
            Self::Lead => "lead",
            Self::Pad => "pad",
            Self::Silence => "silence",
        }
    }
}

/// How one note sounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Voice {
    /// A saw, band-limited by polynomial steps at each reset, with a 5 ms attack and a linear release.
    Saw { release: f64 },
    /// A sine with the same envelope.
    Sine { release: f64 },
    /// A sine whose pitch falls from twice the note to the note with a
    /// 20 ms time constant, under a 0.6 s exponential decay.
    Kick808,
    /// A saw under a 0.12 s exponential decay, cut by a 20 ms release.
    Pluck,
    /// A saw with a 30 ms attack and a 0.1 s release.
    Pad,
    /// A saw under a 1 s exponential decay, cut by a 0.1 s release: a
    /// piano or a plucked string left to ring.
    Keys,
}

/// A note of the score: start and length in beats, a MIDI pitch, and the
/// pitch it glides to over its last `glide` beats and then holds for
/// `hold` beats, if any.
#[derive(Clone, Copy, Debug)]
struct Score {
    start: f64,
    length: f64,
    pitch: f64,
    glide_to: Option<(f64, f64, f64)>,
}

fn n(start: f64, length: f64, pitch: f64) -> Score {
    Score {
        start,
        length,
        pitch,
        glide_to: None,
    }
}

/// One generated part.
pub(crate) struct Part {
    pub(crate) name: String,
    pub(crate) class: Class,
    pub(crate) bpm: f64,
    /// The notes to find.
    pub(crate) notes: Vec<EvalNote>,
    /// Notes of a quiet second part mixed under the first, if any.
    pub(crate) bleed: Vec<EvalNote>,
    pub(crate) audio: Vec<f64>,
}

/// Seconds of the score at a BPM.
fn beats(bpm: f64, beats: f64) -> f64 {
    beats * 60.0 / bpm
}

fn hz(pitch: f64) -> f64 {
    440.0 * 2_f64.powf((pitch - 69.0) / 12.0)
}

/// The polynomial band-limited step correction of a saw at phase `t` with increment `dt`.
fn blep(t: f64, dt: f64) -> f64 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

fn index(seconds: f64) -> usize {
    let samples = (seconds * f64::from(RATE)).round();
    // The set never reaches minutes, so the cast is exact.
    if samples <= 0.0 {
        0
    } else {
        samples_to_usize(samples)
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "callers pass a rounded, non-negative sample count of a short signal"
)]
fn samples_to_usize(samples: f64) -> usize {
    samples as usize
}

/// Renders the notes of a score into `out` at `level`, and returns the
/// reference notes.
fn render(
    out: &mut Vec<f64>,
    bpm: f64,
    voice: Voice,
    level: f64,
    score: &[Score],
) -> Vec<EvalNote> {
    let rate = f64::from(RATE);
    let mut reference = Vec::with_capacity(score.len());
    for note in score {
        let onset = beats(bpm, note.start);
        let offset = beats(bpm, note.start + note.length);
        // A glide splits at its midpoint into two notes.
        let split = note
            .glide_to
            .map_or(offset, |(_, glide, _)| offset - beats(bpm, glide) / 2.0);
        reference.push(EvalNote {
            onset,
            offset: split,
            pitch: note.pitch,
        });
        if let Some((to, _, hold)) = note.glide_to {
            reference.push(EvalNote {
                onset: split,
                offset: offset + beats(bpm, hold),
                pitch: to,
            });
        }
        let release = match voice {
            Voice::Saw { release } | Voice::Sine { release } => release,
            Voice::Kick808 => 0.05,
            Voice::Pluck => 0.02,
            Voice::Pad | Voice::Keys => 0.1,
        };
        let hold = note.glide_to.map_or(0.0, |(_, _, hold)| beats(bpm, hold));
        let end = offset + hold;
        let (first, last) = (index(onset), index(end + release));
        if out.len() < last {
            out.resize(last, 0.0);
        }
        let mut phase = 0.0_f64;
        for (i, sample) in out.iter_mut().enumerate().take(last).skip(first) {
            let t = count(i - first) / rate;
            let now = onset + t;
            let pitch = match note.glide_to {
                Some((to, glide, _)) => {
                    let start = beats(bpm, note.start + note.length - glide);
                    let fraction = ((now - start) / beats(bpm, glide)).clamp(0.0, 1.0);
                    note.pitch + (to - note.pitch) * fraction
                }
                None => note.pitch,
            };
            let mut f = hz(pitch);
            if voice == Voice::Kick808 {
                f += hz(pitch) * (-t / 0.02).exp();
            }
            let dt = f / rate;
            let attack = match voice {
                Voice::Pad => 0.03,
                Voice::Kick808 => 0.002,
                _ => 0.005,
            };
            let mut envelope = (t / attack).min(1.0);
            if now > end {
                envelope *= (1.0 - (now - end) / release).max(0.0);
            }
            envelope *= match voice {
                Voice::Kick808 => (-t / 0.6).exp(),
                Voice::Pluck => (-t / 0.12).exp(),
                Voice::Keys => (-t / 1.0).exp(),
                _ => 1.0,
            };
            let value = match voice {
                Voice::Sine { .. } | Voice::Kick808 => (TAU * phase).sin(),
                Voice::Saw { .. } | Voice::Pluck | Voice::Pad | Voice::Keys => {
                    2.0 * phase - 1.0 - blep(phase, dt)
                }
            };
            *sample += level * envelope * value;
            phase = (phase + dt).fract();
        }
    }
    reference
}

/// SplitMix64, for seeded noise.
pub(super) fn splitmix(seed: u64, count: usize) -> Vec<u64> {
    let mut state = seed;
    (0..count)
        .map(|_| {
            state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        })
        .collect()
}

#[expect(
    clippy::cast_precision_loss,
    reason = "the top 53 bits are exact in f64"
)]
fn unit(bits: u64) -> f64 {
    (bits >> 11) as f64 / (1_u64 << 53) as f64
}

/// Adds a reverb tail: seeded Gaussian noise under an exponential decay of
/// `rt60` seconds, normalised to unit energy, mixed `wet_db` below the dry
/// signal.
fn reverb(dry: &[f64], rt60: f64, wet_db: f64, seed: u64) -> Vec<f64> {
    let length = index(2.0 * rt60);
    let bits = splitmix(seed, length + 1);
    let mut ir: Vec<f64> = (0..length)
        .map(|i| {
            let (u1, u2) = (unit(bits[i]).max(f64::MIN_POSITIVE), unit(bits[i + 1]));
            let gauss = (-2.0 * u1.ln()).sqrt() * (TAU * u2).cos();
            gauss * (-(1000_f64.ln()) * count(i) / f64::from(RATE) / rt60).exp()
        })
        .collect();
    let energy = ir.iter().map(|x| x * x).sum::<f64>().sqrt();
    for x in &mut ir {
        *x /= energy;
    }
    let wet = convolve(dry, &ir);
    let gain = 10_f64.powf(wet_db / 20.0);
    dry.iter()
        .chain(std::iter::repeat_n(&0.0, length))
        .zip(&wet)
        .map(|(d, w)| d + gain * w)
        .collect()
}

/// Full linear convolution by FFT.
fn convolve(x: &[f64], h: &[f64]) -> Vec<f64> {
    let out = x.len() + h.len() - 1;
    let len = out.next_power_of_two();
    let mut planner = RealFftPlanner::<f64>::new();
    let (forward, inverse) = (planner.plan_fft_forward(len), planner.plan_fft_inverse(len));
    let spectrum = |signal: &[f64]| {
        let mut buffer = signal.to_vec();
        buffer.resize(len, 0.0);
        let mut spectrum = forward.make_output_vec();
        forward
            .process(&mut buffer, &mut spectrum)
            .map(|()| spectrum)
    };
    let (Ok(a), Ok(b)) = (spectrum(x), spectrum(h)) else {
        return vec![0.0; out];
    };
    let mut product: Vec<_> = a.iter().zip(&b).map(|(p, q)| p * q).collect();
    let mut result = inverse.make_output_vec();
    if inverse.process(&mut product, &mut result).is_err() {
        return vec![0.0; out];
    }
    result.truncate(out);
    let scale = count(len);
    result.iter().map(|v| v / scale).collect()
}

fn part(name: &str, class: Class, bpm: f64, voice: Voice, score: &[Score]) -> Part {
    let mut audio = Vec::new();
    let notes = render(&mut audio, bpm, voice, 0.3, score);
    audio.extend(std::iter::repeat_n(0.0, index(0.5)));
    Part {
        name: name.to_owned(),
        class,
        bpm,
        notes,
        bleed: Vec::new(),
        audio,
    }
}

fn with_reverb(part: &Part) -> Part {
    Part {
        name: format!("{}+reverb", part.name),
        class: part.class,
        bpm: part.bpm,
        notes: part.notes.clone(),
        bleed: Vec::new(),
        audio: reverb(&part.audio, 1.5, -10.0, 6500),
    }
}

/// A sixteenth-note run over a repeating figure, for two bars.
fn run(figure: &[f64]) -> Vec<Score> {
    (0..32)
        .map(|i| n(1.0 + count(i) * 0.25, 0.2, figure[i % figure.len()]))
        .collect()
}

fn melody() -> Vec<Score> {
    [
        60.0, 64.0, 67.0, 72.0, 71.0, 67.0, 64.0, 62.0, 65.0, 69.0, 72.0, 76.0, 74.0, 71.0, 67.0,
        60.0,
    ]
    .iter()
    .enumerate()
    .map(|(i, &pitch)| n(1.0 + count(i), 0.75, pitch))
    .collect()
}

/// Four bass notes, a rest of four seconds, and four more, the last two
/// of which `soft` renders 20 and 30 dB down.
fn bass_line() -> (Vec<Score>, Vec<Score>) {
    let loud: Vec<Score> = [33.0, 36.0, 38.0, 40.0, 38.0, 36.0]
        .iter()
        .enumerate()
        .map(|(i, &pitch)| {
            let start = if i < 4 {
                1.0 + count(i) * 2.0
            } else {
                17.0 + count(i - 4) * 2.0
            };
            n(start, 1.5, pitch)
        })
        .collect();
    let soft = vec![n(21.0, 1.5, 33.0), n(23.0, 1.5, 31.0)];
    (loud, soft)
}

/// The bass parts.
fn bass_parts() -> Vec<Part> {
    let saw = Voice::Saw { release: 0.05 };
    let lows: Vec<Score> = [23.0, 28.0, 33.0, 23.0, 28.0, 33.0]
        .iter()
        .enumerate()
        .map(|(i, &pitch)| n(1.0 + count(i) * 1.5, 1.0, pitch))
        .collect();
    let repeats: Vec<Score> = (0..16)
        .map(|i| n(1.0 + count(i) * 0.5, 0.4, if i < 8 { 33.0 } else { 38.0 }))
        .collect();
    let release: Vec<Score> = (0..6).map(|i| n(1.0 + count(i) * 2.0, 1.0, 45.0)).collect();
    vec![
        part("bass B0 E1 A1 saw", Class::Bass, 120.0, saw, &lows),
        part(
            "bass B0 E1 A1 sine",
            Class::Bass,
            120.0,
            Voice::Sine { release: 0.05 },
            &lows,
        ),
        part(
            "bass 808",
            Class::Bass,
            120.0,
            Voice::Kick808,
            &[
                n(1.0, 1.5, 28.0),
                n(3.0, 1.5, 33.0),
                n(5.0, 1.5, 38.0),
                n(7.0, 1.5, 31.0),
            ],
        ),
        part(
            "bass 110 Hz release 0.3 s",
            Class::Bass,
            120.0,
            Voice::Saw { release: 0.3 },
            &release,
        ),
        part(
            "bass run 90",
            Class::Bass,
            90.0,
            saw,
            &run(&[33.0, 45.0, 40.0, 43.0]),
        ),
        part(
            "bass run 120",
            Class::Bass,
            120.0,
            saw,
            &run(&[33.0, 45.0, 40.0, 43.0]),
        ),
        part(
            "bass run 140",
            Class::Bass,
            140.0,
            saw,
            &run(&[33.0, 45.0, 40.0, 43.0]),
        ),
        part("bass repeats", Class::Bass, 120.0, saw, &repeats),
    ]
}

/// Bass parts that test the octave cleanup and the split join: octave
/// basses, a fast run with an octave jump, and repeats at 175 BPM.
fn octave_and_fast_parts() -> Vec<Part> {
    vec![
        part(
            "bass octaves 16ths",
            Class::Bass,
            120.0,
            Voice::Saw { release: 0.01 },
            &octaves(0.25, 0.22),
        ),
        part(
            "bass octaves 8ths",
            Class::Bass,
            120.0,
            Voice::Saw { release: 0.01 },
            &octaves(0.5, 0.45),
        ),
        part(
            "bass 175 16ths repeats",
            Class::Bass,
            175.0,
            Voice::Saw { release: 0.01 },
            &[
                38.0, 38.0, 38.0, 45.0, 38.0, 38.0, 43.0, 45.0, 41.0, 41.0, 41.0, 48.0, 41.0, 41.0,
                45.0, 48.0,
            ]
            .repeat(2)
            .iter()
            .enumerate()
            .map(|(i, &pitch)| n(1.0 + count(i) * 0.25, 0.2, pitch))
            .collect::<Vec<_>>(),
        ),
        part(
            "bass run 140 octave jump",
            Class::Bass,
            140.0,
            Voice::Saw { release: 0.01 },
            &[
                45.0, 45.0, 45.0, 45.0, 48.0, 48.0, 52.0, 52.0, 45.0, 45.0, 57.0, 57.0, 45.0, 45.0,
                43.0, 43.0,
            ]
            .iter()
            .enumerate()
            .map(|(i, &pitch)| n(1.0 + count(i) * 0.25, 0.225, pitch))
            .collect::<Vec<_>>(),
        ),
    ]
}

/// A disco octave bass: A2 and A3 in turn, then D2 and D3, 16 notes each,
/// one every `step` beats for `length` beats.
fn octaves(step: f64, length: f64) -> Vec<Score> {
    [45.0, 57.0]
        .repeat(8)
        .into_iter()
        .chain([38.0, 50.0].repeat(8))
        .enumerate()
        .map(|(i, pitch)| n(1.0 + count(i) * step, length, pitch))
        .collect()
}

/// The lead parts.
fn lead_parts() -> Vec<Part> {
    let saw = Voice::Saw { release: 0.05 };
    vec![
        part("lead melody", Class::Lead, 120.0, saw, &melody()),
        part(
            "lead 220 Hz",
            Class::Lead,
            120.0,
            saw,
            &(0..6)
                .map(|i| n(1.0 + count(i), 0.75, 57.0))
                .collect::<Vec<_>>(),
        ),
        part(
            "lead run 120",
            Class::Lead,
            120.0,
            saw,
            &run(&[69.0, 76.0, 72.0, 81.0]),
        ),
        part(
            "lead run 140",
            Class::Lead,
            140.0,
            saw,
            &run(&[69.0, 76.0, 72.0, 81.0]),
        ),
        part(
            "lead plucks",
            Class::Lead,
            120.0,
            Voice::Pluck,
            &melody()
                .iter()
                .map(|s| n(s.start, 0.4, s.pitch))
                .collect::<Vec<_>>(),
        ),
        part(
            "lead glides",
            Class::Lead,
            120.0,
            saw,
            &[
                Score {
                    start: 1.0,
                    length: 2.0,
                    pitch: 57.0,
                    glide_to: Some((64.0, 0.25, 1.5)),
                },
                Score {
                    start: 5.0,
                    length: 2.0,
                    pitch: 72.0,
                    glide_to: Some((67.0, 0.25, 1.5)),
                },
            ],
        ),
        // A6 on a saw is the one tone the f0 track reads an octave low.
        part(
            "lead 1760 Hz saw",
            Class::Lead,
            120.0,
            saw,
            &(0..4)
                .map(|i| n(1.0 + count(i), 0.75, 93.0))
                .collect::<Vec<_>>(),
        ),
        part(
            "lead high",
            Class::Lead,
            120.0,
            saw,
            &[84.0, 88.0, 91.0, 95.0, 91.0, 88.0]
                .iter()
                .enumerate()
                .map(|(i, &p)| n(1.0 + count(i), 0.75, p))
                .collect::<Vec<_>>(),
        ),
    ]
}

/// The pad parts.
fn pad_parts() -> Vec<Part> {
    let chords: Vec<Score> = [
        [57.0, 60.0, 64.0],
        [53.0, 57.0, 60.0],
        [55.0, 59.0, 62.0],
        [52.0, 55.0, 59.0],
    ]
    .iter()
    .enumerate()
    .flat_map(|(i, chord)| chord.map(|pitch| n(1.0 + count(i) * 4.0, 3.5, pitch)))
    .collect();
    vec![
        part("pad chords", Class::Pad, 120.0, Voice::Pad, &chords),
        part(
            "pad sustain 8 s",
            Class::Pad,
            120.0,
            Voice::Pad,
            &[n(1.0, 16.0, 57.0), n(1.0, 16.0, 60.0), n(1.0, 16.0, 64.0)],
        ),
        part(
            "pad staggered releases",
            Class::Pad,
            120.0,
            Voice::Pad,
            &staggered_releases(),
        ),
        part(
            "pad moving inner voice",
            Class::Pad,
            120.0,
            Voice::Pad,
            &moving_inner_voice(),
        ),
        part("pad strummed", Class::Pad, 120.0, Voice::Keys, &strummed()),
        part(
            "pad arpeggio sustain",
            Class::Pad,
            120.0,
            Voice::Keys,
            &arpeggio_sustain(),
        ),
    ]
}

/// The four triads of "pad chords", each note released up to 150 ms early
/// or late (0.3 beats at 120 BPM), as hands lift one finger at a time.
fn staggered_releases() -> Vec<Score> {
    let shifts = [-0.3, 0.0, 0.3];
    [
        [57.0, 60.0, 64.0],
        [53.0, 57.0, 60.0],
        [55.0, 59.0, 62.0],
        [52.0, 55.0, 59.0],
    ]
    .iter()
    .enumerate()
    .flat_map(|(i, chord)| {
        chord
            .iter()
            .zip(shifts.iter().cycle().skip(i))
            .map(move |(&pitch, &shift)| n(1.0 + count(i) * 4.0, 3.0 + shift, pitch))
    })
    .collect()
}

/// A held A3 and E4 over 16 beats while the inner voice moves C4, D4, C4,
/// B3 every 4 beats.
fn moving_inner_voice() -> Vec<Score> {
    let mut notes = vec![n(1.0, 16.0, 57.0), n(1.0, 16.0, 64.0)];
    notes.extend(
        [60.0, 62.0, 60.0, 59.0]
            .iter()
            .enumerate()
            .map(|(i, &pitch)| n(1.0 + count(i) * 4.0, 3.9, pitch)),
    );
    notes
}

/// Four-note chords strummed upward, 30 ms (0.06 beats) between strings,
/// one every 2 beats for 8 chords.
fn strummed() -> Vec<Score> {
    let chords = [
        [45.0, 52.0, 57.0, 61.0],
        [50.0, 57.0, 62.0, 66.0],
        [52.0, 59.0, 64.0, 68.0],
        [45.0, 52.0, 57.0, 61.0],
    ];
    (0..8)
        .flat_map(|i| {
            chords[i % chords.len()]
                .iter()
                .enumerate()
                .map(move |(j, &pitch)| {
                    let start = 1.0 + count(i) * 2.0 + count(j) * 0.06;
                    n(start, 1.8 - count(j) * 0.06, pitch)
                })
        })
        .collect()
}

/// Broken chords on eighth notes with the sustain held: each of the four
/// notes rings on to the end of its bar.
fn arpeggio_sustain() -> Vec<Score> {
    let chords = [
        [48.0, 52.0, 55.0, 60.0],
        [45.0, 52.0, 57.0, 60.0],
        [50.0, 53.0, 57.0, 62.0],
        [43.0, 50.0, 55.0, 59.0],
    ];
    chords
        .iter()
        .enumerate()
        .flat_map(|(bar, chord)| {
            chord.iter().enumerate().map(move |(j, &pitch)| {
                let start = 1.0 + count(bar) * 4.0 + count(j) * 0.5;
                n(start, 1.0 + count(bar) * 4.0 + 3.8 - start, pitch)
            })
        })
        .collect()
}

/// A quiet bass line, MIDI 36 to 48 at 0.02, with one loud event at 3 s:
/// a noise burst of 1000 samples, a 1.5 kHz ring of 3000 samples, or a
/// 60 Hz thump of 6000 samples, each at full scale.
fn transient_parts() -> Vec<Part> {
    let saw = Voice::Saw { release: 0.05 };
    let line: Vec<Score> = [36.0, 38.0, 40.0, 41.0, 43.0, 45.0, 47.0, 48.0]
        .iter()
        .enumerate()
        .map(|(i, &pitch)| n(1.0 + count(i), 0.75, pitch))
        .collect();
    let rate = f64::from(RATE);
    let bits = splitmix(4242, 1001);
    let events: [(&str, Vec<f64>); 3] = [
        (
            "noise burst",
            (0..1000)
                .map(|i| {
                    let (u1, u2) = (unit(bits[i]).max(f64::MIN_POSITIVE), unit(bits[i + 1]));
                    ((-2.0 * u1.ln()).sqrt() * (TAU * u2).cos()).clamp(-1.0, 1.0)
                })
                .collect(),
        ),
        (
            "1.5 kHz ring",
            (0..3000)
                .map(|i| (TAU * 1500.0 * count(i) / rate).sin())
                .collect(),
        ),
        (
            "60 Hz thump",
            (0..6000)
                .map(|i| (TAU * 60.0 * count(i) / rate).sin() * (-count(i) / rate / 0.05).exp())
                .collect(),
        ),
    ];
    events
        .into_iter()
        .map(|(name, event)| {
            let mut audio = Vec::new();
            let notes = render(&mut audio, 120.0, saw, 0.02, &line);
            audio.extend(std::iter::repeat_n(0.0, index(0.5)));
            let at = index(3.0);
            for (sample, x) in audio.iter_mut().skip(at).zip(&event) {
                *sample += x;
            }
            Part {
                name: format!("bass transient {name}"),
                class: Class::Bass,
                bpm: 120.0,
                notes,
                bleed: Vec::new(),
                audio,
            }
        })
        .collect()
}

/// The whole set, in report order.
pub(crate) fn evaluation_set() -> Vec<Part> {
    let saw = Voice::Saw { release: 0.05 };
    let mut parts = bass_parts();
    parts.extend(octave_and_fast_parts());
    parts.extend(lead_parts());
    parts.extend(pad_parts());
    let reverbs: Vec<Part> = parts
        .iter()
        .filter(|p| {
            matches!(
                p.name.as_str(),
                "bass 110 Hz release 0.3 s" | "lead melody" | "pad chords"
            )
        })
        .map(with_reverb)
        .collect();
    parts.extend(reverbs);
    for db in [-20.0, -30.0, -40.0, -50.0] {
        let mut audio = Vec::new();
        let (loud, soft) = bass_line();
        let mut notes = render(&mut audio, 120.0, saw, 0.3, &loud);
        notes.extend(render(&mut audio, 120.0, saw, 0.03, &soft[..1]));
        notes.extend(render(
            &mut audio,
            120.0,
            saw,
            0.3 * 10_f64.powf(-1.5),
            &soft[1..],
        ));
        // The lead plays through the rest, where the bass is silent.
        let bleed_score: Vec<Score> = (0..24)
            .map(|i| n(1.5 + count(i), 0.5, [60.0, 64.0, 67.0, 72.0][i % 4]))
            .collect();
        let bleed = render(
            &mut audio,
            120.0,
            saw,
            0.3 * 10_f64.powf(db / 20.0),
            &bleed_score,
        );
        audio.extend(std::iter::repeat_n(0.0, index(0.5)));
        parts.push(Part {
            name: format!("bass + lead bleed {db} dB"),
            class: Class::Bass,
            bpm: 120.0,
            notes,
            bleed,
            audio,
        });
    }
    parts.extend(transient_parts());
    parts.push(Part {
        name: "silence".to_owned(),
        class: Class::Silence,
        bpm: 120.0,
        notes: Vec::new(),
        bleed: Vec::new(),
        audio: vec![0.0; index(4.0)],
    });
    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_sit_on_the_bpm_grid() {
        let set = evaluation_set();
        let run = set.iter().find(|p| p.name == "bass run 140").unwrap();
        assert_eq!(run.notes.len(), 32);
        let sixteenth = 60.0 / 140.0 / 4.0;
        assert!((run.notes[1].onset - run.notes[0].onset - sixteenth).abs() < 1e-12);
        let glides = set.iter().find(|p| p.name == "lead glides").unwrap();
        assert_eq!(glides.notes.len(), 4);
        assert!(set.iter().all(|p| p.audio.iter().all(|x| x.is_finite())));
    }

    #[test]
    fn a_saw_reaches_its_level_and_a_reverb_keeps_the_dry_part() {
        let mut audio = Vec::new();
        render(
            &mut audio,
            120.0,
            Voice::Saw { release: 0.05 },
            0.3,
            &[n(0.0, 1.0, 45.0)],
        );
        let peak = audio.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
        assert!((0.25..0.4).contains(&peak), "{peak}");
        let wet = reverb(&audio, 1.5, -10.0, 1);
        assert_eq!(wet.len(), audio.len() + index(3.0) - 1);
        let impulse = convolve(&[1.0, 0.0, 0.0], &[0.5, 0.25]);
        assert!(
            impulse
                .iter()
                .zip([0.5, 0.25, 0.0, 0.0])
                .all(|(a, b)| (a - b).abs() < 1e-12)
        );
    }
}
