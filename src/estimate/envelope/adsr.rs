//! The amplitude envelope of one note: attack, hold, decay, sustain, the
//! note-off knee and the peak level.

use crate::audio::SampleRate;
use crate::cast;
use crate::dsp::{FilterError, fit_line, quantile};
use crate::notes::Note;
use crate::units::{Decibels, Hz, MidiPitch, Seconds};

use super::knee::find_knee;
use super::median;
use super::pump::Pump;
use super::track::{
    HARMONIC_CUTOFF_MAX, HARMONIC_CUTOFF_MIN, HOP, LevelTrack, RMS_PERIODS, rms_window,
};

/// How little the level may rise after the attack's top.
///
/// The attack ends at its top: the first frame from the onset that the
/// level exceeds by less than this over as long again as it took to rise to
/// it from under 10 % of it, and at least over the shortest rise the track
/// can tell. The hold starts where the level first comes within this much
/// of the peak.
///
/// The peak of the whole note is no top: on a 10 ms attack that swells by
/// 2 to 3 dB over 600 ms, the rise to 90 % of the peak read 400 to 525 ms.
pub const ATTACK_TOP: Decibels = Decibels::from_const(1.0);
/// A linear ramp spends 80 % of its time between 10 % and 90 % of its
/// peak, so the 10-90 % rise times 1.25 is the full attack.
pub const RAMP_FACTOR: f64 = 1.25;
/// A rise shorter than this many RMS windows cannot be told from a
/// shorter one: the attack is an upper bound.
///
/// The window is one period of the fundamental, or whole periods of at
/// least 2 ms, and a periodic wave gives one amplitude sample per period.
/// On generated saws a 1 ms attack rose in 0.75 windows at 55 Hz and 1.1
/// at 220 Hz; a 5 ms attack at 220 Hz, 1.1 periods, rose in 5.0 ms, under
/// 1.5 windows; a 10 ms attack, 2.2 periods, rose in 7.5 ms.
pub const ATTACK_PERIODS_MIN: f64 = 1.5;
/// The 10-90 % rise of a harmonic band track for an instant step, times
/// its low-pass cutoff: 0.490 to 0.491 at cutoffs of 12 to 40 Hz on a
/// generated 220 Hz sine.
pub const BAND_STEP_RISE: f64 = 0.49;
/// The attack of an overlapping note, read on its harmonic band, is an
/// upper bound when its rise is shorter than this many times the band's
/// rise for an instant step.
pub const BAND_STEPS_MIN: f64 = 2.0;
/// A plateau within [`ATTACK_TOP`] of the peak counts as hold when it
/// lasts longer than this.
pub const HOLD_MIN: Seconds = Seconds::from_const(0.01);
/// The sustain level is the median of this last share of the held part.
pub const SUSTAIN_SHARE: f64 = 0.3;
/// The decay time is read between these shares of the fall in dB from the
/// peak to the sustain level, as the attack is read between 10 % and 90 %
/// of its rise.
///
/// A decay linear in dB reads its time exactly. On generated notes whose
/// decay was linear in amplitude, a least-squares line in dB from -1 dB to
/// 1 dB above the sustain level read 6 to 22 % long; a fall to -3 dB, the
/// shallowest sustain, read 9 to 19 % long even when linear in dB, since
/// the line spanned a third of it.
pub const DECAY_SHARES: (f64, f64) = (0.1, 0.9);
/// The first share of the decay lies at least this far below the peak.
///
/// The band filter of the amp proxy rings after a fast attack by a few
/// tenths of a dB. A sustain of -3 dB put the 10 % point 0.3 dB down,
/// where the ringing moved it: decays of 50 ms read up to 1.5 times long.
pub const DECAY_FIRST_DB: Decibels = Decibels::from_const(0.5);
/// Decay and sustain are observable when the held part lasts the attack,
/// the hold and this many decay times, or when the note has no knee.
pub const OBSERVABLE_DECAYS: f64 = 3.0;
/// The tracks are read no lower than this far below the note's peak, so
/// the floor of a silent frame does not steer a fit.
pub const FIT_FLOOR: Decibels = Decibels::from_const(60.0);
/// A note whose amp proxy peaks at or below this has nothing to read: a
/// list of notes over a silent stretch would read 0 dB sustains at the
/// tracks' floor. 16-bit dither sits near -96 dB.
pub const SILENCE_DB: Decibels = Decibels::from_const(-120.0);
/// The tracks start this long before the onset, so the attack's start is
/// inside them.
pub const TRACK_LEAD: Seconds = Seconds::from_const(0.03);
/// Each note's tracks read its span and this much on either side.
const SPAN_MARGIN: Seconds = Seconds::from_const(0.3);
/// The knee search stops this long before the note's end.
///
/// On dry saws the transcriber's end falls 25 to 60 ms after the release
/// ends (#70). The low edge keeps the whole release inside the search, so
/// a 20 ms release still shows; #70's own cap of 50 ms left 10 ms of it.
pub const KNEE_END_GAP: Seconds = Seconds::from_const(0.025);
/// A harmonic of an overlapping note is free when it lies at least this
/// far from every harmonic of the notes it overlaps.
pub const FREE_GAP: Hz = Hz::from_const(30.0);
/// The amp proxy is the lowest free harmonic up to this one.
///
/// A filter that closes moves the high harmonics, not the fundamental. On
/// a generated 110 Hz saw falling 20 dB in 1.15 s under a low-pass that
/// sweeps from 4 kHz down to the fundamental, the fundamental read the
/// fall as 1.091 s, harmonics 1 to 3 summed 1.056 s and the full band
/// 1.033 s.
pub const PROXY_HARMONICS: u32 = 3;

