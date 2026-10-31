//! The sidechain pump: a level dip that repeats on every beat, read on the
//! beats that a sustained note covers.

use crate::audio::BeatGrid;
use crate::cast;
use crate::dsp::{ModulationSpectrum, SCAN_TOP_HZ, TRACK_RATE_HZ, Track, quantile, snap};
use crate::notes::NoteList;
use crate::units::{Bpm, Cents, Decibels, Hz, Seconds};

use super::median;
use super::track::{HOP, LevelTrack};

/// The phase bins of one beat.
pub const PUMP_BINS: usize = 64;
/// Each beat's curve is taken relative to this quantile of its bins, the
/// level between dips.
pub const PUMP_REFERENCE: f64 = 0.9;
/// The recovery ends where the curve comes back within this much of the
/// reference.
pub const PUMP_BACK: Decibels = Decibels::from_const(1.5);
/// A dip at least this deep counts as a pump, and the layer strips it from
/// the envelope tracks.
///
/// The beating of a constant three-note saw chord reads 1.64 to 1.94 dB,
/// and a 6 dB duck reads 3.2 to 5.8 dB.
pub const PUMP_MIN: Decibels = Decibels::from_const(3.0);
/// The pump needs this many beats that a sustained note covers.
pub const PUMP_BEATS_MIN: usize = 4;
/// The layer strips a beat-locked pump only over this many counted beats.
///
/// With fewer, half the beats is two or three, and other sounds on the
/// beat pass the lock: a sine sub under a kick on every beat, whose tail
/// beats against the sub, read 3.4 to 4.6 dB locked to the beat over 4 or
/// 5 beats.
pub const PUMP_STRIP_BEATS: usize = 8;
/// A beat dips with the curve when its level falls within this many bins
/// of the curve's dip.
pub const PHASE_SLACK: usize = 3;
/// A beat dips with the curve when its level falls by this share of the
/// curve's depth: from its highest bin over the [`FALL_MAX`] bins before
/// the dip, to its lowest within [`PHASE_SLACK`] bins of it.
///
/// The bins before the dip run on into the beat before, as the level does,
/// and do not wrap round the beat. A level that rises over each bar or
/// each two beats and drops back makes every beat's curve start low: on
/// the wrap of the beat, that reads as a fall at bin 0 on every beat.
///
/// A beat's own lowest bin is no test either: the slow beating of a chord
/// moves it. At 174 BPM a 6 dB duck on the saw chord read 5.4 dB, and only
/// 5 of 15 beats had their lowest bin within 3 bins of the curve's.
pub const PHASE_DEPTH: f64 = 0.5;
/// The pump locks to the beat when at least this share of the beats dips
/// with the curve.
pub const PHASE_SHARE: f64 = 0.5;
/// The pump searches for its beat period this far either side of the
/// grid's, as a share of it.
///
/// A sidechain follows the song's own tempo, and a stem's tempo may be off
/// by 1 or 2 %. Over 32 beats a 1 % error moves the duck by a third of a
/// beat, so its beats no longer fold onto one curve.
pub const PERIOD_RANGE: f64 = 0.025;
/// The period search's first stage reads at most this many beats of the
/// longest run; the second refines over all counted beats.
///
/// Its cost grows with the square of the beats it reads: a 10-minute held
/// chord took 277 s over its whole run, against 6.5 s without the layer.
pub const PERIOD_RUN_BEATS: usize = 64;
/// A tremolo runs free when its rate lies this far from every note
/// division of the tempo.
pub const FREE_RESIDUAL: Cents = Cents::from_const(50.0);
/// A free line lies within the period search's reach when it lies this far
/// or nearer from the nearest division.
///
/// The search reaches 46 cents, and the line of a duck 2.5 % off reads up
/// to 52 cents over a short span. 60 cents also holds a duck 3 % off.
pub const FREE_REACH: Cents = Cents::from_const(60.0);
/// A duck takes at least this many bins, and [`DUCK_TAIL_WINDOWS`] more,
/// from half its depth back to a quarter of it.
///
/// A gate comes back at once: at 128 BPM the median is 1 bin for a gate and
/// 7 to 13 for a duck's release of 80 ms or more.
pub const DUCK_TAIL_BINS: f64 = 3.0;
/// The shares of the level window that [`DUCK_TAIL_BINS`] grows by.
///
/// The window smears a gate's edge over its own length, and a bin's time
/// follows the tempo. A flat 5 bins let a gate through at 55 Hz and 174 BPM,
/// and cost ducks at 60 and 80 BPM.
pub const DUCK_TAIL_WINDOWS: f64 = 0.5;
/// The period search's read replaces a read that locks on the grid when it
/// reads at least this much deeper.
///
/// A smeared duck still locks on the grid: 12 dB, 2.5 % off over 16 beats,
/// read 4.4 dB against 9.8 dB at its own period. The search also deepens a
/// duck on the tempo by fitting a chord's beating: at 1.5 dB, 34 of 4847
/// generated grid locks move by up to 0.22 %; at 1 dB, 231 by up to 1.95 %.
pub const FIT_GAIN: Decibels = Decibels::from_const(1.5);
/// A tremolo counts when its line swings the level by at least this much
/// either way. The slow beating of a chord's near-coinciding harmonics
/// stays below it.
pub const FREE_SWING: Decibels = Decibels::from_const(1.0);
/// A beat counts when one note covers it and started at least this long
/// before it. A pluck struck on the beat then never counts, even when the
/// transcriber places its onset a few milliseconds early.
pub const SUSTAIN_LEAD: Seconds = Seconds::from_const(0.05);
/// A pump falls into its dip within this many bins.
///
/// The fall counts from the last bin before the dip that lies within
/// [`PUMP_BACK`] of the reference, on the median of the two-beat curves,
/// so a dip early in the beat falls from the end of the beat before. A
/// sidechain ducks within milliseconds;
/// a note decaying across the beat falls over most of it and comes back
/// at the next beat at once.
pub const FALL_MAX: usize = 8;
/// The lowest tremolo rate searched.
pub const FREE_RATE_MIN: Hz = Hz::from_const(0.5);

