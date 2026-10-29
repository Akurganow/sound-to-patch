//! The rules every word entry, listener number and list row must meet.
//!
//! [`problems`] recomputes each agreement class and few-sample flag from
//! the stored numbers, compares every entry with the listener numbers of
//! its word, and checks that each number names sources whose count its tag
//! states.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use crate::words::{Axis, Class, Entry, KNOWN_FROM, Tag, Value, Words};

const SOURCES: &str = include_str!("../../dictionary/sources.json");

/// Listener numbers below this count would come close to one row, so the
/// script writes none.
pub const AGGREGATE_FROM: u32 = 5;

/// The neutral control groups a technique word may name.
pub const CONTROL_GROUPS: [&str; 16] = [
    "oscillator waveform",
    "distortion drive",
    "filter cutoff",
    "noise level",
    "reverb",
    "lfo modulation",
    "delay",
    "frequency modulation",
    "unison detune",
    "envelope",
    "formant filter",
    "phaser and flanger",
    "chorus",
    "sub oscillator",
    "granular",
    "filter resonance",
];

/// The cards a style word may hint at, besides `pending` and `no hint`.
const CARDS: [&str; 16] = [
    "bass",
    "lead",
    "pad",
    "keys",
    "drums",
    "fx",
    "arp",
    "sequence",
    "pluck",
    "chord and stab",
    "atmosphere",
    "bell and mallet",
    "texture and drone",
    "hit",
    "chip",
    "experimental",
];

#[derive(Deserialize)]
struct Source {
    id: String,
}

/// Every broken rule, as one line each; empty when the data is sound.
#[must_use]
pub fn problems(words: &Words) -> Vec<String> {
    let mut found = Vec::new();
    let sources: BTreeSet<String> = match serde_json::from_str::<Vec<Source>>(SOURCES) {
        Ok(list) => list.into_iter().map(|source| source.id).collect(),
        Err(error) => {
            found.push(format!("dictionary/sources.json: {error}"));
            BTreeSet::new()
        }
    };
    found.extend(listener_problems(words));
    let mut ids = BTreeSet::new();
    for entry in &words.entries {
        if !ids.insert(entry.id.as_str()) {
            found.push(format!("{}: the id is used twice", entry.id));
        }
        found.extend(
            entry_problems(entry, words, &sources)
                .into_iter()
                .map(|problem| format!("{}: {problem}", entry.id)),
        );
    }
    found.extend(sense_problems(words));
    for anchor in &words.ladder.anchors {
        found.extend(
            value_problems(&anchor.value, &sources)
                .into_iter()
                .map(|problem| format!("ladder {} {}: {problem}", anchor.control, anchor.step)),
        );
    }
    found.extend(list_problems(words));
    found
}

fn listener_problems(words: &Words) -> Vec<String> {
    let mut found = Vec::new();
    for row in &words.listeners.eq {
        let word = &row.word;
        if row.class != Class::of(row.value, row.n) {
            found.push(format!(
                "agreement.json {word}: class {:?} does not follow from value {:?} and n {}",
                row.class, row.value, row.n
            ));
        }
        if row.few_sample != (row.n < KNOWN_FROM) {
            found.push(format!(
                "agreement.json {word}: the few-sample flag is wrong"
            ));
        }
        if row.value.is_some() != (row.n >= 2) {
            found.push(format!(
                "agreement.json {word}: a value needs two curves or more"
            ));
        }
        if row.n < AGGREGATE_FROM && !words.entries.iter().any(|entry| entry.word == *word) {
            found.push(format!(
                "agreement.json {word}: a label of {} curves is kept only for an entry word",
                row.n
            ));
        }
        let aggregate = row.bands.is_some() || row.peak_hz.is_some();
        if aggregate && row.n < AGGREGATE_FROM {
            found.push(format!(
                "agreement.json {word}: band means from {} curves come too close to one curve",
                row.n
            ));
        }
    }
    for row in &words.listeners.reverb {
        if row.n < AGGREGATE_FROM {
            found.push(format!(
                "agreement.json reverb {}: {} settings come too close to one row",
                row.word, row.n
            ));
        }
    }
    found
}

