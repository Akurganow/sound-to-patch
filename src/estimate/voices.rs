//! The voices layer: how many notes sound at once, and how a mono line joins
//! its notes.
//!
//! The count comes from the note list and each note's held part. The join
//! statistics come from the mono mix around each join. The method page is
//! `docs/src/analysis/voices.md`.

use std::error::Error;
use std::fmt;

use crate::audio::SampleRate;
use crate::cast;
use crate::confidence::{Confidence, ConfidenceError, Level, NoSpread, Reason, ReasonCode, Signal};
use crate::dsp::{Stft, StftError, StftParams, frame_descriptors, quantile};
use crate::estimate::envelope::{
    EnvelopeLayer, KNEE_END_GAP, LevelTrack, RMS_PERIODS, SILENCE_DB, rms_window,
};
use crate::notes::{Note, NoteList};
use crate::units::{Cents, Decibels, Hz, Seconds};

/// Notes this close in pitch count as one voice: seven detuned copies of one
/// pitch form one cluster.
pub const CLUSTER_CENTS: Cents = Cents::from_const(60.0);
/// Two notes count as two voices when their held parts overlap for this long or
/// more, and a later note starting this far before an earlier note's held end
/// stacks on it.
pub const OVERLAP_MIN: Seconds = Seconds::from_const(0.05);
/// Consecutive notes join when the gap between them is under this.
pub const JOIN_GAP_MAX: Seconds = Seconds::from_const(0.03);
/// The dip of a join is read this far either side of the onset.
pub const JOIN_SPAN: Seconds = Seconds::from_const(0.025);
/// Each note's level beside a join is read over at most this long.
pub const SUSTAIN_SPAN: Seconds = Seconds::from_const(0.1);
/// Each neighbour's level is this quantile of its track: a window of one period
/// of the lower note still ripples on the higher one.
pub const SUSTAIN_QUANTILE: f64 = 0.1;
/// The centroid is read over this long on each side of a join.
pub const CENTROID_SPAN: Seconds = Seconds::from_const(0.02);
/// A join that dips this far under the quieter neighbour is a retrigger.
///
/// Between a 0 dB join and a 12 dB dip that rises over 5 ms at 110 Hz, which
/// reads 1.35 dB through a window of one period. On a sampled set of generated
/// joins, 1 dB keeps 91 % of legato joins and finds 86 % of retriggers of 12 dB
/// or more with attacks of 5 ms or more.
pub const DIP_RETRIGGER: Decibels = Decibels::from_const(1.0);
/// An attack longer than this hides a retrigger, so a join reads low.
pub const ATTACK_HIDES_JOIN: Seconds = Seconds::from_const(0.1);
/// The share of frames that agree for a high count.
const AGREE_HIGH: f64 = 0.9;
/// The share of frames that agree for a medium count.
const AGREE_MEDIUM: f64 = 0.7;

/// The convention of the count.
pub const VOICES_CONVENTION: &str = "each note counts from its onset to its note-off knee, or to \
25 ms before its end without one, so a release tail holds no voice; pitches within 60 cents form \
one voice; max_polyphony is the most voices that have started by any onset and are held 50 ms after \
it, mono when that is 1; mean_polyphony is the mean count over the time any note is held, so short \
overlaps can lift it above max_polyphony; null without notes";

/// The convention of a join.
pub const JOIN_CONVENTION: &str = "consecutive notes of a mono line whose gap, the later onset \
minus the earlier end in the note list, is under 30 ms, and whose later onset lies less than 50 ms \
inside the earlier note's held part; dip_db is the 10th percentile of the \
level of the quieter note over up to 100 ms beside the join minus the lowest level from 25 ms before \
the onset, or from the earlier held end when that comes first, to 25 ms after it, on an RMS over one period of the lower note; centroid_change is the centroid over f0 \
after the join over the same before it, minus 1, over 20 ms or two periods each side, and the \
verdict does not read it, and it is null when either side holds no signal or runs past either end of the stem; \
legato when dip_db is under 1; dip_db and legato are null with no estimate when neither note has a \
level beside the join; confidence is low when the verdict is legato, as a retrigger with an instant \
attack keeps its level too, when no attack was read, or when the median attack is over 100 ms, else \
medium";

/// What the voices layer reads.
#[derive(Clone, Copy, Debug)]
pub struct VoicesInput<'a> {
    /// The mono mix at the native rate.
    pub mono: &'a [f64],
    /// The native rate.
    pub rate: SampleRate,
    /// The note list, transcribed or given.
    pub notes: &'a NoteList,
    /// The envelope layer of the same notes, for their knees and attack.
    pub envelope: &'a EnvelopeLayer,
}

