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
use super::modulation::{Pick, Track, pick_rate, rate_estimate};

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
/// Each trial fits a mean, a sine and a cosine to the samples together:
/// the floating-mean form of Zechmeister and Kürster, the
/// `floating_mean=True` option of `scipy.signal.lombscargle`. The plain form subtracts the overall mean first, and on
/// gated notes a rate at the note rate then loses to its harmonics. The
/// power is in the legacy units `A^2 N / 4` for a sine of amplitude `A`
/// over `N` samples, and a constant offset changes nothing.
#[must_use]
pub fn lomb_scargle(times: &[Seconds], values: &[f64], frequencies: &[Hz]) -> Vec<f64> {
    let len = times.len().min(values.len());
    if len == 0 {
        return vec![0.0; frequencies.len()];
    }
    let n = cast::usize_to_f64(len);
    let (times, values) = (&times[..len], &values[..len]);
    let mean_y = values.iter().sum::<f64>() / n;
    let epsilon = f64::EPSILON / 2.0;
    frequencies
        .iter()
        .map(|frequency| {
            let omega = TAU * frequency.get();
            let (mut c, mut s, mut cc, mut cs) = (0.0, 0.0, 0.0, 0.0);
            for time in times {
                let (sin, cos) = (omega * time.get()).sin_cos();
                c += cos;
                s += sin;
                cc += cos * cos;
                cs += cos * sin;
            }
            let (c, s) = (c / n, s / n);
            let cc_full = cc / n - c * c;
            let ss_full = 1.0 - cc / n - s * s;
            let cs = cs / n - c * s;
            let tau = 0.5 * (2.0 * cs).atan2(cc_full - ss_full);
            let (mut yc, mut ys, mut c, mut s, mut cc) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for (time, y) in times.iter().zip(values) {
                let (sin, cos) = (omega * time.get() - tau).sin_cos();
                yc += y * cos;
                ys += y * sin;
                c += cos;
                s += sin;
                cc += cos * cos;
            }
            let (c, s) = (c / n, s / n);
            let yc = yc / n - mean_y * c;
            let ys = ys / n - mean_y * s;
            let ss = (1.0 - cc / n - s * s).max(epsilon);
            let cc = (cc / n - c * c).max(epsilon);
            2.0 * (yc * yc / cc + ys * ys / ss) * n / 4.0
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
/// The periodogram amplitude goes through the same pick as the modulation
/// spectrum. No estimate when no line stands out or the span holds fewer
/// than two cycles. The spread is the resolution, `1 / T` of the span.
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
    if span.get() <= 0.0 {
        return rate_estimate(Pick::NoLine { prominence: 0.0 }, span);
    }
    // The log level, so a line's amplitude is its relative depth as in the
    // modulation spectrum; `P = A^2 N / 4` gives `A = 2 sqrt(P / N)`.
    let logs: Vec<f64> = values
        .iter()
        .map(|v| v.max(f64::MIN_POSITIVE).ln())
        .collect();
    let n = cast::usize_to_f64(times.len().min(values.len()));
    // One guard step outside each end, see `pick_rate`.
    let frequencies = grid(
        Hz::clamped(lo.get() - GRID_STEP_HZ.get()),
        Hz::clamped(hi.get() + GRID_STEP_HZ.get()),
    );
    let points: Vec<(f64, f64)> = frequencies
        .iter()
        .zip(lomb_scargle(times, &logs, &frequencies))
        .map(|(f, p)| (f.get(), 2.0 * (p.max(0.0) / n).sqrt()))
        .collect();
    rate_estimate(pick_rate(&points), span)
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
