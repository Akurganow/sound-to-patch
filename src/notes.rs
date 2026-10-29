//! The note list that every layer reads, the transcriber, the f0 track and
//! the MIDI files that produce and store it, and the harmonic masks built
//! from it.
//!
//! A note has an onset, an offset, a pitch and, once a pitch track has run,
//! an f0 contour. Three more fields stay unset until a later step fills
//! them: the octave flag, the velocity and the overlap flag. The
//! transcriber and the MIDI reader both produce this type, and every later
//! step consumes it.
//!
//! The transcriber runs the model once per stem ([`Transcriber::run`]) and
//! decodes that run with [`DecodeSettings`], as many times as needed
//! ([`ModelRun::decode`]). A [`Profile`] gives fixed settings per kind of
//! stem; [`detect()`] derives them from the stem instead, from a probe decode
//! and the [`PitchTrack`], the stem's f0 track, which also fills the note
//! contours. [`drop_quiet`] drops notes that sound only far below the
//! stem's loudest passage. [`write_midi`] stores a list as a format 0
//! file.

use std::error::Error;
use std::fmt;

use crate::units::{Hz, MidiPitch, Seconds};

mod cleanup;
mod detect;
mod mask;
mod midi;
mod profile;
mod pyin;
mod quiet;
mod stem;
mod track;
mod transcribe;
mod write;

pub use cleanup::{
    ATTACK_AFTER, ATTACK_BEFORE, ATTACK_DIP, COPY_SLACK, HARMONIC_DIP, HOLD_BEFORE, HOLD_RATIO,
    HOLD_SEARCH, HOLD_SPAN, OCTAVE_AGREEMENT, OCTAVE_FIX_CEILING, OCTAVE_MIN_FRAMES,
    OCTAVE_TOLERANCE, OctaveCheck, RISE_LATEST, RISE_READ, RISE_SLACK, RISE_SPAN, RISE_STEP,
    RISE_WINDOW, SPLIT_GAP, SPLIT_ONSET, check_octaves, join_splits, join_splits_with,
    merge_overlaps,
};
pub use detect::{
    CHORD_OVERLAP, CHORD_SHARE, Detection, HOLD_WINDOW, REFERENCE_PERCENTILE, REGISTER_MARGIN,
    REGISTER_PERCENTILES, REGISTER_WINDOW, RegisterSource, Voicing, detect, detect_with_window,
    probe_settings,
};
pub use mask::{FrameMask, MaskParams, Masker, out_of_mask_fraction};
pub use midi::{
    BPM_TOLERANCE, DRUM_CHANNEL, MidiError, read_midi, read_midi_bytes, read_midi_near,
};
pub use profile::{
    DEFAULT_MIN_NOTE_FRAMES, DecodeSettings, HIGHEST_MODEL_PITCH, LOWEST_MODEL_PITCH, PitchRange,
    Profile, RangeError, min_note_frames,
};
pub use quiet::{HARMONICS, QUIET_FRAME, QUIET_GATE, StemLevels, drop_quiet, drop_quiet_with};
pub use stem::{
    CleanupSteps, NotesError, TRANSCRIBER_VERSION, Transcription, TranscriptionRecord, clean_up,
    transcribe_stem,
};
pub use track::{
    PitchTrack, TRACK_FMAX, TRACK_FMIN, TRACK_FRAME, TRACK_HOP, TrackError, TrackFrame,
};
pub use transcribe::{
    Backend, MODEL_RATE, MODEL_SHA256, ModelRun, Tracker, TranscribeError, Transcriber,
};
pub use write::{
    DEFAULT_VELOCITY, MIN_NOTE_LENGTH, MidiWriteError, TICKS_PER_BEAT, Velocities, midi_track_name,
    write_midi, write_named_midi,
};

/// A MIDI velocity from 1 to 127. Velocity 0 means a note-off in MIDI, so
/// it is not a velocity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Velocity(u8);

impl Velocity {
    /// Checks the range 1 to 127.
    ///
    /// # Errors
    ///
    /// Returns a [`NoteError`] for 0 or a value above 127.
    pub fn new(value: u8) -> Result<Self, NoteError> {
        if (1..=127).contains(&value) {
            Ok(Self(value))
        } else {
            Err(NoteError::Velocity(value))
        }
    }

