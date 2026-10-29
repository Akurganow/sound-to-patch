//! Writing a note list as a Standard MIDI File.
//!
//! The file is format 0: one track holds the tempo, a 4/4 time signature
//! and every note on channel 1. A DAW opens it as one part at the stem's
//! tempo. Bar lines assume 4/4, because the input gives only a BPM.

use std::cmp::Reverse;
use std::error::Error;
use std::fmt;

use midly::num::{u4, u7, u15, u24, u28};
use midly::{Format, Header, MetaMessage, MidiMessage, Timing, TrackEvent, TrackEventKind};

use super::{NoteList, Velocity};
use crate::cast;
use crate::units::{Bpm, Seconds};

/// The resolution of the file in ticks per quarter note.
pub const TICKS_PER_BEAT: u16 = 480;

/// The velocity that the `notes` command passes to
/// [`Velocities::Constant`].
pub const DEFAULT_VELOCITY: Velocity = Velocity::from_const(100);

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
/// `round(60,000,000 / BPM)` microseconds per beat. A fractional pitch
/// rounds to the nearest key. Three rules any player needs come first: a
/// note shorter than [`MIN_NOTE_LENGTH`] or one tick is lengthened, of two
/// notes with one key and one start only the longer stays, and a note that
/// still sounds when its key starts again ends there. At one tick a
/// note-off comes before a note-on, so two touching notes of one pitch
/// stay two. The file has no program change and no pitch bend.
///
/// # Errors
///
/// Returns a [`MidiWriteError`] for an empty list, which no reader of ours
/// accepts, and when two events lie further apart than a MIDI delta time
/// holds, about nine hours at 1000 BPM.
pub fn write_midi(
    notes: &NoteList,
    bpm: Bpm,
    velocities: Velocities,
) -> Result<Vec<u8>, MidiWriteError> {
    encode(notes, bpm, velocities, None)
}

/// Encodes the notes as [`write_midi`] does, with a track name event at
/// tick 0 before the tempo. `notes` names the stem this way, so a later
/// run can tell a file of another stem; [`midi_track_name`] reads it back.
///
/// # Errors
///
/// The errors of [`write_midi`].
pub fn write_named_midi(
    notes: &NoteList,
    bpm: Bpm,
    velocities: Velocities,
    name: &str,
) -> Result<Vec<u8>, MidiWriteError> {
    encode(notes, bpm, velocities, Some(name))
}

/// Returns the first track name of a Standard MIDI File, or `None` when the
/// bytes do not parse or the file names no track.
#[must_use]
pub fn midi_track_name(bytes: &[u8]) -> Option<String> {
    let smf = midly::Smf::parse(bytes).ok()?;
    smf.tracks
        .iter()
        .flatten()
        .find_map(|event| match event.kind {
            TrackEventKind::Meta(MetaMessage::TrackName(name)) => {
                Some(String::from_utf8_lossy(name).into_owned())
            }
            _ => None,
        })
}

fn encode(
    notes: &NoteList,
    bpm: Bpm,
    velocities: Velocities,
    name: Option<&str>,
) -> Result<Vec<u8>, MidiWriteError> {
    if notes.is_empty() {
        return Err(MidiWriteError::NoNotes);
    }
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
            // Unreachable: a `MidiPitch` lies in 0 to 127, so its key fits.
            .unwrap_or(u7::max_value());
        let velocity = match velocities {
            Velocities::Constant(velocity) => velocity,
            Velocities::FromNotes(default) => note.velocity().unwrap_or(default),
        };
        spans.push((key, start, end, velocity));
    }
    // Sorted by key, then start, longest first: the first of one key and
    // start stays, and a note ends no later than the next one of its key.
    spans.sort_by_key(|&(key, start, end, _)| (key, start, Reverse(end)));
    spans.dedup_by_key(|&mut (key, start, ..)| (key, start));
    let mut events = Vec::with_capacity(2 * spans.len());
    for (i, &(key, start, end, velocity)) in spans.iter().enumerate() {
        let end = match spans.get(i + 1) {
            Some(&(next_key, next_start, ..)) if next_key == key => end.min(next_start),
            _ => end,
        };
        events.push((start, true, key, velocity));
        events.push((end, false, key, velocity));
    }
    // Note-offs sort before note-ons at one tick.
    events.sort_by_key(|&(at, on, key, _)| (at, on, key));

    let channel = u4::new(0);
    let mut track: Vec<TrackEvent<'_>> = name
        .map(|name| meta(MetaMessage::TrackName(name.as_bytes())))
        .into_iter()
        .collect();
    track.extend([
        meta(MetaMessage::Tempo(u24::new(tempo_us(bpm)))),
        meta(MetaMessage::TimeSignature(4, 2, 24, 8)),
    ]);
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

fn meta(message: MetaMessage<'_>) -> TrackEvent<'_> {
    TrackEvent {
        delta: u28::new(0),
        kind: TrackEventKind::Meta(message),
    }
}

/// A note list that cannot become a MIDI file.
#[derive(Clone, Debug, PartialEq)]
pub enum MidiWriteError {
    /// The list holds no note.
    NoNotes,
    /// A note lies too far from the previous event for a MIDI delta time.
    TooLong(Seconds),
    /// The encoder failed.
    Encode(String),
}

impl fmt::Display for MidiWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoNotes => write!(f, "cannot write a MIDI file without notes"),
            Self::TooLong(at) => write!(
                f,
                "cannot write a note at {at}: it lies further from the previous note than a MIDI file can hold. Split the stem"
            ),
            Self::Encode(message) => write!(f, "cannot encode the MIDI file: {message}"),
        }
    }
}

impl Error for MidiWriteError {}
