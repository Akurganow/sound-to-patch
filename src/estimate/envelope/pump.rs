//! The sidechain pump: a level dip that repeats on every beat, read on the
//! beats that a sustained note covers.

use crate::audio::BeatGrid;
use crate::cast;
use crate::dsp::{ModulationSpectrum, SCAN_TOP_HZ, TRACK_RATE_HZ, Track, quantile, snap};
use crate::notes::NoteList;
use crate::units::{Cents, Decibels, Hz, Seconds};

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
/// The beating of a constant three-note saw chord reads 1.6 to 1.8 dB, and
/// a 6 dB duck reads 5.6 to 6.1 dB.
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
/// A beat dips with the curve when it falls within this many bins of the
/// curve's dip.
pub const PHASE_SLACK: usize = 3;
/// A beat dips with the curve when it falls there by this share of the
/// curve's depth.
///
/// A beat's own lowest bin is no test: the slow beating of a chord moves
/// it. At 174 BPM a 6 dB duck on the saw chord read 5.4 dB, and only 5 of
/// 15 beats had their lowest bin within 3 bins of the curve's.
pub const PHASE_DEPTH: f64 = 0.5;
/// The pump locks to the beat when at least this share of the beats dips
/// with the curve.
pub const PHASE_SHARE: f64 = 0.5;
/// A tremolo runs free when its rate lies this far from every note
/// division of the tempo.
pub const FREE_RESIDUAL: Cents = Cents::from_const(50.0);
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
/// [`PUMP_BACK`] of the reference. A sidechain ducks within milliseconds;
/// a note decaying across the beat falls over most of it and comes back
/// at the next beat at once.
pub const FALL_MAX: usize = 8;
/// The lowest tremolo rate searched.
pub const FREE_RATE_MIN: Hz = Hz::from_const(0.5);

/// What a level dip follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
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
    /// recovery; `None` without `back`.
    pub tau: Option<Seconds>,
    /// What the dip follows.
    pub lock: PumpLock,
    /// The beats a sustained note covers.
    pub beats: usize,
    /// The median curve over those beats, in dB from the reference, one
    /// value per phase bin.
    pub curve: Vec<f64>,
    grid: BeatGrid,
}

impl Pump {
    /// Reads the pump of a stem from its full-band level track.
    ///
    /// Only beats that the held part of one note covers from start to end
    /// count, so a pluck that decays inside its beat reads no pump and a
    /// release at the end of a beat reads no dip. `held` holds the start
    /// and end of each held part, in seconds; `notes` gives the onsets for
    /// the note lock. Each counted beat is
    /// binned into [`PUMP_BINS`] phase bins and taken relative to its
    /// [`PUMP_REFERENCE`] quantile; the median over the beats is the
    /// curve. `None` with fewer than [`PUMP_BEATS_MIN`] counted beats.
    #[must_use]
    pub fn find(
        track: &LevelTrack,
        grid: BeatGrid,
        notes: &NoteList,
        held: &[(f64, f64)],
    ) -> Option<Self> {
        let period = grid.beat_period().get();
        let mut beats = Vec::new();
        let mut onset_beats = Vec::new();
        let mut longest = (0.0, 0.0);
        let mut run: Option<(f64, f64)> = None;
        for b in 0.. {
            let start = grid.beat_time(b).get();
            let end = start + period;
            if end > track.time(track.db().len()) {
                break;
            }
            let covered = held
                .iter()
                .any(|&(on, off)| on <= start - SUSTAIN_LEAD.get() && off >= end);
            if !covered {
                run = None;
                continue;
            }
            let Some(curve) = beat_curve(track, start, period) else {
                continue;
            };
            let span = run.map_or((start, end), |(s, _)| (s, end));
            run = Some(span);
            if span.1 - span.0 > longest.1 - longest.0 {
                longest = span;
            }
            let onset_inside = notes
                .notes()
                .iter()
                .any(|n| n.onset().get() > start && n.onset().get() < end);
            onset_beats.push(onset_inside);
            beats.push(curve);
        }
        if beats.len() < PUMP_BEATS_MIN {
            return None;
        }
        let curve = median_curve(&beats);
        let depth = dip(&curve);
        let lock = lock(&beats, &onset_beats, &curve, depth, || {
            free_tremolo(track, longest, grid)
        });
        let back = recovery(&curve).map(|bins| bins * period / cast::usize_to_f64(PUMP_BINS));
        let tau = back
            .filter(|_| depth > PUMP_BACK.get())
            .map(|back| back / (depth / PUMP_BACK.get()).ln());
        Some(Self {
            depth: Decibels::clamped(depth),
            back: back.map(Seconds::clamped),
            tau: tau.map(Seconds::clamped),
            lock,
            beats: beats.len(),
            curve,
            grid,
        })
    }

