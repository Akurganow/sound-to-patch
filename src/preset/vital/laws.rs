//! Vital laws that span several controls: unison spread, LFO tempo rates,
//! pan gains, modulation depth and the amplitude path.
//!
//! Each law is read from the Vital 1.0.6 open source and restated here.
//! Where the reading joins several facts, the function says so; a probe or
//! a render confirms it before a mapper relies on it.

use crate::cast::usize_to_f64;
use crate::units::{Cents, Hz, MidiPitch, RangeError, Semitones};

/// The note-number frequency law: `8.1757989156 * 2^(note / 12)` Hz.
pub const MIDI_0_HZ: f64 = 8.175_798_915_6;

/// Vital's power curve: `(e^(p*t) - 1) / (e^p - 1)`, or `t` when
/// `|p| < 0.01`. It maps 0 to 0 and 1 to 1 and bends in between.
#[must_use]
pub fn power_scale(t: f64, power: f64) -> f64 {
    if power.abs() < 0.01 {
        t
    } else {
        (power * t).exp_m1() / power.exp_m1()
    }
}

/// The offset of the outermost unison voice: `detune_range * detune^2`
/// cents, with `detune` the stored `osc_N_unison_detune` and
/// `detune_range` the stored `osc_N_detune_range`.
///
/// # Errors
///
/// A [`RangeError`] when the result is not finite.
pub fn unison_outer_offset(detune_range: f64, detune: f64) -> Result<Cents, RangeError> {
    Cents::new(detune_range * detune * detune)
}

/// The offsets of `voices` unison voices as fractions of the outer offset,
/// from lowest to highest.
///
/// Position `i` of `ceil(voices / 2)` sits at
/// `t = (2i + bump) / max(1, voices - 1)`, with `bump` 1 for an even count.
/// Its offset is `power_scale(t, power)`, one voice up and one down; an odd
/// count puts one voice at the centre. The pairing and the centre voice
/// are a reading of the source; probe 4 of the probe list confirms them.
#[must_use]
pub fn unison_offsets(voices: u8, power: f64) -> Vec<f64> {
    let n = usize::from(voices.max(1));
    let bump = usize::from(n % 2 == 0);
    let divisor = usize_to_f64(n.saturating_sub(1).max(1));
    let mut offsets = Vec::with_capacity(n);
    for i in 0..n.div_ceil(2) {
        let fraction = power_scale(usize_to_f64(2 * i + bump) / divisor, power);
        if fraction == 0.0 {
            offsets.push(0.0);
        } else {
            offsets.extend([-fraction, fraction]);
        }
    }
    offsets.sort_by(f64::total_cmp);
    offsets
}

/// The gains of the centre and the detuned unison voices.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnisonGains {
    /// Gain of the centre voice.
    pub centre: f64,
    /// Gain of each detuned voice.
    pub detuned: f64,
    /// The pair count the normalisation uses.
    pub pairs: usize,
}

/// The level compensation of unison voices at `blend` (stored 0 to 1).
///
/// Up to 2 voices the centre has gain 1 and the rest 0. Above that the
/// centre is `lerp(1, 0.4, blend)` and each detuned voice
/// `lerp(0.6, 0, (1 - blend)^2)`, both scaled so that
/// `centre^2 + detuned^2 * (pairs - 1) = 1`. `pairs` is half the active
/// voices; taking it as `ceil(voices / 2)` is a reading of the source.
#[must_use]
pub fn unison_gains(voices: u8, blend: f64) -> UnisonGains {
    let pairs = usize::from(voices).div_ceil(2);
    if voices <= 2 {
        return UnisonGains {
            centre: 1.0,
            detuned: 0.0,
            pairs,
        };
    }
    let centre = 1.0 + (0.4 - 1.0) * blend;
    let detuned = 0.6 * (1.0 - (1.0 - blend).powi(2));
    let norm = (centre * centre + detuned * detuned * usize_to_f64(pairs - 1))
        .sqrt()
        .recip();
    UnisonGains {
        centre: centre * norm,
        detuned: detuned * norm,
        pairs,
    }
}

/// How a tempo-synced LFO scales its note division.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TempoMode {
    /// The plain division.
    Tempo,
    /// A dotted division, 2/3 of the rate.
    Dotted,
    /// A triplet division, 3/2 of the rate.
    Triplets,
}