/// What a level dip follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PumpLock {
    /// A dip at one phase of every beat: a sidechain pump.
    Beat,
    /// Dips only on beats where a note starts: the notes' own envelopes.
    Note,
    /// A level modulation at a rate tied to no note division: a tremolo.
    Free,
    /// No dip and no tremolo.
    None,
}

/// The pump read of a stem.
#[derive(Clone, Debug, PartialEq)]
pub struct Pump {
    /// How far the median beat curve dips below its reference.
    pub depth: Decibels,
    /// The time from the dip until the curve comes back within
    /// [`PUMP_BACK`]; `None` when it does not within the beat.
    pub back: Option<Seconds>,
    /// `back / ln(depth / 1.5)`, the time constant of an exponential
    /// recovery; `None` without `back` and for any lock but the beat: a dip
    /// that follows no beat has no recovery to model.
    pub tau: Option<Seconds>,
    /// What the dip follows.
    pub lock: PumpLock,
    /// The beats a sustained note covers.
    pub beats: usize,
    /// The median curve over those beats, in dB from the reference, one
    /// value per phase bin.
    pub curve: Vec<f64>,
    grid: BeatGrid,
    /// For a beat lock, each beat's fall into the dip, by beat of the grid;
    /// `None` where a beat holds no frames.
    falls: Vec<Option<f64>>,
    /// The cents from the nearest division to the level's strongest line
    /// over the longest run of counted beats; `None` under [`FREE_SWING`].
    free_line: Option<f64>,
}

impl Pump {
    /// Reads the pump of a stem from its full-band level track.
    ///
    /// Only beats that the held part of one note covers from start to end
    /// count, so a pluck that decays inside its beat reads no pump and a
    /// release at the end of a beat reads no dip. `held` holds the start
    /// and end of each held part, in seconds; `notes` gives the onsets for
    /// the note lock. Each counted beat is binned into [`PUMP_BINS`] phase
    /// bins and taken relative to its [`PUMP_REFERENCE`] quantile; the
    /// median over the beats is the curve. `None` with fewer than
    /// [`PUMP_BEATS_MIN`] counted beats.
    ///
    /// The beats are read on the grid and again at the period, within
    /// [`PERIOD_RANGE`] of it, whose beats fold onto the deepest mean
    /// curve. A second read that locks to the beat replaces the first when
    /// the first:
    ///
    /// - locks to the beat, and the second reads [`FIT_GAIN`] deeper and does
    ///   not give up the strip, as a slower period counts fewer beats;
    /// - runs free on a line within [`FREE_REACH`], and the second has a
    ///   duck's shape: over a short span, a duck near the search's end reads
    ///   free. A gate near a division folds onto one curve as well, but
    ///   comes back at once;
    /// - follows the notes or nothing.
    #[must_use]
    pub fn find(
        track: &LevelTrack,
        grid: BeatGrid,
        notes: &NoteList,
        held: &[(f64, f64)],
    ) -> Option<Self> {
        let read = Self::read(track, grid, notes, held)?;
        let Some(fitted) = fit(track, grid, held)
            .and_then(|g| Self::read(track, g, notes, held))
            .filter(|p| p.lock == PumpLock::Beat)
        else {
            return Some(read);
        };
        let replaces = match read.lock {
            PumpLock::Beat => {
                fitted.depth.get() >= read.depth.get() + FIT_GAIN.get()
                    && (fitted.strips() || !read.strips())
            }
            PumpLock::Free => {
                read.free_line
                    .is_some_and(|cents| cents <= FREE_REACH.get())
                    && fitted.has_a_ducks_shape(track.window().get())
            }
            PumpLock::Note | PumpLock::None => true,
        };
        Some(if replaces { fitted } else { read })
    }

