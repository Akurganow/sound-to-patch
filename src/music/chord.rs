//! The chord-symbol parser.
//!
//! A symbol is a root letter with an optional sharp or flat, a spelling
//! from a chord entry of the dictionary, and an optional `/X` bass note.
//! Parentheses list alterations (`C7(b9,#11)`), and `no3`, `omit3` and
//! `no5` remove a tone. A `5` straight after the root is the power chord,
//! so `Cb5` is a power chord on Cb, not C with a flat fifth. A spelling
//! outside the dictionary is an error that names it. The tones of a symbol with several readings follow the
//! dictionary's policy, not any one program.
//!
//! ```
//! use sound_to_patch::music::chord::parse_chord;
//!
//! let chord = parse_chord("Am7/G")?;
//! assert_eq!((chord.root, chord.tones.as_slice(), chord.bass), (9, &[0, 3, 7, 10][..], Some(7)));
//! assert_eq!(chord.entry, Some("chord.minor-seventh"));
//! # Ok::<(), sound_to_patch::music::MusicError>(())
//! ```

use std::collections::{BTreeSet, HashMap};
use std::sync::OnceLock;

use crate::music::entry::{Group, Rule};
use crate::music::pitch::letter_class;
use crate::music::{MusicError, dictionary};

/// The tones the parser gives `alt`: the dominant's root, third and
/// seventh with a sharp ninth and a flat thirteenth.
pub const ALT_TONES: [u8; 5] = [0, 4, 10, 15, 20];

/// The one chord entry that chooses its tones; its spellings take
/// [`ALT_TONES`].
pub const ALTERED_DOMINANT: &str = "chord.altered-dominant";

/// A parsed chord symbol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChordSymbol {
    /// The root pitch class.
    pub root: u8,
    /// Semitones above the root, rising; 14 is a ninth.
    pub tones: Vec<u8>,
    /// The bass pitch class after a slash, if any.
    pub bass: Option<u8>,
    /// The chord entry with these tones: the entry of the spelling, or after
    /// alterations the entry whose tones match. None when no entry has
    /// exactly these tones.
    pub entry: Option<&'static str>,
}

impl ChordSymbol {
    /// The pitch classes the chord sounds, bass included.
    #[must_use]
    pub fn classes(&self) -> BTreeSet<u8> {
        self.tones
            .iter()
            .map(|tone| (self.root + tone) % 12)
            .chain(self.bass)
            .collect()
    }
}

struct Spellings {
    by_symbol: HashMap<&'static str, (Vec<u8>, &'static str)>,
    by_tones: HashMap<Vec<u8>, &'static str>,
}

fn spellings() -> Result<&'static Spellings, MusicError> {
    static MAP: OnceLock<Result<Spellings, MusicError>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut by_symbol = HashMap::new();
        let mut by_tones = HashMap::new();
        for entry in dictionary()?.group(Group::Chord) {
            let Rule::ChordTones { offsets, .. } = &entry.rule else {
                continue;
            };
            let tones = if entry.id == ALTERED_DOMINANT {
                ALT_TONES.to_vec()
            } else {
                offsets.clone()
            };
            for symbol in &entry.symbols {
                by_symbol.insert(symbol.as_str(), (tones.clone(), entry.id.as_str()));
            }
            by_tones.entry(tones).or_insert(entry.id.as_str());
        }
        Ok(Spellings {
            by_symbol,
            by_tones,
        })
    })
    .as_ref()
    .map_err(Clone::clone)
}