/// The envelope of one note.
#[derive(Clone, Debug, PartialEq)]
pub struct NoteEnvelope {
    /// The note's onset.
    pub onset: Seconds,
    /// The note's end in the note list, or the stem's end when that comes
    /// first.
    pub end: Seconds,
    /// The note's pitch.
    pub pitch: MidiPitch,
    /// The peak of the amp proxy, in dB of the harmonic's power.
    pub peak_db: Decibels,
    /// The 10-90 % rise times [`RAMP_FACTOR`]; `None` when the level does
    /// not start below 10 % of its peak.
    pub attack: Option<Seconds>,
    /// The rise is shorter than [`ATTACK_PERIODS_MIN`] RMS windows, or for
    /// an overlapping note [`BAND_STEPS_MIN`] times its band's rise for an
    /// instant step, so `attack` is an upper bound.
    pub attack_upper_bound: bool,
    /// The plateau after the attack; 0 when none lasts [`HOLD_MIN`].
    pub hold: Seconds,
    /// The time from the peak to the sustain level, read between 10 % and
    /// 90 % of the fall in dB; `None` when not observable or when the level
    /// holds at the peak.
    pub decay: Option<Seconds>,
    /// The sustain level from the peak; `None` when not observable.
    pub sustain: Option<Decibels>,
    /// The note-off knee; `None` when no knee shows.
    pub knee: Option<Seconds>,
    /// The swing of the level over the sustain window, the 90th minus the
    /// 10th percentile after its line; `None` when the sustain is not
    /// observable.
    pub ripple: Option<Decibels>,
    /// The RMS of the held part, in dB, against the model of the hold, the
    /// decay line and the sustain level; `None` when not observable.
    pub residual: Option<Decibels>,
    /// Another note sounds during this one, so the attack reads the amp
    /// proxy, not the full band.
    pub overlap: bool,
}

/// Why a note has no envelope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteSkip {
    /// None of the note's first harmonics can be read: each lies within
    /// [`FREE_GAP`] of a harmonic of a note it overlaps, as in an octave or
    /// a unison, or its band reaches 0 Hz or the Nyquist frequency.
    NoFreeHarmonic,
    /// The note is too short for a track.
    TooShort,
    /// The note's amp proxy peaks at or below [`SILENCE_DB`].
    Silent,
}

