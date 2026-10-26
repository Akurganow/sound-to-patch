//! The Lomb-Scargle periodogram for samples at irregular times.
//!
//! Active note frames leave gaps, and a plain FFT of a gappy track sees the
//! note rhythm as modulation. The periodogram fits a sine and a cosine at
//! each trial frequency to the samples that exist.

use std::f64::consts::TAU;

use crate::cast;
use crate::confidence::{ConfidenceError, Estimate};
use crate::notes::NoteList;
use crate::units::{Decibels, Hz, Seconds};

use super::frames::ACTIVE_GATE_DB;
use super::modulation::{SUM_HARMONICS, Track, pick_fundamental, rate_estimate};

/// The step of the trial frequencies.
pub const GRID_STEP_HZ: Hz = Hz::from_const(0.01);
/// The settling time after an onset whose samples are left out.
pub const SETTLE_S: Seconds = Seconds::from_const(0.06);

/// The trial frequencies from `lo` to `hi` in 0.01 Hz steps.
#[must_use]
pub fn grid(lo: Hz, hi: Hz) -> Vec<Hz> {
    let first = cast::ceil_to_usize(lo.get() / GRID_STEP_HZ.get()).unwrap_or(0);
    let last = cast::floor_to_usize(hi.get() / GRID_STEP_HZ.get() + 1e-9).unwrap_or(0);
    (first..=last)
        .map(|i| Hz::clamped(cast::usize_to_f64(i) * GRID_STEP_HZ.get()))
        .collect()
}

/// The periodogram of `values` at `times`, one power per frequency.
///
/// The power is in the legacy units `A^2 N / 4` for a sine of amplitude `A`
/// over `N` samples. The values are centred on their mean first, so a
/// constant offset changes nothing.
#[must_use]
pub fn lomb_scargle(times: &[Seconds], values: &[f64], frequencies: &[Hz]) -> Vec<f64> {
    let len = times.len().min(values.len());
    if len == 0 {
        return vec![0.0; frequencies.len()];
    }
    let n = cast::usize_to_f64(len);
    let mean = values[..len].iter().sum::<f64>() / n;
    let centred: Vec<f64> = values[..len].iter().map(|v| v - mean).collect();
    let epsilon = f64::EPSILON / 2.0;
    frequencies
        .iter()
        .map(|frequency| {
            let omega = TAU * frequency.get();
            let (mut cc, mut cs) = (0.0, 0.0);
            for time in &times[..len] {
                let (sin, cos) = (omega * time.get()).sin_cos();
                cc += cos * cos;
                cs += cos * sin;
            }
            let (cc, cs) = (cc / n, cs / n);
            let tau = 0.5 * (2.0 * cs).atan2(cc - (1.0 - cc));
            let (mut yc, mut ys, mut cc_tau) = (0.0, 0.0, 0.0);
            for (time, y) in times[..len].iter().zip(&centred) {
                let (sin, cos) = (omega * time.get() - tau).sin_cos();
                yc += y * cos;
                ys += y * sin;
                cc_tau += cos * cos;
            }
            let (yc, ys, cc_tau) = (yc / n, ys / n, (cc_tau / n).max(epsilon));
            let ss_tau = (1.0 - cc_tau).max(epsilon);
            2.0 * (yc * yc / cc_tau + ys * ys / ss_tau) * n / 4.0
        })
        .collect()
}

/// The samples of a level track that a modulation estimate may use: level
/// within 40 dB of the track's peak, inside a note, and at least `settle`
/// after the note's onset.
#[must_use]
pub fn gated_samples(track: &Track, notes: &NoteList, settle: Seconds) -> (Vec<Seconds>, Vec<f64>) {
    let peak = track.values().iter().copied().fold(0.0_f64, f64::max);
    let gate = peak
        * Decibels::clamped(-ACTIVE_GATE_DB.get())
            .to_amplitude_gain()
            .unwrap_or(0.0);
    let rate = track.rate().get();
    track
        .values()
        .iter()
        .enumerate()
        .filter_map(|(i, &value)| {
            let time = Seconds::clamped(cast::usize_to_f64(i) / rate);
            let settled = notes.notes().iter().any(|note| {
                note.sounds_at(time) && time.get() - note.onset().get() >= settle.get()
            });
            (value > 0.0 && value >= gate && settled).then_some((time, value))
        })
        .unzip()
}