/// The rate of a tempo-synced LFO.
///
/// Index `i` of `lfo_N_tempo` (0 to 12) gives `2^(i - 8)` cycles per beat,
/// times the mode factor, times the beat rate. Index 0 freezes the LFO at
/// 0 Hz. At 120 BPM the beat rate is 2 Hz, so index 8 (`1/4`) runs at 2 Hz.
///
/// # Errors
///
/// A [`RangeError`] for an index above 12.
pub fn lfo_tempo_rate(index: u8, mode: TempoMode, beat_rate: Hz) -> Result<Hz, RangeError> {
    if index == 0 {
        return Ok(Hz::from_const(0.0));
    }
    let index =
        crate::units::Range::Closed(0.0, 12.0).check("LFO tempo index", f64::from(index))?;
    let factor = match mode {
        TempoMode::Tempo => 1.0,
        TempoMode::Dotted => 2.0 / 3.0,
        TempoMode::Triplets => 1.5,
    };
    Hz::new((index - 8.0).exp2() * factor * beat_rate.get())
}

/// The rate of a key-tracked LFO: the note-number law applied to
/// `transpose + tune + note`.
///
/// # Errors
///
/// A [`RangeError`] when the result is not finite.
pub fn lfo_keytrack_rate(
    note: MidiPitch,
    transpose: Semitones,
    tune: Cents,
) -> Result<Hz, RangeError> {
    let semitones = note.get() + transpose.get() + tune.get() / 100.0;
    Hz::new(MIDI_0_HZ * (semitones / 12.0).exp2())
}

/// The left and right gains of a pan position `pan` (stored -1 to 1).
///
/// The law is `sqrt(2) * sin(2 pi (1/8 -+ pan/8))`, so the centre gives 1
/// on both channels. The engine uses a polynomial sine, so its gains differ
/// from these by up to about 5e-3.
#[must_use]
pub fn pan_gains(pan: f64) -> (f64, f64) {
    let angle = |x: f64| std::f64::consts::SQRT_2 * (std::f64::consts::TAU * x).sin();
    (angle(0.125 - pan / 8.0), angle(0.125 + pan / 8.0))
}

/// The change a modulation route adds to the stored value of its
/// destination: `(source - 0.5 * bipolar) * amount * (max - min)`. It adds
/// before the engine scale of the destination is applied.
#[must_use]
pub fn modulation_delta(source: f64, amount: f64, bipolar: bool, stored_span: f64) -> f64 {
    let centre = if bipolar { 0.5 } else { 0.0 };
    (source - centre) * amount * stored_span
}