/// Reads the envelope of `note` from the mono mix. `others` are the notes
/// that sound during it; `pump` is stripped first when it locks to the
/// beat.
///
/// # Errors
///
/// Returns a [`FilterError`] when a harmonic band reaches the Nyquist
/// frequency.
pub fn read_note(
    mono: &[f64],
    rate: SampleRate,
    note: &Note,
    others: &[&Note],
    pump: Option<&Pump>,
) -> Result<Result<NoteEnvelope, NoteSkip>, FilterError> {
    let f0 = Hz::from(note.pitch());
    let (from, to) = span(mono, rate, note);
    if to.get() - note.onset().get() < 2.0 * HOLD_MIN.get() {
        return Ok(Err(NoteSkip::TooShort));
    }
    let partials = proxy_partials(f0, others, rate);
    if partials.is_empty() {
        return Ok(Err(NoteSkip::NoFreeHarmonic));
    }
    let overlap = !others.is_empty();
    let (proxy, timing) = tracks(mono, rate, (from, to), f0, &partials, overlap, pump)?;
    let onset = proxy.index(note.onset().get());
    let peak_db = proxy.db()[onset..]
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    if peak_db <= SILENCE_DB.get() {
        return Ok(Err(NoteSkip::Silent));
    }
    let resolution = if overlap {
        BAND_STEPS_MIN * BAND_STEP_RISE / partials[0].1.get()
    } else {
        ATTACK_PERIODS_MIN * rms_window(f0, RMS_PERIODS).get()
    };
    let (attack, attack_upper_bound) = match rise(&timing, onset, resolution) {
        Some(rise) => (
            Some(Seconds::clamped(RAMP_FACTOR * rise)),
            rise < resolution,
        ),
        None => (None, false),
    };
    let rel: Vec<f64> = proxy
        .db()
        .iter()
        .map(|db| (db - peak_db).max(-FIT_FLOOR.get()))
        .collect();
    let top = (onset..rel.len())
        .find(|&i| rel[i] >= -ATTACK_TOP.get())
        .unwrap_or(onset);
    let knee_end = proxy.index(to.get() - KNEE_END_GAP.get());
    let amp: Vec<f64> = rel[top..knee_end.max(top)]
        .iter()
        .map(|db| 10f64.powf(db / 20.0))
        .collect();
    let knee = find_knee(proxy.time(top), &amp);
    let held_end = knee.map_or(knee_end, |k| proxy.index(k)).max(top + 1);
    let window = cast::floor_to_usize(SUSTAIN_SHARE * cast::usize_to_f64(held_end - top))
        .unwrap_or(0)
        .max(1);
    let sustain_window = &rel[held_end - window..held_end];
    let sustain = median(sustain_window).unwrap_or(0.0);
    // The fall is read from the peak: the frames between the first one
    // within 1 dB of it and the peak still rise.
    let peak = (top..held_end)
        .max_by(|&a, &b| rel[a].total_cmp(&rel[b]))
        .unwrap_or(top);
    let fall = (sustain < -ATTACK_TOP.get())
        .then(|| decay_fall(&rel, peak, held_end, sustain))
        .flatten();
    // The decay starts a tenth of its time before its 10 % point; the hold
    // runs from the attack's end to there.
    let plateau = fall.map_or(0.0, |(start, _)| {
        start - cast::usize_to_f64(top) * HOP.get()
    });
    let hold = if plateau > HOLD_MIN.get() {
        plateau
    } else {
        0.0
    };
    let decay = fall.map(|(_, decay)| decay);
    let held_time = cast::usize_to_f64(held_end - onset) * HOP.get();
    // A note with no knee is a pluck that decays until it ends: its
    // sustain is its end level and is always observable (#70).
    let observable = knee.is_none()
        || held_time
            >= attack.map_or(0.0, Seconds::get) + hold + OBSERVABLE_DECAYS * decay.unwrap_or(0.0);
    Ok(Ok(NoteEnvelope {
        onset: note.onset(),
        end: to,
        pitch: note.pitch(),
        peak_db: Decibels::clamped(peak_db),
        attack,
        attack_upper_bound,
        hold: Seconds::clamped(hold),
        decay: decay.filter(|_| observable).map(Seconds::clamped),
        sustain: observable.then(|| Decibels::clamped(sustain)),
        knee: knee.map(Seconds::clamped),
        ripple: observable
            .then(|| ripple(sustain_window))
            .flatten()
            .map(Decibels::clamped),
        residual: observable
            .then(|| residual(&rel[top..held_end], hold, decay, sustain))
            .flatten()
            .map(Decibels::clamped),
        overlap,
    }))
}

