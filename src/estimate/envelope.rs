//! The envelope layer: the amplitude envelope of the notes and the
//! sidechain pump.
//!
//! The pump comes first: a dip that locks to the beat is stripped from the
//! level tracks before the notes are read. Each note gets an attack, a
//! hold, a decay, a sustain level, a note-off knee and a peak level. The
//! notes of similar length and register form a pool, and each value of
//! the layer is the median over the pool with its interquartile range.

mod adsr;
mod knee;
mod pump;
mod track;

pub use adsr::{
    ATTACK_PERIODS_MIN, ATTACK_TOP, BAND_STEP_RISE, BAND_STEPS_MIN, DECAY_FIRST_DB, DECAY_SHARES,
    FIT_FLOOR, FREE_GAP, HOLD_MIN, KNEE_END_GAP, NoteEnvelope, NoteSkip, OBSERVABLE_DECAYS,
    PROXY_HARMONICS, RAMP_FACTOR, SILENCE_DB, SUSTAIN_SHARE, TRACK_LEAD, amp_proxy, read_note,
};
pub use knee::{
    KNEE_FALL, KNEE_FLOOR, KNEE_GAIN, KNEE_LEFT, KNEE_RIGHT, KNEE_SIDE_MIN, KNEE_STEEPEN, find_knee,
};
pub use pump::{
    DUCK_TAIL_BINS, DUCK_TAIL_WINDOWS, FALL_MAX, FIT_GAIN, FREE_RATE_MIN, FREE_REACH,
    FREE_RESIDUAL, FREE_SWING, PERIOD_RANGE, PERIOD_RUN_BEATS, PHASE_DEPTH, PHASE_SHARE,
    PHASE_SLACK, PUMP_BACK, PUMP_BEATS_MIN, PUMP_BINS, PUMP_MIN, PUMP_REFERENCE, PUMP_STRIP_BEATS,
    Pump, PumpLock, SUSTAIN_LEAD,
};
pub use track::{
    HARMONIC_CUTOFF_MAX, HARMONIC_CUTOFF_MIN, HOP, LevelTrack, PUMP_RMS_PERIODS, RMS_PERIODS,
    RMS_WINDOW_MIN, rms_window,
};

use crate::audio::{BeatGrid, SampleRate};
use crate::cast;
use crate::confidence::{
    Confidence, ConfidenceError, Estimate, Level, NoSpread, Reason, ReasonCode, Signal,
};
use crate::dsp::{FilterError, Stats, Summary, quantile};
use crate::notes::{Note, NoteList};
use crate::units::{Decibels, Hz, Seconds};

/// A value over the pool needs this many notes; with fewer it reads low.
pub const POOL_NOTES_MIN: usize = 3;
/// Above this interquartile range over the median, a value reads low.
pub const RELATIVE_SPREAD_MAX: f64 = 0.5;
/// The pool holds notes whose length lies within this factor of the
/// median length.
pub const POOL_LENGTH_FACTOR: f64 = 2.0;
/// The pool holds notes within this many semitones of the median pitch.
pub const POOL_PITCH_RANGE: f64 = 12.0;
/// The pool leaves out a note whose peak lies this far or more below the
/// peak of a note it overlaps: a quiet second voice, such as another
/// part's bleed or a ghost note, with an envelope of its own.
pub const POOL_LEVEL_GATE: Decibels = Decibels::from_const(12.0);
/// The level spread is the gap between these quantiles of the pooled
/// peak levels.
pub const SPREAD_QUANTILES: (f64, f64) = (0.1, 0.9);

