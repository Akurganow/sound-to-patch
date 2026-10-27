//! The cleanup of decoded notes: the octave check against the f0 track,
//! the merge of octave ghosts into their notes, and the join of split
//! notes.
//!
//! The decoder adds a short copy an octave above a low note, and starts a
//! second note at the same key when the onset activation peaks again
//! inside a held note. A note an octave above the f0 track's reading, over
//! a note of the lower key, is such a copy: it moves down, overlaps its
//! true note and merges into it. The track alone moves nothing, because
//! it can read a true note an octave low. A note that starts where an earlier
//! one of the same key ends, on a weak onset, is the rest of that note.

use super::{ModelRun, Note, NoteList, PitchTrack};
use crate::units::{MidiPitch, Seconds, Semitones};

/// How far a note may lie from the f0 track's median and still agree.
pub const OCTAVE_AGREEMENT: Semitones = Semitones::from_const(1.0);
/// How close to an octave a gap must lie to count as one.
pub const OCTAVE_TOLERANCE: Semitones = Semitones::from_const(0.5);
/// The highest f0 median, A2 (110 Hz), at which a note an octave above it
/// moves down. The ghosts are measured at and below it; above it a gap of
/// an octave only sets the flag.
pub const OCTAVE_FIX_CEILING: MidiPitch = MidiPitch::from_const(45.0);
/// The share of a note's span that a note one octave below must share
/// with it before the note moves down. The track can read the lower octave
/// of a true note, so its median alone moves nothing.
pub const COPY_SHARE: f64 = 0.5;
/// The fewest voiced frames, 17 ms of track, that check a note.
pub const OCTAVE_MIN_FRAMES: usize = 3;
/// The onset activation below which a note that starts where a note of
/// the same key ends joins it. Splits on the generated set peak at 0.65
/// or less, repeated notes at 0.77 or more.
pub const SPLIT_ONSET: f64 = 0.7;
/// The widest gap a split may leave: two model frames.
pub const SPLIT_GAP: Seconds = Seconds::from_const(2.0 * 256.0 / 22_050.0);

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
/// when the median lies at or below [`OCTAVE_FIX_CEILING`] and a note of
/// that lower key shares [`COPY_SHARE`] of its span. Any other gap keeps
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
                && has_lower_octave(notes, note)
                && let Some(moved) = moved_down(note)
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

/// Whether a note one octave below `note` sounds for at least
/// [`COPY_SHARE`] of its span: the model heard the lower note too.
fn has_lower_octave(notes: &NoteList, note: &Note) -> bool {
    let span = note.offset().get() - note.onset().get();
    notes.notes().iter().any(|other| {
        let shared = other.offset().get().min(note.offset().get())
            - other.onset().get().max(note.onset().get());
        (note.pitch().get() - other.pitch().get() - 12.0).abs() < 0.5 && shared >= COPY_SHARE * span
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

/// The note an octave lower, with its other fields kept and its flag
/// cleared.
fn moved_down(note: &Note) -> Option<Note> {
    let pitch = MidiPitch::new(note.pitch().get() - 12.0).ok()?;
    let mut moved = Note::new(note.onset(), note.offset(), pitch)
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
/// disappears into it. The merged note keeps the fields of the note that
/// starts first, and the higher velocity.
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

/// Joins every note that starts within [`SPLIT_GAP`] of the end of a note
/// of the same key, on an onset activation below [`SPLIT_ONSET`], into
/// that note. A repeated note starts on a strong onset and stays apart.
#[must_use]
pub fn join_splits(notes: &NoteList, run: &ModelRun) -> NoteList {
    let mut joined_notes: Vec<Note> = Vec::with_capacity(notes.notes().len());
    for note in notes.notes() {
        let weak = run
            .onset_strength(note.onset(), note.pitch())
            .is_some_and(|strength| strength < SPLIT_ONSET);
        let earlier = joined_notes.iter_mut().rev().find(|kept| {
            same_key(kept, note)
                && kept.offset() <= note.onset()
                && note.onset().get() - kept.offset().get() <= SPLIT_GAP.get()
        });
        match earlier {
            Some(kept) if weak => *kept = joined(kept, note),
            _ => joined_notes.push(note.clone()),
        }
    }
    NoteList::new(joined_notes)
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
    let Ok(mut note) = Note::new(first.onset(), offset, first.pitch()) else {
        return first.clone();
    };
    if let Some(flag) = first.octave_uncertain() {
        note = note.with_octave_uncertain(flag);
    }
    if let Some(velocity) = first.velocity().max(second.velocity()) {
        note = note.with_velocity(velocity);
    }
    note
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::TrackFrame;
    use crate::units::{Decibels, Hz};

    fn s(value: f64) -> Seconds {
        Seconds::new(value).unwrap()
    }

    fn note(onset: f64, offset: f64, pitch: f64) -> Note {
        Note::new(s(onset), s(offset), MidiPitch::new(pitch).unwrap()).unwrap()
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
        let notes = NoteList::new(vec![note(0.5, 1.0, 45.0), note(0.5, 0.6, 57.0)]);
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
            &[note(0.5, 1.0, 45.0).with_octave_uncertain(false)]
        );
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
            let notes = NoteList::new(vec![note(0.45, 1.0, pitch - 12.0), note(0.5, 1.0, pitch)]);
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
    fn a_note_with_too_few_voiced_frames_stays_unchecked() {
        let track = PitchTrack::from_frames(
            (0..2)
                .map(|i| TrackFrame {
                    time: s(0.5 + f64::from(i) * 0.005),
                    f0: Some(Hz::from(MidiPitch::new(45.0).unwrap())),
                    level: Decibels::new(-3.0).unwrap(),
                })
                .collect(),
        );
        let (checked, check) = check_octaves(&NoteList::new(vec![note(0.5, 1.0, 57.0)]), &track);
        assert_eq!(checked.notes()[0].octave_uncertain(), None);
        assert_eq!(check.unchecked, 1);
    }

    /// A model run of 80 frames with an onset of `strength` at A2 on
    /// frame 30, and the time of a frame.
    fn run_with_onset(strength: f32) -> (ModelRun, impl Fn(usize) -> f64) {
        use basic_pitch::{Posteriorgram, Posteriorgrams};
        let (frames, bin) = (80, 45 - 21);
        let mut onsets = Posteriorgram::zeros(frames, 88);
        onsets.as_mut_slice()[30 * 88 + bin] = strength;
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

    #[test]
    fn a_split_on_a_weak_onset_joins_and_a_repeat_stays() {
        for (strength, expected) in [(0.6, 1), (0.69, 1), (0.71, 2), (0.9, 2)] {
            let (run, time) = run_with_onset(strength);
            let notes = NoteList::new(vec![
                note(time(10), time(30), 45.0),
                note(time(30), time(50), 45.0),
            ]);
            let joined = join_splits(&notes, &run);
            assert_eq!(joined.notes().len(), expected, "onset {strength}");
            if expected == 1 {
                assert_eq!(joined.notes()[0], note(time(10), time(50), 45.0));
            }
        }
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