/// The span of a note's tracks: from [`TRACK_LEAD`] before its onset to its
/// end, inside the stem.
fn span(mono: &[f64], rate: SampleRate, note: &Note) -> (Seconds, Seconds) {
    let stem_end = cast::usize_to_f64(mono.len()) / rate.hz().get();
    (
        Seconds::clamped(note.onset().get() - TRACK_LEAD.get()),
        Seconds::clamped(note.offset().get().min(stem_end)),
    )
}

/// The RMS in dB of the held part against its model: 0 dB over the hold,
/// a fall linear in dB from 0 dB to `sustain` over the decay, then
/// `sustain`. `None` for an empty part.
fn residual(part: &[f64], hold: f64, decay: Option<f64>, sustain: f64) -> Option<f64> {
    if part.is_empty() {
        return None;
    }
    let decay = decay.unwrap_or(0.0);
    let model = |t: f64| {
        if t < hold {
            0.0
        } else if t < hold + decay {
            sustain * (t - hold) / decay
        } else {
            sustain
        }
    };
    let sum: f64 = part
        .iter()
        .enumerate()
        .map(|(i, db)| (db - model(cast::usize_to_f64(i) * HOP.get())).powi(2))
        .sum();
    Some((sum / cast::usize_to_f64(part.len())).sqrt())
}

/// The amp proxy of `note` against the notes that sound with it, from
/// [`TRACK_LEAD`] before its onset to its end, in dB of the harmonic's
/// power; `None` when no harmonic is free.
///
/// # Errors
///
/// Returns a [`FilterError`] when a harmonic band reaches the Nyquist
/// frequency.
pub fn amp_proxy(
    mono: &[f64],
    rate: SampleRate,
    note: &Note,
    others: &[&Note],
) -> Result<Option<LevelTrack>, FilterError> {
    let f0 = Hz::from(note.pitch());
    let (from, to) = span(mono, rate, note);
    let partials = proxy_partials(f0, others, rate);
    if partials.is_empty() {
        return Ok(None);
    }
    let (proxy, _) = tracks(
        mono,
        rate,
        (from, to),
        f0,
        &partials,
        !others.is_empty(),
        None,
    )?;
    Ok(Some(proxy))
}