    /// Builds a velocity in a `const` item. Every caller passes a literal
    /// from 1 to 127 that a test also builds through [`Self::new`].
    pub(crate) const fn from_const(value: u8) -> Self {
        Self(value)
    }

    /// Returns the velocity.
    #[must_use]
    pub fn get(self) -> u8 {
        self.0
    }
}

/// One point of an f0 contour.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContourPoint {
    /// The time from the start of the stem.
    pub time: Seconds,
    /// The fundamental at that time.
    pub f0: Hz,
}

/// The f0 of a note over time, so masks follow bends and glides. Points
/// are sorted by time; between two points the pitch moves linearly in
/// log-frequency, and outside them it holds the end value.
#[derive(Clone, Debug, PartialEq)]
pub struct Contour {
    points: Vec<ContourPoint>,
}

impl Contour {
    /// Sorts the points by time and checks them.
    ///
    /// # Errors
    ///
    /// Returns a [`NoteError`] for no point, a zero f0 or two points at the
    /// same time.
    pub fn new(mut points: Vec<ContourPoint>) -> Result<Self, NoteError> {
        if points.is_empty() {
            return Err(NoteError::EmptyContour);
        }
        if points.iter().any(|point| point.f0.get() <= 0.0) {
            return Err(NoteError::ZeroF0);
        }
        points.sort_by(|a, b| a.time.get().total_cmp(&b.time.get()));
        if points.windows(2).any(|pair| pair[0].time == pair[1].time) {
            return Err(NoteError::DuplicateTime);
        }
        Ok(Self { points })
    }

    /// Returns the points in time order.
    #[must_use]
    pub fn points(&self) -> &[ContourPoint] {
        &self.points
    }

    /// Returns the f0 at a time.
    #[must_use]
    pub fn f0_at(&self, time: Seconds) -> Hz {
        let t = time.get();
        let after = self.points.partition_point(|point| point.time.get() <= t);
        match (
            after.checked_sub(1).and_then(|i| self.points.get(i)),
            self.points.get(after),
        ) {
            (Some(before), Some(next)) if before.time.get() < t => {
                let span = next.time.get() - before.time.get();
                let fraction = (t - before.time.get()) / span;
                let log = before.f0.get().ln() * (1.0 - fraction) + next.f0.get().ln() * fraction;
                Hz::clamped(log.exp())
            }
            (Some(only), _) | (None, Some(only)) => only.f0,
            (None, None) => Hz::clamped(0.0),
        }
    }
}

/// One note: onset, offset, pitch, and the fields later steps fill.
#[derive(Clone, Debug, PartialEq)]
pub struct Note {
    onset: Seconds,
    offset: Seconds,
    pitch: MidiPitch,
    contour: Option<Contour>,
    octave_uncertain: Option<bool>,
    velocity: Option<Velocity>,
    overlap: Option<bool>,
}

impl Note {
    /// Builds a note with every optional field unset.
    ///
    /// # Errors
    ///
    /// Returns a [`NoteError`] when the offset is not after the onset.
    pub fn new(onset: Seconds, offset: Seconds, pitch: MidiPitch) -> Result<Self, NoteError> {
        if offset <= onset {
            return Err(NoteError::OffsetNotAfterOnset { onset, offset });
        }
        Ok(Self {
            onset,
            offset,
            pitch,
            contour: None,
            octave_uncertain: None,
            velocity: None,
            overlap: None,
        })
    }

    /// Sets the f0 contour.
    #[must_use]
    pub fn with_contour(self, contour: Contour) -> Self {
        Self {
            contour: Some(contour),
            ..self
        }
    }

    /// Sets whether the pitch may be an octave off.
    #[must_use]
    pub fn with_octave_uncertain(self, flag: bool) -> Self {
        Self {
            octave_uncertain: Some(flag),
            ..self
        }
    }

    /// Sets the velocity.
    #[must_use]
    pub fn with_velocity(self, velocity: Velocity) -> Self {
        Self {
            velocity: Some(velocity),
            ..self
        }
    }

