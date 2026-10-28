//! Generated pitch motion: saw glides, a sine pitch drop and saw vibrato,
//! each optionally as a detuned unison.

use std::f64::consts::TAU;

use super::cast::round_to_usize;
use super::rng::uniform;

/// A band-limited saw that follows `f(t)`, one value per sample: harmonic
/// `h` at amplitude `1 / h` below the Nyquist frequency of `top`, the
/// highest frequency the voice reaches. The phase is the running sum of
/// `f / rate`.
fn saw_voice(f: impl Fn(f64) -> f64, top: f64, rate: f64, length: usize, phase0: f64) -> Vec<f64> {
    let mut phase = phase0;
    let harmonics = (rate / 2.0 / top).floor();
    let mut out = Vec::with_capacity(length);
    for n in 0..length {
        let t = f64::from(u32::try_from(n).unwrap()) / rate;
        let mut x = 0.0;
        let mut h = 1.0;
        while h <= harmonics {
            x += (TAU * h * phase).sin() / h;
            h += 1.0;
        }
        out.push(x);
        phase += f(t) / rate;
    }
    out
}

/// `voices` saws detuned evenly over `±detune_ct` cents, each with a seeded
/// random start phase unless `in_phase`, summed and scaled to a peak near
/// 0.5. One voice plays the pitch itself.
fn unison(
    f: impl Fn(f64) -> f64 + Copy,
    top: f64,
    rate: f64,
    length: usize,
    (voices, detune_ct): (usize, f64),
    in_phase: bool,
) -> Vec<f64> {
    let phases = uniform(17, voices);
    let mut out = vec![0.0; length];
    for (v, random) in phases.into_iter().enumerate() {
        let offset = if voices == 1 {
            0.0
        } else {
            detune_ct
                * (2.0 * f64::from(u32::try_from(v).unwrap())
                    / f64::from(u32::try_from(voices - 1).unwrap())
                    - 1.0)
        };
        let ratio = 2f64.powf(offset / 1200.0);
        let start = if voices == 1 || in_phase { 0.0 } else { random };
        for (sample, x) in out.iter_mut().zip(saw_voice(
            move |t| f(t) * ratio,
            top * ratio,
            rate,
            length,
            start,
        )) {
            *sample += x;
        }
    }
    let peak = out.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
    out.iter().map(|x| 0.5 * x / peak).collect()
}

/// The glide curve `s(x) = 1 - (1 - x)^p` on `0..1`.
pub(crate) fn curve(x: f64, p: f64) -> f64 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powf(p)
}

/// A saw that holds `start_hz` for `hold_s`, glides up `semitones` over
/// `full_s` along [`curve`] with power `p`, and holds the top for `hold_s`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Glide {
    pub(crate) start_hz: f64,
    pub(crate) semitones: f64,
    pub(crate) full_s: f64,
    pub(crate) p: f64,
    pub(crate) hold_s: f64,
    pub(crate) voices: usize,
    pub(crate) detune_ct: f64,
    /// Every voice starts at phase 0, so the unison beats hardest at the
    /// start.
    pub(crate) in_phase: bool,
}

impl Glide {
    /// A 7-semitone glide up from 110 Hz with holds of 0.5 s, one voice.
    pub(crate) fn mono(full_s: f64, p: f64) -> Self {
        Self {
            start_hz: 110.0,
            semitones: 7.0,
            full_s,
            p,
            hold_s: 0.5,
            voices: 1,
            detune_ct: 0.0,
            in_phase: false,
        }
    }

    /// The same glide on a 7-voice unison detuned over ±25 cents.
    pub(crate) fn unison(full_s: f64, p: f64) -> Self {
        Self {
            voices: 7,
            detune_ct: 25.0,
            ..Self::mono(full_s, p)
        }
    }

    pub(crate) fn render(&self, rate: f64) -> Vec<f64> {
        let me = *self;
        let length = round_to_usize(rate * (2.0 * me.hold_s + me.full_s));
        let f = move |t: f64| {
            let x = (t - me.hold_s) / me.full_s;
            me.start_hz * 2f64.powf(me.semitones * curve(x, me.p) / 12.0)
        };
        let top = me.start_hz * 2f64.powf(me.semitones / 12.0);
        unison(f, top, rate, length, (me.voices, me.detune_ct), me.in_phase)
    }
}

