//! The final note list: the list rewritten with the layer report, in the
//! ticks of `notes.mid`.
//!
//! [`finish`] reads what the report says of each note and of the line. It
//! merges each pitch drop into one note at its settled pitch, gives a note
//! under a vibrato the key of the vibrato's mean pitch, ends each note at
//! its note-off knee, and writes each glide as two overlapping notes. The
//! velocity follows the full-band level of each note when the level spread
//! gives velocity a path to the level, and is one constant otherwise. Only
//! the pitch motion of a mono line that no route models becomes bends.

use super::write::{Span, encode_spans, ticks_per_second};
use super::{
    Contour, DEFAULT_VELOCITY, MODEL_RATE, MidiWriteError, NoteList, TRACK_FRAME, TRACK_HOP,
    Velocity, min_note_length, profile,
};

use crate::cast;
use crate::units::{Bpm, Decibels, Hz, MidiPitch, Seconds, Semitones};

/// A glide join overlaps the two notes by at least this long, rounded up
/// to whole ticks.
pub const GLIDE_OVERLAP: Seconds = Seconds::from_const(0.005);
/// A note without a note-off knee ends this long before its end in the
/// list.
pub const NO_KNEE_CUT: Seconds = Seconds::from_const(0.050);
/// A pad note ends where its level falls this far below its plateau.
pub const PAD_FALL: Decibels = Decibels::from_const(20.0);
/// A glide joins two notes when the later onset lies this close to the
/// glide's motion, from its start to its end.
pub const GLIDE_SLACK: Seconds = Seconds::from_const(0.050);
/// A pitch drop takes in the notes that start from this long before its
/// onset until it settles.
pub const DROP_SLACK: Seconds = Seconds::from_const(0.030);
/// A note of a mono line gets bends when its contour leaves its key by
/// this much outside every route's span, over the tracker's reach.
pub const BEND_MIN: Semitones = Semitones::from_const(0.5);
/// The bend range the file assumes; a host must set the synth to it, and
/// `render` reads no bend.
pub const BEND_RANGE: Semitones = Semitones::from_const(2.0);
/// The level law: a note `d` dB under the loudest gets velocity
/// `127 * 10^(-d / 40)`.
pub const VELOCITY_LAW_DB: f64 = 40.0;

/// The centre of the 14-bit pitch-bend value.
const BEND_CENTRE: u16 = 8192;

/// What the layer report says of one note of the list.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NoteFacts {
    /// The note-off knee; `None` when no knee shows.
    pub knee: Option<Seconds>,
    /// Where the level of a note of a polyphonic line falls [`PAD_FALL`]
    /// below its plateau, within its span in the list; `None` when the
    /// level does not fall that far by its end in the list, or the line is
    /// mono.
    pub pad_end: Option<Seconds>,
    /// The full-band level of the note.
    pub level: Option<Decibels>,
    /// The envelope layer read the note, so a missing knee means none
    /// shows; without a read nothing was observed.
    pub read: bool,
}

/// A glide of the pitch layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlideSpan {
    /// The start of the motion.
    pub start: Seconds,
    /// The interval from the start pitch to the end pitch.
    pub interval: Semitones,
    /// The full 0 to 100 % time.
    pub time: Seconds,
}

/// A pitch drop of the pitch layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropSpan {
    /// The onset of the strike.
    pub onset: Seconds,
    /// The settled frequency.
    pub f_end: Hz,
    /// The frequency above `f_end` at the onset; negative for a rise.
    pub df_hz: f64,
    /// Where the drop has settled.
    pub settled: Seconds,
}

/// A vibrato of the pitch layer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VibratoSpan {
    /// The start of the span.
    pub start: Seconds,
    /// The end of the span.
    pub end: Seconds,
    /// The mean pitch of the span.
    pub pitch: Hz,
}

/// Reads the full-band level of the stem from one time to another, with
/// the RMS window of a frequency's period.
pub type LevelReader<'a> = &'a dyn Fn(Seconds, Seconds, Hz) -> Option<Decibels>;

/// What [`finish`] reads.
#[derive(Clone, Copy)]
pub struct FinishInput<'a> {
    /// The note list.
    pub notes: &'a NoteList,
    /// One record per note of the list, in its order; a missing one reads
    /// as no knee, no pad end and no level.
    pub facts: &'a [NoteFacts],
    /// The tempo of the file.
    pub bpm: Bpm,
    /// One voice at a time: notes overlap only at glide joins. Otherwise
    /// the line is polyphonic, and a note ends at its pad end where it has
    /// one.
    pub mono: bool,
    /// Velocity has a path to the level, so it follows each note's level.
    pub level_velocity: bool,
    /// The glides, in time order.
    pub glides: &'a [GlideSpan],
    /// The pitch drops.
    pub drops: &'a [DropSpan],
    /// The vibratos.
    pub vibrato: &'a [VibratoSpan],
    /// The full-band level of the stem. It fills a level that the facts
    /// lack.
    pub level: LevelReader<'a>,
}

/// Which rule set the end of a final note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndRule {
    /// The note-off knee.
    Knee,
    /// No knee shows: [`NO_KNEE_CUT`] before the end in the list.
    NoKnee,
    /// The pad end.
    Pad,
    /// The cut would make the note shorter than the BPM's minimum.
    Minimum,
    /// The next note of a mono line starts there.
    NextNote,
    /// A glide join: [`GLIDE_OVERLAP`] after the next note-on.
    Glide,
    /// No rule read the note: the envelope layer skipped it and it has no
    /// pad end. It ends [`NO_KNEE_CUT`] before its end in the list, as
    /// without a knee.
    Unread,
}

impl EndRule {
    /// The name `layers.json` gives the rule.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Knee => "knee",
            Self::NoKnee => "no_knee",
            Self::Pad => "pad",
            Self::Minimum => "minimum",
            Self::NextNote => "next_note",
            Self::Glide => "glide",
            Self::Unread => "unread",
        }
    }
}

/// One note of the final list, in ticks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FinalNote {
    /// The note-on tick.
    pub start: u64,
    /// The note-off tick, after `start`.
    pub end: u64,
    /// The key.
    pub key: u8,
    /// The velocity.
    pub velocity: Velocity,
    /// The rule that set the end.
    pub end_rule: EndRule,
    /// The note's onset in the list; the earliest one for a merged drop.
    pub list_onset: Seconds,
    /// The note's end in the list; the latest one for a merged drop.
    pub list_end: Seconds,
    /// The full-band level, the highest of a merged drop.
    pub level: Option<Decibels>,
    /// The transcriber's octave flag, true when any merged note has it.
    pub octave_uncertain: Option<bool>,
}

/// The final note list.
#[derive(Clone, Debug, PartialEq)]
pub struct FinalNotes {
    /// The notes, sorted by start, then key.
    pub notes: Vec<FinalNote>,
    /// The pitch-bend events: a tick and a 14-bit value, centre 8192.
    pub bends: Vec<(u64, u16)>,
    /// The tempo of the ticks.
    pub bpm: Bpm,
    /// Velocity follows each note's level.
    pub level_velocity: bool,
    /// The line was mono, so the mono-line rules ran.
    pub mono: bool,
    /// The notes of a mono line dropped because another started on the
    /// same tick.
    pub dropped: usize,
}

impl FinalNotes {
    /// The time of a tick.
    #[must_use]
    pub fn seconds(&self, tick: u64) -> Seconds {
        seconds_of(tick, ticks_per_second(self.bpm))
    }

    /// Encodes the list as a format 0 file on channel 1, with a track
    /// name when one is given.
    ///
    /// # Errors
    ///
    /// The errors of [`write_midi`](super::write_midi).
    pub fn to_midi(&self, name: Option<&str>) -> Result<Vec<u8>, MidiWriteError> {
        let spans: Vec<Span> = self
            .notes
            .iter()
            .map(|n| (midly::num::u7::new(n.key), n.start, n.end, n.velocity))
            .collect();
        encode_spans(spans, &self.bends, self.bpm, name)
    }
}

