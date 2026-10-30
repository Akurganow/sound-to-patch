//! The cleanup of decoded notes: the octave check against the f0 track,
//! the merge of octave ghosts into their notes, the split of notes at
//! attacks inside them, and the join of split notes.
//!
//! The decoder adds a short copy an octave above a low note, and starts a
//! second note at the same key when the onset activation peaks again
//! inside a held note. A note an octave above the f0 track's reading, over
//! a note of the lower key, is such a copy: it moves down, overlaps its
//! true note and merges into it. The track alone moves nothing, because
//! it can read a true note an octave low. A note that starts where an earlier
//! one of the same key ends, on a weak onset and with no attack of its own
//! in the stem, is the rest of that note. A note that holds an attack of
//! its own in the stem holds a repeat the decoder missed, and splits there.

use super::{MODEL_RATE, ModelRun, Note, NoteList, PitchTrack, QUIET_FRAME, StemLevels};
use crate::cast;
use crate::units::Hz;
use crate::units::{Decibels, MidiPitch, Seconds, Semitones};

/// How far a note may lie from the f0 track's median and still agree.
pub const OCTAVE_AGREEMENT: Semitones = Semitones::from_const(1.0);
/// How close to an octave a gap must lie to count as one.
pub const OCTAVE_TOLERANCE: Semitones = Semitones::from_const(0.5);
/// The f0 median, A2 (110 Hz), at or below which, within
/// [`OCTAVE_TOLERANCE`], a note an octave above it moves down. The ghosts
/// are measured there; above it a gap of an octave only sets the flag.
pub const OCTAVE_FIX_CEILING: MidiPitch = MidiPitch::from_const(45.0);
/// How far a note may reach past the note one octave below it and still
/// lie inside it: two model frames.
///
/// The decoder's times sit up to 12 ms off its frame grid. A note moves
/// down only inside such a note,
/// because the track can read the lower octave of a true note, and a late
/// end of the decoder makes a true note's neighbour overlap it.
pub const COPY_SLACK: Seconds = Seconds::from_const(2.0 * 256.0 / 22_050.0);
/// The fewest voiced frames, 17 ms of track, that check a note.
pub const OCTAVE_MIN_FRAMES: usize = 3;
/// The onset activation below which a note that starts where a note of
/// the same key ends joins it.
///
/// On the generated set every piece of a held note starts at 0.66 or
/// less. The weakest repeated note, a sixteenth at 175 BPM, starts at 0.70.
pub const SPLIT_ONSET: f64 = 0.68;
/// The widest gap a split may leave: two model frames.
pub const SPLIT_GAP: Seconds = Seconds::from_const(2.0 * 256.0 / 22_050.0);
/// The lowest key whose notes [`split_at_attacks`] splits: A1, 55 Hz.
///
/// Below A1 the envelope window of one period lasts 19 ms or more, as long
/// as the recovery of a fast sidechain duck. On the generated sets ducks to
/// -40 dB that recover in 30 to 45 ms split on every key from D1 to G#1,
/// and a duck at E1 reads the same envelope and onset activation as a gate
/// with a 30 ms gap and a 5 ms tail. Full-depth sine tremolos of 10 Hz
/// split at A0 and B0. From A1 up no duck to -40 or -48 dB that recovers
/// in 25 to 45 ms splits.
pub const SPLIT_LOWEST: MidiPitch = MidiPitch::from_const(33.0);
/// How far before a note's onset the quiet frames of its attack lie:
/// frames centred from 75 ms before the onset to the onset, and not
/// before the earlier note starts.
///
/// The decoder starts a 55 Hz note up to 37 ms after its attack, so the
/// gap before the attack lies further back.
pub const ATTACK_BEFORE: Seconds = Seconds::from_const(0.075);
/// How far after a note's onset its loudest frame may lie: frames centred
/// from the onset to 50 ms after it, and not past its end. The attack
/// split's piece after a cut lasts at least this long.
pub const ATTACK_AFTER: Seconds = Seconds::from_const(0.05);
/// How far the stem's level, with any DC offset removed, must dip below
/// the loudest frame in the quiet frames of an attack.
///
/// On the generated sets 17 to 19 dB give the same notes on every stem
/// but square tremolos: at 20 dB eighths on E1 and A1 with a 40 ms tail
/// keep fewer repeats, and at 21 dB a 55 Hz repeat after a 30 ms gap with
/// a 5 ms tail joins.
pub const ATTACK_DIP: Decibels = Decibels::from_const(19.0);
/// How far the level at the note's fundamental alone must dip across a cut
/// of the attack split.
///
/// Below about A2 the harmonics' window cannot tell a note from a key
/// nearby: a held A1 20 dB under a louder gated A2 dips there as the gate
/// closes. Its fundamental does not, and the split cuts it no further than
/// the split join alone. The bin of [`StemLevels::fundamental_level`]
/// rejects the note's harmonics alone, fully only where a period is a
/// whole number of samples. From about A6 up a harmonic leaks within
/// 23 dB at the worst phase.
///
/// On the generated sets 21 to 25 dB give the same notes, and 23 dB lies
/// in the middle. At 19 and 20 dB squares loud for 60 to 80 % of the
/// cycle split a held note further in 72 runs where 23 dB gives 52, and
/// at 30 dB gated runs hit 8 fewer gates.
pub const FUNDAMENTAL_DIP: Decibels = Decibels::from_const(23.0);
/// How far the level at the note's own harmonics must dip in the same
/// frames.
///
/// The tail of a note under the attack of another key dips 18.1 dB or
/// less. On the generated sets 18, 19, 22, 24 and 25 dB differ only on
/// square tremolos and on eighths on A1 with a 40 ms tail: 18 and 19 dB
/// keep 10 of their 16 repeats, 22 dB 6, and 24 and 25 dB 5.
pub const HARMONIC_DIP: Decibels = Decibels::from_const(22.0);
/// The shortest envelope window of the rise test; it spans one period of
/// the note when that is longer.
///
/// One period reads a periodic tone steady. Two periods smooth the dip of
/// a tremolo at A1 away, and a window between the two ripples.
pub const RISE_WINDOW: Seconds = Seconds::from_const(0.004);
/// The step between envelope readings of the rise test.
pub const RISE_STEP: Seconds = Seconds::from_const(0.001);
/// The share of the loudest envelope reading a rise starts below, and the
/// share it ends above.
pub const RISE_SPAN: (f64, f64) = (0.1, 0.9);
/// How far after the onset a rise may start: the decoder starts a note up
/// to 15 ms before its attack.
pub const RISE_LATEST: Seconds = Seconds::from_const(0.02);
/// How many rise limits past [`RISE_LATEST`] the rise test reads.
///
/// A tremolo that rises from a tenth to nine tenths within the limit peaks
/// about 1.7 limits after its trough, and the loudest reading must hold
/// that peak: with readings that end before it, a slow rise to the level
/// at their end reads fast. On the generated sets 2.5 to 4 limits give
/// the same notes.
pub const RISE_READ: f64 = 3.0;
/// How much longer than a step read through the envelope window a rise
/// may take: about the attack of a gate.
///
/// A full-depth tremolo to silence at `r` Hz rises in about 0.3 / `r` s.
/// On the first generated set 6 to 8 ms give the same held notes; at 9 ms
/// a duck to -40 dB on E1 splits, and each ms less keeps fewer repeats
/// with a slow attack. On the second, 8 ms splits a duck to -40 dB on A1.
pub const RISE_SLACK: Seconds = Seconds::from_const(0.007);
/// How long the stem must hold above the first share of [`HOLD_SPAN`]
/// before it last fell below the second share ahead of a rise.
///
/// Below A1 the window of one period is as long as the rise of a tremolo
/// of 14 to 20 Hz, so the rise alone cannot tell the two apart. A gated
/// note holds its level until its gap, and a fast tremolo never holds. On
/// the generated sets 50 to 70 ms keep every repeat and split no sine
/// tremolo; at 45 ms a tremolo of 14 Hz on E1 splits, and at 80 ms
/// sixteenths on E1 with attacks of 19 and 23 ms join.
pub const HOLD_BEFORE: Seconds = Seconds::from_const(0.06);
/// The share of the loudest reading the hold must stay above, and the
/// share at which it ends.
///
/// The level ripples by a few hundredths at the fall of a gate, so a hold
/// that ended at the share it must stay above could end on a ripple.
pub const HOLD_SPAN: (f64, f64) = (0.5, 0.6);
/// How far back from the start of a rise the hold may end.
///
/// A sine tremolo that reads as fast runs faster than about 8 Hz, and at
/// 8 Hz and full depth it stays below six tenths of its peak for about
/// 70 ms. A square tremolo off half its cycle stays low for 167 ms at
/// 3 Hz, and the search must reach its fall for [`HOLD_RATIO`] to weigh
/// it. A longer dip is no tremolo's trough, and the bound keeps the
/// search from reading a whole quiet passage before a weak repeat. On the
/// generated sets 200 to 500 ms give the same notes; at 150 ms an E1
/// under a 3 Hz square off half its cycle splits into 12 notes where the
/// onset rule gives 7.
pub const HOLD_SEARCH: Seconds = Seconds::from_const(0.4);
/// How many times longer than its dip the stem must hold before it.
///
/// A gated note holds longer than its gap, and a tremolo holds about as
/// long as it dips: a square tremolo off half its cycle holds exactly as
/// long. On the generated sets 1.5 splits no square tremolo off half its
/// cycle. At 1.25 a square at 9.1 Hz on C2 splits a held note into 10
/// where the onset rule gives 5. At 2 eighths on E1 with a 10 ms attack
/// and a 10 ms tail over a -40 dB floor keep 3 of 16 repeats in the bass
/// profile, as the onset rule does, where 1.5 keeps 12. The cost of 1.5:
/// a gap longer than about two thirds of the note before it joins, and
/// A1 sixteenths with 44 ms gaps and a 10 ms attack keep 3 of 32 repeats,
/// as the onset rule does, where a hold of 60 ms alone kept 7.
pub const HOLD_RATIO: f64 = 1.5;

