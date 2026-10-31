//! Level tracks on a 1 ms hop: the centred RMS of the full band and the
//! zero-phase magnitude of single harmonics.

use crate::audio::SampleRate;
use crate::cast;
use crate::dsp::{FilterError, band_magnitude};
use crate::units::{Hz, Seconds};

/// The hop of every envelope track.
pub const HOP: Seconds = Seconds::from_const(0.001);
/// The RMS window of a note spans this many periods of its fundamental.
///
/// A whole number of periods of a periodic wave leaves no ripple, and one
/// period follows an attack best. On generated saws a window of two
/// periods read the 10-90 % rise of a 20 ms attack at 55 Hz as 34 ms and
/// one period as 19.4 ms; at 220 Hz both read 18 ms for 16 ms.
pub const RMS_PERIODS: f64 = 1.0;
/// The RMS window of the pump spans this many periods of the lowest note.
///
/// A chord is not periodic in one period of its lowest note, and one
/// period lets its beating through: on the pump's saw chord the floor rose
/// from 1.8 to 3.7 dB.
pub const PUMP_RMS_PERIODS: f64 = 2.0;
/// The RMS window spans at least this long, in whole periods.
///
/// A window of whole periods leaves no ripple, and a short one follows an
/// attack best. On generated saws a floor of 5 ms read the 10-90 % rise of
/// a 10 ms attack at 440 Hz as 9.8 ms for 8 ms, and 2 ms read 7.6 ms; at
/// 220 Hz, where 5 ms is 1.1 periods, a 20 ms attack read 17.7 and 14.9
/// ms for 16 ms.
pub const RMS_WINDOW_MIN: Seconds = Seconds::from_const(0.002);
/// The low-pass of a harmonic track sits at most this far from the
/// harmonic. A wider band follows an attack faster.
pub const HARMONIC_CUTOFF_MAX: Hz = Hz::from_const(40.0);
/// The low-pass of a harmonic track sits at least this far from the
/// harmonic. A 12 Hz low-pass smears a rise by about 30 ms.
pub const HARMONIC_CUTOFF_MIN: Hz = Hz::from_const(12.0);

/// The level floor of a track, in dB: digital silence reads this.
const FLOOR_DB: f64 = -200.0;

/// A level track in dB, one value per [`HOP`] from `start`.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelTrack {
    start: Seconds,
    db: Vec<f64>,
    window: Seconds,
}

impl LevelTrack {
    /// Pairs dB values with the time of the first.
    #[must_use]
    pub fn new(start: Seconds, db: Vec<f64>) -> Self {
        Self {
            start,
            db,
            window: Seconds::clamped(0.0),
        }
    }

    /// The centred RMS of `samples` in dB, from `from` to `to`, over the
    /// window of [`rms_window`]. `samples` starts at sample `first` of the
    /// stem, so a note reads only its own span. The window shrinks at the
    /// ends of the samples.
    #[must_use]
    pub fn rms(
        samples: &[f64],
        first: usize,
        rate: SampleRate,
        (from, to): (Seconds, Seconds),
        f0: Hz,
        periods: f64,
    ) -> Self {
        let fs = rate.hz().get();
        let window = rms_window(f0, periods);
        let half = cast::round_to_usize(0.5 * window.get() * fs)
            .unwrap_or(0)
            .max(1);
        let mut sums = Vec::with_capacity(samples.len() + 1);
        sums.push(0.0);
        let mut total = 0.0;
        for x in samples {
            total += x * x;
            sums.push(total);
        }
        let db = frame_times(from, to)
            .map(|t| {
                let centre = cast::round_to_usize(t * fs)
                    .unwrap_or(0)
                    .saturating_sub(first);
                let lo = centre.saturating_sub(half).min(samples.len());
                let hi = (centre + half).min(samples.len());
                if hi <= lo {
                    return FLOOR_DB;
                }
                power_db((sums[hi] - sums[lo]) / cast::usize_to_f64(hi - lo))
            })
            .collect();
        Self {
            start: from,
            db,
            window,
        }
    }

