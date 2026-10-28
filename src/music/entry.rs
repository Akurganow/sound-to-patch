//! One entry of the music dictionary, its checkable note rule and its
//! example fragments. `schemas/music.schema.json` describes the same
//! shape for editors.

use serde::{Deserialize, Serialize};

use crate::synth::Synth;

/// One sense of a producer word and the rule a note list must meet for it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// Stable id, `<group>.<word>` or `<group>.<word>.<sense>`.
    pub id: String,
    /// The topic the entry belongs to.
    pub group: Group,
    /// The word a producer uses.
    pub word: String,
    /// Other spellings and names of the word.
    pub aliases: Vec<String>,
    /// The plain meaning, in our own words.
    pub meaning: String,
    /// The sense id within the word, for a word with several senses.
    pub sense: Option<String>,
    /// The sense to take when the request gives no context.
    pub default: bool,
    /// Ids of the other senses of the word and of entries this one cannot
    /// share a note list with.
    pub conflicts: Vec<String>,
    /// The checkable note rule.
    pub rule: Rule,
    /// A fragment that meets the rule; none for [`Rule::None`].
    pub example_pass: Option<Fragment>,
    /// A fragment that breaks the rule; none for [`Rule::None`].
    pub example_fail: Option<Fragment>,
    /// The roles the entry applies to.
    pub roles: Vec<String>,
    /// Ids of the genre entries the entry belongs to.
    pub genres: Vec<String>,
    /// How much the entry is needed.
    pub tier: Tier,
    /// Independent publishers who state the claim, an upper bound.
    pub n_sources: u16,
    /// Ids from `dictionary/sources.json`.
    pub source: Vec<String>,
    /// How the claim is known.
    pub tag: Tag,
    /// The day the claim was last read, `YYYY-MM-DD`.
    pub verified: String,
    /// Whether the word alone names notes.
    pub notes_trigger: bool,
    /// The preset controls the entry needs, per synth.
    pub controls: Vec<Control>,
    /// Chord entries: the spellings after the root.
    pub symbols: Vec<String>,
    /// Genre and rhythm entries: the tempo range.
    pub bpm: Option<Bpm>,
    /// Progression entries: the scale for the chord-root check.
    pub scale: Option<String>,
    /// Ask words: the one question and its options.
    pub ask: Option<Ask>,
    /// Mood words: the scale to pick when the producer names none.
    pub default_pick: Option<DefaultPick>,
}

/// The topic of an entry. Each topic file holds one or more groups.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Group {
    /// Note names, middle C and spelling.
    Notation,
    /// Key names, Camelot, Open Key and Roman numerals.
    Key,
    /// The pitch range and default voice of a role.
    Role,
    /// Scales and modes.
    Scale,
    /// Mood words with a default scale.
    Mood,
    /// The snap rule and the allowed outside notes.
    Outside,
    /// Chord qualities and symbols.
    Chord,
    /// Voicing words and the low-interval limits.
    Voicing,
    /// Voice-leading rules.
    VoiceLeading,
    /// Named progressions.
    Progression,
    /// Harmonic rhythm, borrowed chords and genre chord habits.
    Harmony,
    /// Pulse and position words.
    Position,
    /// Swing.
    Swing,
    /// Feel and humanize words.
    Feel,
    /// Named rhythms.
    Rhythm,
    /// Euclidean patterns.
    Euclid,
    /// Note length and articulation.
    Articulation,
    /// Bass techniques.
    Bass,
    /// Lead and phrase techniques.
    Lead,
    /// Arpeggio techniques.
    Arp,
    /// Pad, stab and keys techniques.
    Pad,
    /// Phrase structure.
    Phrase,
    /// Genre templates.
    Genre,
}

impl Group {
    /// The text form, as in the data.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Notation => "notation",
            Self::Key => "key",
            Self::Role => "role",
            Self::Scale => "scale",
            Self::Mood => "mood",
            Self::Outside => "outside",
            Self::Chord => "chord",
            Self::Voicing => "voicing",
            Self::VoiceLeading => "voice-leading",
            Self::Progression => "progression",
            Self::Harmony => "harmony",
            Self::Position => "position",
            Self::Swing => "swing",
            Self::Feel => "feel",
            Self::Rhythm => "rhythm",
            Self::Euclid => "euclid",
            Self::Articulation => "articulation",
            Self::Bass => "bass",
            Self::Lead => "lead",
            Self::Arp => "arp",
            Self::Pad => "pad",
            Self::Phrase => "phrase",
            Self::Genre => "genre",
        }
    }
}

