//! The word dictionary: what a producer's sound word means.
//!
//! An entry says what a word such as "warm" or "plucky" means, how far
//! listeners agree on it and which analyzer number checks it. The entries
//! name no synth control; the per-synth references bind them to controls.
//!
//! The data lives in `dictionary/words/`: one file per axis, the listener
//! numbers of `agreement.json` (written by `cargo xtask agreement`), the
//! intensity ladder and the style and technique lists. The binary embeds
//! them and parses them once ([`dictionary`]).
//!
//! A word with several senses has one entry per sense and one default.
//! [`Words::pick`] takes the sense a named role selects, else the default.
//! [`Words::question`] is the one short question that picks a sense.
//!
//! ```
//! use sound_to_patch::words::dictionary;
//!
//! let words = dictionary()?;
//! let dark = words.pick("dark", None).unwrap();
//! assert_eq!(dark.agreement.n, 8);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

pub mod check;

/// One axis file of the dictionary.
#[derive(Clone, Copy, Debug)]
pub struct AxisFile {
    /// The axis.
    pub axis: Axis,
    data: &'static str,
}

/// Every axis file, in reference order.
pub const AXES: [AxisFile; 8] = [
    AxisFile {
        axis: Axis::Tone,
        data: include_str!("../dictionary/words/tone.json"),
    },
    AxisFile {
        axis: Axis::Body,
        data: include_str!("../dictionary/words/body.json"),
    },
    AxisFile {
        axis: Axis::Texture,
        data: include_str!("../dictionary/words/texture.json"),
    },
    AxisFile {
        axis: Axis::Structure,
        data: include_str!("../dictionary/words/structure.json"),
    },
    AxisFile {
        axis: Axis::Space,
        data: include_str!("../dictionary/words/space.json"),
    },
    AxisFile {
        axis: Axis::Movement,
        data: include_str!("../dictionary/words/movement.json"),
    },
    AxisFile {
        axis: Axis::Envelope,
        data: include_str!("../dictionary/words/envelope.json"),
    },
    AxisFile {
        axis: Axis::Mood,
        data: include_str!("../dictionary/words/mood.json"),
    },
];

const AGREEMENT: &str = include_str!("../dictionary/words/agreement.json");
const LADDER: &str = include_str!("../dictionary/words/ladder.json");
const STYLE: &str = include_str!("../dictionary/words/style.json");
const TECHNIQUE: &str = include_str!("../dictionary/words/technique.json");

/// The kind of sound change a word names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Axis {
    /// Brightness and spectral balance.
    Tone,
    /// Weight and low end.
    Body,
    /// Grit, noise and distortion.
    Texture,
    /// Harmonic structure and resonance.
    Structure,
    /// Width, room and distance.
    Space,
    /// Change over time.
    Movement,
    /// Attack, decay and dynamics.
    Envelope,
    /// Mood and character, with no control.
    Mood,
}

impl Axis {
    /// The file stem under `dictionary/words/`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tone => "tone",
            Self::Body => "body",
            Self::Texture => "texture",
            Self::Structure => "structure",
            Self::Space => "space",
            Self::Movement => "movement",
            Self::Envelope => "envelope",
            Self::Mood => "mood",
        }
    }
}

/// The agreement class of a word. Our thresholds: high from 0.45, medium
/// from 0.20, else low; known only from [`KNOWN_FROM`] curves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Class {
    /// Agreement of 0.45 or more.
    High,
    /// Agreement from 0.20 to under 0.45.
    Medium,
    /// Agreement under 0.20.
    Low,
    /// Listener curves exist, but fewer than [`KNOWN_FROM`].
    Unknown,
    /// No listener curve holds the word.
    #[serde(rename = "no data")]
    NoData,
}

/// The fewest curves for a known class; fewer set the few-sample flag.
pub const KNOWN_FROM: u32 = 8;