/// The convention of the envelope stages in `layers.json`.
pub const ENVELOPE_CONVENTION: &str = "each note is read on a 1 ms hop from 30 ms before its onset \
in the note list to its end; the amp proxy is the note's lowest harmonic among the first three \
that lies 30 Hz or more from every harmonic of the notes sounding with it: for a note that \
overlaps no other, a sliding DFT over one period of the fundamental, else the zero-phase band \
magnitude, low-passed at half the distance to the nearest foreign or own neighbouring harmonic, 12 \
to 40 Hz; levels are dB from the note's peak of the amp proxy, read no lower than -60 dB; attack_s \
is 1.25 times the 10 to 90 % rise of the linear amplitude to the attack's top, the full 0 to 100 % \
time of a linear ramp, read on the centred RMS over one period of the fundamental, or the fewest \
whole periods that span 2 ms, for a note that overlaps no other, else on the amp proxy; the top is \
the first frame from the onset that the level exceeds by less than 1 dB over as long again as it \
took to rise to it from under 10 % of it, and at least over the shortest rise the track can tell, \
the upper bound's figure below, and the shares are of the highest frame over that span; \
the attack is an upper bound \
when the rise is shorter than 1.5 RMS windows, or on the amp proxy shorter than 0.98 over its \
low-pass cutoff, twice its rise for an instant step; hold_s is the time from the first frame \
within 1 dB of the peak to the start of the decay, 0 when under 10 ms; decay_s is the time between \
the first frames after the peak, linear between frames, at which the level falls through 10 % and \
90 % of the sustain level in dB, the first at least 0.5 dB down, divided by the gap between the \
two shares, and the decay starts the first share of it before the first; sustain_db is the median \
of the last 30 % of the held part, from the first frame within 1 dB of the peak to the note-off \
knee, or to 25 ms before the note's end without one; decay and sustain are null when a note with a \
knee holds for less than the attack, the hold and three decay times, and a note without one \
sustains at its end level; fit_residual_db is the median over the pool of the RMS in dB of each \
held part against 0 dB over the hold, a fall linear in dB to the sustain level over the decay, \
then the sustain level; ripple_db is the median, over the pooled notes whose sustain is \
observable, of the 90th minus the 10th percentile of the sustain window after its line; each \
statistic is the median and interquartile range over the pooled notes: of the notes whose peak lies \
less than 12 dB below the peak of every note they overlap, those whose length to the knee, or to \
the end without one, lies within a factor of 2 of their median length and whose pitch lies within \
12 semitones of their median pitch; a value over fewer than 3 notes, with an interquartile range \
above half its median, or with a median of 0 under an interquartile range above 0, has low \
confidence; attack_is_upper_bound is set when more than half the pooled attacks are upper bounds; \
a note whose amp proxy peaks at -120 dB or lower is skipped as silent";
/// The convention of the note-off knee and its offset.
pub const KNEE_CONVENTION: &str = "the note-off knee is read on the linear amplitude of the amp \
proxy, from the first frame within 1 dB of the peak to 25 ms before the note's end; at each split \
a line over up to 100 ms before it and one over up to 200 ms after it, stopping where the level \
falls under 2 % of the mean before it, fit against one line over both; a split counts when the \
right line is steeper by half the mean before it per second, falls by half that mean over its \
side, and the two lines leave at most half the squared error of one; splits that count in a row \
form a corner, and the split with the largest gain in the last corner wins; the knee is where the \
left line crosses a line over the first 10 ms after the split, or the split itself when that \
crossing lies outside the two lines; null when no split counts, as for a pluck that decays at one \
rate; note_off_offset_s is the note's end in the note list minus its \
knee, over the pooled notes with a knee";
/// The convention of the level spread.
pub const LEVEL_SPREAD_CONVENTION: &str = "the 90th minus the 10th percentile of the peak dB of \
the amp proxy over the pooled notes, quantiles by linear interpolation; always low confidence, \
a rule no stem has tested; null under 3 pooled notes";
/// The convention of the pump.
pub const PUMP_CONVENTION: &str = "the full-band centred RMS in dB over two periods of the lowest \
note on a 1 ms hop, cut into beats of the used tempo from 0 s; a beat counts when the held part of \
one note covers it, from 50 ms or more before it, the held part running from the note's onset to \
its knee, or to 25 ms before its end without one; each counted beat is binned into 64 phase bins \
and taken relative to the 90th percentile of its bins, and the curve is the median over the beats; \
pump_depth_db is minus the curve's minimum; pump_recovery_s is the time from the minimum until the \
curve comes back within 1.5 dB, going on over the beat's wrap, null when it does not within one \
beat; pump_tau_ms is pump_recovery_s over ln(pump_depth_db / 1.5), in milliseconds, null unless \
the pump locks to the beat; pump_locked_to is note when the beats with a note onset inside, from \
50 ms before the beat, dip 3 dB or more and the others under 1.5 dB, each group of 2 beats or more; \
else free when the level swings 1 dB or more at a rate of 0.5 to 20 Hz that lies 50 cents or more \
from every note division; else beat when the curve dips 3 dB or more in one run of bins half its \
depth down, falls into its dip within 8 bins, reading on into the beat before, the odd and the even \
beats' own median curves do the same to half its depth at its dip, and half the beats fall by half \
its depth from their highest bin over the 8 bins before the 3 bins either side of the dip to their \
lowest within those; else none; the read at the period up to about 2.7 % from the tempo's whose counted \
beats fold onto the deepest mean curve replaces this read if that reads beat and this one reads \
note or none, reads beat 1.5 dB or more shallower without giving up the strip, or reads free on a \
line within 60 cents of a division under a curve that falls faster than it recovers and takes 3 \
bins and half the level window from half its depth back to a quarter; \
the curve is stripped from the tracks of a note only when it locks to the beat over 8 counted beats \
or more and the median fall of the beats over the note reaches half the curve's depth, the beats \
counted again over each note to 25 ms before its end when the held parts give fewer \
or no pump; null with fewer than 4 counted beats";