/// What the octave check did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OctaveCheck {
    /// Notes moved down an octave.
    pub moved: usize,
    /// Notes that disagree with the track and keep their pitch, flagged.
    pub flagged: usize,
    /// Notes with too few voiced frames to check.
    pub unchecked: usize,
}

/// Checks every note against the median of the voiced f0 frames inside
/// it, and returns the notes with their octave flag set.
///
/// A note within [`OCTAVE_AGREEMENT`] of the median is fine. A note an
/// octave above it, within [`OCTAVE_TOLERANCE`], moves down 12 semitones
/// when the median lies within [`OCTAVE_TOLERANCE`] of
/// [`OCTAVE_FIX_CEILING`] or below, and a note of that lower key with a
/// higher velocity holds it within [`COPY_SLACK`] at each end. Any other gap keeps
/// the pitch and sets the flag. A note with fewer than
/// [`OCTAVE_MIN_FRAMES`] voiced frames keeps its flag unset.
#[must_use]
pub fn check_octaves(notes: &NoteList, track: &PitchTrack) -> (NoteList, OctaveCheck) {
    let mut check = OctaveCheck::default();
    let checked = notes
        .notes()
        .iter()
        .map(|note| {
            let Some(median) = median_pitch(track, note) else {
                check.unchecked += 1;
                return note.clone();
            };
            let gap = note.pitch().get() - median;
            if gap.abs() <= OCTAVE_AGREEMENT.get() {
                return note.clone().with_octave_uncertain(false);
            }
            if (gap - 12.0).abs() <= OCTAVE_TOLERANCE.get()
                && median <= OCTAVE_FIX_CEILING.get() + OCTAVE_TOLERANCE.get()
                && let Some(lower) = lower_octave(notes, note)
                && let Some(moved) = moved_down(note, lower)
            {
                check.moved += 1;
                return moved;
            }
            check.flagged += 1;
            note.clone().with_octave_uncertain(true)
        })
        .collect();
    (NoteList::new(checked), check)
}

/// The note one octave below `note` that holds it, within [`COPY_SLACK`]
/// at each end, with a higher velocity: the model heard the lower note
/// around this one, and more surely. The decoder gives a note an octave
/// twin with nearly its span, so only the velocity, the mean activation,
/// tells the copy from the true note.
fn lower_octave<'a>(notes: &'a NoteList, note: &Note) -> Option<&'a Note> {
    let slack = COPY_SLACK.get();
    notes.notes().iter().find(|other| {
        (note.pitch().get() - other.pitch().get() - 12.0).abs() < 0.5
            && other.onset().get() <= note.onset().get() + slack
            && note.offset().get() <= other.offset().get() + slack
            && note
                .velocity()
                .zip(other.velocity())
                .is_some_and(|(copy, lower)| copy < lower)
    })
}

/// The median pitch of the voiced frames inside a note, if enough are
/// voiced.
fn median_pitch(track: &PitchTrack, note: &Note) -> Option<f64> {
    let mut pitches: Vec<f64> = track
        .voiced_between(note.onset(), note.offset())
        .filter_map(|(_, f0)| MidiPitch::try_from(f0).ok())
        .map(MidiPitch::get)
        .collect();
    if pitches.len() < OCTAVE_MIN_FRAMES {
        return None;
    }
    pitches.sort_by(f64::total_cmp);
    let middle = pitches.len() / 2;
    Some(if pitches.len().is_multiple_of(2) {
        f64::midpoint(pitches[middle - 1], pitches[middle])
    } else {
        pitches[middle]
    })
}

/// The copy moved onto its lower note: that note's key and span, so it
/// merges into that note alone and never bridges two repeats. Its
/// velocity stays and its flag is cleared.
fn moved_down(note: &Note, lower: &Note) -> Option<Note> {
    let mut moved = Note::new(lower.onset(), lower.offset(), lower.pitch())
        .ok()?
        .with_octave_uncertain(false);
    if let Some(velocity) = note.velocity() {
        moved = moved.with_velocity(velocity);
    }
    Some(moved)
}

/// Merges every pair of notes of one key that sound at the same time into
/// one note from the earlier onset to the later offset.
///
/// A ghost moved down by [`check_octaves`] lands on its true note and
/// disappears into it. The decoder itself never gives two notes of one
/// key that overlap, so a repeat it decodes never merges. The merged note
/// keeps the onset, pitch and octave flag of the note that starts first,
/// and the higher velocity. It has no contour and no overlap flag:
/// [`PitchTrack::with_contours`] and [`NoteList::with_overlaps`] fill
/// those after the cleanup.
#[must_use]
pub fn merge_overlaps(notes: &NoteList) -> NoteList {
    let mut merged: Vec<Note> = Vec::with_capacity(notes.notes().len());
    for note in notes.notes() {
        let same_key = merged
            .iter_mut()
            .rev()
            .find(|kept| same_key(kept, note) && note.onset() < kept.offset());
        match same_key {
            Some(kept) => *kept = joined(kept, note),
            None => merged.push(note.clone()),
        }
    }
    NoteList::new(merged)
}

/// Joins the pieces of a held note that the decoder split, on the onset
/// rule alone: [`join_splits_with`] with a stem of silence, which never
/// gives an attack.
#[must_use]
pub fn join_splits(notes: &NoteList, run: &ModelRun) -> NoteList {
    join_splits_with(notes, run, &StemLevels::new(&[], MODEL_RATE))
}

/// Joins the pieces of a held note that the decoder split.
///
/// A note joins a note of the same key that ends within [`SPLIT_GAP`]
/// before it, on an onset activation below [`SPLIT_ONSET`], when its
/// velocity is no higher than that note's and `levels` show no attack of
/// its own.
///
/// A repeated note starts on a strong onset or on an attack in the stem,
/// and stays apart. The decoder can fill the gap before a low
/// repeat and read its onset weak, so the onset alone misses it. A split
/// piece is the weaker rest of a held note; a weaker piece before a
/// stronger note is an early fragment of the attack, and joining it would
/// move the onset.
#[must_use]
pub fn join_splits_with(notes: &NoteList, run: &ModelRun, levels: &StemLevels<'_>) -> NoteList {
    let mut joined_notes: Vec<Note> = Vec::with_capacity(notes.notes().len());
    for note in notes.notes() {
        let weak = run
            .onset_strength(note.onset(), note.pitch())
            .is_some_and(|strength| strength < SPLIT_ONSET);
        let earlier = joined_notes.iter_mut().rev().find(|kept| {
            same_key(kept, note)
                && kept.offset() <= note.onset()
                && note.onset().get() - kept.offset().get() <= SPLIT_GAP.get()
                && note.velocity() <= kept.velocity()
        });
        match earlier {
            Some(kept) if weak && !attacks(levels, kept, note) => {
                *kept = joined(kept, note);
            }
            _ => joined_notes.push(note.clone()),
        }
    }
    NoteList::new(joined_notes)
}

/// Splits each note where the stem has an attack of its own inside it.
///
/// The decoder can read a low repeat with a short gap as part of one long
/// note, so no later step sees two notes. A frame inside a note marks a
/// dip when it is no louder than the frame before it, quieter than the
/// frame after it, and [`ATTACK_DIP`] or more below the loudest frame in
/// the [`ATTACK_AFTER`] after it. The split falls on the last envelope
/// reading at or below the first share of [`RISE_SPAN`] of the loudest
/// reading in that span, before the first reading at the second share
/// after the quietest one, and the note's fundamental must dip by
/// [`FUNDAMENTAL_DIP`] across it. It holds when the piece from there has
/// an attack of its own after the piece before it, by the rule of the
/// split join, which then keeps the two apart. Each piece keeps the
/// note's key, octave flag and velocity.
///
/// A last piece of at most twice [`ATTACK_AFTER`] that runs into the
/// decoder's next note of its key, within [`SPLIT_GAP`], starts that note
/// instead, unless that note has an attack of its own after the piece.
/// The decoder can start a note 58 ms after its attack, which leaves such
/// a piece. A repeat after a gap of 20 ms or less can show no attack by
/// the rule of the split join, so the limit keeps a longer piece before
/// one a note of its own. A gated repeat of 60 to 90 ms after such a gap
/// still loses its onset to the piece before it.
#[must_use]
pub fn split_at_attacks(notes: &NoteList, levels: &StemLevels<'_>) -> NoteList {
    let decoded = notes.notes();
    let mut starts: Vec<Option<Seconds>> = vec![None; decoded.len()];
    let mut pieces: Vec<Note> = Vec::with_capacity(decoded.len());
    for (index, found) in decoded.iter().enumerate() {
        let note = starts[index]
            .and_then(|onset| piece(found, onset, found.offset()))
            .unwrap_or_else(|| found.clone());
        if note.pitch() < SPLIT_LOWEST {
            pieces.push(note);
            continue;
        }
        let frames = levels.frames_between(note.onset(), note.offset());
        let reach = levels.frames_between(
            note.onset(),
            Seconds::clamped(note.offset().get() + ATTACK_AFTER.get()),
        );
        // Each frame's level once: the loudest earlier frame is kept as the
        // frames advance, which keeps a long note linear in its frames.
        let first = frames.start.saturating_sub(1);
        let cached: Vec<f64> = (first..=reach.end).map(|m| levels.ac_rms(m)).collect();
        let level = |m: usize| cached[m - first];
        let mut rest = note.clone();
        let (mut seen, mut before) = (frames.start, 0.0_f64);
        for frame in frames {
            let hold_end = Seconds::clamped(levels.frame_time(frame).get() - HOLD_BEFORE.get());
            while seen < levels.frames_between(rest.onset(), hold_end).end {
                before = before.max(level(seen));
                seen += 1;
            }
            let Some(onset) = split_point(levels, frame, note.pitch(), &level, before)
                .filter(|onset| rest.offset().get() - onset.get() >= ATTACK_AFTER.get())
            else {
                continue;
            };
            if let (Some(earlier), Some(after)) = (
                piece(&rest, rest.onset(), onset),
                piece(&rest, onset, rest.offset()),
            ) && attacks(levels, &earlier, &after)
            {
                pieces.push(earlier);
                rest = after;
                seen = levels.frames_between(rest.onset(), rest.offset()).start;
                before = 0.0;
            }
        }
        let next = (rest.onset() > note.onset())
            .then(|| {
                decoded[index + 1..]
                    .iter()
                    .position(|later| same_key(later, &rest) && later.onset() >= rest.offset())
            })
            .flatten()
            .map(|offset| index + 1 + offset)
            .filter(|&later| {
                rest.offset().get() - rest.onset().get() <= 2.0 * ATTACK_AFTER.get()
                    && decoded[later].onset().get() - rest.offset().get() <= SPLIT_GAP.get()
                    && !attacks(levels, &rest, &decoded[later])
            });
        match next {
            Some(later) => starts[later] = Some(rest.onset()),
            None => pieces.push(rest),
        }
    }
    NoteList::new(pieces)
}