/// The voice gain after the amplitude envelope:
/// `(envelope * lerp(1, velocity, velocity_track) * voice_amplitude)^2`.
///
/// The square means that a measured RMS contour maps to the square root of
/// the envelope level. That reading is a probe target, not a checked fact.
#[must_use]
pub fn amp_gain(envelope: f64, velocity: f64, velocity_track: f64, voice_amplitude: f64) -> f64 {
    let velocity_mix = 1.0 + (velocity - 1.0) * velocity_track;
    (envelope * velocity_mix * voice_amplitude).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    /// `(e^(1.5 t) - 1) / (e^1.5 - 1)`, the default power curve.
    fn curve(t: f64) -> f64 {
        ((1.5 * t).exp() - 1.0) / (1.5f64.exp() - 1.0)
    }

    fn assert_offsets(voices: u8, expected: &[f64]) {
        let got = unison_offsets(voices, 1.5);
        assert_eq!(got.len(), expected.len(), "{voices} voices: {got:?}");
        for (got, want) in got.iter().zip(expected) {
            assert_relative_eq!(*got, *want, epsilon = 1e-12, max_relative = 1e-12);
        }
    }

    #[test]
    fn unison_offsets_match_the_formula() {
        assert_offsets(1, &[0.0]);
        assert_offsets(2, &[-1.0, 1.0]);
        assert_offsets(3, &[-1.0, 0.0, 1.0]);
        let third = curve(1.0 / 3.0);
        assert_offsets(4, &[-1.0, -third, third, 1.0]);
        let (inner, outer) = (curve(2.0 / 6.0), curve(4.0 / 6.0));
        assert_offsets(7, &[-1.0, -outer, -inner, 0.0, inner, outer, 1.0]);
        let (p1, p3, p5) = (curve(1.0 / 7.0), curve(3.0 / 7.0), curve(5.0 / 7.0));
        assert_offsets(8, &[-1.0, -p5, -p3, -p1, p1, p3, p5, 1.0]);
    }

    #[test]
    fn unison_offsets_round_to_the_fact_table() {
        let round = |x: f64| (x * 1000.0).round() / 1000.0;
        let positive = |voices| -> Vec<f64> {
            unison_offsets(voices, 1.5)
                .into_iter()
                .filter(|x| *x >= 0.0)
                .map(round)
                .collect()
        };
        assert_eq!(positive(4), [0.186, 1.0]);
        assert_eq!(positive(7), [0.0, 0.186, 0.494, 1.0]);
        assert_eq!(positive(8), [0.069, 0.259, 0.551, 1.0]);
    }

    #[test]
    fn outer_offset_is_range_times_detune_squared() {
        // 2 * 4.472135955^2 = 40 cents; 48 * 10^2 = 4800 cents.
        assert_relative_eq!(
            unison_outer_offset(2.0, 4.472_135_955).unwrap().get(),
            40.0,
            max_relative = 1e-9
        );
        assert_relative_eq!(
            unison_outer_offset(48.0, 10.0).unwrap().get(),
            4800.0,
            max_relative = 1e-12
        );
    }

    #[test]
    fn unison_gains_are_normalised() {
        for voices in 3..=16 {
            for blend in [0.0, 0.3, 0.8, 1.0] {
                let gains = unison_gains(voices, blend);
                let sum =
                    gains.centre.powi(2) + gains.detuned.powi(2) * usize_to_f64(gains.pairs - 1);
                assert_relative_eq!(sum, 1.0, epsilon = 1e-12);
            }
        }
        assert_eq!(unison_gains(2, 0.8).centre, 1.0);
        assert_eq!(unison_gains(2, 0.8).detuned, 0.0);
    }

    #[test]
    fn tempo_rates_follow_the_division() {
        let beat = Hz::new(2.0).unwrap();
        assert_relative_eq!(
            lfo_tempo_rate(8, TempoMode::Tempo, beat).unwrap().get(),
            2.0,
            max_relative = 1e-12
        );
        assert_relative_eq!(
            lfo_tempo_rate(6, TempoMode::Tempo, beat).unwrap().get(),
            0.5,
            max_relative = 1e-12
        );
        assert_relative_eq!(
            lfo_tempo_rate(8, TempoMode::Dotted, beat).unwrap().get(),
            4.0 / 3.0,
            max_relative = 1e-12
        );
        assert_relative_eq!(
            lfo_tempo_rate(8, TempoMode::Triplets, beat).unwrap().get(),
            3.0,
            max_relative = 1e-12
        );
        assert_eq!(
            lfo_tempo_rate(0, TempoMode::Tempo, beat).unwrap().get(),
            0.0
        );
        assert!(lfo_tempo_rate(13, TempoMode::Tempo, beat).is_err());
    }

    #[test]
    fn keytrack_rate_follows_the_note_law() {
        // 8.1757989156 * 2^((69 - 12 + 0) / 12) = 220 Hz
        let rate = lfo_keytrack_rate(
            MidiPitch::new(69.0).unwrap(),
            Semitones::new(-12.0).unwrap(),
            Cents::new(0.0).unwrap(),
        )
        .unwrap();
        assert_relative_eq!(rate.get(), 220.0, max_relative = 1e-6);
    }

    #[test]
    fn pan_gains_follow_the_sine_law() {
        let (l, r) = pan_gains(0.0);
        assert_relative_eq!(l, 1.0, epsilon = 5e-3);
        assert_relative_eq!(r, 1.0, epsilon = 5e-3);
        let (l, r) = pan_gains(-1.0);
        assert_relative_eq!(l, 1.414, epsilon = 5e-3);
        assert_relative_eq!(r, 0.0, epsilon = 5e-3);
        let (l, r) = pan_gains(0.5);
        assert_relative_eq!(l, 0.541, epsilon = 5e-3);
        assert_relative_eq!(r, 1.307, epsilon = 5e-3);
    }

    #[test]
    fn modulation_depth_scales_by_the_stored_span() {
        // (1 - 0) * 0.25 * (136 - 8) = 32 semitones
        assert_relative_eq!(
            modulation_delta(1.0, 0.25, false, 128.0),
            32.0,
            max_relative = 1e-12
        );
        assert_relative_eq!(
            modulation_delta(1.0, 0.25, true, 128.0),
            16.0,
            max_relative = 1e-12
        );
        assert_relative_eq!(
            modulation_delta(0.0, 0.25, true, 128.0),
            -16.0,
            max_relative = 1e-12
        );
    }

    #[test]
    fn power_scale_is_linear_near_zero_power() {
        assert_eq!(power_scale(0.3, 0.005), 0.3);
        assert_relative_eq!(power_scale(1.0, -2.0), 1.0, max_relative = 1e-12);
        assert_relative_eq!(power_scale(0.5, 1.5), curve(0.5), max_relative = 1e-12);
    }

    #[test]
    fn amp_gain_squares_the_envelope() {
        // (0.5 * lerp(1, 0.7, 1) * 1)^2 = 0.1225
        assert_relative_eq!(amp_gain(0.5, 0.7, 1.0, 1.0), 0.1225, max_relative = 1e-12);
        assert_relative_eq!(amp_gain(0.5, 0.7, 0.0, 1.0), 0.25, max_relative = 1e-12);
    }
}
