//! Vibrato: periodic pitch motion of 3 to 12 Hz, read from the f0 track.

use super::fit::golden_section;
use crate::cast;
use crate::units::{Cents, Hz, Seconds};

/// The slowest vibrato that counts.
pub const VIBRATO_RATE_MIN: Hz = Hz::from_const(3.0);
/// The fastest vibrato that counts.
pub const VIBRATO_RATE_MAX: Hz = Hz::from_const(12.0);
/// The share by which a rate may read outside 3 to 12 Hz and still count.
///
/// Through the f0 track the generated rates read within 0.8 % of the
/// truth, so a vibrato at either edge stays in.
pub const RATE_SLACK: f64 = 0.01;
/// The smallest extent that counts, after the correction.
pub const VIBRATO_EXTENT_MIN: Cents = Cents::from_const(10.0);
/// The shortest voiced span the search reads.
pub const VIBRATO_SPAN_MIN: Seconds = Seconds::from_const(0.5);
/// The smallest share of the detrended contour's power that the sine
/// must explain, so jitter does not read as a vibrato.
pub const VIBRATO_SHARE_MIN: f64 = 0.5;
/// The fewest cycles of the sine over the span.
///
/// Checked against an independent f0 on real stems, every vibrato of 2.5 cycles or more
/// matched it; the shorter ones, 1.6 to 2.4 cycles at 3 to 4.7 Hz, did not
/// or held under 2 cycles of it.
pub const VIBRATO_CYCLES_MIN: f64 = 2.5;
/// The search runs from this rate, or from 1.5 cycles over the span.
const SEARCH_LO: f64 = 1.0;
/// The search runs up to this rate.
const SEARCH_HI: f64 = 20.0;
/// The step of the search grid before the refinement.
const SEARCH_STEP: f64 = 0.01;

/// The convention of a vibrato, as `layers.json` names it.
pub const VIBRATO_CONVENTION: &str = "the voiced f0 frames of a steady span, a voiced run of \
the track split where two stable pitches lie 150 cents or more apart, without 50 ms at each end, \
without glides and without the settling of a drop, five time constants and at least 100 ms, in \
cents; start_s and end_s are its first and last frame in audio time; a least-squares line and sine fitted \
together, at the rate whose sine explains the most power beyond the line, searched from 1 Hz or \
1.5 cycles over the span up to 20 Hz in 0.01 Hz steps and refined by golden-section search; extent_raw_ct is that sine's amplitude, half the peak-to-peak swing; extent_ct is \
extent_raw_ct times extent_factor, which undoes the smoothing of the f0 track at that rate; a \
vibrato counts at 2.97 to 12.12 Hz, 3 to 12 Hz with 1 % slack, with extent_ct of 10 or more and a share, the power the sine \
explains over the power left after the line alone, of 0.5 or more, over 2.5 cycles or more";

/// A vibrato read from one span.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VibratoFit {
    /// The time of the first frame read: track time from [`fit_vibrato`],
    /// audio time in the pitch layer, which subtracts the track's lag.
    pub start: Seconds,
    /// The time of the last frame read, as `start`.
    pub end: Seconds,
    /// The rate.
    pub rate: Hz,
    /// The amplitude of the fitted sine.
    pub extent_raw: Cents,
    /// The factor that turns the raw extent into the extent.
    pub factor: f64,
    /// The share of the detrended contour's power the sine explains.
    pub share: f64,
    /// The kurtosis of the contour after the line: 1.5 for a sine, 1.0
    /// for a square wave.
    pub kurtosis: f64,
    /// The mean pitch of the span.
    pub pitch: Hz,
    /// The number of frames read.
    pub frames: usize,
}

impl VibratoFit {
    /// The corrected extent, `extent_raw * factor`.
    #[must_use]
    pub fn extent(&self) -> Cents {
        Cents::clamped(self.extent_raw.get() * self.factor)
    }

    /// The cycles of the sine over the span.
    #[must_use]
    pub fn cycles(&self) -> f64 {
        self.rate.get() * (self.end.get() - self.start.get())
    }

