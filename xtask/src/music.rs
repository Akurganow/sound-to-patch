//! Writes the music reference files under `reference/core/` from the music
//! dictionary: one file per topic, the words with several senses and the
//! stated shares. The Camelot, middle-C, Euclidean, swing and tick tables
//! come from the functions the binary uses, not from stored rows.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use sound_to_patch::music::check::coverage;
use sound_to_patch::music::entry::{ArpOrder, Entry, Group, LengthUnit, Rule, Voice, VoicingRule};
use sound_to_patch::music::key::{Camelot, Mode};
use sound_to_patch::music::pitch::{OctaveLabel, Spelling, note_name};
use sound_to_patch::music::rhythm::{
    Division, euclid, onsets, rotate, swing_delay, swing_ratio, swing_ticks, tick_ms,
};
use sound_to_patch::music::{Dictionary, TOPICS, dictionary};

use crate::reference::{CONTENTS_ABOVE, HEADER, REFERENCE, anchor};

/// The folder of the files every synth shares.
pub(crate) const CORE: &str = "core";

const CONVENTIONS: &str = "Pitches are MIDI numbers, 60 = middle C. Onsets are slots 0 to 15 \
of a sixteenth grid, so slot 4 is beat 2; lengths are in beats. Tiers: M1 is needed in a first \
test set, M2 has one or two sources or is a jazz or world idiom, M3 is single-source, inferred \
or theory only. Tags: `source states`, `measured`, `derived` (our computation on public data), \
`inferred` (our reading), `snippet` (a search excerpt), `single source`, `proposal` (a policy, \
not a fact). Source ids are listed in `dictionary/sources.json`.";

/// Every music file, by path relative to the repository root.
pub(crate) fn files() -> crate::Result<BTreeMap<PathBuf, String>> {
    let dictionary = dictionary()?;
    let dir = Path::new(REFERENCE).join(CORE);
    let mut files = BTreeMap::new();
    for topic in TOPICS {
        let mut sections: Vec<(String, String)> = topic
            .groups
            .iter()
            .map(|group| {
                (
                    group_title(*group).to_owned(),
                    entry_table(dictionary, *group),
                )
            })
            .collect();
        sections.extend(extra_sections(topic.name));
        files.insert(
            dir.join(format!("{}.md", topic.name)),
            page(topic.title, CONVENTIONS, &sections),
        );
    }
    files.insert(dir.join("senses.md"), senses_page(dictionary));
    files.insert(
        dir.join("statistics-harmony.md"),
        statistics_page(dictionary, "Stated shares: chords and progressions", true),
    );
    files.insert(
        dir.join("statistics-keys.md"),
        statistics_page(
            dictionary,
            "Stated shares: keys, modes, melody and rhythm",
            false,
        ),
    );
    Ok(files)
}

/// The coverage report, and an error when an M1 or M2 group falls short.
pub(crate) fn check_coverage() -> crate::Result<()> {
    let rows = coverage(dictionary()?);
    let gaps: Vec<String> = rows
        .iter()
        .filter(|row| !row.meets_floor())
        .map(|row| {
            format!(
                "{} {:?}: {} of {} landed, needs {} %",
                row.group.as_str(),
                row.tier,
                row.landed,
                row.planned,
                row.tier.floor()
            )
        })
        .collect();
    if gaps.is_empty() {
        Ok(())
    } else {
        Err(format!("music coverage gaps: {}", gaps.join("; ")).into())
    }
}

pub(crate) fn page(title: &str, intro: &str, sections: &[(String, String)]) -> String {
    let mut text = format!("{HEADER}\n\n# {title}\n\n{intro}\n");
    let mut body = String::new();
    for (heading, section) in sections {
        let _ = write!(body, "\n## {heading}\n\n{section}");
    }
    if text.lines().count() + body.lines().count() > CONTENTS_ABOVE && sections.len() > 1 {
        text.push_str("\n## Contents\n\n");
        for (heading, _) in sections {
            let _ = writeln!(text, "- [{heading}](#{})", anchor(heading));
        }
    }
    text.push_str(&body);
    text
}