/// What the envelope layer reads.
#[derive(Clone, Copy, Debug)]
pub struct EnvelopeInput<'a> {
    /// The mono mix at the native rate.
    pub mono: &'a [f64],
    /// The native rate.
    pub rate: SampleRate,
    /// The note list, transcribed or given.
    pub notes: &'a NoteList,
    /// The beat grid of the used tempo, for the pump.
    pub grid: BeatGrid,
}

/// Flags of the envelope layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct EnvelopeFlags {
    /// The stem gave no note list, so no note was read.
    pub no_notes: bool,
    /// Most pooled attacks are upper bounds, see
    /// [`NoteEnvelope::attack_upper_bound`], so a preset should not take
    /// the attack.
    pub attack_is_upper_bound: bool,
    /// The pump locks to the beat over [`PUMP_STRIP_BEATS`] counted beats
    /// or more, so its curve was stripped from the level tracks before the
    /// notes were read.
    pub pump_stripped: bool,
}

/// How many notes each step read or skipped.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EnvelopeCounts {
    /// Notes with an envelope.
    pub read: usize,
    /// Notes that overlap another with no free harmonic.
    pub no_free_harmonic: usize,
    /// Notes too short for a track.
    pub too_short: usize,
    /// Notes whose amp proxy peaks at the silence floor.
    pub silent: usize,
    /// Read notes with a note-off knee.
    pub knees: usize,
}

/// The envelope layer of a stem.
#[derive(Clone, Debug, PartialEq)]
pub struct EnvelopeLayer {
    /// The envelope of each read note, in note order.
    pub notes: Vec<NoteEnvelope>,
    /// Which of `notes` form the pool.
    pub pooled: Vec<bool>,
    /// The attack over the pool: 0 to 100 % for a linear ramp.
    pub attack: Estimate<Stats<Seconds>, Seconds>,
    /// The hold over the pool.
    pub hold: Estimate<Stats<Seconds>, Seconds>,
    /// The decay over the pool.
    pub decay: Estimate<Stats<Seconds>, Seconds>,
    /// The sustain level from the peak over the pool.
    pub sustain: Estimate<Stats<Decibels>, Decibels>,
    /// The note's end in the list minus its knee, over the pooled notes
    /// with a knee.
    pub note_off_offset: Estimate<Stats<Seconds>, Seconds>,
    /// The 90th minus the 10th percentile of the pooled peak levels.
    pub level_spread: Estimate<Decibels, NoSpread>,
    /// The median ripple over the pool.
    pub ripple: Option<Decibels>,
    /// The median fit residual over the pool.
    pub residual: Option<Decibels>,
    /// The pump read; `None` with fewer than [`PUMP_BEATS_MIN`] sustained
    /// beats.
    pub pump: Option<Pump>,
    /// The share of the notes in the list that overlap another.
    pub overlap_fraction: f64,
    /// The flags.
    pub flags: EnvelopeFlags,
    /// The confidence of the layer as a whole; no estimate without notes.
    pub confidence: Confidence<NoSpread>,
    /// The counts behind the summary.
    pub counts: EnvelopeCounts,
}

