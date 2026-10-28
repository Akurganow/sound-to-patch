//! Roman numerals, stored as semitones above the tonic.
//!
//! Upper case is a major chord and lower case a minor one; `°` (or `o`)
//! marks diminished, `ø` half-diminished and `+` augmented. A numeral
//! without a sign takes its degree from the mode of the line, which is the
//! mode of the first numeral. A `b` or `#` is read against major. So
//! `i-VI-III-VII` and `i-bVI-bIII-bVII` name the same chords.
//!
//! ```
//! use sound_to_patch::music::roman::parse_line;
//!
//! let natural: Vec<u8> = parse_line("i-VI-III-VII")?.iter().map(|n| n.offset).collect();
//! let flats: Vec<u8> = parse_line("i-bVI-bIII-bVII")?.iter().map(|n| n.offset).collect();
//! assert_eq!(natural, [0, 8, 3, 10]);
//! assert_eq!(natural, flats);
//! # Ok::<(), sound_to_patch::music::MusicError>(())
//! ```

use crate::music::MusicError;

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
}

const MAJOR: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
const MINOR: [u8; 7] = [0, 2, 3, 5, 7, 8, 10];
const DEGREES: [&str; 7] = ["vii", "vi", "iv", "v", "iii", "ii", "i"];

/// Parses a line of numerals separated by `-`, `–`, spaces or commas.
///
/// # Errors
///
/// [`MusicError::UnknownNumeral`] for the first part that is no numeral.
pub fn parse_line(text: &str) -> Result<Vec<Numeral>, MusicError> {
    let parts: Vec<&str> = text
        .split(|c: char| matches!(c, '-' | '–' | ',') || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .collect();
    let first = parts
        .first()
        .ok_or_else(|| MusicError::UnknownNumeral(text.to_owned()))?;
    let minor = read(first)
        .ok_or_else(|| MusicError::UnknownNumeral((*first).to_owned()))?
        .lower;
    parts
        .iter()
        .map(|part| parse_numeral(part, minor))
        .collect()
}

/// Parses one numeral in a minor or a major line.
///
/// # Errors
///
/// [`MusicError::UnknownNumeral`] when the text is no numeral.
pub fn parse_numeral(text: &str, minor_line: bool) -> Result<Numeral, MusicError> {
    let parsed = read(text).ok_or_else(|| MusicError::UnknownNumeral(text.to_owned()))?;
    let quality = match parsed.sign {
        Some('°' | 'o') => Quality::Diminished,
        Some('ø') => Quality::HalfDiminished,
        Some('+') => Quality::Augmented,
        _ if parsed.lower => Quality::Minor,
        _ => Quality::Major,
    };
    let offset = match parsed.accidental {
        0 if minor_line && parsed.degree == 6 && quality == Quality::Diminished => 11,
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
    let known_chars = extension
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '#' | '/'));
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
        parse_line(line)
            .unwrap_or_else(|error| panic!("{line}: {error}"))
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
    fn both_minor_spellings_name_the_same_chords() {
        assert_eq!(offsets("i-VI-III-VII"), offsets("i-bVI-bIII-bVII"));
        assert_eq!(offsets("I-bVII-IV"), [0, 10, 5]);
        assert_eq!(offsets("bII-i"), [1, 0]);
        assert_eq!(offsets("i-iv-V"), [0, 5, 7]);
        assert_eq!(offsets("i vii°"), [0, 11]);
    }

    #[test]
    fn keeps_quality_and_extension() {
        let line = parse_line("ii7-V7-Imaj7 iiø7").unwrap();
        let read: Vec<(u8, Quality, &str)> = line
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
        assert_eq!(parse_line("III+").unwrap()[0].quality, Quality::Augmented);
    }

    #[test]
    fn rejects_text_that_is_no_numeral() {
        for text in ["", "X", "Iv", "VIII", "IVx", "C-G-Am"] {
            assert!(parse_line(text).is_err(), "{text}");
        }
    }
}