/// A note on its way to the final list, in seconds until the ticks.
#[derive(Clone, Debug)]
struct Work<'a> {
    onset: f64,
    list_onset: f64,
    list_end: f64,
    key: u8,
    facts: NoteFacts,
    octave_uncertain: Option<bool>,
    contour: Option<&'a Contour>,
}

/// Rewrites the note list with the layer report.
///
/// The steps run in this order:
///
/// 1. Each pitch drop merges the notes that start from [`DROP_SLACK`]
///    before its onset until it settles, touch each other and lie on keys
///    from `f_end` to its start pitch, into one note at the key of
///    `f_end`.
/// 2. A note that a vibrato covers for more than half its length takes the
///    key of the vibrato's mean pitch, when that lies within a semitone.
/// 3. On a polyphonic line a note ends at its pad end where it has one.
///    Otherwise it ends at its knee, else [`NO_KNEE_CUT`] before its end in
///    the list. No note ends after its end in the list. A cut never makes a
///    note shorter than [`min_note_length`]; a note the list gives shorter
///    keeps its length.
/// 4. Times become ticks with `round(seconds * BPM / 60 * 480)`, and each
///    note lasts at least one tick. A note without a level reads it over
///    this written span, which on a mono line stops at the next later
///    onset.
/// 5. In a mono line, of the notes that start on one tick only one stays:
///    a note without the transcriber's octave flag before one with it, then
///    the longer, then the lower key.
/// 6. In a mono line, a glide joins the pair of neighbours whose interval
///    lies within a semitone of its own and whose later onset lies within
///    [`GLIDE_SLACK`] of its motion, the nearest to its start. The later
///    note starts at the glide's start, or at its own onset when the glide
///    starts after it, and the earlier one ends [`GLIDE_OVERLAP`] after
///    that start, rounded up to whole ticks, whatever its knee says. Every
///    other note of a mono line ends no later than the next note starts.
/// 7. With level velocity, the loudest note gets 127 and a note `d` dB
///    under it `round(127 * 10^(-d / 40))`, at least 1; a note without a
///    level, and every note otherwise, gets [`DEFAULT_VELOCITY`].
/// 8. On a mono line, a note whose contour leaves its key by [`BEND_MIN`]
///    or more, of one sign, over adjacent frames that span the tracker's
///    reach (a quarter frame plus one period), outside every glide, drop
///    and vibrato span, gets a bend at each contour point outside those
///    spans, clamped to [`BEND_RANGE`], and a centred bend at its end.
#[must_use]
pub fn finish(input: &FinishInput<'_>) -> FinalNotes {
    let mut work: Vec<Work<'_>> = input
        .notes
        .notes()
        .iter()
        .enumerate()
        .map(|(i, note)| Work {
            onset: note.onset().get(),
            list_onset: note.onset().get(),
            list_end: note.offset().get(),
            key: key_of(note.pitch().get()),
            facts: input.facts.get(i).copied().unwrap_or_default(),
            octave_uncertain: note.octave_uncertain(),
            contour: note.contour(),
        })
        .collect();
    for drop in input.drops {
        merge_drop(&mut work, drop);
    }
    for note in &mut work {
        follow_vibrato(note, input.vibrato);
    }
    let tps = ticks_per_second(input.bpm);
    let tick = |seconds: f64| tick_of(seconds, tps);
    let minimum = min_note_length(input.bpm).get();
    let mut notes: Vec<(FinalNote, Option<&Contour>)> = work
        .iter()
        .map(|w| {
            let (mut end, mut rule) = match (input.mono, w.facts.pad_end, w.facts.knee) {
                (false, Some(pad), _) => (pad.get(), EndRule::Pad),
                (_, _, Some(knee)) => (knee.get(), EndRule::Knee),
                _ if w.facts.read => (w.list_end - NO_KNEE_CUT.get(), EndRule::NoKnee),
                _ => (w.list_end - NO_KNEE_CUT.get(), EndRule::Unread),
            };
            end = end.min(w.list_end);
            // The minimum lengthens a cut note up to its list end, and
            // shortens none.
            let floor = (w.onset + minimum).min(w.list_end);
            if end < floor {
                end = floor;
                rule = EndRule::Minimum;
            }
            let start = tick(w.onset);
            let note = FinalNote {
                start,
                end: tick(end).max(start + 1),
                key: w.key,
                velocity: DEFAULT_VELOCITY,
                end_rule: rule,
                list_onset: Seconds::clamped(w.list_onset),
                list_end: Seconds::clamped(w.list_end),
                level: w.facts.level,
                octave_uncertain: w.octave_uncertain,
            };
            (note, w.contour)
        })
        .collect();
    notes.sort_by_key(|(n, _)| (n.start, n.key));
    for i in 0..notes.len() {
        let note = notes[i].0;
        if note.level.is_none() {
            // On a mono line the span stops at the next later onset, as the
            // layer's own span does, so a short note reads none of the next.
            let next = notes[i..]
                .iter()
                .map(|(n, _)| n.start)
                .find(|&start| input.mono && start > note.start)
                .unwrap_or(u64::MAX);
            notes[i].0.level = (input.level)(
                seconds_of(note.start, tps),
                seconds_of(note.end.min(next), tps),
                Hz::from(MidiPitch::clamped(f64::from(note.key))),
            );
        }
    }
    let dropped = if input.mono {
        join_mono(&mut notes, input, tps)
    } else {
        Vec::new()
    };
    let bends = if input.mono {
        bends_of(&notes, &dropped, input, tps)
    } else {
        Vec::new()
    };
    let mut list: Vec<FinalNote> = notes.into_iter().map(|(n, _)| n).collect();
    if input.level_velocity {
        level_velocities(&mut list);
    }
    FinalNotes {
        notes: list,
        bends,
        bpm: input.bpm,
        level_velocity: input.level_velocity,
        mono: input.mono,
        dropped: dropped.len(),
    }
}

/// The time of a tick.
fn seconds_of(tick: u64, tps: f64) -> Seconds {
    Seconds::clamped(cast::u64_to_f64(tick) / tps)
}

/// The nearest tick of a time, 0 before the start.
fn tick_of(seconds: f64, tps: f64) -> u64 {
    cast::round_to_usize(seconds * tps)
        .and_then(|t| u64::try_from(t).ok())
        .unwrap_or(0)
}

/// The nearest key of a pitch, within 0 to 127.
fn key_of(pitch: f64) -> u8 {
    cast::round_to_usize(pitch.clamp(0.0, 127.0))
        .and_then(|key| u8::try_from(key).ok())
        .unwrap_or(127)
}

/// The MIDI pitch of a frequency, `None` outside MIDI notes 0 to 127.
fn pitch_of(f0: Hz) -> Option<f64> {
    MidiPitch::try_from(f0).ok().map(MidiPitch::get)
}