/// The count of sources a tag allows, over the sources that are not search
/// excerpts, and whether the sources must all be search excerpts.
fn tag_problem(tag: Tag, sources: &[String]) -> Option<&'static str> {
    if sources.iter().collect::<BTreeSet<_>>().len() < sources.len() {
        return Some("a source is listed twice");
    }
    // The listener data is derived, so it states nothing and never counts
    // toward `source states` or `single source`.
    let pages = sources
        .iter()
        .filter(|id| !id.starts_with("snip-") && *id != "crowd-eq")
        .count();
    let ok = match tag {
        Tag::SourceStates => pages >= 2,
        Tag::SingleSource => pages == 1,
        Tag::Snippet => pages == 0 && sources.iter().any(|id| id.starts_with("snip-")),
        Tag::CrowdData => sources == ["crowd-eq"],
        Tag::Inferred => sources.is_empty(),
    };
    (!ok).then_some(match tag {
        Tag::SourceStates => "`source states` needs two sources or more",
        Tag::SingleSource => "`single source` needs exactly one source besides search excerpts",
        Tag::Snippet => "`snippet` cites search excerpts only",
        Tag::CrowdData => "`crowd data` cites the listener data alone",
        Tag::Inferred => "`inferred` cites no source",
    })
}

fn value_problems(value: &Value, sources: &BTreeSet<String>) -> Vec<String> {
    let mut found = Vec::new();
    if value.tag == Tag::CrowdData || value.tag == Tag::Inferred {
        found.push(format!("{}: a number needs a source tag", value.what));
    } else if let Some(problem) = tag_problem(value.tag, &value.sources) {
        found.push(format!("{}: {problem}", value.what));
    }
    if value.low > value.high {
        found.push(format!("{}: low lies above high", value.what));
    }
    if value.what.chars().any(|c| c.is_ascii_digit()) {
        found.push(format!("`{}` holds a digit", value.what));
    }
    for source in &value.sources {
        if !sources.contains(source) {
            found.push(format!(
                "source `{source}` is not in dictionary/sources.json"
            ));
        }
    }
    found
}

fn entry_problems(entry: &Entry, words: &Words, sources: &BTreeSet<String>) -> Vec<String> {
    let mut found = Vec::new();
    for name in std::iter::once(&entry.word).chain(&entry.aliases) {
        if *name != name.to_lowercase() {
            found.push(format!(
                "`{name}` is not lowercase, so a lookup never finds it"
            ));
        }
    }
    if !entry.id.ends_with(&format!(".{}", entry.sense)) {
        found.push(format!(
            "the id does not end with the sense `{}`",
            entry.sense
        ));
    }
    for (field, text) in [
        ("meaning", Some(&entry.meaning)),
        ("label", entry.label.as_ref()),
    ] {
        if text.is_some_and(|text| text.chars().any(|c| c.is_ascii_digit())) {
            found.push(format!(
                "the {field} holds a digit: numbers go into `values`"
            ));
        }
    }
    if let Some(problem) = tag_problem(entry.tag, &entry.sources) {
        found.push(problem.to_owned());
    }
    if entry.tag == Tag::Inferred && !entry.values.is_empty() {
        found.push("`inferred` holds no number".to_owned());
    }
    for source in &entry.sources {
        if !sources.contains(source) {
            found.push(format!(
                "source `{source}` is not in dictionary/sources.json"
            ));
        }
    }
    for value in &entry.values {
        found.extend(value_problems(value, sources));
    }
    found.extend(listener_match_problems(entry, words));
    if let Some(measure) = &entry.measure
        && (measure.key == "band_share") != measure.band.is_some()
    {
        found.push("a measure names a band exactly when it is `band_share`".to_owned());
    }
    if entry.control.is_some() != entry.card_hint.is_some() {
        found.push("`no control` and a card hint go together".to_owned());
    }
    if !entry.music.is_empty() {
        match crate::music::dictionary() {
            Ok(music) => {
                for id in &entry.music {
                    if music.entry(id).is_none() {
                        found.push(format!("music entry `{id}` does not exist"));
                    }
                }
            }
            Err(error) => found.push(error.to_string()),
        }
    }
    for conflict in &entry.conflicts {
        match words.entry(conflict) {
            None => found.push(format!("conflict `{conflict}` does not exist")),
            Some(other) if !other.conflicts.contains(&entry.id) => {
                found.push(format!(
                    "conflict `{conflict}` does not list this entry back"
                ));
            }
            Some(_) => {}
        }
    }
    found
}

