//! The music dictionary for notes: scales, keys, chords, progressions,
//! rhythm and technique words, each with a checkable note rule, and the
//! parsers and functions that read them.
//!
//! The data lives in `dictionary/music/`, one JSON file per topic. The
//! binary embeds those files and parses them once ([`dictionary`]); it
//! reads nothing else from the dictionary. `cargo xtask reference` writes
//! the reference files under `reference/core/` from the same data.
//!
//! - [`entry`]: the entry, its rule and its example fragments.
//! - [`check`]: the rules every entry must meet, the coverage of the
//!   tiers and the recomputed shares.
//! - [`pitch`]: pitch classes, spelling and the three middle-C labels.
//! - [`key`]: the key parser, Camelot and Open Key.
//! - [`roman`]: Roman numerals as semitones.
//! - [`chord`]: the chord-symbol parser.
//! - [`scale`]: scale lookup and the snap rule.
//! - [`rhythm`]: Euclidean patterns, swing and tick arithmetic.
//!
//! ```
//! use sound_to_patch::music::{dictionary, key::parse_key};
//!
//! let phrygian = dictionary()?.entry("scale.phrygian").unwrap();
//! assert_eq!(phrygian.word, "Phrygian");
//! let reading = parse_key("8A")?;
//! assert_eq!(reading.key.to_string(), "A minor");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::{BTreeMap, HashMap};
use std::error::Error;
use std::fmt;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::music::entry::{Entry, Group, Tag, Tier};

pub mod check;
pub mod chord;
pub mod entry;
pub mod key;
pub mod pitch;
pub mod rhythm;
pub mod roman;
pub mod scale;

/// One topic file of the dictionary and the groups it may hold.
#[derive(Clone, Copy, Debug)]
pub struct Topic {
    /// The file stem under `dictionary/music/`, and of the reference file.
    pub name: &'static str,
    /// A title for people.
    pub title: &'static str,
    /// The groups its entries may belong to.
    pub groups: &'static [Group],
    data: &'static str,
}

/// Every topic, in reference order.
pub const TOPICS: [Topic; 10] = [
    Topic {
        name: "notation",
        title: "Notes, keys and numerals",
        groups: &[Group::Notation, Group::Key],
        data: include_str!("../dictionary/music/notation.json"),
    },
    Topic {
        name: "roles",
        title: "Role pitch ranges",
        groups: &[Group::Role],
        data: include_str!("../dictionary/music/roles.json"),
    },
    Topic {
        name: "scales",
        title: "Scales, moods and outside notes",
        groups: &[Group::Scale, Group::Mood, Group::Outside],
        data: include_str!("../dictionary/music/scales.json"),
    },
    Topic {
        name: "chords",
        title: "Chords and symbols",
        groups: &[Group::Chord],
        data: include_str!("../dictionary/music/chords.json"),
    },
    Topic {
        name: "voicing",
        title: "Voicing and voice leading",
        groups: &[Group::Voicing, Group::VoiceLeading],
        data: include_str!("../dictionary/music/voicing.json"),
    },
    Topic {
        name: "progressions",
        title: "Progressions and harmony",
        groups: &[Group::Progression, Group::Harmony],
        data: include_str!("../dictionary/music/progressions.json"),
    },
    Topic {
        name: "rhythm",
        title: "Rhythm, swing and feel",
        groups: &[
            Group::Position,
            Group::Swing,
            Group::Feel,
            Group::Rhythm,
            Group::Euclid,
        ],
        data: include_str!("../dictionary/music/rhythm.json"),
    },
    Topic {
        name: "technique",
        title: "Articulation and technique",
        groups: &[
            Group::Articulation,
            Group::Bass,
            Group::Lead,
            Group::Arp,
            Group::Pad,
        ],
        data: include_str!("../dictionary/music/technique.json"),
    },
    Topic {
        name: "phrase",
        title: "Phrase structure",
        groups: &[Group::Phrase],
        data: include_str!("../dictionary/music/phrase.json"),
    },
    Topic {
        name: "genres",
        title: "Genre templates",
        groups: &[Group::Genre],
        data: include_str!("../dictionary/music/genres.json"),
    },
];

const STATS: &str = include_str!("../dictionary/music/stats.json");
const PLAN: &str = include_str!("../dictionary/music/plan.json");

/// A share the dictionary states.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stat {
    /// Stable id.
    pub id: String,
    /// The set of shares that exclude each other, if any.
    pub set: Option<String>,
    /// What the share counts.
    pub claim: String,
    /// Percent, with the decimals the source gives.
    pub share: f64,
    /// The counted items, when the source gives counts.
    pub count: Option<u64>,
    /// All items, when the source gives counts.
    pub total: Option<u64>,
    /// Source ids.
    pub source: Vec<String>,
    /// How the share is known.
    pub tag: Tag,
}

