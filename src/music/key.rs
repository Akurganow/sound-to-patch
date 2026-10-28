//! Keys: the key parser, relative and parallel keys, Camelot and Open Key.
//!
//! The parser tries three forms in order: a Camelot code (`8A`), an Open
//! Key code (`1m`), then a letter name with an optional sharp or flat and
//! mode word (`A`, `Am`, `A minor`, `Bb`, `E♭ moll`, `C sharp minor`). A
//! bare capital reads as major and a lone lowercase letter as minor; both
//! readings come back flagged, so the caller records or asks.
//!
//! ```
//! use sound_to_patch::music::key::{Key, Mode, parse_key};
//!
//! let reading = parse_key("C sharp minor")?;
//! assert_eq!(reading.key, Key::new(1, Mode::Minor));
//! assert_eq!(reading.key.camelot().to_string(), "12A");
//! assert_eq!(reading.key.open_key().to_string(), "5m");
//! # Ok::<(), sound_to_patch::music::MusicError>(())
//! ```

use std::fmt;

use crate::music::MusicError;
use crate::music::pitch::{Spelling, class_name, letter_class, read_class};

/// Major or minor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Major.
    Major,
    /// Minor.
    Minor,
}

/// A tonic pitch class and a mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    /// The tonic, 0 (C) to 11 (B).
    pub tonic: u8,
    /// Major or minor.
    pub mode: Mode,
}

impl Key {
    /// A key; the tonic is taken modulo 12.
    #[must_use]
    pub fn new(tonic: u8, mode: Mode) -> Self {
        Self {
            tonic: tonic % 12,
            mode,
        }
    }

    /// The relative key: the minor a major sixth up, the major a minor
    /// third up.
    #[must_use]
    pub fn relative(self) -> Self {
        match self.mode {
            Mode::Major => Self::new(self.tonic + 9, Mode::Minor),
            Mode::Minor => Self::new(self.tonic + 3, Mode::Major),
        }
    }

    /// The parallel key: the same tonic in the other mode.
    #[must_use]
    pub fn parallel(self) -> Self {
        match self.mode {
            Mode::Major => Self::new(self.tonic, Mode::Minor),
            Mode::Minor => Self::new(self.tonic, Mode::Major),
        }
    }

    /// Sharps or flats for display: flats for F, Bb, Eb, Ab and Db major
    /// and for D, G, C, F and Bb minor; sharps otherwise.
    #[must_use]
    pub fn spelling(self) -> Spelling {
        let flats: &[u8] = match self.mode {
            Mode::Major => &[5, 10, 3, 8, 1],
            Mode::Minor => &[2, 7, 0, 5, 10],
        };
        if flats.contains(&self.tonic) {
            Spelling::Flats
        } else {
            Spelling::Sharps
        }
    }

    /// The Camelot code: the number of the major tonic `t` is the `n`
    /// with `7 * (n - 8) = t` modulo 12; minor keys take the number of
    /// their relative major.
    #[must_use]
    pub fn camelot(self) -> Camelot {
        let major = match self.mode {
            Mode::Major => self.tonic,
            Mode::Minor => self.relative().tonic,
        };
        Camelot {
            number: (7 * major + 7) % 12 + 1,
            mode: self.mode,
        }
    }

    /// The Open Key code: number `(n + 4) mod 12 + 1` of the Camelot number
    /// `n`.
    #[must_use]
    pub fn open_key(self) -> OpenKey {
        let camelot = self.camelot();
        OpenKey {
            number: (camelot.number + 4) % 12 + 1,
            mode: self.mode,
        }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mode = match self.mode {
            Mode::Major => "major",
            Mode::Minor => "minor",
        };
        write!(f, "{} {mode}", class_name(self.tonic, self.spelling()))
    }
}

/// A Camelot code: a number from 1 to 12, `A` for minor and `B` for major.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Camelot {
    /// 1 to 12.
    pub number: u8,
    /// `A` is minor, `B` major.
    pub mode: Mode,
}

