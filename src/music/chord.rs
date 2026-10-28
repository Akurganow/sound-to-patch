//! The chord-symbol parser.
//!
//! A symbol is a root letter with an optional sharp or flat, a spelling
//! from a chord entry of the dictionary, and an optional `/X` bass note.
//! Parentheses list alterations (`C7(b9,#11)`), and `no3`, `omit3` and
//! `no5` remove a tone. A spelling outside the dictionary is an error that
//! names it. The tones of a symbol with several readings follow the
//! dictionary's policy, not any one program.
//!
//! ```
//! use sound_to_patch::music::chord::parse_chord;
//!
//! let chord = parse_chord("Am7/G")?;
//! assert_eq!((chord.root, chord.tones.as_slice(), chord.bass), (9, &[0, 3, 7, 10][..], Some(7)));
//! assert_eq!(chord.entry, "chord.minor-seventh");
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

/// A parsed chord symbol.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChordSymbol {
    /// The root pitch class.
    pub root: u8,
    /// Semitones above the root, rising; 14 is a ninth.
    pub tones: Vec<u8>,
    /// The bass pitch class after a slash, if any.
    pub bass: Option<u8>,
    /// The id of the chord entry the spelling belongs to.
    pub entry: &'static str,
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

struct Spelling {
    tones: Vec<u8>,
    entry: &'static str,
}

fn spellings() -> Result<&'static HashMap<&'static str, Spelling>, MusicError> {
    static MAP: OnceLock<Result<HashMap<&'static str, Spelling>, MusicError>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut map = HashMap::new();
        for entry in dictionary()?.group(Group::Chord) {
            let Rule::ChordTones { offsets, choose } = &entry.rule else {
                continue;
            };
            let tones = if choose.is_some() {
                ALT_TONES.to_vec()
            } else {
                offsets.clone()
            };
            for symbol in &entry.symbols {
                map.insert(
                    symbol.as_str(),
                    Spelling {
                        tones: tones.clone(),
                        entry: &entry.id,
                    },
                );
            }
        }
        Ok(map)
    })
    .as_ref()
    .map_err(Clone::clone)
}

/// Parses a chord symbol such as `Cmaj7`, `F#m7b5`, `Bb7(b9)` or `C/E`.
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
    let map = spellings()?;
    if let Some((tones, entry)) = quality(rest, map) {
        return Ok(ChordSymbol {
            root,
            tones,
            bass: None,
            entry,
        });
    }
    let (quality_text, bass_text) = rest.rsplit_once('/').ok_or_else(unsupported)?;
    let bass = read_root(bass_text)
        .filter(|(_, after)| after.is_empty())
        .map(|(bass, _)| bass)
        .ok_or_else(unsupported)?;
    let (tones, entry) = quality(quality_text, map).ok_or_else(unsupported)?;
    Ok(ChordSymbol {
        root,
        tones,
        bass: Some(bass),
        entry,
    })
}

/// An upper-case letter with at most one sharp or flat.
fn read_root(text: &str) -> Option<(u8, &str)> {
    let letter = text.chars().next().filter(char::is_ascii_uppercase)?;
    let class = letter_class(letter)?;
    let rest = &text[1..];
    let (step, rest) = match rest.chars().next() {
        Some(sign @ ('#' | '♯')) => (1, &rest[sign.len_utf8()..]),
        Some(sign @ ('b' | '♭')) => (-1, &rest[sign.len_utf8()..]),
        _ => (0, rest),
    };
    u8::try_from((class + step).rem_euclid(12))
        .ok()
        .map(|root| (root, rest))
}

/// The tones of the text after the root: a spelling, a spelling with
/// alterations in parentheses, or a spelling with a removed tone.
fn quality(text: &str, map: &HashMap<&str, Spelling>) -> Option<(Vec<u8>, &'static str)> {
    // A bare root is the major triad.
    let text = if text.is_empty() { "maj" } else { text };
    if let Some(spelling) = map.get(text) {
        return Some((spelling.tones.clone(), spelling.entry));
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
            let (mut tones, entry) = quality(base, map)?;
            tones.retain(|tone| !removed.contains(tone));
            return Some((tones, entry));
        }
    }
    let (base, alterations) = text.strip_suffix(')')?.split_once('(')?;
    let (mut tones, entry) = quality(base, map)?;
    for alteration in alterations.split(',') {
        alter(&mut tones, alteration.trim())?;
    }
    Some((tones, entry))
}

/// Applies one alteration such as `b9`, `#11` or `add13`.
fn alter(tones: &mut Vec<u8>, alteration: &str) -> Option<()> {
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
    tones.sort_unstable();
    tones.dedup();
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!((chord.root, chord.entry), (10, "chord.major-triad"));
    }

    #[test]
    fn applies_alterations_and_removals() {
        let chord = parse_chord("C7(b9,#11)").unwrap();
        assert_eq!(chord.tones, [0, 4, 7, 10, 13, 18]);
        let chord = parse_chord("C7(no3)").unwrap();
        assert_eq!(chord.tones, [0, 7, 10]);
        assert_eq!(parse_chord("C7alt").unwrap().tones, ALT_TONES);
    }

    #[test]
    fn names_an_unsupported_symbol() {
        for text in ["Cxyz", "c", "H7", "C/H", "C7(b10)", ""] {
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