    /// Sets whether the note overlaps another.
    #[must_use]
    pub fn with_overlap(self, flag: bool) -> Self {
        Self {
            overlap: Some(flag),
            ..self
        }
    }

    /// Returns the onset.
    #[must_use]
    pub fn onset(&self) -> Seconds {
        self.onset
    }

    /// Returns the offset.
    #[must_use]
    pub fn offset(&self) -> Seconds {
        self.offset
    }

    /// Returns the pitch.
    #[must_use]
    pub fn pitch(&self) -> MidiPitch {
        self.pitch
    }

    /// Returns the f0 contour, if a pitch track filled it.
    #[must_use]
    pub fn contour(&self) -> Option<&Contour> {
        self.contour.as_ref()
    }

    /// Returns the octave flag, if set.
    #[must_use]
    pub fn octave_uncertain(&self) -> Option<bool> {
        self.octave_uncertain
    }

    /// Returns the velocity, if set.
    #[must_use]
    pub fn velocity(&self) -> Option<Velocity> {
        self.velocity
    }

    /// Returns the overlap flag, if set.
    #[must_use]
    pub fn overlap(&self) -> Option<bool> {
        self.overlap
    }

    /// Returns whether the note sounds at a time: onset inclusive, offset
    /// exclusive.
    #[must_use]
    pub fn sounds_at(&self, time: Seconds) -> bool {
        self.onset <= time && time < self.offset
    }

    /// Returns the fundamental at a time: the contour when set, else the
    /// pitch as a constant.
    #[must_use]
    pub fn f0_at(&self, time: Seconds) -> Hz {
        self.contour
            .as_ref()
            .map_or_else(|| Hz::from(self.pitch), |contour| contour.f0_at(time))
    }
}

/// The notes of a stem, sorted by onset.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NoteList {
    notes: Vec<Note>,
}

impl NoteList {
    /// Sorts the notes by onset, then by pitch.
    #[must_use]
    pub fn new(mut notes: Vec<Note>) -> Self {
        notes.sort_by(|a, b| {
            a.onset
                .get()
                .total_cmp(&b.onset.get())
                .then(a.pitch.get().total_cmp(&b.pitch.get()))
        });
        Self { notes }
    }

    /// Returns the notes in order.
    #[must_use]
    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    /// Returns whether the list holds no note.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    /// Sets the overlap flag of every note: true when another note sounds
    /// during part of it.
    #[must_use]
    pub fn with_overlaps(self) -> Self {
        // The notes are sorted by onset, so an earlier note overlaps when its
        // offset passes this onset, and a later one when the next onset comes
        // before this offset.
        let mut latest_offset = f64::NEG_INFINITY;
        let flags: Vec<bool> = self
            .notes
            .iter()
            .enumerate()
            .map(|(i, note)| {
                let before = latest_offset > note.onset.get();
                let after = self
                    .notes
                    .get(i + 1)
                    .is_some_and(|next| next.onset < note.offset);
                latest_offset = latest_offset.max(note.offset.get());
                before || after
            })
            .collect();
        Self {
            notes: self
                .notes
                .into_iter()
                .zip(flags)
                .map(|(note, flag)| note.with_overlap(flag))
                .collect(),
        }
    }
}

/// A note or contour that breaks a rule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NoteError {
    /// The offset is at or before the onset.
    OffsetNotAfterOnset {
        /// The onset.
        onset: Seconds,
        /// The offset.
        offset: Seconds,
    },
    /// A velocity outside 1 to 127.
    Velocity(u8),
    /// A contour with no point.
    EmptyContour,
    /// A contour point with an f0 of 0 Hz.
    ZeroF0,
    /// Two contour points at the same time.
    DuplicateTime,
}

impl fmt::Display for NoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OffsetNotAfterOnset { onset, offset } => write!(
                f,
                "a note ends at {offset} but starts at {onset}; the offset must come after the onset"
            ),
            Self::Velocity(value) => write!(f, "velocity {value} is outside 1 to 127"),
            Self::EmptyContour => write!(f, "an f0 contour needs at least one point"),
            Self::ZeroF0 => write!(
                f,
                "an f0 contour point is at 0 Hz; every f0 must be above 0 Hz"
            ),
            Self::DuplicateTime => write!(f, "two f0 contour points share a time"),
        }
    }
}