    /// Reads the pump on the beats of one grid.
    fn read(
        track: &LevelTrack,
        grid: BeatGrid,
        notes: &NoteList,
        held: &[(f64, f64)],
    ) -> Option<Self> {
        let period = grid.beat_period().get();
        let mut beats = Vec::new();
        let mut onset_beats = Vec::new();
        let mut odd = Vec::new();
        let mut longest = (0.0, 0.0);
        let mut run: Option<(usize, f64)> = None;
        for b in covered(track, grid, held) {
            let start = grid.beat_time(b).get();
            let end = start + period;
            let first = match run {
                Some((last, first)) if b == last + 1 => first,
                _ => start,
            };
            run = Some((b, first));
            let Some(curve) = beat_curve(track, start, period, 1) else {
                continue;
            };
            if end - first > longest.1 - longest.0 {
                longest = (first, end);
            }
            onset_beats.push(starts_in(notes, start, end));
            odd.push(b % 2 == 1);
            beats.push(curve);
        }
        if beats.len() < PUMP_BEATS_MIN {
            return None;
        }
        let wide = median_curve(&beats);
        let curve = wide[PUMP_BINS..].to_vec();
        let depth = dip(&curve);
        let low = PUMP_BINS + dip_bin(&curve);
        let falls: Vec<f64> = beats.iter().map(|b| fall_into(b, low)).collect();
        let free_line = free_line(track, longest, grid);
        let lock = lock(&beats, &onset_beats, &odd, &falls, &wide, depth, || {
            free_line.is_some_and(|cents| cents >= FREE_RESIDUAL.get())
        });
        let falls = if lock == PumpLock::Beat {
            let end = track.time(track.db().len()) - grid.offset().get();
            (0..cast::floor_to_usize(end / period).unwrap_or(0))
                .map(|b| {
                    beat_curve(track, grid.beat_time(b).get(), period, 1)
                        .map(|c| fall_into(&c, low))
                })
                .collect()
        } else {
            Vec::new()
        };
        let back = recovery(&curve).map(|bins| bins * period / cast::usize_to_f64(PUMP_BINS));
        let tau = back
            .filter(|_| lock == PumpLock::Beat)
            .map(|back| back / (depth / PUMP_BACK.get()).ln());
        Some(Self {
            depth: Decibels::clamped(depth),
            back: back.map(Seconds::clamped),
            tau: tau.map(Seconds::clamped),
            lock,
            beats: beats.len(),
            curve,
            grid,
            falls,
            free_line,
        })
    }

    /// Returns whether the beats over `from` to `to` carry the duck: their
    /// median fall into the dip is at least [`PHASE_DEPTH`] of its depth.
    /// A stem can duck in one section only, and stripping a note outside it
    /// would write the curve's ripple into its envelope. True when no beat
    /// over the span has frames.
    #[must_use]
    pub fn carries(&self, from: f64, to: f64) -> bool {
        let (period, offset) = (self.grid.beat_period().get(), self.grid.offset().get());
        let first = cast::floor_to_usize((from - offset) / period).unwrap_or(0);
        let last = cast::ceil_to_usize((to - offset) / period)
            .unwrap_or(0)
            .min(self.falls.len());
        let falls: Vec<f64> = self.falls[first.min(last)..last]
            .iter()
            .flatten()
            .copied()
            .collect();
        median(&falls).is_none_or(|fall| fall >= PHASE_DEPTH * self.depth.get())
    }

    /// Returns whether the layer strips the pump: it locks to the beat,
    /// and so dips at least [`PUMP_MIN`], over [`PUMP_STRIP_BEATS`] or more
    /// counted beats.
    #[must_use]
    pub fn strips(&self) -> bool {
        self.lock == PumpLock::Beat && self.beats >= PUMP_STRIP_BEATS
    }

    /// Returns the tempo the pump was read at: the given tempo, or the
    /// fitted one when that read replaced the read on the grid.
    #[must_use]
    pub fn bpm(&self) -> Bpm {
        self.grid.bpm()
    }

    /// Whether the curve falls faster than it recovers, and takes long
    /// enough from half its depth back to a quarter: [`DUCK_TAIL_BINS`] and
    /// [`DUCK_TAIL_WINDOWS`] of a level window of `window` seconds.
    fn has_a_ducks_shape(&self, window: f64) -> bool {
        let low = dip_bin(&self.curve);
        let depth = self.depth.get();
        let first = |at: &dyn Fn(usize) -> usize, level: f64| {
            (1..PUMP_BINS)
                .find(|&k| self.curve[at(k) % PUMP_BINS] >= -level * depth)
                .unwrap_or(PUMP_BINS)
        };
        let (after, before) = (|k| low + k, |k| low + PUMP_BINS - k);
        let rise = first(&after, 0.25);
        let bins = cast::usize_to_f64(PUMP_BINS) * window / self.grid.beat_period().get();
        let tail = cast::usize_to_f64(rise.saturating_sub(first(&after, 0.5)));
        tail >= DUCK_TAIL_BINS + DUCK_TAIL_WINDOWS * bins && rise > first(&before, 0.25)
    }

    /// Returns the curve at time `t`, linear between bin centres and
    /// wrapping over the beat.
    #[must_use]
    pub fn at(&self, t: f64) -> f64 {
        let period = self.grid.beat_period().get();
        let phase = ((t - self.grid.offset().get()) / period).rem_euclid(1.0);
        let bins = cast::usize_to_f64(PUMP_BINS);
        let x = (phase * bins - 0.5).rem_euclid(bins);
        let lo = cast::floor_to_usize(x).unwrap_or(0).min(PUMP_BINS - 1);
        let hi = (lo + 1) % PUMP_BINS;
        let w = x - cast::usize_to_f64(lo);
        self.curve[lo] * (1.0 - w) + self.curve[hi] * w
    }

    /// Adds the dip back to a level track, so the track shows the note's
    /// own envelope.
    pub fn strip(&self, track: &mut LevelTrack) {
        let start = track.time(0);
        for (i, db) in track.db_mut().iter_mut().enumerate() {
            *db -= self.at(start + cast::usize_to_f64(i) * HOP.get());
        }
    }
}