/// The amp proxy and the attack's track of a note from `from` to `to`, with
/// the pump stripped when it locks to the beat and its beats carry the duck.
///
/// A note that overlaps no other reads its harmonic by a sliding DFT over
/// one period and its attack on the one-period RMS; an overlapping note
/// reads both on the band of its free harmonic.
fn tracks(
    mono: &[f64],
    rate: SampleRate,
    (from, to): (Seconds, Seconds),
    f0: Hz,
    partials: &[(Hz, Hz)],
    overlap: bool,
    pump: Option<&Pump>,
) -> Result<(LevelTrack, LevelTrack), FilterError> {
    // Every track reads the note's span and a margin, so a note costs its
    // own length, not the stem's. The band filters reflect three periods
    // of their cutoff at each end, and 0.3 s keeps the note clear of them;
    // it also covers the longest RMS window and DFT period.
    let fs = rate.hz().get();
    let lo = cast::floor_to_usize((from.get() - SPAN_MARGIN.get()).max(0.0) * fs).unwrap_or(0);
    let hi = cast::ceil_to_usize((to.get() + SPAN_MARGIN.get()) * fs)
        .unwrap_or(mono.len())
        .min(mono.len());
    let span = &mono[lo.min(hi)..hi];
    let mut proxy = if overlap {
        let offset = Seconds::clamped(cast::usize_to_f64(lo) / fs);
        LevelTrack::partials(span, offset, rate, (from, to), partials)?
    } else {
        LevelTrack::harmonic(span, lo, rate, (from, to), partials[0].0, f0)
    };
    let mut timing = if overlap {
        proxy.clone()
    } else {
        LevelTrack::rms(span, lo, rate, (from, to), f0, RMS_PERIODS)
    };
    if let Some(pump) = pump.filter(|p| p.strips() && p.carries(from.get(), to.get())) {
        pump.strip(&mut proxy);
        pump.strip(&mut timing);
    }
    Ok((proxy, timing))
}

/// The harmonic of the amp proxy with its low-pass: the lowest of
/// harmonics 1 to [`PROXY_HARMONICS`] below the Nyquist frequency that
/// lies at least [`FREE_GAP`] from every harmonic of `others` and whose
/// band lies above 0 Hz. The
/// low-pass is half the distance to the nearest foreign or own
/// neighbouring harmonic, between [`HARMONIC_CUTOFF_MIN`] and
/// [`HARMONIC_CUTOFF_MAX`]. Empty when no harmonic is free.
fn proxy_partials(f0: Hz, others: &[&Note], rate: SampleRate) -> Vec<(Hz, Hz)> {
    let nyquist = rate.nyquist().get();
    (1..=PROXY_HARMONICS)
        .filter_map(|k| {
            let f = f64::from(k) * f0.get();
            let foreign = others
                .iter()
                .map(|n| {
                    let g = Hz::from(n.pitch()).get();
                    let nearest = (f / g).round().max(1.0) * g;
                    (f - nearest).abs()
                })
                .fold(f64::INFINITY, f64::min);
            if foreign < FREE_GAP.get() {
                return None;
            }
            let cutoff = (0.5 * foreign.min(f0.get()))
                .clamp(HARMONIC_CUTOFF_MIN.get(), HARMONIC_CUTOFF_MAX.get());
            (f > cutoff && f + cutoff < nyquist).then(|| (Hz::clamped(f), Hz::clamped(cutoff)))
        })
        .take(1)
        .collect()
}

/// The 10-90 % rise of a track's linear amplitude from `onset` to the top
/// of the attack, in seconds: from the last frame under 10 % of the top
/// level before the first frame at 90 %, with both crossings linear
/// between frames. `None` when the level never rises from under 10 % of a
/// frame to a top.
///
/// The top is the first frame from `onset` whose level the track exceeds
/// by less than [`ATTACK_TOP`] over as long again as the level took to
/// rise to it from under 10 % of it, and at least over `resolution`, the
/// shortest rise the track can tell; the top level is the highest frame
/// over that span. A note that swells or ripples after a fast attack rises
/// to its top first, and a slow attack has no top before its end.
fn rise(track: &LevelTrack, onset: usize, resolution: f64) -> Option<f64> {
    let db = track.db();
    let peak = attack_top(db, onset, resolution)?;
    let amp: Vec<f64> = db.iter().map(|d| 10f64.powf((d - peak) / 20.0)).collect();
    let up = (onset.max(1)..amp.len()).find(|&i| amp[i] >= 0.9)?;
    let t90 = between(&amp, up, 0.9);
    let low = (0..up).rev().find(|&i| amp[i] < 0.1)?;
    let t10 = between(&amp, low + 1, 0.1);
    Some((t90 - t10) * HOP.get())
}

