//! Reading a note list from a Standard MIDI File.
//!
//! A render analysed against the MIDI file that produced it needs no
//! transcription: the file gives the notes. Note times come from the file's
//! own tempo map, and every tempo in it must match the stem's BPM, because
//! a wrong time base would shift every note against the audio.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use midly::{Format, MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};

use super::{Note, NoteError, NoteList, Velocity};
use crate::cast;
use crate::units::{Bpm, MidiPitch, Seconds};

/// The tempo of a file without a tempo event, by the MIDI standard.
const DEFAULT_TEMPO_US: u32 = 500_000;
/// How far a tempo in the file may lie from `--bpm`. A tempo is stored in
/// whole microseconds per beat, which rounds 140 BPM to 140.00003.
pub const BPM_TOLERANCE: f64 = 0.01;
/// The General MIDI percussion channel, channel 10, counted from 0. Its
/// keys name drums, not pitches.
pub const DRUM_CHANNEL: u8 = 9;

/// Reads the notes of a MIDI file at `path`, checking its tempo against
/// `bpm`.
///
/// Every note-on with a velocity above 0 starts a note, and the next
/// note-off of the same key on the same channel and track ends it, first
/// in, first out. A note still sounding at the end of its track ends there.
/// A note of zero length carries no sound and is left out. Notes on the
/// percussion channel, channel 10 ([`DRUM_CHANNEL`] counted from 0), are
/// drums whose keys are no pitch, so they are left out too.
///
/// # Errors
///
/// Returns a [`MidiError`] that names the file and the cause: an unreadable
/// or malformed file, a sequential (format 2) file, a tempo that differs
/// from `bpm`, or a file without notes.
pub fn read_midi(path: &Path, bpm: Bpm) -> Result<NoteList, MidiError> {
    read_midi_near(path, bpm, None)
}

/// Reads the notes of a MIDI file at `path` as [`read_midi`] does, and
/// also accepts a file whose every tempo matches `refined`, the tempo
/// refined from the stem.
///
/// # Errors
///
/// As [`read_midi`], with a tempo that matches neither `bpm` nor
/// `refined`.
pub fn read_midi_near(path: &Path, bpm: Bpm, refined: Option<Bpm>) -> Result<NoteList, MidiError> {
    let fail = |kind| MidiError {
        path: path.to_path_buf(),
        kind,
    };
    let bytes = std::fs::read(path).map_err(|source| fail(Kind::Open(source)))?;
    let smf = Smf::parse(&bytes).map_err(|source| fail(Kind::Parse(source.to_string())))?;
    match (notes_of(&smf, bpm), refined) {
        (Err(Kind::Tempo { .. }), Some(refined)) => {
            notes_of(&smf, refined).map_err(|kind| match kind {
                Kind::Tempo {
                    file_bpm, default, ..
                } => Kind::Tempo {
                    file_bpm,
                    bpm,
                    refined: Some(refined),
                    default,
                },
                other => other,
            })
        }
        (result, _) => result,
    }
    .map_err(fail)
}

fn notes_of(smf: &Smf<'_>, bpm: Bpm) -> Result<NoteList, Kind> {
    if smf.header.format == Format::Sequential {
        return Err(Kind::Sequential);
    }
    let clock = match smf.header.timing {
        Timing::Metrical(ticks) => Clock::metrical(smf, ticks.as_int(), bpm)?,
        Timing::Timecode(fps, subframes) => Clock::Timecode {
            ticks_per_second: f64::from(fps.as_f32()) * f64::from(subframes),
        },
    };
    let mut notes = Vec::new();
    let mut drums = 0_usize;
    for (track, events) in smf.tracks.iter().enumerate() {
        let mut tick = 0_u64;
        let mut open: HashMap<(u8, u8), Vec<(u64, u8)>> = HashMap::new();
        for event in events {
            tick += u64::from(event.delta.as_int());
            let TrackEventKind::Midi { channel, message } = event.kind else {
                continue;
            };
            if channel.as_int() == DRUM_CHANNEL {
                if matches!(message, MidiMessage::NoteOn { vel, .. } if vel.as_int() > 0) {
                    drums += 1;
                }
                continue;
            }
            let slot = |key: midly::num::u7| (channel.as_int(), key.as_int());
            match message {
                MidiMessage::NoteOn { key, vel } if vel.as_int() > 0 => {
                    open.entry(slot(key))
                        .or_default()
                        .push((tick, vel.as_int()));
                }
                MidiMessage::NoteOn { key, .. } | MidiMessage::NoteOff { key, .. } => {
                    if let Some(queue) = open.get_mut(&slot(key))
                        && !queue.is_empty()
                    {
                        let (onset, velocity) = queue.remove(0);
                        push_note(
                            &mut notes,
                            &clock,
                            track,
                            onset,
                            tick,
                            key.as_int(),
                            velocity,
                        )?;
                    }
                }
                _ => {}
            }
        }
        let mut unclosed: Vec<_> = open
            .into_iter()
            .flat_map(|((_, key), queue)| queue.into_iter().map(move |(onset, v)| (onset, key, v)))
            .collect();
        unclosed.sort_unstable();
        for (onset, key, velocity) in unclosed {
            push_note(&mut notes, &clock, track, onset, tick, key, velocity)?;
        }
    }
    if notes.is_empty() {
        return Err(Kind::NoNotes { drums });
    }
    Ok(NoteList::new(notes))
}