/// How much an entry is needed. M1 entries must all land.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Tier {
    /// Needed in a first test set.
    M1,
    /// One or two sources, or a jazz or world idiom with a rule.
    M2,
    /// Single source, inferred or theory only.
    M3,
}

impl Tier {
    /// Every tier, in order.
    pub const ALL: [Self; 3] = [Self::M1, Self::M2, Self::M3];

    /// The smallest share of the planned entries that must land, in
    /// percent.
    #[must_use]
    pub fn floor(self) -> u8 {
        match self {
            Self::M1 => 100,
            Self::M2 => 50,
            Self::M3 => 25,
        }
    }
}

/// How a claim is known. One tag per cell, never two.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Tag {
    /// A source states it.
    #[serde(rename = "source states")]
    SourceStates,
    /// Measured on data.
    Measured,
    /// Our computation on public data.
    Derived,
    /// Our reading.
    Inferred,
    /// Rests on a search excerpt.
    Snippet,
    /// One publisher states it.
    #[serde(rename = "single source")]
    SingleSource,
    /// An invented value, a policy.
    Proposal,
}

impl Tag {
    /// The text form, as in the data.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SourceStates => "source states",
            Self::Measured => "measured",
            Self::Derived => "derived",
            Self::Inferred => "inferred",
            Self::Snippet => "snippet",
            Self::SingleSource => "single source",
            Self::Proposal => "proposal",
        }
    }
}

/// The checkable note rule of an entry. The kind list is fixed; free text
/// is rejected.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Rule {
    /// Every pitch class lies in `offsets` above the root.
    PitchSet {
        /// Semitones above the root.
        offsets: Vec<u8>,
        /// Offsets a line shows at least once per 4 bars to claim the
        /// scale.
        colour: Vec<u8>,
        /// Offsets that name a neighbouring scale instead.
        giveaway: Vec<u8>,
    },
    /// The first or last note of a phrase, or the held bass note, is the
    /// root.
    Tonic {},
    /// The chord holds these tones above its root.
    ChordTones {
        /// Semitones above the chord root; 14 is a ninth.
        offsets: Vec<u8>,
        /// Tones of which at least some must also sound.
        choose: Option<Choose>,
    },
    /// The spacing of a chord, or the motion between chords.
    Voicing(Box<VoicingRule>),
    /// A chord loop: roots above the tonic and a chord per root.
    Cycle {
        /// Semitones above the tonic, one per chord.
        roots: Vec<u8>,
        /// Chord entry ids, one per root.
        chords: Vec<String>,
        /// Bars per chord, when the entry fixes it.
        bars_per_chord: Option<f64>,
    },
    /// Onsets fall on these slots, per part.
    OnsetSet {
        /// The slots of each part.
        parts: Vec<PartSlots>,
    },
    /// At least `min_share` of the onsets fall on `slots`.
    OnsetShare {
        /// The slots that count.
        slots: Vec<u8>,
        /// The smallest share, 0 to 1.
        min_share: f64,
    },
    /// The number of notes per bar.
    CountPerBar {
        /// The fewest notes.
        min: u8,
        /// The most notes.
        max: u8,
    },
    /// The sounding length, `len` times `gate`.
    Length {
        /// Beats, or a share of the grid step.
        unit: LengthUnit,
        /// The shortest length.
        min: Option<f64>,
        /// The longest length.
        max: Option<f64>,
    },
    /// The velocity band.
    Velocity {
        /// The lowest velocity.
        min: u8,
        /// The highest velocity.
        max: u8,
    },
    /// The next note starts before this one ends.
    Overlap {
        /// The smallest overlap in ms.
        min_ms: f64,
    },
    /// The second note of each pair starts late.
    Swing {
        /// 16 for a pair of sixteenths, 8 for a pair of eighths.
        note: u8,
        /// The lowest pair percent.
        min_pct: f64,
        /// The highest pair percent.
        max_pct: f64,
    },
    /// Every onset moves by the same amount, late positive.
    Shift {
        /// The smallest shift in ms.
        min_ms: f64,
        /// The largest shift in ms.
        max_ms: f64,
    },
    /// Onsets and velocities vary at random.
    Jitter {
        /// The largest onset deviation in ms.
        max_ms: f64,
        /// The largest velocity deviation.
        velocity: u8,
    },
    /// The pitch range and default voice of a role.
    Register {
        /// Where most notes sit, inclusive.
        core: [u8; 2],
        /// The limits outside which a note is rejected, inclusive.
        hard: [u8; 2],
        /// Mono, poly or either.
        voice: Voice,
    },
    /// A Euclidean pattern: `k` onsets spread over `n` steps.
    Pattern {
        /// Onsets.
        k: u8,
        /// Steps.
        n: u8,
        /// Steps the pattern is turned by.
        rotation: u8,
    },
    /// Chord notes one per step.
    Arp {
        /// The play order.
        order: ArpOrder,
        /// Beats per step.
        rate: f64,
        /// The gate range in percent of the step.
        gate: Option<[u8; 2]>,
        /// `sequence`: chord degrees in play order.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        degrees: Vec<u8>,
    },
    /// Bars identical to an earlier bar.
    Repeat {
        /// The smallest share of identical bars.
        min_share: Option<f64>,
        /// The largest share of identical bars.
        max_share: Option<f64>,
        /// The fewest bars the repetition lasts.
        min_bars: Option<u8>,
    },
    /// Change events fall on bars `1 + k * every`.
    PhraseLength {
        /// The bar counts between changes.
        every: Vec<u8>,
    },
    /// The word names no note rule.
    None {},
}

