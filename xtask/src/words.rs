//! Writes the word reference files under `reference/core/` from the word
//! dictionary: one file per axis, and one for the intensity ladder and the
//! style and technique lists.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use sound_to_patch::words::check::CONTROL_GROUPS;
use sound_to_patch::words::{AXES, Axis, Entry, Tag, Value, Words, dictionary};

use crate::music::{CORE, page};
use crate::reference::REFERENCE;

const CONVENTIONS: &str = "Each row is one sense of a producer word, with no synth control: \
the synth reference binds the sense to controls. Agreement tells how alike the EQ curves were \
that listeners drew for the word: 1 means every listener drew the same curve shape, 0 means no \
shared shape. It reads high from 0.45, medium from 0.20 and low below. `n` is the number of \
curves, and the class is known only from 8; `few` marks fewer, so treat that word as a guess. \
Reverb numbers show how listeners set a reverb for the word: how far each setting (delay, \
decay, stereo spread, cutoff, wet gain) moved from the average, in standard deviations, and \
the spread, which is low when listeners agreed and 1 when they did not. Numbers carry their \
source ids and a tag: `source states` (two or more sources), `single source`, `snippet` \
(search excerpts only), `crowd data` (the listener data alone, which counts as no source) or \
`inferred` (no source). Source ids are listed in `dictionary/sources.json`.";

/// Every word file, by path relative to the repository root.
pub(crate) fn files() -> crate::Result<BTreeMap<PathBuf, String>> {
    let words = dictionary()?;
    let dir = Path::new(REFERENCE).join(CORE);
    let mut files = BTreeMap::new();
    for file in AXES {
        files.insert(
            dir.join(format!("words-{}.md", file.axis.as_str())),
            axis_page(words, file.axis),
        );
    }
    files.insert(dir.join("words-lists.md"), lists_page(words));
    Ok(files)
}

fn title(axis: Axis) -> &'static str {
    match axis {
        Axis::Tone => "Tone words: brightness and balance",
        Axis::Body => "Body words: weight and low end",
        Axis::Texture => "Texture words: grit, noise and era",
        Axis::Structure => "Structure words: harmonics and resonance",
        Axis::Space => "Space words: width, room and distance",
        Axis::Movement => "Movement words: change over time",
        Axis::Envelope => "Envelope words: attack, decay and dynamics",
        Axis::Mood => "Mood words: character with no control",
    }
}

fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

fn tag(tag: Tag) -> &'static str {
    match tag {
        Tag::SourceStates => "source states",
        Tag::SingleSource => "single source",
        Tag::Snippet => "snippet",
        Tag::CrowdData => "crowd data",
        Tag::Inferred => "inferred",
    }
}

fn value_text(value: &Value) -> String {
    // Display prints an integral f64 without a fraction: 22050, not 22050.0.
    let range = if value.low.to_bits() == value.high.to_bits() {
        value.low.to_string()
    } else {
        format!("{} to {}", value.low, value.high)
    };
    let unit = match value.unit.as_str() {
        "cents" | "semitones" | "bits" | "voices" if range == "1" => {
            value.unit.trim_end_matches('s')
        }
        unit => unit,
    };
    format!(
        "{}: {range} {unit} ({}, {})",
        value.what,
        tag(value.tag),
        value.sources.join(" ")
    )
}

fn agreement_text(entry: &Entry) -> String {
    let agreement = &entry.agreement;
    let class = serde_json::to_value(agreement.class)
        .ok()
        .and_then(|class| class.as_str().map(str::to_owned))
        .unwrap_or_default();
    let mut text = match agreement.value {
        Some(value) => format!("{class} {value:.2}, n {}", agreement.n),
        None if agreement.n > 0 => format!("{class}, n {}", agreement.n),
        None => class,
    };
    if entry.few_sample {
        text.push_str(", few");
    }
    text
}

fn reverb_text(entry: &Entry) -> String {
    entry.reverb.map_or_else(String::new, |reverb| {
        let e = reverb.effects;
        format!(
            "n {}, spread {:.2}; delay {:+.2}, decay {:+.2}, stereo {:+.2}, cutoff {:+.2}, wet {:+.2}",
            reverb.n,
            reverb.spread_ratio,
            e.delay_time,
            e.decay,
            e.stereo_spread,
            e.cutoff_freq,
            e.wet_gain
        )
    })
}

fn row(entry: &Entry) -> String {
    let mut word = format!("**{}**", entry.word);
    if !entry.aliases.is_empty() {
        let _ = write!(word, " ({})", entry.aliases.join(", "));
    }
    let mut sense = format!("`{}`", entry.id);
    if entry.default {
        sense.push_str(" default");
    }
    if !entry.roles.is_empty() {
        let _ = write!(sense, "; roles {}", entry.roles.join(", "));
    }
    let mut meaning = entry.meaning.clone();
    if let Some(hint) = &entry.card_hint {
        let _ = write!(meaning, " No control; card hint: {hint}.");
    }
    if !entry.music.is_empty() {
        let _ = write!(meaning, " Music: {}.", entry.music.join(", "));
    }
    if !entry.conflicts.is_empty() {
        let _ = write!(meaning, " Conflicts with {}.", entry.conflicts.join(", "));
    }
    let numbers: Vec<String> = entry.values.iter().map(value_text).collect();
    let measure = entry.measure.as_ref().map_or_else(String::new, |measure| {
        let band = measure
            .band
            .as_ref()
            .map_or_else(String::new, |band| format!(" {band}"));
        format!("`{}`{band} {}", measure.key, measure.direction)
    });
    format!(
        "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
        cell(&word),
        cell(&sense),
        cell(&meaning),
        cell(&numbers.join("<br>")),
        cell(&agreement_text(entry)),
        cell(&reverb_text(entry)),
        cell(&measure),
        cell(&if entry.sources.is_empty() {
            tag(entry.tag).to_owned()
        } else {
            format!("{} ({})", tag(entry.tag), entry.sources.join(" "))
        })
    )
}

