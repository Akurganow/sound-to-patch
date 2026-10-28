//! The rules every entry of the music dictionary meets, the coverage of the
//! tiers and the recomputed shares. [`dictionary`](super::dictionary)
//! refuses data that breaks one.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::Deserialize;

use crate::music::Dictionary;
use crate::music::chord::ALTERED_DOMINANT;
use crate::music::entry::{Entry, Fragment, Group, Rule, Tier};
use crate::notes::{HIGHEST_MODEL_PITCH, LOWEST_MODEL_PITCH};
use crate::preset::registry::registry_for;

/// The roles an entry may name. `drum` and `fx` have no range row.
pub const ROLES: [&str; 13] = [
    "sub-bass", "808", "bass", "lead", "pad", "keys", "pluck", "arp", "vocal", "chords", "stab",
    "drum", "fx",
];

/// The parts an onset rule or a fragment note may name.
pub const PARTS: [&str; 11] = [
    "any", "kick", "snare", "clap", "rim", "hat", "open-hat", "ride", "bass", "chord", "lead",
];

/// The largest gap between a recomputed share and the stated one, in
/// units of the last stated decimal.
const ROUNDING: f64 = 0.5;

/// The largest sum of the shares of one exclusive set, in percent.
const SET_SUM: f64 = 100.5;

const SOURCES: &str = include_str!("../../dictionary/sources.json");

#[derive(Deserialize)]
struct Source {
    id: String,
}

/// Every broken rule, as text that names the entry. Empty when the data is
/// sound.
#[must_use]
pub fn problems(dictionary: &Dictionary) -> Vec<String> {
    let mut found = Vec::new();
    let sources: BTreeSet<String> = match serde_json::from_str::<Vec<Source>>(SOURCES) {
        Ok(list) => list.into_iter().map(|source| source.id).collect(),
        Err(error) => {
            found.push(format!("dictionary/sources.json: {error}"));
            BTreeSet::new()
        }
    };
    let mut ids = BTreeSet::new();
    for entry in &dictionary.entries {
        if !ids.insert(entry.id.as_str()) {
            found.push(format!("{}: the id appears twice", entry.id));
        }
    }
    for topic in super::TOPICS {
        for entry in dictionary.topic(topic.name) {
            if !topic.groups.contains(&entry.group) {
                found.push(format!(
                    "{}: group {} does not belong in {}.json",
                    entry.id,
                    entry.group.as_str(),
                    topic.name
                ));
            }
        }
    }
    for entry in &dictionary.entries {
        for problem in entry_problems(entry, dictionary, &sources) {
            found.push(format!("{}: {problem}", entry.id));
        }
    }
    let mut symbols: HashMap<&str, &str> = HashMap::new();
    for entry in dictionary
        .entries
        .iter()
        .filter(|entry| entry.group == Group::Chord)
    {
        for symbol in &entry.symbols {
            if let Some(other) = symbols.insert(symbol, &entry.id) {
                found.push(format!(
                    "{}: symbol `{symbol}` is also in {other}",
                    entry.id
                ));
            }
        }
    }
    found.extend(sense_problems(dictionary));
    found.extend(stat_problems(dictionary, &sources));
    found
}