impl EnvelopeLayer {
    /// Reads the envelope layer of a stem.
    ///
    /// A 220 Hz sine held from 0.1 s, released linearly from 0.8 to 0.9 s,
    /// reads one note at a sustain near its peak, with its note-off knee
    /// where the release starts:
    ///
    /// ```
    /// use sound_to_patch::audio::{BeatGrid, SampleRate};
    /// use sound_to_patch::estimate::envelope::{EnvelopeInput, EnvelopeLayer};
    /// use sound_to_patch::notes::{Note, NoteList};
    /// use sound_to_patch::units::{Bpm, MidiPitch, Seconds};
    ///
    /// let rate = SampleRate::new(22_050)?;
    /// let mono: Vec<f64> = (0..22_050)
    ///     .map(|n| {
    ///         let t = f64::from(n) / 22_050.0;
    ///         let gain = if t < 0.1 { 0.0 } else { ((0.9 - t) / 0.1).clamp(0.0, 1.0) };
    ///         0.5 * gain * (std::f64::consts::TAU * 220.0 * t).sin()
    ///     })
    ///     .collect();
    /// let note = Note::new(Seconds::new(0.1)?, Seconds::new(0.9)?, MidiPitch::new(57.0)?)?;
    /// let notes = NoteList::new(vec![note]);
    /// let grid = BeatGrid::new(Bpm::new(120.0)?, Seconds::new(0.0)?);
    /// let layer = EnvelopeLayer::of(&EnvelopeInput { mono: &mono, rate, notes: &notes, grid })?;
    /// assert_eq!(layer.counts.read, 1);
    /// let sustain = layer.sustain.value().map(|s| s.median().get());
    /// assert!(sustain.is_some_and(|s| s > -1.0));
    /// let knee = layer.notes[0].knee.map(Seconds::get);
    /// assert!(knee.is_some_and(|k| (k - 0.8).abs() < 0.01));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an [`EnvelopeError`] when a harmonic band reaches the
    /// Nyquist frequency, or a confidence cannot be built.
    pub fn of(input: &EnvelopeInput<'_>) -> Result<Self, EnvelopeError> {
        let list = input.notes.clone().with_overlaps();
        let notes = list.notes();
        let mut counts = EnvelopeCounts::default();
        let mut flags = EnvelopeFlags {
            no_notes: notes.is_empty(),
            ..EnvelopeFlags::default()
        };
        let (read, pump) = read_with_pump(input, notes, &mut counts)?;
        flags.pump_stripped = pump.as_ref().is_some_and(Pump::strips);
        counts.read = read.len();
        counts.knees = read.iter().filter(|n| n.knee.is_some()).count();
        let pooled = pool(&read);
        let pool: Vec<&NoteEnvelope> = read
            .iter()
            .zip(&pooled)
            .filter(|&(_, &p)| p)
            .map(|(n, _)| n)
            .collect();
        let bounds = pool.iter().filter(|n| n.attack_upper_bound).count();
        let bound_share = share(bounds, pool.iter().filter(|n| n.attack.is_some()).count());
        flags.attack_is_upper_bound = bound_share > 0.5;
        let overlap_fraction = share(
            list.notes()
                .iter()
                .filter(|n| n.overlap() == Some(true))
                .count(),
            notes.len(),
        );
        let confidence = if read.is_empty() {
            Confidence::no_estimate(vec![if notes.is_empty() {
                Reason::new(ReasonCode::NoNoteList, Signal::Flag(true))?
            } else {
                Reason::new(ReasonCode::TooFewNotes, Signal::Count(0))?
            }])?
        } else {
            Confidence::estimate(
                if pool.len() < POOL_NOTES_MIN {
                    Level::Low
                } else {
                    Level::Medium
                },
                None,
                vec![Reason::new(
                    ReasonCode::TooFewNotes,
                    Signal::Count(count(pool.len())),
                )?],
            )?
        };
        Ok(Self {
            attack: statistic(
                pool.iter().filter_map(|n| n.attack).map(Seconds::get),
                Seconds::clamped,
                Some(Reason::new(
                    ReasonCode::UpperBoundShare,
                    Signal::Number(bound_share),
                )?),
            )?,
            hold: statistic(pool.iter().map(|n| n.hold.get()), Seconds::clamped, None)?,
            decay: statistic(
                pool.iter().filter_map(|n| n.decay).map(Seconds::get),
                Seconds::clamped,
                None,
            )?,
            sustain: statistic(
                pool.iter().filter_map(|n| n.sustain).map(Decibels::get),
                Decibels::clamped,
                None,
            )?,
            note_off_offset: statistic(
                pool.iter()
                    .filter_map(|n| n.knee.map(|k| n.end.get() - k.get())),
                Seconds::clamped,
                None,
            )?,
            level_spread: level_spread(&pool)?,
            ripple: pool_median(&pool, |n| n.ripple),
            residual: pool_median(&pool, |n| n.residual),
            pump,
            overlap_fraction,
            flags,
            confidence,
            counts,
            pooled,
            notes: read,
        })
    }

