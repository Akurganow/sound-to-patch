//! The tail after a note-off (#70): a linear release plus a decaying tail.

use super::POOL_NOTES_MIN;
use super::adsr::NoteEnvelope;
use super::track::HOP;
use crate::audio::{Band, SampleRate, stereo_screen};
use crate::cast;
use crate::dsp::{StftParams, Summary, describe};
use crate::estimate::pitch::fit::nelder_mead;
use crate::notes::Note;
use crate::units::{Decibels, Seconds};

/// A tail RT60 counts only when the frames fitted span this share of it: a
/// fall of 15 dB.
const RT60_SPAN_SHARE: f64 = 0.25;
/// The fit reads the tail down to this far under the sustain level.
pub const TAIL_FLOOR: Decibels = Decibels::from_const(-55.0);
/// The fewest frames of a tail worth a fit.
const TAIL_FRAMES_MIN: usize = 8;
/// The starting releases, in seconds.
const RELEASE_STARTS: [f64; 4] = [0.02, 0.08, 0.3, 1.0];
/// The starting tail RT60s, in seconds.
const RT60_STARTS: [f64; 3] = [0.4, 1.2, 3.0];
/// The starting tail weight.
const WEIGHT_START: f64 = 0.1;
/// A tail reads every frame up to here.
const DENSE_FRAMES: usize = 200;
/// Past [`DENSE_FRAMES`] a tail reads every this many frames.
const SPARSE_STEP: usize = 5;
/// The simplex steps of one fit.
const FIT_ITERATIONS: usize = 400;
/// The releases a fit can read, in seconds. A level that never falls would
/// otherwise run the fit to a release of 1e19 s and more.
pub const TAIL_RELEASE_RANGE: (f64, f64) = (0.001, 10.0);
/// The tail weights a fit can read.
pub const TAIL_WEIGHT_RANGE: (f64, f64) = (1e-4, 10.0);
/// The RT60s a fit can read, in seconds.
pub const TAIL_RT60_RANGE: (f64, f64) = (0.01, 30.0);

/// The tail model fitted to a note after its note-off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TailFit {
    /// The linear release.
    pub release: Seconds,
    /// The weight of the tail at the note-off, over the sustain level.
    pub weight: f64,
    /// The RT60 of the tail.
    pub rt60: Seconds,
    /// The frames fitted span enough of the RT60 to read it.
    pub rt60_observed: bool,
    /// The RMS of the fit in dB, over the frames fitted.
    pub residual: Decibels,
    /// The shortest RT60 that the filter of the track can show.
    pub rt60_floor: Seconds,
}

impl TailFit {
    /// The tail is heavy but fits too smoothly for a reverb, whose level wobbles:
    /// the note's own exponential release, with no linear part to find.
    #[must_use]
    pub fn is_smooth(&self) -> bool {
        self.weight > TAIL_WEIGHT_MAX && self.residual.get() <= TAIL_SMOOTH.get()
    }

    /// The fit shows an RT60 of a tail: one heavy enough, read long enough, past
    /// what the filter shows by itself, and not a release of the note's own.
    #[must_use]
    pub fn reads_rt60(&self) -> bool {
        self.weight >= TAIL_WEIGHT_MIN
            && self.rt60_observed
            && self.rt60.get() >= self.rt60_floor.get()
            && !self.is_smooth()
    }
}

/// The model in dB, `t` seconds after the note-off, no lower than the floor.
fn model_db(t: f64, release: f64, weight: f64, rt60: f64) -> f64 {
    let amplitude = (1.0 - t / release).max(0.0) + weight * 10f64.powf(-3.0 * t / rt60);
    (20.0 * amplitude.max(1e-12).log10()).max(TAIL_FLOOR.get())
}