fn entry_problems(
    entry: &Entry,
    dictionary: &Dictionary,
    sources: &BTreeSet<String>,
) -> Vec<String> {
    let mut found = Vec::new();
    let prefix = format!("{}.", entry.group.as_str());
    if !entry.id.starts_with(&prefix) {
        found.push(format!("the id must start with `{prefix}`"));
    }
    match (&entry.sense, entry.id.rsplit('.').next()) {
        (Some(sense), Some(last)) if sense != last => {
            found.push(format!("the id must end with the sense `{sense}`"));
        }
        _ => {}
    }
    for role in &entry.roles {
        if !ROLES.contains(&role.as_str()) {
            found.push(format!("unknown role `{role}`"));
        }
    }
    for id in &entry.conflicts {
        if dictionary.entry(id).is_none() {
            found.push(format!("conflict `{id}` is no entry"));
        }
    }
    for id in &entry.genres {
        if dictionary
            .entry(id)
            .is_none_or(|genre| genre.group != Group::Genre)
        {
            found.push(format!("genre `{id}` is no genre entry"));
        }
    }
    for source in &entry.source {
        if !sources.contains(source) {
            found.push(format!(
                "source `{source}` is not in dictionary/sources.json"
            ));
        }
    }
    if !is_date(&entry.verified) {
        found.push(format!(
            "verified `{}` is not a YYYY-MM-DD date",
            entry.verified
        ));
    }
    let names_tones = matches!(entry.rule, Rule::ChordTones { .. });
    if entry.group == Group::Chord && names_tones && entry.symbols.is_empty() {
        found.push("a chord entry with chord_tones lists its symbols".into());
    }
    if entry.group != Group::Chord && !entry.symbols.is_empty() {
        found.push("only chord entries list symbols".into());
    }
    if let Some(scale) = &entry.scale {
        if entry.group != Group::Progression {
            found.push("only progression entries name a scale".into());
        }
        match (
            dictionary.entry(scale).map(|picked| &picked.rule),
            &entry.rule,
        ) {
            (Some(Rule::PitchSet { offsets, .. }), Rule::Cycle { roots, .. }) => {
                if roots.iter().any(|root| !offsets.contains(root)) {
                    found.push(format!("a cycle root lies outside its scale `{scale}`"));
                }
            }
            _ if !is_scale(dictionary, scale) => {
                found.push(format!("scale `{scale}` is no scale entry"));
            }
            _ => {}
        }
    }
    if let Some(pick) = &entry.default_pick {
        for id in pick.scale.iter().chain(&pick.alternatives) {
            if !is_scale(dictionary, id) {
                found.push(format!("default pick `{id}` is no scale entry"));
            }
        }
        let picked = pick.scale.as_deref().and_then(|id| dictionary.entry(id));
        if let (Group::Scale, Some(picked)) = (entry.group, picked)
            && offsets(&entry.rule) != offsets(&picked.rule)
        {
            found.push(format!(
                "a scale-name sense needs the offsets of its pick `{}`",
                picked.id
            ));
        }
    }
    if let Rule::ChordTones {
        choose: Some(_), ..
    } = &entry.rule
        && entry.id != ALTERED_DOMINANT
    {
        found.push(format!("only `{ALTERED_DOMINANT}` may choose its tones"));
    }
    found.extend(control_problems(entry, sources));
    found.extend(rule_problems(&entry.rule, dictionary));
    found.extend(example_problems(entry));
    found
}

fn example_problems(entry: &Entry) -> Vec<String> {
    let mut found = Vec::new();
    let examples = [&entry.example_pass, &entry.example_fail];
    if matches!(entry.rule, Rule::None {}) {
        if examples.iter().any(|example| example.is_some()) {
            found.push("an entry with rule `none` has no examples".into());
        }
    } else {
        for (name, example) in ["example_pass", "example_fail"].iter().zip(examples) {
            match example {
                None => found.push(format!("{name} is missing")),
                Some(fragment) => {
                    for problem in fragment_problems(fragment) {
                        found.push(format!("{name}: {problem}"));
                    }
                }
            }
        }
        if entry.example_pass.is_some() && entry.example_pass == entry.example_fail {
            found.push("example_pass and example_fail are the same".into());
        }
    }
    found
}

fn offsets(rule: &Rule) -> Option<&[u8]> {
    match rule {
        Rule::PitchSet { offsets, .. } => Some(offsets),
        _ => None,
    }
}

fn is_scale(dictionary: &Dictionary, id: &str) -> bool {
    dictionary
        .entry(id)
        .is_some_and(|entry| entry.group == Group::Scale)
}

fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(index, byte)| {
            if index == 4 || index == 7 {
                *byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
}

fn control_problems(entry: &Entry, sources: &BTreeSet<String>) -> Vec<String> {
    let mut found = Vec::new();
    for control in &entry.controls {
        match registry_for(control.synth) {
            Ok(registry) => {
                if !registry
                    .entries()
                    .iter()
                    .any(|spec| spec.label == control.label)
                {
                    found.push(format!(
                        "control `{}` is no label of the {} registry",
                        control.label, control.synth
                    ));
                }
            }
            Err(_) => found.push(format!(
                "control `{}` names {}, which has no registry yet",
                control.label, control.synth
            )),
        }
        if !["up", "down", "on", "off", "set"].contains(&control.direction.as_str()) {
            found.push(format!("control direction `{}`", control.direction));
        }
        if !["from facts", "measured", "uncalibrated"].contains(&control.tag.as_str()) {
            found.push(format!("control tag `{}`", control.tag));
        }
        for source in &control.source {
            if !sources.contains(source) {
                found.push(format!(
                    "control source `{source}` is not in dictionary/sources.json"
                ));
            }
        }
    }
    found
}

fn rising(offsets: &[u8]) -> bool {
    offsets.first() == Some(&0) && offsets.windows(2).all(|pair| pair[0] < pair[1])
}

fn rule_problems(rule: &Rule, dictionary: &Dictionary) -> Vec<String> {
    let mut found = Vec::new();
    match rule {
        Rule::PitchSet {
            offsets,
            colour,
            giveaway,
        } => {
            if !rising(offsets) || offsets.iter().any(|&offset| offset > 11) {
                found
                    .push("pitch_set offsets must start at 0, rise and stay within 0 to 11".into());
            }
            if colour.iter().any(|offset| !offsets.contains(offset)) {
                found.push("a colour offset lies outside the scale".into());
            }
            if giveaway.iter().any(|offset| offsets.contains(offset)) {
                found.push("a giveaway offset lies inside the scale".into());
            }
        }
        Rule::ChordTones { offsets, choose } => {
            if !rising(offsets) {
                found.push("chord_tones offsets must start at 0 and rise".into());
            }
            if let Some(choose) = choose
                && usize::from(choose.at_least) > choose.from.len()
            {
                found.push("choose asks for more tones than it offers".into());
            }
        }
        Rule::Cycle { roots, chords, .. } => {
            if roots.len() != chords.len() {
                found.push("a cycle needs one chord per root".into());
            }
            for chord in chords {
                if dictionary
                    .entry(chord)
                    .is_none_or(|entry| entry.group != Group::Chord)
                {
                    found.push(format!("cycle chord `{chord}` is no chord entry"));
                }
            }
        }
        Rule::OnsetSet { parts } => {
            for part in parts {
                if !PARTS.contains(&part.part.as_str()) {
                    found.push(format!("unknown part `{}`", part.part));
                }
                if part.slots.iter().any(|&slot| slot > 15) {
                    found.push("onset slots run from 0 to 15".into());
                }
            }
        }
        Rule::CountPerBar { min, max } | Rule::Velocity { min, max } if min > max => {
            found.push("min exceeds max".into());
        }
        Rule::Length {
            min: Some(min),
            max: Some(max),
            ..
        } if min > max => found.push("min exceeds max".into()),
        Rule::Swing {
            min_pct, max_pct, ..
        } if min_pct > max_pct => found.push("min_pct exceeds max_pct".into()),
        Rule::Shift { min_ms, max_ms } if min_ms > max_ms => {
            found.push("min_ms exceeds max_ms".into());
        }
        Rule::Register { core, hard, .. } => {
            let inside = |range: [u8; 2]| {
                range[0] <= range[1]
                    && range[0] >= LOWEST_MODEL_PITCH
                    && range[1] <= HIGHEST_MODEL_PITCH
            };
            if !inside(*core) || !inside(*hard) || core[0] < hard[0] || core[1] > hard[1] {
                found.push(
                    "the core range must lie inside the hard limits, and both inside 21 to 108"
                        .into(),
                );
            }
        }
        Rule::Pattern { k, n, rotation } if k > n || rotation >= n => {
            found.push("a pattern needs k <= n and rotation < n".into());
        }
        _ => {}
    }
    found
}

fn fragment_problems(fragment: &Fragment) -> Vec<String> {
    let mut found = Vec::new();
    if ![12, 16, 24].contains(&fragment.grid) {
        found.push(format!("grid {} is not 12, 16 or 24", fragment.grid));
    }
    if fragment.root.is_some_and(|root| root > 11) {
        found.push("root is a pitch class, 0 to 11".into());
    }
    if fragment.notes.is_empty() {
        found.push("a fragment holds at least one note".into());
    }
    for note in &fragment.notes {
        if !(LOWEST_MODEL_PITCH..=HIGHEST_MODEL_PITCH).contains(&note.pitch) {
            found.push(format!("pitch {} lies outside 21 to 108", note.pitch));
        }
        if !(1..=127).contains(&note.vel) {
            found.push(format!("velocity {} lies outside 1 to 127", note.vel));
        }
        if !(1..=100).contains(&note.gate) {
            found.push(format!("gate {} lies outside 1 to 100", note.gate));
        }
        if !(note.len > 0.0 && note.len <= 64.0) {
            found.push(format!("length {} beats lies outside 0 to 64", note.len));
        }
        if note
            .part
            .as_ref()
            .is_some_and(|part| !PARTS.contains(&part.as_str()))
        {
            found.push("unknown part".into());
        }
    }
    if fragment
        .notes
        .windows(2)
        .any(|pair| pair[0].start > pair[1].start)
    {
        found.push("notes must be sorted by start".into());
    }
    found
}

/// The senses of each word: one entry per sense, distinct sense ids and
/// exactly one default; an ask question offers senses of its own word.
fn sense_problems(dictionary: &Dictionary) -> Vec<String> {
    let mut found = Vec::new();
    let mut words: BTreeMap<&str, Vec<&Entry>> = BTreeMap::new();
    for entry in &dictionary.entries {
        words.entry(entry.word.as_str()).or_default().push(entry);
    }
    for (word, senses) in &words {
        let defaults = senses.iter().filter(|entry| entry.default).count();
        if defaults != 1 {
            found.push(format!(
                "word `{word}` has {defaults} default senses, not 1"
            ));
        }
        if senses.len() > 1 {
            let mut seen = BTreeSet::new();
            for entry in senses {
                match &entry.sense {
                    None => found.push(format!(
                        "{}: word `{word}` has several senses, so each needs a sense id",
                        entry.id
                    )),
                    Some(sense) if !seen.insert(sense.as_str()) => {
                        found.push(format!("word `{word}` has sense `{sense}` twice"));
                    }
                    Some(_) => {}
                }
            }
        }
    }
    for entry in &dictionary.entries {
        let Some(ask) = &entry.ask else { continue };
        if !ask.options.contains(&ask.default) {
            found.push(format!("{}: the ask default is not an option", entry.id));
        }
        for option in &ask.options {
            if dictionary
                .entry(option)
                .is_none_or(|sense| sense.word != entry.word)
            {
                found.push(format!(
                    "{}: ask option `{option}` is no sense of `{}`",
                    entry.id, entry.word
                ));
            }
        }
    }
    found
}

fn stat_problems(dictionary: &Dictionary, sources: &BTreeSet<String>) -> Vec<String> {
    let mut found = Vec::new();
    let mut ids = BTreeSet::new();
    let mut sets: BTreeMap<&str, f64> = BTreeMap::new();
    for stat in &dictionary.stats {
        if !ids.insert(stat.id.as_str()) {
            found.push(format!("stat {}: the id appears twice", stat.id));
        }
        if !(0.0..=100.0).contains(&stat.share) {
            found.push(format!(
                "stat {}: share {} lies outside 0 to 100",
                stat.id, stat.share
            ));
        }
        match (stat.count, stat.total) {
            (Some(count), Some(total)) => {
                if let Some(problem) = recompute(stat.share, count, total) {
                    found.push(format!("stat {}: {problem}", stat.id));
                }
            }
            (None, None) => {}
            _ => found.push(format!(
                "stat {}: give both count and total, or neither",
                stat.id
            )),
        }
        if let Some(set) = &stat.set {
            *sets.entry(set.as_str()).or_default() += stat.share;
        }
        for source in &stat.source {
            if !sources.contains(source) {
                found.push(format!(
                    "stat {}: source `{source}` is not in dictionary/sources.json",
                    stat.id
                ));
            }
        }
    }
    for (set, sum) in sets {
        if sum > SET_SUM {
            found.push(format!(
                "stat set {set}: the shares sum to {sum:.1} %, over {SET_SUM} %"
            ));
        }
    }
    found
}

/// Recomputes a share from its counts; `None` when it matches the stated
/// share at the stated decimals.
#[must_use]
pub fn recompute(share: f64, count: u64, total: u64) -> Option<String> {
    if count > total {
        return Some(format!("count {count} exceeds total {total}"));
    }
    #[allow(clippy::cast_precision_loss, reason = "counts stay far below 2^52")]
    let computed = count as f64 / total as f64 * 100.0;
    let step = decimal_step(share);
    if (computed - share).abs() <= step * ROUNDING + f64::EPSILON * 100.0 {
        None
    } else {
        Some(format!(
            "{count} of {total} is {computed:.3} %, not the stated {share} %"
        ))
    }
}

/// The value of one unit of the last decimal of `share`, as written in the
/// shortest form that round-trips, and at least one decimal: JSON keeps
/// no trailing zero, so `97.0` reads as `97`.
fn decimal_step(share: f64) -> f64 {
    let text = share.to_string();
    let decimals = text
        .split_once('.')
        .map_or(1, |(_, fraction)| fraction.len().max(1));
    let decimals = i32::try_from(decimals).unwrap_or(i32::MAX);
    10f64.powi(-decimals)
}

/// One row of the coverage report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoverageRow {
    /// The group.
    pub group: Group,
    /// The tier.
    pub tier: Tier,
    /// The planned entries.
    pub planned: u16,
    /// The entries that landed, counting one per word.
    pub landed: u16,
}