impl Error for NoteError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(value: f64) -> Seconds {
        Seconds::new(value).unwrap()
    }

    fn note(onset: f64, offset: f64, pitch: f64) -> Note {
        Note::new(s(onset), s(offset), MidiPitch::new(pitch).unwrap()).unwrap()
    }

    proptest::proptest! {
        #[test]
        fn overlap_flags_match_every_pair(
            spans in proptest::collection::vec((0u8..40, 1u8..10), 0..30)
        ) {
            let list = NoteList::new(
                spans
                    .iter()
                    .map(|&(start, len)| {
                        let onset = f64::from(start) / 4.0;
                        note(onset, onset + f64::from(len) / 4.0, 60.0)
                    })
                    .collect(),
            )
            .with_overlaps();
            let notes = list.notes();
            for (i, a) in notes.iter().enumerate() {
                let expected = notes.iter().enumerate().any(|(j, b)| {
                    i != j && b.onset() < a.offset() && a.onset() < b.offset()
                });
                proptest::prop_assert_eq!(a.overlap(), Some(expected));
            }
        }
    }

    #[test]
    fn a_new_note_reads_its_optional_fields_as_unset() {
        let note = note(0.0, 1.0, 45.0);
        assert_eq!(note.octave_uncertain(), None);
        assert_eq!(note.velocity(), None);
        assert_eq!(note.overlap(), None);
        assert_eq!(note.contour(), None);
        let set = note
            .with_octave_uncertain(true)
            .with_velocity(Velocity::new(100).unwrap())
            .with_overlap(false);
        assert_eq!(set.octave_uncertain(), Some(true));
        assert_eq!(set.velocity().map(Velocity::get), Some(100));
        assert_eq!(set.overlap(), Some(false));
    }

    #[test]
    fn a_note_needs_its_offset_after_its_onset() {
        let error = Note::new(s(1.0), s(1.0), MidiPitch::new(60.0).unwrap()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "a note ends at 1 s but starts at 1 s; the offset must come after the onset"
        );
        assert!(Velocity::new(0).is_err());
        assert!(Velocity::new(128).is_err());
    }

    #[test]
    fn without_a_contour_the_f0_is_the_pitch() {
        let a2 = note(0.0, 1.0, 45.0);
        assert!((a2.f0_at(s(0.5)).get() - 110.0).abs() < 1e-9);
    }

    #[test]
    fn a_contour_moves_in_log_frequency_and_holds_its_ends() {
        let point = |time, f0| ContourPoint {
            time: s(time),
            f0: Hz::new(f0).unwrap(),
        };
        let contour = Contour::new(vec![point(1.0, 220.0), point(0.0, 110.0)]).unwrap();
        assert!((contour.f0_at(s(0.5)).get() - 110.0 * 2.0_f64.sqrt()).abs() < 1e-9);
        assert_eq!(contour.f0_at(s(2.0)).get(), 220.0);
        assert_eq!(contour.f0_at(s(0.0)).get(), 110.0);
        let later = Contour::new(vec![point(1.0, 110.0), point(3.0, 220.0)]).unwrap();
        assert!((later.f0_at(s(1.5)).get() - 110.0 * 2.0_f64.powf(0.25)).abs() < 1e-9);
        assert!(Contour::new(vec![]).is_err());
        assert!(Contour::new(vec![point(0.0, 110.0), point(0.0, 220.0)]).is_err());
    }

    #[test]
    fn overlaps_mark_notes_that_share_time() {
        let list = NoteList::new(vec![
            note(1.0, 2.0, 60.0),
            note(0.0, 1.5, 48.0),
            note(3.0, 4.0, 50.0),
        ])
        .with_overlaps();
        let flags: Vec<_> = list.notes().iter().map(Note::overlap).collect();
        assert_eq!(flags, vec![Some(true), Some(true), Some(false)]);
        assert_eq!(list.notes()[0].onset(), s(0.0));
    }
}