/// An 808-style tone: a sine whose frequency falls as
/// `f(t) = f_end + df exp(-t / tau)`, times `exp(-t / decay_s)`, from
/// `lead_s` of silence on, for `length_s` in all.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PitchDrop {
    pub(crate) f_end_hz: f64,
    pub(crate) df_hz: f64,
    pub(crate) tau_s: f64,
    pub(crate) decay_s: f64,
    pub(crate) lead_s: f64,
    pub(crate) length_s: f64,
}

impl PitchDrop {
    /// The four rows of the drop table, with a 0.5 s amplitude decay.
    pub(crate) const ROWS: [(f64, f64, f64); 4] = [
        (41.2, 41.2, 0.020),
        (41.2, 41.2, 0.050),
        (55.0, 165.0, 0.030),
        (41.2, 13.8, 0.100),
    ];

    pub(crate) fn row(f_end_hz: f64, df_hz: f64, tau_s: f64) -> Self {
        Self {
            f_end_hz,
            df_hz,
            tau_s,
            decay_s: 0.5,
            lead_s: 0.25,
            length_s: 1.25,
        }
    }

    pub(crate) fn render(&self, rate: f64) -> Vec<f64> {
        (0..round_to_usize(rate * self.length_s))
            .map(|n| {
                let t = f64::from(u32::try_from(n).unwrap()) / rate - self.lead_s;
                if t < 0.0 {
                    return 0.0;
                }
                let phase =
                    self.f_end_hz * t + self.df_hz * self.tau_s * (1.0 - (-t / self.tau_s).exp());
                0.8 * (TAU * phase).sin() * (-t / self.decay_s).exp()
            })
            .collect()
    }
}

/// 808 hits without a pitch drop, one per MIDI pitch: a sine that
/// restarts its phase at each hit, driven through `tanh(drive x)` when
/// `drive` is above zero, with a 3 ms attack, times `exp(-t / decay_s)`,
/// `hit_s` long and followed by `gap_s` of silence. `lead_s` of silence
/// comes before the first hit and after the last.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Hits {
    pub(crate) pitches: &'static [f64],
    pub(crate) hit_s: f64,
    pub(crate) decay_s: f64,
    pub(crate) gap_s: f64,
    pub(crate) lead_s: f64,
    pub(crate) drive: f64,
}

impl Hits {
    /// Hits of 0.3 s with a 0.25 s decay.
    pub(crate) fn new(pitches: &'static [f64], gap_s: f64) -> Self {
        Self {
            pitches,
            hit_s: 0.3,
            decay_s: 0.25,
            gap_s,
            lead_s: 0.2,
            drive: 0.0,
        }
    }

    pub(crate) fn render(&self, rate: f64) -> Vec<f64> {
        let hit = round_to_usize(rate * self.hit_s);
        let attack = round_to_usize(rate * 0.003);
        let mut out = vec![0.0; round_to_usize(rate * self.lead_s)];
        for &pitch in self.pitches {
            let f = 440.0 * 2f64.powf((pitch - 69.0) / 12.0);
            out.extend((0..hit).map(|n| {
                let t = f64::from(u32::try_from(n).unwrap()) / rate;
                let ramp = (f64::from(u32::try_from(n).unwrap())
                    / f64::from(u32::try_from(attack).unwrap()))
                .min(1.0);
                let x = (TAU * f * t).sin();
                let x = if self.drive > 0.0 {
                    (self.drive * x).tanh()
                } else {
                    x
                };
                0.5 * ramp * x * (-t / self.decay_s).exp()
            }));
            out.extend(std::iter::repeat_n(0.0, round_to_usize(rate * self.gap_s)));
        }
        out.extend(std::iter::repeat_n(0.0, round_to_usize(rate * self.lead_s)));
        out
    }
}

/// A saw at `f0_hz` whose pitch moves as `extent_ct sin(2 pi rate_hz t)`
/// cents, for `length_s` after `lead_s` of silence.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Vibrato {
    pub(crate) f0_hz: f64,
    pub(crate) rate_hz: f64,
    pub(crate) extent_ct: f64,
    pub(crate) length_s: f64,
    pub(crate) lead_s: f64,
    pub(crate) voices: usize,
    pub(crate) detune_ct: f64,
}

impl Vibrato {
    /// A 1 s mono saw at 220 Hz.
    pub(crate) fn mono(rate_hz: f64, extent_ct: f64) -> Self {
        Self {
            f0_hz: 220.0,
            rate_hz,
            extent_ct,
            length_s: 1.0,
            lead_s: 0.2,
            voices: 1,
            detune_ct: 0.0,
        }
    }

    /// The same on a 7-voice unison detuned over ±25 cents.
    pub(crate) fn unison(rate_hz: f64, extent_ct: f64) -> Self {
        Self {
            voices: 7,
            detune_ct: 25.0,
            ..Self::mono(rate_hz, extent_ct)
        }
    }