impl Camelot {
    /// The key of the code.
    #[must_use]
    pub fn key(self) -> Key {
        // 7 is its own inverse modulo 12, so t = 7 * (n - 8).
        let major = Key::new((7 * (self.number + 4)) % 12, Mode::Major);
        match self.mode {
            Mode::Major => major,
            Mode::Minor => major.relative(),
        }
    }

    /// The code a fifth away: one number up, 12 wrapping to 1.
    #[must_use]
    pub fn fifth_up(self) -> Self {
        Self {
            number: self.number % 12 + 1,
            ..self
        }
    }

    /// The code a fifth down: one number down, 1 wrapping to 12.
    #[must_use]
    pub fn fifth_down(self) -> Self {
        Self {
            number: (self.number + 10) % 12 + 1,
            ..self
        }
    }
}

impl fmt::Display for Camelot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let letter = match self.mode {
            Mode::Minor => 'A',
            Mode::Major => 'B',
        };
        write!(f, "{}{letter}", self.number)
    }
}

/// An Open Key code: a number from 1 to 12, `m` for minor and `d` for
/// major.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OpenKey {
    /// 1 to 12.
    pub number: u8,
    /// `m` is minor, `d` major.
    pub mode: Mode,
}

impl OpenKey {
    /// The key of the code.
    #[must_use]
    pub fn key(self) -> Key {
        Camelot {
            number: (self.number + 6) % 12 + 1,
            mode: self.mode,
        }
        .key()
    }
}

impl fmt::Display for OpenKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let letter = match self.mode {
            Mode::Minor => 'm',
            Mode::Major => 'd',
        };
        write!(f, "{}{letter}", self.number)
    }
}

/// A mode the parser assumed because the text named none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyAssumption {
    /// A bare capital, read as major.
    BareCapital,
    /// A lone lowercase letter, read as minor.
    BareLowercase,
}

/// A parsed key and the assumption it rests on, if any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyReading {
    /// The key.
    pub key: Key,
    /// Set when the mode was not written.
    pub assumption: Option<KeyAssumption>,
}

/// Parses a key name.
///
/// # Errors
///
/// [`MusicError::UnknownKey`] for text no branch of the grammar accepts,
/// such as `13A`, `Z` or the chord `Am7`.
pub fn parse_key(text: &str) -> Result<KeyReading, MusicError> {
    let unknown = || MusicError::UnknownKey(text.to_owned());
    let trimmed = text.trim();
    if let Some(key) = code(trimmed) {
        return Ok(KeyReading {
            key,
            assumption: None,
        });
    }
    letter_key(trimmed).ok_or_else(unknown)
}

/// A Camelot or Open Key code.
fn code(text: &str) -> Option<Key> {
    let split = text.find(|c: char| !c.is_ascii_digit())?;
    let (digits, rest) = text.split_at(split);
    if digits.starts_with('0') {
        return None;
    }
    let number: u8 = digits.parse().ok().filter(|n| (1..=12).contains(n))?;
    match rest.trim() {
        "A" | "a" => Some(
            Camelot {
                number,
                mode: Mode::Minor,
            }
            .key(),
        ),
        "B" | "b" => Some(
            Camelot {
                number,
                mode: Mode::Major,
            }
            .key(),
        ),
        "m" => Some(
            OpenKey {
                number,
                mode: Mode::Minor,
            }
            .key(),
        ),
        "d" => Some(
            OpenKey {
                number,
                mode: Mode::Major,
            }
            .key(),
        ),
        _ => None,
    }
}

