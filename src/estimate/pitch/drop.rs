//! The pitch drop of a low note, the "thump" of an 808 or a pluck: the fit
//! `f(t) = f_end + df exp(-t / tau)` on the sub-band line.

use super::FAST_TAU;
use super::fit::golden_section;
use super::line::SubBandLine;
use crate::audio::SampleRate;
use crate::cast;
use crate::units::{Hz, Seconds, Semitones};

/// The fit starts this long after the onset. The line of a tone switched
/// on from silence overshoots by about 11 Hz at 3 ms and by 1 to 4 Hz at
/// 7 ms, at 30 to 150 Hz, and settles within 1 Hz by 30 ms.
pub const DROP_FIT_START: Seconds = Seconds::from_const(0.007);
/// The fit ends this long after the onset, or at the end of the note.
pub(crate) const DROP_FIT_END: Seconds = Seconds::from_const(0.4);
/// The smallest drop that counts.
pub const DROP_MIN: Semitones = Semitones::from_const(2.0);
/// The largest drop that counts: three octaves. The line reads up to
/// 200 Hz, so a larger fall starts far outside the band and its `df` is
/// extrapolated from the tail alone.
pub const DROP_MAX: Semitones = Semitones::from_const(36.0);
/// The smallest `tau` of a fit, about 3 ms.
///
/// A tenth of the fall is still left at [`DROP_FIT_START`],
/// `exp(-7 ms / tau) >= 0.1`. A faster fall ends before the span starts,
/// and its `df` is extrapolated from the swing of the line at a
/// retriggered or switched-on tone.
pub const DROP_TAU_MIN: Seconds = Seconds::from_const(0.007 / std::f64::consts::LN_10);
/// The largest RMS residual of an accepted fit, as a share of the fall the
/// fit span shows.
pub const DROP_MAX_RESIDUAL: f64 = 0.15;
/// The onset of a strike: the first sample whose band envelope reaches
/// this share of its peak.
pub const DROP_ONSET_SHARE: f64 = 0.5;
/// The attack of a strike starts where the band envelope reaches this
/// share of its peak.
pub(crate) const ATTACK_FROM: f64 = 0.1;
/// The attack ends where the band envelope reaches this share of its peak.
pub(crate) const ATTACK_TO: f64 = 0.9;
/// The centred average of the band envelope that the onset search reads.
pub const ONSET_SMOOTHING: Seconds = Seconds::from_const(0.01);
/// The largest `tau` the search covers.
const TAU_MAX: f64 = 1.0;
/// A fit whose `tau` lies within this factor of [`DROP_TAU_MIN`] sits on
/// the bound of the search.
const TAU_EDGE: f64 = 1.01;
/// A fit whose `tau` spans this many periods of `f_end` or more has its
/// error averaged over a full period.
const FULL_PERIODS: f64 = 1.5;
/// Points of the log grid over `tau` before the golden-section refinement.
const TAU_GRID: usize = 240;

/// The convention of a drop, as `layers.json` names it.
pub const DROP_CONVENTION: &str = "f(t) = f_end + df exp(-t / tau) fitted by least squares to \
the sub-band line from 7 ms to 400 ms after the onset, or to the end of the note; tau by a log \
grid and golden-section search from 3.04 ms, where exp(-7 ms / tau) = 0.1, to 1 s, f_end and \
df linear for each tau; a fit with tau under 10 ms, or after a strike whose full-band RMS rises \
20 dB, is fitted again on the line with its high-pass at 10 Hz, and the refit counts; \
df_st = 12 log2((f_end + df) / f_end); onset_s is the first sample after the lowest point of \
the onset search where the 20 to 200 Hz envelope, averaged over a centred 10 ms, reaches half \
the peak that follows; a strike whose full-band RMS over the 20 ms after the onset lies more \
than 40 dB below the loudest 25 ms frame of the stem, or whose 20 to 200 Hz RMS over those \
20 ms lies more than 18 dB below the loudest 25 ms frame of that band, is no candidate; a drop \
with tau under 10 ms needs an envelope that takes 30 ms or less from 10 to 90 % of its peak; a \
drop with tau under 10 ms, one that starts at 200 Hz or above, or one whose envelope takes over \
30 ms from 10 to 90 % of its peak needs the full-band RMS over the 20 ms after the onset 6 dB or \
more above the 20 ms that end 25 ms before it; residual is the RMS of the fit's error, averaged \
over half a period of f_end when tau is 10 ms or more and over a full period when tau spans 1.5 \
periods or more, over the fitted fall from 7 ms on, df exp(-7 ms / tau); a drop counts when \
df_st is above 2 and at most 36, that fall is above 2 semitones too, tau is above the bound of \
the search, residual is below 0.15, and f_end lies within 100 cents of a note that sounds from \
5 tau after the onset, or from the end of the fit span if that comes first, to the end of the \
fit span";