    /// Whether the fit counts as a vibrato.
    #[must_use]
    pub fn accepted(&self) -> bool {
        let rate = self.rate.get();
        (VIBRATO_RATE_MIN.get() * (1.0 - RATE_SLACK)..=VIBRATO_RATE_MAX.get() * (1.0 + RATE_SLACK))
            .contains(&rate)
            && self.extent() >= VIBRATO_EXTENT_MIN
            && self.share >= VIBRATO_SHARE_MIN
            && self.cycles() >= VIBRATO_CYCLES_MIN
    }
}

/// The raw extent the f0 track reads, as a share of the true extent, by
/// rate.
///
/// Measured on one generated 1 s mono saw at 220 Hz with ±35 cents
/// per rate, through the transcriber's notes and the track. The track's
/// frame of 93 ms smooths a fast swing, so the share falls with the rate.
/// Between the rates the share is interpolated linearly; outside them it
/// holds the end value.
pub const EXTENT_SHARE: [(f64, f64); 9] = [
    (3.0, 1.003),
    (4.0, 0.973),
    (5.0, 0.944),
    (5.5, 0.939),
    (6.0, 0.909),
    (7.0, 0.812),
    (8.0, 0.817),
    (10.0, 0.800),
    (12.0, 0.732),
];

/// The factor that undoes the track's smoothing at `rate`: one over the
/// interpolated [`EXTENT_SHARE`].
#[must_use]
pub fn extent_factor(rate: Hz) -> f64 {
    let r = rate.get();
    let share = match EXTENT_SHARE.iter().position(|&(at, _)| at >= r) {
        None => EXTENT_SHARE[EXTENT_SHARE.len() - 1].1,
        Some(0) => EXTENT_SHARE[0].1,
        Some(k) => {
            let ((r0, s0), (r1, s1)) = (EXTENT_SHARE[k - 1], EXTENT_SHARE[k]);
            s0 + (s1 - s0) * (r - r0) / (r1 - r0)
        }
    };
    1.0 / share
}

/// Fits a vibrato to voiced frames `(time, f0)` in time order. `None` when
/// the frames span less than [`VIBRATO_SPAN_MIN`] or hold no motion.
#[must_use]
pub fn fit_vibrato(frames: &[(Seconds, Hz)]) -> Option<VibratoFit> {
    let (first, last) = (frames.first()?, frames.last()?);
    let span = last.0.get() - first.0.get();
    if span < VIBRATO_SPAN_MIN.get() {
        return None;
    }
    let mut cents: Vec<f64> = frames
        .iter()
        .filter(|(_, f0)| f0.get() > 0.0)
        .map(|(_, f0)| 1200.0 * f0.get().log2())
        .collect();
    let times: Vec<f64> = frames
        .iter()
        .filter(|(_, f0)| f0.get() > 0.0)
        .map(|(t, _)| t.get() - first.0.get())
        .collect();
    let mean = cents.iter().sum::<f64>() / cast::usize_to_f64(cents.len());
    for c in &mut cents {
        *c -= mean;
    }
    let power = residual_power(&times, &cents);
    if power <= 0.0 {
        return None;
    }
    let explained = |rate: f64| sine_fit(&times, &cents, rate).0;
    let lo = SEARCH_LO.max(1.5 / span);
    let steps = cast::floor_to_usize((SEARCH_HI - lo) / SEARCH_STEP)?;
    let best = (0..=steps)
        .map(|k| lo + SEARCH_STEP * cast::usize_to_f64(k))
        .max_by(|a, b| explained(*a).total_cmp(&explained(*b)))?;
    let (rate, _) = golden_section(
        |rate| -explained(rate),
        (best - SEARCH_STEP).max(lo),
        (best + SEARCH_STEP).min(SEARCH_HI),
        1e-6,
    );
    let (explained_power, amplitude) = sine_fit(&times, &cents, rate);
    let kurtosis = kurtosis(&times, &cents);
    let rate = Hz::new(rate).ok()?;
    Some(VibratoFit {
        start: first.0,
        end: last.0,
        rate,
        extent_raw: Cents::new(amplitude).ok()?,
        factor: extent_factor(rate),
        share: (explained_power / power).min(1.0),
        kurtosis,
        pitch: Hz::new(2_f64.powf(mean / 1200.0)).ok()?,
        frames: cents.len(),
    })
}