    /// Returns the lines that tell what the layer found.
    #[must_use]
    pub fn summary(&self) -> Vec<String> {
        if self.flags.no_notes {
            return vec!["envelope: no notes, so no envelope".to_owned()];
        }
        let stat = |name: &str, e: &Estimate<Stats<Seconds>, Seconds>| {
            e.value().map_or_else(
                || format!("{name} none"),
                |s| format!("{name} {:.0} ms", 1000.0 * s.median().get()),
            )
        };
        let total = self.counts.read
            + self.counts.no_free_harmonic
            + self.counts.too_short
            + self.counts.silent;
        let mut lines = vec![format!(
            "envelope: {} of {total} notes read, {} pooled, {} with a note-off knee{}",
            self.counts.read,
            self.pooled.iter().filter(|&&p| p).count(),
            self.counts.knees,
            skipped(self.counts),
        )];
        lines.push(format!(
            "envelope: {}{}, {}, {}, sustain {}",
            stat("attack", &self.attack),
            if self.flags.attack_is_upper_bound {
                " (upper bound)"
            } else {
                ""
            },
            stat("hold", &self.hold),
            stat("decay", &self.decay),
            self.sustain.value().map_or_else(
                || "none".to_owned(),
                // Adding 0 turns a level that rounds to -0.0 into 0.0.
                |s| format!("{:.1} dB", (10.0 * s.median().get()).round() / 10.0 + 0.0)
            ),
        ));
        lines.push(match &self.pump {
            Some(p) => format!(
                "pump: {:.1} dB over {} beats, {}{}",
                p.depth.get(),
                p.beats,
                lock_name(p.lock),
                if p.strips() { ", stripped" } else { "" }
            ),
            None => "pump: too few sustained beats".to_owned(),
        });
        lines
    }
}

/// Reads the notes and the pump. The pump counts beats inside the held
/// parts, which the knees of a first read bound. When those give no pump,
/// or a beat lock over fewer than [`PUMP_STRIP_BEATS`] beats, the beats
/// are counted again over the notes to 25 ms before their ends, and that
/// count is kept when it strips. When the pump strips, the notes are read
/// again with it stripped.
fn read_with_pump(
    input: &EnvelopeInput<'_>,
    notes: &[Note],
    counts: &mut EnvelopeCounts,
) -> Result<(Vec<NoteEnvelope>, Option<Pump>), FilterError> {
    let first = read_notes(input, notes, None, counts)?;
    let mut knees = vec![None; notes.len()];
    for (i, e) in &first {
        knees[*i] = e.knee;
    }
    let spans: Vec<(f64, f64)> = notes
        .iter()
        .map(|n| (n.onset().get(), n.offset().get() - KNEE_END_GAP.get()))
        .collect();
    let held: Vec<(f64, f64)> = spans
        .iter()
        .zip(&knees)
        .map(|(&(on, end), knee)| (on, knee.map_or(end, Seconds::get)))
        .collect();
    let track = pump_track(input);
    let find = |parts: &[(f64, f64)]| {
        track
            .as_ref()
            .and_then(|t| Pump::find(t, input.grid, input.notes, parts))
    };
    // A duck's dip can read as a note-off knee in the first read and cut
    // the held parts short of the beats that strip.
    let pump = match find(&held) {
        Some(p) if p.lock != PumpLock::Beat || p.strips() => Some(p),
        short => find(&spans).filter(Pump::strips).or(short),
    };
    let read = if pump.as_ref().is_some_and(Pump::strips) {
        *counts = EnvelopeCounts::default();
        read_notes(input, notes, pump.as_ref(), counts)?
    } else {
        first
    };
    Ok((read.into_iter().map(|(_, e)| e).collect(), pump))
}

/// Reads the envelope of every note against the notes that sound with it,
/// with the index of its note, and counts the skipped ones.
fn read_notes(
    input: &EnvelopeInput<'_>,
    notes: &[Note],
    pump: Option<&Pump>,
    counts: &mut EnvelopeCounts,
) -> Result<Vec<(usize, NoteEnvelope)>, FilterError> {
    let mut read = Vec::new();
    for (i, note) in notes.iter().enumerate() {
        let others: Vec<&Note> = notes
            .iter()
            .enumerate()
            .filter(|&(j, other)| {
                j != i && other.onset() < note.offset() && note.onset() < other.offset()
            })
            .map(|(_, other)| other)
            .collect();
        match read_note(input.mono, input.rate, note, &others, pump)? {
            Ok(envelope) => read.push((i, envelope)),
            Err(NoteSkip::NoFreeHarmonic) => counts.no_free_harmonic += 1,
            Err(NoteSkip::TooShort) => counts.too_short += 1,
            Err(NoteSkip::Silent) => counts.silent += 1,
        }
    }
    Ok(read)
}

/// The median of a dB field over the pooled notes that have it.
fn pool_median(
    pool: &[&NoteEnvelope],
    field: impl Fn(&NoteEnvelope) -> Option<Decibels>,
) -> Option<Decibels> {
    let values: Vec<f64> = pool
        .iter()
        .filter_map(|n| field(n))
        .map(Decibels::get)
        .collect();
    median(&values).map(Decibels::clamped)
}