/// A fitted drop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropFit {
    /// The onset the fit counts `t` from.
    pub onset: Seconds,
    /// The settled frequency.
    pub f_end: Hz,
    /// The frequency above `f_end` at the onset, in hertz; negative for a
    /// rise.
    pub df_hz: f64,
    /// The time constant.
    pub tau: Seconds,
    /// The RMS residual over the fall the fit span shows,
    /// `|df_hz| exp(-DROP_FIT_START / tau)`. Over the extrapolated `df_hz`
    /// a fast swing at the onset, fitted with a `tau` under a millisecond,
    /// would read a tiny share.
    pub residual: f64,
    /// The number of line values the fit read.
    pub points: usize,
}

impl DropFit {
    /// The drop in semitones, `12 log2((f_end + df) / f_end)`, or `None`
    /// when `f_end + df` is not above zero.
    #[must_use]
    pub fn df(&self) -> Option<Semitones> {
        self.above_end(self.f_end.get() + self.df_hz)
    }

    /// The drop the line shows inside the fit span, in semitones: from the
    /// fitted frequency at [`DROP_FIT_START`] down to `f_end`. A fast fall
    /// that ends before the span starts is extrapolated, not seen, such as
    /// the overshoot of the line at a tone's onset.
    #[must_use]
    pub fn visible(&self) -> Option<Semitones> {
        self.above_end(
            self.f_end.get() + self.df_hz * (-DROP_FIT_START.get() / self.tau.get()).exp(),
        )
    }

    /// `start` above `f_end` in semitones, `12 log2(start / f_end)`, or
    /// `None` when either is not above zero.
    fn above_end(&self, start: f64) -> Option<Semitones> {
        (start > 0.0 && self.f_end.get() > 0.0)
            .then(|| Semitones::clamped(12.0 * (start / self.f_end.get()).log2()))
    }

    /// Whether the fit counts as a drop: `df` from above [`DROP_MIN`] up
    /// to [`DROP_MAX`], the visible drop above [`DROP_MIN`], `tau` above
    /// [`DROP_TAU_MIN`] and the residual below [`DROP_MAX_RESIDUAL`]. A
    /// `tau` on the bound means the best fit lies below it.
    #[must_use]
    pub fn accepted(&self) -> bool {
        self.miss().is_none()
    }

    /// The first condition of [`Self::accepted`] the fit misses, or `None`
    /// when it counts as a drop.
    #[must_use]
    pub fn miss(&self) -> Option<DropMiss> {
        if !(self.df().is_some_and(|df| df > DROP_MIN && df <= DROP_MAX)
            && self.visible().is_some_and(|visible| visible > DROP_MIN))
        {
            Some(DropMiss::Size)
        } else if self.tau.get() <= TAU_EDGE * DROP_TAU_MIN.get() {
            Some(DropMiss::TauBound)
        } else if self.residual >= DROP_MAX_RESIDUAL {
            Some(DropMiss::Residual)
        } else {
            None
        }
    }
}