/// The least-squares fit of `a + b t + c cos(2 pi f t) + d sin(2 pi f t)`
/// to `y`: the power the sine explains beyond the line, and its amplitude
/// `sqrt(c^2 + d^2)`. Fitting the line and the sine together keeps a
/// sine over a fraction of a cycle from leaking into the line.
fn sine_fit(times: &[f64], values: &[f64], rate: f64) -> (f64, f64) {
    let mut normal = [[0.0; 5]; 4];
    for (&time, &value) in times.iter().zip(values) {
        let (sine, cosine) = (std::f64::consts::TAU * rate * time).sin_cos();
        let basis = [1.0, time, cosine, sine];
        for (row, &u) in normal.iter_mut().zip(&basis) {
            for (cell, &v) in row.iter_mut().zip(&basis) {
                *cell += u * v;
            }
            row[4] += u * value;
        }
    }
    let Some([_, _, along_cos, along_sin]) = solve4(normal) else {
        return (0.0, 0.0);
    };
    let explained: f64 = times
        .iter()
        .map(|&time| {
            let (sine, cosine) = (std::f64::consts::TAU * rate * time).sin_cos();
            (along_cos * cosine + along_sin * sine).powi(2)
        })
        .sum();
    (explained, along_cos.hypot(along_sin))
}

/// The kurtosis of `y` after its least-squares line, the fourth moment
/// over the squared second, or 0 when nothing is left.
fn kurtosis(t: &[f64], y: &[f64]) -> f64 {
    let n = cast::usize_to_f64(t.len());
    let (st, sy) = (t.iter().sum::<f64>(), y.iter().sum::<f64>());
    let stt: f64 = t.iter().map(|t| t * t).sum();
    let sty: f64 = t.iter().zip(y).map(|(t, y)| t * y).sum();
    let det = n * stt - st * st;
    let slope = if det > 0.0 {
        (n * sty - st * sy) / det
    } else {
        0.0
    };
    let intercept = (sy - slope * st) / n;
    let (mut m2, mut m4) = (0.0, 0.0);
    for (&t, &y) in t.iter().zip(y) {
        let r = y - intercept - slope * t;
        m2 += r * r;
        m4 += r.powi(4);
    }
    if m2 > 0.0 { n * m4 / (m2 * m2) } else { 0.0 }
}

/// The power of `y` left after its least-squares line.
fn residual_power(t: &[f64], y: &[f64]) -> f64 {
    let n = cast::usize_to_f64(t.len());
    let (st, sy) = (t.iter().sum::<f64>(), y.iter().sum::<f64>());
    let stt: f64 = t.iter().map(|t| t * t).sum();
    let sty: f64 = t.iter().zip(y).map(|(t, y)| t * y).sum();
    let det = n * stt - st * st;
    let slope = if det == 0.0 {
        0.0
    } else {
        (n * sty - st * sy) / det
    };
    let intercept = (sy - slope * st) / n;
    t.iter()
        .zip(y)
        .map(|(t, y)| (y - intercept - slope * t).powi(2))
        .sum()
}