impl CoverageRow {
    /// Whether the landed share reaches the floor of the tier.
    #[must_use]
    pub fn meets_floor(self) -> bool {
        u32::from(self.landed) * 100 >= u32::from(self.planned) * u32::from(self.tier.floor())
    }
}

/// The coverage of every planned group and tier. An entry counts once per
/// word, through its default sense.
#[must_use]
pub fn coverage(dictionary: &Dictionary) -> Vec<CoverageRow> {
    let mut landed: HashMap<(Group, Tier), u16> = HashMap::new();
    for entry in dictionary.entries.iter().filter(|entry| entry.default) {
        *landed.entry((entry.group, entry.tier)).or_default() += 1;
    }
    let mut rows = Vec::new();
    for (group, tiers) in &dictionary.plan {
        for (tier, planned) in tiers {
            rows.push(CoverageRow {
                group: *group,
                tier: *tier,
                planned: *planned,
                landed: landed.get(&(*group, *tier)).copied().unwrap_or(0),
            });
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recomputes_shares_at_the_stated_decimals() {
        assert_eq!(recompute(34.2, 57_543, 168_242), None);
        assert_eq!(recompute(53.1, 402, 757), None);
        assert_eq!(recompute(96.7, 29, 30), None);
        assert!(recompute(34.3, 57_543, 168_242).is_some());
        assert!(recompute(97.0, 29, 30).is_some());
        assert!(recompute(50.0, 3, 2).is_some());
    }

    #[test]
    fn only_the_altered_dominant_chooses_its_tones() {
        let dictionary = crate::music::dictionary().unwrap();
        let Some(Rule::ChordTones { choose, .. }) =
            dictionary.entry(ALTERED_DOMINANT).map(|entry| &entry.rule)
        else {
            panic!("{ALTERED_DOMINANT} has no chord tones");
        };
        let mut entry = dictionary.entry("chord.major-triad").unwrap().clone();
        if let Rule::ChordTones { choose: other, .. } = &mut entry.rule {
            other.clone_from(choose);
        }
        let problems = entry_problems(&entry, dictionary, &BTreeSet::new());
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("may choose its tones")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_tier_floor_counts_whole_percent() {
        let row = |tier, planned, landed| CoverageRow {
            group: Group::Scale,
            tier,
            planned,
            landed,
        };
        assert!(row(Tier::M1, 8, 8).meets_floor());
        assert!(!row(Tier::M1, 8, 7).meets_floor());
        assert!(row(Tier::M2, 22, 11).meets_floor());
        assert!(!row(Tier::M2, 22, 10).meets_floor());
        assert!(row(Tier::M3, 27, 7).meets_floor());
        assert!(!row(Tier::M3, 27, 6).meets_floor());
    }
}