    /// Returns the RMS window of the track, 0 s when it has none.
    #[must_use]
    pub fn window(&self) -> Seconds {
        self.window
    }

    /// The summed power of single partials in dB, from `from` to `to`.
    ///
    /// Each partial is a `(frequency, cutoff)` pair: the zero-phase band
    /// magnitude of [`band_magnitude`] over `frequency ± cutoff`, so a
    /// track has no lag. `samples` starts at `offset` in the stem.
    ///
    /// # Errors
    ///
    /// Returns a [`FilterError`] when a band reaches 0 Hz or the Nyquist
    /// frequency.
    pub fn partials(
        samples: &[f64],
        offset: Seconds,
        rate: SampleRate,
        (from, to): (Seconds, Seconds),
        partials: &[(Hz, Hz)],
    ) -> Result<Self, FilterError> {
        let fs = rate.hz().get();
        let mut power = vec![0.0; frame_times(from, to).count()];
        for &(frequency, cutoff) in partials {
            let magnitude = band_magnitude(
                samples,
                rate,
                Hz::clamped(frequency.get() - cutoff.get()),
                Hz::clamped(frequency.get() + cutoff.get()),
            )?;
            for (p, t) in power.iter_mut().zip(frame_times(from, to)) {
                let i = cast::round_to_usize((t - offset.get()) * fs).unwrap_or(0);
                if let Some(m) = magnitude.get(i) {
                    // A partial of amplitude A reads A; its power is A^2 / 2,
                    // as the RMS track reads it.
                    *p += 0.5 * m * m;
                }
            }
        }
        Ok(Self {
            start: from,
            db: power.into_iter().map(power_db).collect(),
            window: Seconds::clamped(0.0),
        })
    }

    /// The magnitude of one harmonic in dB, from `from` to `to`: a sliding
    /// DFT at `frequency` over one period of `f0`, centred on each frame.
    ///
    /// The window spans a whole period of the fundamental, so every other
    /// harmonic of the note falls on a null, and its step response rises
    /// without overshoot, unlike a Butterworth low-pass. A partial of
    /// amplitude A reads A, its power A^2 / 2 as the RMS track reads it.
    /// `samples` starts at sample `first` of the stem. At the start of the
    /// samples the window keeps its whole period and runs from the first
    /// sample, so the nulls hold; at their end it shrinks.
    #[must_use]
    pub fn harmonic(
        samples: &[f64],
        first: usize,
        rate: SampleRate,
        (from, to): (Seconds, Seconds),
        frequency: Hz,
        f0: Hz,
    ) -> Self {
        let fs = rate.hz().get();
        let len = cast::round_to_usize(fs / f0.get().max(f64::MIN_POSITIVE))
            .unwrap_or(1)
            .max(1);
        let step = std::f64::consts::TAU * frequency.get() / fs;
        let (mut re, mut im) = (vec![0.0; samples.len() + 1], vec![0.0; samples.len() + 1]);
        for (n, x) in samples.iter().enumerate() {
            // Reduce the phase per sample so it stays exact on long stems.
            let (sin, cos) =
                ((step * cast::usize_to_f64(first + n)) % std::f64::consts::TAU).sin_cos();
            re[n + 1] = re[n] + x * cos;
            im[n + 1] = im[n] - x * sin;
        }
        let db = frame_times(from, to)
            .map(|t| {
                let centre = cast::round_to_usize(t * fs)
                    .unwrap_or(0)
                    .saturating_sub(first);
                let lo = centre.saturating_sub(len / 2).min(samples.len());
                let hi = (lo + len).min(samples.len());
                if hi <= lo {
                    return FLOOR_DB;
                }
                let n = cast::usize_to_f64(hi - lo);
                let amplitude = 2.0 * (re[hi] - re[lo]).hypot(im[hi] - im[lo]) / n;
                power_db(0.5 * amplitude * amplitude)
            })
            .collect();
        Self {
            start: from,
            db,
            window: Seconds::clamped(0.0),
        }
    }

    /// Returns the time of the first value.
    #[must_use]
    pub fn start(&self) -> Seconds {
        self.start
    }