/// The full-band RMS of the stem over two periods of its lowest note,
/// which the pump reads; `None` without notes.
fn pump_track(input: &EnvelopeInput<'_>) -> Option<LevelTrack> {
    let lowest = input
        .notes
        .notes()
        .iter()
        .map(Note::pitch)
        .min_by(|a, b| a.get().total_cmp(&b.get()))?;
    let end = cast::usize_to_f64(input.mono.len()) / input.rate.hz().get();
    Some(LevelTrack::rms(
        input.mono,
        0,
        input.rate,
        (Seconds::clamped(0.0), Seconds::clamped(end)),
        Hz::from(lowest),
        PUMP_RMS_PERIODS,
    ))
}

/// Which notes form the pool: of the notes that lie less than
/// [`POOL_LEVEL_GATE`] below every note they overlap, those whose length
/// lies within [`POOL_LENGTH_FACTOR`] of their median length and whose
/// pitch lies within [`POOL_PITCH_RANGE`] of their median pitch. The
/// length runs to the knee, or to the note's end without one.
fn pool(notes: &[NoteEnvelope]) -> Vec<bool> {
    let length = |n: &NoteEnvelope| n.knee.unwrap_or(n.end).get() - n.onset.get();
    let pitch = |n: &NoteEnvelope| n.pitch.get();
    let loud: Vec<bool> = notes
        .iter()
        .map(|n| {
            !notes.iter().any(|o| {
                o.onset < n.end
                    && n.onset < o.end
                    && o.peak_db.get() - n.peak_db.get() >= POOL_LEVEL_GATE.get()
            })
        })
        .collect();
    let kept = || notes.iter().zip(&loud).filter(|&(_, &l)| l).map(|(n, _)| n);
    let lengths: Vec<f64> = kept().map(length).collect();
    let pitches: Vec<f64> = kept().map(pitch).collect();
    let (Some(l), Some(p)) = (median(&lengths), median(&pitches)) else {
        return vec![false; notes.len()];
    };
    notes
        .iter()
        .zip(loud)
        .map(|(n, loud)| {
            let ratio = length(n) / l;
            loud && (1.0 / POOL_LENGTH_FACTOR..=POOL_LENGTH_FACTOR).contains(&ratio)
                && (pitch(n) - p).abs() <= POOL_PITCH_RANGE
        })
        .collect()
}

/// The median and interquartile range of values in a unit, at low
/// confidence with fewer than [`POOL_NOTES_MIN`] values, a relative IQR
/// above [`RELATIVE_SPREAD_MAX`] or a median of 0 under an IQR above 0;
/// no estimate without values.
fn statistic<T: crate::confidence::Spread>(
    values: impl Iterator<Item = f64>,
    unit: impl Fn(f64) -> T + Copy,
    extra: Option<Reason>,
) -> Result<Estimate<Stats<T>, T>, ConfidenceError> {
    let values: Vec<f64> = values.collect();
    let n = Reason::new(ReasonCode::TooFewNotes, Signal::Count(count(values.len())))?;
    let Some(summary) = Summary::of(&values) else {
        return Estimate::none(vec![n]);
    };
    let (spread, wide) = if summary.median().abs() > 0.0 {
        let relative = summary.iqr() / summary.median().abs();
        (
            Reason::new(ReasonCode::RelativeSpread, Signal::Number(relative))?,
            relative > RELATIVE_SPREAD_MAX,
        )
    } else if summary.iqr() > 0.0 {
        (
            Reason::new(ReasonCode::ZeroMedian, Signal::Number(summary.iqr()))?,
            true,
        )
    } else {
        (
            Reason::new(ReasonCode::RelativeSpread, Signal::Number(0.0))?,
            false,
        )
    };
    let level = if values.len() < POOL_NOTES_MIN || wide {
        Level::Low
    } else {
        Level::Medium
    };
    let mut reasons = vec![n, spread];
    reasons.extend(extra);
    Estimate::new(
        summary.to_stats(unit),
        level,
        Some(unit(summary.iqr())),
        reasons,
    )
}

/// The level spread over the pool, always at low confidence: the rule is a
/// proposal no stem has tested.
fn level_spread(pool: &[&NoteEnvelope]) -> Result<Estimate<Decibels, NoSpread>, ConfidenceError> {
    let n = Reason::new(ReasonCode::TooFewNotes, Signal::Count(count(pool.len())))?;
    if pool.len() < POOL_NOTES_MIN {
        return Estimate::none(vec![n]);
    }
    let mut peaks: Vec<f64> = pool.iter().map(|p| p.peak_db.get()).collect();
    peaks.sort_by(f64::total_cmp);
    let (lo, hi) = SPREAD_QUANTILES;
    let spread = quantile(&peaks, hi).unwrap_or(0.0) - quantile(&peaks, lo).unwrap_or(0.0);
    Estimate::new(Decibels::clamped(spread), Level::Low, None, vec![n])
}