/// Fits the model to `db`, the level in dB over the sustain level on frames
/// from the note-off. `None` when too few frames lie above the floor.
#[must_use]
pub fn fit_tail(db: &[f64]) -> Option<TailFit> {
    let floor = TAIL_FLOOR.get();
    let last = db.iter().rposition(|v| *v > floor)?;
    if db.iter().filter(|v| **v > floor).count() < TAIL_FRAMES_MIN {
        return None;
    }
    // Past the release's first frames a tail moves slowly, so a sparser read
    // costs less without moving the fit.
    let data: Vec<(f64, f64, f64)> = db[..=last]
        .iter()
        .enumerate()
        .filter(|(i, _)| *i < DENSE_FRAMES || i % SPARSE_STEP == 0)
        .map(|(i, v)| {
            let weight = if i < DENSE_FRAMES {
                1.0
            } else {
                cast::usize_to_f64(SPARSE_STEP)
            };
            (cast::usize_to_f64(i) * HOP.get(), v.max(floor), weight)
        })
        .collect();
    let frames: f64 = data.iter().map(|d| d.2).sum();
    let bounded = |p: &[f64; 3]| {
        (
            p[0].exp().clamp(TAIL_RELEASE_RANGE.0, TAIL_RELEASE_RANGE.1),
            p[1].exp().clamp(TAIL_WEIGHT_RANGE.0, TAIL_WEIGHT_RANGE.1),
            p[2].exp().clamp(TAIL_RT60_RANGE.0, TAIL_RT60_RANGE.1),
        )
    };
    let cost = |p: &[f64; 3]| -> f64 {
        let (release, weight, rt60) = bounded(p);
        data.iter()
            .map(|&(t, v, w)| w * (v - model_db(t, release, weight, rt60)).powi(2))
            .sum()
    };
    let mut best: Option<([f64; 3], f64)> = None;
    for release in RELEASE_STARTS {
        for rt60 in RT60_STARTS {
            let start = [release.ln(), WEIGHT_START.ln(), rt60.ln()];
            let (p, value) = nelder_mead(cost, start, [0.3, 0.5, 0.3], FIT_ITERATIONS, 1e-6);
            if best.is_none_or(|(_, b)| value < b) {
                best = Some((p, value));
            }
        }
    }
    let (p, value) = best?;
    let (release, weight, rt60) = bounded(&p);
    Some(TailFit {
        release: Seconds::clamped(release),
        weight,
        rt60: Seconds::clamped(rt60),
        rt60_observed: cast::usize_to_f64(last) * HOP.get() >= RT60_SPAN_SHARE * rt60,
        residual: Decibels::clamped((value / frames).sqrt()),
        rt60_floor: Seconds::clamped(0.0),
    })
}

/// A tail lighter than this at the note-off has no RT60 to read: on dry notes
/// the fit often leaves a little weight, and its RT60 is noise.
pub const TAIL_WEIGHT_MIN: f64 = 0.05;
/// Over this tail weight the release is not identifiable, and no release is
/// written.
pub const TAIL_WEIGHT_MAX: f64 = 0.5;
/// Over this residual the release is not identifiable, and no release is
/// written.
pub const TAIL_RESIDUAL_MAX: Decibels = Decibels::from_const(3.0);
/// A tail over [`TAIL_WEIGHT_MAX`] that fits within this RMS reads as the note's
/// own exponential release: a reverb's level wobbles more.
pub const TAIL_SMOOTH: Decibels = Decibels::from_const(1.1);
/// The shortest RT60 a band can show is this many periods of its cutoff: a dry
/// chord's release through a band reads 3 to 5 periods, whatever its length up
/// to 50 ms.
pub const TAIL_BAND_PERIODS: f64 = 6.0;
/// The votes that a tail is reverb need this many in favour.
pub const TAIL_VOTES_MIN: usize = 2;
/// A tail outlasts a gap when its level before the next note lies above this
/// level under the sustain level.
pub const GAP_LEVEL: Decibels = Decibels::from_const(-40.0);
/// Side over mid grows by this much from the note body to the tail.
pub const SIDE_GROWTH: Decibels = Decibels::from_const(3.0);
/// The flatness rises by this much from the note body to the tail.
pub const FLATNESS_RISE: f64 = 0.1;
/// The tail rate holds over the notes when its interquartile range stays under
/// this share of its median.
pub const SAME_RATE_SPREAD: f64 = 0.25;
/// The share of the bands from 1 kHz falls by this much from the first half of
/// a tail to the second.
pub const HIGH_BAND_FALL: Decibels = Decibels::from_const(3.0);
/// The body of a note runs up to this long before the knee.
const BODY_SPAN: Seconds = Seconds::from_const(0.2);
/// The level before the next note reads this span.
const GAP_SPAN: Seconds = Seconds::from_const(0.02);
/// The frames of a vote's spectrum.
const VOTE_FRAME: usize = 1024;