fn push_note(
    notes: &mut Vec<Note>,
    clock: &Clock,
    track: usize,
    onset: u64,
    offset: u64,
    key: u8,
    velocity: u8,
) -> Result<(), Kind> {
    if offset <= onset {
        return Ok(());
    }
    let note = Note::new(
        clock.seconds(onset),
        clock.seconds(offset),
        MidiPitch::clamped(f64::from(key)),
    )
    .and_then(|note| Ok(note.with_velocity(Velocity::new(velocity)?)))
    .map_err(|source| Kind::Note { track, source })?;
    notes.push(note);
    Ok(())
}

/// Ticks to seconds.
enum Clock {
    /// Ticks per beat with a tempo map of `(tick, seconds at that tick,
    /// seconds per tick from there)`, sorted by tick.
    Metrical { map: Vec<(u64, f64, f64)> },
    /// A fixed number of ticks per second.
    Timecode { ticks_per_second: f64 },
}

impl Clock {
    /// The tempo map of a metrical file, from the tempo events of every
    /// track. Every tempo, including the default before the first event,
    /// must match `bpm`.
    fn metrical(smf: &Smf<'_>, ticks_per_beat: u16, bpm: Bpm) -> Result<Self, Kind> {
        let mut tempos = Vec::new();
        for events in &smf.tracks {
            let mut tick = 0_u64;
            for event in events {
                tick += u64::from(event.delta.as_int());
                if let TrackEventKind::Meta(MetaMessage::Tempo(us)) = event.kind {
                    tempos.push((tick, us.as_int(), false));
                }
            }
        }
        tempos.sort_by_key(|&(tick, _, _)| tick);
        if tempos.first().is_none_or(|&(tick, _, _)| tick > 0) {
            tempos.insert(0, (0, DEFAULT_TEMPO_US, true));
        }
        let per_beat = f64::from(ticks_per_beat.max(1));
        let mut map: Vec<(u64, f64, f64)> = Vec::with_capacity(tempos.len());
        for (tick, us, default) in tempos {
            let file_bpm = 60e6 / f64::from(us.max(1));
            if (file_bpm - bpm.get()).abs() > BPM_TOLERANCE {
                return Err(Kind::Tempo {
                    file_bpm,
                    bpm,
                    refined: None,
                    default,
                });
            }
            let seconds = map.last().map_or(0.0, |&(start, at, per_tick)| {
                at + cast::u64_to_f64(tick - start) * per_tick
            });
            map.push((tick, seconds, f64::from(us) * 1e-6 / per_beat));
        }
        Ok(Self::Metrical { map })
    }

    fn seconds(&self, tick: u64) -> Seconds {
        let seconds = match self {
            Self::Metrical { map } => {
                let index = map.partition_point(|&(start, _, _)| start <= tick);
                map.get(index.saturating_sub(1))
                    .map_or(0.0, |&(start, at, per_tick)| {
                        at + cast::u64_to_f64(tick - start) * per_tick
                    })
            }
            Self::Timecode { ticks_per_second } => {
                cast::u64_to_f64(tick) / ticks_per_second.max(1.0)
            }
        };
        Seconds::clamped(seconds)
    }
}

/// A MIDI file that cannot give a note list.
#[derive(Debug)]
pub struct MidiError {
    path: PathBuf,
    kind: Kind,
}

impl MidiError {
    /// Returns the path of the file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug)]
enum Kind {
    Open(io::Error),
    Parse(String),
    Sequential,
    Tempo {
        file_bpm: f64,
        bpm: Bpm,
        refined: Option<Bpm>,
        default: bool,
    },
    Note {
        track: usize,
        source: NoteError,
    },
    NoNotes {
        drums: usize,
    },
}

impl fmt::Display for MidiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot read the notes of {}: ", self.path.display())?;
        match &self.kind {
            Kind::Open(source) => write!(f, "cannot open the file ({source}). Check the path"),
            Kind::Parse(message) => write!(
                f,
                "the file is not a valid MIDI file ({message}). Export the notes again as a Standard MIDI File"
            ),
            Kind::Sequential => write!(
                f,
                "the file is a sequential (format 2) MIDI file. Export the notes as format 0 or 1"
            ),
            Kind::Tempo {
                file_bpm,
                bpm,
                refined,
                default,
            } => {
                let origin = if *default {
                    " (the MIDI default; the file has no tempo event at its start)"
                } else {
                    ""
                };
                let refined = refined.map_or_else(String::new, |refined| {
                    format!(" and the tempo refined from the stem is {}", refined.get())
                });
                write!(
                    f,
                    "the file plays at {file_bpm:.3} BPM{origin}, but --bpm is {}{refined}. Export the notes at the stem's tempo or pass the file's tempo as --bpm",
                    bpm.get()
                )
            }
            Kind::Note { track, source } => write!(f, "track {track} holds a bad note: {source}"),
            Kind::NoNotes { drums: 0 } => {
                write!(f, "the file holds no notes. Export the notes again")
            }
            Kind::NoNotes { drums } => write!(
                f,
                "the file holds only drum notes ({drums} on channel 10), which have no pitch. Export the pitched part"
            ),
        }
    }
}

impl Error for MidiError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            Kind::Open(source) => Some(source),
            Kind::Note { source, .. } => Some(source),
            _ => None,
        }
    }
}