/// A letter name with optional accidentals and a mode word.
fn letter_key(text: &str) -> Option<KeyReading> {
    let first = text.chars().next()?;
    let german = {
        let lower = text.to_lowercase();
        lower.ends_with("dur") || lower.ends_with("moll")
    };
    let (class, rest) = german_class(text, german).or_else(|| english_class(text))?;
    let rest = rest.trim();
    let rest = match rest.strip_prefix('-') {
        Some(word) if !word.is_empty() => word,
        _ => rest,
    };
    let mode = match rest {
        "" => None,
        "M" => Some(Mode::Major),
        "m" | "-" | "−" | "–" => Some(Mode::Minor),
        word => match word.to_lowercase().as_str() {
            "maj" | "major" | "dur" => Some(Mode::Major),
            "min" | "minor" | "moll" => Some(Mode::Minor),
            _ => return None,
        },
    };
    let tonic = u8::try_from(class.rem_euclid(12)).ok()?;
    let (mode, assumption) = match mode {
        Some(mode) => (mode, None),
        None if first.is_ascii_lowercase() => (Mode::Minor, Some(KeyAssumption::BareLowercase)),
        None => (Mode::Major, Some(KeyAssumption::BareCapital)),
    };
    Some(KeyReading {
        key: Key::new(tonic, mode),
        assumption,
    })
}

/// A letter with sign characters or the words `sharp` and `flat`.
fn english_class(text: &str) -> Option<(i16, &str)> {
    let (class, rest) = read_class(text)?;
    let trimmed = rest.trim_start();
    for (word, step) in [("sharp", 1), ("flat", -1)] {
        if let Some(after) = trimmed.strip_prefix(word)
            && ends_word(after)
        {
            return Some((class + step, after));
        }
    }
    Some((class, rest))
}

/// Whether a word ends where `rest` starts.
fn ends_word(rest: &str) -> bool {
    rest.is_empty() || rest.starts_with(|c: char| c.is_whitespace() || c == '-')
}