fn median(values: &[f64]) -> Option<f64> {
    Summary::of(values).map(Summary::median)
}

fn share(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        cast::usize_to_f64(part) / cast::usize_to_f64(whole)
    }
}

fn count(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

fn skipped(counts: EnvelopeCounts) -> String {
    let items: Vec<String> = [
        (counts.no_free_harmonic, "no free harmonic"),
        (counts.too_short, "too short"),
        (counts.silent, "silent"),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, why)| format!("{n} {why}"))
    .collect();
    if items.is_empty() {
        String::new()
    } else {
        format!("; skipped {}", items.join(", "))
    }
}

/// The name of a lock in the report.
#[must_use]
pub fn lock_name(lock: PumpLock) -> &'static str {
    match lock {
        PumpLock::Beat => "beat",
        PumpLock::Note => "note",
        PumpLock::Free => "free",
        PumpLock::None => "none",
    }
}

/// The envelope layer failed.
#[derive(Debug)]
pub enum EnvelopeError {
    /// A harmonic band reaches 0 Hz or the Nyquist frequency.
    Filter(FilterError),
    /// A confidence could not be built.
    Confidence(ConfidenceError),
}

impl std::fmt::Display for EnvelopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Filter(e) => write!(f, "cannot read the envelope layer: {e}"),
            Self::Confidence(e) => write!(f, "cannot rate the envelope layer: {e}"),
        }
    }
}

impl std::error::Error for EnvelopeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Filter(e) => Some(e),
            Self::Confidence(e) => Some(e),
        }
    }
}

impl From<FilterError> for EnvelopeError {
    fn from(e: FilterError) -> Self {
        Self::Filter(e)
    }
}