fn group_title(group: Group) -> &'static str {
    match group {
        Group::Notation => "Note names and middle C",
        Group::Key => "Keys",
        Group::Role => "Role ranges",
        Group::Scale => "Scales and modes",
        Group::Mood => "Mood words",
        Group::Outside => "Outside notes",
        Group::Chord => "Chords",
        Group::Voicing => "Voicings",
        Group::VoiceLeading => "Voice leading",
        Group::Progression => "Progressions",
        Group::Harmony => "Harmony habits",
        Group::Position => "Pulse and positions",
        Group::Swing => "Swing",
        Group::Feel => "Feel and humanize",
        Group::Rhythm => "Named rhythms",
        Group::Euclid => "Euclidean patterns",
        Group::Articulation => "Length and articulation",
        Group::Bass => "Bass techniques",
        Group::Lead => "Lead and phrase techniques",
        Group::Arp => "Arpeggios",
        Group::Pad => "Pads, stabs and keys",
        Group::Phrase => "Phrase structure",
        Group::Genre => "Genre templates",
        _ => "Other",
    }
}

fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

fn list<T: ToString>(values: &[T]) -> String {
    values
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(" ")
}

/// A column title and the cell of an entry.
type Column = (&'static str, fn(&Entry) -> String);

/// The extra column of a group, if any.
fn extra_column(group: Group) -> Option<Column> {
    match group {
        Group::Chord => Some(("Symbols on C", |entry| {
            entry
                .symbols
                .iter()
                .map(|symbol| format!("`C{symbol}`"))
                .collect::<Vec<_>>()
                .join(", ")
        })),
        Group::Genre => Some(("BPM", |entry| {
            entry.bpm.map_or_else(String::new, |bpm| match bpm.center {
                Some(center) => format!("{} to {}, center {center}", bpm.low, bpm.high),
                None => format!("{} to {}", bpm.low, bpm.high),
            })
        })),
        Group::Mood | Group::Scale => Some(("Default pick", |entry| {
            entry
                .default_pick
                .as_ref()
                .map_or_else(String::new, |pick| {
                    let mut text = pick
                        .scale
                        .as_ref()
                        .map_or_else(|| "none".to_owned(), |scale| format!("`{scale}`"));
                    if !pick.alternatives.is_empty() {
                        let others: Vec<String> = pick
                            .alternatives
                            .iter()
                            .map(|id| format!("`{id}`"))
                            .collect();
                        let _ = write!(text, "; also {}", others.join(", "));
                    }
                    let _ = write!(text, ". {}", pick.note);
                    text
                })
        })),
        Group::Progression | Group::Harmony => Some(("Scale", |entry| {
            entry
                .scale
                .as_ref()
                .map_or_else(String::new, |scale| format!("`{scale}`"))
        })),
        _ => None,
    }
}

fn entry_table(dictionary: &Dictionary, group: Group) -> String {
    let extra = extra_column(group);
    let mut text = String::from("| Word | Id | Rule | Meaning |");
    if let Some((title, _)) = extra {
        let _ = write!(text, " {title} |");
    }
    text.push_str(" Tier | Tag | Sources |\n| :-- | :-- | :-- | :-- |");
    if extra.is_some() {
        text.push_str(" :-- |");
    }
    text.push_str(" :-- | :-- | :-- |\n");
    for entry in dictionary.group(group) {
        let mut word = entry.word.clone();
        if !entry.aliases.is_empty() {
            let _ = write!(word, " ({})", entry.aliases.join(", "));
        }
        if let Some(sense) = &entry.sense {
            let _ = write!(word, ", sense `{sense}`");
            if entry.default {
                word.push_str(", default");
            }
        }
        let _ = write!(
            text,
            "| {} | `{}` | {} | {} |",
            cell(&word),
            entry.id,
            cell(&rule_text(&entry.rule)),
            cell(&entry.meaning)
        );
        if let Some((_, value)) = extra {
            let _ = write!(text, " {} |", cell(&value(entry)));
        }
        let _ = writeln!(
            text,
            " {:?} | {} | {} |",
            entry.tier,
            entry.tag.as_str(),
            entry.source.join(", ")
        );
    }
    text
}

/// The rule of an entry in a short line.
fn rule_text(rule: &Rule) -> String {
    match rule {
        Rule::PitchSet {
            offsets,
            colour,
            giveaway,
        } => pitch_set_text(offsets, colour, giveaway),
        Rule::Tonic {} => {
            "tonic: the first or last phrase note, or the held bass, is the root".into()
        }
        Rule::ChordTones { offsets, choose } => {
            let mut text = format!("tones {}", list(offsets));
            if let Some(choose) = choose {
                let _ = write!(
                    text,
                    "; at least {} of {}",
                    choose.at_least,
                    list(&choose.from)
                );
            }
            text
        }
        Rule::Voicing(voicing) => voicing_text(voicing),
        Rule::Cycle {
            roots,
            chords,
            bars_per_chord,
        } => cycle_text(roots, chords, *bars_per_chord),
        Rule::OnsetSet { parts } => parts
            .iter()
            .map(|part| format!("{} at {}", part.part, list(&part.slots)))
            .collect::<Vec<_>>()
            .join("; "),
        Rule::OnsetShare { slots, min_share } => format!(
            "at least {:.0} % of onsets on slots {}",
            min_share * 100.0,
            list(slots)
        ),
        Rule::CountPerBar { min, max } => format!("{min} to {max} notes per bar"),
        Rule::Length { unit, min, max } => length_text(*unit, *min, *max),
        Rule::Velocity { min, max } => format!("velocity {min} to {max}"),
        Rule::Overlap { min_ms } => {
            format!("the next note starts at least {min_ms} ms before this one ends")
        }
        Rule::Swing {
            note,
            min_pct,
            max_pct,
        } => format!("{note}th swing, pair percent {min_pct} to {max_pct}"),
        Rule::Shift { min_ms, max_ms } => {
            format!("every onset {min_ms} to {max_ms} ms off the grid, late positive")
        }
        Rule::Jitter { max_ms, velocity } => {
            format!("onsets vary up to {max_ms} ms, velocities up to {velocity}")
        }
        Rule::Register { core, hard, voice } => {
            let voice = match voice {
                Voice::Mono => "mono",
                Voice::Poly => "poly",
                Voice::Either => "mono or poly",
            };
            format!(
                "core {} to {}, hard {} to {}, {voice}",
                core[0], core[1], hard[0], hard[1]
            )
        }
        Rule::Pattern { k, n, rotation } => {
            let pattern = rotate(
                &euclid(usize::from(*k), usize::from(*n)),
                usize::from(*rotation),
            );
            format!(
                "E({k},{n}) rotated {rotation}: steps {}",
                list(&onsets(&pattern))
            )
        }
        Rule::Arp {
            order,
            rate,
            gate,
            degrees,
        } => arp_text(*order, *rate, *gate, degrees),
        Rule::Repeat {
            min_share,
            max_share,
            min_bars,
        } => repeat_text(*min_share, *max_share, *min_bars),
        Rule::PhraseLength { every } => {
            let every: Vec<String> = every.iter().map(ToString::to_string).collect();
            format!(
                "changes every {} bars, on bars 1 + k times that",
                every.join(" or ")
            )
        }
        Rule::None {} => "none".into(),
    }
}

fn pitch_set_text(offsets: &[u8], colour: &[u8], giveaway: &[u8]) -> String {
    let mut text = format!("offsets {}", list(offsets));
    if !colour.is_empty() {
        let _ = write!(text, "; colour {}", list(colour));
    }
    if !giveaway.is_empty() {
        let _ = write!(text, "; giveaway {}", list(giveaway));
    }
    text
}

fn cycle_text(roots: &[u8], chords: &[String], bars_per_chord: Option<f64>) -> String {
    let pairs: Vec<String> = roots
        .iter()
        .zip(chords)
        .map(|(root, chord)| format!("{root} {}", chord.trim_start_matches("chord.")))
        .collect();
    let mut text = format!("roots and chords: {}", pairs.join(", "));
    if let Some(bars) = bars_per_chord {
        let unit = if (bars - 1.0).abs() < f64::EPSILON {
            "bar"
        } else {
            "bars"
        };
        let _ = write!(text, "; {bars} {unit} per chord");
    }
    text
}

fn length_text(unit: LengthUnit, min: Option<f64>, max: Option<f64>) -> String {
    let unit = match unit {
        LengthUnit::Beat => "beats",
        LengthUnit::Step => "of the grid step",
    };
    match (min, max) {
        (Some(min), Some(max)) => format!("sounding length {min} to {max} {unit}"),
        (Some(min), None) => format!("sounding length at least {min} {unit}"),
        (None, Some(max)) => format!("sounding length at most {max} {unit}"),
        (None, None) => format!("sounding length in {unit}"),
    }
}

fn arp_text(order: ArpOrder, rate: f64, gate: Option<[u8; 2]>, degrees: &[u8]) -> String {
    let order = match order {
        ArpOrder::Up => "up".to_owned(),
        ArpOrder::Down => "down".to_owned(),
        ArpOrder::UpDown => "up and down".to_owned(),
        ArpOrder::Sequence => format!("degrees {}", list(degrees)),
    };
    let mut text = format!("{order}, {rate} beats per step");
    if let Some([low, high]) = gate {
        let _ = write!(text, ", gate {low} to {high} %");
    }
    text
}

fn repeat_text(min_share: Option<f64>, max_share: Option<f64>, min_bars: Option<u8>) -> String {
    let mut parts = Vec::new();
    if let Some(min) = min_share {
        parts.push(format!(
            "at least {:.0} % of bars repeat an earlier bar",
            min * 100.0
        ));
    }
    if let Some(max) = max_share {
        parts.push(format!(
            "at most {:.0} % of bars repeat an earlier bar",
            max * 100.0
        ));
    }
    if let Some(bars) = min_bars {
        parts.push(format!("for at least {bars} bars"));
    }
    parts.join("; ")
}

fn voicing_text(voicing: &VoicingRule) -> String {
    let shape = serde_json::to_value(voicing.shape)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_default();
    let mut parts = vec![shape];
    let numbers = [
        ("span at most", voicing.max_span),
        ("span at least", voicing.min_span),
        ("gap at most", voicing.max_gap),
        ("gap at least", voicing.min_gap),
        ("step", voicing.step),
        ("step tolerance", voicing.step_tolerance),
        ("notes at least", voicing.min_notes),
        ("notes at most", voicing.max_notes),
        ("pitch classes at least", voicing.min_pitch_classes),
        ("lowest", voicing.lowest),
        ("tones under", voicing.below),
        ("at most under it", voicing.max_below),
        ("leap at most", voicing.max_leap),
    ];
    for (name, value) in numbers {
        if let Some(value) = value {
            parts.push(format!("{name} {value}"));
        }
    }
    if !voicing.drop.is_empty() {
        parts.push(format!("drop voices {} from the top", list(&voicing.drop)));
    }
    if let Some(bass) = voicing.bass {
        let bass = serde_json::to_value(bass)
            .ok()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .unwrap_or_default();
        parts.push(format!("bass {bass}"));
    }
    if !voicing.degrees.is_empty() {
        parts.push(format!(
            "degrees {} from the bottom",
            list(&voicing.degrees)
        ));
    }
    if !voicing.floors.is_empty() {
        let floors: Vec<String> = voicing
            .floors
            .iter()
            .map(|floor| match floor.lowest {
                Some(lowest) => format!("{}:{lowest}", floor.interval),
                None => format!("{}:none", floor.interval),
            })
            .collect();
        parts.push(format!(
            "floors (interval:lowest pitch) {}",
            floors.join(" ")
        ));
    }
    parts.join("; ")
}

fn extra_sections(topic: &str) -> Vec<(String, String)> {
    match topic {
        "notation" => vec![
            ("Middle-C labels".into(), middle_c_table()),
            ("Camelot and Open Key".into(), camelot_table()),
        ],
        "rhythm" => vec![
            ("Euclidean pattern table".into(), euclid_tables()),
            ("Swing table".into(), swing_table()),
            ("Ticks".into(), tick_table()),
        ],
        _ => Vec::new(),
    }
}

fn middle_c_table() -> String {
    let mut text = String::from(
        "A program labels note 60 C3, C4 or C5. Ask the producer which label their program \
         shows before reading a note name; store MIDI numbers.\n\n\
         | MIDI | Label C4 (scientific) | Label C3 | Label C5 |\n| --: | :-- | :-- | :-- |\n",
    );
    for pitch in [21, 28, 36, 60, 69, 127] {
        let _ = writeln!(
            text,
            "| {pitch} | {} | {} | {} |",
            note_name(pitch, OctaveLabel::C4, Spelling::Sharps),
            note_name(pitch, OctaveLabel::C3, Spelling::Sharps),
            note_name(pitch, OctaveLabel::C5, Spelling::Sharps)
        );
    }
    text.push_str(
        "\nOctave number of MIDI `n`: `n div 12 - 1` with label C4, `n div 12 - 2` with C3, \
         `n div 12` with C5. Frequency: `440 * 2^((n - 69) / 12)` Hz.\n",
    );
    text
}

fn camelot_table() -> String {
    let mut text = String::from(
        "The same number with the other letter is the relative key; one number up or down is a \
         fifth; 12 wraps to 1.\n\n\
         | Camelot | Minor (A) | Major (B) | Open Key |\n| --: | :-- | :-- | :-- |\n",
    );
    for number in 1..=12 {
        let minor = Camelot {
            number,
            mode: Mode::Minor,
        }
        .key();
        let major = Camelot {
            number,
            mode: Mode::Major,
        }
        .key();
        let _ = writeln!(
            text,
            "| {number} | {minor} | {major} | {} and {} |",
            minor.open_key(),
            major.open_key()
        );
    }
    text
}

fn euclid_tables() -> String {
    let mut text = String::from(
        "Onset steps of E(k, n), from the grouping algorithm, rotation 0. With a sixteenth \
         step, n = 16 fills a bar and n = 8 half a bar.\n\n| n | k | Steps |\n| --: | --: | :-- |\n",
    );
    for n in [8_usize, 12, 16] {
        for k in 2..n {
            let _ = writeln!(text, "| {n} | {k} | {} |", list(&onsets(&euclid(k, n))));
        }
    }
    text
}

fn swing_table() -> String {
    let mut text = String::from(
        "Pair percent of 16th swing: the second sixteenth of each pair starts at that share of \
         the pair. 50 is straight. Store the pair percent, never a program's own swing scale.\n\n\
         | Pair % | Ratio | Delay in 16ths | Ticks late | ms late at 90 / 120 / 140 BPM |\n\
         | --: | --: | --: | --: | :-- |\n",
    );
    for pct in [54.0, 58.0, 62.0, 66.7, 75.0] {
        let ticks = swing_ticks(pct, 16);
        let ms: Vec<String> = [90.0, 120.0, 140.0]
            .iter()
            .map(|bpm| format!("{:.1}", ticks * tick_ms(*bpm)))
            .collect();
        let _ = writeln!(
            text,
            "| {pct} | {:.2}:1 | {:.2} | {ticks:.1} | {} |",
            swing_ratio(pct),
            swing_delay(pct),
            ms.join(" / ")
        );
    }
    text
}

fn tick_table() -> String {
    let mut text = String::from(
        "480 ticks per quarter note. One tick lasts `60000 / (BPM * 480)` ms.\n\n\
         | Division | Ticks |\n| :-- | --: |\n",
    );
    for division in Division::ALL {
        let _ = writeln!(text, "| {} | {} |", division.name(), division.ticks());
    }
    text
}

fn senses_page(dictionary: &Dictionary) -> String {
    let mut words: BTreeMap<&str, Vec<&Entry>> = BTreeMap::new();
    for entry in &dictionary.entries {
        words.entry(entry.word.as_str()).or_default().push(entry);
    }
    let mut body =
        String::from("| Word | Senses | Default | Question |\n| :-- | :-- | :-- | :-- |\n");
    for (word, senses) in words.iter().filter(|(_, senses)| senses.len() > 1) {
        let listed: Vec<String> = senses
            .iter()
            .map(|entry| format!("`{}`: {}", entry.id, cell(&entry.meaning)))
            .collect();
        let default = senses
            .iter()
            .find(|entry| entry.default)
            .map_or_else(String::new, |entry| format!("`{}`", entry.id));
        let question = senses
            .iter()
            .find_map(|entry| entry.ask.as_ref())
            .map_or_else(String::new, |ask| {
                format!(
                    "{} When to ask: {} Batch default: `{}`.",
                    ask.question,
                    ask.when.trim_end_matches('.').to_owned() + ".",
                    ask.default
                )
            });
        let _ = writeln!(
            body,
            "| {} | {} | {default} | {} |",
            cell(word),
            listed.join("<br>"),
            cell(&question)
        );
    }
    page(
        "Words with several senses",
        "Each sense is its own entry. Take the default when the request gives no context. Ask \
         the question only when the request leaves the sense open and the answer changes the \
         notes; a batch run takes the stated option without asking.",
        &[("Senses".to_owned(), body)],
    )
}

/// The stat prefixes of the harmony page; every other prefix goes to the
/// keys page.
const HARMONY_STATS: [&str; 3] = ["chords", "harmony", "progressions"];

fn statistics_page(dictionary: &Dictionary, title: &str, harmony: bool) -> String {
    let mut topics: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for stat in &dictionary.stats {
        let topic = stat.id.split('.').next().unwrap_or("other");
        if HARMONY_STATS.contains(&topic) != harmony {
            continue;
        }
        let counts = match (stat.count, stat.total) {
            (Some(count), Some(total)) => format!("{count} of {total}"),
            _ => String::new(),
        };
        topics.entry(topic).or_default().push(format!(
            "| {} | `{}` | {} | {counts} | {} | {} |",
            cell(&stat.claim),
            stat.id,
            stat.share,
            stat.tag.as_str(),
            stat.source.join(", ")
        ));
    }
    let sections: Vec<(String, String)> = topics
        .into_iter()
        .map(|(topic, rows)| {
            let mut body = String::from(
                "| Claim | Id | Share % | Counts | Tag | Sources |\n| :-- | :-- | --: | :-- | :-- | :-- |\n",
            );
            for row in rows {
                body.push_str(&row);
                body.push('\n');
            }
            (topic_title(topic), body)
        })
        .collect();
    page(
        title,
        "Shares the music dictionary rests on, in percent. A share with counts is recomputed \
         from them. Corpora skew to pop and rock and annotated sets favour corrected tracks, so \
         use these as priors and warning bands, never as targets.",
        &sections,
    )
}

fn topic_title(prefix: &str) -> String {
    TOPICS
        .iter()
        .find(|known| known.name == prefix)
        .map_or_else(
            || {
                let words = prefix.replace('-', " ");
                let mut chars = words.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_uppercase().chain(chars).collect()
                })
            },
            |known| known.title.to_owned(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn music_files_stay_under_the_line_cap() {
        for (path, text) in files().unwrap() {
            assert!(text.lines().count() <= 300, "{}", path.display());
            if text.lines().count() > CONTENTS_ABOVE {
                assert!(text.contains("\n## Contents\n"), "{}", path.display());
            }
        }
    }

    #[test]
    fn the_camelot_table_starts_on_the_published_row() {
        let table = camelot_table();
        assert!(
            table.contains("| 8 | A minor | C major | 1m and 1d |"),
            "{table}"
        );
        assert!(
            table.contains("| 1 | G# minor | B major | 6m and 6d |"),
            "{table}"
        );
    }
}