/// Merges the notes a drop takes in into one at the key of `f_end`: from
/// [`DROP_SLACK`] before its onset until it settles, each starting before
/// the one before it ends plus that slack, each on a key from `f_end` to
/// the drop's start pitch.
fn merge_drop(work: &mut Vec<Work<'_>>, drop: &DropSpan) {
    let Some(pitch) = pitch_of(drop.f_end) else {
        return;
    };
    let start = Hz::new(drop.f_end.get() + drop.df_hz)
        .ok()
        .and_then(pitch_of)
        .unwrap_or(pitch);
    let (low, high) = (pitch.min(start).round(), pitch.max(start).round());
    let from = drop.onset.get() - DROP_SLACK.get();
    let mut taken: Vec<usize> = Vec::new();
    for (i, note) in work.iter().enumerate() {
        let touches = taken
            .last()
            .is_none_or(|&j| note.onset <= work[j].list_end + DROP_SLACK.get());
        if (from..=drop.settled.get()).contains(&note.onset)
            && (low..=high).contains(&f64::from(note.key))
            && touches
        {
            taken.push(i);
        }
    }
    let Some(&first) = taken.first() else {
        return;
    };
    let mut merged = work[first].clone();
    for &i in &taken[1..] {
        let other = &work[i];
        merged.onset = merged.onset.min(other.onset);
        merged.list_onset = merged.list_onset.min(other.list_onset);
        if other.list_end > merged.list_end {
            merged.list_end = other.list_end;
            merged.facts.knee = other.facts.knee;
            merged.facts.pad_end = other.facts.pad_end;
            merged.facts.read = other.facts.read;
        }
        merged.facts.level = match (merged.facts.level, other.facts.level) {
            (Some(a), Some(b)) => Some(if b.get() > a.get() { b } else { a }),
            (a, b) => a.or(b),
        };
        merged.octave_uncertain = match (merged.octave_uncertain, other.octave_uncertain) {
            (Some(a), Some(b)) => Some(a || b),
            (a, b) => a.or(b),
        };
    }
    merged.key = key_of(pitch);
    // The drop is a route of the preset: its motion writes no bend.
    merged.contour = None;
    for &i in taken.iter().rev() {
        work.remove(i);
    }
    work.insert(first.min(work.len()), merged);
}

/// Gives a note the key of a vibrato that covers most of it.
fn follow_vibrato(note: &mut Work<'_>, vibrato: &[VibratoSpan]) {
    let length = note.list_end - note.onset;
    for span in vibrato {
        let covered = note.list_end.min(span.end.get()) - note.onset.max(span.start.get());
        if covered > 0.5 * length
            && let Some(pitch) = pitch_of(span.pitch)
            && (pitch - f64::from(note.key)).abs() <= 1.0
        {
            note.key = key_of(pitch);
        }
    }
}

/// Keeps one of the notes of a mono line that start on one tick, writes
/// the glide joins and ends every other note where the next starts.
/// Returns the list spans of the notes that went.
fn join_mono(
    notes: &mut Vec<(FinalNote, Option<&Contour>)>,
    input: &FinishInput<'_>,
    tps: f64,
) -> Vec<(f64, f64)> {
    let dropped = keep_one_per_tick(notes);
    let glides = input.glides;
    let overlap = cast::ceil_to_usize(GLIDE_OVERLAP.get() * tps)
        .and_then(|t| u64::try_from(t).ok())
        .unwrap_or(1)
        .max(1);
    let tick = |seconds: f64| tick_of(seconds, tps);
    let mut joined = vec![false; notes.len()];
    for glide in glides {
        let (from, to) = (
            glide.start.get() - GLIDE_SLACK.get(),
            glide.start.get() + glide.time.get() + GLIDE_SLACK.get(),
        );
        let at = tick(glide.start.get());
        let best = (0..notes.len().saturating_sub(1))
            .filter(|&i| !joined[i])
            .filter(|&i| {
                let (a, b) = (&notes[i].0, &notes[i + 1].0);
                let onset = b.list_onset.get();
                let interval = f64::from(b.key) - f64::from(a.key);
                (from..=to).contains(&onset)
                    && (interval - glide.interval.get()).abs() < 1.0
                    && a.start < at.min(b.start)
            })
            .min_by(|&i, &j| {
                let gap = |k: usize| (notes[k + 1].0.list_onset.get() - glide.start.get()).abs();
                gap(i).total_cmp(&gap(j))
            });
        if let Some(i) = best {
            // A glide that starts after the later onset leaves that onset.
            let b = &mut notes[i + 1].0;
            b.start = b.start.min(at);
            let start = b.start;
            let a = &mut notes[i].0;
            a.end = start + overlap;
            a.end_rule = EndRule::Glide;
            joined[i] = true;
        }
    }
    for i in 0..notes.len().saturating_sub(1) {
        if joined[i] {
            continue;
        }
        let next = notes[i + 1].0.start;
        let a = &mut notes[i].0;
        if a.end > next {
            a.end = next;
            a.end_rule = EndRule::NextNote;
        }
    }
    dropped
}

/// Of the notes sorted by start, then key, that start on one tick, keeps a
/// note without the octave flag before one with it, then the longer, then
/// the lower key. Levels cannot rank them: twins read the same mix from the
/// same start, so their levels differ only where the longer peaks later.
/// Returns the list spans of the notes that went.
fn keep_one_per_tick(notes: &mut Vec<(FinalNote, Option<&Contour>)>) -> Vec<(f64, f64)> {
    let mut kept: Vec<(FinalNote, Option<&Contour>)> = Vec::with_capacity(notes.len());
    let mut dropped = Vec::new();
    for group in notes.chunk_by(|(a, _), (b, _)| a.start == b.start) {
        let rank = |n: &FinalNote| (n.octave_uncertain != Some(true), n.end - n.start);
        // The first of the best, so a tie keeps the lower key.
        let best = (1..group.len()).fold(0, |best, i| {
            if rank(&group[i].0) > rank(&group[best].0) {
                i
            } else {
                best
            }
        });
        for (i, item) in group.iter().enumerate() {
            if i == best {
                kept.push(*item);
            } else {
                dropped.push((item.0.list_onset.get(), item.0.list_end.get()));
            }
        }
    }
    *notes = kept;
    dropped
}

/// Sets each velocity from the note's level under the loudest.
fn level_velocities(notes: &mut [FinalNote]) {
    let Some(loudest) = notes
        .iter()
        .filter_map(|n| n.level.map(Decibels::get))
        .reduce(f64::max)
    else {
        return;
    };
    for note in notes {
        note.velocity = note.level.map_or(DEFAULT_VELOCITY, |level| {
            let value = 127.0 * 10f64.powf((level.get() - loudest) / VELOCITY_LAW_DB);
            cast::round_to_usize(value.clamp(1.0, 127.0))
                .and_then(|v| u8::try_from(v).ok())
                .and_then(|v| Velocity::new(v).ok())
                .unwrap_or(DEFAULT_VELOCITY)
        });
    }
}

/// The time step of the transcriber's note times, one model frame.
fn model_frame() -> f64 {
    profile::frame_ms() / 1000.0
}

/// How far the f0 track reads around a frame: from a quarter frame before
/// its time to one period of `f0` after it. A frame at `t` reads the
/// audio from `t - before` to `t + after`, and its audio time, where the
/// pitch layer puts its records, is `t` less the lag `before - after / 2`.
fn track_reach(f0: Hz) -> (f64, f64) {
    let before = cast::usize_to_f64(TRACK_FRAME / 4) / MODEL_RATE.hz().get();
    (before, 1.0 / f0.get().max(f64::MIN_POSITIVE))
}