/// Whether an entry carries the listener numbers of its word.
fn listener_match_problems(entry: &Entry, words: &Words) -> Vec<String> {
    let mut found = Vec::new();
    let word = entry.word.to_lowercase();
    let row = words.eq(&word);
    let (want, few) = row.map_or(((None, 0, Class::NoData), false), |row| {
        ((row.value, row.n, row.class), row.few_sample)
    });
    let agreement = &entry.agreement;
    if (agreement.value, agreement.n, agreement.class) != want || entry.few_sample != few {
        found.push(format!(
            "agreement {:?}, n {}, {:?}, few-sample {} differ from agreement.json {want:?}, few-sample {few}",
            agreement.value, agreement.n, agreement.class, entry.few_sample
        ));
    }
    for alias in &entry.aliases {
        if words
            .eq(alias)
            .is_some_and(|row| matches!(row.class, Class::High | Class::Medium | Class::Low))
        {
            found.push(format!(
                "alias `{alias}` has its own agreement class: give it its own entry"
            ));
        }
    }
    match (&entry.reverb, words.reverb(&word)) {
        (Some(stored), Some(row)) => {
            if (stored.n, stored.spread_ratio, stored.effects)
                != (row.n, row.spread_ratio, row.effects)
            {
                found.push("the reverb numbers differ from agreement.json".to_owned());
            }
        }
        (Some(_), None) => found.push("agreement.json has no reverb row for the word".to_owned()),
        (None, Some(_)) if entry.axis == Axis::Space => {
            found.push("a space word with listener reverb numbers must carry them".to_owned());
        }
        (None, _) => {}
    }
    found
}

/// One default per word; a word with several senses offers two or three
/// labelled senses, the default among them.
fn sense_problems(words: &Words) -> Vec<String> {
    let mut by_word: BTreeMap<&str, Vec<&Entry>> = BTreeMap::new();
    for entry in &words.entries {
        by_word.entry(&entry.word).or_default().push(entry);
    }
    let mut found = Vec::new();
    let mut lead_of: BTreeMap<&str, &str> = BTreeMap::new();
    for entry in &words.entries {
        for name in std::iter::once(&entry.word).chain(&entry.aliases) {
            match lead_of.insert(name, &entry.word) {
                Some(other) if other != entry.word => found.push(format!(
                    "`{name}` names both `{other}` and `{}`: keep it on one word",
                    entry.word
                )),
                _ => {}
            }
        }
    }
    for (word, senses) in by_word {
        let defaults = senses.iter().filter(|entry| entry.default).count();
        if defaults != 1 {
            found.push(format!("{word}: {defaults} default senses, want one"));
        }
        let labelled: Vec<&&Entry> = senses
            .iter()
            .filter(|entry| entry.label.is_some())
            .collect();
        if senses.len() == 1 {
            if !labelled.is_empty() {
                found.push(format!("{word}: a single sense needs no label"));
            }
        } else if !(2..=3).contains(&labelled.len()) || !labelled.iter().any(|entry| entry.default)
        {
            found.push(format!(
                "{word}: the question needs two or three labelled senses, the default among them"
            ));
        }
    }
    found
}