    /// Returns the dB values.
    #[must_use]
    pub fn db(&self) -> &[f64] {
        &self.db
    }

    /// Returns the dB values to change in place.
    pub fn db_mut(&mut self) -> &mut [f64] {
        &mut self.db
    }

    /// Returns the time of value `i`.
    #[must_use]
    pub fn time(&self, i: usize) -> f64 {
        self.start.get() + cast::usize_to_f64(i) * HOP.get()
    }

    /// Returns the index of the value at or after time `t`, clamped to the
    /// track.
    #[must_use]
    pub fn index(&self, t: f64) -> usize {
        cast::ceil_to_usize((t - self.start.get()) / HOP.get() - 1e-9)
            .unwrap_or(0)
            .min(self.db.len())
    }
}

/// The RMS window over `periods` periods of `f0`, or the fewest whole
/// periods that span [`RMS_WINDOW_MIN`] when that is more.
#[must_use]
pub fn rms_window(f0: Hz, periods: f64) -> Seconds {
    let f0 = f0.get().max(f64::MIN_POSITIVE);
    let whole = (RMS_WINDOW_MIN.get() * f0).ceil();
    Seconds::clamped(periods.max(whole) / f0)
}

/// The frame times from `from` up to `to`, one per [`HOP`].
fn frame_times(from: Seconds, to: Seconds) -> impl Iterator<Item = f64> {
    let n = cast::floor_to_usize((to.get() - from.get()) / HOP.get() + 1e-9).map_or(0, |n| n + 1);
    (0..n).map(move |i| from.get() + cast::usize_to_f64(i) * HOP.get())
}