/// The top level of an attack from `onset`, in dB: see [`rise`].
fn attack_top(db: &[f64], onset: usize, resolution: f64) -> Option<f64> {
    // A tenth of the amplitude.
    let tenth = 20.0;
    let least = cast::ceil_to_usize(resolution / HOP.get()).unwrap_or(0);
    let first = onset.max(1);
    let mut lowest = db[..first.min(db.len())]
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    (first..db.len()).find_map(|i| {
        // The running minimum skips the search on a level that never rose,
        // which would otherwise read a long note's frames once per frame.
        let low = lowest;
        lowest = lowest.min(db[i]);
        if low >= db[i] - tenth {
            return None;
        }
        let start = (0..i).rev().find(|&j| db[j] < db[i] - tenth)?;
        let ahead = db[i..=(i + (i - start).max(least)).min(db.len() - 1)]
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        (ahead < db[i] + ATTACK_TOP.get()).then_some(ahead)
    })
}

/// The fractional index where `amp` rises through `level` between frames
/// `i - 1` and `i`.
fn between(amp: &[f64], i: usize, level: f64) -> f64 {
    let (a, b) = (amp[i - 1], amp[i]);
    let w = if b > a { (level - a) / (b - a) } else { 1.0 };
    cast::usize_to_f64(i - 1) + w.clamp(0.0, 1.0)
}

/// The decay from the track between `from` and `to`: the times at which
/// the dB level first falls through [`DECAY_SHARES`] of `sustain`, linear
/// between frames, the first share at least [`DECAY_FIRST_DB`] down. The
/// decay time is the time between them over the gap between the shares,
/// and the decay starts that share of it before the first. Returns the
/// start, from the track start, and the decay time; `None` when the level
/// does not reach the second share before `to`.
fn decay_fall(rel: &[f64], from: usize, to: usize, sustain: f64) -> Option<(f64, f64)> {
    let (lo, hi) = DECAY_SHARES;
    let lo = lo.max(DECAY_FIRST_DB.get() / -sustain);
    let first = falls_through(rel, from, to, lo * sustain)?;
    let second = falls_through(rel, cast::floor_to_usize(first)?, to, hi * sustain)?;
    let decay = (second - first) * HOP.get() / (hi - lo);
    Some((first * HOP.get() - lo * decay, decay))
}

/// The fractional index where `rel` first falls through `level` at or
/// after `from` and before `to`.
fn falls_through(rel: &[f64], from: usize, to: usize, level: f64) -> Option<f64> {
    let i = (from.max(1)..to.min(rel.len())).find(|&i| rel[i] <= level)?;
    let (a, b) = (rel[i - 1], rel[i]);
    let w = if a > b { (a - level) / (a - b) } else { 1.0 };
    Some(cast::usize_to_f64(i - 1) + w.clamp(0.0, 1.0))
}

