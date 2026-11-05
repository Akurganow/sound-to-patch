//! The final note list: the list rewritten with the layer report, in the
//! ticks of `notes.mid`.
//!
//! [`finish`] reads what the report says of each note and of the line. It
//! merges each pitch drop into one note at its settled pitch, gives a note
//! under a vibrato the key of the vibrato's mean pitch, ends each note at
//! its note-off knee, and writes each glide as two overlapping notes. The
//! velocity follows the full-band level of each note when the level spread
//! gives velocity a path to the level, and is one constant otherwise.

use super::write::{Span, encode_spans, ticks_per_second};
use super::{Contour, DEFAULT_VELOCITY, MidiWriteError, NoteList, Velocity, min_note_length};
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
/// With bends on, a note gets bends when its contour leaves its key by
/// this much outside every route's span.
pub const BEND_MIN: Semitones = Semitones::from_const(0.5);
/// The bend range the file assumes; a host must set the synth to it.
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
    /// Where a pad note's level falls [`PAD_FALL`] below its plateau;
    /// `None` when the level does not, or the note is no pad.
    pub pad_end: Option<Seconds>,
    /// The full-band level of the note.
    pub level: Option<Decibels>,
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

/// Whether the file holds pitch bends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Bends {
    /// No bend: the preset carries the glides, drops and vibrato as routes.
    #[default]
    Off,
    /// Bends for the motion of a mono line that no route models, within
    /// [`BEND_RANGE`].
    Unmodelled,
}

/// What [`finish`] reads.
#[derive(Clone, Copy, Debug)]
pub struct FinishInput<'a> {
    /// The note list.
    pub notes: &'a NoteList,
    /// One record per note of the list, in its order; a missing one reads
    /// as no knee, no pad end and no level.
    pub facts: &'a [NoteFacts],
    /// The tempo of the file.
    pub bpm: Bpm,
    /// One voice at a time: notes overlap only at glide joins.
    pub mono: bool,
    /// The line is a pad: a note ends at its pad end where it has one.
    pub pad: bool,
    /// Velocity has a path to the level, so it follows each note's level.
    pub level_velocity: bool,
    /// The glides, in time order.
    pub glides: &'a [GlideSpan],
    /// The pitch drops.
    pub drops: &'a [DropSpan],
    /// The vibratos.
    pub vibrato: &'a [VibratoSpan],
    /// Whether to write bends.
    pub bends: Bends,
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
}