/// Why a fit does not count as a drop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropMiss {
    /// The drop, in full or inside the fit span, lies outside
    /// [`DROP_MIN`] to [`DROP_MAX`].
    Size,
    /// `tau` sits on the bound of the search.
    TauBound,
    /// The residual reaches [`DROP_MAX_RESIDUAL`].
    Residual,
}

/// The onset of a strike inside `from..to`, from the band envelope.
///
/// The envelope is averaged over a centred [`ONSET_SMOOTHING`]. The onset
/// is the first sample after the lowest point of the span where the
/// average reaches [`DROP_ONSET_SHARE`] of the peak that follows. `None`
/// when the span is empty or its lowest point already stands above that
/// share, so no attack lies inside it.
///
/// The average keeps a held note from reading as a strike: several
/// partials of a saw share the band and beat, so its raw envelope dips at
/// every period. A centred average leaves the midpoint of a step in place.
#[must_use]
pub fn band_onset(line: &SubBandLine, from: Seconds, to: Seconds) -> Option<Seconds> {
    let (start, smooth) = smoothed_envelope(line, from, to)?;
    let (low, peak) = low_and_peak(&smooth)?;
    let first = smooth[low..]
        .iter()
        .position(|&e| e >= DROP_ONSET_SHARE * peak)?;
    Some(line.sample_time(start + low + first))
}

/// The attack of a strike inside `from..to`.
///
/// It is the time the band envelope, averaged as for [`band_onset`], takes
/// from [`ATTACK_FROM`] to [`ATTACK_TO`] of the peak that follows its
/// lowest point. `None` when no attack lies inside the span.
#[must_use]
pub(crate) fn band_attack(line: &SubBandLine, from: Seconds, to: Seconds) -> Option<Seconds> {
    let (_, smooth) = smoothed_envelope(line, from, to)?;
    let (low, peak) = low_and_peak(&smooth)?;
    let after = &smooth[low..];
    let first = after.iter().position(|&e| e >= ATTACK_FROM * peak)?;
    let last = after.iter().position(|&e| e >= ATTACK_TO * peak)?;
    Some(Seconds::clamped(
        cast::usize_to_f64(last.saturating_sub(first)) / line.sample_rate().hz().get(),
    ))
}

/// The band envelope inside `from..to`, averaged over a centred
/// [`ONSET_SMOOTHING`], with the index of its first sample.
fn smoothed_envelope(line: &SubBandLine, from: Seconds, to: Seconds) -> Option<(usize, Vec<f64>)> {
    let rate = line.sample_rate().hz().get();
    let half = cast::round_to_usize(0.5 * ONSET_SMOOTHING.get() * rate).unwrap_or(0);
    let envelope = line.envelope();
    let (start, end) = (line.index_at(from), line.index_at(to).min(envelope.len()));
    if start >= end {
        return None;
    }
    let mut sums = Vec::with_capacity(envelope.len() + 1);
    sums.push(0.0);
    for e in envelope {
        sums.push(sums[sums.len() - 1] + e);
    }
    let smooth = (start..end)
        .map(|n| {
            let (a, b) = (n.saturating_sub(half), (n + half + 1).min(envelope.len()));
            (sums[b] - sums[a]) / cast::usize_to_f64(b - a)
        })
        .collect();
    Some((start, smooth))
}

/// The lowest point of a smoothed envelope and the peak after it, or
/// `None` when the lowest point already stands above
/// [`DROP_ONSET_SHARE`] of that peak.
fn low_and_peak(smooth: &[f64]) -> Option<(usize, f64)> {
    let low = (0..smooth.len()).min_by(|&a, &b| smooth[a].total_cmp(&smooth[b]))?;
    let peak = smooth[low..].iter().copied().fold(0.0, f64::max);
    (peak > 0.0 && smooth[low] <= DROP_ONSET_SHARE * peak).then_some((low, peak))
}