impl Class {
    /// The class of an agreement value, rounded to two decimals, over `n`
    /// curves.
    #[must_use]
    pub fn of(value: Option<f64>, n: u32) -> Self {
        match value {
            _ if n == 0 => Self::NoData,
            Some(value) if n >= KNOWN_FROM => {
                if value >= 0.45 {
                    Self::High
                } else if value >= 0.20 {
                    Self::Medium
                } else {
                    Self::Low
                }
            }
            _ => Self::Unknown,
        }
    }
}

/// How far listeners agree on a word's EQ curve.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agreement {
    /// The mean pairwise correlation of the word's curves, two decimals.
    pub value: Option<f64>,
    /// The number of curves.
    pub n: u32,
    /// The class of the value.
    pub class: Class,
}

/// A word's effect on the five reverb parameters, in global standard
/// deviations from the global mean.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(missing_docs)]
pub struct Effects {
    pub delay_time: f64,
    pub decay: f64,
    pub stereo_spread: f64,
    pub cutoff_freq: f64,
    pub wet_gain: f64,
}

/// The listener reverb numbers of a word.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reverb {
    /// The number of reverb settings the word labels.
    pub n: u32,
    /// The word's variance over the global variance, averaged over the
    /// parameters: lower is more shared, 1.0 is none.
    pub spread_ratio: f64,
    /// The mean effect per parameter.
    pub effects: Effects,
}

/// The analyzer number that checks a sense.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measure {
    /// The field of the layer report, such as `centroid_over_f0`.
    pub key: String,
    /// The band of a `band_share` measure.
    pub band: Option<String>,
    /// `up` or `down`.
    pub direction: String,
}

/// How many sources back a number or a meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tag {
    /// Two or more sources state it.
    #[serde(rename = "source states")]
    SourceStates,
    /// One source states it.
    #[serde(rename = "single source")]
    SingleSource,
    /// Only search excerpts back it.
    #[serde(rename = "snippet")]
    Snippet,
    /// The listener data alone backs it.
    #[serde(rename = "crowd data")]
    CrowdData,
    /// No source states it.
    #[serde(rename = "inferred")]
    Inferred,
}

/// A number of an entry with its sources.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Value {
    /// What the number is.
    pub what: String,
    /// The low end, or the number.
    pub low: f64,
    /// The high end; equal to `low` for one number.
    pub high: f64,
    /// The unit, such as `Hz` or `cents`.
    pub unit: String,
    /// Source ids that state it.
    pub sources: Vec<String>,
    /// How many of them back it.
    pub tag: Tag,
}

/// One sense of a producer word.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// `<word>.<sense>`.
    pub id: String,
    /// The lead word of the cluster.
    pub word: String,
    /// Synonyms the entry covers.
    pub aliases: Vec<String>,
    /// The axis.
    pub axis: Axis,
    /// The sense id within the word.
    pub sense: String,
    /// The option text of this sense in the word's question.
    pub label: Option<String>,
    /// The plain meaning, with no number.
    pub meaning: String,
    /// The roles the sense applies to; empty for all.
    pub roles: Vec<String>,
    /// Senses this one cannot share a sound with.
    pub conflicts: Vec<String>,
    /// The sense taken when nothing else decides.
    pub default: bool,
    /// The listener agreement of the word.
    pub agreement: Agreement,
    /// Listener curves exist, but too few for a class.
    pub few_sample: bool,
    /// The listener reverb numbers of the word.
    pub reverb: Option<Reverb>,
    /// The analyzer number that checks the sense.
    pub measure: Option<Measure>,
    /// The numbers, each with its sources.
    pub values: Vec<Value>,
    /// `no control` for mood and character words.
    pub control: Option<String>,
    /// The card the word hints at, or `unmapped`.
    pub card_hint: Option<String>,
    /// Music dictionary entries the word points to.
    #[serde(default)]
    pub music: Vec<String>,
    /// What backs the meaning.
    pub tag: Tag,
    /// The sources of the meaning.
    pub sources: Vec<String>,
}