/// The join between two consecutive notes of a mono line.
#[derive(Clone, Debug, PartialEq)]
pub struct Join {
    /// The later note's onset.
    pub time: Seconds,
    /// The later onset minus the earlier note's end in the list; negative
    /// when they overlap there.
    pub gap: f64,
    /// How far the level falls at the join under the quieter neighbour; `None`
    /// when neither note has a level beside the join.
    pub dip: Option<Decibels>,
    /// The centroid over f0 after the join over the same before it, minus 1;
    /// `None` when either side holds no signal or runs past either end of the
    /// stem.
    pub centroid_change: Option<f64>,
    /// The join shows no new attack; `None` without a dip.
    pub legato: Option<bool>,
    /// How far the layer trusts the verdict.
    pub confidence: Confidence<NoSpread>,
}

/// The voices layer of a stem.
#[derive(Clone, Debug, PartialEq)]
pub struct VoicesLayer {
    /// One voice at a time, by [`VoicesLayer::max_polyphony`].
    pub mono: bool,
    /// The most voices at once.
    pub max_polyphony: usize,
    /// The mean count over the time that any note is held. An overlap under
    /// [`OVERLAP_MIN`] counts here and not in `max_polyphony`, so this can exceed
    /// it.
    pub mean_polyphony: f64,
    /// The joins of a mono line, in time order; empty on any other line.
    pub joins: Vec<Join>,
    /// The confidence of the count; no estimate without notes.
    pub confidence: Confidence<NoSpread>,
}

/// Why the voices layer failed.
#[derive(Debug)]
pub enum VoicesError {
    /// A confidence could not be built.
    Confidence(ConfidenceError),
    /// A spectrum could not be read.
    Stft(StftError),
}

impl fmt::Display for VoicesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Confidence(error) => write!(f, "voices confidence: {error}"),
            Self::Stft(error) => write!(f, "voices spectrum: {error}"),
        }
    }
}

impl Error for VoicesError {}

impl From<ConfidenceError> for VoicesError {
    fn from(error: ConfidenceError) -> Self {
        Self::Confidence(error)
    }
}

impl From<StftError> for VoicesError {
    fn from(error: StftError) -> Self {
        Self::Stft(error)
    }
}

/// A note's onset, the end of its held part and its pitch in hertz.
#[derive(Clone, Copy, Debug)]
struct Held {
    onset: f64,
    end: f64,
    hz: f64,
}

impl VoicesLayer {
    /// Reads the voices layer of a stem.
    ///
    /// # Errors
    ///
    /// Returns a [`VoicesError`] when a confidence or a spectrum cannot be
    /// built.
    pub fn of(input: &VoicesInput<'_>) -> Result<Self, VoicesError> {
        let notes = input.notes.notes();
        if notes.is_empty() {
            return Self::no_count(Reason::new(ReasonCode::NoNoteList, Signal::Flag(true))?);
        }
        let held = held_parts(notes, input.envelope);
        if held.is_empty() {
            return Self::no_count(Reason::new(ReasonCode::TooFewNotes, Signal::Count(0))?);
        }
        let max_polyphony = held
            .iter()
            .map(|h| voices_at(&held, h.onset, h.onset + OVERLAP_MIN.get()).max(1))
            .max()
            .unwrap_or(1);
        let (mean_polyphony, agree) = frame_counts(&held, max_polyphony);
        let level = if agree >= AGREE_HIGH {
            Level::High
        } else if agree >= AGREE_MEDIUM {
            Level::Medium
        } else {
            Level::Low
        };
        let mono = max_polyphony == 1;
        Ok(Self {
            mono,
            max_polyphony,
            mean_polyphony,
            joins: if mono {
                joins(input, notes)?
            } else {
                Vec::new()
            },
            confidence: Confidence::estimate(
                level,
                None,
                vec![Reason::new(
                    ReasonCode::CountAgreement,
                    Signal::Number(agree),
                )?],
            )?,
        })
    }

    fn no_count(reason: Reason) -> Result<Self, VoicesError> {
        Ok(Self {
            mono: false,
            max_polyphony: 0,
            mean_polyphony: 0.0,
            joins: Vec::new(),
            confidence: Confidence::no_estimate(vec![reason])?,
        })
    }

    /// The lines the command prints.
    #[must_use]
    pub fn summary(&self) -> Vec<String> {
        if !self.confidence.is_estimate() {
            return vec!["voices: no notes, so no count".to_owned()];
        }
        let legato = self.joins.iter().filter(|j| j.legato == Some(true)).count();
        vec![format!(
            "voices: {}, up to {}, mean {:.2}; {} joins, {legato} legato",
            if self.mono { "mono" } else { "poly" },
            self.max_polyphony,
            self.mean_polyphony,
            self.joins.len(),
        )]
    }
}