    /// Returns whether the layer strips the pump: it locks to the beat,
    /// and so dips at least [`PUMP_MIN`], over [`PUMP_STRIP_BEATS`] or more
    /// counted beats.
    #[must_use]
    pub fn strips(&self) -> bool {
        self.lock == PumpLock::Beat && self.beats >= PUMP_STRIP_BEATS
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
        let times: Vec<f64> = (0..track.db().len()).map(|i| track.time(i)).collect();
        for (db, t) in track.db_mut().iter_mut().zip(times) {
            *db -= self.at(t);
        }
    }
}

/// One beat's curve: the mean level of each phase bin, minus the
/// [`PUMP_REFERENCE`] quantile of the bins. `None` when a bin holds no
/// frame.
fn beat_curve(track: &LevelTrack, start: f64, period: f64) -> Option<Vec<f64>> {
    let mut sums = [0.0; PUMP_BINS];
    let mut counts = [0_usize; PUMP_BINS];
    let bins = cast::usize_to_f64(PUMP_BINS);
    for i in track.index(start)..track.index(start + period) {
        let phase = (track.time(i) - start) / period;
        let bin = cast::floor_to_usize(phase * bins)
            .unwrap_or(0)
            .min(PUMP_BINS - 1);
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
    let mut sorted = means.clone();
    sorted.sort_by(f64::total_cmp);
    let reference = quantile(&sorted, PUMP_REFERENCE)?;
    Some(means.into_iter().map(|m| m - reference).collect())
}

/// The median over beats of each bin.
fn median_curve(beats: &[Vec<f64>]) -> Vec<f64> {
    (0..PUMP_BINS)
        .map(|k| {
            let mut values: Vec<f64> = beats.iter().map(|b| b[k]).collect();
            values.sort_by(f64::total_cmp);
            quantile(&values, 0.5).unwrap_or(0.0)
        })
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

/// The bins from the last bin before the dip, going back over the beat's
/// wrap, that lies within [`PUMP_BACK`] of the reference, to the dip.
fn fall(curve: &[f64]) -> usize {
    let low = dip_bin(curve);
    (1..PUMP_BINS)
        .find(|&k| curve[(low + PUMP_BINS - k) % PUMP_BINS] >= -PUMP_BACK.get())
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
///   beat rate, a division, so it never reads free.
/// - `Beat`: the curve dips by [`PUMP_MIN`] or more, once per beat, and
///   falls into its dip within [`FALL_MAX`] bins, and at least
///   [`PHASE_SHARE`] of the beats fall by [`PHASE_DEPTH`] of its depth
///   within [`PHASE_SLACK`] bins of the curve's dip.
/// - `None` otherwise.
fn lock(
    beats: &[Vec<f64>],
    onset_beats: &[bool],
    curve: &[f64],
    depth: f64,
    free: impl FnOnce() -> bool,
) -> PumpLock {
    let group = |with_onset: bool| -> Option<f64> {
        let group: Vec<Vec<f64>> = beats
            .iter()
            .zip(onset_beats)
            .filter(|&(_, &o)| o == with_onset)
            .map(|(b, _)| b.clone())
            .collect();
        (group.len() >= 2).then(|| dip(&median_curve(&group)))
    };
    if let (Some(with), Some(without)) = (group(true), group(false))
        && with >= PUMP_MIN.get()
        && without < 0.5 * PUMP_MIN.get()
    {
        return PumpLock::Note;
    }
    if free() {
        return PumpLock::Free;
    }
    if depth >= PUMP_MIN.get() && fall(curve) <= FALL_MAX && dips(curve, depth) == 1 {
        let low = dip_bin(curve);
        let near = beats
            .iter()
            .filter(|b| {
                (0..=2 * PHASE_SLACK)
                    .map(|k| b[(low + PUMP_BINS + k - PHASE_SLACK) % PUMP_BINS])
                    .fold(f64::INFINITY, f64::min)
                    <= -PHASE_DEPTH * depth
            })
            .count();
        if cast::usize_to_f64(near) >= PHASE_SHARE * cast::usize_to_f64(beats.len()) {
            return PumpLock::Beat;
        }
    }
    PumpLock::None
}

/// Whether the level over the longest covered span swings at a rate tied
/// to no note division: a line of the modulation spectrum between
/// [`FREE_RATE_MIN`] and 20 Hz that swings by [`FREE_SWING`] or more, and
/// lies [`FREE_RESIDUAL`] or more from the nearest division.
fn free_tremolo(track: &LevelTrack, (start, end): (f64, f64), grid: BeatGrid) -> bool {
    let (lo, hi) = (track.index(start), track.index(end));
    let block = cast::round_to_usize(1.0 / (TRACK_RATE_HZ.get() * HOP.get())).unwrap_or(5);
    // The scan takes the natural log of the amplitude itself.
    let values: Vec<f64> = track.db()[lo..hi]
        .chunks_exact(block.max(1))
        .map(|c| 10f64.powf(c.iter().sum::<f64>() / cast::usize_to_f64(c.len()) / 20.0))
        .collect();
    let track = Track::new(values, TRACK_RATE_HZ);
    let spectrum = ModulationSpectrum::of(&track);
    let Ok(rate) = spectrum.rate(FREE_RATE_MIN, SCAN_TOP_HZ) else {
        return false;
    };
    let Some(&rate) = rate.value() else {
        return false;
    };
    // A sine of amplitude `A` peaks at `A N / 4` under the Hann window.
    let amplitude =
        spectrum.magnitude_at(rate.get()) * 4.0 / (track.span().get() * TRACK_RATE_HZ.get());
    // The amplitude is in natural-log units; 20 / ln 10 turns it into dB.
    let swing = amplitude * 20.0 / std::f64::consts::LN_10;
    swing >= FREE_SWING.get() && snap(rate, grid.bpm()).residual.get().abs() >= FREE_RESIDUAL.get()
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

    #[test]
    fn the_beat_lock_needs_half_the_beats_to_dip_near_the_curve() {
        let curve = dipped(10, 8.0);
        let never = || false;
        // 4 of 8 beats dip 8 dB at bin 10, the others 1 dB: half dip.
        let mut beats: Vec<Vec<f64>> = (0..4).map(|_| dipped(10, 8.0)).collect();
        beats.extend((0..4).map(|_| dipped(10, 1.0)));
        assert_eq!(
            lock(&beats, &[false; 8], &curve, 8.0, never),
            PumpLock::Beat
        );
        // 3 of 8: under half, and a 1 dB dip is no dip with the curve.
        beats[3] = dipped(10, 1.0);
        assert_eq!(
            lock(&beats, &[false; 8], &curve, 8.0, never),
            PumpLock::None
        );
        // A beat dips with the curve up to 3 bins away, on either side.
        let beats: Vec<Vec<f64>> = (0..8)
            .map(|i| {
                if i < 5 {
                    dipped(13, 8.0)
                } else {
                    dipped(7, 8.0)
                }
            })
            .collect();
        assert_eq!(
            lock(&beats, &[false; 8], &curve, 8.0, never),
            PumpLock::Beat
        );
        let beats: Vec<Vec<f64>> = (0..8)
            .map(|i| {
                if i < 5 {
                    dipped(7, 8.0)
                } else {
                    dipped(13, 8.0)
                }
            })
            .collect();
        assert_eq!(
            lock(&beats, &[false; 8], &curve, 8.0, never),
            PumpLock::Beat
        );
        let beats: Vec<Vec<f64>> = (0..8).map(|_| dipped(14, 8.0)).collect();
        assert_eq!(
            lock(&beats, &[false; 8], &curve, 8.0, never),
            PumpLock::None
        );
    }

    #[test]
    fn the_note_lock_needs_the_other_beats_under_half_the_floor() {
        let curve = dipped(10, 4.0);
        let onsets = [true, true, false, false];
        let with = |other: f64| {
            let beats = vec![
                dipped(10, 4.0),
                dipped(10, 4.0),
                dipped(30, other),
                dipped(30, other),
            ];
            lock(&beats, &onsets, &curve, 4.0, || false)
        };
        assert_eq!(with(1.0), PumpLock::Note);
        assert_ne!(with(1.6), PumpLock::Note);
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