/// Solves four linear equations, the augmented rows of `m`, by Gaussian
/// elimination with partial pivoting. `None` for a singular system.
#[expect(
    clippy::needless_range_loop,
    reason = "each step reads two rows of `m`"
)]
fn solve4(mut m: [[f64; 5]; 4]) -> Option<[f64; 4]> {
    for col in 0..4 {
        let pivot = (col..4).max_by(|&a, &b| m[a][col].abs().total_cmp(&m[b][col].abs()))?;
        if m[pivot][col].abs() < 1e-12 {
            return None;
        }
        m.swap(col, pivot);
        for row in col + 1..4 {
            let factor = m[row][col] / m[col][col];
            for k in col..5 {
                m[row][k] -= factor * m[col][k];
            }
        }
    }
    let mut x = [0.0; 4];
    for row in (0..4).rev() {
        let tail: f64 = (row + 1..4).map(|k| m[row][k] * x[k]).sum();
        x[row] = (m[row][4] - tail) / m[row][row];
    }
    Some(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(rate: f64, extent: f64, span: f64) -> Vec<(Seconds, Hz)> {
        let hop = 128.0 / 22_050.0;
        let count = cast::floor_to_usize(span / hop).unwrap();
        (0..=count)
            .map(|k| {
                let t = 0.2 + cast::usize_to_f64(k) * hop;
                let cents = 0.3 * t + extent * (std::f64::consts::TAU * rate * t).sin();
                (
                    Seconds::new(t).unwrap(),
                    Hz::new(220.0 * 2f64.powf(cents / 1200.0)).unwrap(),
                )
            })
            .collect()
    }

    #[test]
    fn an_exact_sine_contour_reads_its_rate_and_extent() {
        let fit = fit_vibrato(&frames(5.5, 35.0, 1.0)).unwrap();
        assert!((fit.rate.get() - 5.5).abs() < 1e-3, "{fit:?}");
        assert!((fit.extent_raw.get() - 35.0).abs() < 0.1, "{fit:?}");
        assert!((fit.factor - 1.0 / 0.939).abs() < 1e-3, "{fit:?}");
        assert!(fit.share > 0.99 && fit.accepted(), "{fit:?}");
        assert!((fit.kurtosis - 1.5).abs() < 0.02, "{fit:?}");
    }

    #[test]
    fn a_vibrato_needs_two_and_a_half_cycles() {
        // 3 Hz over 0.6 s holds 1.8 cycles, over 1 s 3.
        let short = fit_vibrato(&frames(3.0, 35.0, 0.6)).unwrap();
        assert!(
            short.cycles() < VIBRATO_CYCLES_MIN && !short.accepted(),
            "{short:?}"
        );
        let long = fit_vibrato(&frames(3.0, 35.0, 1.0)).unwrap();
        assert!(long.accepted(), "{long:?}");
    }

    #[test]
    fn a_contour_that_jumps_between_two_levels_reads_a_kurtosis_near_1() {
        let square: Vec<(Seconds, Hz)> = frames(5.5, 35.0, 1.0)
            .into_iter()
            .map(|(t, _)| {
                let up = (2.0 * 5.5 * t.get()).floor() % 2.0;
                (t, Hz::new(220.0 * 2f64.powf(up / 12.0)).unwrap())
            })
            .collect();
        let fit = fit_vibrato(&square).unwrap();
        assert!(fit.kurtosis < 1.1, "{fit:?}");
    }

    #[test]
    fn the_factor_interpolates_between_rates_and_holds_at_the_ends() {
        let factor = |rate: f64| extent_factor(Hz::new(rate).unwrap());
        assert!((factor(5.25) - 1.0 / 0.9415).abs() < 1e-12);
        assert!((factor(2.0) - 1.0 / 1.003).abs() < 1e-12);
        assert!((factor(15.0) - 1.0 / 0.732).abs() < 1e-12);
    }

    #[test]
    fn slow_fast_shallow_and_short_motion_is_not_a_vibrato() {
        assert!(!fit_vibrato(&frames(2.0, 35.0, 1.0)).unwrap().accepted());
        assert!(!fit_vibrato(&frames(15.0, 35.0, 1.0)).unwrap().accepted());
        assert!(!fit_vibrato(&frames(5.5, 5.0, 1.0)).unwrap().accepted());
        assert_eq!(fit_vibrato(&frames(5.5, 35.0, 0.4)), None);
        assert_eq!(fit_vibrato(&[]), None);
    }
}
