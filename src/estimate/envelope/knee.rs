//! The note-off knee: where the level of a held note breaks into its
//! release.
//!
//! The search reads the linear amplitude, not dB. A linear release is a
//! straight line from the held level there, and an exponential one is
//! steepest at its start, so the note-off is the sharpest corner either
//! way. In dB a linear release starts gently and plunges at its end: on
//! generated notes a dB search put the knee of a 200 ms release 168 ms
//! late.

use crate::cast;

use super::track::HOP;

/// Each side of a knee spans at least this many frames, 10 ms.
pub const KNEE_SIDE_MIN: usize = 10;
/// The held side of a knee spans up to this many frames, 100 ms.
pub const KNEE_LEFT: usize = 100;
/// The release side of a knee spans up to this many frames, 200 ms.
pub const KNEE_RIGHT: usize = 200;
/// At a knee the slope steepens by at least this share of the held level
/// per second: a release of 2 s or shorter from that level.
pub const KNEE_STEEPEN: f64 = 0.5;
/// A two-line fit with a knee removes at least this share of the squared
/// error that one line leaves.
pub const KNEE_GAIN: f64 = 0.5;
/// The release side of a knee falls by at least this share of the held
/// level, 6 dB.
pub const KNEE_FALL: f64 = 0.5;
/// The release side of a knee ends where the level falls under this share
/// of the held level, -34 dB.
///
/// A line over the silence after a release bends and hides the corner: a
/// 200 ms release from -24 dB, read 15 ms past its end, put the best split
/// 21 to 98 ms after the note-off.
pub const KNEE_FLOOR: f64 = 0.02;

/// The knee of a level track: the split where two lines fit best against
/// one, at the crossing of the left line with the release's first frames.
///
/// The left line spans up to [`KNEE_LEFT`] frames before the split, the
/// right one up to [`KNEE_RIGHT`] after it, and the crossing takes the line
/// over the first [`KNEE_SIDE_MIN`] frames after the split.
///
/// `amp` holds the linear amplitude, one value per [`HOP`] from `start`.
/// Every split leaves at least [`KNEE_SIDE_MIN`] frames on each side. A
/// split counts when the right line is steeper by [`KNEE_STEEPEN`] times
/// the left side's mean per second, falls by [`KNEE_FALL`] times that
/// mean over its side, and the two lines leave at most `1 - KNEE_GAIN` of
/// the squared error of one line. Splits that count in a row form a
/// corner, and the split with the largest gain in the last corner wins.
/// The lines are local, so a decay earlier in the note does not hide its
/// note-off. `None` when no split counts: a pluck that decays
/// at one rate has no note-off to see.
///
/// A level held at 1 for 0.5 s that then falls linearly to 0 over 0.2 s
/// has its knee where the fall starts:
///
/// ```
/// use sound_to_patch::estimate::envelope::find_knee;
///
/// let amp: Vec<f64> = (0..800)
///     .map(|i| (1.0 - (f64::from(i) - 500.0).max(0.0) / 200.0).max(0.0))
///     .collect();
/// let knee = find_knee(0.0, &amp);
/// assert!(knee.is_some_and(|t| (t - 0.5).abs() < 0.002));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn find_knee(start: f64, amp: &[f64]) -> Option<f64> {
    let n = amp.len();
    if n < 2 * KNEE_SIDE_MIN {
        return None;
    }
    let sums = Sums::of(amp);
    let mut best: Option<(f64, Line, usize)> = None;
    let mut last: Option<usize> = None;
    for k in KNEE_SIDE_MIN..=n - KNEE_SIDE_MIN {
        let lo = k.saturating_sub(KNEE_LEFT);
        let level = sums.mean(lo, k);
        // The release side stops where the release has ended: a line over
        // the silence after it bends and hides the corner.
        let hi = (k..(k + KNEE_RIGHT).min(n))
            .find(|&j| amp[j] <= KNEE_FLOOR * level)
            .unwrap_or((k + KNEE_RIGHT).min(n))
            .max(k + KNEE_SIDE_MIN)
            .min(n);
        let (Some(left), Some(right), Some(one)) =
            (sums.fit(lo, k), sums.fit(k, hi), sums.fit(lo, hi))
        else {
            continue;
        };
        if one.error <= 0.0 {
            continue;
        }
        let gain = 1.0 - (left.error + right.error) / one.error;
        let fall = right.at(k) - right.at(hi - 1);
        let counts = right.slope <= left.slope - KNEE_STEEPEN * level * HOP.get()
            && fall >= KNEE_FALL * level
            && gain >= KNEE_GAIN;
        // Splits that count in a row form one corner; a later corner
        // replaces an earlier one, since a hold that breaks into a linear
        // decay is a corner too and the note-off comes last.
        if counts {
            let new_corner = last.is_none_or(|l| k > l + 1);
            if new_corner || best.as_ref().is_none_or(|b| gain > b.0) {
                best = Some((gain, left, k));
            }
            last = Some(k);
        }
    }
    let (_, left, k) = best?;
    // The release side is read again over its first frames only: a line
    // over 200 ms of an exponential release is a secant that crosses the
    // held level late, and the line over the first 10 ms is near its
    // tangent at the split.
    let tangent = sums.fit(k, (k + KNEE_SIDE_MIN).min(n))?;
    let crossing = (tangent.intercept - left.intercept) / (left.slope - tangent.slope);
    // The crossing counts only over the frames of the two lines, so the
    // knee never lies before the search starts.
    let first = cast::usize_to_f64(k.saturating_sub(KNEE_LEFT));
    let k = cast::usize_to_f64(k);
    let x = if (first..=k + cast::usize_to_f64(KNEE_SIDE_MIN)).contains(&crossing) {
        crossing
    } else {
        k
    };
    Some(start + x * HOP.get())
}