/// The listener numbers of one EQ word.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EqWord {
    /// The lowercase word.
    pub word: String,
    /// The number of curves.
    pub n: u32,
    /// The agreement value, two decimals; none below two curves.
    pub value: Option<f64>,
    /// The class of the value.
    pub class: Class,
    /// Fewer curves than [`KNOWN_FROM`].
    pub few_sample: bool,
    /// The mean curve per band, from five curves.
    pub bands: Option<BTreeMap<String, f64>>,
    /// The band centre of the mean curve's peak, from five curves.
    pub peak_hz: Option<u32>,
}

/// The listener reverb numbers of one word.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReverbWord {
    /// The lowercase word.
    pub word: String,
    /// The number of settings, at least five.
    pub n: u32,
    /// See [`Reverb::spread_ratio`].
    pub spread_ratio: f64,
    /// See [`Reverb::effects`].
    pub effects: Effects,
}

/// The derived listener numbers of `agreement.json`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Listeners {
    /// The source id.
    pub source: String,
    /// The pinned revision of the listener data.
    pub revision: String,
    /// The tool versions that computed the numbers.
    pub tools: BTreeMap<String, String>,
    /// EQ curves read.
    pub eq_curves: u32,
    /// Reverb settings read.
    pub reverb_rows: u32,
    /// Distinct reverb parameter sets among them.
    pub reverb_distinct_settings: u32,
    /// Every EQ word.
    pub eq: Vec<EqWord>,
    /// Every reverb word with at least five settings.
    pub reverb: Vec<ReverbWord>,
}

/// A row of the style or technique list.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ranked {
    /// The rank in the published list.
    pub rank: u32,
    /// The lead word.
    pub word: String,
    /// The other words of the group.
    pub aliases: Vec<String>,
    /// Style words: `pending`, a card or `no hint`.
    pub hint: Option<String>,
    /// Technique words: the neutral control group, or none when unmapped.
    pub group: Option<String>,
}

/// A modifier group of the intensity ladder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Modifier {
    /// The modifier words, such as `slightly`.
    pub words: Vec<String>,
    /// `mild`, `medium`, `strong`, or `against` for one step back.
    pub step: String,
    /// Add a warning about harshness, muddiness or mono loss.
    #[serde(default)]
    pub warn: bool,
}

/// A sourced number that anchors one step of one kind of control.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    /// The kind of control, in neutral words, such as `eq gain`.
    pub control: String,
    /// `mild`, `medium`, `strong`, or `edge` where a source names a defect.
    pub step: String,
    /// The number and its sources.
    pub value: Value,
}

/// The intensity ladder: which step a modifier picks, and the sourced
/// anchors per kind of control.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ladder {
    /// How the steps are known: no source quantifies the modifiers.
    pub tag: Tag,
    /// The rule in plain words.
    pub meaning: String,
    /// The modifier groups.
    pub modifiers: Vec<Modifier>,
    /// The anchors.
    pub anchors: Vec<Anchor>,
}

/// The parsed word dictionary.
#[derive(Debug)]
pub struct Words {
    /// Every entry, axis by axis in file order.
    pub entries: Vec<Entry>,
    /// The listener numbers.
    pub listeners: Listeners,
    /// The intensity ladder.
    pub ladder: Ladder,
    /// The style words in rank order.
    pub style: Vec<Ranked>,
    /// The technique words in rank order.
    pub technique: Vec<Ranked>,
}

impl Words {
    /// The entry with this id.
    #[must_use]
    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    /// The senses of a word, found by its lead word or an alias.
    #[must_use]
    pub fn senses(&self, word: &str) -> Vec<&Entry> {
        let word = word.to_lowercase();
        let lead = self
            .entries
            .iter()
            .find(|entry| entry.word == word || entry.aliases.contains(&word))
            .map(|entry| entry.word.as_str());
        self.entries
            .iter()
            .filter(|entry| Some(entry.word.as_str()) == lead)
            .collect()
    }