/// German names, read only when the text uses them: `H` is B, `B` is B
/// flat before `dur` or `moll`, and `-is` and `-es` add a sharp or a
/// flat (`Fis`, `Es`, `As`).
fn german_class(text: &str, german: bool) -> Option<(i16, &str)> {
    let mut chars = text.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    let rest = chars.as_str();
    let class = match letter {
        'H' => 11,
        'B' if german => 10,
        _ => letter_class(letter)?,
    };
    let lower = rest.to_ascii_lowercase();
    let suffix = [("is", 1), ("es", -1), ("s", -1)]
        .into_iter()
        .find(|(suffix, _)| lower.starts_with(suffix) && ends_word(&rest[suffix.len()..]));
    match suffix {
        Some(("s", _)) if !matches!(letter, 'A' | 'E') => None,
        Some((_, _)) if letter == 'B' => None,
        Some((suffix, step)) => Some((class + step, &rest[suffix.len()..])),
        None if letter == 'H' || (letter == 'B' && german && ends_word(rest)) => {
            Some((class, rest))
        }
        None => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> (Key, Option<KeyAssumption>) {
        let reading = parse_key(text).unwrap_or_else(|error| panic!("{text}: {error}"));
        (reading.key, reading.assumption)
    }

    #[test]
    fn parses_the_key_forms() {
        let minor = |tonic| Key::new(tonic, Mode::Minor);
        let major = |tonic| Key::new(tonic, Mode::Major);
        let cases = [
            ("Am", minor(9)),
            ("A min", minor(9)),
            ("A minor", minor(9)),
            ("A-", minor(9)),
            ("A−", minor(9)),
            ("A maj", major(9)),
            ("A major", major(9)),
            ("AM", major(9)),
            ("A dur", major(9)),
            ("A# major", major(10)),
            ("A♯ maj", major(10)),
            ("Bb major", major(10)),
            ("B♭ M", major(10)),
            ("A sharp major", major(10)),
            ("B flat minor", minor(10)),
            ("C sharp minor", minor(1)),
            ("E♭ moll", minor(3)),
            ("F## major", major(7)),
            ("Gbb minor", minor(5)),
            ("H dur", major(11)),
            ("B moll", minor(10)),
            ("Fis moll", minor(6)),
            ("Es dur", major(3)),
            ("As dur", major(8)),
            ("Cis-moll", minor(1)),
            ("8A", minor(9)),
            ("8B", major(0)),
            ("1m", minor(9)),
            ("1d", major(0)),
            ("12 B", major(4)),
        ];
        for (text, key) in cases {
            assert_eq!(read(text), (key, None), "{text}");
        }
    }

    #[test]
    fn a_bare_letter_is_flagged() {
        assert_eq!(
            read("a"),
            (Key::new(9, Mode::Minor), Some(KeyAssumption::BareLowercase))
        );
        assert_eq!(
            read("A"),
            (Key::new(9, Mode::Major), Some(KeyAssumption::BareCapital))
        );
        assert_eq!(
            read("f#"),
            (Key::new(6, Mode::Minor), Some(KeyAssumption::BareLowercase))
        );
        for text in ["A#", "A♯", "Bb", "B♭", "A sharp", "B flat", "H"] {
            assert_eq!(read(text).1, Some(KeyAssumption::BareCapital), "{text}");
        }
    }

    #[test]
    fn rejects_text_outside_the_grammar() {
        for text in [
            "13A", "0A", "Z", "Am7", "A7", "", "8C", "13m", "A minorr", "Bes",
        ] {
            assert!(parse_key(text).is_err(), "{text}");
        }
    }

    #[test]
    fn camelot_and_open_key_follow_the_formulas() {
        for number in 1..=12u8 {
            let major = Camelot {
                number,
                mode: Mode::Major,
            }
            .key();
            assert_eq!(u16::from(major.tonic), (7 * (u16::from(number) + 4)) % 12);
            let minor = Camelot {
                number,
                mode: Mode::Minor,
            }
            .key();
            assert_eq!(minor, major.relative());
            for key in [major, minor] {
                assert_eq!(key.camelot().number, number);
                assert_eq!(key.open_key().number, (number + 4) % 12 + 1);
                assert_eq!(key.open_key().key(), key);
            }
        }
    }

    #[test]
    fn every_key_round_trips_through_its_codes() {
        for tonic in 0..12 {
            for mode in [Mode::Major, Mode::Minor] {
                let key = Key::new(tonic, mode);
                assert_eq!(read(&key.camelot().to_string()).0, key);
                assert_eq!(read(&key.open_key().to_string()).0, key);
                assert_eq!(read(&key.to_string()).0, key);
            }
        }
    }

    #[test]
    fn neighbours_are_a_fifth_or_the_relative_key() {
        for tonic in 0..12 {
            for mode in [Mode::Major, Mode::Minor] {
                let key = Key::new(tonic, mode);
                let code = key.camelot();
                assert_eq!(code.fifth_up().key(), Key::new(tonic + 7, mode));
                assert_eq!(code.fifth_down().key(), Key::new(tonic + 5, mode));
                assert_eq!(code.fifth_up().fifth_down(), code);
                let other = Camelot {
                    mode: key.relative().mode,
                    ..code
                };
                assert_eq!(other.key(), key.relative());
                assert_eq!(key.parallel().tonic, tonic);
            }
        }
        assert_eq!(
            Camelot {
                number: 12,
                mode: Mode::Major
            }
            .fifth_up()
            .number,
            1
        );
        assert_eq!(
            Camelot {
                number: 1,
                mode: Mode::Major
            }
            .fifth_down()
            .number,
            12
        );
    }

    #[test]
    fn spells_by_the_display_rule() {
        assert_eq!(Key::new(10, Mode::Major).to_string(), "Bb major");
        assert_eq!(Key::new(6, Mode::Minor).to_string(), "F# minor");
        assert_eq!(Key::new(10, Mode::Minor).to_string(), "Bb minor");
        assert_eq!(Key::new(8, Mode::Minor).to_string(), "G# minor");
    }
}
