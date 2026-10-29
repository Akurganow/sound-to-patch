//! The render core that both sides of the plug-in host share: the notes
//! and controller changes of a job, their sample-frame events, the
//! [`Recipe`] and the wire protocol of the host process.
//!
//! The library renders through these types, and the host process plays
//! them, so both sides render by one rule.

use std::error::Error;
use std::fmt;

use crate::audio::SampleRate;
use crate::cast;
use crate::notes::Velocity;
use crate::units::Seconds;

pub mod recipe;
pub mod wire;

pub use recipe::{
    ClipEnd, Material, Oversampling, PreRoll, ProcessMode, RECIPE_VERSION, Recipe, TailWatch,
};

/// The highest MIDI key.
const MAX_KEY: u8 = 127;
/// The latest time a note may end: 10 minutes. A clip holds every sample
/// in memory, and a longer part than a stem takes splits into several
/// renders.
pub const MAX_NOTE_END: Seconds = Seconds::from_const(600.0);

/// One note to play: a MIDI key, its start and end in the clip and its
/// velocity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderNote {
    key: u8,
    onset: Seconds,
    offset: Seconds,
    velocity: Velocity,
}

impl RenderNote {
    /// Checks the key, that the note ends after it starts and that it ends
    /// by [`MAX_NOTE_END`].
    ///
    /// # Errors
    ///
    /// Returns a [`PlayError`] for a key above 127, an offset that is not
    /// after the onset or an offset after [`MAX_NOTE_END`].
    pub fn new(
        key: u8,
        onset: Seconds,
        offset: Seconds,
        velocity: Velocity,
    ) -> Result<Self, PlayError> {
        if key > MAX_KEY {
            return Err(PlayError::Note(format!(
                "key {key} is above {MAX_KEY}; pass keys 0 to {MAX_KEY}"
            )));
        }
        if offset <= onset {
            return Err(PlayError::Note(format!(
                "key {key} ends at {offset}, not after its start at {onset}; end each note after it starts"
            )));
        }
        if offset > MAX_NOTE_END {
            return Err(PlayError::Note(format!(
                "key {key} ends at {offset}, after the longest clip of {MAX_NOTE_END}; split the part into several renders"
            )));
        }
        Ok(Self {
            key,
            onset,
            offset,
            velocity,
        })
    }

    /// Returns the MIDI key.
    #[must_use]
    pub fn key(self) -> u8 {
        self.key
    }

    /// Returns the start in the clip.
    #[must_use]
    pub fn onset(self) -> Seconds {
        self.onset
    }

    /// Returns the end in the clip.
    #[must_use]
    pub fn offset(self) -> Seconds {
        self.offset
    }

    /// Returns the velocity.
    #[must_use]
    pub fn velocity(self) -> Velocity {
        self.velocity
    }
}

/// Whether a note event starts or ends a note.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventKind {
    /// The note ends. It sorts first, so a key that ends and starts on one
    /// frame plays again.
    Off,
    /// The note starts.
    On,
}

/// A note-on or note-off at a sample frame of the clip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoteEvent {
    /// The frame, `round(t × rate)`.
    pub frame: usize,
    /// Start or end.
    pub kind: EventKind,
    /// The MIDI key.
    pub key: u8,
    /// The velocity of the note.
    pub velocity: Velocity,
}

/// Turns notes into events sorted by frame, a note-off before a note-on on
/// the same frame.
///
/// A frame is the note time times the rate, rounded to the nearest sample.
/// A note shorter than half a frame still lasts one frame, so its note-off
/// never comes before its own note-on.
#[must_use]
pub fn note_events(notes: &[RenderNote], rate: SampleRate) -> Vec<NoteEvent> {
    let mut events: Vec<NoteEvent> = notes
        .iter()
        .flat_map(|note| {
            let on = frame_of(note.onset, rate);
            let off = frame_of(note.offset, rate).max(on + 1);
            [(on, EventKind::On), (off, EventKind::Off)].map(|(frame, kind)| NoteEvent {
                frame,
                kind,
                key: note.key,
                velocity: note.velocity,
            })
        })
        .collect();
    events.sort_by_key(|event| (event.frame, event.kind, event.key));
    events
}

/// The frame of a time: the time times the rate, rounded to the nearest
/// sample.
///
/// A time too large for a frame index gives `usize::MAX`; note times are at
/// most [`MAX_NOTE_END`], so the fallback does not act on them.
#[must_use]
pub fn frame_of(time: Seconds, rate: SampleRate) -> usize {
    cast::round_to_usize(time.get() * f64::from(rate.get())).unwrap_or(usize::MAX)
}

/// A MIDI controller change at a time of the clip. A plug-in in a VST3 host
/// gets controllers only through its MIDI mapping; a controller it does not
/// map is dropped without an error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlKind {
    /// Pitch bend, 0 to 16383 with the centre at 8192.
    PitchBend(u16),
    /// The mod wheel, MIDI controller 1, 0 to 127.
    ModWheel(u8),
}

/// The largest pitch bend value: 14 bits.
pub const MAX_PITCH_BEND: u16 = 16_383;
/// The largest value of a MIDI controller: 7 bits.
pub const MAX_CONTROLLER: u8 = 127;