fn list_problems(words: &Words) -> Vec<String> {
    let mut found = Vec::new();
    for (name, list) in [("style", &words.style), ("technique", &words.technique)] {
        for (index, row) in list.iter().enumerate() {
            if usize::try_from(row.rank).ok() != Some(index + 1) {
                found.push(format!(
                    "{name}.json {}: rank {} out of order",
                    row.word, row.rank
                ));
            }
        }
    }
    for row in &words.style {
        let hint = row.hint.as_deref().unwrap_or("");
        if row.group.is_some() || !(hint == "pending" || hint == "no hint" || CARDS.contains(&hint))
        {
            found.push(format!(
                "style.json {}: the hint is `pending`, a card or `no hint`, with no group",
                row.word
            ));
        }
    }
    let mut groups = BTreeSet::new();
    for (index, row) in words.technique.iter().enumerate() {
        let mapped = index < CONTROL_GROUPS.len();
        match &row.group {
            Some(group) if mapped && CONTROL_GROUPS.contains(&group.as_str()) => {
                if !groups.insert(group.as_str()) {
                    found.push(format!("technique.json: group `{group}` named twice"));
                }
            }
            None if !mapped => {}
            _ => found.push(format!(
                "technique.json {}: the first {} words name a control group, the rest none",
                row.word,
                CONTROL_GROUPS.len()
            )),
        }
        if row.hint.is_some() {
            found.push(format!(
                "technique.json {}: a technique word has no hint",
                row.word
            ));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    #[test]
    fn a_wrong_class_or_flag_in_the_listener_numbers_fails() {
        let mut words = crate::words::parse().unwrap();
        assert_eq!(listener_problems(&words), Vec::<String>::new());
        let dark = words
            .listeners
            .eq
            .iter_mut()
            .find(|row| row.word == "dark")
            .unwrap();
        dark.class = Class::Medium;
        dark.few_sample = true;
        let found = listener_problems(&words);
        assert!(found.iter().any(|p| p.contains("dark: class")), "{found:?}");
        assert!(
            found.iter().any(|p| p.contains("dark: the few-sample")),
            "{found:?}"
        );
    }

    #[test]
    fn listener_numbers_from_too_few_rows_fail() {
        let mut words = crate::words::parse().unwrap();
        let few = words
            .listeners
            .eq
            .iter_mut()
            .find(|row| row.n == 1)
            .unwrap();
        few.peak_hz = Some(120);
        words.listeners.reverb[0].n = 1;
        let found = listener_problems(&words);
        assert!(
            found.iter().any(|p| p.contains("too close to one curve")),
            "{found:?}"
        );
        assert!(
            found.iter().any(|p| p.contains("too close to one row")),
            "{found:?}"
        );
    }

    #[test]
    fn a_tag_states_how_many_sources_back_it() {
        assert!(tag_problem(Tag::SourceStates, &strings(&["a", "b"])).is_none());
        assert!(tag_problem(Tag::SourceStates, &strings(&["a", "snip-b"])).is_some());
        assert!(tag_problem(Tag::SingleSource, &strings(&["a", "snip-b"])).is_none());
        assert!(tag_problem(Tag::SingleSource, &strings(&["a", "b"])).is_some());
        assert!(tag_problem(Tag::Snippet, &strings(&["snip-a"])).is_none());
        assert!(tag_problem(Tag::Snippet, &strings(&["snip-a", "b"])).is_some());
        assert!(tag_problem(Tag::CrowdData, &strings(&["crowd-eq"])).is_none());
        assert!(tag_problem(Tag::CrowdData, &strings(&["crowd-eq", "b"])).is_some());
        assert!(tag_problem(Tag::SourceStates, &strings(&["crowd-eq", "a"])).is_some());
        assert!(tag_problem(Tag::SingleSource, &strings(&["crowd-eq", "a"])).is_none());
        assert!(tag_problem(Tag::Inferred, &[]).is_none());
        assert!(tag_problem(Tag::Inferred, &strings(&["a"])).is_some());
        assert_eq!(
            tag_problem(Tag::SourceStates, &strings(&["a", "a"])),
            Some("a source is listed twice")
        );
    }

    #[test]
    fn a_capitalised_word_or_alias_fails() {
        let mut words = crate::words::parse().unwrap();
        words.entries[0].word = words.entries[0].word.to_uppercase();
        let found = problems(&words);
        assert!(
            found.iter().any(|p| p.contains("is not lowercase")),
            "{found:?}"
        );
    }
}
