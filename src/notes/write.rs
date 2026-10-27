//! Writing a note list as a Standard MIDI File.
//!
//! The file is format 0: one track holds the tempo, a 4/4 time signature
//! and every note on channel 1. A DAW opens it as one part at the stem's
//! tempo. Bar lines assume 4/4, because the input gives only a BPM.

use std::error::Error;
use std::fmt;

use midly::num::{u4, u7, u15, u24, u28};
use midly::{Format, Header, MetaMessage, MidiMessage, Timing, TrackEvent, TrackEventKind};

use super::{NoteList, Velocity};
use crate::cast;
use crate::units::{Bpm, Seconds};

/// The resolution of the file in ticks per quarter note.
pub const TICKS_PER_BEAT: u16 = 480;

/// The velocity of a note that carries none, and of every note under
/// [`Velocities::Constant`] with the default.
pub const DEFAULT_VELOCITY: Velocity = Velocity(100);

/// The shortest note the file holds. A shorter note is lengthened to it,
/// and to at least one tick.
pub const MIN_NOTE_LENGTH: Seconds = Seconds::from_const(0.010);

/// Which velocity each note gets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Velocities {
    /// Every note gets this one value. The transcriber's velocity is a
    /// confidence, not a loudness, so the audio path writes a constant.
    Constant(Velocity),
    /// A note gets its own velocity, and a note without one gets this
    /// value.
    FromNotes(Velocity),
}

/// Encodes the notes as a format 0 file at a BPM.
///
/// Times become ticks with `round(seconds * BPM / 60 * 480)`, so each lies
/// within half a tick of the note. The tempo event at tick 0 holds
/// `round(60,000,000 / BPM)` microseconds per beat. Three rules any player
/// needs come first: a note shorter than [`MIN_NOTE_LENGTH`] or one tick
/// is lengthened, a note that still sounds when the same pitch starts again
/// ends there, and a note left with no length is dropped. At one tick a
/// note-off comes before a note-on, so two touching notes of one pitch
/// stay two. The file has no program change and no pitch bend.
///
/// # Errors
///
/// Returns a [`MidiWriteError`] when two events lie further apart than a
/// MIDI delta time holds, about nine hours at 1000 BPM.
pub fn write_midi(
    notes: &NoteList,
    bpm: Bpm,
    velocities: Velocities,
) -> Result<Vec<u8>, MidiWriteError> {
    let ticks_per_second = bpm.get() / 60.0 * f64::from(TICKS_PER_BEAT);
    let tick = |seconds: Seconds| {
        cast::round_to_usize(seconds.get() * ticks_per_second)
            .and_then(|tick| u64::try_from(tick).ok())
            .ok_or(MidiWriteError::TooLong(seconds))
    };
    let mut spans = Vec::with_capacity(notes.notes().len());
    for note in notes.notes() {
        let start = tick(note.onset())?;
        let floor = note.onset().get() + MIN_NOTE_LENGTH.get();
        let end = tick(Seconds::clamped(note.offset().get().max(floor)))?.max(start + 1);
        let key = cast::round_to_usize(note.pitch().get())
            .and_then(|key| u8::try_from(key).ok())
            .and_then(u7::try_from)
            .unwrap_or(u7::max_value());
        let velocity = match velocities {
            Velocities::Constant(velocity) => velocity,
            Velocities::FromNotes(default) => note.velocity().unwrap_or(default),
        };
        spans.push((key, start, end, velocity));
    }
    // Sorted by key, then start: a note ends no later than the next one of
    // its key.
    spans.sort_by_key(|&(key, start, ..)| (key, start));
    let mut events = Vec::with_capacity(2 * spans.len());
    for (i, &(key, start, end, velocity)) in spans.iter().enumerate() {
        let end = match spans.get(i + 1) {
            Some(&(next_key, next_start, ..)) if next_key == key => end.min(next_start),
            _ => end,
        };
        if end > start {
            events.push((start, true, key, velocity));
            events.push((end, false, key, velocity));
        }
    }
    // Note-offs sort before note-ons at one tick.
    events.sort_by_key(|&(at, on, key, _)| (at, on, key));

    let channel = u4::new(0);
    let mut track = vec![
        meta(MetaMessage::Tempo(u24::new(tempo_us(bpm)))),
        meta(MetaMessage::TimeSignature(4, 2, 24, 8)),
    ];
    let mut now = 0_u64;
    for (at, on, key, velocity) in events {
        let message = if on {
            MidiMessage::NoteOn {
                key,
                vel: u7::new(velocity.get()),
            }
        } else {
            MidiMessage::NoteOff {
                key,
                vel: u7::new(0),
            }
        };
        track.push(TrackEvent {
            delta: u32::try_from(at - now).ok().and_then(u28::try_from).ok_or(
                MidiWriteError::TooLong(Seconds::clamped(cast::u64_to_f64(at) / ticks_per_second)),
            )?,
            kind: TrackEventKind::Midi { channel, message },
        });
        now = at;
    }
    track.push(meta(MetaMessage::EndOfTrack));

    let header = Header::new(
        Format::SingleTrack,
        Timing::Metrical(u15::new(TICKS_PER_BEAT)),
    );
    let mut bytes = Vec::new();
    midly::write_std(&header, std::iter::once(&track), &mut bytes)
        .map_err(|source| MidiWriteError::Encode(source.to_string()))?;
    Ok(bytes)
}

/// The tempo in whole microseconds per quarter note.
fn tempo_us(bpm: Bpm) -> u32 {
    // 4 to 1000 BPM gives 60,000 to 15,000,000, which fits 24 bits.
    cast::round_to_usize(60e6 / bpm.get())
        .and_then(|us| u32::try_from(us).ok())
        .unwrap_or(u32::MAX)
}

fn meta(message: MetaMessage<'static>) -> TrackEvent<'static> {
    TrackEvent {
        delta: u28::new(0),
        kind: TrackEventKind::Meta(message),
    }
}

/// A note list that cannot become a MIDI file.
#[derive(Clone, Debug, PartialEq)]
pub enum MidiWriteError {
    /// A note lies too far from the previous event for a MIDI delta time.
    TooLong(Seconds),
    /// The encoder failed.
    Encode(String),
}

impl fmt::Display for MidiWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong(at) => write!(
                f,
                "cannot write a note at {at}: it lies further from the previous note than a MIDI file can hold. Split the stem"
            ),
            Self::Encode(message) => write!(f, "cannot encode the MIDI file: {message}"),
        }
    }
}

impl Error for MidiWriteError {}