/// The votes that a tail is reverb: `None` when nothing could be read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct TailVotes {
    /// The tail outlasts the silent gap before the next note.
    pub outlasts_gap: Option<bool>,
    /// Side over mid grows from the note body to the tail.
    pub side_grows: Option<bool>,
    /// Spectral flatness rises in the tail.
    pub flatness_rises: Option<bool>,
    /// The tail rate is the same for every note.
    pub same_rate: Option<bool>,
    /// The high band decays faster than the low band.
    pub high_band_faster: Option<bool>,
}

impl TailVotes {
    /// Returns the votes in favour.
    #[must_use]
    pub fn count(self) -> usize {
        [
            self.outlasts_gap,
            self.side_grows,
            self.flatness_rises,
            self.same_rate,
            self.high_band_faster,
        ]
        .into_iter()
        .filter(|v| *v == Some(true))
        .count()
    }
}

/// The share of the votes reads as a majority; `None` without any.
fn majority(values: &[bool]) -> Option<bool> {
    (!values.is_empty()).then(|| 2 * values.iter().filter(|v| **v).count() > values.len())
}

fn rms_db(x: &[f64]) -> Option<f64> {
    let energy = x.iter().map(|v| v * v).sum::<f64>();
    (energy > 0.0).then(|| 10.0 * (energy / cast::usize_to_f64(x.len())).log10())
}

/// The sample at `t` seconds in a signal of `len` samples. A time past the
/// last index maps to `len`.
fn sample_index(t: f64, fs: f64, len: usize) -> usize {
    cast::round_to_usize(t.max(0.0) * fs)
        .unwrap_or(len)
        .min(len)
}