impl Rule {
    /// The kind name, as in the data.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::PitchSet { .. } => "pitch_set",
            Self::Tonic {} => "tonic",
            Self::ChordTones { .. } => "chord_tones",
            Self::Voicing(_) => "voicing",
            Self::Cycle { .. } => "cycle",
            Self::OnsetSet { .. } => "onset_set",
            Self::OnsetShare { .. } => "onset_share",
            Self::CountPerBar { .. } => "count_per_bar",
            Self::Length { .. } => "length",
            Self::Velocity { .. } => "velocity",
            Self::Overlap { .. } => "overlap",
            Self::Swing { .. } => "swing",
            Self::Shift { .. } => "shift",
            Self::Jitter { .. } => "jitter",
            Self::Register { .. } => "register",
            Self::Pattern { .. } => "pattern",
            Self::Arp { .. } => "arp",
            Self::Repeat { .. } => "repeat",
            Self::PhraseLength { .. } => "phrase_length",
            Self::None {} => "none",
        }
    }
}

/// Tones of which at least `at_least` must sound.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Choose {
    /// The candidate tones, in semitones above the root.
    pub from: Vec<u8>,
    /// How many of them must sound.
    pub at_least: u8,
}

/// The parameters of a voicing rule. `shape` names the test; the numbers
/// that shape uses are set and the rest are absent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoicingRule {
    /// The test the evaluator runs.
    pub shape: VoicingShape,
    /// The widest span from the lowest to the highest tone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_span: Option<u8>,
    /// The narrowest span.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_span: Option<u8>,
    /// The widest gap between adjacent tones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_gap: Option<u8>,
    /// The narrowest gap between adjacent tones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_gap: Option<u8>,
    /// The interval between adjacent tones.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<u8>,
    /// How far a step may differ from `step`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_tolerance: Option<u8>,
    /// The fewest notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_notes: Option<u8>,
    /// The most notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_notes: Option<u8>,
    /// The fewest distinct pitch classes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_pitch_classes: Option<u8>,
    /// No tone below this pitch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lowest: Option<u8>,
    /// With `max_below`: the pitch under which tones are counted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub below: Option<u8>,
    /// The most tones under `below`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_below: Option<u8>,
    /// Voices counted from the top that move down an octave.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drop: Vec<u8>,
    /// The chord tone in the bass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bass: Option<BassTone>,
    /// Chord degrees from the bottom up.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub degrees: Vec<u8>,
    /// The largest leap of an upper voice between chords.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_leap: Option<u8>,
    /// The lowest pitch of the lower note of each interval.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub floors: Vec<Floor>,
}

/// The test a voicing rule runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
#[allow(
    missing_docs,
    reason = "each shape is the voicing word of the same name in the reference"
)]
pub enum VoicingShape {
    Close,
    Open,
    Inversion,
    Slash,
    #[serde(rename = "drop-2")]
    Drop2,
    #[serde(rename = "drop-3")]
    Drop3,
    #[serde(rename = "drop-2-and-4")]
    Drop2And4,
    Shell,
    RootlessA,
    RootlessB,
    Power,
    Quartal,
    Quintal,
    NeoSoul,
    Lush,
    Wide,
    Muddy,
    Airy,
    Thick,
    IntervalFloor,
    CommonTone,
    NearestTone,
    SmallestMotion,
    NoParallels,
    TopHold,
    Planing,
    LoopReturn,
    Cluster,
    Polychord,
}

/// The chord tone in the bass.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BassTone {
    /// The root.
    Root,
    /// The third.
    Third,
    /// The fifth.
    Fifth,
    /// The seventh.
    Seventh,
    /// The note after the slash.
    Named,
}

