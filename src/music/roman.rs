//! Roman numerals, stored as semitones above the tonic.
//!
//! Upper case is a major chord and lower case a minor one; `°` (or `o`)
//! marks diminished, `ø` half-diminished and `+` augmented. A numeral
//! without a sign takes its degree from the mode of the line, and a `b` or
//! `#` is read against major. So `i-VI-III-VII` and `i-bVI-bIII-bVII` name
//! the same chords. `X/Y` is a secondary chord: X read in the major key of
//! the degree Y, so `V/V` lies 2 semitones above the tonic.
//!
//! Give the mode when the key is known. Without one, a line is minor when
//! it starts with `i` or holds a bare `III`, `VI` or `VII`, the degrees
//! that only minor makes major; any other line reads as major and comes
//! back flagged.
//!
//! ```
//! use sound_to_patch::music::roman::parse_line;
//!
//! let offsets = |text| -> Vec<u8> {
//!     parse_line(text, None).unwrap().numerals.iter().map(|n| n.offset).collect()
//! };
//! assert_eq!(offsets("i-VI-III-VII"), [0, 8, 3, 10]);
//! assert_eq!(offsets("i-bVI-bIII-bVII"), [0, 8, 3, 10]);
//! assert_eq!(offsets("vi-IV-I-V"), [9, 5, 0, 7]);
//! ```

use crate::music::MusicError;
use crate::music::key::Mode;

/// The quality a numeral names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Quality {
    /// Upper case.
    Major,
    /// Lower case.
    Minor,
    /// `°` or `o`.
    Diminished,
    /// `ø`.
    HalfDiminished,
    /// `+`.
    Augmented,
}

/// One numeral of a line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Numeral {
    /// Semitones above the tonic.
    pub offset: u8,
    /// The quality the case and the sign name.
    pub quality: Quality,
    /// What follows the numeral, such as `7` or `maj7`.
    pub extension: String,
    /// For a secondary chord `X/Y`, the offset of Y above the tonic.
    pub of: Option<u8>,
}

/// A parsed line of numerals and the mode it was read in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// The numerals in order.
    pub numerals: Vec<Numeral>,
    /// The mode the degrees were read in.
    pub mode: Mode,
    /// True when no mode was given and nothing in the line names one, so
    /// major is an assumption to record or ask about.
    pub assumed: bool,
}

const MAJOR: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
const MINOR: [u8; 7] = [0, 2, 3, 5, 7, 8, 10];
const DEGREES: [&str; 7] = ["vii", "vi", "iv", "v", "iii", "ii", "i"];

/// Parses a line of numerals separated by `-`, `–`, spaces or commas, in
/// `mode` when given.
///
/// # Errors
///
/// [`MusicError::UnknownNumeral`] for the first part that is no numeral.
pub fn parse_line(text: &str, mode: Option<Mode>) -> Result<Line, MusicError> {
    let parts: Vec<&str> = text
        .split(|c: char| matches!(c, '-' | '–' | ',') || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .collect();
    if parts.is_empty() {
        return Err(MusicError::UnknownNumeral(text.to_owned()));
    }
    let (mode, assumed) = match mode {
        Some(mode) => (mode, false),
        None if names_minor(&parts) => (Mode::Minor, false),
        None => (Mode::Major, true),
    };
    let numerals = parts
        .iter()
        .map(|part| parse_numeral(part, mode))
        .collect::<Result<_, _>>()?;
    Ok(Line {
        numerals,
        mode,
        assumed,
    })
}

/// Whether a line names minor: it starts with `i`, or a part is a bare
/// `III`, `VI` or `VII`.
fn names_minor(parts: &[&str]) -> bool {
    fn chord<'a>(part: &&'a str) -> Option<Parsed<'a>> {
        read(part.split('/').next().unwrap_or(part))
    }
    let tonic = parts
        .first()
        .and_then(chord)
        .is_some_and(|first| first.degree == 0 && first.lower && first.accidental == 0);
    tonic
        || parts.iter().filter_map(chord).any(|numeral| {
            numeral.accidental == 0
                && !numeral.lower
                && numeral.sign.is_none()
                && matches!(numeral.degree, 2 | 5 | 6)
        })
}