/// The beats of `grid` that the held part of one note covers from
/// [`SUSTAIN_LEAD`] before the beat to its end, inside the track, in order.
fn covered(track: &LevelTrack, grid: BeatGrid, held: &[(f64, f64)]) -> Vec<usize> {
    let (period, offset) = (grid.beat_period().get(), grid.offset().get());
    let end = track.time(track.db().len());
    let mut beats: Vec<usize> = held
        .iter()
        .flat_map(|&(on, off)| {
            let first = cast::ceil_to_usize((on + SUSTAIN_LEAD.get() - offset) / period - 1e-9);
            let last = cast::floor_to_usize((off.min(end) - offset) / period + 1e-9);
            first.unwrap_or(0)..last.unwrap_or(0)
        })
        .collect();
    beats.sort_unstable();
    beats.dedup();
    beats
}

/// Whether a note starts in the beat from `start` to `end`: an onset up to
/// [`SUSTAIN_LEAD`] before the beat belongs to it.
fn starts_in(notes: &NoteList, start: f64, end: f64) -> bool {
    let lead = SUSTAIN_LEAD.get();
    notes
        .notes()
        .iter()
        .any(|n| (start - lead..end - lead).contains(&n.onset().get()))
}

/// The grid at the beat period, within [`PERIOD_RANGE`] of `grid`'s, whose
/// counted beats fold onto the deepest mean curve; `None` when that is the
/// grid's own period, or with fewer than [`PUMP_BEATS_MIN`] beats in a row.
///
/// The search runs over up to [`PERIOD_RUN_BEATS`] beats of the longest run
/// of counted beats first, then over all of them around the best period
/// of the run. Each step moves the last
/// beat of the span searched by 1 bin.
fn fit(track: &LevelTrack, grid: BeatGrid, held: &[(f64, f64)]) -> Option<BeatGrid> {
    let beats = covered(track, grid, held);
    let (mut longest, mut run) = ((0, 0), (0, 0));
    for (i, &b) in beats.iter().enumerate() {
        run = if i > 0 && b == beats[i - 1] + 1 {
            (run.0, run.1 + 1)
        } else {
            (b, 1)
        };
        if run.1 > longest.1 {
            longest = run;
        }
    }
    if longest.1 < PUMP_BEATS_MIN {
        return None;
    }
    let time = |b: usize| grid.beat_time(b).get();
    let at = |share: f64| {
        BeatGrid::new(
            Bpm::clamped(grid.bpm().get() * (1.0 + share)),
            grid.offset(),
        )
    };
    let step = |(from, to): (f64, f64)| {
        grid.beat_period().get() / (cast::usize_to_f64(PUMP_BINS) * (to - from))
    };
    let search = |centre: f64, half: f64, span: (f64, f64)| -> Option<f64> {
        let step = step(span);
        let n = cast::floor_to_usize(half / step).unwrap_or(0);
        (0..=2 * n)
            .map(|k| centre + (cast::usize_to_f64(k) - cast::usize_to_f64(n)) * step)
            .map(|share| (share, folded_depth(track, at(share), held, span)))
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(share, _)| share)
    };
    let run = (
        time(longest.0),
        time(longest.0 + longest.1.min(PERIOD_RUN_BEATS)),
    );
    let all = (time(beats[0]), time(beats[beats.len() - 1] + 1));
    let coarse = search(0.0, PERIOD_RANGE, run)?;
    let share = search(coarse, step(run), all)?;
    (share != 0.0).then(|| at(share))
}

/// The depth of the mean curve over the counted beats of `grid` inside
/// `(from, to)`.
fn folded_depth(
    track: &LevelTrack,
    grid: BeatGrid,
    held: &[(f64, f64)],
    (from, to): (f64, f64),
) -> f64 {
    let period = grid.beat_period().get();
    let mut sum = [0.0; PUMP_BINS];
    let mut count = 0.0;
    for b in covered(track, grid, held) {
        let start = grid.beat_time(b).get();
        if start < from - 1e-9 || start + period > to + 1e-9 {
            continue;
        }
        if let Some(curve) = beat_curve(track, start, period, 0) {
            for (s, c) in sum.iter_mut().zip(curve) {
                *s += c;
            }
            count += 1.0;
        }
    }
    if count == 0.0 {
        return 0.0;
    }
    dip(&sum.map(|s| s / count))
}

/// How far a two-beat curve falls into the dip at bin `low`: its highest
/// bin over the [`FALL_MAX`] bins before the [`PHASE_SLACK`] bins either
/// side of `low`, minus its lowest bin within those.
fn fall_into(curve: &[f64], low: usize) -> f64 {
    let near = low - PHASE_SLACK..(low + PHASE_SLACK + 1).min(curve.len());
    let top = curve[near.start - FALL_MAX..near.start]
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let bottom = curve[near].iter().copied().fold(f64::INFINITY, f64::min);
    top - bottom
}

/// The mean level of each phase bin of the beat from `start` and of
/// `before` beats before it, minus the [`PUMP_REFERENCE`] quantile of the
/// beat's own bins. `None` when a bin holds no frame.
fn beat_curve(track: &LevelTrack, start: f64, period: f64, before: usize) -> Option<Vec<f64>> {
    let count = (before + 1) * PUMP_BINS;
    let mut sums = vec![0.0; count];
    let mut counts = vec![0_usize; count];
    let bins = cast::usize_to_f64(PUMP_BINS);
    let first = start - cast::usize_to_f64(before) * period;
    for i in track.index(first.max(track.time(0)))..track.index(start + period) {
        let phase = (track.time(i) - start) / period;
        // A frame can read a rounding error before the first bin.
        let bin = if phase >= 0.0 || before == 0 {
            cast::floor_to_usize(phase * bins)
                .unwrap_or(0)
                .min(PUMP_BINS - 1)
                + count
                - PUMP_BINS
        } else {
            cast::floor_to_usize((phase + cast::usize_to_f64(before)) * bins)
                .unwrap_or(0)
                .min(count - PUMP_BINS - 1)
        };
        sums[bin] += track.db()[i];
        counts[bin] += 1;
    }
    if counts.contains(&0) {
        return None;
    }
    let means: Vec<f64> = sums
        .iter()
        .zip(counts)
        .map(|(s, c)| s / cast::usize_to_f64(c))
        .collect();
    let mut sorted = means[count - PUMP_BINS..].to_vec();
    sorted.sort_by(f64::total_cmp);
    let reference = quantile(&sorted, PUMP_REFERENCE)?;
    Some(means.into_iter().map(|m| m - reference).collect())
}