/// Where a dip at `frame` ends, by the rule of [`split_at_attacks`] for a
/// note of `pitch`, or `None` when the frame marks no dip. `level` reads a
/// frame's level, and `before` is the loudest frame of the note centred
/// [`HOLD_BEFORE`] or more before this one.
fn split_point(
    levels: &StemLevels<'_>,
    frame: usize,
    pitch: MidiPitch,
    level: &dyn Fn(usize) -> f64,
    before: f64,
) -> Option<Seconds> {
    let centre = levels.frame_time(frame);
    let after = Seconds::clamped(centre.get() + ATTACK_AFTER.get());
    let loud = levels
        .frames_between(centre, after)
        .map(level)
        .fold(0.0, f64::max);
    // The dip test repeats the attack rule's, and keeps the readings to
    // frames that can split.
    if frame == 0
        || level(frame - 1) < level(frame)
        || level(frame + 1) <= level(frame)
        || 20.0 * (loud / level(frame)).log10() < ATTACK_DIP.get()
        || before < HOLD_SPAN.1 * loud
    {
        return None;
    }
    let (from, to) = RISE_SPAN;
    let step = RISE_STEP.get();
    let start = centre.get() - QUIET_FRAME.get() / 2.0;
    let last = cast::floor_to_usize((after.get() - start) / step)?;
    let readings = readings(
        levels,
        (0..=last).map(|k| start + cast::usize_to_f64(k) * step),
        rise_window(pitch),
    );
    let peak = peak_from(&readings, centre.get());
    // The climb starts at the quietest reading: the note before the dip
    // can read louder than the one after it.
    let trough = readings
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.1.total_cmp(&b.1.1))
        .map(|(k, _)| k)?;
    let rise = trough
        + readings[trough..]
            .iter()
            .position(|&(_, level)| level >= to * peak)?;
    let onset = readings[..rise]
        .iter()
        .rev()
        .find(|&&(_, level)| level <= from * peak)
        .map(|&(time, _)| time)?;
    fundamental_dips(levels, &readings, onset, pitch).then(|| Seconds::clamped(onset))
}

/// Whether the level at the fundamental of `pitch`, at the times of
/// `readings`, dips by [`FUNDAMENTAL_DIP`] from the loudest reading from
/// `onset` on to the quietest before it.
fn fundamental_dips(
    levels: &StemLevels<'_>,
    readings: &[(f64, f64)],
    onset: f64,
    pitch: MidiPitch,
) -> bool {
    let level = |time: f64| levels.fundamental_level(Seconds::clamped(time), pitch);
    let (low, high) = readings
        .iter()
        .fold((f64::INFINITY, 0.0_f64), |(low, high), &(time, _)| {
            if time < onset {
                (low.min(level(time)), high)
            } else {
                (low, high.max(level(time)))
            }
        });
    20.0 * (high / low).log10() >= FUNDAMENTAL_DIP.get()
}

/// The envelope window of the rise test for a note of `pitch`: one period,
/// and at least [`RISE_WINDOW`].
fn rise_window(pitch: MidiPitch) -> f64 {
    (1.0 / Hz::from(pitch).get()).max(RISE_WINDOW.get())
}

/// The envelope through `window` at each of `times`, with its time.
fn readings(
    levels: &StemLevels<'_>,
    times: impl Iterator<Item = f64>,
    window: f64,
) -> Vec<(f64, f64)> {
    times
        .map(|time| {
            (
                time,
                levels.envelope(Seconds::clamped(time), Seconds::clamped(window)),
            )
        })
        .collect()
}

/// The loudest of `readings` at `time` or later.
fn peak_from(readings: &[(f64, f64)], time: f64) -> f64 {
    readings
        .iter()
        .filter(|(at, _)| *at >= time)
        .map(|&(_, level)| level)
        .fold(0.0, f64::max)
}

/// `note` cut to the span from `onset` to `offset`, with its key, octave
/// flag and velocity, or `None` when the span is empty.
fn piece(note: &Note, onset: Seconds, offset: Seconds) -> Option<Note> {
    let mut piece = Note::new(onset, offset, note.pitch()).ok()?;
    if let Some(flag) = note.octave_uncertain() {
        piece = piece.with_octave_uncertain(flag);
    }
    if let Some(velocity) = note.velocity() {
        piece = piece.with_velocity(velocity);
    }
    Some(piece)
}

/// Whether `note` starts on an attack of its own after `kept`.
///
/// The quiet frames are those centred from [`ATTACK_BEFORE`] the onset to
/// the onset, and not before `kept` starts; the loud frames those centred
/// from the onset to [`ATTACK_AFTER`] it, and not past the note's end. An
/// attack needs the stem to dip by [`ATTACK_DIP`] and the note's harmonics
/// by [`HARMONIC_DIP`] from the loudest frame to the quietest, and no slow
/// rise. The stem's dip keeps the pieces of a held note under a line of
/// other keys together, the harmonics' dip a tail under the attack of
/// another key, and the rise test a tremolo.
fn attacks(levels: &StemLevels<'_>, kept: &Note, note: &Note) -> bool {
    let start =
        Seconds::clamped((note.onset().get() - ATTACK_BEFORE.get()).max(kept.onset().get()));
    let end = Seconds::clamped((note.onset().get() + ATTACK_AFTER.get()).min(note.offset().get()));
    let quiet = levels.frames_between(start, note.onset());
    let loud = levels.frames_between(note.onset(), end);
    let dips = |level: &dyn Fn(usize) -> f64, dip: Decibels| {
        let low = quiet.clone().map(level).fold(f64::INFINITY, f64::min);
        let high = loud.clone().map(level).fold(0.0, f64::max);
        // A dip from silence is endless; silence on both sides is none.
        20.0 * (high / low).log10() >= dip.get()
    };
    dips(&|m| levels.ac_rms(m), ATTACK_DIP)
        && dips(&|m| levels.harmonic_level(m, note.pitch()), HARMONIC_DIP)
        && !rises_slowly(levels, note, start)
}

/// Whether the stem's envelope around `note`, read every [`RISE_STEP`]
/// from `start` to [`RISE_READ`] limits past [`RISE_LATEST`] after the
/// onset, rises slowly: from below the first share of [`RISE_SPAN`] of its
/// loudest reading after the onset, at the latest [`RISE_LATEST`] after
/// the onset, to above the second share, its fastest rise takes longer
/// than the limit, a step read through the window plus [`RISE_SLACK`].
///
/// The window spans one period of the note, at least [`RISE_WINDOW`], so a
/// periodic tone reads steady. The RMS of a step reads in the window as the
/// square root of the share of the window past it, which climbs from the
/// first share to the second over the difference of their squares, 0.8 of
/// the window. An envelope that never falls below the first share rises
/// from no silence, and counts as slow. A rise counts only when the stem
/// [`held`] before its dip.
fn rises_slowly(levels: &StemLevels<'_>, note: &Note, start: Seconds) -> bool {
    let onset = note.onset().get();
    let window = rise_window(note.pitch());
    let (from, to) = RISE_SPAN;
    let limit = (to * to - from * from) * window + RISE_SLACK.get();
    let end = onset + RISE_LATEST.get() + RISE_READ * limit;
    let step = RISE_STEP.get();
    let first = ((start.get() - onset) / step).ceil();
    let Some(last) = cast::floor_to_usize((end - onset) / step - first) else {
        return true;
    };
    let readings = readings(
        levels,
        (0..=last).map(|k| onset + (first + cast::usize_to_f64(k)) * step),
        window,
    );
    let peak = peak_from(&readings, onset);
    let mut low = None;
    let mut fastest: Option<f64> = None;
    for &(time, level) in &readings {
        if level <= from * peak && time <= onset + RISE_LATEST.get() {
            low = Some(time);
        }
        if let Some(since) = low
            && level >= to * peak
        {
            let rose = readings
                .iter()
                .find(|&&(after, level)| after > since && level >= HOLD_SPAN.1 * peak)
                .map_or(time, |&(after, _)| after);
            if held(levels, since, rose, peak, window) {
                fastest = Some(fastest.map_or(time - since, |rise| rise.min(time - since)));
            }
            low = None;
        }
    }
    fastest.is_none_or(|rise| rise > limit)
}