fn axis_page(words: &Words, axis: Axis) -> String {
    let mut table = String::from(
        "| Word | Sense | Meaning | Numbers | Agreement | Reverb | Measure | Tag |\n| :-- | :-- | :-- | :-- | :-- | :-- | :-- | :-- |\n",
    );
    let entries: Vec<&Entry> = words.entries.iter().filter(|e| e.axis == axis).collect();
    for entry in &entries {
        table.push_str(&row(entry));
    }
    let mut sections = vec![("Words".to_owned(), table)];
    // Each word is asked once, at its first sense, wherever its senses sit.
    let asked: Vec<&str> = entries
        .iter()
        .filter(|entry| {
            let senses = words.senses(&entry.word);
            senses.len() > 1 && senses[0].id == entry.id
        })
        .map(|entry| entry.word.as_str())
        .collect();
    if !asked.is_empty() {
        let mut questions = String::from(
            "When the request names a role, take the one sense that lists it. Otherwise take \
             the default, or ask the question once when the senses sound strongly different.\n\n",
        );
        for word in asked {
            if let Some(question) = words.question(word) {
                let _ = writeln!(questions, "- {question}");
            }
        }
        sections.push(("Questions".to_owned(), questions));
    }
    page(title(axis), CONVENTIONS, &sections)
}

fn lists_page(words: &Words) -> String {
    let ladder = &words.ladder;
    let mut intensity = format!(
        "{} Tag: {}.\n\n| Modifier | Step |\n| :-- | :-- |\n| (none) | medium |\n",
        ladder.meaning,
        tag(ladder.tag)
    );
    for modifier in &ladder.modifiers {
        let warn = if modifier.warn { ", warn" } else { "" };
        let _ = writeln!(
            intensity,
            "| {} | {}{warn} |",
            modifier.words.join(", "),
            modifier.step
        );
    }
    intensity.push_str("\n| Control | Step | Anchor |\n| :-- | :-- | :-- |\n");
    for anchor in &ladder.anchors {
        let _ = writeln!(
            intensity,
            "| {} | {} | {} |",
            anchor.control,
            anchor.step,
            cell(&value_text(&anchor.value))
        );
    }
    let mut style = String::from(
        "A style word hints at a card and never picks one alone. `pending` waits for the \
         routing data.\n\n| Rank | Word | Hint |\n| :-- | :-- | :-- |\n",
    );
    for row in &words.style {
        let hint = row.hint.as_deref().unwrap_or("");
        let _ = writeln!(style, "| {} | {} | {hint} |", row.rank, ranked_word(row));
    }
    let mut technique = format!(
        "The first {} name a neutral control group that the synth reference binds to \
         controls; the rest are unmapped.\n\n| Rank | Word | Control group |\n| :-- | :-- | :-- |\n",
        CONTROL_GROUPS.len()
    );
    for row in &words.technique {
        let group = row.group.as_deref().unwrap_or("unmapped");
        let _ = writeln!(
            technique,
            "| {} | {} | {group} |",
            row.rank,
            ranked_word(row)
        );
    }
    page(
        "Intensity, style and technique words",
        "The modifier steps and their sourced anchors, then the style and technique words in \
         the rank order of the published word lexicon (source `word-lexicon`).",
        &[
            ("Intensity ladder".to_owned(), intensity),
            ("Style words".to_owned(), style),
            ("Technique words".to_owned(), technique),
        ],
    )
}

fn ranked_word(row: &sound_to_patch::words::Ranked) -> String {
    if row.aliases.is_empty() {
        row.word.clone()
    } else {
        format!("{} ({})", row.word, row.aliases.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_prints_its_range_unit_tag_and_sources() {
        let value = Value {
            what: "band of boominess".to_owned(),
            low: 60.0,
            high: 125.0,
            unit: "Hz".to_owned(),
            sources: vec!["gloss-a".to_owned(), "gloss-b".to_owned()],
            tag: Tag::SourceStates,
        };
        assert_eq!(
            value_text(&value),
            "band of boominess: 60 to 125 Hz (source states, gloss-a gloss-b)"
        );
        let one = Value {
            low: 1.0,
            high: 1.0,
            unit: "cents".to_owned(),
            ..value
        };
        assert!(
            value_text(&one).contains(": 1 cent ("),
            "{}",
            value_text(&one)
        );
        let wide = Value {
            low: 0.2,
            high: 22050.0,
            unit: "Hz".to_owned(),
            ..one
        };
        assert!(
            value_text(&wide).contains(": 0.2 to 22050 Hz ("),
            "{}",
            value_text(&wide)
        );
    }
}