    /// The sense to take: the one sense that lists `role`, else the
    /// default.
    #[must_use]
    pub fn pick(&self, word: &str, role: Option<&str>) -> Option<&Entry> {
        let senses = self.senses(word);
        if let Some(role) = role {
            let named: Vec<&&Entry> = senses
                .iter()
                .filter(|entry| entry.roles.iter().any(|r| r == role))
                .collect();
            if let [only] = named.as_slice() {
                return Some(only);
            }
        }
        senses.into_iter().find(|entry| entry.default)
    }

    /// The question that picks a sense, such as "Warm: more low-mid
    /// weight (default), duller highs or both?". None for a word with one
    /// sense.
    #[must_use]
    pub fn question(&self, word: &str) -> Option<String> {
        let senses = self.senses(word);
        if senses.len() < 2 {
            return None;
        }
        let mut options: Vec<String> = Vec::new();
        for entry in senses.iter().filter(|entry| entry.default) {
            options.extend(entry.label.iter().map(|label| format!("{label} (default)")));
        }
        for entry in senses.iter().filter(|entry| !entry.default) {
            options.extend(entry.label.iter().cloned());
        }
        let last = options.pop()?;
        let mut lead = senses[0].word.clone();
        if let Some(first) = lead.get_mut(..1) {
            first.make_ascii_uppercase();
        }
        Some(format!("{lead}: {} or {last}?", options.join(", ")))
    }

    /// The listener numbers of an EQ word.
    #[must_use]
    pub fn eq(&self, word: &str) -> Option<&EqWord> {
        self.listeners.eq.iter().find(|row| row.word == word)
    }

    /// The listener reverb numbers of a word.
    #[must_use]
    pub fn reverb(&self, word: &str) -> Option<&ReverbWord> {
        self.listeners.reverb.iter().find(|row| row.word == word)
    }
}

/// The word dictionary, parsed once.
///
/// # Errors
///
/// [`WordsError`] when a data file does not parse or an entry breaks a
/// rule of [`check::problems`].
pub fn dictionary() -> Result<&'static Words, WordsError> {
    static WORDS: OnceLock<Result<Words, String>> = OnceLock::new();
    WORDS
        .get_or_init(build)
        .as_ref()
        .map_err(|cause| WordsError(cause.clone()))
}

fn build() -> Result<Words, String> {
    let words = parse()?;
    let problems = check::problems(&words);
    if problems.is_empty() {
        Ok(words)
    } else {
        Err(problems.join("; "))
    }
}

/// Parses the embedded files without checking them.
pub(crate) fn parse() -> Result<Words, String> {
    fn read<T: serde::de::DeserializeOwned>(name: &str, data: &str) -> Result<T, String> {
        serde_json::from_str(data).map_err(|error| format!("dictionary/words/{name}: {error}"))
    }
    let mut entries = Vec::new();
    for file in AXES {
        entries.extend(read::<Vec<Entry>>(
            &format!("{}.json", file.axis.as_str()),
            file.data,
        )?);
    }
    let ladder = read("ladder.json", LADDER)?;
    let listeners = read("agreement.json", AGREEMENT)?;
    let style = read("style.json", STYLE)?;
    let technique = read("technique.json", TECHNIQUE)?;
    Ok(Words {
        entries,
        listeners,
        ladder,
        style,
        technique,
    })
}

/// The embedded word data does not parse or breaks a rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WordsError(pub String);

impl fmt::Display for WordsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the word dictionary is broken: {}", self.0)
    }
}

impl Error for WordsError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_class_follows_the_thresholds_and_the_curve_count() {
        assert_eq!(Class::of(Some(0.45), 8), Class::High);
        assert_eq!(Class::of(Some(0.44), 8), Class::Medium);
        assert_eq!(Class::of(Some(0.20), 29), Class::Medium);
        assert_eq!(Class::of(Some(0.19), 9), Class::Low);
        assert_eq!(Class::of(Some(-0.03), 15), Class::Low);
        assert_eq!(Class::of(Some(0.82), 7), Class::Unknown);
        assert_eq!(Class::of(None, 1), Class::Unknown);
        assert_eq!(Class::of(None, 0), Class::NoData);
    }
}