/// Whether the envelope, read every [`RISE_STEP`], held before the dip
/// that ends at `low`. The dip runs from the last reading at or above the
/// second share of [`HOLD_SPAN`] of `peak`, at most [`HOLD_SEARCH`] before
/// `low`, to `rose`, the first reading back above it. Before the dip the
/// envelope must stay above the first share for [`HOLD_BEFORE`] and for
/// [`HOLD_RATIO`] times the dip. A dip that lasts the whole search is too
/// long for a tremolo, and holds. The level before the stem starts is
/// unknown, so a dip or a hold that reaches the start is too short.
fn held(levels: &StemLevels<'_>, low: f64, rose: f64, peak: f64, window: f64) -> bool {
    let (stay, end) = HOLD_SPAN;
    let step = RISE_STEP.get();
    let level = |time: f64| levels.envelope(Seconds::clamped(time), Seconds::clamped(window));
    let back = |from: f64| (0..).map(move |k| from - cast::usize_to_f64(k) * step);
    let Some(fell) = back(low)
        .take_while(|&time| time >= 0.0 && time >= low - HOLD_SEARCH.get())
        .find(|&time| level(time) >= end * peak)
    else {
        return low >= HOLD_SEARCH.get();
    };
    let hold = HOLD_BEFORE.get().max(HOLD_RATIO * (rose - fell));
    fell >= hold
        && back(fell)
            .take_while(|&time| time > fell - hold)
            .all(|time| level(time) >= stay * peak)
}

fn same_key(a: &Note, b: &Note) -> bool {
    (a.pitch().get() - b.pitch().get()).abs() < 0.5
}

