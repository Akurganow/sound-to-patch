//! Pitch classes, spelling for display and the three middle-C labels.
//!
//! Inside the tool a pitch is a MIDI number and a pitch class an integer
//! from 0 (C) to 11 (B). Names are for display only. MIDI fixes note 60 as
//! middle C but names no octave, so programs label it C3, C4 or C5; an
//! [`OctaveLabel`] names each convention by that label.
//!
//! ```
//! use sound_to_patch::music::pitch::{OctaveLabel, Spelling, note_name, parse_note};
//!
//! assert_eq!(note_name(60, OctaveLabel::C4, Spelling::Sharps), "C4");
//! assert_eq!(note_name(60, OctaveLabel::C3, Spelling::Sharps), "C3");
//! assert_eq!(parse_note("A-1", OctaveLabel::C3)?, 21);
//! # Ok::<(), sound_to_patch::music::MusicError>(())
//! ```

use std::fmt;

use crate::music::MusicError;

/// The pitch class of a MIDI number.
#[must_use]
pub fn pitch_class(pitch: u8) -> u8 {
    pitch % 12
}

/// Whether a name uses sharps or flats.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Spelling {
    /// C#, D#, F#, G#, A#.
    Sharps,
    /// Db, Eb, Gb, Ab, Bb.
    Flats,
}

const SHARPS: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
const FLATS: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
];

/// The name of a pitch class.
#[must_use]
pub fn class_name(class: u8, spelling: Spelling) -> &'static str {
    let names = match spelling {
        Spelling::Sharps => &SHARPS,
        Spelling::Flats => &FLATS,
    };
    names[usize::from(class % 12)]
}

/// A convention for octave numbers, named by the label it gives note 60.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OctaveLabel {
    /// Note 60 is C3; Serum 2 uses this label.
    C3,
    /// Note 60 is C4: scientific pitch notation.
    C4,
    /// Note 60 is C5.
    C5,
}

impl OctaveLabel {
    /// Every convention, scientific pitch first.
    pub const ALL: [Self; 3] = [Self::C4, Self::C3, Self::C5];

    /// The octave number of a MIDI pitch.
    #[must_use]
    pub fn octave(self, pitch: u8) -> i8 {
        // A MIDI pitch is at most 127, so pitch / 12 fits in an i8.
        let base = i8::try_from(pitch / 12).unwrap_or(i8::MAX);
        base - self.shift()
    }

    /// How many octaves the number of note 60 lies below 5.
    fn shift(self) -> i8 {
        match self {
            Self::C3 => 2,
            Self::C4 => 1,
            Self::C5 => 0,
        }
    }
}

impl fmt::Display for OctaveLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(match self {
            Self::C3 => "C3",
            Self::C4 => "C4",
            Self::C5 => "C5",
        })
    }
}

/// The name of a MIDI pitch in a convention, such as `C4` or `A-1`.
#[must_use]
pub fn note_name(pitch: u8, label: OctaveLabel, spelling: Spelling) -> String {
    format!(
        "{}{}",
        class_name(pitch_class(pitch), spelling),
        label.octave(pitch)
    )
}

/// The pitch class of a letter, `C` to `B`, in either case.
pub(crate) fn letter_class(letter: char) -> Option<i16> {
    match letter.to_ascii_uppercase() {
        'C' => Some(0),
        'D' => Some(2),
        'E' => Some(4),
        'F' => Some(5),
        'G' => Some(7),
        'A' => Some(9),
        'B' => Some(11),
        _ => None,
    }
}

/// Reads a letter and its sharps and flats from the start of `text`. Returns
/// the pitch-class offset from C, which may fall outside 0 to 11, and the
/// rest of the text.
pub(crate) fn read_class(text: &str) -> Option<(i16, &str)> {
    let mut chars = text.char_indices();
    let (_, letter) = chars.next()?;
    let mut class = letter_class(letter)?;
    let mut rest = &text[letter.len_utf8()..];
    for _ in 0..2 {
        let next = rest.chars().next();
        match next {
            Some('#' | '♯') => class += 1,
            Some('b' | '♭') => class -= 1,
            _ => break,
        }
        rest = &rest[next.map_or(0, char::len_utf8)..];
    }
    Some((class, rest))
}

/// The MIDI pitch of a note name in a convention: a letter, up to two
/// sharps or flats and an octave number, such as `C4`, `F#2` or `A-1`.
///
/// # Errors
///
/// [`MusicError::UnknownNote`] for any other text or a pitch outside 0 to
/// 127.
pub fn parse_note(text: &str, label: OctaveLabel) -> Result<u8, MusicError> {
    let unknown = || MusicError::UnknownNote(text.to_owned());
    let (class, rest) = read_class(text.trim()).ok_or_else(unknown)?;
    let octave: i16 = rest.trim().parse().map_err(|_| unknown())?;
    let pitch = octave
        .checked_add(i16::from(label.shift()))
        .and_then(|octave| octave.checked_mul(12))
        .and_then(|pitch| pitch.checked_add(class))
        .ok_or_else(unknown)?;
    u8::try_from(pitch)
        .ok()
        .filter(|pitch| *pitch <= 127)
        .ok_or_else(unknown)
}