/// Fits the drop model to the line from `onset` on, up to `end` at most.
/// `None` when fewer than three line values fall inside the fit span.
#[must_use]
pub fn fit_drop(line: &SubBandLine, onset: Seconds, end: Seconds) -> Option<DropFit> {
    let last = (onset.get() + DROP_FIT_END.get()).min(end.get());
    let from = line.index_at(Seconds::clamped(onset.get() + DROP_FIT_START.get()));
    let to = line.index_at(Seconds::clamped(last)).min(line.hz().len());
    let points: Vec<(f64, f64)> = (from..to)
        .map(|n| (line.time_of(n).get() - onset.get(), line.hz()[n]))
        .collect();
    if points.len() < 3 {
        return None;
    }
    let (lo, hi) = (DROP_TAU_MIN.get().ln(), TAU_MAX.ln());
    let step = (hi - lo) / cast::usize_to_f64(TAU_GRID - 1);
    let sse = |log_tau: f64| solve(&points, log_tau.exp()).2;
    let (best, _) = (0..TAU_GRID)
        .map(|k| lo + step * cast::usize_to_f64(k))
        .map(|log_tau| (log_tau, sse(log_tau)))
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    let (log_tau, _) = golden_section(sse, (best - step).max(lo), (best + step).min(hi), 1e-9);
    let tau = log_tau.exp();
    let (f_end, df, sse) = solve(&points, tau);
    // A fall faster than FAST_TAU ends within a period of a low note, and
    // the average would spread its misfit.
    let rms = if tau >= FAST_TAU.get() {
        period_rms(&points, f_end, df, tau, line.sample_rate())
    } else {
        (sse / cast::usize_to_f64(points.len())).sqrt()
    };
    Some(DropFit {
        onset,
        f_end: Hz::new(f_end).ok()?,
        df_hz: df,
        tau: Seconds::new(tau).ok()?,
        residual: {
            let seen = df.abs() * (-DROP_FIT_START.get() / tau).exp();
            if seen > 0.0 {
                rms / seen
            } else {
                f64::INFINITY
            }
        },
        points: points.len(),
    })
}

/// The RMS of the fit's error averaged over half a period of `f_end`, or
/// over a full period when `tau` spans [`FULL_PERIODS`] periods or more,
/// centred and shortened at the ends.
///
/// The third harmonic of a driven note beats with the fundamental and
/// swings the line at twice the note: a driven 808 left a raw residual of
/// 23 % of its fall. Half a period removes that swing and keeps a slower
/// error, from a wrong model or from the phase restart of a retrigger,
/// which a full period hid: two same-pitch retriggers then read falls of
/// 4 and 11 semitones with a `tau` of 12 ms. A second harmonic swings the
/// line once per period, and a sweep with one at half the fundamental
/// failed on the residual, at a `tau` of 50 to 150 ms.
fn period_rms(points: &[(f64, f64)], f_end: f64, df: f64, tau: f64, rate: SampleRate) -> f64 {
    let errors: Vec<f64> = points
        .iter()
        .map(|&(t, y)| y - f_end - df * (-t / tau).exp())
        .collect();
    let period = if f_end > 0.0 {
        cast::round_to_usize(rate.hz().get() / f_end).unwrap_or(1)
    } else {
        1
    };
    // A fall over FULL_PERIODS periods or longer changes little within
    // one, so a full period removes the swing of the second harmonic too.
    let half = if tau * f_end >= FULL_PERIODS {
        period.max(1) / 2
    } else {
        period.max(1) / 4
    };
    let mut sums = vec![0.0; errors.len() + 1];
    for (k, e) in errors.iter().enumerate() {
        sums[k + 1] = sums[k] + e;
    }
    let squares: f64 = (0..errors.len())
        .map(|k| {
            let (a, b) = (k.saturating_sub(half), (k + half + 1).min(errors.len()));
            ((sums[b] - sums[a]) / cast::usize_to_f64(b - a)).powi(2)
        })
        .sum();
    (squares / cast::usize_to_f64(errors.len())).sqrt()
}