/// The bends of the motion no route models.
///
/// A contour frame counts only when everything it reads lies inside its
/// note and outside every other note of the list: the frames of a note
/// change read the note before, and while two notes sound the one-voice
/// f0 track follows only one of them. A note dropped for starting on the
/// same tick still sounds in the stem, so its list span counts too. The
/// note's span is its list onset and end, each one model frame inward, the
/// time step of the transcriber's notes.
fn bends_of(
    notes: &[(FinalNote, Option<&Contour>)],
    dropped: &[(f64, f64)],
    input: &FinishInput<'_>,
    tps: f64,
) -> Vec<(u64, u16)> {
    let modelled = |t: f64| {
        input
            .glides
            .iter()
            .any(|g| (g.start.get()..=g.start.get() + g.time.get()).contains(&t))
            || input
                .drops
                .iter()
                .any(|d| (d.onset.get()..=d.settled.get()).contains(&t))
            || input
                .vibrato
                .iter()
                .any(|v| (v.start.get()..=v.end.get()).contains(&t))
    };
    let mut events: Vec<(u64, u16)> = Vec::new();
    for (i, (note, contour)) in notes.iter().enumerate() {
        let Some(contour) = contour else { continue };
        // A bend stops where the next note starts, so it never bends that
        // note.
        let stop = notes
            .get(i + 1)
            .map_or(note.end, |(next, _)| note.end.min(next.start));
        let (before, after) = track_reach(Hz::from(MidiPitch::clamped(f64::from(note.key))));
        let (onset, end) = (
            note.list_onset.get() + model_frame(),
            note.list_end.get() - model_frame(),
        );
        let apart = |(onset, end): (f64, f64), (from, to): (f64, f64)| to <= onset || end <= from;
        let alone = |from: f64, to: f64| {
            notes.iter().enumerate().all(|(j, (other, _))| {
                j == i || apart((other.list_onset.get(), other.list_end.get()), (from, to))
            }) && dropped.iter().all(|&span| apart(span, (from, to)))
        };
        // Each kept frame: its tick, its offset from the key and its track
        // time.
        let points: Vec<(u64, f64, f64)> = contour
            .points()
            .iter()
            .filter(|p| {
                let (from, to) = (p.time.get() - before, p.time.get() + after);
                from >= onset && to <= end && alone(from, to)
            })
            .filter_map(|p| {
                let audio = p.time.get() - (before - 0.5 * after);
                let at = tick_of(audio, tps);
                let deviation = pitch_of(p.f0)? - f64::from(note.key);
                (!modelled(audio) && (note.start..stop).contains(&at)).then_some((
                    at,
                    deviation,
                    p.time.get(),
                ))
            })
            .collect();
        if !persists(&points, before + after) {
            continue;
        }
        for (at, deviation, _) in points {
            let share = deviation.clamp(-BEND_RANGE.get(), BEND_RANGE.get()) / BEND_RANGE.get();
            let value = cast::round_to_usize((f64::from(BEND_CENTRE) + share * 8191.0).max(0.0))
                .and_then(|v| u16::try_from(v).ok())
                .unwrap_or(BEND_CENTRE)
                .min(16_383);
            events.push((at, value));
        }
        events.push((stop, BEND_CENTRE));
    }
    in_force(events)
}

/// Whether the kept frames hold an offset of [`BEND_MIN`] or more, of one
/// sign, over adjacent track frames that span `reach`, the tracker's reach
/// of the note: a quarter frame plus one period. A shorter excursion fits
/// inside what one frame reads, so jitter and a single event, such as a
/// hit inside a merged note, cannot tell it from motion.
fn persists(points: &[(u64, f64, f64)], reach: f64) -> bool {
    let hop = cast::usize_to_f64(TRACK_HOP) / MODEL_RATE.hz().get();
    // (first time, last time, above the key) of the run of offset frames.
    let mut run: Option<(f64, f64, bool)> = None;
    for &(_, deviation, time) in points {
        if deviation.abs() < BEND_MIN.get() {
            run = None;
            continue;
        }
        let sign = deviation > 0.0;
        // Adjacent frames lie one hop apart; a missing frame ends the run.
        let first = match run {
            Some((first, last, s)) if s == sign && time - last <= 1.5 * hop => first,
            _ => time,
        };
        if time - first >= reach {
            return true;
        }
        run = Some((first, time, sign));
    }
    false
}