/// Parses one numeral in a line of `mode`.
///
/// # Errors
///
/// [`MusicError::UnknownNumeral`] when the text is no numeral.
pub fn parse_numeral(text: &str, mode: Mode) -> Result<Numeral, MusicError> {
    let unknown = || MusicError::UnknownNumeral(text.to_owned());
    if let Some((chord, target)) = text.split_once('/')
        && read(target).is_some()
    {
        let target = parse_numeral(target, mode)?;
        if target.of.is_some() {
            return Err(unknown());
        }
        let chord = parse_numeral(chord, Mode::Major)?;
        if chord.of.is_some() {
            return Err(unknown());
        }
        return Ok(Numeral {
            offset: (target.offset + chord.offset) % 12,
            of: Some(target.offset),
            ..chord
        });
    }
    let parsed = read(text).ok_or_else(unknown)?;
    let quality = match parsed.sign {
        Some('°' | 'o') => Quality::Diminished,
        Some('ø') => Quality::HalfDiminished,
        Some('+') => Quality::Augmented,
        _ if parsed.lower => Quality::Minor,
        _ => Quality::Major,
    };
    let minor_line = mode == Mode::Minor;
    let leading_tone = matches!(quality, Quality::Diminished | Quality::HalfDiminished);
    let offset = match parsed.accidental {
        0 if minor_line && parsed.degree == 6 && leading_tone => 11,
        0 if minor_line => MINOR[parsed.degree],
        step => {
            let base = i16::from(MAJOR[parsed.degree]) + i16::from(step);
            u8::try_from(base.rem_euclid(12)).unwrap_or(0)
        }
    };
    Ok(Numeral {
        offset,
        quality,
        extension: parsed.extension.to_owned(),
        of: None,
    })
}

struct Parsed<'a> {
    accidental: i8,
    degree: usize,
    lower: bool,
    sign: Option<char>,
    extension: &'a str,
}