/// Reads the votes over the pooled notes that have a tail fit.
pub(super) fn read_votes(
    mono: &[f64],
    stereo: Option<(&[f64], &[f64])>,
    rate: SampleRate,
    notes: &[Note],
    pool: &[&NoteEnvelope],
) -> TailVotes {
    let index = |t: f64| sample_index(t, rate.hz().get(), mono.len());
    let mut gaps = Vec::new();
    let mut sides = Vec::new();
    let mut flats = Vec::new();
    let mut highs = Vec::new();
    let mut rates = Vec::new();
    for note in pool {
        let (Some(knee), Some(fit)) = (note.knee.map(Seconds::get), note.tail) else {
            continue;
        };
        let next = notes.iter().map(|n| n.onset().get()).find(|t| *t > knee);
        let stop = next.map_or(note.end.get(), |n| n.min(note.end.get()));
        let body = index((knee - BODY_SPAN.get()).max(note.onset.get()))..index(knee);
        let tail = index(knee + fit.release.get())..index(stop);
        if fit.reads_rt60() {
            rates.push(fit.rt60.get());
        }
        // Only a gap with no other note in it shows a tail.
        let silent_gap = |next: f64| {
            !notes.iter().any(|n| {
                n.onset() != note.onset
                    && n.onset().get() < next
                    && n.offset().get() > next - GAP_SPAN.get()
            })
        };
        if let Some(next) = next
            .filter(|n| *n - knee > fit.release.get())
            .filter(|n| silent_gap(*n))
        {
            let before = index(next - GAP_SPAN.get())..index(next);
            if let (Some(gap), Some(level)) = (rms_db(&mono[before]), rms_db(&mono[body.clone()])) {
                gaps.push(gap - level > GAP_LEVEL.get());
            }
        }
        if tail.is_empty() || body.is_empty() {
            continue;
        }
        if let Some((left, right)) = stereo {
            let sm = |r: std::ops::Range<usize>| {
                stereo_screen(left.get(r.clone())?, right.get(r)?, rate, None)
                    .ok()
                    .and_then(|s| s.band(Band::Full).sm_db)
                    .map(Decibels::get)
            };
            if let (Some(a), Some(b)) = (sm(body.clone()), sm(tail.clone())) {
                sides.push(b - a >= SIDE_GROWTH.get());
            }
        }
        let spectra = |r: std::ops::Range<usize>| {
            describe(
                &mono[r],
                rate,
                StftParams::new(VOTE_FRAME).ok()?,
                None,
                false,
            )
            .ok()
        };
        if let (Some(a), Some(b)) = (spectra(body), spectra(tail.clone()))
            && let (Some(a), Some(b)) = (a.flatness.value(), b.flatness.value())
        {
            flats.push(b.median().get() - a.median().get() >= FLATNESS_RISE);
        }
        let middle = tail.start + tail.len() / 2;
        if let (Some(a), Some(b)) = (spectra(tail.start..middle), spectra(middle..tail.end)) {
            let high = |d: &crate::dsp::Descriptors| {
                d.band_share.value().map(|bands| {
                    bands
                        .iter()
                        .filter(|p| p.lo.get() >= 1000.0)
                        .map(|p| p.share.get())
                        .sum::<f64>()
                })
            };
            if let (Some(a), Some(b)) = (high(&a), high(&b))
                && a > 0.0
                && b > 0.0
            {
                highs.push(10.0 * (b / a).log10() <= -HIGH_BAND_FALL.get());
            }
        }
    }
    let same_rate = (rates.len() >= POOL_NOTES_MIN)
        .then(|| Summary::of(&rates).is_some_and(|s| s.iqr() <= SAME_RATE_SPREAD * s.median()));
    TailVotes {
        outlasts_gap: majority(&gaps),
        side_grows: majority(&sides),
        flatness_rises: majority(&flats),
        same_rate,
        high_band_faster: majority(&highs),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve(release: f64, weight: f64, rt60: f64) -> Vec<f64> {
        (0..4000)
            .map(|i| model_db(cast::usize_to_f64(i) * HOP.get(), release, weight, rt60))
            .collect()
    }

    #[test]
    fn the_fit_recovers_the_model_it_reads() {
        for (release, weight, rt60) in [(0.05, 0.15, 0.6), (0.2, 0.4, 1.5), (0.02, 0.05, 3.0)] {
            let fit = fit_tail(&curve(release, weight, rt60)).unwrap();
            assert!((fit.release.get() / release - 1.0).abs() < 0.03, "{fit:?}");
            assert!((fit.weight / weight - 1.0).abs() < 0.03, "{fit:?}");
            assert!((fit.rt60.get() / rt60 - 1.0).abs() < 0.03, "{fit:?}");
            assert!(fit.residual.get() < 0.1, "{fit:?}");
        }
    }

    #[test]
    fn a_level_that_never_falls_stays_inside_the_ranges() {
        let fit = fit_tail(&[0.0; 200]).unwrap();
        let inside = |v: f64, (lo, hi): (f64, f64)| (lo..=hi).contains(&v);
        assert!(inside(fit.release.get(), TAIL_RELEASE_RANGE), "{fit:?}");
        assert!(inside(fit.weight, TAIL_WEIGHT_RANGE), "{fit:?}");
        assert!(inside(fit.rt60.get(), TAIL_RT60_RANGE), "{fit:?}");
        assert!(fit.residual.get() < 0.1, "{fit:?}");
        let loud = fit_tail(&[40.0; 200]).unwrap();
        assert!(inside(loud.weight, TAIL_WEIGHT_RANGE), "{loud:?}");
    }

    #[test]
    fn a_time_past_the_last_index_maps_to_the_end() {
        assert_eq!(sample_index(1e30, 44_100.0, 100), 100);
        assert_eq!(sample_index(1.0, 50.0, 100), 50);
        assert_eq!(sample_index(-1.0, 50.0, 100), 0);
    }

    #[test]
    fn a_tail_under_the_floor_or_too_short_has_no_fit() {
        assert_eq!(fit_tail(&[-60.0; 100]), None);
        let mut one_frame = [-60.0; 100];
        one_frame[50] = -1.0;
        assert_eq!(fit_tail(&one_frame), None);
        assert_eq!(fit_tail(&[-1.0; 7]), None);
        assert!(fit_tail(&[-1.0; 8]).is_some());
    }

    #[test]
    fn an_rt60_counts_over_a_fall_of_15_db_of_it() {
        let short: Vec<f64> = curve(0.05, 0.3, 3.0)[..500].to_vec();
        assert!(!fit_tail(&short).unwrap().rt60_observed);
        assert!(fit_tail(&curve(0.05, 0.3, 3.0)).unwrap().rt60_observed);
    }

    #[test]
    fn the_votes_count_only_those_in_favour() {
        let votes = TailVotes {
            outlasts_gap: Some(true),
            side_grows: Some(false),
            flatness_rises: None,
            same_rate: Some(true),
            high_band_faster: None,
        };
        assert_eq!(votes.count(), 2);
        assert_eq!(TailVotes::default().count(), 0);
    }

    #[test]
    fn a_majority_needs_more_than_half() {
        assert_eq!(majority(&[]), None);
        assert_eq!(majority(&[true, false]), Some(false));
        assert_eq!(majority(&[true, true, false]), Some(true));
    }
}
