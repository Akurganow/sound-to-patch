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

/// Joins every note that starts within [`SPLIT_GAP`] of the end of a note
/// of the same key, on an onset activation below [`SPLIT_ONSET`], into
/// that note, when its velocity is no higher than that note's.
///
/// A repeated note starts on a strong onset and stays apart. A split piece
/// is the weaker rest of a held note; a weaker piece before a stronger note
/// is an early fragment of the attack, and joining it would move the
/// onset.
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
                && note.velocity() <= kept.velocity()
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