/// The median over beats of each bin.
fn median_curve<B: AsRef<[f64]>>(beats: &[B]) -> Vec<f64> {
    (0..beats.first().map_or(0, |b| b.as_ref().len()))
        .map(|k| median(&beats.iter().map(|b| b.as_ref()[k]).collect::<Vec<_>>()).unwrap_or(0.0))
        .collect()
}

/// How far a curve dips below 0 dB, at least 0.
fn dip(curve: &[f64]) -> f64 {
    -curve.iter().copied().fold(0.0, f64::min)
}

/// The bin of a curve's lowest value.
fn dip_bin(curve: &[f64]) -> usize {
    curve
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.total_cmp(b.1))
        .map_or(0, |(k, _)| k)
}

/// The bins from the dip until the curve first comes back within
/// [`PUMP_BACK`], going on over the beat's wrap and interpolated between
/// bins; `None` when it does not within one beat. The grid starts at 0 s,
/// so a stem whose duck sits late in the beat recovers after the wrap.
fn recovery(curve: &[f64]) -> Option<f64> {
    let low = dip_bin(curve);
    let back = -PUMP_BACK.get();
    let at = |k: usize| curve[(low + k) % PUMP_BINS];
    (1..PUMP_BINS).find(|&k| at(k) >= back).map(|k| {
        let (a, b) = (at(k - 1), at(k));
        let w = if b > a { (back - a) / (b - a) } else { 1.0 };
        cast::usize_to_f64(k - 1) + w.clamp(0.0, 1.0)
    })
}

/// The bins from the last bin before bin `low` that lies within
/// [`PUMP_BACK`] of the reference, to `low`, on a two-beat curve.
fn fall_to(curve: &[f64], low: usize) -> usize {
    (1..=low)
        .find(|&k| curve[low - k] >= -PUMP_BACK.get())
        .unwrap_or(PUMP_BINS)
}

/// The dips of a curve around the beat's wrap: a dip starts where the
/// curve falls half its depth or more below the reference and ends where
/// it comes back within a quarter of its depth. The gap between the two
/// keeps the ripple of a chord's beating inside one dip from counting as
/// two.
fn dips(curve: &[f64], depth: f64) -> usize {
    // Start the scan where the curve is up, so no dip straddles the wrap.
    let Some(start) = (0..PUMP_BINS).find(|&k| curve[k] >= -0.25 * depth) else {
        return 1;
    };
    let mut inside = false;
    let mut count = 0;
    for k in 0..PUMP_BINS {
        let v = curve[(start + k) % PUMP_BINS];
        if !inside && v <= -0.5 * depth {
            inside = true;
            count += 1;
        } else if inside && v >= -0.25 * depth {
            inside = false;
        }
    }
    count
}

/// What a dip follows, tested in this order.
///
/// - `Note`: the beats with a note onset inside dip by [`PUMP_MIN`] or
///   more, in their median curve, and the other beats by under half of
///   it. Each group needs two beats.
/// - `Free`: `free` finds a tremolo. It comes before the beat: a 5.3 Hz
///   tremolo over 17 beats at 128 BPM left a 3.6 dB dip in the median
///   curve that passed the beat test. A duck's strongest line lies on the
///   beat rate, so it reads free only when that rate lies
///   [`FREE_RESIDUAL`] or more from every division of the grid; `find` then
///   tries the period search.
/// - `Beat`: the curve dips by [`PUMP_MIN`] or more, once per beat, and
///   falls into its dip within [`FALL_MAX`] bins, and at least
///   [`PHASE_SHARE`] of the beats fall by [`PHASE_DEPTH`] of its depth
///   into the curve's dip: `falls` holds each beat's fall from
///   [`fall_into`].
///
/// `beats` and `wide` hold two-beat curves, the beat before and the beat;
/// `wide` is their median.
/// - `None` otherwise.
fn lock(
    beats: &[Vec<f64>],
    onset_beats: &[bool],
    odd: &[bool],
    falls: &[f64],
    wide: &[f64],
    depth: f64,
    free: impl FnOnce() -> bool,
) -> PumpLock {
    let curve = &wide[PUMP_BINS..];
    let median_of = |flags: &[bool], value: bool| -> Option<Vec<f64>> {
        let group: Vec<&Vec<f64>> = beats
            .iter()
            .zip(flags)
            .filter(|&(_, &f)| f == value)
            .map(|(b, _)| b)
            .collect();
        (group.len() >= 2).then(|| median_curve(&group))
    };
    let group = |with_onset: bool| median_of(onset_beats, with_onset).map(|c| dip(&c[PUMP_BINS..]));
    if let (Some(with), Some(without)) = (group(true), group(false))
        && with >= PUMP_MIN.get()
        && without < 0.5 * PUMP_MIN.get()
    {
        return PumpLock::Note;
    }
    if free() {
        return PumpLock::Free;
    }
    let low = PUMP_BINS + dip_bin(curve);
    let half_falls = |parity: bool| {
        median_of(odd, parity).is_some_and(|half| {
            fall_to(&half, low) <= FALL_MAX && half[low] <= -PHASE_DEPTH * depth
        })
    };
    if depth >= PUMP_MIN.get()
        && fall_to(wide, low) <= FALL_MAX
        && dips(curve, depth) == 1
        && half_falls(false)
        && half_falls(true)
    {
        let near = falls.iter().filter(|&&f| f >= PHASE_DEPTH * depth).count();
        if cast::usize_to_f64(near) >= PHASE_SHARE * cast::usize_to_f64(falls.len()) {
            return PumpLock::Beat;
        }
    }
    PumpLock::None
}