fn read(text: &str) -> Option<Parsed<'_>> {
    let (accidental, rest) = match text.chars().next()? {
        'b' | '♭' => (-1, &text[text.chars().next()?.len_utf8()..]),
        '#' | '♯' => (1, &text[text.chars().next()?.len_utf8()..]),
        _ => (0, text),
    };
    let lower_rest = rest.to_ascii_lowercase();
    let (index, name) = DEGREES
        .iter()
        .enumerate()
        .find(|(_, name)| lower_rest.starts_with(*name))?;
    let numeral = &rest[..name.len()];
    let lower = numeral.chars().all(|c| c.is_ascii_lowercase());
    if !lower && !numeral.chars().all(|c| c.is_ascii_uppercase()) {
        return None;
    }
    let after = &rest[name.len()..];
    let sign = after
        .chars()
        .next()
        .filter(|c| matches!(c, '°' | 'o' | 'ø' | '+'));
    let extension = &after[sign.map_or(0, char::len_utf8)..];
    let known_start = extension.is_empty()
        || extension.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '#' | 'b'))
        || ["maj", "sus", "add"]
            .iter()
            .any(|word| extension.starts_with(word));
    // A slash belongs only to the six-nine extension; `X/Y` is read by
    // `parse_numeral`.
    let known_chars = extension
        .replace("6/9", "")
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '#');
    if !known_start || !known_chars {
        return None;
    }
    let degree = match index {
        0 => 6,
        1 => 5,
        2 => 3,
        3 => 4,
        4 => 2,
        5 => 1,
        _ => 0,
    };
    Some(Parsed {
        accidental,
        degree,
        lower,
        sign,
        extension,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offsets(line: &str) -> Vec<u8> {
        parse_line(line, None)
            .unwrap_or_else(|error| panic!("{line}: {error}"))
            .numerals
            .iter()
            .map(|numeral| numeral.offset)
            .collect()
    }

    #[test]
    fn reads_the_major_and_minor_degrees() {
        assert_eq!(offsets("I ii iii IV V vi vii°"), [0, 2, 4, 5, 7, 9, 11]);
        assert_eq!(offsets("i ii° III iv v VI VII"), [0, 2, 3, 5, 7, 8, 10]);
        assert_eq!(offsets("bII bIII bVI bVII"), [1, 3, 8, 10]);
    }

    #[test]
    fn a_line_is_minor_only_when_it_says_so() {
        assert_eq!(offsets("vi-IV-I-V"), [9, 5, 0, 7]);
        assert_eq!(offsets("iii-vi-ii-V-I"), [4, 9, 2, 7, 0]);
        assert_eq!(offsets("vi-V-IV-V"), [9, 7, 5, 7]);
        assert_eq!(offsets("IV-V-iii-vi"), [5, 7, 4, 9]);
        assert_eq!(offsets("VI-VII-i-i"), [8, 10, 0, 0]);
        assert_eq!(offsets("iv-VII-III-VI"), [5, 10, 3, 8]);
        assert!(parse_line("vi-IV-I-V", None).unwrap().assumed);
        assert!(!parse_line("i-iv-v", None).unwrap().assumed);
        let given = parse_line("vi-IV-I-V", Some(Mode::Minor)).unwrap();
        assert_eq!((given.mode, given.assumed), (Mode::Minor, false));
        assert_eq!(given.numerals[0].offset, 8);
    }

    #[test]
    fn both_minor_spellings_name_the_same_chords() {
        assert_eq!(offsets("i-VI-III-VII"), offsets("i-bVI-bIII-bVII"));
        assert_eq!(offsets("I-bVII-IV"), [0, 10, 5]);
        assert_eq!(offsets("bII-i"), [1, 0]);
        assert_eq!(offsets("i-iv-V"), [0, 5, 7]);
        assert_eq!(offsets("i vii°"), [0, 11]);
        assert_eq!(offsets("i viiø7"), [0, 11]);
    }

    #[test]
    fn reads_secondary_chords() {
        let line = parse_line("I V7/V V", None).unwrap();
        assert_eq!(line.numerals[1].offset, 2);
        assert_eq!(line.numerals[1].of, Some(7));
        assert_eq!(line.numerals[1].extension, "7");
        assert_eq!(offsets("I V/vi vi"), [0, 4, 9]);
        assert_eq!(offsets("I viiø7/V V"), [0, 6, 7]);
        assert_eq!(
            parse_line("I6/9", None).unwrap().numerals[0].extension,
            "6/9"
        );
        for text in ["V7/X", "V/V/V", "I7/9", "V/"] {
            assert!(parse_line(text, None).is_err(), "{text}");
        }
    }

    #[test]
    fn keeps_quality_and_extension() {
        let line = parse_line("ii7-V7-Imaj7 iiø7", None).unwrap();
        let read: Vec<(u8, Quality, &str)> = line
            .numerals
            .iter()
            .map(|numeral| (numeral.offset, numeral.quality, numeral.extension.as_str()))
            .collect();
        assert_eq!(
            read,
            [
                (2, Quality::Minor, "7"),
                (7, Quality::Major, "7"),
                (0, Quality::Major, "maj7"),
                (2, Quality::HalfDiminished, "7"),
            ]
        );
        assert_eq!(
            parse_line("III+", None).unwrap().numerals[0].quality,
            Quality::Augmented
        );
    }

    #[test]
    fn rejects_text_that_is_no_numeral() {
        for text in ["", "X", "Iv", "VIII", "IVx", "C-G-Am"] {
            assert!(parse_line(text, None).is_err(), "{text}");
        }
    }
}