/// The least-squares `f_end` and `df` for one `tau`, and the sum of the
/// squared residuals.
fn solve(points: &[(f64, f64)], tau: f64) -> (f64, f64, f64) {
    let n = cast::usize_to_f64(points.len());
    let g: Vec<f64> = points.iter().map(|&(t, _)| (-t / tau).exp()).collect();
    let (mut sg, mut sgg, mut sy, mut sgy) = (0.0, 0.0, 0.0, 0.0);
    for (&(_, y), &g) in points.iter().zip(&g) {
        sg += g;
        sgg += g * g;
        sy += y;
        sgy += g * y;
    }
    let det = n * sgg - sg * sg;
    let (a, b) = if det.abs() <= f64::EPSILON * n * sgg {
        (sy / n, 0.0)
    } else {
        ((sgg * sy - sg * sgy) / det, (n * sgy - sg * sy) / det)
    };
    let sse = points
        .iter()
        .zip(&g)
        .map(|(&(_, y), &g)| (y - a - b * g).powi(2))
        .sum();
    (a, b, sse)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::SampleRate;

    #[test]
    fn the_convention_states_the_constants() {
        use super::super::line::{LINE_HI, LINE_LO, LINE_LO_FAST};
        use super::super::{
            DROP_BAND_GATE, DROP_NOTE_MATCH, RUN_START_RISE, STRIKE_ATTACK, STRIKE_RISE,
            STRIKE_SPANS, num, ratio_db, says,
        };
        use crate::dsp::ACTIVE_GATE_DB;
        use crate::notes::QUIET_FRAME;
        let ms = |s: f64| num(1000.0 * s);
        let [before, after] = STRIKE_SPANS;
        assert!((DROP_ONSET_SHARE - 0.5).abs() < 1e-12, "half the peak");
        says(
            DROP_CONVENTION,
            &[
                format!(
                    "from {} ms to {} ms after the onset",
                    ms(DROP_FIT_START.get()),
                    ms(DROP_FIT_END.get())
                ),
                format!(
                    "search from {} ms, where exp(-{} ms / tau) = 0.1, to {} s",
                    ms(DROP_TAU_MIN.get()),
                    ms(DROP_FIT_START.get()),
                    num(TAU_MAX)
                ),
                format!(
                    "a fit with tau under {} ms, or after a strike whose full-band RMS rises {}, \
                     is fitted again on the line with its high-pass at {} Hz",
                    ms(FAST_TAU.get()),
                    ratio_db(RUN_START_RISE),
                    num(LINE_LO_FAST.get())
                ),
                format!(
                    "where the {} to {} Hz envelope, averaged over a centred {} ms, reaches half",
                    num(LINE_LO.get()),
                    num(LINE_HI.get()),
                    ms(ONSET_SMOOTHING.get())
                ),
                format!(
                    "RMS over the {} ms after the onset lies more than {} dB below the loudest \
                     {} ms frame of the stem",
                    ms(after.1 - after.0),
                    num(ACTIVE_GATE_DB.get()),
                    ms(QUIET_FRAME.get())
                ),
                format!(
                    "lies more than {} dB below the loudest {} ms frame of that band",
                    num(DROP_BAND_GATE.get()),
                    ms(QUIET_FRAME.get())
                ),
                format!(
                    "a drop with tau under {} ms needs an envelope that takes {} ms or less from \
                     {} to {} % of its peak",
                    ms(FAST_TAU.get()),
                    ms(STRIKE_ATTACK.get()),
                    num(100.0 * ATTACK_FROM),
                    num(100.0 * ATTACK_TO)
                ),
                format!("one that starts at {} Hz or above", num(LINE_HI.get())),
                format!(
                    "the {} ms after the onset {} or more above the {} ms that end {} ms before it",
                    ms(after.1 - after.0),
                    ratio_db(STRIKE_RISE),
                    ms(before.1 - before.0),
                    ms(-before.1)
                ),
                format!(
                    "half a period of f_end when tau is {} ms or more and over a full period \
                     when tau spans {} periods or more",
                    ms(FAST_TAU.get()),
                    num(FULL_PERIODS)
                ),
                format!(
                    "df_st is above {} and at most {}, that fall is above {} semitones too",
                    num(DROP_MIN.get()),
                    num(DROP_MAX.get()),
                    num(DROP_MIN.get())
                ),
                format!("residual is below {}", num(DROP_MAX_RESIDUAL)),
                format!(
                    "within {} cents of a note that sounds",
                    num(DROP_NOTE_MATCH.get())
                ),
            ],
        );
    }

    #[test]
    fn the_fit_recovers_an_exact_exponential() {
        let points: Vec<(f64, f64)> = (0..400)
            .map(|k| {
                let t = 0.007 + cast::usize_to_f64(k) * 0.001;
                (t, 41.2 + 41.2 * (-t / 0.02).exp())
            })
            .collect();
        let (a, b, sse) = solve(&points, 0.02);
        assert!((a - 41.2).abs() < 1e-9 && (b - 41.2).abs() < 1e-9 && sse < 1e-18);
    }

    #[test]
    fn a_constant_line_has_no_drop() {
        let rate = SampleRate::new(22_050).unwrap();
        let x: Vec<f64> = (0..22_050)
            .map(|n| (std::f64::consts::TAU * 55.0 * cast::usize_to_f64(n) / 22_050.0).sin())
            .collect();
        let line = SubBandLine::of(&x, rate, Seconds::new(0.0).unwrap()).unwrap();
        let fit = fit_drop(
            &line,
            Seconds::new(0.3).unwrap(),
            Seconds::new(0.9).unwrap(),
        )
        .unwrap();
        assert!((fit.f_end.get() - 55.0).abs() < 0.05, "{fit:?}");
        assert!(!fit.accepted(), "{fit:?}");
    }

    #[test]
    fn the_drop_in_semitones_follows_its_definition() {
        let fit = DropFit {
            onset: Seconds::new(0.0).unwrap(),
            f_end: Hz::new(41.2).unwrap(),
            df_hz: 41.2,
            tau: Seconds::new(0.02).unwrap(),
            residual: 0.05,
            points: 100,
        };
        assert!((fit.df().unwrap().get() - 12.0).abs() < 1e-12);
        assert!(fit.accepted());
        // A fall with tau 2 ms is mostly over at 7 ms.
        let fast = DropFit {
            tau: Seconds::new(0.002).unwrap(),
            ..fit
        };
        assert!(fast.df().unwrap() > DROP_MIN && fast.visible().unwrap() < DROP_MIN);
        assert!(!fast.accepted());
        // A tau on the bound of the search, with the fall still visible.
        let bound = DropFit {
            tau: DROP_TAU_MIN,
            df_hz: 41.2 * 3.0,
            ..fit
        };
        assert!(bound.visible().unwrap() > DROP_MIN && !bound.accepted());
        assert!(
            DropFit {
                tau: Seconds::new(1.02 * DROP_TAU_MIN.get()).unwrap(),
                ..bound
            }
            .accepted()
        );
        assert!((-DROP_FIT_START.get() / DROP_TAU_MIN.get()).exp() - 0.1 < 1e-12);
        // Three octaves count; more do not.
        let octaves = |n: f64| DropFit {
            df_hz: 41.2 * (2f64.powf(n) - 1.0),
            ..fit
        };
        assert!(octaves(3.0).accepted() && !octaves(3.01).accepted());
        assert!(
            !DropFit {
                residual: 0.15,
                ..fit
            }
            .accepted()
        );
        assert!(
            !DropFit {
                df_hz: 41.2 * (2f64.powf(1.99 / 12.0) - 1.0),
                ..fit
            }
            .accepted()
        );
        assert_eq!(
            DropFit {
                df_hz: -50.0,
                ..fit
            }
            .df(),
            None
        );
    }
}