    pub(crate) fn render(&self, rate: f64) -> Vec<f64> {
        let me = *self;
        let lead = round_to_usize(rate * me.lead_s);
        let length = round_to_usize(rate * me.length_s);
        let f = move |t: f64| {
            me.f0_hz * 2f64.powf(me.extent_ct * (TAU * me.rate_hz * t).sin() / 1200.0)
        };
        let top = me.f0_hz * 2f64.powf(me.extent_ct / 1200.0);
        let tone = unison(f, top, rate, length, (me.voices, me.detune_ct), false);
        let mut out = vec![0.0; lead];
        out.extend(tone);
        out.extend(std::iter::repeat_n(0.0, lead));
        out
    }
}

/// A saw that alternates between `f0_hz` and a semitone above it,
/// `rate_hz` times a second, for `length_s` after `lead_s` of silence.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Trill {
    pub(crate) f0_hz: f64,
    pub(crate) rate_hz: f64,
    pub(crate) length_s: f64,
    pub(crate) lead_s: f64,
}

impl Trill {
    /// A 1 s trill at 220 Hz.
    pub(crate) fn new(rate_hz: f64) -> Self {
        Self {
            f0_hz: 220.0,
            rate_hz,
            length_s: 1.0,
            lead_s: 0.2,
        }
    }

    pub(crate) fn render(&self, rate: f64) -> Vec<f64> {
        let me = *self;
        let lead = round_to_usize(rate * me.lead_s);
        let f = move |t: f64| {
            let upper = (2.0 * me.rate_hz * t).floor() % 2.0;
            me.f0_hz * 2f64.powf(upper / 12.0)
        };
        let top = me.f0_hz * 2f64.powf(1.0 / 12.0);
        let mut out = vec![0.0; lead];
        out.extend(unison(
            f,
            top,
            rate,
            round_to_usize(rate * me.length_s),
            (1, 0.0),
            false,
        ));
        out.extend(std::iter::repeat_n(0.0, lead));
        out
    }
}

/// A bass line: an 808 drop to E1 at 0.25 s that decays until 1.0 s, then
/// a saw A1 from 1.2 s that glides up 5 semitones in 150 ms at 1.7 s and
/// holds D2 until 2.5 s, with a 50 ms fade at each end.
#[derive(Clone, Copy, Debug)]
pub(crate) struct BassLine;

impl BassLine {
    /// The notes as a MIDI file would give them: onset, offset, pitch.
    pub(crate) const NOTES: [(f64, f64, f64); 3] =
        [(0.25, 1.0, 28.0), (1.2, 1.7, 33.0), (1.7, 2.5, 38.0)];
    /// The drop.
    pub(crate) const DROP: (f64, f64, f64) = (41.2, 41.2, 0.02);
    /// The glide: start, semitones, full time, power.
    pub(crate) const GLIDE: (f64, f64, f64, f64) = (1.7, 5.0, 0.15, 1.0);

    pub(crate) fn render(rate: f64) -> Vec<f64> {
        let (f_end, df, tau) = Self::DROP;
        let mut out: Vec<f64> = PitchDrop {
            f_end_hz: f_end,
            df_hz: df,
            tau_s: tau,
            decay_s: 0.5,
            lead_s: 0.25,
            length_s: 1.0,
        }
        .render(rate);
        let fade = |t: f64, from: f64, to: f64| {
            ((t - from) / 0.05).clamp(0.0, 1.0) * ((to - t) / 0.05).clamp(0.0, 1.0)
        };
        for (n, x) in out.iter_mut().enumerate() {
            *x *= fade(f64::from(u32::try_from(n).unwrap()) / rate, 0.0, 1.0);
        }
        out.resize(round_to_usize(1.2 * rate), 0.0);
        let (start, semitones, full, p) = Self::GLIDE;
        let f =
            move |t: f64| 55.0 * 2f64.powf(semitones * curve((t + 1.2 - start) / full, p) / 12.0);
        let length = round_to_usize(1.3 * rate);
        let saw = unison(
            f,
            55.0 * 2f64.powf(semitones / 12.0),
            rate,
            length,
            (1, 0.0),
            false,
        );
        out.extend(
            saw.iter().enumerate().map(|(n, x)| {
                0.8 * x * fade(f64::from(u32::try_from(n).unwrap()) / rate, 0.0, 1.3)
            }),
        );
        out.resize(round_to_usize(3.0 * rate), 0.0);
        out
    }
}
