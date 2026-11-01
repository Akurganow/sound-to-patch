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
use crate::units::Decibels;

use super::track::HOP;

/// Each side of a knee spans at least this many frames, 10 ms.
pub const KNEE_SIDE_MIN: usize = 10;
/// The held side of a knee spans up to this many frames, 100 ms.
pub const KNEE_LEFT: usize = 100;
/// The release side of a knee spans up to this many frames, 200 ms.
pub const KNEE_RIGHT: usize = 200;
/// At a knee the slope steepens by at least this share of the held level
/// per second. With [`KNEE_FALL`] over [`KNEE_RIGHT`], a linear release shows a
/// knee up to about 400 ms.
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

/// A later corner replaces an earlier one only when the level between them
/// bends by at most this much.
///
/// The bend is the median size of the second difference in dB over
/// [`KNEE_BEND_LAG`] frames each way. A decay, a sustain and a slow tremolo read
/// near 0, and the fades of a reverb read more.
pub const KNEE_BEND: Decibels = Decibels::from_const(0.4);
/// The frames each way of the second difference of [`KNEE_BEND`], 10 ms.
pub const KNEE_BEND_LAG: usize = 10;
/// A later corner replaces an earlier one only when the level after it rises
/// by at most this share of the level before it. A note-off falls for good,
/// and a fade of a reverb comes back.
pub const KNEE_RISE: f64 = 0.05;
/// The frames of the mean that the rise reads past its start, so that a noise
/// floor's wobble after the note-off does not count as a level that comes back.
pub const KNEE_RISE_FRAMES: usize = 20;

/// A level that stays under this share of the held level has left it for good.
const KNEE_LEAVE: f64 = 0.85;
/// The lowest level the bend reads, -80 dB.
const KNEE_DB_FLOOR: f64 = 1e-4;

/// The knee of a level track: the split where two lines fit best against
/// one, at the crossing of the left line with the release's first frames.
///
/// The left line spans up to [`KNEE_LEFT`] frames before the split, the
/// right one up to [`KNEE_RIGHT`] after it, and the crossing takes the line
/// over the first [`KNEE_SIDE_MIN`] frames after the split.
///
/// `amp` holds the linear amplitude, one value per [`HOP`] from `start`. The last
/// `settle` frames can read what follows the track, so the rise leaves them out.
/// Every split leaves at least [`KNEE_SIDE_MIN`] frames on each side. A
/// split counts when the right line is steeper by [`KNEE_STEEPEN`] times
/// the left side's mean per second, falls by [`KNEE_FALL`] times that
/// mean over its side, and the two lines leave at most `1 - KNEE_GAIN` of
/// the squared error of one line. Splits that count in a row form a
/// corner, and within a corner the split with the largest gain wins. A later
/// corner replaces an earlier one, by [`KNEE_BEND`] and [`KNEE_RISE`].
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
/// let knee = find_knee(0.0, &amp, 0);
/// assert!(knee.is_some_and(|t| (t - 0.5).abs() < 0.002));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn find_knee(start: f64, amp: &[f64], settle: usize) -> Option<f64> {
    let n = amp.len();
    if n < 2 * KNEE_SIDE_MIN {
        return None;
    }
    let sums = Sums::of(amp);
    let mut corners: Vec<Corner> = Vec::new();
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
        if right.slope <= left.slope - KNEE_STEEPEN * level * HOP.get()
            && fall >= KNEE_FALL * level
            && gain >= KNEE_GAIN
        {
            let found = Corner {
                first: k,
                k,
                gain,
                left,
                level,
            };
            // Splits that count in a row form one corner.
            match corners.last_mut() {
                Some(corner) if last == Some(k - 1) => {
                    if gain > corner.gain {
                        *corner = Corner {
                            first: corner.first,
                            ..found
                        };
                    }
                }
                _ => corners.push(found),
            }
            last = Some(k);
        }
    }
    let (first, later) = corners.split_first()?;
    // A hold that breaks into a decay is a corner too and the note-off comes
    // last, so a later corner replaces an earlier one, unless it is a fade of
    // a reverb: the level before it wobbles, or the level after it comes back.
    let db: Vec<f64> = amp
        .iter()
        .map(|v| 20.0 * v.max(KNEE_DB_FLOOR).log10())
        .collect();
    let mut best = first;
    for corner in later {
        if bend(&db, best.k, corner.first, KNEE_BEND_LAG) <= KNEE_BEND.get()
            && rise(&amp[corner.k..n.saturating_sub(settle).max(corner.k)])
                <= KNEE_RISE * corner.level
        {
            best = corner;
        }
    }
    let Corner { k, left, .. } = *best;
    // A level that wobbles over its hold, as under a reverb of the note, can win
    // a split well before the level leaves for good: read the fall itself there.
    let held = left.at(k);
    let reach = (k + KNEE_RIGHT).min(n);
    let leaves = (k..reach).rev().find(|&j| amp[j] >= KNEE_LEAVE * held);
    if let Some(from) = leaves.filter(|&j| j > k + KNEE_SIDE_MIN) {
        let to = (from..reach)
            .find(|&j| amp[j] <= KNEE_FALL * held)
            .unwrap_or(reach)
            .max(from + KNEE_SIDE_MIN)
            .min(n);
        if let Some(line) = sums.fit(from, to).filter(|l| l.slope < 0.0) {
            let back = (held - line.intercept) / line.slope;
            if back > cast::usize_to_f64(k + KNEE_SIDE_MIN) {
                return Some(start + back.min(cast::usize_to_f64(from)) * HOP.get());
            }
        }
    }
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