/// Parses a chord symbol such as `Cmaj7`, `F#m7b5`, `Bb7(b9)`, `C13b9` or
/// `C/E`.
///
/// # Errors
///
/// [`MusicError::UnsupportedChord`] naming the symbol when its spelling is
/// not in the dictionary, and [`MusicError::BrokenData`] when the
/// dictionary does not load.
pub fn parse_chord(text: &str) -> Result<ChordSymbol, MusicError> {
    let unsupported = || MusicError::UnsupportedChord(text.to_owned());
    let trimmed = text.trim();
    let (root, rest) = read_root(trimmed).ok_or_else(unsupported)?;
    let spellings = spellings()?;
    let (quality_text, bass) = if quality(rest, spellings).is_some() {
        (rest, None)
    } else {
        let (quality_text, bass_text) = rest.rsplit_once('/').ok_or_else(unsupported)?;
        let bass = read_root(bass_text)
            .filter(|(_, after)| after.is_empty())
            .map(|(bass, _)| bass)
            .ok_or_else(unsupported)?;
        (quality_text, Some(bass))
    };
    let (tones, entry) = quality(quality_text, spellings).ok_or_else(unsupported)?;
    let entry = entry.or_else(|| spellings.by_tones.get(&tones).copied());
    Ok(ChordSymbol {
        root,
        tones,
        bass,
        entry,
    })
}

/// An upper-case letter with up to two sharps or two flats.
fn read_root(text: &str) -> Option<(u8, &str)> {
    let letter = text.chars().next().filter(char::is_ascii_uppercase)?;
    let class = letter_class(letter)?;
    let mut rest = &text[1..];
    let mut step = 0;
    for _ in 0..2 {
        let sign = match rest.chars().next() {
            Some(sign @ ('#' | '♯')) if step >= 0 => {
                step += 1;
                sign
            }
            Some(sign @ ('b' | '♭')) if step <= 0 => {
                step -= 1;
                sign
            }
            _ => break,
        };
        rest = &rest[sign.len_utf8()..];
    }
    u8::try_from((class + step).rem_euclid(12))
        .ok()
        .map(|root| (root, rest))
}

/// The tones of the text after the root, and the entry when the text is a
/// spelling: a spelling, a spelling with a removed tone, with alterations
/// in parentheses, or with trailing alterations such as `13b9`.
fn quality(text: &str, spellings: &Spellings) -> Option<(Vec<u8>, Option<&'static str>)> {
    // A bare root is the major triad.
    let text = if text.is_empty() { "maj" } else { text };
    if let Some((tones, entry)) = spellings.by_symbol.get(text) {
        return Some((tones.clone(), Some(entry)));
    }
    for (suffix, removed) in [
        ("(no3)", [3, 4]),
        ("(omit3)", [3, 4]),
        ("no3", [3, 4]),
        ("omit3", [3, 4]),
        ("(no5)", [7, 7]),
        ("(omit5)", [7, 7]),
        ("no5", [7, 7]),
        ("omit5", [7, 7]),
    ] {
        if let Some(base) = text.strip_suffix(suffix) {
            let (mut tones, _) = quality(base, spellings)?;
            tones.retain(|tone| !removed.contains(tone));
            return Some((tones, None));
        }
    }
    if let Some((base, alterations)) = text.strip_suffix(')').and_then(|t| t.split_once('(')) {
        let (mut tones, _) = quality(base, spellings)?;
        for alteration in alterations.split(',') {
            alter(&mut tones, alteration.trim())?;
        }
        return Some((tones, None));
    }
    for sign in ["b", "♭", "#", "♯"] {
        for degree in ["13", "11", "9", "5"] {
            let token = format!("{sign}{degree}");
            if let Some(base) = text.strip_suffix(&token)
                && !base.is_empty()
                && let Some((mut tones, _)) = quality(base, spellings)
            {
                alter(&mut tones, &token)?;
                return Some((tones, None));
            }
        }
    }
    None
}