impl From<ConfidenceError> for EnvelopeError {
    fn from(e: ConfidenceError) -> Self {
        Self::Confidence(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::MidiPitch;
    use proptest::prelude::*;

    fn note(onset: f64, length: f64, f0: f64, knee: Option<f64>) -> NoteEnvelope {
        NoteEnvelope {
            onset: Seconds::clamped(onset),
            end: Seconds::clamped(onset + length),
            pitch: MidiPitch::try_from(Hz::clamped(f0)).unwrap(),
            peak_db: Decibels::clamped(-6.0),
            attack: None,
            attack_upper_bound: false,
            hold: Seconds::clamped(0.0),
            decay: None,
            sustain: None,
            knee: knee.map(|k| Seconds::clamped(onset + k * length)),
            ripple: None,
            residual: None,
            overlap: false,
        }
    }

    #[test]
    fn the_pool_keeps_notes_near_the_median_length_and_pitch() {
        let mut notes: Vec<NoteEnvelope> = (0..5)
            .map(|i| note(f64::from(i), 1.0, 110.0, None))
            .collect();
        // 11 semitones up and 0.6 times as long: pooled.
        notes.push(note(6.0, 1.0, 110.0 * 2f64.powf(11.0 / 12.0), None));
        notes.push(note(7.0, 0.6, 110.0, None));
        // Two octaves up, or three times as long: not pooled.
        notes.push(note(8.0, 1.0, 440.0, None));
        notes.push(note(9.0, 3.0, 110.0, None));
        assert_eq!(
            pool(&notes),
            [true, true, true, true, true, true, true, false, false]
        );
        // The length runs to the knee.
        let mut late = notes[..5].to_vec();
        late.push(note(10.0, 3.0, 110.0, Some(0.33)));
        assert!(pool(&late)[5]);
    }

    fn level_of(values: &[f64]) -> Option<Level> {
        let stat = statistic(values.iter().copied(), Seconds::clamped, None).unwrap();
        assert_eq!(
            stat.confidence().reasons()[0].value(),
            Signal::Count(u64::try_from(values.len()).unwrap())
        );
        stat.confidence().level()
    }

    #[test]
    fn a_statistic_reads_low_on_few_notes_or_a_wide_spread() {
        assert_eq!(level_of(&[1.0, 1.0]), Some(Level::Low));
        assert_eq!(level_of(&[1.0, 1.0, 1.0]), Some(Level::Medium));
        // A median of 0 with no spread is no spread; with one, it is wide.
        assert_eq!(level_of(&[0.0; 8]), Some(Level::Medium));
        assert_eq!(
            level_of(&[0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0]),
            Some(Level::Low)
        );
        // An IQR of exactly half the median still reads medium.
        assert_eq!(level_of(&[3.0, 3.0, 4.0, 5.0, 5.0]), Some(Level::Medium));
        assert_eq!(level_of(&[3.0, 3.0, 4.0, 5.1, 5.1]), Some(Level::Low));
    }

    #[test]
    fn the_level_gate_needs_a_louder_note_that_overlaps() {
        let peak = |mut n: NoteEnvelope, db: f64| {
            n.peak_db = Decibels::clamped(db);
            n
        };
        // A loud note before and after, touching and overlapping.
        let touching: Vec<NoteEnvelope> = (0..3)
            .map(|i| {
                peak(
                    note(f64::from(i), 1.0, 110.0, None),
                    -6.0 - 20.0 * f64::from(i % 2),
                )
            })
            .collect();
        assert_eq!(pool(&touching), [true, true, true]);
        let overlapping = vec![
            peak(note(0.0, 1.5, 110.0, None), -6.0),
            peak(note(1.0, 1.5, 110.0, None), -26.0),
            peak(note(3.0, 1.5, 110.0, None), -6.0),
        ];
        assert_eq!(pool(&overlapping), [true, false, true]);
        // 8 dB down lies inside the gate.
        let inside = vec![
            peak(note(0.0, 1.5, 110.0, None), -6.0),
            peak(note(1.0, 1.5, 110.0, None), -14.0),
            peak(note(3.0, 1.5, 110.0, None), -6.0),
        ];
        assert_eq!(pool(&inside), [true, true, true]);
        let quiet = |db: f64| {
            pool(&[
                peak(note(0.0, 1.5, 110.0, None), -6.0),
                peak(note(1.0, 1.5, 110.0, None), db),
                peak(note(3.0, 1.5, 110.0, None), -6.0),
            ])
        };
        assert_eq!(quiet(-18.0), [true, false, true], "exactly 12 dB down");
        assert_eq!(quiet(-17.9), [true, true, true]);
    }

    #[test]
    fn a_median_of_0_under_a_spread_writes_the_spread_itself() {
        // Five holds of 0 ms and three of 20 ms: the IQR is 20 ms, and a
        // ratio to the median would be infinite.
        let values = [0.0, 0.0, 0.0, 0.0, 0.0, 0.02, 0.02, 0.02];
        let read = statistic(values.into_iter(), Seconds::clamped, None).unwrap();
        let reasons = read.confidence().reasons();
        assert_eq!(reasons[1].code(), ReasonCode::ZeroMedian);
        assert_eq!(reasons[1].value(), Signal::Number(0.02));
        assert!(
            reasons
                .iter()
                .all(|r| r.code() != ReasonCode::RelativeSpread)
        );
    }

    #[test]
    fn the_level_spread_needs_3_pooled_notes() {
        let notes: Vec<NoteEnvelope> = (0..3)
            .map(|i| note(f64::from(i), 1.0, 110.0, None))
            .collect();
        let pool: Vec<&NoteEnvelope> = notes.iter().collect();
        assert!(level_spread(&pool).unwrap().value().is_some());
        assert!(level_spread(&pool[..2]).unwrap().value().is_none());
    }

    #[test]
    fn each_lock_has_its_report_name() {
        assert_eq!(lock_name(PumpLock::Beat), "beat");
        assert_eq!(lock_name(PumpLock::Note), "note");
        assert_eq!(lock_name(PumpLock::Free), "free");
        assert_eq!(lock_name(PumpLock::None), "none");
    }

    #[test]
    fn a_filter_error_names_the_layer_and_keeps_its_source() {
        use std::error::Error as _;
        let rate = SampleRate::new(22_050).unwrap();
        let filter =
            crate::dsp::band_magnitude(&[0.0; 64], rate, Hz::clamped(100.0), Hz::clamped(20_000.0))
                .unwrap_err();
        let error = EnvelopeError::from(filter);
        assert_eq!(
            error.to_string(),
            format!("cannot read the envelope layer: {filter}")
        );
        assert!(error.source().is_some());
    }

    proptest! {
        #[test]
        fn the_pool_does_not_depend_on_note_order(
            notes in proptest::collection::vec(
                (0.0..10.0f64, 0.05..4.0f64, 30.0..2000.0f64, proptest::option::of(0.2..1.0f64)),
                1..12,
            ),
            shift in 0usize..12,
        ) {
            let notes: Vec<NoteEnvelope> =
                notes.into_iter().map(|(o, l, f, k)| note(o, l, f, k)).collect();
            let mut rotated = notes.clone();
            let shift = shift % notes.len();
            rotated.rotate_left(shift);
            let mut pooled = pool(&notes);
            pooled.rotate_left(shift);
            prop_assert_eq!(pool(&rotated), pooled);
        }
    }
}