/// A least-squares line over frame indices, with its squared error.
#[derive(Clone, Copy, Debug)]
struct Line {
    slope: f64,
    intercept: f64,
    error: f64,
}

impl Line {
    fn at(self, i: usize) -> f64 {
        self.intercept + self.slope * cast::usize_to_f64(i)
    }
}

/// Prefix sums of `x`, `y`, `x^2`, `x y` and `y^2` over frame indices, so a
/// line over any range fits in constant time.
struct Sums {
    x: Vec<f64>,
    y: Vec<f64>,
    xx: Vec<f64>,
    xy: Vec<f64>,
    yy: Vec<f64>,
}

impl Sums {
    fn of(values: &[f64]) -> Self {
        let mut sums = Self {
            x: vec![0.0],
            y: vec![0.0],
            xx: vec![0.0],
            xy: vec![0.0],
            yy: vec![0.0],
        };
        for (i, &y) in values.iter().enumerate() {
            let x = cast::usize_to_f64(i);
            for (v, add) in [
                (&mut sums.x, x),
                (&mut sums.y, y),
                (&mut sums.xx, x * x),
                (&mut sums.xy, x * y),
                (&mut sums.yy, y * y),
            ] {
                let last = v.last().copied().unwrap_or(0.0);
                v.push(last + add);
            }
        }
        sums
    }

    /// The mean over frames `lo..hi`; 0 for none.
    fn mean(&self, lo: usize, hi: usize) -> f64 {
        if hi > lo {
            (self.y[hi] - self.y[lo]) / cast::usize_to_f64(hi - lo)
        } else {
            0.0
        }
    }