/// The 90th minus the 10th percentile of values after their line; `None`
/// for fewer than ten values.
fn ripple(values: &[f64]) -> Option<f64> {
    if values.len() < 10 {
        return None;
    }
    let points: Vec<(f64, f64)> = values
        .iter()
        .enumerate()
        .map(|(i, &v)| (cast::usize_to_f64(i), v))
        .collect();
    let b = fit_line(&points).map_or(0.0, |(slope, _)| slope);
    let mut rest: Vec<f64> = points.iter().map(|&(x, y)| y - b * x).collect();
    rest.sort_by(f64::total_cmp);
    Some(quantile(&rest, 0.9)? - quantile(&rest, 0.1)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_residual_is_the_rms_against_hold_decay_and_sustain() {
        // 10 ms of hold, a 50 ms fall to -6 dB, then the sustain.
        let model: Vec<f64> = (0..100)
            .map(|i| {
                let t = cast::usize_to_f64(i) * HOP.get();
                if t < 0.01 {
                    0.0
                } else if t < 0.06 {
                    -6.0 * (t - 0.01) / 0.05
                } else {
                    -6.0
                }
            })
            .collect();
        assert!(residual(&model, 0.01, Some(0.05), -6.0).unwrap() < 1e-9);
        let off: Vec<f64> = model.iter().map(|v| v + 1.0).collect();
        assert!((residual(&off, 0.01, Some(0.05), -6.0).unwrap() - 1.0).abs() < 1e-9);
        // Without a decay the model steps to the sustain after the hold.
        let step: Vec<f64> = (0..20).map(|i| if i < 10 { 0.0 } else { -3.0 }).collect();
        assert!(residual(&step, 0.0095, None, -3.0).unwrap() < 1e-9);
        assert!(residual(&[], 0.0, None, 0.0).is_none());
    }

    #[test]
    fn a_fall_through_a_level_interpolates_between_frames() {
        let ramp = [0.0, -1.0, -2.0, -3.0, -4.0];
        assert!((falls_through(&ramp, 0, 5, -2.5).unwrap() - 2.5).abs() < 1e-12);
        // The search starts at `from` and stops before `to`.
        assert!((falls_through(&ramp, 4, 5, -0.5).unwrap() - 3.0).abs() < 1e-12);
        assert!(falls_through(&ramp, 0, 3, -2.5).is_none());
        // A level reached without a fall lands on the frame itself.
        assert!((falls_through(&[-3.0, -3.0], 0, 2, -3.0).unwrap() - 1.0).abs() < 1e-12);
    }

    #[test]
    fn the_decay_reads_a_fall_linear_in_db_exactly() {
        // A fall from 0 dB over 100 frames to the sustain, then the sustain:
        // a decay of 0.1 s that starts at frame 0, whichever share leads.
        for sustain in [-10.0, -4.0] {
            let rel: Vec<f64> = (0..200)
                .map(|i| sustain * (cast::usize_to_f64(i) / 100.0).min(1.0))
                .collect();
            let (start, decay) = decay_fall(&rel, 0, 200, sustain).unwrap();
            assert!(
                start.abs() < 1e-9 && (decay - 0.1).abs() < 1e-9,
                "{sustain}: {start} {decay}"
            );
        }
        // A fall of 2 dB over 20 frames, then 2 dB more over 80, to -4 dB:
        // the first share is 0.5 dB down, a share of 0.125, at frame 5,
        // and the 90 % share at frame 84.
        let rel: Vec<f64> = (0..200)
            .map(|i| {
                let i = cast::usize_to_f64(i);
                if i < 20.0 {
                    -0.1 * i
                } else {
                    (-2.0 - 2.0 * (i - 20.0) / 80.0).max(-4.0)
                }
            })
            .collect();
        let (start, decay) = decay_fall(&rel, 0, 200, -4.0).unwrap();
        let want = 0.079 / 0.775;
        assert!((decay - want).abs() < 1e-9, "{decay}");
        assert!((start - (0.005 - 0.125 * want)).abs() < 1e-9, "{start}");
        // The second share must come before `to`.
        let rel: Vec<f64> = (0..200)
            .map(|i| -10.0 * (cast::usize_to_f64(i) / 100.0).min(1.0))
            .collect();
        assert!(decay_fall(&rel, 0, 80, -10.0).is_none());
    }

    #[test]
    fn the_attack_top_looks_ahead_at_least_over_the_resolution() {
        // A rise from -60 dB that stalls at -20 dB for 3 frames before it
        // goes on to 0 dB: over 3 frames of look-ahead the stall is a top.
        let mut db: Vec<f64> = vec![-60.0; 5];
        db.extend([-40.0, -20.0, -20.0, -20.0, -20.0, -10.0, -5.0, 0.0]);
        db.extend([0.0; 20]);
        assert_eq!(attack_top(&db, 0, 0.0), Some(-20.0));
        assert_eq!(attack_top(&db, 0, 0.01), Some(0.0));
    }

    #[test]
    fn the_ripple_is_the_spread_after_the_line() {
        // A slope of 0.5 dB per frame with a swing of 1 dB either way.
        let values: Vec<f64> = (0..200)
            .map(|i| 0.5 * cast::usize_to_f64(i) + if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        assert!((ripple(&values).unwrap() - 2.0).abs() < 0.05);
        let flat: Vec<f64> = (0..20)
            .map(|i| -3.0 - 0.2 * cast::usize_to_f64(i))
            .collect();
        assert!(ripple(&flat).unwrap().abs() < 1e-9);
        assert!(ripple(&[0.0; 9]).is_none());
        assert_eq!(ripple(&[0.0; 10]), Some(0.0));
    }

    #[test]
    fn the_constants_pass_the_unit_check() {
        assert_eq!(ATTACK_TOP, Decibels::new(1.0).unwrap());
        assert_eq!(HOLD_MIN, Seconds::new(0.01).unwrap());
        assert_eq!(FIT_FLOOR, Decibels::new(60.0).unwrap());
        assert_eq!(DECAY_FIRST_DB, Decibels::new(0.5).unwrap());
        assert_eq!(TRACK_LEAD, Seconds::new(0.03).unwrap());
        assert_eq!(KNEE_END_GAP, Seconds::new(0.025).unwrap());
        assert_eq!(FREE_GAP, Hz::new(30.0).unwrap());
    }

    #[test]
    fn a_linear_ramp_rises_from_10_to_90_percent_in_80_percent_of_its_time() {
        // 1 ms frames of a 100 ms linear ramp from silence.
        let db: Vec<f64> = (0..300)
            .map(|i| {
                let a = (cast::usize_to_f64(i) - 50.0).clamp(0.0, 100.0) / 100.0;
                20.0 * a.max(1e-9).log10()
            })
            .collect();
        let track = LevelTrack::new(Seconds::clamped(0.0), db);
        let rise = rise(&track, 0, 0.0).unwrap();
        assert!((rise - 0.08).abs() < 1e-3, "{rise}");
    }

    #[test]
    fn a_harmonic_near_another_notes_harmonic_is_not_free() {
        let rate = SampleRate::new(44_100).unwrap();
        let fifth = Note::new(
            Seconds::clamped(0.0),
            Seconds::clamped(1.0),
            crate::units::MidiPitch::new(52.0).unwrap(),
        )
        .unwrap();
        // A2 at 110 Hz under E3 at 164.8 Hz keeps its fundamental, 54.8 Hz
        // from E3.
        let kept: Vec<f64> = proxy_partials(Hz::new(110.0).unwrap(), &[&fifth], rate)
            .iter()
            .map(|p| p.0.get())
            .collect();
        assert_eq!(kept, vec![110.0]);
        // Against 120 Hz, 110 and 220 Hz lie 10 and 20 Hz from 120 and 240
        // Hz; 330 Hz lies 30 Hz from 360 Hz, free, with a 15 Hz low-pass.
        let near = Note::new(
            Seconds::clamped(0.0),
            Seconds::clamped(1.0),
            crate::units::MidiPitch::try_from(Hz::new(120.0).unwrap()).unwrap(),
        )
        .unwrap();
        let kept = proxy_partials(Hz::new(110.0).unwrap(), &[&near], rate);
        assert_eq!(kept.len(), 1);
        assert!((kept[0].0.get() - 330.0).abs() < 1e-9);
        assert!((kept[0].1.get() - 15.0).abs() < 1e-6, "{:?}", kept[0].1);
        let octave = Note::new(
            Seconds::clamped(0.0),
            Seconds::clamped(1.0),
            crate::units::MidiPitch::new(57.0).unwrap(),
        )
        .unwrap();
        assert!(
            proxy_partials(Hz::new(440.0).unwrap(), &[&octave], rate).is_empty(),
            "an octave above has no free harmonic"
        );
    }
}