/// The modulation rate of irregular samples between `lo` and `hi`.
///
/// The harmonic sum of the periodogram amplitude picks it, as in the
/// modulation spectrum. No estimate when the span holds fewer than two
/// cycles. The spread is the resolution, `1 / T` of the span.
///
/// # Errors
///
/// Never in practice; the reasons are built from counts.
pub fn periodogram_rate(
    times: &[Seconds],
    values: &[f64],
    lo: Hz,
    hi: Hz,
) -> Result<Estimate<Hz, Hz>, ConfidenceError> {
    let span = match (times.first(), times.last()) {
        (Some(first), Some(last)) => Seconds::clamped(last.get() - first.get()),
        _ => Seconds::clamped(0.0),
    };
    let top = Hz::clamped(hi.get() * cast::usize_to_f64(SUM_HARMONICS));
    let all = grid(lo, top);
    let amplitude: Vec<f64> = lomb_scargle(times, values, &all)
        .iter()
        .map(|p| p.max(0.0).sqrt())
        .collect();
    let first = all.first().map_or(0.0, |f| f.get());
    let at = |frequency: f64| {
        cast::round_to_usize((frequency - first) / GRID_STEP_HZ.get())
            .and_then(|i| amplitude.get(i))
            .copied()
            .unwrap_or(0.0)
    };
    let points: Vec<(f64, f64)> = all
        .iter()
        .zip(&amplitude)
        .filter(|(f, _)| f.get() <= hi.get())
        .map(|(f, a)| (f.get(), *a))
        .collect();
    let rate = pick_fundamental(&points, at)
        .filter(|_| span.get() > 0.0)
        .map(Hz::clamped);
    rate_estimate(rate, span)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::Note;
    use crate::units::MidiPitch;

    #[test]
    fn the_constants_pass_the_unit_check() {
        assert_eq!(GRID_STEP_HZ, Hz::new(0.01).unwrap());
        assert_eq!(SETTLE_S, Seconds::new(0.06).unwrap());
    }

    #[test]
    fn the_grid_steps_by_one_hundredth_of_a_hertz() {
        let g = grid(Hz::new(0.2).unwrap(), Hz::new(0.25).unwrap());
        let values: Vec<f64> = g.iter().map(|f| f.get()).collect();
        assert_eq!(values.len(), 6);
        assert!((values[5] - 0.25).abs() < 1e-12);
    }

    #[test]
    fn a_sine_has_power_a_squared_n_over_4_at_its_frequency() {
        let times: Vec<Seconds> = (0..400)
            .map(|i| Seconds::new(cast::usize_to_f64(i) / 200.0).unwrap())
            .collect();
        let values: Vec<f64> = times
            .iter()
            .map(|t| 2.0 * (TAU * 5.0 * t.get()).cos())
            .collect();
        let power = lomb_scargle(&times, &values, &[Hz::new(5.0).unwrap()]);
        assert!((power[0] - 4.0 * 400.0 / 4.0).abs() < 1e-9, "{power:?}");
    }

    #[test]
    fn gated_samples_skip_silence_gaps_and_the_settling_time() {
        let track = Track::new(vec![1.0; 200], Hz::new(200.0).unwrap());
        let note = Note::new(
            Seconds::new(0.5).unwrap(),
            Seconds::new(0.8).unwrap(),
            MidiPitch::new(60.0).unwrap(),
        )
        .unwrap();
        let (times, _) = gated_samples(&track, &NoteList::new(vec![note]), SETTLE_S);
        assert_eq!(times.first().map(|t| t.get()), Some(0.56));
        assert_eq!(times.len(), 48);
    }
}