    /// The line over frames `lo..hi`; `None` for fewer than two frames.
    fn fit(&self, lo: usize, hi: usize) -> Option<Line> {
        if hi < lo + 2 {
            return None;
        }
        let n = cast::usize_to_f64(hi - lo);
        let d = |v: &[f64]| v[hi] - v[lo];
        let (sx, sy, sxx, sxy, syy) = (
            d(&self.x),
            d(&self.y),
            d(&self.xx),
            d(&self.xy),
            d(&self.yy),
        );
        let var = n * sxx - sx * sx;
        if var <= 0.0 {
            return None;
        }
        let slope = (n * sxy - sx * sy) / var;
        let intercept = (sy - slope * sx) / n;
        // The squared error of the line: sum (y - a - b x)^2 expanded.
        let error = (syy - 2.0 * intercept * sy - 2.0 * slope * sxy
            + n * intercept * intercept
            + 2.0 * intercept * slope * sx
            + slope * slope * sxx)
            .max(0.0);
        Some(Line {
            slope,
            intercept,
            error,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The linear amplitude of a dB curve, one value per hop.
    fn track(f: impl Fn(f64) -> f64, seconds: f64) -> Vec<f64> {
        (0..cast::round_to_usize(seconds / HOP.get()).unwrap())
            .map(|i| 10f64.powf(f(cast::usize_to_f64(i) * HOP.get()) / 20.0))
            .collect()
    }

    #[test]
    fn a_linear_release_has_its_knee_at_the_note_off() {
        // Held at -6 dB, then a linear 200 ms fade, read up to 190 ms in.
        let amp: Vec<f64> = (0..590)
            .map(|i| {
                let t = cast::usize_to_f64(i) * HOP.get();
                0.5 * (1.0 - (t - 0.4).max(0.0) / 0.2)
            })
            .collect();
        let knee = find_knee(0.0, &amp).unwrap();
        assert!((knee - 0.4).abs() < 0.002, "{knee}");
    }

    #[test]
    fn a_held_level_that_breaks_into_a_fall_has_its_knee_at_the_break() {
        let db = track(
            |t| {
                if t < 0.4 {
                    -6.0
                } else {
                    -6.0 - 200.0 * (t - 0.4)
                }
            },
            0.5,
        );
        let knee = find_knee(1.0, &db).unwrap();
        assert!((knee - 1.4).abs() < 0.002, "{knee}");
    }

    #[test]
    fn a_hold_that_breaks_into_a_linear_decay_is_not_the_note_off() {
        // Held at 1 for 30 ms, a linear decay to 0.063 over 300 ms, held,
        // then a linear 20 ms release at 0.8 s, read 15 ms past its end.
        let amp: Vec<f64> = (0..835)
            .map(|i| {
                let t = cast::usize_to_f64(i) * HOP.get();
                let held = if t < 0.03 {
                    1.0
                } else {
                    (1.0 - 0.937 * (t - 0.03) / 0.3).max(0.063)
                };
                held * (1.0 - (t - 0.8).max(0.0) / 0.02).max(0.0)
            })
            .collect();
        let knee = find_knee(0.0, &amp).unwrap();
        assert!((knee - 0.8).abs() < 0.002, "{knee}");
    }

    #[test]
    fn a_hold_that_breaks_into_a_curved_fall_has_its_knee_at_the_break() {
        // Held at 1 for 500 frames, then `1 - (t / 150)^1.1` over 150
        // frames: the fall steepens from the break on.
        let amp: Vec<f64> = (0..650)
            .map(|i| {
                let t = cast::usize_to_f64(i.max(500) - 500);
                1.0 - (t / 150.0).powf(1.1)
            })
            .collect();
        let knee = find_knee(0.0, &amp).unwrap();
        assert!((knee - 0.5011).abs() < 5e-4, "{knee}");
    }

    #[test]
    fn a_two_stage_release_reads_the_later_corner() {
        // A shelf in the release looks like a decay into a sustain: the last
        // corner wins, so the knee reads the second stage. Held at 1 to
        // 0.4 s, down to 0.5 by 0.45 s, flat to 0.7 s, then to 0 by 0.8 s.
        let amp: Vec<f64> = (0..820)
            .map(|i| {
                let t = cast::usize_to_f64(i) * HOP.get();
                let first = 1.0 - 0.5 * ((t - 0.4) / 0.05).clamp(0.0, 1.0);
                first * (1.0 - ((t - 0.7) / 0.1).clamp(0.0, 1.0))
            })
            .collect();
        let knee = find_knee(0.0, &amp).unwrap();
        assert!((knee - 0.7).abs() < 0.002, "{knee}");
    }

    #[test]
    fn a_fall_at_one_rate_has_no_knee() {
        let db = track(|t| -40.0 * t, 1.0);
        assert_eq!(find_knee(0.0, &db), None);
    }

    #[test]
    fn a_slope_that_flattens_is_no_knee() {
        // A decay into its sustain level steepens nowhere.
        let db = track(|t| if t < 0.3 { -20.0 * t / 0.3 } else { -20.0 }, 0.8);
        assert_eq!(find_knee(0.0, &db), None);
    }

    #[test]
    fn the_knee_of_a_decay_sustain_release_is_the_release() {
        let db = track(
            |t| {
                if t < 0.2 {
                    -30.0 * t
                } else if t < 0.7 {
                    -6.0
                } else {
                    -6.0 - 300.0 * (t - 0.7)
                }
            },
            0.8,
        );
        let knee = find_knee(0.0, &db).unwrap();
        assert!((knee - 0.7).abs() < 0.002, "{knee}");
    }

    #[test]
    fn a_crossing_before_the_search_starts_falls_back_to_the_split() {
        // 20 frames at 1, a step to 0.6, then a slow fall to 0 over 60
        // frames: the tangent after the step crosses the held level 20
        // frames before the first frame, so the knee is the split itself.
        let amp: Vec<f64> = (0..100)
            .map(|i| match i {
                0..20 => 1.0,
                20..80 => 0.6 * (1.0 - cast::usize_to_f64(i - 20) / 60.0),
                _ => 0.0,
            })
            .collect();
        let knee = find_knee(1.0, &amp).unwrap();
        assert!(
            (knee - (1.0 + 20.0 * HOP.get())).abs() < 1.5 * HOP.get(),
            "{knee}"
        );
    }

    proptest::proptest! {
        #[test]
        fn a_knee_lies_inside_the_searched_frames(
            amp in proptest::collection::vec(0.0..1.0f64, 0..400),
            start in 0.0..10.0f64,
        ) {
            if let Some(knee) = find_knee(start, &amp) {
                let end = start + cast::usize_to_f64(amp.len()) * HOP.get();
                proptest::prop_assert!(knee >= start && knee <= end, "{knee}");
            }
        }
    }
}