/// The splits that count in a row, with the values of the one that wins.
#[derive(Clone, Copy, Debug)]
struct Corner {
    first: usize,
    k: usize,
    gain: f64,
    left: Line,
    level: f64,
}

/// The median size of the second difference of `db` over the frames `from..to`,
/// in dB; 0 when the span holds no second difference.
pub(super) fn bend(db: &[f64], from: usize, to: usize, lag: usize) -> f64 {
    let (lo, hi) = (from + lag, to.saturating_sub(lag));
    if hi <= lo {
        return 0.0;
    }
    let mut sizes: Vec<f64> = (lo..hi)
        .map(|i| (db[i - lag] - 2.0 * db[i] + db[i + lag]).abs())
        .collect();
    sizes.sort_by(f64::total_cmp);
    sizes[(sizes.len() - 1) / 2]
}

/// The largest rise of `amp` over its first [`KNEE_RISE_FRAMES`], and of its mean
/// over that many frames after them.
fn rise(amp: &[f64]) -> f64 {
    let head = amp.iter().take(KNEE_RISE_FRAMES).copied();
    let means = amp
        .windows(KNEE_RISE_FRAMES)
        .map(|w| w.iter().sum::<f64>() / cast::usize_to_f64(KNEE_RISE_FRAMES));
    climb(head).max(climb(means))
}