/// The parsed dictionary.
#[derive(Debug)]
pub struct Dictionary {
    /// Every entry, topic by topic in file order.
    pub entries: Vec<Entry>,
    /// The stated shares.
    pub stats: Vec<Stat>,
    /// The planned entry count per group and tier.
    pub plan: BTreeMap<Group, BTreeMap<Tier, u16>>,
    topic_of: Vec<&'static str>,
    by_id: HashMap<String, usize>,
}

impl Dictionary {
    /// The entry with this id.
    #[must_use]
    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.by_id.get(id).map(|&index| &self.entries[index])
    }

    /// The entries of one group, in file order.
    pub fn group(&self, group: Group) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .filter(move |entry| entry.group == group)
    }

    /// The entries of one topic, in file order.
    pub fn topic(&self, name: &str) -> impl Iterator<Item = &Entry> {
        self.entries
            .iter()
            .zip(&self.topic_of)
            .filter(move |(_, topic)| **topic == name)
            .map(|(entry, _)| entry)
    }

    /// The stat with this id.
    #[must_use]
    pub fn stat(&self, id: &str) -> Option<&Stat> {
        self.stats.iter().find(|stat| stat.id == id)
    }
}

/// The dictionary, parsed once.
///
/// # Errors
///
/// [`MusicError::BrokenData`] when a data file does not parse or an entry
/// breaks a rule of [`check::problems`].
pub fn dictionary() -> Result<&'static Dictionary, MusicError> {
    static DICTIONARY: OnceLock<Result<Dictionary, String>> = OnceLock::new();
    DICTIONARY
        .get_or_init(build)
        .as_ref()
        .map_err(|cause| MusicError::BrokenData(cause.clone()))
}

fn build() -> Result<Dictionary, String> {
    let mut entries = Vec::new();
    let mut topic_of = Vec::new();
    for topic in TOPICS {
        let parsed: Vec<Entry> = serde_json::from_str(topic.data)
            .map_err(|error| format!("dictionary/music/{}.json: {error}", topic.name))?;
        topic_of.extend(std::iter::repeat_n(topic.name, parsed.len()));
        entries.extend(parsed);
    }
    let stats: Vec<Stat> = serde_json::from_str(STATS)
        .map_err(|error| format!("dictionary/music/stats.json: {error}"))?;
    let plan = serde_json::from_str(PLAN)
        .map_err(|error| format!("dictionary/music/plan.json: {error}"))?;
    let by_id = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| (entry.id.clone(), index))
        .collect();
    let dictionary = Dictionary {
        entries,
        stats,
        plan,
        topic_of,
        by_id,
    };
    let problems = check::problems(&dictionary);
    if problems.is_empty() {
        Ok(dictionary)
    } else {
        Err(problems.join("; "))
    }
}

/// A failure of the music dictionary or of one of its parsers.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MusicError {
    /// The embedded data does not parse or breaks a rule.
    BrokenData(String),
    /// A key name the grammar does not accept.
    UnknownKey(String),
    /// A note name the grammar does not accept, or a pitch outside 0 to
    /// 127.
    UnknownNote(String),
    /// A Roman numeral the grammar does not accept.
    UnknownNumeral(String),
    /// A chord symbol outside the dictionary.
    UnsupportedChord(String),
    /// A scale id outside the dictionary.
    UnknownScale(String),
}

impl fmt::Display for MusicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BrokenData(cause) => write!(f, "the music dictionary is broken: {cause}"),
            Self::UnknownKey(text) => write!(
                f,
                "`{text}` is not a key name: write a letter with an optional sharp or flat and \
                 mode, such as `A minor` or `Bb`, a Camelot code such as `8A`, or an Open Key \
                 code such as `1m`"
            ),
            Self::UnknownNote(text) => write!(
                f,
                "`{text}` is not a note name: write a letter, an optional sharp or flat and an \
                 octave number, such as `C4` or `F#2`, or a MIDI number from 0 to 127"
            ),
            Self::UnknownNumeral(text) => write!(
                f,
                "`{text}` is not a Roman numeral: write I to VII, upper case for major and \
                 lower case for minor, with an optional `b` or `#` before it"
            ),
            Self::UnsupportedChord(text) => write!(
                f,
                "chord symbol `{text}` is not in the music dictionary: write it with a \
                 spelling the chord reference lists, such as `Cmaj7` or `Am7b5`"
            ),
            Self::UnknownScale(text) => write!(
                f,
                "`{text}` is not a scale id: use an id from the scale reference, such as \
                 `scale.dorian`"
            ),
        }
    }
}

impl Error for MusicError {}