/// The bends that change the value in force: of the bends on one tick the
/// last added wins, and one equal to the value before it is left out. A
/// file starts centred.
fn in_force(mut events: Vec<(u64, u16)>) -> Vec<(u64, u16)> {
    // Stable, so the bends of one tick keep their order.
    events.sort_by_key(|&(at, _)| at);
    let mut bends: Vec<(u64, u16)> = Vec::new();
    for (i, &(at, value)) in events.iter().enumerate() {
        if events.get(i + 1).is_some_and(|&(next, _)| next == at) {
            continue;
        }
        if bends.last().map_or(BEND_CENTRE, |&(_, v)| v) != value {
            bends.push((at, value));
        }
    }
    bends
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::{ContourPoint, Note};
    use midly::{MidiMessage, Smf, TrackEventKind};
    use proptest::prelude::*;

    fn s(value: f64) -> Seconds {
        Seconds::new(value).unwrap()
    }

    fn note(onset: f64, offset: f64, key: f64) -> Note {
        Note::new(s(onset), s(offset), MidiPitch::new(key).unwrap()).unwrap()
    }

    fn knee(at: f64) -> NoteFacts {
        NoteFacts {
            knee: Some(s(at)),
            read: true,
            ..NoteFacts::default()
        }
    }

    /// A note the envelope layer read without a knee.
    fn kneeless() -> NoteFacts {
        NoteFacts {
            read: true,
            ..NoteFacts::default()
        }
    }

    fn bpm(value: f64) -> Bpm {
        Bpm::new(value).unwrap()
    }

    fn input<'a>(notes: &'a NoteList, facts: &'a [NoteFacts], tempo: f64) -> FinishInput<'a> {
        FinishInput {
            notes,
            facts,
            bpm: bpm(tempo),
            mono: true,
            level_velocity: false,
            glides: &[],
            drops: &[],
            vibrato: &[],
            level: &|_, _, _| None,
        }
    }

    /// The notes of a file as (start, end, key, velocity) ticks, and its
    /// bends as (tick, value).
    #[expect(clippy::type_complexity, reason = "a test reader")]
    fn parse(bytes: &[u8]) -> (Vec<(u64, u64, u8, u8)>, Vec<(u64, u16)>) {
        let smf = Smf::parse(bytes).unwrap();
        let (mut now, mut open, mut notes, mut bends) = (0_u64, Vec::new(), Vec::new(), Vec::new());
        for event in &smf.tracks[0] {
            now += u64::from(event.delta.as_int());
            if let TrackEventKind::Midi { message, .. } = event.kind {
                match message {
                    MidiMessage::NoteOn { key, vel } => {
                        open.push((now, key.as_int(), vel.as_int()));
                    }
                    MidiMessage::NoteOff { key, .. } => {
                        let i = open.iter().position(|o| o.1 == key.as_int()).unwrap();
                        let (start, key, vel) = open.remove(i);
                        notes.push((start, now, key, vel));
                    }
                    MidiMessage::PitchBend { bend } => bends.push((now, bend.0.as_int())),
                    _ => {}
                }
            }
        }
        notes.sort_unstable();
        (notes, bends)
    }

    #[test]
    fn a_known_line_writes_its_glide_and_pluck_at_fixed_ticks() {
        // 120 BPM: 960 ticks per second. A glides into B from 0.95 s over
        // 100 ms; C is a pluck with no knee.
        let notes = NoteList::new(vec![
            note(0.5, 1.0, 45.0).with_octave_uncertain(false),
            note(1.0, 1.6, 50.0).with_octave_uncertain(true),
            note(2.0, 2.6, 47.0).with_octave_uncertain(false),
        ]);
        let facts = [knee(0.9), knee(1.5), kneeless()];
        let glides = [GlideSpan {
            start: s(0.95),
            interval: Semitones::new(5.0).unwrap(),
            time: s(0.1),
        }];
        let finished = finish(&FinishInput {
            glides: &glides,
            ..input(&notes, &facts, 120.0)
        });
        let rows: Vec<_> = finished
            .notes
            .iter()
            .map(|n| (n.start, n.end, n.key, n.end_rule, n.octave_uncertain))
            .collect();
        assert_eq!(
            rows,
            [
                // 912 + ceil(0.005 s * 960) = 912 + 5.
                (480, 917, 45, EndRule::Glide, Some(false)),
                (912, 1440, 50, EndRule::Knee, Some(true)),
                // 2.6 s - 50 ms.
                (1920, 2448, 47, EndRule::NoKnee, Some(false)),
            ]
        );
        let (parsed, bends) = parse(&finished.to_midi(Some("line.wav")).unwrap());
        assert_eq!(
            parsed,
            [
                (480, 917, 45, 100),
                (912, 1440, 50, 100),
                (1920, 2448, 47, 100)
            ]
        );
        assert_eq!(bends, []);
    }

    #[test]
    fn velocities_follow_the_level_only_with_a_path_to_it() {
        let notes = NoteList::new(vec![note(0.0, 0.5, 40.0), note(1.0, 1.5, 40.0)]);
        let level = |db| NoteFacts {
            level: Some(Decibels::new(db).unwrap()),
            ..knee(10.0)
        };
        let facts = [level(-10.0), level(-16.0)];
        let velocities = |level_velocity| {
            finish(&FinishInput {
                level_velocity,
                ..input(&notes, &facts, 120.0)
            })
            .notes
            .iter()
            .map(|n| n.velocity.get())
            .collect::<Vec<_>>()
        };
        // 127 * 10^(-6 / 40) = 89.9.
        assert_eq!(velocities(true), [127, 90]);
        assert_eq!(velocities(false), [100, 100]);
    }

    #[test]
    fn a_pitch_drop_leaves_one_note_at_f_end() {
        let notes = NoteList::new(vec![
            note(1.0, 1.04, 43.0),
            note(1.03, 1.6, 42.0),
            note(2.0, 2.5, 45.0),
        ]);
        // The drop starts 3 semitones up, so the 45 after it stays.
        let f_end = Hz::from(MidiPitch::new(41.0).unwrap());
        let drops = [DropSpan {
            onset: s(1.0),
            f_end,
            df_hz: f_end.get() * (2f64.powf(0.25) - 1.0),
            settled: s(2.5),
        }];
        let finished = finish(&FinishInput {
            drops: &drops,
            ..input(&notes, &[], 120.0)
        });
        let keys: Vec<_> = finished
            .notes
            .iter()
            .map(|n| (n.key, n.list_onset, n.list_end))
            .collect();
        assert_eq!(keys, [(41, s(1.0), s(1.6)), (45, s(2.0), s(2.5))]);
    }

    #[test]
    fn a_vibrato_gives_its_mean_key() {
        let notes = NoteList::new(vec![note(0.0, 1.0, 61.0)]);
        let vibrato = [VibratoSpan {
            start: s(0.1),
            end: s(0.9),
            pitch: Hz::from(MidiPitch::new(60.2).unwrap()),
        }];
        let finished = finish(&FinishInput {
            vibrato: &vibrato,
            ..input(&notes, &[], 120.0)
        });
        assert_eq!(finished.notes[0].key, 60);
    }

    #[test]
    fn a_pad_note_ends_at_its_pad_end() {
        let notes = NoteList::new(vec![note(0.0, 2.0, 60.0), note(0.0, 2.0, 64.0)]);
        let facts = [
            NoteFacts {
                pad_end: Some(s(1.5)),
                ..knee(1.0)
            },
            knee(1.0),
        ];
        let finished = finish(&FinishInput {
            mono: false,
            ..input(&notes, &facts, 120.0)
        });
        let ends: Vec<_> = finished.notes.iter().map(|n| (n.end, n.end_rule)).collect();
        assert_eq!(ends, [(1440, EndRule::Pad), (960, EndRule::Knee)]);
        // An unread note of a polyphonic line ends 50 ms before its list
        // end, as on a mono line, where a pad end counts for nothing.
        let facts = [
            NoteFacts {
                pad_end: Some(s(1.5)),
                ..knee(1.0)
            },
            NoteFacts::default(),
        ];
        let finished = finish(&FinishInput {
            mono: false,
            ..input(&notes, &facts, 120.0)
        });
        let ends: Vec<_> = finished.notes.iter().map(|n| (n.end, n.end_rule)).collect();
        assert_eq!(ends, [(1440, EndRule::Pad), (1872, EndRule::Unread)]);
        let one = NoteList::new(vec![note(0.0, 2.0, 60.0)]);
        let finished = finish(&input(&one, &facts[..1], 120.0));
        let ends: Vec<_> = finished.notes.iter().map(|n| (n.end, n.end_rule)).collect();
        assert_eq!(ends, [(960, EndRule::Knee)]);
    }

    /// The track's frame step.
    fn hop() -> f64 {
        cast::usize_to_f64(TRACK_HOP) / MODEL_RATE.hz().get()
    }

    /// A contour on the track's frames that holds the key in the note's
    /// first half and `cents` off it in the second.
    fn bent(onset: f64, offset: f64, key: f64, cents: f64) -> Note {
        let f0 = |c: f64| Hz::from(MidiPitch::new(key + c / 100.0).unwrap());
        let middle = f64::midpoint(onset, offset);
        let n = cast::floor_to_usize((offset - onset) / hop()).unwrap_or(0);
        let points = (0..n)
            .map(|i| {
                let t = onset + hop() * cast::usize_to_f64(i);
                ContourPoint {
                    time: s(t),
                    f0: f0(if t >= middle { cents } else { 0.0 }),
                }
            })
            .collect();
        note(onset, offset, key).with_contour(Contour::new(points).unwrap())
    }

    #[test]
    fn only_unmodelled_motion_of_a_mono_line_writes_bends() {
        let notes = NoteList::new(vec![bent(0.0, 1.0, 50.0, 300.0)]);
        let facts = [knee(0.95)];
        let on = input(&notes, &facts, 120.0);
        let chords = FinishInput { mono: false, ..on };
        assert_eq!(finish(&chords).bends, []);
        let (_, bends) = parse(&finish(&on).to_midi(None).unwrap());
        // The first frame off the key is frame 87 at 0.5050 s of track time,
        // 0.4852 s of audio after the lag of 19.8 ms at 146.8 Hz, tick 466.
        // Three semitones clamp to the top of the 2-semitone range, and the
        // note's end centres the bend again.
        assert_eq!(bends, [(466, 16_383), (912, 8192)]);
        let glides = [GlideSpan {
            start: s(0.0),
            interval: Semitones::new(3.0).unwrap(),
            time: s(1.0),
        }];
        let bends = finish(&FinishInput {
            glides: &glides,
            ..on
        })
        .bends;
        assert_eq!(bends, []);
        let small = NoteList::new(vec![bent(0.0, 1.0, 50.0, 30.0)]);
        let bends = finish(&FinishInput {
            notes: &small,
            ..on
        })
        .bends;
        assert_eq!(bends, []);
    }

    #[test]
    fn the_frames_of_a_note_change_write_no_bend() {
        // The track still reads the note before over its first quarter
        // frame: a contour that starts 2 semitones low there, then holds the
        // key, bends nothing.
        let key = 45.0;
        let points = (0..25)
            .map(|i| {
                let t = 1.0 + 0.01 * f64::from(i);
                let cents = if t < 1.02 { -200.0 } else { 0.0 };
                ContourPoint {
                    time: s(t),
                    f0: Hz::from(MidiPitch::new(key + cents / 100.0).unwrap()),
                }
            })
            .collect();
        let notes = NoteList::new(vec![
            note(0.75, 1.0, 43.0),
            note(1.0, 1.25, key).with_contour(Contour::new(points).unwrap()),
        ]);
        let facts = [kneeless(), kneeless()];
        assert_eq!(finish(&input(&notes, &facts, 120.0)).bends, []);
    }

    /// A 49 Hz note (key 31) whose track jitters within ±35 cents, with a
    /// spike past 50 cents shorter than the tracker's reach every 0.1 s,
    /// then, when `held` is set, a bend of +60 cents from 0.6 to 0.9 s.
    fn sub_line(held: bool) -> Note {
        let reach_frames = cast::floor_to_usize(
            (cast::usize_to_f64(TRACK_FRAME / 4) / MODEL_RATE.hz().get() + 1.0 / 49.0) / hop(),
        )
        .unwrap();
        let n = cast::floor_to_usize(1.0 / hop()).unwrap();
        let points = (0..n)
            .map(|i| {
                let t = 0.1 + hop() * cast::usize_to_f64(i);
                let jitter = 35.0 * (cast::usize_to_f64(i) * 1.7).sin();
                // A spike of half the reach, as at a hit inside a merged note.
                let spike = if i % 17 < reach_frames / 2 {
                    70.0
                } else {
                    jitter
                };
                let cents = if held && (0.6..0.9).contains(&t) {
                    60.0
                } else {
                    spike
                };
                ContourPoint {
                    time: s(t),
                    f0: Hz::from(MidiPitch::new(31.0 + cents / 100.0).unwrap()),
                }
            })
            .collect();
        note(0.1, 1.1, 31.0).with_contour(Contour::new(points).unwrap())
    }

    #[test]
    fn a_steady_sub_line_with_jitter_writes_no_bend() {
        let notes = NoteList::new(vec![sub_line(false)]);
        let facts = [kneeless()];
        assert_eq!(finish(&input(&notes, &facts, 120.0)).bends, []);
    }

    #[test]
    fn a_held_bend_of_60_cents_writes_bends() {
        let notes = NoteList::new(vec![sub_line(true)]);
        let facts = [kneeless()];
        let bends = finish(&input(&notes, &facts, 120.0)).bends;
        // +60 cents of a 2-semitone range: 8192 + 0.3 * 8191.
        let at = |t: f64| in_force_at(&bends, tick_of(t, 960.0)).1;
        assert_eq!(at(0.75), 10_649);
        assert_eq!(at(1.2), BEND_CENTRE);
    }

    #[test]
    fn a_note_inside_another_reads_no_bend_from_the_shared_track() {
        // An octave ghost inside a held note: the one-voice track reads the
        // held note, 12 semitones under the ghost, so neither note bends.
        let held = contoured(0.5, 1.5, 47, 0.0);
        let points = (0..6)
            .map(|i| ContourPoint {
                time: s(0.9 + 0.01 * f64::from(i)),
                f0: Hz::from(MidiPitch::new(47.0).unwrap()),
            })
            .collect();
        let ghost = note(0.9, 0.96, 59.0).with_contour(Contour::new(points).unwrap());
        let notes = NoteList::new(vec![held, ghost]);
        let facts = [kneeless(), kneeless()];
        let finished = finish(&FinishInput {
            mono: true,
            ..input(&notes, &facts, 120.0)
        });
        assert_eq!(finished.bends, []);
    }

    #[test]
    fn a_glide_after_the_later_onset_keeps_that_onset() {
        // The later note starts at 1.0 s and the glide at 1.04 s: the note
        // keeps 1.0 s, 960 ticks, and the earlier one ends 5 ticks later.
        let notes = NoteList::new(vec![note(0.5, 1.0, 45.0), note(1.0, 1.08, 50.0)]);
        let glides = [GlideSpan {
            start: s(1.04),
            interval: Semitones::new(5.0).unwrap(),
            time: s(0.03),
        }];
        let facts = [kneeless(), kneeless()];
        let finished = finish(&FinishInput {
            glides: &glides,
            ..input(&notes, &facts, 120.0)
        });
        let spans: Vec<_> = finished.notes.iter().map(|n| (n.start, n.end)).collect();
        assert_eq!(spans, [(480, 965), (960, 1016)]);
    }

    #[test]
    fn of_two_notes_on_one_tick_a_mono_line_keeps_the_longer() {
        let notes = NoteList::new(vec![note(1.0, 1.5, 40.0), note(1.0, 1.4, 52.0)]);
        // Levels do not rank twins: the louder of the facts still goes.
        let loud = |db| NoteFacts {
            level: Some(Decibels::new(db).unwrap()),
            ..kneeless()
        };
        let facts = [loud(-20.0), loud(-6.0)];
        let finished = finish(&input(&notes, &facts, 120.0));
        assert_eq!(finished.dropped, 1);
        assert_eq!(finished.notes.len(), 1);
        assert_eq!(finished.notes[0].key, 40);
        // Equal lengths keep the lower key.
        let notes = NoteList::new(vec![note(1.0, 1.5, 52.0), note(1.0, 1.5, 40.0)]);
        assert_eq!(finish(&input(&notes, &facts, 120.0)).notes[0].key, 40);
    }

    #[test]
    fn of_two_notes_on_one_tick_the_one_without_an_octave_flag_stays() {
        let notes = NoteList::new(vec![
            note(1.0, 1.5, 40.0).with_octave_uncertain(true),
            note(1.0, 1.4, 52.0).with_octave_uncertain(false),
        ]);
        let facts = [kneeless(), kneeless()];
        let finished = finish(&input(&notes, &facts, 120.0));
        assert_eq!(finished.notes.len(), 1);
        assert_eq!(finished.notes[0].key, 52);
    }

    #[test]
    fn a_missing_level_of_a_mono_line_reads_no_further_than_the_next_onset() {
        // A short quiet note without a level, 5 ms before a loud one: its
        // written span runs to the minimum, past the next onset, but its
        // level stops at that onset.
        let notes = NoteList::new(vec![note(2.0, 2.04, 50.0), note(2.005, 2.8, 52.0)]);
        let facts = [
            kneeless(),
            NoteFacts {
                level: Decibels::new(-6.0).ok(),
                ..kneeless()
            },
        ];
        let next = tick_of(2.005, ticks_per_second(bpm(120.0)));
        let reader = |from: Seconds, to: Seconds, _| {
            assert_eq!(from, s(2.0));
            assert!(
                to <= seconds_of(next, ticks_per_second(bpm(120.0))),
                "{to:?}"
            );
            Decibels::new(-24.0).ok()
        };
        let finished = finish(&FinishInput {
            level: &reader,
            level_velocity: true,
            ..input(&notes, &facts, 120.0)
        });
        // 127 * 10^(-18 / 40) = 45.0.
        let velocities: Vec<_> = finished.notes.iter().map(|n| n.velocity.get()).collect();
        assert_eq!(velocities, [45, 127]);
        // On a polyphonic line the span is the written one.
        let reader = |_, to: Seconds, _| {
            assert!(to > s(2.03), "{to:?}");
            Decibels::new(-24.0).ok()
        };
        let finished = finish(&FinishInput {
            level: &reader,
            mono: false,
            ..input(&notes, &facts, 120.0)
        });
        assert_eq!(finished.notes.len(), 2);
    }

    #[test]
    fn a_dropped_twin_still_keeps_its_span_from_the_kept_note_s_bends() {
        // The track follows the dropped twin at 52 for the kept note's whole
        // length; the twin still sounds, so those frames bend nothing.
        let n = cast::floor_to_usize(0.4 / hop()).unwrap();
        let points = (0..n)
            .map(|i| ContourPoint {
                time: s(1.0 + hop() * cast::usize_to_f64(i)),
                f0: Hz::from(MidiPitch::new(52.0).unwrap()),
            })
            .collect();
        let kept = note(1.0, 1.4, 40.0).with_contour(Contour::new(points).unwrap());
        let ghost = note(1.0, 1.5, 52.0).with_octave_uncertain(true);
        let notes = NoteList::new(vec![kept, ghost]);
        let facts = [kneeless(), kneeless()];
        let finished = finish(&input(&notes, &facts, 120.0));
        assert_eq!((finished.notes.len(), finished.notes[0].key), (1, 40));
        assert_eq!(finished.bends, []);
    }

    #[test]
    fn a_merged_drop_takes_its_read_flag_with_its_knee() {
        // The later note ends the merged one and the envelope skipped it, so
        // the merged note is unread, not without a knee.
        let notes = NoteList::new(vec![note(1.0, 1.04, 43.0), note(1.03, 1.6, 42.0)]);
        let facts = [kneeless(), NoteFacts::default()];
        let f_end = Hz::from(MidiPitch::new(41.0).unwrap());
        let drops = [DropSpan {
            onset: s(1.0),
            f_end,
            df_hz: f_end.get() * (2f64.powf(0.25) - 1.0),
            settled: s(2.5),
        }];
        let finished = finish(&FinishInput {
            drops: &drops,
            ..input(&notes, &facts, 120.0)
        });
        let rules: Vec<_> = finished.notes.iter().map(|n| n.end_rule).collect();
        assert_eq!(rules, [EndRule::Unread]);
        // And the other way round.
        let facts = [NoteFacts::default(), kneeless()];
        let finished = finish(&FinishInput {
            drops: &drops,
            ..input(&notes, &facts, 120.0)
        });
        let rules: Vec<_> = finished.notes.iter().map(|n| n.end_rule).collect();
        assert_eq!(rules, [EndRule::NoKnee]);
    }

    #[test]
    fn a_note_no_rule_read_is_unread_and_a_missing_level_is_filled() {
        let notes = NoteList::new(vec![note(0.0, 0.5, 40.0), note(1.0, 1.5, 40.0)]);
        let facts = [
            NoteFacts {
                level: Some(Decibels::new(-6.0).unwrap()),
                ..NoteFacts::default()
            },
            kneeless(),
        ];
        let finished = finish(&FinishInput {
            level_velocity: true,
            ..input(&notes, &facts, 120.0)
        });
        let rules: Vec<_> = finished.notes.iter().map(|n| n.end_rule).collect();
        assert_eq!(rules, [EndRule::Unread, EndRule::NoKnee]);
        assert_eq!(finished.notes[1].velocity, DEFAULT_VELOCITY);
        // The written span of the second note fills its level.
        let written = |from: Seconds, to: Seconds, _| {
            assert_eq!((from, to), (s(1.0), s(1.45)));
            Decibels::new(-12.0).ok()
        };
        let finished = finish(&FinishInput {
            level_velocity: true,
            level: &written,
            ..input(&notes, &facts, 120.0)
        });
        // 127 * 10^(-6 / 40) = 89.9.
        let velocities: Vec<_> = finished.notes.iter().map(|n| n.velocity.get()).collect();
        assert_eq!(velocities, [127, 90]);
    }

    /// The last bend at or before a tick, which the file plays before a
    /// note-on of that tick, and its tick.
    fn in_force_at(bends: &[(u64, u16)], at: u64) -> (u64, u16) {
        bends
            .iter()
            .rev()
            .find(|&&(t, _)| t <= at)
            .map_or((0, BEND_CENTRE), |&b| b)
    }

    /// One note of a generated mono line: the gap to the next onset, the
    /// length, whether it overlaps the next note in the list, the key, the
    /// knee as a share of its length, whether the envelope read it, its
    /// level in dB, its contour's offset in cents, a drop on it, and a glide
    /// into the next note with its start against that onset.
    #[derive(Clone, Debug)]
    struct Spec {
        gap: f64,
        length: f64,
        overlaps: bool,
        key: u8,
        knee: Option<f64>,
        read: bool,
        level: Option<f64>,
        cents: f64,
        drop: bool,
        glide: Option<f64>,
    }

    fn spec() -> impl Strategy<Value = Spec> {
        (
            (
                0.2f64..1.0,
                0.03f64..0.8,
                prop::bool::weighted(0.2),
                30u8..90,
            ),
            (
                prop::option::of(0.0f64..1.0),
                prop::bool::weighted(0.8),
                prop::option::of(-30.0f64..0.0),
            ),
            (
                prop::sample::select(vec![0.0, 30.0, -30.0, 80.0, -150.0]),
                prop::bool::weighted(0.1),
                prop::option::weighted(0.3, -0.04f64..0.04),
            ),
        )
            .prop_map(
                |((gap, length, overlaps, key), (knee, read, level), (cents, drop, glide))| Spec {
                    gap,
                    length,
                    overlaps,
                    key,
                    knee: knee.filter(|_| read),
                    read,
                    level,
                    cents,
                    drop,
                    glide,
                },
            )
    }

    /// A contour on the track's frames over the note, `cents` off its key.
    fn contoured(onset: f64, offset: f64, key: u8, cents: f64) -> Note {
        let pitch = f64::from(key);
        let n = cast::floor_to_usize((offset - onset) / hop())
            .unwrap_or(0)
            .max(1);
        let points = (0..n)
            .map(|i| ContourPoint {
                time: s(onset + hop() * cast::usize_to_f64(i)),
                f0: Hz::from(MidiPitch::new(pitch + cents / 100.0).unwrap()),
            })
            .collect();
        note(onset, offset, pitch).with_contour(Contour::new(points).unwrap())
    }

    proptest! {
        #[test]
        fn a_mono_line_keeps_its_rules(
            specs in prop::collection::vec(spec(), 2..12),
            tempo in 4.0f64..1000.0,
            level in proptest::bool::ANY,
        ) {
            let mut onsets = Vec::new();
            let mut onset = 0.1;
            for spec in &specs {
                onsets.push(onset);
                onset += spec.length.max(spec.gap) + 0.01;
            }
            let ends: Vec<f64> = specs
                .iter()
                .enumerate()
                .map(|(i, spec)| match onsets.get(i + 1) {
                    Some(&next) if spec.overlaps => next + 0.05,
                    _ => onsets[i] + spec.length,
                })
                .collect();
            let notes = NoteList::new(
                specs
                    .iter()
                    .enumerate()
                    .map(|(i, spec)| contoured(onsets[i], ends[i], spec.key, spec.cents))
                    .collect(),
            );
            let facts: Vec<NoteFacts> = specs
                .iter()
                .enumerate()
                .map(|(i, spec)| NoteFacts {
                    knee: spec.knee.map(|k| s(onsets[i] + k * (ends[i] - onsets[i]))),
                    pad_end: None,
                    level: spec.level.map(|l| Decibels::new(l).unwrap()),
                    read: spec.read,
                })
                .collect();
            let drops: Vec<DropSpan> = specs
                .iter()
                .enumerate()
                .filter(|(_, spec)| spec.drop)
                .map(|(i, spec)| {
                    // From a semitone above the key down to two below it.
                    let f_end = Hz::from(MidiPitch::new(f64::from(spec.key) - 2.0).unwrap());
                    DropSpan {
                        onset: s(onsets[i]),
                        f_end,
                        df_hz: f_end.get() * (2f64.powf(0.25) - 1.0),
                        settled: s(onsets[i] + 0.1),
                    }
                })
                .collect();
            let joins: Vec<usize> = (0..specs.len() - 1)
                .filter(|&i| {
                    specs[i].glide.is_some()
                        && !specs[i].drop
                        && !specs[i + 1].drop
                        && specs[i].key != specs[i + 1].key
                })
                .collect();
            let glides: Vec<GlideSpan> = joins
                .iter()
                .map(|&i| GlideSpan {
                    start: s(onsets[i + 1] + specs[i].glide.unwrap_or(0.0)),
                    interval: Semitones::new(
                        f64::from(specs[i + 1].key) - f64::from(specs[i].key) + 0.3,
                    )
                    .unwrap(),
                    time: s(0.08),
                })
                .collect();
            let finished = finish(&FinishInput {
                glides: &glides,
                drops: &drops,
                level_velocity: level,
                ..input(&notes, &facts, tempo)
            });
            let tps = ticks_per_second(bpm(tempo));
            let tick = 1.0 / tps;
            let minimum = min_note_length(bpm(tempo)).get();
            let list = &finished.notes;
            prop_assert_eq!(list.len(), specs.len());
            prop_assert_eq!(finished.dropped, 0);
            for (i, (n, spec)) in list.iter().zip(&specs).enumerate() {
                prop_assert!(n.end > n.start);
                prop_assert!((1..=127).contains(&n.velocity.get()));
                if !level || n.level.is_none() {
                    prop_assert_eq!(n.velocity, DEFAULT_VELOCITY);
                }
                prop_assert_eq!(n.key, if spec.drop { spec.key - 2 } else { spec.key });
                let glided = i.checked_sub(1).is_some_and(|p| list[p].end_rule == EndRule::Glide);
                let joined = n.end_rule == EndRule::Glide || glided;
                let own_start = tick_of(onsets[i], tps);
                if glided {
                    // The later note starts at the glide's start or its own
                    // onset, whichever comes first.
                    let at = tick_of(onsets[i] + specs[i - 1].glide.unwrap_or(0.0), tps);
                    prop_assert_eq!(n.start, own_start.min(at));
                } else {
                    prop_assert_eq!(n.start, own_start);
                }
                match n.end_rule {
                    EndRule::Glide => {
                        let next = &list[i + 1];
                        let overlap =
                            finished.seconds(n.end).get() - finished.seconds(next.start).get();
                        prop_assert!(overlap >= GLIDE_OVERLAP.get() - 1e-9, "{overlap}");
                    }
                    rule => {
                        prop_assert!(n.end <= tick_of(ends[i], tps));
                        if let Some(next) = list.get(i + 1) {
                            prop_assert!(n.end <= next.start);
                        }
                        let expected = if spec.knee.is_some() {
                            EndRule::Knee
                        } else if spec.read {
                            EndRule::NoKnee
                        } else {
                            EndRule::Unread
                        };
                        prop_assert!(
                            [expected, EndRule::Minimum, EndRule::NextNote].contains(&rule),
                            "{rule:?} for {spec:?}"
                        );
                    }
                }
                if !joined && n.end_rule != EndRule::NextNote {
                    let written = finished.seconds(n.end).get() - finished.seconds(n.start).get();
                    if ends[i] - onsets[i] >= minimum {
                        prop_assert!(written >= minimum - tick - 1e-9);
                    } else {
                        // A note the list gives shorter keeps its length.
                        prop_assert_eq!(n.end, tick_of(ends[i], tps).max(n.start + 1));
                    }
                }
                if n.end_rule == EndRule::NextNote {
                    // Only a list note that runs into the next is cut there.
                    prop_assert!(spec.overlaps);
                    prop_assert_eq!(n.end, list[i + 1].start);
                }
            }
            // Bends: never before a note's note-on, centred again before the
            // next, within range, and present for an unmodelled long offset.
            for (i, n) in list.iter().enumerate() {
                let spec = &specs[i];
                // At a note-on the bend is centred, unless it already holds
                // the note's own bend value, as when the note before bent
                // by as much.
                let (t, v) = in_force_at(&finished.bends, n.start);
                let own = cast::round_to_usize(
                    f64::from(BEND_CENTRE) + (spec.cents / 100.0).clamp(-2.0, 2.0) / 2.0 * 8191.0,
                )
                .and_then(|v| u16::try_from(v).ok());
                prop_assert!(
                    v == BEND_CENTRE || (spec.cents.abs() >= 50.0 && Some(v) == own),
                    "note {i}: {v} at {t}"
                );
                // A glide into the note models its start, which can leave
                // less than the tracker's reach before its end.
                let glided = i.checked_sub(1).is_some_and(|p| list[p].end_rule == EndRule::Glide);
                let clear = !spec.drop
                    && !glided
                    && spec.cents.abs() >= 80.0
                    && ends[i] - onsets[i] >= 0.2
                    && finished.seconds(n.end).get() - onsets[i] >= 0.15;
                if clear {
                    // A bend of the note's own, or one already in force at
                    // its note-on with the same value.
                    let bent = in_force_at(&finished.bends, n.start).1 != BEND_CENTRE
                        || finished
                            .bends
                            .iter()
                            .any(|&(t, v)| (n.start..n.end).contains(&t) && v != BEND_CENTRE);
                    prop_assert!(bent, "note {i}: {spec:?}");
                }
                if spec.cents.abs() < 50.0 || spec.drop {
                    let stop = list.get(i + 1).map_or(n.end, |m| n.end.min(m.start));
                    prop_assert!(
                        finished
                            .bends
                            .iter()
                            .all(|&(t, v)| !(n.start..stop).contains(&t) || v == BEND_CENTRE),
                        "note {i}: {spec:?}"
                    );
                }
            }
            prop_assert!(finished.bends.iter().all(|&(_, v)| v <= 16_383));
            prop_assert!(finished.bends.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 != w[1].1));
            let (parsed, bends) = parse(&finished.to_midi(None).unwrap());
            prop_assert_eq!(&bends, &finished.bends);
            let mut written: Vec<_> =
                list.iter().map(|n| (n.start, n.end, n.key, n.velocity.get())).collect();
            written.sort_unstable();
            prop_assert_eq!(parsed, written);
        }

        #[test]
        fn a_polyphonic_line_keeps_its_rules(
            raw in prop::collection::vec(
                (
                    0.0f64..3.0,
                    0.03f64..1.5,
                    30u8..90,
                    prop::option::of(0.0f64..1.2),
                    prop::option::of(0.0f64..1.2),
                    prop::bool::weighted(0.7),
                ),
                1..12,
            ),
            tempo in 4.0f64..1000.0,
        ) {
            let notes = NoteList::new(
                raw.iter()
                    .map(|&(on, length, key, ..)| contoured(on, on + length, key, 120.0))
                    .collect(),
            );
            // The list sorts by onset, then pitch: read the facts in its order.
            let facts: Vec<NoteFacts> = notes
                .notes()
                .iter()
                .map(|n| {
                    let &(on, length, _, knee, pad_end, read) = raw
                        .iter()
                        .find(|r| s(r.0) == n.onset() && key_of(n.pitch().get()) == r.2)
                        .unwrap();
                    NoteFacts {
                        knee: knee.filter(|_| read).map(|k| s(on + k * length)),
                        pad_end: pad_end.map(|p| s(on + p * length)),
                        level: None,
                        read,
                    }
                })
                .collect();
            let finished = finish(&FinishInput {
                mono: false,
                ..input(&notes, &facts, tempo)
            });
            let tps = ticks_per_second(bpm(tempo));
            let minimum = min_note_length(bpm(tempo)).get();
            let list = &finished.notes;
            prop_assert_eq!(list.len(), notes.notes().len());
            prop_assert_eq!(&finished.bends, &Vec::new());
            for n in list {
                let (i, listed) = notes
                    .notes()
                    .iter()
                    .enumerate()
                    .find(|(_, l)| l.onset() == n.list_onset && key_of(l.pitch().get()) == n.key)
                    .unwrap();
                let f = facts[i];
                prop_assert!(n.end > n.start);
                prop_assert_eq!(n.velocity, DEFAULT_VELOCITY);
                let at_list_end = tick_of(listed.offset().get(), tps).max(n.start + 1);
                prop_assert!(n.end <= at_list_end);
                if n.end_rule == EndRule::Unread {
                    let cut = listed.offset().get() - NO_KNEE_CUT.get();
                    prop_assert_eq!(n.end, tick_of(cut, tps).max(n.start + 1));
                }
                let expected = if f.pad_end.is_some() {
                    EndRule::Pad
                } else if f.knee.is_some() {
                    EndRule::Knee
                } else if f.read {
                    EndRule::NoKnee
                } else {
                    EndRule::Unread
                };
                prop_assert!([expected, EndRule::Minimum].contains(&n.end_rule), "{:?}", n.end_rule);
                let length = listed.offset().get() - listed.onset().get();
                if length < minimum {
                    // A note the list gives shorter keeps its length.
                    prop_assert_eq!(n.end, at_list_end);
                } else {
                    let written = finished.seconds(n.end).get() - finished.seconds(n.start).get();
                    prop_assert!(written >= minimum - 1.0 / tps - 1e-9);
                }
            }
        }
    }
}