/// A mean power in dB, floored at digital silence.
fn power_db(power: f64) -> f64 {
    if power > 0.0 {
        (10.0 * power.log10()).max(FLOOR_DB)
    } else {
        FLOOR_DB
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    #[test]
    fn the_constants_pass_the_unit_check() {
        assert_eq!(HOP, Seconds::new(0.001).unwrap());
        assert_eq!(RMS_WINDOW_MIN, Seconds::new(0.002).unwrap());
        assert_eq!(HARMONIC_CUTOFF_MAX, Hz::new(40.0).unwrap());
        assert_eq!(HARMONIC_CUTOFF_MIN, Hz::new(12.0).unwrap());
    }

    fn rate() -> SampleRate {
        SampleRate::new(44_100).unwrap()
    }

    fn sine(f: f64, amplitude: f64, seconds: f64) -> Vec<f64> {
        (0..cast::round_to_usize(seconds * 44_100.0).unwrap())
            .map(|n| amplitude * (TAU * f * cast::usize_to_f64(n) / 44_100.0).sin())
            .collect()
    }

    #[test]
    fn a_steady_sine_reads_its_rms_with_no_ripple() {
        let x = sine(110.0, 0.5, 0.5);
        let track = LevelTrack::rms(
            &x,
            0,
            rate(),
            (Seconds::new(0.1).unwrap(), Seconds::new(0.4).unwrap()),
            Hz::new(110.0).unwrap(),
            RMS_PERIODS,
        );
        assert_eq!(track.db().len(), 301);
        let expected = 10.0 * (0.125_f64).log10();
        for db in track.db() {
            assert!((db - expected).abs() < 0.05, "{db}");
        }
    }

    #[test]
    fn a_partial_track_reads_one_harmonic_of_two() {
        let x: Vec<f64> = sine(110.0, 0.5, 0.6)
            .iter()
            .zip(sine(220.0, 0.25, 0.6))
            .map(|(a, b)| a + b)
            .collect();
        let track = LevelTrack::partials(
            &x,
            Seconds::new(0.0).unwrap(),
            rate(),
            (Seconds::new(0.2).unwrap(), Seconds::new(0.4).unwrap()),
            &[(Hz::new(220.0).unwrap(), Hz::new(40.0).unwrap())],
        )
        .unwrap();
        let expected = 10.0 * (0.5 * 0.25 * 0.25_f64).log10();
        for db in track.db() {
            assert!((db - expected).abs() < 0.1, "{db}");
        }
    }

    #[test]
    fn a_harmonic_track_reads_one_harmonic_with_no_leak_from_the_others() {
        let x: Vec<f64> = sine(110.0, 0.5, 0.6)
            .iter()
            .zip(sine(220.0, 0.25, 0.6))
            .zip(sine(330.0, 0.4, 0.6))
            .map(|((a, b), c)| a + b + c)
            .collect();
        let track = LevelTrack::harmonic(
            &x,
            0,
            rate(),
            (Seconds::new(0.2).unwrap(), Seconds::new(0.4).unwrap()),
            Hz::new(220.0).unwrap(),
            Hz::new(110.0).unwrap(),
        );
        let expected = 10.0 * (0.5 * 0.25 * 0.25_f64).log10();
        for db in track.db() {
            assert!((db - expected).abs() < 0.05, "{db}");
        }
    }

    #[test]
    fn a_harmonic_track_reads_a_whole_period_at_the_start_of_the_samples() {
        // A 220 Hz sine from the first sample, with its second harmonic:
        // the frame at 0 s reads the fundamental alone, at amplitude 1.
        let fs = 22_050.0;
        let x: Vec<f64> = (0..2205)
            .map(|n| {
                let t = cast::usize_to_f64(n) / fs;
                (TAU * 220.0 * t).sin() + 0.5 * (TAU * 440.0 * t).sin()
            })
            .collect();
        let track = LevelTrack::harmonic(
            &x,
            0,
            SampleRate::new(22_050).unwrap(),
            (Seconds::clamped(0.0), Seconds::clamped(0.01)),
            Hz::new(220.0).unwrap(),
            Hz::new(220.0).unwrap(),
        );
        let want = 10.0 * 0.5_f64.log10();
        assert!((track.db()[0] - want).abs() < 0.1, "{}", track.db()[0]);
    }

    #[test]
    fn a_harmonic_track_is_centred_on_its_frame() {
        // A 110 Hz sine switched on at 0.2 s: the frame there sees half a
        // window of it, so it reads half the amplitude, 6 dB down.
        let mut x = sine(110.0, 0.5, 0.6);
        for v in &mut x[..8820] {
            *v = 0.0;
        }
        let track = LevelTrack::harmonic(
            &x,
            0,
            rate(),
            (Seconds::new(0.2).unwrap(), Seconds::new(0.4).unwrap()),
            Hz::new(110.0).unwrap(),
            Hz::new(110.0).unwrap(),
        );
        let full = 10.0 * (0.5 * 0.5 * 0.5_f64).log10();
        assert!(
            (track.db()[0] - (full - 6.02)).abs() < 0.3,
            "{}",
            track.db()[0]
        );
        assert!((track.db()[20] - full).abs() < 0.05, "{}", track.db()[20]);
    }

    #[test]
    fn the_window_is_whole_periods_of_at_least_2_ms() {
        let window = |f0: f64, periods: f64| rms_window(Hz::new(f0).unwrap(), periods).get();
        assert!((window(110.0, 1.0) - 1.0 / 110.0).abs() < 1e-12);
        assert!((window(110.0, 2.0) - 2.0 / 110.0).abs() < 1e-12);
        // 1.14 ms periods at 880 Hz: two of them span 2.27 ms.
        assert!((window(880.0, 1.0) - 2.0 / 880.0).abs() < 1e-12);
    }

    #[test]
    fn silence_reads_the_floor_and_times_step_by_the_hop() {
        let track = LevelTrack::rms(
            &vec![0.0; 4410],
            0,
            rate(),
            (Seconds::new(0.01).unwrap(), Seconds::new(0.02).unwrap()),
            Hz::new(100.0).unwrap(),
            RMS_PERIODS,
        );
        assert!(track.db().iter().all(|&db| db == FLOOR_DB));
        assert!((track.time(10) - 0.02).abs() < 1e-12);
        assert_eq!(track.index(0.0155), 6);
        assert_eq!(track.index(0.0150), 5);
    }
}