/// The largest rise of a sequence from any lower value before it.
fn climb(values: impl Iterator<Item = f64>) -> f64 {
    let mut low = f64::INFINITY;
    let mut climb = 0.0_f64;
    for v in values {
        low = low.min(v);
        climb = climb.max(v - low);
    }
    climb
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

    /// A hold that breaks into a dB-linear decay to `sustain`, a note-off, and
    /// a linear release, with `wobble` in dB over the level.
    fn adsr(sustain: f64, decay: f64, gap: f64, wobble: impl Fn(f64) -> f64) -> Vec<f64> {
        let off = 0.1 + decay + gap;
        track(
            |t| {
                let held = sustain * ((t - 0.1) / decay).clamp(0.0, 1.0);
                let fall = 1.0 - ((t - off) / 0.05).clamp(0.0, 1.0);
                held + 20.0 * fall.max(1e-4).log10() + wobble(t)
            },
            off + 0.1,
        )
    }

    #[test]
    fn a_sustain_shorter_than_its_decay_still_has_its_knee_at_the_note_off() {
        // The 0.2 s decay would fill any look-back over the 50 ms sustain.
        let amp = adsr(-12.0, 0.2, 0.05, |_| 0.0);
        let knee = find_knee(0.0, &amp, 0).unwrap();
        assert!((knee - 0.355).abs() < 0.002, "{knee}");
    }

    #[test]
    fn a_bend_over_the_limit_keeps_the_earlier_corner() {
        let off = 0.1 + 0.1 + 0.3;
        let bend_of = |size: f64, hz: f64| {
            adsr(-18.0, 0.1, 0.3, move |t| {
                if t > 0.2 && t < off {
                    size * (std::f64::consts::TAU * hz * t).sin()
                } else {
                    0.0
                }
            })
        };
        let smooth = find_knee(0.0, &bend_of(0.3, 25.0), 0).unwrap();
        assert!((smooth - off).abs() < 0.002, "{smooth}");
        let wobbling = find_knee(0.0, &bend_of(3.0, 25.0), 0).unwrap();
        assert!(wobbling < 0.2, "{wobbling}");
    }

    #[test]
    fn the_rise_leaves_out_the_frames_that_read_past_the_track() {
        // A decay into a shelf, a release, and a next note's attack in the last 10
        // frames. A rise over them would keep the corner of the decay.
        let amp: Vec<f64> = (0..800_u32)
            .map(|i| match i {
                0..200 => 1.0,
                200..300 => 1.0 - 0.75 * f64::from(i - 200) / 100.0,
                300..500 => 0.25,
                500..550 => 0.25 * (1.0 - f64::from(i - 500) / 50.0),
                550..790 => 0.0,
                _ => f64::from(i - 790) / 10.0,
            })
            .collect();
        let at = |settle| find_knee(0.0, &amp, settle).unwrap();
        assert!((at(10) - 0.5).abs() < 0.002, "{}", at(10));
        assert!((at(0) - 0.2).abs() < 0.002, "{}", at(0));
    }

    /// A decay at 200 ms into a shelf, and a release at 500 ms into silence.
    fn two_corners(shelf: impl Fn(u32) -> f64, after: impl Fn(u32) -> f64) -> Vec<f64> {
        (0..800_u32)
            .map(|i| match i {
                0..200 => 1.0,
                200..300 => 1.0 - 0.75 * f64::from(i - 200) / 100.0,
                300..500 => shelf(i),
                500..550 => 0.25 * (1.0 - f64::from(i - 500) / 50.0),
                _ => after(i),
            })
            .collect()
    }

    #[test]
    fn a_later_corner_needs_a_bend_of_at_most_0_4_db() {
        // A decay straight in dB at 200 ms into a shelf and a release at 500 ms, under
        // a triangle 20 frames long whose bend reads 2.4 times its peak: 0.384 and 0.408.
        let read = |peak_db: f64| {
            let amp: Vec<f64> = (0..800_u32)
                .map(|i| {
                    let phase = f64::from(i % 20) / 5.0;
                    let shape = if phase < 2.0 {
                        phase - 1.0
                    } else {
                        3.0 - phase
                    };
                    let body = -12.0 * (f64::from(i.max(200) - 200) / 100.0).min(1.0);
                    let level = 10f64.powf((body + peak_db * shape) / 20.0);
                    match i {
                        0..500 => level,
                        500..550 => 0.25 * (1.0 - f64::from(i - 500) / 50.0),
                        _ => 0.0,
                    }
                })
                .collect();
            find_knee(0.0, &amp, 0).unwrap()
        };
        assert!((read(0.16) - 0.5).abs() < 0.01, "{}", read(0.16));
        assert!((read(0.17) - 0.19).abs() < 0.01, "{}", read(0.17));
    }

    #[test]
    fn a_later_corner_needs_a_rise_of_at_most_a_twentieth_of_the_level_before_it() {
        let read = |share: f64| {
            let back = |i: u32| 0.25 * share * (f64::from(i.saturating_sub(600)) / 50.0).min(1.0);
            find_knee(0.0, &two_corners(|_| 0.25, back), 0).unwrap()
        };
        assert!((read(0.045) - 0.5).abs() < 0.002, "{}", read(0.045));
        assert!((read(0.055) - 0.2).abs() < 0.002, "{}", read(0.055));
    }

    #[test]
    fn the_fall_is_read_from_a_level_of_85_percent_of_the_held_one() {
        // At 0.82 of the held level a step reads no fall, where 80 % would.
        let step: Vec<f64> = (0..600_u32)
            .map(|i| match i {
                0..300 => 1.0,
                300..330 => 0.82,
                330..430 => 0.82 * (1.0 - f64::from(i - 330) / 100.0),
                _ => 0.0,
            })
            .collect();
        let knee = find_knee(0.0, &step, 0).unwrap();
        assert!((knee - 0.285).abs() < 0.003, "{knee}");
        // A slope down to 0.89 before a steeper fall reads the fall from it, where 95 %
        // would read it from the slope's start.
        let points = [
            (0.0, 1.0),
            (312.0, 1.0),
            (366.0, 0.89),
            (397.0, 0.37),
            (447.0, 0.0),
            (647.0, 0.0),
        ];
        let amp: Vec<f64> = (0..647_u32)
            .map(|i| {
                let x = f64::from(i);
                let w = points.windows(2).find(|w| x < w[1].0).unwrap();
                w[0].1 + (w[1].1 - w[0].1) * (x - w[0].0) / (w[1].0 - w[0].0)
            })
            .collect();
        let knee = find_knee(0.0, &amp, 0).unwrap();
        assert!((knee - 0.362).abs() < 0.002, "{knee}");
    }

    #[test]
    fn a_fall_read_stops_at_the_frame_it_starts_from() {
        // A swell after a fall: the line over the last fall starts above the held level,
        // so it crosses that level past the frame it starts from.
        let points = [
            (0.0, 1.0),
            (99.0, 0.91),
            (129.0, 0.91),
            (173.0, 0.49),
            (201.0, 0.49),
            (239.0, 0.35),
            (268.0, 0.004),
            (283.0, 3.85),
            (298.0, 3.85),
            (328.0, 0.0),
            (498.0, 0.0),
        ];
        let amp: Vec<f64> = (0..498_u32)
            .map(|i| {
                let x = f64::from(i);
                let w = points.windows(2).find(|w| x < w[1].0).unwrap();
                w[0].1 + (w[1].1 - w[0].1) * (x - w[0].0) / (w[1].0 - w[0].0)
            })
            .collect();
        let knee = find_knee(0.0, &amp, 0).unwrap();
        assert!((knee - 0.299).abs() < 1e-3, "{knee}");
    }

    #[test]
    fn a_noise_floor_under_the_tail_does_not_hide_the_note_off() {
        // A floor that wobbles by a tenth of the shelf after the release, as noise
        // at -40 dBFS does under a quiet sustain.
        let amp: Vec<f64> = (0..800_u32)
            .map(|i| match i {
                0..200 => 1.0,
                200..300 => 1.0 - 0.75 * f64::from(i - 200) / 100.0,
                300..500 => 0.25,
                500..550 => 0.25 - 0.235 * f64::from(i - 500) / 50.0,
                _ => 0.015 + if i % 20 < 10 { 0.0 } else { 0.025 },
            })
            .collect();
        let knee = find_knee(0.0, &amp, 0).unwrap();
        assert!((knee - 0.5).abs() < 0.002, "{knee}");
        // A wobble twice as long shows through a mean of 20 frames.
        let slow = |i: u32| if i % 40 < 20 { 0.0 } else { 0.05 };
        let knee = find_knee(0.0, &two_corners(|_| 0.25, slow), 0).unwrap();
        assert!((knee - 0.2).abs() < 0.01, "{knee}");
    }

    #[test]
    fn a_later_corner_with_a_rise_after_it_keeps_the_earlier_one() {
        // A level that returns to a tenth of the shelf after its fall.
        let amp: Vec<f64> = (0..800_u32)
            .map(|i| match i {
                0..200 => 1.0,
                200..300 => 1.0 - 0.75 * f64::from(i - 200) / 100.0,
                300..500 => 0.25,
                500..550 => 0.25 * (1.0 - f64::from(i - 500) / 50.0),
                550..600 => 0.25 * 0.3 * f64::from(i - 550) / 50.0,
                _ => 0.25 * 0.3,
            })
            .collect();
        let knee = find_knee(0.0, &amp, 0).unwrap();
        assert!((knee - 0.2).abs() < 0.002, "{knee}");
    }

    #[test]
    fn the_fall_read_starts_from_the_level_at_the_split() {
        // A slow rise to 1, under which the 100 ms mean lies: the fall from 1
        // reads from the level at the split, not from the mean.
        let amp: Vec<f64> = (0..600_u32)
            .map(|i| {
                let x = f64::from(i);
                match i {
                    0..300 => 0.6 + 0.4 * x / 300.0,
                    300..315 => 1.0 - 0.45 * (x - 300.0) / 15.0,
                    315..340 => 0.55 + 0.45 * (x - 315.0) / 25.0,
                    340..440 => 1.0 - (x - 340.0) / 100.0,
                    _ => 0.0,
                }
            })
            .collect();
        let knee = find_knee(0.0, &amp, 0).unwrap();
        assert!((knee - 0.3445).abs() < 0.002, "{knee}");
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
        let knee = find_knee(0.0, &amp, 0).unwrap();
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
        let knee = find_knee(1.0, &db, 0).unwrap();
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
        let knee = find_knee(0.0, &amp, 0).unwrap();
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
        let knee = find_knee(0.0, &amp, 0).unwrap();
        assert!((knee - 0.5011).abs() < 5e-4, "{knee}");
    }

    #[test]
    fn a_two_stage_release_reads_the_later_corner() {
        // A shelf in the release looks like a decay into a sustain, so the
        // knee reads the second stage. Held at 1 to
        // 0.4 s, down to 0.5 by 0.45 s, flat to 0.7 s, then to 0 by 0.8 s.
        let amp: Vec<f64> = (0..820)
            .map(|i| {
                let t = cast::usize_to_f64(i) * HOP.get();
                let first = 1.0 - 0.5 * ((t - 0.4) / 0.05).clamp(0.0, 1.0);
                first * (1.0 - ((t - 0.7) / 0.1).clamp(0.0, 1.0))
            })
            .collect();
        let knee = find_knee(0.0, &amp, 0).unwrap();
        assert!((knee - 0.7).abs() < 0.002, "{knee}");
    }

    #[test]
    fn a_fall_at_one_rate_has_no_knee() {
        let db = track(|t| -40.0 * t, 1.0);
        assert_eq!(find_knee(0.0, &db, 0), None);
    }

    #[test]
    fn a_slope_that_flattens_is_no_knee() {
        // A decay into its sustain level steepens nowhere.
        let db = track(|t| if t < 0.3 { -20.0 * t / 0.3 } else { -20.0 }, 0.8);
        assert_eq!(find_knee(0.0, &db, 0), None);
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
        let knee = find_knee(0.0, &db, 0).unwrap();
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
        let knee = find_knee(1.0, &amp, 0).unwrap();
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
            if let Some(knee) = find_knee(start, &amp, 0) {
                let end = start + cast::usize_to_f64(amp.len()) * HOP.get();
                proptest::prop_assert!(knee >= start && knee <= end, "{knee}");
            }
        }
    }
}