/// Applies one alteration such as `b9`, `#11`, `add13` or `maj7`.
fn alter(tones: &mut Vec<u8>, alteration: &str) -> Option<()> {
    match alteration {
        "maj7" => {
            tones.retain(|tone| *tone != 10);
            tones.push(11);
        }
        "maj9" => {
            tones.retain(|tone| *tone != 10);
            tones.extend([11, 14]);
        }
        _ => {
            let text = alteration.strip_prefix("add").unwrap_or(alteration);
            let (step, degree) = match text.chars().next()? {
                'b' | '♭' | '-' => (-1, &text[text.chars().next()?.len_utf8()..]),
                '#' | '♯' | '+' => (1, &text[text.chars().next()?.len_utf8()..]),
                _ => (0, text),
            };
            let natural: i16 = match degree {
                "5" => 7,
                "9" => 14,
                "11" => 17,
                "13" => 21,
                _ => return None,
            };
            let tone = u8::try_from(natural + step).ok()?;
            let replaced = u8::try_from(natural).ok()?;
            tones.retain(|existing| *existing != replaced);
            tones.push(tone);
        }
    }
    tones.sort_unstable();
    tones.dedup();
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tones(text: &str) -> Vec<u8> {
        parse_chord(text)
            .unwrap_or_else(|error| panic!("{text}: {error}"))
            .tones
    }

    #[test]
    fn parses_roots_and_slash_basses() {
        let chord = parse_chord("F#m7b5").unwrap();
        assert_eq!(
            (chord.root, chord.tones.as_slice()),
            (6, &[0, 3, 6, 10][..])
        );
        let chord = parse_chord("C/E").unwrap();
        assert_eq!((chord.root, chord.bass), (0, Some(4)));
        assert_eq!(chord.classes(), BTreeSet::from([0, 4, 7]));
        let chord = parse_chord("Bb").unwrap();
        assert_eq!((chord.root, chord.entry), (10, Some("chord.major-triad")));
        let chord = parse_chord("Dbb7").unwrap();
        assert_eq!(
            (chord.root, chord.entry),
            (0, Some("chord.dominant-seventh"))
        );
        assert_eq!(parse_chord("C/Bbb").unwrap().bass, Some(9));
        assert_eq!(parse_chord("F##").unwrap().root, 7);
        let chord = parse_chord("Cb5").unwrap();
        assert_eq!((chord.root, chord.tones.as_slice()), (11, &[0, 7][..]));
    }

    #[test]
    fn applies_alterations_and_removals() {
        assert_eq!(tones("C7(b9,#11)"), [0, 4, 7, 10, 13, 18]);
        assert_eq!(tones("C7b9#11"), [0, 4, 7, 10, 13, 18]);
        assert_eq!(tones("C7#9b13"), [0, 4, 7, 10, 15, 20]);
        assert_eq!(tones("C13b9"), [0, 4, 7, 10, 13, 21]);
        assert_eq!(tones("Cm(maj7)"), [0, 3, 7, 11]);
        assert_eq!(tones("Cm(maj9)"), [0, 3, 7, 11, 14]);
        assert_eq!(tones("C7alt"), ALT_TONES);
    }

    #[test]
    fn removals_drop_the_third_or_the_fifth() {
        for text in ["C7(no3)", "C7no3", "C7omit3", "C7(omit3)"] {
            assert_eq!(tones(text), [0, 7, 10], "{text}");
        }
        assert_eq!(tones("Cm7no3"), [0, 7, 10]);
        for text in ["C7(no5)", "C7no5", "C7omit5", "C7(omit5)"] {
            assert_eq!(tones(text), [0, 4, 10], "{text}");
        }
        assert_eq!(tones("C6no5"), [0, 4, 9]);
        assert_eq!(tones("C13no5"), [0, 4, 10, 14, 21]);
    }

    #[test]
    fn an_altered_chord_takes_the_entry_of_its_tones() {
        assert_eq!(
            parse_chord("Cm7(b5)").unwrap().entry,
            Some("chord.half-diminished")
        );
        assert_eq!(
            parse_chord("Cm(maj7)").unwrap().entry,
            Some("chord.minor-major-seventh")
        );
        assert_eq!(parse_chord("C7(no5)").unwrap().entry, None);
    }

    #[test]
    fn names_an_unsupported_symbol() {
        for text in [
            "Cxyz", "c", "H7", "C/H", "C7(b10)", "", "C2", "C4", "Cadd2", "C#b7",
        ] {
            let error = parse_chord(text).unwrap_err();
            assert_eq!(
                error,
                MusicError::UnsupportedChord(text.to_owned()),
                "{text}"
            );
        }
        assert!(
            parse_chord("Cxyz")
                .unwrap_err()
                .to_string()
                .contains("`Cxyz`")
        );
    }
}