/// A note name read without a stated convention.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteReading {
    /// The MIDI pitch.
    pub pitch: u8,
    /// The convention taken.
    pub label: OctaveLabel,
    /// Always true: the convention is an assumption to record or ask about.
    pub assumed: bool,
}

/// Reads a note name when the producer names no convention.
///
/// Picks the one convention whose pitch lies in `range`; when several or
/// none do, takes scientific pitch. The reading is flagged as an assumption
/// either way.
///
/// # Errors
///
/// [`MusicError::UnknownNote`] when no convention reads the text.
pub fn read_note(text: &str, range: Option<[u8; 2]>) -> Result<NoteReading, MusicError> {
    let readings: Vec<(OctaveLabel, u8)> = OctaveLabel::ALL
        .into_iter()
        .filter_map(|label| parse_note(text, label).ok().map(|pitch| (label, pitch)))
        .collect();
    let inside: Vec<&(OctaveLabel, u8)> = readings
        .iter()
        .filter(|(_, pitch)| range.is_some_and(|[low, high]| (low..=high).contains(pitch)))
        .collect();
    let (label, pitch) = match inside.as_slice() {
        [only] => **only,
        _ => readings
            .iter()
            .find(|(label, _)| *label == OctaveLabel::C4)
            .or(readings.first())
            .copied()
            .ok_or_else(|| MusicError::UnknownNote(text.to_owned()))?,
    };
    Ok(NoteReading {
        pitch,
        label,
        assumed: true,
    })
}

/// The pitch in Hz of a MIDI number in equal temperament, A4 = 440 Hz.
#[must_use]
pub fn frequency(pitch: u8) -> f64 {
    440.0 * 2f64.powf((f64::from(pitch) - 69.0) / 12.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_the_published_test_vectors() {
        let vectors = [
            (21, "A0", "A-1", "A1"),
            (28, "E1", "E0", "E2"),
            (36, "C2", "C1", "C3"),
            (60, "C4", "C3", "C5"),
            (69, "A4", "A3", "A5"),
            (127, "G9", "G8", "G10"),
        ];
        for (pitch, scientific, c3, c5) in vectors {
            assert_eq!(
                note_name(pitch, OctaveLabel::C4, Spelling::Sharps),
                scientific
            );
            assert_eq!(note_name(pitch, OctaveLabel::C3, Spelling::Sharps), c3);
            assert_eq!(note_name(pitch, OctaveLabel::C5, Spelling::Sharps), c5);
        }
    }

    #[test]
    fn every_pitch_round_trips_in_every_convention() {
        for label in OctaveLabel::ALL {
            for spelling in [Spelling::Sharps, Spelling::Flats] {
                for pitch in 0..=127 {
                    let name = note_name(pitch, label, spelling);
                    assert_eq!(parse_note(&name, label), Ok(pitch), "{name} in {label}");
                }
            }
        }
    }

    #[test]
    fn rejects_names_outside_midi() {
        for text in [
            "C-2", "H4", "C", "4", "C#x", "G#9", "G#5461", "C-32768", "B32767",
        ] {
            assert!(parse_note(text, OctaveLabel::C4).is_err(), "{text}");
        }
        assert_eq!(parse_note("Cb4", OctaveLabel::C4), Ok(59));
        assert_eq!(parse_note("B#3", OctaveLabel::C4), Ok(60));
        assert_eq!(parse_note("D♭4", OctaveLabel::C4), Ok(61));
    }

    #[test]
    fn a_name_with_no_convention_picks_the_reading_in_the_range() {
        let bass = Some([28, 48]);
        let c3 = read_note("C3", bass).unwrap();
        assert_eq!(
            (c3.pitch, c3.label, c3.assumed),
            (48, OctaveLabel::C4, true)
        );
        let c1 = read_note("C1", bass).unwrap();
        assert_eq!((c1.pitch, c1.label), (36, OctaveLabel::C3));
        let lead = read_note("C5", Some([60, 84])).unwrap();
        assert_eq!((lead.pitch, lead.label), (72, OctaveLabel::C4));
        let unbound = read_note("A4", None).unwrap();
        assert_eq!((unbound.pitch, unbound.label), (69, OctaveLabel::C4));
        let wide = read_note("C4", Some([21, 108])).unwrap();
        assert_eq!((wide.pitch, wide.label), (60, OctaveLabel::C4));
    }

    #[test]
    fn middle_c_and_a4_have_their_frequencies() {
        assert!((frequency(69) - 440.0).abs() < 1e-9);
        assert!((frequency(60) - 261.6256).abs() < 1e-4);
    }
}