fn hz(note: &Note) -> f64 {
    Hz::from(note.pitch()).get()
}

/// The end of a note's held part: its knee, or [`KNEE_END_GAP`] before its end
/// when it has none. A release tail holds no voice.
fn held_end(note: &Note, envelope: &EnvelopeLayer) -> f64 {
    let knee = envelope
        .notes
        .iter()
        .find(|e| e.onset == note.onset() && e.pitch == note.pitch())
        .and_then(|e| e.knee);
    knee.map_or(note.offset().get() - KNEE_END_GAP.get(), Seconds::get)
        .max(note.onset().get())
}

fn is_silent(note: &Note, envelope: &EnvelopeLayer) -> bool {
    envelope.silent.contains(&(note.onset(), note.pitch()))
}

/// Each note's held part. A note that does not sound holds none, nor does one
/// that ends within [`KNEE_END_GAP`] of its onset.
fn held_parts(notes: &[Note], envelope: &EnvelopeLayer) -> Vec<Held> {
    notes
        .iter()
        .filter(|n| !is_silent(n, envelope))
        .map(|n| Held {
            onset: n.onset().get(),
            end: held_end(n, envelope),
            hz: hz(n),
        })
        .filter(|h| h.end > h.onset)
        .collect()
}

/// The clusters of the pitches that started by `started` and are held at `t`.
fn voices_at(held: &[Held], started: f64, t: f64) -> usize {
    let mut pitches: Vec<f64> = held
        .iter()
        .filter(|h| h.onset <= started && t <= h.end)
        .map(|h| h.hz)
        .collect();
    pitches.sort_by(f64::total_cmp);
    let mut count = usize::from(!pitches.is_empty());
    for pair in pitches.windows(2) {
        if 1200.0 * (pair[1] / pair[0]).log2() > CLUSTER_CENTS.get() {
            count += 1;
        }
    }
    count
}

/// The mean count over the time any note is held, and the share of that time
/// that agrees with `max`: one voice for a mono line, two or more otherwise.
fn frame_counts(held: &[Held], max: usize) -> (f64, f64) {
    let mut edges: Vec<f64> = held.iter().flat_map(|h| [h.onset, h.end]).collect();
    edges.sort_by(f64::total_cmp);
    let (mut total, mut time, mut agree) = (0.0, 0.0, 0.0);
    for pair in edges.windows(2) {
        let span = pair[1] - pair[0];
        let middle = f64::midpoint(pair[0], pair[1]);
        let count = voices_at(held, middle, middle);
        if span > 0.0 && count > 0 {
            total += cast::usize_to_f64(count) * span;
            time += span;
            if (max == 1) == (count == 1) {
                agree += span;
            }
        }
    }
    if time > 0.0 {
        (total / time, agree / time)
    } else {
        (1.0, 1.0)
    }
}

/// The joins of a mono line. A note that starts well inside the earlier one's
/// held part stacks on it, joins nothing and leaves the earlier note in place, as
/// a silent note does.
fn joins(input: &VoicesInput<'_>, notes: &[Note]) -> Result<Vec<Join>, VoicesError> {
    let attack = input.envelope.attack.value().map(|s| s.median().get());
    let mut read = Vec::new();
    let mut earlier: Option<&Note> = None;
    for note in notes.iter().filter(|n| !is_silent(n, input.envelope)) {
        let start = note.onset().get();
        if let Some(before) = earlier {
            let stacked = start <= before.onset().get()
                || start + OVERLAP_MIN.get() < held_end(before, input.envelope);
            if stacked {
                continue;
            }
            let gap = start - before.offset().get();
            if gap < JOIN_GAP_MAX.get() {
                read.push(join(input, before, note, gap, attack)?);
            }
        }
        earlier = Some(note);
    }
    Ok(read)
}