/// The lowest allowed pitch for the lower note of an interval.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Floor {
    /// The interval in semitones.
    pub interval: u8,
    /// The floor; none for an interval with no floor.
    pub lowest: Option<u8>,
}

/// The slots of one part.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartSlots {
    /// The part, such as `kick`, or `any`.
    pub part: String,
    /// Slots 0 to 15.
    pub slots: Vec<u8>,
}

/// The unit of a length rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LengthUnit {
    /// Beats; a quarter note is 1.
    Beat,
    /// A share of the grid step.
    Step,
}

/// The default voice of a role.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Voice {
    /// One note at a time.
    Mono,
    /// Chords.
    Poly,
    /// Either.
    Either,
}

/// The play order of an arpeggio.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArpOrder {
    /// Low to high.
    Up,
    /// High to low.
    Down,
    /// Low to high and back.
    UpDown,
    /// The degrees in `degrees`.
    Sequence,
}

/// A tiny note list that meets or breaks a rule.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fragment {
    /// The tonic or chord root the rule reads, as a pitch class.
    pub root: Option<u8>,
    /// The tempo, for rules in ms.
    pub bpm: Option<f64>,
    /// Slots per bar: 12, 16 or 24.
    pub grid: u8,
    /// The notes, sorted by start.
    pub notes: Vec<FragmentNote>,
}

/// One note of a fragment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FragmentNote {
    /// MIDI pitch, 21 to 108.
    pub pitch: u8,
    /// The onset slot, counted from bar 1 slot 0.
    pub start: u16,
    /// The written length in beats.
    pub len: f64,
    /// Velocity, 1 to 127.
    pub vel: u8,
    /// Percent of `len` that sounds.
    #[serde(default = "full_gate")]
    pub gate: u8,
    /// Shift from the slot in ms, late positive.
    #[serde(default)]
    pub offset_ms: f64,
    /// The drum or instrument part.
    #[serde(default)]
    pub part: Option<String>,
}

fn full_gate() -> u8 {
    100
}

/// A preset control an entry needs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    /// The synth whose registry holds the label.
    pub synth: Synth,
    /// The registry label.
    pub label: String,
    /// Which way to move it.
    pub direction: String,
    /// The value or step to write, with its unit.
    pub step: String,
    /// The order among the entry's controls.
    pub order: u8,
    /// Source ids.
    pub source: Vec<String>,
    /// `from facts`, `measured` or `uncalibrated`.
    pub tag: String,
}

/// A tempo range.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bpm {
    /// The slowest tempo.
    pub low: f64,
    /// The usual tempo, when the sources give one.
    pub center: Option<f64>,
    /// The fastest tempo.
    pub high: f64,
}

/// The one question of an ask word.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ask {
    /// The question, as the agent asks it.
    pub question: String,
    /// Entry ids of the senses to choose from.
    pub options: Vec<String>,
    /// The option a batch run takes without asking.
    pub default: String,
    /// When the question is asked.
    pub when: String,
}

/// The scale a mood word picks when the producer names none. A policy, not
/// a fact.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DefaultPick {
    /// The scale entry id, or none when the word maps to no scale.
    pub scale: Option<String>,
    /// Other scale entry ids the sources attach to the word.
    pub alternatives: Vec<String>,
    /// How the pick combines with other words.
    pub note: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_read_their_kind_and_reject_unknown_fields() {
        let voicing: Rule =
            serde_json::from_str(r#"{"kind": "voicing", "shape": "drop-2", "drop": [2]}"#).unwrap();
        assert_eq!(voicing.kind(), "voicing");
        let Rule::Voicing(rule) = &voicing else {
            unreachable!()
        };
        assert_eq!(
            (rule.shape, rule.drop.as_slice()),
            (VoicingShape::Drop2, &[2][..])
        );
        let set: Rule = serde_json::from_str(
            r#"{"kind": "pitch_set", "offsets": [0, 2, 3], "colour": [], "giveaway": []}"#,
        )
        .unwrap();
        assert_eq!(set.kind(), "pitch_set");
        assert!(serde_json::from_str::<Rule>(r#"{"kind": "none", "text": "x"}"#).is_err());
        assert!(
            serde_json::from_str::<Rule>(r#"{"kind": "voicing", "shape": "drop-2", "x": 1}"#)
                .is_err()
        );
        assert!(serde_json::from_str::<Rule>(r#"{"kind": "free text"}"#).is_err());
        assert!(serde_json::from_str::<Rule>(r#"{"kind": "voicing", "shape": "pretty"}"#).is_err());
    }
}