/// The cents from the nearest division to the strongest line of the
/// modulation spectrum between [`FREE_RATE_MIN`] and 20 Hz, over a span of
/// the level. `None` when that line swings by under [`FREE_SWING`].
fn free_line(track: &LevelTrack, (start, end): (f64, f64), grid: BeatGrid) -> Option<f64> {
    let (lo, hi) = (track.index(start), track.index(end));
    let block = cast::round_to_usize(1.0 / (TRACK_RATE_HZ.get() * HOP.get())).unwrap_or(5);
    // Each block averages in dB, as the curve does, so its mean is the
    // geometric mean of the amplitude. The scan takes the natural log of
    // the amplitude itself, so it reads that same mean level.
    let values: Vec<f64> = track.db()[lo..hi]
        .chunks_exact(block.max(1))
        .map(|c| 10f64.powf(c.iter().sum::<f64>() / cast::usize_to_f64(c.len()) / 20.0))
        .collect();
    let track = Track::new(values, TRACK_RATE_HZ);
    let spectrum = ModulationSpectrum::of(&track);
    let rate = spectrum.rate(FREE_RATE_MIN, SCAN_TOP_HZ).ok()?;
    let &rate = rate.value()?;
    // A sine of amplitude `A` peaks at `A N / 4` under the Hann window.
    let amplitude =
        spectrum.magnitude_at(rate.get()) * 4.0 / (track.span().get() * TRACK_RATE_HZ.get());
    // The amplitude is in natural-log units; 20 / ln 10 turns it into dB.
    let swing = amplitude * 20.0 / std::f64::consts::LN_10;
    (swing >= FREE_SWING.get()).then(|| snap(rate, grid.bpm()).residual.get().abs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::Bpm;

    #[test]
    fn the_constants_pass_the_unit_check() {
        assert_eq!(PUMP_BACK, Decibels::new(1.5).unwrap());
        assert_eq!(PUMP_MIN, Decibels::new(3.0).unwrap());
        assert_eq!(FREE_RESIDUAL, Cents::new(50.0).unwrap());
        assert_eq!(FREE_SWING, Decibels::new(1.0).unwrap());
        assert_eq!(FREE_RATE_MIN, Hz::new(0.5).unwrap());
        assert_eq!(SUSTAIN_LEAD, Seconds::new(0.05).unwrap());
    }

    fn grid() -> BeatGrid {
        BeatGrid::new(Bpm::new(120.0).unwrap(), Seconds::clamped(0.0))
    }

    #[test]
    fn the_counted_beats_follow_the_grid_offset() {
        let track = LevelTrack::new(Seconds::clamped(0.0), vec![0.0; 16_000]);
        let grid = BeatGrid::new(Bpm::new(120.0).unwrap(), Seconds::clamped(0.6));
        // Beat 1 starts at 1.1 s, 100 ms after the note and past the lead;
        // beat 29 ends at 15.6 s, and beat 30 at 16.1 s, past the note.
        assert_eq!(
            covered(&track, grid, &[(1.0, 16.0)]),
            (1..30).collect::<Vec<_>>()
        );
    }

    #[test]
    fn an_onset_up_to_the_lead_before_a_beat_belongs_to_it() {
        let starts = |onset: f64| {
            let note = crate::notes::Note::new(
                Seconds::clamped(onset),
                Seconds::clamped(onset + 5.0),
                crate::units::MidiPitch::new(45.0).unwrap(),
            )
            .unwrap();
            starts_in(&NoteList::new(vec![note]), 1.0, 1.5)
        };
        assert!(starts(0.97));
        assert!(!starts(0.93), "70 ms before the beat");
        assert!(starts(1.43), "70 ms before the next beat");
        assert!(!starts(1.47), "30 ms before the next beat");
    }

    #[test]
    fn the_folded_depth_is_the_mean_over_the_beats_whatever_their_count() {
        // A 6 dB dip over the first 50 ms of each 500 ms beat.
        let db: Vec<f64> = (0..4_000)
            .map(|i| if i % 500 < 50 { -6.0 } else { 0.0 })
            .collect();
        let track = LevelTrack::new(Seconds::clamped(0.0), db);
        for beats in [4.0, 7.0] {
            let depth = folded_depth(&track, grid(), &[(-1.0, 4.0)], (0.0, 0.5 * beats));
            assert!((depth - 6.0).abs() < 1e-9, "{beats}: {depth}");
        }
    }

    #[test]
    fn the_recovery_interpolates_between_bins() {
        let mut curve = vec![0.0; PUMP_BINS];
        curve[0] = -12.0;
        curve[1] = -6.0;
        curve[2] = -2.0;
        curve[3] = -0.5;
        // -1.5 lies a third of the way from bin 2 to bin 3.
        assert!((recovery(&curve).unwrap() - (2.0 + 1.0 / 3.0)).abs() < 1e-12);
        assert!(recovery(&vec![-4.0; PUMP_BINS]).is_none());
        // The same dip moved to the last bin recovers over the wrap.
        curve.rotate_left(1);
        assert!((recovery(&curve).unwrap() - (2.0 + 1.0 / 3.0)).abs() < 1e-12);
    }

    /// A curve with one fast dip of `depth` dB at bin `at`.
    fn dipped(at: usize, depth: f64) -> Vec<f64> {
        let mut curve = vec![0.0; PUMP_BINS];
        curve[at] = -depth;
        curve[at + 1] = -0.5 * depth;
        curve
    }

    /// The parity of 8 beats in a row.
    const ALTERNATE: [bool; 8] = [false, true, false, true, false, true, false, true];

    /// The two-beat curve of a flat beat before one with that dip.
    fn wide(at: usize, depth: f64) -> Vec<f64> {
        [vec![0.0; PUMP_BINS], dipped(at, depth)].concat()
    }

    #[test]
    fn the_beat_lock_needs_half_the_beats_to_fall_into_the_dip() {
        let curve = wide(10, 8.0);
        let beats: Vec<Vec<f64>> = (0..8).map(|_| wide(10, 8.0)).collect();
        let never = || false;
        // 4 of 8 beats fall 4 dB into the dip, half the curve's depth.
        let mut falls = [4.0, 4.0, 4.0, 4.0, 3.9, 1.0, 1.0, 0.0];
        assert_eq!(
            lock(&beats, &[false; 8], &ALTERNATE, &falls, &curve, 8.0, never),
            PumpLock::Beat
        );
        // 3 of 8: under half.
        falls[3] = 3.9;
        assert_eq!(
            lock(&beats, &[false; 8], &ALTERNATE, &falls, &curve, 8.0, never),
            PumpLock::None
        );
    }

    #[test]
    fn the_beat_lock_needs_the_curve_to_fall_from_the_beat_before() {
        // A level that rises over each beat and drops back at the next:
        // its curve wraps from 0 dB at bin 63 to -6 dB at bin 0, but the
        // beat before ends where the beat starts.
        let ramp: Vec<f64> = (0..PUMP_BINS)
            .map(|k| -6.0 * (1.0 - cast::usize_to_f64(k) / 63.0))
            .collect();
        let rising = [ramp.iter().map(|v| v - 6.0).collect(), ramp.clone()].concat();
        let beats = vec![rising.clone(); 8];
        assert_eq!(
            lock(
                &beats,
                &[false; 8],
                &ALTERNATE,
                &[6.0; 8],
                &rising,
                6.0,
                || false
            ),
            PumpLock::None
        );
        // The same beat after a flat one falls into its dip at once.
        let ducked = [vec![0.0; PUMP_BINS], ramp].concat();
        let beats = vec![ducked.clone(); 8];
        assert_eq!(
            lock(
                &beats,
                &[false; 8],
                &ALTERNATE,
                &[6.0; 8],
                &ducked,
                6.0,
                || false
            ),
            PumpLock::Beat
        );
    }

    #[test]
    fn the_beat_lock_needs_the_odd_and_the_even_beats_to_dip() {
        // A dip on every second beat only: the even beats dip 8 dB, the odd
        // ones are flat, and the median curve still dips 8 dB.
        let beats: Vec<Vec<f64>> = (0..9)
            .map(|i| {
                if i % 2 == 0 {
                    wide(10, 8.0)
                } else {
                    vec![0.0; 2 * PUMP_BINS]
                }
            })
            .collect();
        let odd: Vec<bool> = (0..9).map(|i| i % 2 == 1).collect();
        let curve = median_curve(&beats);
        assert!((dip(&curve[PUMP_BINS..]) - 8.0).abs() < 1e-12);
        assert_eq!(
            lock(&beats, &[false; 9], &odd, &[8.0; 9], &curve, 8.0, || false),
            PumpLock::None
        );
        // The odd beats dip, but under half the curve's depth at its dip.
        let beats: Vec<Vec<f64>> = (0..9)
            .map(|i| wide(10, if i % 2 == 0 { 8.0 } else { 3.9 }))
            .collect();
        assert_eq!(
            lock(&beats, &[false; 9], &odd, &[8.0; 9], &curve, 8.0, || false),
            PumpLock::None
        );
        let beats = vec![wide(10, 8.0); 9];
        assert_eq!(
            lock(&beats, &[false; 9], &odd, &[8.0; 9], &curve, 8.0, || false),
            PumpLock::Beat
        );
    }

    #[test]
    fn a_beat_falls_from_the_bins_before_its_dip_to_the_lowest_near_it() {
        // The dip at bin 10 of the beat: the 3 bins either side, 71 to 77,
        // and the 8 bins before them, 63 to 70.
        let mut curve = vec![-1.0; 2 * PUMP_BINS];
        curve[63] = 0.5;
        curve[77] = -9.0;
        assert!((fall_into(&curve, 74) - 9.5).abs() < 1e-12);
        let mut curve = vec![-1.0; 2 * PUMP_BINS];
        curve[62] = 0.5;
        curve[78] = -9.0;
        assert!(fall_into(&curve, 74).abs() < 1e-12);
        // A dip late in the beat looks no further than its last bin.
        let mut curve = vec![-1.0; 2 * PUMP_BINS];
        curve[126] = -9.0;
        assert!((fall_into(&curve, 126) - 8.0).abs() < 1e-12);
    }

    #[test]
    fn the_note_lock_needs_the_other_beats_under_half_the_floor() {
        let curve = wide(10, 4.0);
        let onsets = [true, true, false, false];
        let with = |other: f64| {
            let beats = vec![
                wide(10, 4.0),
                wide(10, 4.0),
                wide(30, other),
                wide(30, other),
            ];
            lock(
                &beats,
                &onsets,
                &ALTERNATE[..4],
                &[0.0; 4],
                &curve,
                4.0,
                || false,
            )
        };
        assert_eq!(with(1.0), PumpLock::Note);
        assert_ne!(with(1.6), PumpLock::Note);
    }

    #[test]
    fn a_note_takes_the_strip_by_the_beats_after_the_grid_offset() {
        let pump = Pump {
            depth: Decibels::clamped(8.0),
            back: None,
            tau: None,
            lock: PumpLock::Beat,
            beats: 8,
            curve: vec![0.0; PUMP_BINS],
            grid: BeatGrid::new(Bpm::new(120.0).unwrap(), Seconds::clamped(0.6)),
            falls: [0.0, 0.0, 8.0, 8.0, 8.0, 8.0, 8.0, 8.0].map(Some).to_vec(),
            free_line: None,
        };
        assert!(!pump.carries(0.6, 1.6), "beats 0 and 1 do not fall");
        assert!(pump.carries(1.6, 2.6), "beats 2 and 3 fall");
    }

    #[test]
    fn a_ducks_shape_recovers_slower_than_it_falls() {
        // A dip of 8 dB at bin 32, by the bins it takes each way.
        let shaped = |fall: f64, rise: f64, window: f64| {
            let curve: Vec<f64> = (0..PUMP_BINS)
                .map(|bin| {
                    let k = cast::usize_to_f64(bin) - 32.0;
                    let width = if k < 0.0 { fall } else { rise };
                    -8.0 * (1.0 - k.abs() / width).max(0.0)
                })
                .collect();
            Pump {
                depth: Decibels::clamped(8.0),
                back: None,
                tau: None,
                lock: PumpLock::Beat,
                beats: 8,
                curve,
                grid: grid(),
                falls: Vec::new(),
                free_line: None,
            }
            .has_a_ducks_shape(window)
        };
        // Beat 0.5 s: 128 bins to the second, so a 16 ms window spans 2 bins
        // and asks 4 bins of the tail. A rise of `r` bins has a tail of `r / 4`.
        assert!(shaped(4.0, 16.0, 0.0156));
        assert!(!shaped(4.0, 12.0, 0.0156), "3 bins under a window");
        assert!(shaped(4.0, 12.0, 0.0));
        assert!(!shaped(4.0, 8.0, 0.0), "2 bins without a window");
        assert!(!shaped(40.0, 24.0, 0.0), "falls slower than it recovers");
        assert!(!shaped(24.0, 24.0, 0.0), "falls as it recovers");
    }

    #[test]
    fn the_curve_follows_the_grid_offset() {
        let mut curve = vec![0.0; PUMP_BINS];
        curve[0] = -8.0;
        let pump = Pump {
            depth: Decibels::clamped(8.0),
            back: None,
            tau: None,
            lock: PumpLock::Beat,
            beats: 8,
            curve,
            grid: BeatGrid::new(Bpm::new(120.0).unwrap(), Seconds::clamped(0.1)),
            falls: Vec::new(),
            free_line: None,
        };
        assert!((pump.at(0.1 + 0.5 / 128.0) + 8.0).abs() < 1e-9);
        assert!(pump.at(0.5 / 128.0).abs() < 1e-9);
    }

    #[test]
    fn the_curve_wraps_between_the_last_and_the_first_bin() {
        let mut curve = vec![0.0; PUMP_BINS];
        curve[0] = -8.0;
        let pump = Pump {
            depth: Decibels::clamped(8.0),
            back: None,
            tau: None,
            lock: PumpLock::Beat,
            beats: 4,
            curve,
            grid: grid(),
            falls: Vec::new(),
            free_line: None,
        };
        // Beat 0.5 s; bin 0's centre at 0.5 / 128 s, the last bin's at
        // 0.5 - 0.5 / 128 s, and the beat start halfway between them.
        assert!((pump.at(0.5 / 128.0) + 8.0).abs() < 1e-9);
        assert!((pump.at(0.0) + 4.0).abs() < 1e-9);
        assert!((pump.at(1.0) + 4.0).abs() < 1e-9);
    }

    proptest::proptest! {
        #[test]
        fn the_dips_and_the_recovery_do_not_depend_on_where_the_beat_starts(
            curve in proptest::collection::vec(-20.0..0.0f64, PUMP_BINS),
            shift in 0..PUMP_BINS,
        ) {
            let mut rotated = curve.clone();
            rotated.rotate_left(shift);
            let depth = dip(&curve);
            proptest::prop_assert_eq!(dips(&curve, depth), dips(&rotated, depth));
            proptest::prop_assert_eq!(recovery(&curve), recovery(&rotated));
        }
    }
}