fn join(
    input: &VoicesInput<'_>,
    before: &Note,
    after: &Note,
    gap: f64,
    attack: Option<f64>,
) -> Result<Join, VoicesError> {
    let at = after.onset().get();
    let span = JOIN_SPAN.get();
    // A transcribed onset can land after the silence it follows, so the dip reads
    // from the earlier note's held end at the latest.
    let dip_from = (at - span).min(held_end(before, input.envelope));
    let from = (dip_from - SUSTAIN_SPAN.get()).max(before.onset().get());
    let stem_end = cast::usize_to_f64(input.mono.len()) / input.rate.hz().get();
    let to = (at + span + SUSTAIN_SPAN.get())
        .min(after.offset().get())
        .min(stem_end);
    // Shorter than a period, a window ripples with the waveform.
    let lower = Hz::clamped(hz(before).min(hz(after)));
    let fs = input.rate.hz().get();
    let margin = rms_window(lower, RMS_PERIODS).get();
    let first = cast::floor_to_usize((from - margin).max(0.0) * fs)
        .unwrap_or(0)
        .min(input.mono.len());
    let last = cast::ceil_to_usize((to + margin) * fs)
        .unwrap_or(first)
        .clamp(first, input.mono.len());
    let track = LevelTrack::rms(
        &input.mono[first..last],
        first,
        input.rate,
        (Seconds::clamped(from), Seconds::clamped(to)),
        lower,
        RMS_PERIODS,
    );
    let level = |a: f64, b: f64| -> &[f64] {
        let (lo, hi) = (track.index(a), track.index(b));
        &track.db()[lo.min(hi)..hi]
    };
    let low_level = |a: f64, b: f64| {
        let mut sorted = level(a, b).to_vec();
        sorted.sort_by(f64::total_cmp);
        quantile(&sorted, SUSTAIN_QUANTILE).filter(|v| *v > SILENCE_DB.get())
    };
    let left = low_level(from, dip_from);
    let right = low_level(at + span, to);
    let low = level(dip_from, at + span)
        .iter()
        .copied()
        .fold(f64::INFINITY, f64::min);
    let quieter = match (left, right) {
        (Some(l), Some(r)) => Some(l.min(r)),
        (Some(v), None) | (None, Some(v)) => Some(v),
        (None, None) => None,
    };
    let dip = quieter
        .filter(|_| low.is_finite())
        .map(|q| (q - low).max(0.0));
    let centroid_change = centroid_change(input, before, after)?;
    let legato = dip.map(|d| d < DIP_RETRIGGER.get());
    let confidence = match (dip, attack) {
        (None, _) => Confidence::no_estimate(vec![Reason::new(
            ReasonCode::TooFewFrames,
            Signal::Count(0),
        )?])?,
        (Some(_), None) => Confidence::estimate(
            Level::Low,
            None,
            vec![Reason::new(ReasonCode::NoAttackRead, Signal::Flag(true))?],
        )?,
        (Some(_), Some(attack)) => {
            let hides = attack > ATTACK_HIDES_JOIN.get();
            let kept = legato == Some(true);
            Confidence::estimate(
                if hides || kept {
                    Level::Low
                } else {
                    Level::Medium
                },
                None,
                vec![
                    Reason::new(ReasonCode::AttackHidesJoin, Signal::Flag(hides))?,
                    Reason::new(ReasonCode::LevelKeptAtJoin, Signal::Flag(kept))?,
                ],
            )?
        }
    };
    Ok(Join {
        time: after.onset(),
        gap,
        dip: dip.map(Decibels::clamped),
        centroid_change,
        legato,
        confidence,
    })
}

/// The centroid over f0 over [`CENTROID_SPAN`] after the join over the same
/// before it, minus 1; `None` when either side holds no signal or runs past
/// either end of the stem.
fn centroid_change(
    input: &VoicesInput<'_>,
    before: &Note,
    after: &Note,
) -> Result<Option<f64>, VoicesError> {
    let fs = input.rate.hz().get();
    let at = cast::round_to_usize(after.onset().get() * fs).unwrap_or(0);
    let lower = hz(before).min(hz(after));
    let len = cast::round_to_usize(f64::max(CENTROID_SPAN.get(), 2.0 / lower) * fs)
        .unwrap_or(0)
        .max(4);
    if at < len {
        return Ok(None);
    }
    let side = |range: std::ops::Range<usize>, f0: f64| -> Result<Option<f64>, VoicesError> {
        let Some(samples) = input.mono.get(range) else {
            return Ok(None);
        };
        let params = StftParams::new(samples.len())?;
        let mut stft = Stft::new(params);
        // Frame 2 of a hop of a quarter is centred on the samples.
        let magnitudes: Vec<f64> = stft.frame(samples, 2).iter().map(|c| c.norm()).collect();
        Ok(frame_descriptors(&magnitudes, params.bin_hz(1, input.rate))
            .map(|d| d.centroid.get() / f0))
    };
    let early = side(at - len..at, hz(before))?;
    let late = side(at..at + len, hz(after))?;
    Ok(match (early, late) {
        (Some(e), Some(l)) if e > 0.0 => Some(l / e - 1.0),
        _ => None,
    })
}