/// `first` stretched to the later offset of the two, with the higher
/// velocity.
fn joined(first: &Note, second: &Note) -> Note {
    let offset = if second.offset() > first.offset() {
        second.offset()
    } else {
        first.offset()
    };
    let Some(note) = piece(first, first.onset(), offset) else {
        return first.clone();
    };
    match first.velocity().max(second.velocity()) {
        Some(velocity) => note.with_velocity(velocity),
        None => note,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::SampleRate;
    use crate::notes::{TrackFrame, Velocity};
    use crate::units::{Decibels, Hz};

    fn s(value: f64) -> Seconds {
        Seconds::new(value).unwrap()
    }

    fn note(onset: f64, offset: f64, pitch: f64) -> Note {
        Note::new(s(onset), s(offset), MidiPitch::new(pitch).unwrap()).unwrap()
    }

    /// A note with the decoder's velocity, its mean activation.
    fn heard(onset: f64, offset: f64, pitch: f64, velocity: u8) -> Note {
        note(onset, offset, pitch).with_velocity(Velocity::new(velocity).unwrap())
    }

    /// A track voiced at `pitch` from 0 to 2 s, every 5 ms.
    fn track_at(pitch: f64) -> PitchTrack {
        PitchTrack::from_frames(
            (0..400)
                .map(|i| TrackFrame {
                    time: s(f64::from(i) * 0.005),
                    f0: Some(Hz::from(MidiPitch::new(pitch).unwrap())),
                    level: Decibels::new(-3.0).unwrap(),
                })
                .collect(),
        )
    }

    fn pitches(list: &NoteList) -> Vec<f64> {
        list.notes().iter().map(|n| n.pitch().get()).collect()
    }

    #[test]
    fn a_note_an_octave_above_the_track_moves_down() {
        let notes = NoteList::new(vec![heard(0.5, 1.0, 45.0, 90), heard(0.5, 0.6, 57.0, 37)]);
        let (checked, check) = check_octaves(&notes, &track_at(45.0));
        assert_eq!(pitches(&checked), vec![45.0, 45.0]);
        assert_eq!(
            check,
            OctaveCheck {
                moved: 1,
                flagged: 0,
                unchecked: 0
            }
        );
        assert!(
            checked
                .notes()
                .iter()
                .all(|n| n.octave_uncertain() == Some(false))
        );
        // The copy lands on its note and merges into it.
        let merged = merge_overlaps(&checked);
        assert_eq!(
            merged.notes(),
            &[heard(0.5, 1.0, 45.0, 90).with_octave_uncertain(false)]
        );
    }

    /// The decoder gives every note of an octave bass a twin an octave
    /// away with nearly its span; the true note is the stronger. Figures
    /// from the 16th-note octave part of the evaluation.
    #[test]
    fn an_octave_bass_keeps_its_true_upper_notes() {
        let notes = NoteList::new(vec![
            heard(0.755, 0.859, 45.0, 94),
            heard(0.755, 0.871, 57.0, 46),
            heard(0.859, 0.998, 45.0, 48),
            heard(0.871, 0.998, 57.0, 100),
        ]);
        let (checked, check) = check_octaves(&notes, &track_at(45.0));
        assert_eq!(check.moved, 1);
        let merged = merge_overlaps(&checked);
        let kept: Vec<(f64, f64)> = merged
            .notes()
            .iter()
            .map(|n| (n.onset().get(), n.pitch().get()))
            .collect();
        // The weak A2 twin of the true A3 stays for the manual pass.
        assert_eq!(kept, vec![(0.755, 45.0), (0.859, 45.0), (0.871, 57.0)]);
    }

    #[test]
    fn a_copy_moves_only_inside_its_lower_note() {
        let moved = |lower: Note| {
            let notes = NoteList::new(vec![lower, heard(0.5, 0.6, 57.0, 37)]);
            check_octaves(&notes, &track_at(45.0)).1.moved
        };
        let slack = COPY_SLACK.get();
        assert_eq!(moved(heard(0.5 + slack, 0.6 - slack, 45.0, 90)), 1);
        assert_eq!(moved(heard(0.5 + slack + 1e-3, 1.0, 45.0, 90)), 0);
        assert_eq!(moved(heard(0.4, 0.6 - slack - 1e-3, 45.0, 90)), 0);
        // A weaker lower note, or one without a velocity, moves nothing.
        assert_eq!(moved(heard(0.5, 1.0, 45.0, 37)), 0);
        assert_eq!(moved(note(0.5, 1.0, 45.0)), 0);
    }

    #[test]
    fn a_lone_note_an_octave_above_the_track_keeps_its_pitch() {
        let notes = NoteList::new(vec![note(0.5, 1.0, 57.0), note(0.8, 1.2, 45.0)]);
        let (checked, check) = check_octaves(&notes, &track_at(45.0));
        assert_eq!(pitches(&checked), vec![57.0, 45.0]);
        assert_eq!(checked.notes()[0].octave_uncertain(), Some(true));
        assert_eq!((check.moved, check.flagged), (0, 1));
    }

    #[test]
    fn a_note_three_semitones_off_keeps_its_pitch_and_the_flag() {
        let notes = NoteList::new(vec![note(0.5, 1.0, 48.0)]);
        let (checked, check) = check_octaves(&notes, &track_at(45.0));
        assert_eq!(pitches(&checked), vec![48.0]);
        assert_eq!(checked.notes()[0].octave_uncertain(), Some(true));
        assert_eq!(check.flagged, 1);
    }

    #[test]
    fn the_gap_rules_hold_at_their_bounds() {
        // The note sounds over its lower octave, which starts earlier, so
        // the note under test sorts last.
        let flag = |pitch: f64, track: f64| {
            let notes = NoteList::new(vec![
                heard(0.45, 1.0, pitch - 12.0, 90),
                heard(0.5, 1.0, pitch, 40),
            ]);
            let (checked, _) = check_octaves(&notes, &track_at(track));
            let last = checked.notes().last().unwrap();
            (last.pitch().get(), last.octave_uncertain())
        };
        // Agreement within one semitone.
        assert_eq!(flag(46.0, 45.0), (46.0, Some(false)));
        assert_eq!(flag(46.0, 44.9), (46.0, Some(true)));
        // An octave within half a semitone, at or below the ceiling.
        assert_eq!(flag(57.0, 45.5), (45.0, Some(false)));
        assert_eq!(flag(57.0, 44.4), (57.0, Some(true)));
        // Above the ceiling an octave only flags.
        assert_eq!(flag(58.0, 46.0), (58.0, Some(true)));
        // A note an octave below the track keeps its pitch.
        assert_eq!(flag(33.0, 45.0), (33.0, Some(true)));
    }

    #[test]
    fn an_even_count_of_frames_takes_the_middle_two() {
        // Frames at 44, 44, 46 and 46 have the median 45, an octave below
        // the copy; either middle frame alone would not be.
        let track = PitchTrack::from_frames(
            [44.0, 44.0, 46.0, 46.0]
                .iter()
                .enumerate()
                .map(|(i, &pitch)| TrackFrame {
                    time: s(0.5 + 0.005 * f64::from(u32::try_from(i).unwrap())),
                    f0: Some(Hz::from(MidiPitch::new(pitch).unwrap())),
                    level: Decibels::new(-3.0).unwrap(),
                })
                .collect(),
        );
        let notes = NoteList::new(vec![heard(0.49, 1.0, 45.0, 90), heard(0.5, 0.6, 57.0, 37)]);
        assert_eq!(check_octaves(&notes, &track).1.moved, 1);
    }

    #[test]
    fn a_note_with_too_few_voiced_frames_stays_unchecked() {
        for (frames, unchecked) in [(2, 1), (3, 0)] {
            let track = PitchTrack::from_frames(
                (0..frames)
                    .map(|i| TrackFrame {
                        time: s(0.5 + f64::from(i) * 0.005),
                        f0: Some(Hz::from(MidiPitch::new(45.0).unwrap())),
                        level: Decibels::new(-3.0).unwrap(),
                    })
                    .collect(),
            );
            let notes = NoteList::new(vec![note(0.5, 1.0, 57.0)]);
            let (checked, check) = check_octaves(&notes, &track);
            assert_eq!(check.unchecked, unchecked, "{frames} frames");
            assert_eq!(
                checked.notes()[0].octave_uncertain().is_none(),
                unchecked == 1
            );
        }
    }

    /// A model run of 80 frames with an onset of `strength` at A2 on
    /// frame 30, and the time of a frame.
    fn run_with_onset(strength: f32) -> (ModelRun, impl Fn(usize) -> f64) {
        run_with_onset_at(strength, 30)
    }

    /// A model run with an onset of `strength` at A2 on `frame`, 50 frames
    /// before its end, and the time of a frame.
    fn run_with_onset_at(strength: f32, frame: usize) -> (ModelRun, impl Fn(usize) -> f64) {
        use basic_pitch::{Posteriorgram, Posteriorgrams};
        let (frames, bin) = (frame + 50, 45 - 21);
        let mut onsets = Posteriorgram::zeros(frames, 88);
        onsets.as_mut_slice()[frame * 88 + bin] = strength;
        let grams = Posteriorgrams::new(
            Posteriorgram::zeros(frames, 88),
            onsets,
            Posteriorgram::zeros(frames, 264),
        )
        .unwrap();
        (
            ModelRun::from_posteriorgrams(grams),
            basic_pitch::notes::frame_to_time,
        )
    }

    /// The ghost merge relies on this: a held key with a second onset
    /// decodes as two notes that touch and do not overlap.
    #[test]
    fn the_decoder_gives_a_repeat_of_one_key_no_overlap() {
        use basic_pitch::{Posteriorgram, Posteriorgrams};
        let (frames, bin) = (80, 45 - 21);
        let mut held = Posteriorgram::zeros(frames, 88);
        let mut onsets = Posteriorgram::zeros(frames, 88);
        for frame in 10..60 {
            held.as_mut_slice()[frame * 88 + bin] = 0.8;
        }
        for frame in [10, 30] {
            onsets.as_mut_slice()[frame * 88 + bin] = 0.9;
        }
        let run = ModelRun::from_posteriorgrams(
            Posteriorgrams::new(held, onsets, Posteriorgram::zeros(frames, 264)).unwrap(),
        );
        let settings = crate::notes::Profile::Bass.settings(crate::units::Bpm::new(120.0).unwrap());
        let decoded = run.decode(&settings).unwrap();
        let a2: Vec<&Note> = decoded
            .notes()
            .iter()
            .filter(|decoded| (decoded.pitch().get() - 45.0).abs() < 0.5)
            .collect();
        assert_eq!(a2.len(), 2);
        assert!(a2[0].offset() <= a2[1].onset());
        assert_eq!(
            merge_overlaps(&decoded).notes().len(),
            decoded.notes().len()
        );
    }

    #[test]
    fn the_onset_strength_reads_one_frame_either_side() {
        let (run, time) = run_with_onset(0.9);
        let a2 = MidiPitch::new(45.0).unwrap();
        let at = |t| run.onset_strength(s(t), a2).unwrap();
        assert!((at(time(30) + 0.01) - 0.9).abs() < 1e-6);
        assert!(at(time(33)).abs() < 1e-6);
        assert_eq!(
            run.onset_strength(s(time(30)), MidiPitch::new(120.0).unwrap()),
            None
        );
    }

    #[test]
    fn a_split_on_a_weak_onset_joins_and_a_repeat_stays() {
        for (strength, expected) in [(0.6, 1), (0.67, 1), (0.69, 2), (0.9, 2)] {
            let (run, time) = run_with_onset(strength);
            let notes = NoteList::new(vec![
                note(time(10), time(30), 45.0),
                note(time(30), time(45), 45.0),
            ]);
            let joined = join_splits(&notes, &run);
            assert_eq!(joined.notes().len(), expected, "onset {strength}");
            if expected == 1 {
                assert_eq!(joined.notes()[0], note(time(10), time(45), 45.0));
            }
        }
    }

    #[test]
    fn a_split_joins_within_its_gap_and_only_as_a_tail() {
        let (run, time) = run_with_onset(0.0);
        let gap = SPLIT_GAP.get();
        let count = |first: (f64, f64), second: (f64, f64)| {
            let notes = NoteList::new(vec![
                note(first.0, first.1, 45.0),
                note(second.0, second.1, 45.0),
            ]);
            join_splits(&notes, &run).notes().len()
        };
        let (start, end) = (time(30), time(40));
        assert_eq!(count((0.1, start - gap), (start, end)), 1);
        assert_eq!(count((0.1, start - gap - 1e-3), (start, end)), 2);
        // Two model frames are 23.2 ms.
        assert_eq!(count((0.1, start - 0.023), (start, end)), 1);
        assert_eq!(count((0.1, start - 0.024), (start, end)), 2);
    }

    #[test]
    fn a_weaker_piece_never_takes_a_stronger_note() {
        let (run, time) = run_with_onset(0.0);
        let joined = |first: u8, second: u8| {
            let notes = NoteList::new(vec![
                heard(time(10), time(30), 45.0, first),
                heard(time(30), time(60), 45.0, second),
            ]);
            join_splits(&notes, &run).notes().len()
        };
        assert_eq!(joined(90, 50), 1);
        assert_eq!(joined(90, 90), 1);
        // An early fragment of the attack before the true note: joining it
        // would move the onset.
        assert_eq!(joined(40, 90), 2);
    }

    fn rate() -> SampleRate {
        SampleRate::new(22_050).unwrap()
    }

    /// The model frames of [`LEAD_SAMPLES`].
    const LEAD_FRAMES: usize = 137;

    /// The samples of [`stem_at`] before its time 0: more than
    /// [`HOLD_SEARCH`], and a whole number of model frames and of level hops
    /// of 137 samples, so both read the stem as without them.
    const LEAD_SAMPLES: i32 = 35_072;

    /// A stem at 22,050 Hz from [`LEAD_SAMPLES`] before time 0 to 1 s after
    /// it: a sine at `f0` and a 1500 Hz sine with amplitudes `level(t)`,
    /// `t` in seconds from `onset`.
    fn stem_at(f0: f64, onset: f64, level: impl Fn(f64) -> (f64, f64)) -> Vec<f64> {
        (-LEAD_SAMPLES..22_050)
            .map(|index| {
                let t = f64::from(index) / 22_050.0;
                let (note, high) = level(t - onset);
                note * (std::f64::consts::TAU * f0 * t).sin()
                    + high * (std::f64::consts::TAU * 1500.0 * t).sin()
            })
            .collect()
    }

    /// [`stem_at`] an A2, whose first eight harmonics lie far from 1500 Hz.
    fn stem(onset: f64, level: impl Fn(f64) -> (f64, f64)) -> Vec<f64> {
        stem_at(110.0, onset, level)
    }

    /// The notes of `pitch` after the join of two touching pieces that meet
    /// at [`onset`], on a weak onset, over a stem from [`stem_at`].
    fn joined_at(pitch: f64, mono: &[f64]) -> usize {
        let (run, time) = run_with_onset_at(0.0, LEAD_FRAMES + 30);
        let notes = NoteList::new(vec![
            note(time(LEAD_FRAMES + 10), time(LEAD_FRAMES + 30), pitch),
            note(time(LEAD_FRAMES + 30), time(LEAD_FRAMES + 45), pitch),
        ]);
        join_splits_with(&notes, &run, &StemLevels::new(mono, rate()))
            .notes()
            .len()
    }

    fn joined_a2(mono: &[f64]) -> usize {
        joined_at(45.0, mono)
    }

    fn onset() -> f64 {
        basic_pitch::notes::frame_to_time(30)
    }

    #[test]
    fn a_repeat_after_a_gap_of_silence_stays_apart() {
        let gap = stem(onset(), |t| {
            (if (-0.05..0.0).contains(&t) { 0.0 } else { 0.5 }, 0.0)
        });
        assert_eq!(joined_a2(&gap), 2);
        assert_eq!(joined_a2(&stem(onset(), |_| (0.5, 0.0))), 1);
        // Silence on both sides is no attack.
        assert_eq!(joined_a2(&vec![0.0; 22_050]), 1);
    }

    #[test]
    fn the_stem_dips_by_attack_dip() {
        // 10 ms of silence before the onset gives the rise its start. The
        // 1500 Hz tone before them holds the stem's quietest frame 19.5 or
        // 18.5 dB below its loudest, while the harmonics dip past 40 dB.
        for (high, expected) in [(0.07, 2), (0.079, 1)] {
            let mono = stem(onset(), |t| {
                if t >= 0.0 {
                    (0.5, high)
                } else if t > -0.01 {
                    (0.0, 0.0)
                } else {
                    (0.001, high)
                }
            });
            assert_eq!(joined_a2(&mono), expected, "1500 Hz at {high}");
        }
    }

    #[test]
    fn the_harmonics_dip_by_harmonic_dip() {
        // The A2 harmonics dip 23.5 or 20.5 dB; the 1500 Hz tone after the
        // onset lifts the stem's dip past 30 dB.
        for (low, expected) in [(0.0334, 2), (0.0472, 1)] {
            let mono = stem(onset(), |t| if t < 0.0 { (low, 0.0) } else { (0.5, 0.5) });
            assert_eq!(joined_a2(&mono), expected, "A2 at {low} before");
        }
    }

    #[test]
    fn a_dc_offset_counts_for_nothing() {
        let gap = |offset: f64| {
            stem(onset(), |t| {
                (if (-0.05..0.0).contains(&t) { 0.0 } else { 0.5 }, 0.0)
            })
            .into_iter()
            .map(|x| x + offset)
            .collect::<Vec<_>>()
        };
        assert_eq!(joined_a2(&gap(0.2)), 2);
    }

    #[test]
    fn a_slow_rise_is_no_attack() {
        // At A2 the envelope window spans 9.1 ms, which a step climbs in
        // 7.3 ms, so a rise may take 14.3 ms: a linear attack of 13 ms
        // stays apart, one of 14 ms joins.
        for (ramp, expected) in [(0.013, 2), (0.014, 1)] {
            let mono = stem(onset(), |t| (0.5 * (t / ramp).clamp(0.0, 1.0), 0.0));
            assert_eq!(joined_a2(&mono), expected, "ramp {ramp} s");
        }
    }

    #[test]
    fn a_high_note_reads_through_rise_window() {
        // One period of A5 spans 1.1 ms; the 4 ms window lets a linear
        // attack of 10 ms stay apart, and one of 11 ms joins.
        for (ramp, expected) in [(0.01, 2), (0.011, 1)] {
            let mono = stem_at(880.0, onset(), |t| (0.5 * (t / ramp).clamp(0.0, 1.0), 0.0));
            assert_eq!(joined_at(81.0, &mono), expected, "ramp {ramp} s");
        }
    }

    #[test]
    fn a_rise_starts_by_rise_latest() {
        // Silence from 50 ms before the onset to 30 ms after it: the rise
        // from 20 ms after the onset is fast. To 35 ms after it, the rise
        // from there is slow, and the rise from later silence is too late.
        for (end, expected) in [(0.03, 2), (0.035, 1)] {
            let mono = stem(onset(), |t| {
                (if (-0.05..end).contains(&t) { 0.0 } else { 0.5 }, 0.0)
            });
            assert_eq!(joined_a2(&mono), expected, "silence to {end} s");
        }
    }

    #[test]
    fn a_low_note_reads_past_attack_after() {
        // At B0 the window spans 32.4 ms and a rise may take 32.9 ms. With
        // silence to 46 ms after the onset, the rise from 20 ms is fast. To
        // 54 ms, it is slow, but only readings past 50 ms reach its peak.
        for (end, expected) in [(0.046, 2), (0.054, 1)] {
            let mono = stem_at(30.868, onset(), |t| {
                (if (-0.05..end).contains(&t) { 0.0 } else { 0.5 }, 0.0)
            });
            assert_eq!(joined_at(23.0, &mono), expected, "silence to {end} s");
        }
    }

    /// The notes after the join over `mono` from [`stem_at`] without its
    /// lead, which starts the stem 345 ms before [`onset`].
    fn joined_without_lead(mono: &[f64]) -> usize {
        let (run, time) = run_with_onset(0.0);
        let notes = NoteList::new(vec![
            note(time(10), time(30), 45.0),
            note(time(30), time(45), 45.0),
        ]);
        let lead = usize::try_from(LEAD_SAMPLES).unwrap();
        join_splits_with(&notes, &run, &StemLevels::new(&mono[lead..], rate()))
            .notes()
            .len()
    }

    /// A note of `hold` s from silence, `gap` s of silence and the onset.
    fn note_before_gap(hold: f64, gap: f64) -> Vec<f64> {
        stem(onset(), |t| {
            let on = (-gap - hold..-gap).contains(&t) || t >= 0.0;
            (if on { 0.5 } else { 0.0 }, 0.0)
        })
    }

    #[test]
    fn a_rise_needs_a_hold_before_its_gap() {
        // Before a 30 ms gap the dip below six tenths of the peak lasts
        // 32 ms, and HOLD_RATIO times that stays under HOLD_BEFORE. Through
        // the 9.1 ms window of A2 a note of 59 ms holds above half the peak
        // for HOLD_BEFORE and the repeat stays apart; one of 58 ms joins.
        for (hold, expected) in [(0.059, 2), (0.058, 1)] {
            assert_eq!(
                joined_a2(&note_before_gap(hold, 0.03)),
                expected,
                "held {hold} s"
            );
        }
    }

    #[test]
    fn a_hold_lasts_hold_ratio_times_the_dip() {
        // Before a 100 ms gap the dip lasts 100 ms, so the note must hold
        // for 150 ms. A note of 147 ms holds above half the peak for 151 ms
        // and the repeat stays apart; one of 146 ms holds for 148 ms and
        // joins.
        for (hold, expected) in [(0.147, 2), (0.146, 1)] {
            assert_eq!(
                joined_a2(&note_before_gap(hold, 0.1)),
                expected,
                "held {hold} s"
            );
        }
    }

    #[test]
    fn a_hold_cut_by_the_stem_start_is_too_short() {
        // Before a 150 ms gap the note must hold for 225 ms. Without the
        // lead the stem starts 195 ms before the gap.
        let mono = stem(onset(), |t| {
            (if t < -0.15 || t >= 0.0 { 0.5 } else { 0.0 }, 0.0)
        });
        assert_eq!(joined_a2(&mono), 2);
        assert_eq!(joined_without_lead(&mono), 1);
    }

    #[test]
    fn a_dip_cut_by_the_stem_start_is_no_gap() {
        // Silence from the stem start to the onset: with the lead it lasts
        // longer than HOLD_SEARCH, without it 345 ms.
        let mono = stem(onset(), |t| (if t < 0.0 { 0.0 } else { 0.5 }, 0.0));
        assert_eq!(joined_a2(&mono), 2);
        assert_eq!(joined_without_lead(&mono), 1);
    }

    #[test]
    fn the_rise_reads_rise_read_limits_past_rise_latest() {
        // After a 50 ms gap, a step to 0.4 and a second step to 1. At A2 a
        // rise may take 14.3 ms, and the readings run to 62.9 ms. A second
        // step at 65 ms reads inside them, so the climb from silence to
        // its peak is slow and the note joins; at 66 ms the peak lies past
        // them and the step to 0.4 reads fast.
        for (late, expected) in [(0.065, 1), (0.066, 2)] {
            let mono = stem(onset(), |t| {
                let level = if t < -0.05 {
                    0.5
                } else if t < 0.0 {
                    0.0
                } else if t < late {
                    0.4
                } else {
                    1.0
                };
                (level, 0.0)
            });
            assert_eq!(joined_a2(&mono), expected, "step to 1 at {late} s");
        }
    }

    #[test]
    fn the_hold_stays_above_the_first_share_of_hold_span() {
        // A note that ramps up over 40 ms and then holds, before a 50 ms
        // gap that asks for a hold of 75 ms. The ramp lies above half the
        // peak for its last 20 ms, so a hold of 55 ms after it lasts long
        // enough and one of 54 ms does not. Above four tenths the ramp
        // would add 4 ms.
        for (hold, expected) in [(0.055, 2), (0.054, 1)] {
            let mono = stem(onset(), |t| {
                let start = -0.09 - hold;
                let level = if t >= 0.0 {
                    0.5
                } else if t >= -0.05 {
                    0.0
                } else {
                    0.5 * ((t - start) / 0.04).clamp(0.0, 1.0)
                };
                (level, 0.0)
            });
            assert_eq!(joined_a2(&mono), expected, "held {hold} s");
        }
    }

    #[test]
    fn the_hold_ends_within_hold_search() {
        // A 20 ms burst over a floor below six tenths of the peak, before a
        // 50 ms gap. Ending 356 ms before the gap, the burst's fall lies
        // within HOLD_SEARCH of the rise, whose low lies a few ms before
        // the onset: the burst holds too briefly and the note joins.
        // Ending 357 ms before it, the fall lies past the search, and the
        // long dip holds.
        for (before, expected) in [(0.356, 1), (0.357, 2)] {
            let mono = stem(onset(), |t| {
                let level = if t >= 0.0 {
                    0.5
                } else if t >= -0.05 {
                    0.0
                } else if (-0.07 - before..-0.05 - before).contains(&t) {
                    0.5
                } else {
                    0.2
                };
                (level, 0.0)
            });
            assert_eq!(joined_a2(&mono), expected, "burst {before} s before");
        }
    }

    #[test]
    fn the_fastest_rise_counts() {
        // A held note, a 25 ms gap and a step 35 ms before the onset, then
        // 15 ms of silence and a slow climb over 45 ms from the onset.
        let mono = stem(onset(), |t| {
            let level = if t < -0.06 {
                0.4
            } else if t < -0.035 {
                0.0
            } else if t < -0.015 {
                0.4
            } else if t < 0.0 {
                0.0
            } else {
                0.4 * (t / 0.045).min(1.0)
            };
            (level, 0.0)
        });
        assert_eq!(joined_a2(&mono), 2);
    }

    #[test]
    fn the_fastest_held_rise_counts() {
        // At A5 a rise may take 10.2 ms. A held note, 30 ms of silence and
        // a slow ramp over 20 ms from 60 ms before the onset, then a 6 ms
        // dip 13 ms after it and a step back. The ramp holds 63 ms before
        // the dip, so both rises count, and the step is the faster.
        let mono = stem_at(880.0, onset(), |t| {
            let level = if t < -0.09 {
                0.5
            } else if t < -0.06 {
                0.0
            } else if t < -0.04 {
                25.0 * (t + 0.06)
            } else if (0.013..0.019).contains(&t) {
                0.0
            } else {
                0.5
            };
            (level, 0.0)
        });
        assert_eq!(joined_at(81.0, &mono), 2);
    }

    #[test]
    fn the_loudest_frame_lies_within_attack_after() {
        // A step to 0.4 at the onset climbs half the way to a step to 1 at
        // 100 ms, but not to one at 40 ms.
        for (late, expected) in [(0.04, 1), (0.1, 2)] {
            let mono = stem(onset(), |t| {
                let level = if t < 0.0 {
                    0.0
                } else if t < late {
                    0.4
                } else {
                    1.0
                };
                (level, 0.0)
            });
            assert_eq!(joined_a2(&mono), expected, "step to 1 at {late} s");
        }
    }

    #[test]
    fn the_quiet_frames_lie_within_attack_before() {
        // 30 ms of silence ending 35 ms before the onset, or 80 ms before.
        for (end, expected) in [(0.035, 2), (0.08, 1)] {
            let mono = stem(onset(), |t| {
                (
                    if (-end - 0.03..-end).contains(&t) {
                        0.0
                    } else {
                        0.5
                    },
                    0.0,
                )
            });
            assert_eq!(joined_a2(&mono), expected, "gap ending {end} s before");
        }
    }

    #[test]
    fn a_note_at_either_end_of_the_stem_reads_only_frames_inside_it() {
        let (run, _) = run_with_onset(0.0);
        let mono = &stem(0.0, |_| (0.5, 0.0))[..11_025];
        let levels = StemLevels::new(mono, rate());
        for (first, second) in [((0.0, 0.01), (0.01, 0.2)), ((0.3, 0.497), (0.497, 0.6))] {
            let notes = NoteList::new(vec![
                note(first.0, first.1, 45.0),
                note(second.0, second.1, 45.0),
            ]);
            assert_eq!(join_splits_with(&notes, &run, &levels).notes().len(), 1);
        }
    }

    #[test]
    fn the_quiet_before_the_held_note_never_counts() {
        let (run, time) = run_with_onset_at(0.0, LEAD_FRAMES + 30);
        // The held note starts from silence; its split piece 46 ms later
        // sees only frames from that start.
        let mono = stem(time(26), |t| (if t < 0.0 { 0.0 } else { 0.5 }, 0.0));
        let notes = NoteList::new(vec![
            note(time(LEAD_FRAMES + 26), time(LEAD_FRAMES + 30), 45.0),
            note(time(LEAD_FRAMES + 30), time(LEAD_FRAMES + 45), 45.0),
        ]);
        assert_eq!(
            join_splits_with(&notes, &run, &StemLevels::new(&mono, rate()))
                .notes()
                .len(),
            1
        );
    }

    /// The onsets, in s from [`onset`], of the pieces of one note of
    /// `pitch` from `from` s to `to` s around it, split over `mono` from
    /// [`stem_at`].
    fn split_at(pitch: f64, mono: &[f64], from: f64, to: f64) -> Vec<f64> {
        let start = f64::from(LEAD_SAMPLES) / 22_050.0 + onset();
        let notes = NoteList::new(vec![note(start + from, start + to, pitch)]);
        split_at_attacks(&notes, &StemLevels::new(mono, rate()))
            .notes()
            .iter()
            .map(|piece| piece.onset().get() - start)
            .collect()
    }

    /// [`stem_at`] `f0` at `before` until 50 ms before [`onset`], silent
    /// to it, and at 0.5 from it.
    fn gap_at(f0: f64, before: f64) -> Vec<f64> {
        stem_at(f0, onset(), |t| {
            let level = if t < -0.05 {
                before
            } else if t < 0.0 {
                0.0
            } else {
                0.5
            };
            (level, 0.0)
        })
    }

    #[test]
    fn a_note_splits_at_the_last_tenth_before_the_climb() {
        // At A2 the window spans 9.1 ms, and its reading falls to a tenth
        // of the peak 0.49 of it, 4.5 ms, before a step from silence; the
        // readings start 12.5 ms before the dip frame, a few ms off that.
        let pieces = split_at(45.0, &gap_at(110.0, 0.5), -0.3, 0.3);
        assert_eq!(pieces.len(), 2);
        assert!(
            (pieces[1] + 0.0053).abs() < 0.0005,
            "split at {}",
            pieces[1]
        );
        assert_eq!(
            split_at(45.0, &stem(onset(), |_| (0.5, 0.0)), -0.3, 0.3).len(),
            1
        );
    }

    #[test]
    fn the_piece_after_a_split_lasts_attack_after() {
        // The split falls 5.3 ms before the onset.
        for (end, expected) in [(0.0452, 2), (0.0442, 1)] {
            assert_eq!(
                split_at(45.0, &gap_at(110.0, 0.5), -0.3, end).len(),
                expected,
                "note to {end} s"
            );
        }
    }

    #[test]
    fn the_piece_before_a_split_reaches_the_second_share_of_hold_span() {
        // Before the gap the stem lies at 0.61 or 0.59 of its level after
        // it. Below six tenths the hold search finds no fall and the long
        // dip holds, so only the piece before keeps it whole.
        for (before, expected) in [(0.305, 2), (0.295, 1)] {
            assert_eq!(
                split_at(45.0, &gap_at(110.0, before), -0.3, 0.3).len(),
                expected,
                "{before} before the gap"
            );
        }
    }

    #[test]
    fn the_piece_before_a_split_is_loud_hold_before_ahead_of_the_dip() {
        // The dip frame, the last silent one, centres 12.5 ms before the
        // onset. From 75 ms before it, the note holds a frame centred
        // HOLD_BEFORE before that; from 70 ms, it holds none.
        for (from, expected) in [(-0.075, 2), (-0.07, 1)] {
            assert_eq!(
                split_at(45.0, &gap_at(110.0, 0.5), from, 0.3).len(),
                expected,
                "note from {from} s"
            );
        }
    }

    #[test]
    fn a_split_needs_an_attack() {
        // At A2 a rise may take 14.3 ms. After a 50 ms gap a linear attack
        // of 12 ms splits and one of 13 ms rises too slowly.
        for (ramp, expected) in [(0.012, 2), (0.013, 1)] {
            let mono = stem(onset(), |t| {
                let level = if t < -0.05 {
                    0.5
                } else {
                    0.5 * (t / ramp).clamp(0.0, 1.0)
                };
                (level, 0.0)
            });
            assert_eq!(
                split_at(45.0, &mono, -0.3, 0.3).len(),
                expected,
                "ramp {ramp} s"
            );
        }
    }

    #[test]
    fn a_quieter_repeat_splits() {
        // After a 25 ms gap the first readings still see the note before
        // it, 6 dB louder, above nine tenths of the peak after the dip;
        // the climb starts at the quietest reading.
        let mono = stem(onset(), |t| {
            let level = if t < -0.025 {
                1.0
            } else if t < 0.0 {
                0.0
            } else {
                0.5
            };
            (level, 0.0)
        });
        let pieces = split_at(45.0, &mono, -0.3, 0.3);
        assert_eq!(pieces.len(), 2);
        assert!(
            (pieces[1] + 0.0053).abs() < 0.0005,
            "split at {}",
            pieces[1]
        );
    }

    /// [`stem`] with its A2 at `low(t)` and a tone at its second harmonic
    /// at `high(t)`.
    fn harmonic_stem(level: impl Fn(f64) -> (f64, f64)) -> Vec<f64> {
        (-LEAD_SAMPLES..22_050)
            .map(|index| {
                let t = f64::from(index) / 22_050.0;
                let (low, high) = level(t - onset());
                low * (std::f64::consts::TAU * 110.0 * t).sin()
                    + high * (std::f64::consts::TAU * 220.0 * t).sin()
            })
            .collect()
    }

    #[test]
    fn the_fundamental_dips_by_fundamental_dip() {
        // An A2 at 0.5 under a tone at its second harmonic at 2, and in the
        // gap the A2 alone: the stem and the harmonics dip past 30 dB. The
        // fundamental reads up to about 0.55 after the gap, where the
        // tone's step leaks into its bin, so 0.0375 lies 23.3 dB below it
        // and 0.04 22.7 dB.
        for (floor, expected) in [(0.0375, 2), (0.04, 1)] {
            let mono = harmonic_stem(|t| {
                if (-0.05..0.0).contains(&t) {
                    (floor, 0.0)
                } else {
                    (0.5, 2.0)
                }
            });
            assert_eq!(
                split_at(45.0, &mono, -0.3, 0.3).len(),
                expected,
                "A2 at {floor} in the gap"
            );
        }
    }

    #[test]
    fn a_last_piece_starts_the_next_note_within_split_gap() {
        // The decoder ends the note 58 ms after the onset and starts the
        // next one of its key 23 or 24 ms later. Within SPLIT_GAP the cut
        // piece starts that note; past it, the piece stays a note.
        let start = f64::from(LEAD_SAMPLES) / 22_050.0 + onset();
        let levels_mono = gap_at(110.0, 0.5);
        let levels = StemLevels::new(&levels_mono, rate());
        for (gap, expected) in [(0.023, 2), (0.024, 3)] {
            let notes = NoteList::new(vec![
                heard(start - 0.3, start + 0.058, 45.0, 80),
                heard(start + 0.058 + gap, start + 0.3, 45.0, 90).with_octave_uncertain(true),
            ]);
            let pieces = split_at_attacks(&notes, &levels);
            assert_eq!(pieces.notes().len(), expected, "gap {gap} s");
            let last = &pieces.notes()[expected - 1];
            if expected == 2 {
                assert!((last.onset().get() - start + 0.0053).abs() < 0.0005);
                assert_eq!(last.velocity(), Some(Velocity::new(90).unwrap()));
                assert_eq!(last.octave_uncertain(), Some(true));
            }
        }
        // A note with no cut keeps the next note as it was.
        let steady = stem(onset(), |_| (0.5, 0.0));
        let notes = NoteList::new(vec![
            heard(start - 0.3, start + 0.058, 45.0, 80),
            heard(start + 0.07, start + 0.3, 45.0, 90),
        ]);
        assert_eq!(
            split_at_attacks(&notes, &StemLevels::new(&steady, rate()))
                .notes()
                .len(),
            2
        );
        // A next note of another key stays as it was.
        let notes = NoteList::new(vec![
            heard(start - 0.3, start + 0.058, 45.0, 80),
            heard(start + 0.07, start + 0.3, 47.0, 90),
        ]);
        assert_eq!(split_at_attacks(&notes, &levels).notes().len(), 3);
    }

    #[test]
    fn a_short_last_piece_stays_before_a_next_note_with_its_own_attack() {
        // A second gap from 60 to 90 ms after the onset: the decoder ends
        // the note 25 ms into it and starts the next note 10 ms later, at
        // 95 ms. The cut piece lasts 90.3 ms, within the 100 ms limit, but
        // the next note has its own attack, so the piece stays a note and
        // the next one keeps its onset.
        let start = f64::from(LEAD_SAMPLES) / 22_050.0 + onset();
        let mono = stem(onset(), |t| {
            let gap = (-0.05..0.0).contains(&t) || (0.06..0.09).contains(&t);
            (if gap { 0.0 } else { 0.5 }, 0.0)
        });
        let notes = NoteList::new(vec![
            heard(start - 0.3, start + 0.085, 45.0, 80),
            heard(start + 0.095, start + 0.4, 45.0, 90),
        ]);
        let pieces = split_at_attacks(&notes, &StemLevels::new(&mono, rate()));
        let onsets: Vec<f64> = pieces
            .notes()
            .iter()
            .map(|piece| piece.onset().get() - start)
            .collect();
        assert_eq!(onsets.len(), 3, "{onsets:?}");
        assert!((onsets[1] + 0.0053).abs() < 0.0005, "{onsets:?}");
        assert!((onsets[2] - 0.095).abs() < 1e-9, "{onsets:?}");
    }

    #[test]
    fn a_long_last_piece_stays_before_a_next_note_with_its_own_attack() {
        // A second gap from 100 to 140 ms after the onset: the decoder ends
        // the note 25 ms into it and starts the next note 20 ms later, at
        // 145 ms. The cut piece lasts 130 ms, past the 100 ms limit, so it
        // stays a note and the next one keeps its onset.
        let start = f64::from(LEAD_SAMPLES) / 22_050.0 + onset();
        let mono = stem(onset(), |t| {
            let gap = (-0.05..0.0).contains(&t) || (0.1..0.14).contains(&t);
            (if gap { 0.0 } else { 0.5 }, 0.0)
        });
        let notes = NoteList::new(vec![
            heard(start - 0.3, start + 0.125, 45.0, 80),
            heard(start + 0.145, start + 0.4, 45.0, 90),
        ]);
        let pieces = split_at_attacks(&notes, &StemLevels::new(&mono, rate()));
        let onsets: Vec<f64> = pieces
            .notes()
            .iter()
            .map(|piece| piece.onset().get() - start)
            .collect();
        assert_eq!(onsets.len(), 3, "{onsets:?}");
        assert!((onsets[1] + 0.0053).abs() < 0.0005, "{onsets:?}");
        assert!((onsets[2] - 0.145).abs() < 1e-9, "{onsets:?}");
    }

    #[test]
    fn a_short_repeat_after_a_short_gap_takes_the_cut_piece() {
        // Gates end 100 ms before the onset and `length` after it, and a
        // third opens 15 ms later, until 450 ms. The decoder starts the
        // repeat 5 ms before the third gate. With a gate of 80 ms the cut
        // piece lasts 85.3 ms, and the attack rule reads no attack after a
        // gap that short, so the piece starts the repeat: 2 notes, and the
        // repeat starts at the cut. A gate of 100 ms gives a piece past
        // the limit, and the repeat keeps its own onset.
        let start = f64::from(LEAD_SAMPLES) / 22_050.0 + onset();
        for (length, expected) in [(0.08, 2), (0.1, 3)] {
            let mono = stem(onset(), |t| {
                let open =
                    t < -0.1 || (0.0..length).contains(&t) || (length + 0.015..0.45).contains(&t);
                (if open { 0.5 } else { 0.0 }, 0.0)
            });
            let notes = NoteList::new(vec![
                heard(start - 0.3, start + length, 45.0, 80),
                heard(start + length + 0.01, start + 0.45, 45.0, 90),
            ]);
            let pieces = split_at_attacks(&notes, &StemLevels::new(&mono, rate()));
            let onsets: Vec<f64> = pieces
                .notes()
                .iter()
                .map(|piece| piece.onset().get() - start)
                .collect();
            assert_eq!(onsets.len(), expected, "gate {length} s: {onsets:?}");
            let last = onsets[expected - 1];
            let onset = if expected == 2 {
                -0.0053
            } else {
                length + 0.01
            };
            assert!((last - onset).abs() < 0.0005, "gate {length} s: {onsets:?}");
        }
    }

    #[test]
    fn a_last_piece_starts_the_next_note_up_to_twice_attack_after() {
        // The cut falls 5.3 ms before the onset, and the decoder ends the
        // note 94 or 96 ms after it: a last piece of 99.3 or 101.3 ms.
        let start = f64::from(LEAD_SAMPLES) / 22_050.0 + onset();
        let mono = gap_at(110.0, 0.5);
        let levels = StemLevels::new(&mono, rate());
        for (end, expected) in [(0.094, 2), (0.096, 3)] {
            let notes = NoteList::new(vec![
                heard(start - 0.3, start + end, 45.0, 80),
                heard(start + end + 0.02, start + 0.4, 45.0, 90),
            ]);
            let pieces = split_at_attacks(&notes, &levels);
            assert_eq!(pieces.notes().len(), expected, "end {end} s");
        }
    }

    #[test]
    fn a_long_last_piece_stays_before_a_repeat_after_a_short_gap() {
        // Gates end 100 ms before the onset and 200 ms after it, and a
        // third opens 20, 15 or 10 ms later, until 450 ms. The decoder
        // reads the first two gates as one note to 200 ms and starts the
        // next 5 ms before the third gate, within SPLIT_GAP. The cut piece
        // lasts 200 ms, and the repeat keeps its own onset.
        let start = f64::from(LEAD_SAMPLES) / 22_050.0 + onset();
        for gap in [0.02, 0.015, 0.01] {
            let mono = stem(onset(), |t| {
                let open = t < -0.1 || (0.0..0.2).contains(&t) || (0.2 + gap..0.45).contains(&t);
                (if open { 0.5 } else { 0.0 }, 0.0)
            });
            let notes = NoteList::new(vec![
                heard(start - 0.3, start + 0.2, 45.0, 80),
                heard(start + 0.2 + gap - 0.005, start + 0.45, 45.0, 90),
            ]);
            let pieces = split_at_attacks(&notes, &StemLevels::new(&mono, rate()));
            let onsets: Vec<f64> = pieces
                .notes()
                .iter()
                .map(|piece| piece.onset().get() - start)
                .collect();
            assert_eq!(onsets.len(), 3, "gap {gap} s: {onsets:?}");
            assert!((onsets[2] - (0.195 + gap)).abs() < 1e-9, "{onsets:?}");
        }
    }

    #[test]
    fn a_note_from_the_stem_start_reads_only_the_stem() {
        // No lead: the note starts on the stem's first frame. A steady stem
        // and a gap 20 ms after the start give no split.
        let lead = usize::try_from(LEAD_SAMPLES).unwrap();
        for mono in [
            stem(onset(), |_| (0.5, 0.0)),
            stem(0.0, |t| {
                (if (0.02..0.07).contains(&t) { 0.0 } else { 0.5 }, 0.0)
            }),
        ] {
            let levels = StemLevels::new(&mono[lead..], rate());
            let notes = NoteList::new(vec![note(0.0, 0.5, 45.0)]);
            assert_eq!(split_at_attacks(&notes, &levels).notes().len(), 1);
        }
    }

    #[test]
    fn every_piece_keeps_the_velocity_and_the_octave_flag() {
        let start = f64::from(LEAD_SAMPLES) / 22_050.0 + onset();
        let mono = gap_at(110.0, 0.5);
        let notes = NoteList::new(vec![
            heard(start - 0.3, start + 0.3, 45.0, 77).with_octave_uncertain(true),
        ]);
        let pieces = split_at_attacks(&notes, &StemLevels::new(&mono, rate()));
        assert_eq!(pieces.notes().len(), 2);
        for piece in pieces.notes() {
            assert_eq!(piece.velocity(), Some(Velocity::new(77).unwrap()));
            assert_eq!(piece.octave_uncertain(), Some(true));
        }
    }

    #[test]
    fn the_rise_window_spans_a_period_and_at_least_rise_window() {
        let at = |pitch: f64| rise_window(MidiPitch::new(pitch).unwrap());
        assert!((at(45.0) - 1.0 / 110.0).abs() < 1e-12);
        // A5 spans 1.1 ms.
        assert_eq!(at(81.0), RISE_WINDOW.get());
    }

    #[test]
    fn notes_split_from_split_lowest() {
        assert_eq!(split_at(33.0, &gap_at(55.0, 0.5), -0.3, 0.3).len(), 2);
        assert_eq!(split_at(32.0, &gap_at(51.913, 0.5), -0.3, 0.3).len(), 1);
        // The same stem read as A1 splits.
        assert_eq!(split_at(33.0, &gap_at(51.913, 0.5), -0.3, 0.3).len(), 2);
    }

    #[test]
    fn two_repeated_notes_with_a_clear_gap_stay_two() {
        let (run, time) = run_with_onset(0.0);
        let notes = NoteList::new(vec![
            note(time(10), time(25), 45.0),
            note(time(30), time(50), 45.0),
        ]);
        assert_eq!(join_splits(&notes, &run).notes().len(), 2);
    }

    #[test]
    fn a_merged_note_takes_the_higher_velocity() {
        let notes = NoteList::new(vec![heard(0.0, 0.5, 45.0, 40), heard(0.2, 0.8, 45.0, 90)]);
        assert_eq!(merge_overlaps(&notes).notes(), &[heard(0.0, 0.8, 45.0, 90)]);
    }

    #[test]
    fn notes_of_one_key_merge_only_when_they_overlap() {
        let notes = NoteList::new(vec![
            note(0.0, 0.5, 45.0),
            note(0.2, 0.8, 45.0),
            note(0.3, 0.4, 52.0),
            note(1.0, 1.5, 45.0),
        ]);
        assert_eq!(
            merge_overlaps(&notes).notes(),
            &[
                note(0.0, 0.8, 45.0),
                note(0.3, 0.4, 52.0),
                note(1.0, 1.5, 45.0)
            ]
        );
    }
}