impl FinalNotes {
    /// The time of a tick.
    #[must_use]
    pub fn seconds(&self, tick: u64) -> Seconds {
        Seconds::clamped(cast::u64_to_f64(tick) / ticks_per_second(self.bpm))
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
/// 3. Each note ends at its pad end on a pad, else at its knee, else
///    [`NO_KNEE_CUT`] before its end in the list, never after that end. A
///    cut never makes a note shorter than [`min_note_length`]; a note the
///    list gives shorter keeps its length.
/// 4. Times become ticks with `round(seconds * BPM / 60 * 480)`, and each
///    note lasts at least one tick.
/// 5. In a mono line, a glide joins the pair of neighbours whose interval
///    lies within a semitone of its own and whose later onset lies within
///    [`GLIDE_SLACK`] of its motion, the nearest to its start. The later
///    note starts at the glide's start, and the earlier one ends
///    [`GLIDE_OVERLAP`] later, rounded up to whole ticks, whatever its knee
///    says. Every other note of a mono line ends no later than the next
///    note starts, and one that starts on the same tick as the next is
///    dropped.
/// 6. With level velocity, the loudest note gets 127 and a note `d` dB
///    under it `round(127 * 10^(-d / 40))`, at least 1; a note without a
///    level, and every note otherwise, gets [`DEFAULT_VELOCITY`].
/// 7. With [`Bends::Unmodelled`] on a mono line, a note whose contour
///    leaves its key by [`BEND_MIN`] or more outside every glide, drop and
///    vibrato span gets a bend at each contour point outside those spans,
///    clamped to [`BEND_RANGE`], and a centred bend at its end.
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
            let (mut end, mut rule) = match (input.pad, w.facts.pad_end, w.facts.knee) {
                (true, Some(pad), _) => (pad.get(), EndRule::Pad),
                (_, _, Some(knee)) => (knee.get(), EndRule::Knee),
                _ => (w.list_end - NO_KNEE_CUT.get(), EndRule::NoKnee),
            };
            end = end.min(w.list_end);
            if end < w.onset + minimum {
                end = (w.onset + minimum).min(w.list_end);
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
    if input.mono {
        join_mono(&mut notes, input.glides, tps);
    }
    if input.level_velocity {
        level_velocities(&mut notes);
    }
    let bends = if input.mono && input.bends == Bends::Unmodelled {
        bends_of(&notes, input, tps)
    } else {
        Vec::new()
    };
    FinalNotes {
        notes: notes.into_iter().map(|(n, _)| n).collect(),
        bends,
        bpm: input.bpm,
        level_velocity: input.level_velocity,
    }
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

/// Writes the glide joins of a mono line and ends every other note where
/// the next starts.
fn join_mono(notes: &mut Vec<(FinalNote, Option<&Contour>)>, glides: &[GlideSpan], tps: f64) {
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
        let start = tick(glide.start.get());
        let best = (0..notes.len().saturating_sub(1))
            .filter(|&i| !joined[i])
            .filter(|&i| {
                let (a, b) = (&notes[i].0, &notes[i + 1].0);
                let onset = b.list_onset.get();
                let interval = f64::from(b.key) - f64::from(a.key);
                (from..=to).contains(&onset)
                    && (interval - glide.interval.get()).abs() < 1.0
                    && a.start < start
            })
            .min_by(|&i, &j| {
                let gap = |k: usize| (notes[k + 1].0.list_onset.get() - glide.start.get()).abs();
                gap(i).total_cmp(&gap(j))
            });
        if let Some(i) = best {
            let b = &mut notes[i + 1].0;
            b.start = start;
            b.end = b.end.max(start + 1);
            let a = &mut notes[i].0;
            a.end = start + overlap;
            a.end_rule = EndRule::Glide;
            joined[i] = true;
        }
    }
    let mut keep = vec![true; notes.len()];
    for i in 0..notes.len().saturating_sub(1) {
        if joined[i] {
            continue;
        }
        let next = notes[i + 1].0.start;
        let a = &mut notes[i].0;
        if next <= a.start {
            keep[i] = false;
        } else if a.end > next {
            a.end = next;
            a.end_rule = EndRule::NextNote;
        }
    }
    let mut flags = keep.into_iter();
    notes.retain(|_| flags.next().unwrap_or(true));
}

/// Sets each velocity from the note's level under the loudest.
fn level_velocities(notes: &mut [(FinalNote, Option<&Contour>)]) {
    let Some(loudest) = notes
        .iter()
        .filter_map(|(n, _)| n.level.map(Decibels::get))
        .reduce(f64::max)
    else {
        return;
    };
    for (note, _) in notes {
        if let Some(level) = note.level {
            let value = 127.0 * 10f64.powf((level.get() - loudest) / VELOCITY_LAW_DB);
            note.velocity = cast::round_to_usize(value.clamp(1.0, 127.0))
                .and_then(|v| u8::try_from(v).ok())
                .and_then(|v| Velocity::new(v).ok())
                .unwrap_or(DEFAULT_VELOCITY);
        }
    }
}

/// The bends of the motion no route models.
fn bends_of(
    notes: &[(FinalNote, Option<&Contour>)],
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
    let mut bends: Vec<(u64, u16)> = Vec::new();
    for (note, contour) in notes {
        let Some(contour) = contour else { continue };
        let points: Vec<(u64, f64)> = contour
            .points()
            .iter()
            .filter(|p| !modelled(p.time.get()))
            .filter_map(|p| {
                let at = tick_of(p.time.get(), tps);
                let deviation = pitch_of(p.f0)? - f64::from(note.key);
                (note.start..note.end)
                    .contains(&at)
                    .then_some((at, deviation))
            })
            .collect();
        if !points.iter().any(|&(_, d)| d.abs() >= BEND_MIN.get()) {
            continue;
        }
        for (at, deviation) in points {
            let share = deviation.clamp(-BEND_RANGE.get(), BEND_RANGE.get()) / BEND_RANGE.get();
            let value = cast::round_to_usize((f64::from(BEND_CENTRE) + share * 8191.0).max(0.0))
                .and_then(|v| u16::try_from(v).ok())
                .unwrap_or(BEND_CENTRE)
                .min(16_383);
            push_bend(&mut bends, at, value);
        }
        push_bend(&mut bends, note.end, BEND_CENTRE);
    }
    bends
}

/// Adds a bend; a later one at the same tick replaces it, and a repeat of
/// the value in force is left out.
fn push_bend(bends: &mut Vec<(u64, u16)>, at: u64, value: u16) {
    if let Some(last) = bends.last_mut()
        && last.0 == at
    {
        last.1 = value;
        return;
    }
    // A file starts centred, so a centred bend first changes nothing.
    if bends.last().map_or(BEND_CENTRE, |&(_, v)| v) == value {
        return;
    }
    bends.push((at, value));
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
            pad: false,
            level_velocity: false,
            glides: &[],
            drops: &[],
            vibrato: &[],
            bends: Bends::Off,
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
        let facts = [knee(0.9), knee(1.5), NoteFacts::default()];
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
            pad: true,
            ..input(&notes, &facts, 120.0)
        });
        let ends: Vec<_> = finished.notes.iter().map(|n| (n.end, n.end_rule)).collect();
        assert_eq!(ends, [(1440, EndRule::Pad), (960, EndRule::Knee)]);
    }

    fn bent(onset: f64, offset: f64, key: f64, cents: f64) -> Note {
        let f0 = |c: f64| Hz::from(MidiPitch::new(key + c / 100.0).unwrap());
        let points = (0..10)
            .map(|i| {
                let t = onset + (offset - onset) * f64::from(i) / 10.0;
                ContourPoint {
                    time: s(t),
                    f0: f0(if i >= 5 { cents } else { 0.0 }),
                }
            })
            .collect();
        note(onset, offset, key).with_contour(Contour::new(points).unwrap())
    }

    #[test]
    fn only_unmodelled_motion_writes_bends_and_only_when_asked() {
        let notes = NoteList::new(vec![bent(0.0, 1.0, 50.0, 300.0)]);
        let facts = [knee(0.95)];
        let on = FinishInput {
            bends: Bends::Unmodelled,
            ..input(&notes, &facts, 120.0)
        };
        assert_eq!(finish(&input(&notes, &facts, 120.0)).bends, []);
        let (_, bends) = parse(&finish(&on).to_midi(None).unwrap());
        // Three semitones clamp to the top of the 2-semitone range, and the
        // note's end centres the bend again.
        assert_eq!(bends, [(480, 16_383), (912, 8192)]);
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

    /// A mono line: gaps of at least 0.2 s between onsets, lengths from
    /// the decoder's longest minimum up, a knee or none per note, and
    /// glides at some joins.
    /// Onset, offset, key and knee of each note, the joins and the BPM.
    type Line = (Vec<(f64, f64, u8, Option<f64>)>, Vec<usize>, f64);

    fn line() -> impl Strategy<Value = Line> {
        (
            prop::collection::vec(
                (
                    0.2f64..1.0,
                    0.13f64..0.8,
                    30u8..90,
                    prop::option::of(0.0f64..1.0),
                ),
                2..12,
            ),
            prop::collection::vec(0usize..12, 0..4),
            4.0f64..1000.0,
        )
            .prop_map(|(raw, joins, tempo)| {
                let mut onset = 0.1;
                let notes = raw
                    .into_iter()
                    .map(|(gap, length, key, knee)| {
                        let n = (onset, onset + length, key, knee.map(|k| onset + k * length));
                        onset += length.max(gap) + 0.01;
                        n
                    })
                    .collect();
                (notes, joins, tempo)
            })
    }

    proptest! {
        #[test]
        fn the_final_list_keeps_its_rules((spans, joins, tempo) in line(), level in proptest::bool::ANY) {
            let notes = NoteList::new(
                spans.iter().map(|&(on, off, key, _)| note(on, off, f64::from(key))).collect(),
            );
            let facts: Vec<NoteFacts> = spans
                .iter()
                .enumerate()
                .map(|(i, &(.., k))| NoteFacts {
                    knee: k.map(s),
                    pad_end: None,
                    level: Some(Decibels::new(-3.0 * f64::from(u8::try_from(i).unwrap())).unwrap()),
                })
                .collect();
            let mut joins = joins;
            joins.sort_unstable();
            joins.dedup();
            let glides: Vec<GlideSpan> = joins
                .iter()
                .filter(|&&i| i + 1 < spans.len() && spans[i].2 != spans[i + 1].2)
                .map(|&i| GlideSpan {
                    start: s(spans[i + 1].0 - 0.02),
                    interval: Semitones::new(f64::from(spans[i + 1].2) - f64::from(spans[i].2) + 0.3).unwrap(),
                    time: s(0.08),
                })
                .collect();
            let finished = finish(&FinishInput {
                glides: &glides,
                level_velocity: level,
                ..input(&notes, &facts, tempo)
            });
            let tps = ticks_per_second(bpm(tempo));
            let tick = 1.0 / tps;
            let minimum = min_note_length(bpm(tempo)).get();
            let list = &finished.notes;
            prop_assert_eq!(list.len(), spans.len());
            for (i, n) in list.iter().enumerate() {
                prop_assert!(n.end > n.start);
                prop_assert!((1..=127).contains(&n.velocity.get()));
                if !level {
                    prop_assert_eq!(n.velocity, DEFAULT_VELOCITY);
                }
                let joined = n.end_rule == EndRule::Glide
                    || i.checked_sub(1).is_some_and(|p| list[p].end_rule == EndRule::Glide);
                let next = list.get(i + 1);
                if n.end_rule == EndRule::Glide {
                    let next = next.unwrap();
                    let overlap = finished.seconds(n.end).get() - finished.seconds(next.start).get();
                    prop_assert!(overlap >= GLIDE_OVERLAP.get() - 1e-9, "{overlap}");
                } else {
                    prop_assert!(n.end <= tick_of(n.list_end.get(), tps));
                    if let Some(next) = next {
                        prop_assert!(n.end <= next.start);
                    }
                }
                if !joined {
                    prop_assert!(finished.seconds(n.end).get() - finished.seconds(n.start).get() >= minimum - tick - 1e-9);
                }
            }
            prop_assert_eq!(list.iter().filter(|n| n.end_rule == EndRule::Glide).count(), glides.len());
            for glide in &glides {
                let at = tick_of(glide.start.get(), tps);
                prop_assert!(list.iter().any(|n| n.start.abs_diff(at) <= 1));
            }
            prop_assert!(finished.bends.is_empty());
            let (parsed, bends) = parse(&finished.to_midi(None).unwrap());
            prop_assert!(bends.is_empty());
            let written: Vec<_> = list.iter().map(|n| (n.start, n.end, n.key, n.velocity.get())).collect();
            let mut sorted = written.clone();
            sorted.sort_unstable();
            prop_assert_eq!(parsed, sorted);
            for (i, (n, spec)) in list.iter().zip(&spans).enumerate() {
                prop_assert_eq!(n.key, spec.2);
                let glided = i.checked_sub(1).is_some_and(|p| list[p].end_rule == EndRule::Glide);
                if !glided {
                    prop_assert!((finished.seconds(n.start).get() - spec.0).abs() <= tick);
                }
            }
        }
    }
}