impl ControlKind {
    /// Returns the change when its value fits its MIDI message.
    ///
    /// # Errors
    ///
    /// Returns [`PlayError::Control`] for a pitch bend above
    /// [`MAX_PITCH_BEND`] or a mod wheel above [`MAX_CONTROLLER`].
    pub fn checked(self) -> Result<Self, PlayError> {
        match self {
            Self::PitchBend(value) if value > MAX_PITCH_BEND => Err(PlayError::Control(format!(
                "pitch bend {value} is above {MAX_PITCH_BEND}; pass 0 to {MAX_PITCH_BEND}, with the centre at 8192"
            ))),
            Self::ModWheel(value) if value > MAX_CONTROLLER => Err(PlayError::Control(format!(
                "mod wheel {value} is above {MAX_CONTROLLER}; pass 0 to {MAX_CONTROLLER}"
            ))),
            kind => Ok(kind),
        }
    }
}

/// A note or a controller change that cannot be played.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayError {
    /// A note cannot be played.
    Note(String),
    /// A controller change does not fit its MIDI message.
    Control(String),
}

impl fmt::Display for PlayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Note(cause) => write!(f, "cannot play a note: {cause}"),
            Self::Control(cause) => write!(f, "cannot send a controller change: {cause}"),
        }
    }
}

impl Error for PlayError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(value: f64) -> Seconds {
        Seconds::new(value).unwrap()
    }

    fn v(value: u8) -> Velocity {
        Velocity::new(value).unwrap()
    }

    #[test]
    fn a_note_must_end_after_it_starts_and_fit_the_key_range() {
        let error = RenderNote::new(60, s(1.0), s(1.0), v(100)).unwrap_err();
        assert!(
            error.to_string().contains("end each note after it starts"),
            "{error}"
        );
        let error = RenderNote::new(128, s(0.0), s(1.0), v(100)).unwrap_err();
        assert!(error.to_string().contains("pass keys 0 to 127"), "{error}");
        assert!(RenderNote::new(127, s(0.0), s(1.0), v(100)).is_ok());
    }

    #[test]
    fn a_note_must_end_by_the_longest_clip() {
        assert_eq!(Seconds::new(MAX_NOTE_END.get()).ok(), Some(MAX_NOTE_END));
        assert!(RenderNote::new(60, s(0.0), MAX_NOTE_END, v(100)).is_ok());
        let error = RenderNote::new(60, s(0.0), s(600.001), v(100)).unwrap_err();
        assert!(
            error
                .to_string()
                .ends_with("split the part into several renders"),
            "{error}"
        );
        assert!(RenderNote::new(60, s(0.0), s(1e300), v(100)).is_err());
    }

    fn frames(notes: &[RenderNote]) -> Vec<(usize, EventKind)> {
        note_events(notes, SampleRate::new(44_100).unwrap())
            .iter()
            .map(|e| (e.frame, e.kind))
            .collect()
    }

    #[test]
    fn a_note_shorter_than_half_a_frame_still_lasts_one_frame() {
        let note = RenderNote::new(60, s(0.0), s(0.000_01), v(100)).unwrap();
        assert_eq!(frames(&[note]), [(0, EventKind::On), (1, EventKind::Off)]);
    }

    #[test]
    fn a_chain_of_one_key_keeps_every_off_after_its_on() {
        let notes = [
            RenderNote::new(60, s(0.0), s(1.0), v(100)).unwrap(),
            RenderNote::new(60, s(1.0), s(1.000_01), v(100)).unwrap(),
            RenderNote::new(60, s(1.000_01), s(2.0), v(100)).unwrap(),
        ];
        // The second note rounds to frame 44 100 at both ends and lasts one
        // frame; the third starts on frame 44 100 too.
        assert_eq!(
            frames(&notes),
            [
                (0, EventKind::On),
                (44_100, EventKind::Off),
                (44_100, EventKind::On),
                (44_100, EventKind::On),
                (44_101, EventKind::Off),
                (88_200, EventKind::Off),
            ]
        );
    }

    #[test]
    fn events_round_to_the_nearest_frame_and_put_offs_first() {
        let rate = SampleRate::new(44_100).unwrap();
        let notes = [
            RenderNote::new(60, s(0.5), s(1.0), v(100)).unwrap(),
            RenderNote::new(60, s(1.0), s(1.25), v(90)).unwrap(),
        ];
        let events = note_events(&notes, rate);
        let frames: Vec<(usize, EventKind)> = events.iter().map(|e| (e.frame, e.kind)).collect();
        assert_eq!(
            frames,
            [
                (22_050, EventKind::On),
                (44_100, EventKind::Off),
                (44_100, EventKind::On),
                (55_125, EventKind::Off),
            ]
        );
        // 1/3 s is 14 700 frames exactly; 0.1 ms is 4.41 frames and rounds to 4.
        let odd = [RenderNote::new(36, s(0.0001), s(1.0 / 3.0), v(1)).unwrap()];
        let frames: Vec<usize> = note_events(&odd, rate).iter().map(|e| e.frame).collect();
        assert_eq!(frames, [4, 14_700]);
    }

    #[test]
    fn a_controller_value_must_fit_its_midi_message() {
        let error = ControlKind::PitchBend(MAX_PITCH_BEND + 1)
            .checked()
            .unwrap_err();
        assert!(error.to_string().contains("pass 0 to 16383"), "{error}");
        let error = ControlKind::ModWheel(128).checked().unwrap_err();
        assert!(error.to_string().contains("pass 0 to 127"), "{error}");
        assert!(ControlKind::PitchBend(MAX_PITCH_BEND).checked().is_ok());
        assert!(ControlKind::ModWheel(MAX_CONTROLLER).checked().is_ok());
    }
}
