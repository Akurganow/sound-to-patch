//! The music dictionary data: schemas, the loader's rules, counts and the
//! oracle comparisons.

use std::collections::BTreeSet;
use std::fs;

use serde_json::Value;
use sound_to_patch::music::check::coverage;
use sound_to_patch::music::chord::parse_chord;
use sound_to_patch::music::entry::{Fragment, Group, Rule};
use sound_to_patch::music::roman::parse_line;
use sound_to_patch::music::{MusicError, TOPICS, dictionary};

fn validator(schema: &str) -> jsonschema::Validator {
    let schema: Value = serde_json::from_str(&fs::read_to_string(schema).unwrap()).unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

fn load(path: &str) -> Value {
    serde_json::from_str(&fs::read_to_string(path).unwrap())
        .unwrap_or_else(|error| panic!("{path}: {error}"))
}

#[test]
fn the_music_files_match_their_schemas() {
    let music = validator("schemas/music.schema.json");
    for topic in TOPICS {
        let path = format!("dictionary/music/{}.json", topic.name);
        let value = load(&path);
        let errors: Vec<String> = music
            .iter_errors(&value)
            .map(|error| format!("{} at {}", error, error.instance_path()))
            .collect();
        assert!(errors.is_empty(), "{path}:\n{}", errors.join("\n"));
    }
    let stats = load("dictionary/music/stats.json");
    let errors: Vec<String> = validator("schemas/stats.schema.json")
        .iter_errors(&stats)
        .map(|error| format!("{} at {}", error, error.instance_path()))
        .collect();
    assert!(errors.is_empty(), "stats.json:\n{}", errors.join("\n"));
}

#[test]
fn the_dictionary_loads_and_keeps_its_rules() {
    if let Err(error) = dictionary() {
        panic!("{}", error.to_string().replace("; ", "\n"));
    }
}

#[test]
fn every_planned_m1_and_m2_entry_landed() {
    let gaps: Vec<String> = coverage(dictionary().unwrap())
        .into_iter()
        .filter(|row| !row.meets_floor())
        .map(|row| {
            format!(
                "{} {:?}: {} of {} landed",
                row.group.as_str(),
                row.tier,
                row.landed,
                row.planned
            )
        })
        .collect();
    assert!(gaps.is_empty(), "coverage gaps:\n{}", gaps.join("\n"));
}

/// Every chord spelling on root C with the pitch classes the dictionary's
/// parser gives it.
fn spellings() -> Vec<(String, BTreeSet<u8>)> {
    let mut spellings = Vec::new();
    for entry in dictionary().unwrap().group(Group::Chord) {
        for symbol in &entry.symbols {
            let text = format!("C{symbol}");
            let classes = parse_chord(&text)
                .unwrap_or_else(|error| panic!("{text}: {error}"))
                .classes();
            spellings.push((text, classes));
        }
    }
    spellings
}

#[test]
fn every_chord_spelling_parses_to_its_entry() {
    for entry in dictionary().unwrap().group(Group::Chord) {
        for symbol in &entry.symbols {
            let chord = parse_chord(&format!("C{symbol}")).unwrap();
            assert_eq!(chord.entry, Some(entry.id.as_str()), "C{symbol}");
        }
    }
    assert!(matches!(
        parse_chord("Cblorp"),
        Err(MusicError::UnsupportedChord(symbol)) if symbol == "Cblorp"
    ));
}

#[test]
fn chordparser_differs_only_on_the_pinned_spellings() {
    let mut parser = chordparser::parsing::Parser::new();
    let mut differ = BTreeSet::new();
    let mut unsupported = BTreeSet::new();
    for (text, classes) in spellings() {
        match parser.parse(&text) {
            Ok(chord) => {
                let oracle: BTreeSet<u8> = chord.semitones.iter().map(|s| s % 12).collect();
                if oracle != classes {
                    differ.insert(text);
                }
            }
            Err(_) => {
                unsupported.insert(text);
            }
        }
    }
    assert_eq!(
        differ,
        BTreeSet::from(CHORDPARSER_DIFFERS.map(str::to_owned))
    );
    assert_eq!(
        unsupported,
        BTreeSet::from(CHORDPARSER_UNSUPPORTED.map(str::to_owned))
    );
}

/// Spellings where the dictionary's policy differs from `chordparser`: the
/// minor 13 without its 11th, the 11 chords with their third, `+9` and
/// `-9` as altered ninths, the `alt` set, and the flat 13.
const CHORDPARSER_DIFFERS: [&str; 9] = [
    "C-13", "C11", "C7+9", "C7-9", "C7alt", "C7b13", "Calt", "Cm13", "Cmaj11",
];

/// Spellings `chordparser` cannot parse.
const CHORDPARSER_UNSUPPORTED: [&str; 14] = [
    "C7alt7", "Cdom", "Cdom7", "Cdom9", "Ch", "Ch7", "Cm/M7", "Cm/maj7", "Cmadd9", "Cmaj#4",
    "Cquartal", "Cquintal", "C−", "C−7",
];

fn offsets_of(id: &str) -> BTreeSet<u8> {
    match &dictionary()
        .unwrap()
        .entry(id)
        .unwrap_or_else(|| panic!("{id}"))
        .rule
    {
        Rule::PitchSet { offsets, .. } | Rule::ChordTones { offsets, .. } => {
            offsets.iter().map(|offset| offset % 12).collect()
        }
        rule => panic!("{id}: {}", rule.kind()),
    }
}

#[test]
fn the_scales_rust_music_theory_has_equal_the_dictionary() {
    let shared = [
        ("C ionian", "scale.major"),
        ("C dorian", "scale.dorian"),
        ("C phrygian", "scale.phrygian"),
        ("C lydian", "scale.lydian"),
        ("C mixolydian", "scale.mixolydian"),
        ("C aeolian", "scale.natural-minor"),
        ("C locrian", "scale.locrian"),
        ("C harmonic minor", "scale.harmonic-minor"),
        ("C melodic minor", "scale.melodic-minor"),
        ("C pentatonic major", "scale.major-pentatonic"),
        ("C pentatonic minor", "scale.minor-pentatonic"),
        ("C blues", "scale.minor-blues"),
        ("C whole tone", "scale.whole-tone"),
    ];
    for (name, id) in shared {
        let scale = rust_music_theory::scale::Scale::from_regex(name).unwrap();
        let oracle: BTreeSet<u8> = scale
            .absolute_intervals()
            .iter()
            .map(|interval| interval.semitone_count % 12)
            .collect();
        assert_eq!(oracle, offsets_of(id), "{name}");
    }
}

#[test]
fn the_chords_rust_music_theory_has_differ_only_by_policy() {
    use rust_music_theory::chord::{Chord, Number, Quality};
    let shared = [
        (Quality::Major, Number::Triad, "chord.major-triad"),
        (Quality::Minor, Number::Triad, "chord.minor-triad"),
        (Quality::Suspended2, Number::Triad, "chord.suspended-2nd"),
        (Quality::Suspended4, Number::Triad, "chord.suspended-4th"),
        (Quality::Augmented, Number::Triad, "chord.augmented-triad"),
        (Quality::Diminished, Number::Triad, "chord.diminished-triad"),
        (Quality::Major, Number::MajorSeventh, "chord.major-seventh"),
        (Quality::Minor, Number::Seventh, "chord.minor-seventh"),
        (
            Quality::Augmented,
            Number::Seventh,
            "chord.augmented-seventh",
        ),
        (
            Quality::Augmented,
            Number::MajorSeventh,
            "chord.augmented-major-seventh",
        ),
        (
            Quality::Diminished,
            Number::Seventh,
            "chord.diminished-seventh",
        ),
        (
            Quality::HalfDiminished,
            Number::Seventh,
            "chord.half-diminished",
        ),
        (
            Quality::Minor,
            Number::MajorSeventh,
            "chord.minor-major-seventh",
        ),
        (Quality::Dominant, Number::Seventh, "chord.dominant-seventh"),
        (Quality::Dominant, Number::Ninth, "chord.dominant-9"),
        (Quality::Major, Number::Ninth, "chord.major-9"),
        (Quality::Minor, Number::Ninth, "chord.minor-9"),
        (Quality::Dominant, Number::Eleventh, "chord.dominant-11"),
        (Quality::Major, Number::Eleventh, "chord.major-11"),
        (Quality::Minor, Number::Eleventh, "chord.minor-11"),
        (Quality::Dominant, Number::Thirteenth, "chord.dominant-13"),
        (Quality::Major, Number::Thirteenth, "chord.major-13"),
        (Quality::Minor, Number::Thirteenth, "chord.minor-13"),
    ];
    let mut differ = Vec::new();
    for (quality, number, id) in shared {
        let mut tone = 0;
        let mut oracle = BTreeSet::from([0]);
        for interval in Chord::try_chord_intervals(quality, number).unwrap() {
            tone += interval.semitone_count;
            oracle.insert(tone % 12);
        }
        if oracle != offsets_of(id) {
            differ.push(id);
        }
    }
    // The dictionary's policy: a 13 chord has no eleventh.
    assert_eq!(
        differ,
        ["chord.dominant-13", "chord.major-13", "chord.minor-13"]
    );
}

fn count(group: Group) -> usize {
    dictionary().unwrap().group(group).count()
}

fn default_words(group: Group) -> usize {
    dictionary()
        .unwrap()
        .group(group)
        .filter(|entry| entry.default)
        .count()
}

#[test]
fn the_dictionary_holds_the_published_rows() {
    let dictionary = dictionary().unwrap();
    let scale_rows = dictionary
        .group(Group::Scale)
        .filter(|entry| entry.sense.is_none() && entry.default_pick.is_none())
        .filter(|entry| !MAQAM.iter().any(|name| entry.id == format!("scale.{name}")))
        .count();
    assert_eq!(scale_rows, 57);
    assert_eq!(count(Group::Chord), 47);
    assert_eq!(default_words(Group::Mood), 14);
    assert_eq!(count(Group::Role), 9);
    assert_eq!(count(Group::Progression), 22);
    assert_eq!(count(Group::Genre), 20);
    assert_eq!(count(Group::Phrase), 7);
}

/// The maqam entries of the scale group, beside the 57 scale rows.
const MAQAM: [&str; 9] = [
    "hijaz", "nahawand", "ajam", "kurd", "nikriz", "rast", "bayati", "sikah", "saba",
];

#[test]
fn only_the_published_pair_of_scale_rows_shares_a_set() {
    let mut sets: Vec<(Vec<u8>, &str)> = dictionary()
        .unwrap()
        .group(Group::Scale)
        .filter(|entry| entry.sense.is_none() && entry.default_pick.is_none())
        .filter(|entry| !MAQAM.iter().any(|name| entry.id == format!("scale.{name}")))
        .filter_map(|entry| match &entry.rule {
            Rule::PitchSet { offsets, .. } => Some((offsets.clone(), entry.id.as_str())),
            _ => None,
        })
        .collect();
    sets.sort();
    let shared: Vec<(&str, &str)> = sets
        .windows(2)
        .filter(|pair| pair[0].0 == pair[1].0)
        .map(|pair| (pair[0].1, pair[1].1))
        .collect();
    assert_eq!(shared, [("scale.egyptian-pentatonic", "scale.slendro")]);
}

#[test]
fn role_ranges_keep_their_limits() {
    let mut hard_floors = Vec::new();
    for entry in dictionary().unwrap().group(Group::Role) {
        let Rule::Register { core, hard, .. } = &entry.rule else {
            panic!("{} has no register rule", entry.id)
        };
        assert!(hard[0] <= core[0] && core[1] <= hard[1], "{}", entry.id);
        assert!(hard[0] >= 21 && hard[1] <= 108, "{}", entry.id);
        hard_floors.push((entry.id.as_str(), hard[0]));
    }
    assert!(hard_floors.contains(&("role.808", 23)));
    for role in ["fx", "drum"] {
        assert!(
            dictionary()
                .unwrap()
                .entry(&format!("role.{role}"))
                .is_none()
        );
    }
}

/// The words of section 14 with several senses, and the ask words.
const ASK_WORDS: [&str; 9] = [
    "offbeat",
    "rolling",
    "half-time",
    "shuffle",
    "diminished",
    "bebop minor",
    "in A",
    "cinematic",
    "open",
];

#[test]
fn every_ask_word_has_its_senses_and_its_question() {
    let dictionary = dictionary().unwrap();
    for word in ASK_WORDS {
        let senses: Vec<_> = dictionary
            .entries
            .iter()
            .filter(|entry| entry.word == word)
            .collect();
        assert!(senses.len() >= 2, "{word} needs every sense");
        let asks: Vec<_> = senses
            .iter()
            .filter_map(|entry| entry.ask.as_ref())
            .collect();
        assert_eq!(asks.len(), 1, "{word} needs one question");
        assert_eq!(
            asks[0].options.len(),
            senses.len(),
            "{word}: every sense is an option"
        );
    }
    for word in [
        "arabic",
        "gypsy",
        "egyptian",
        "pentatonic",
        "shell",
        "upbeat",
    ] {
        let senses = dictionary
            .entries
            .iter()
            .filter(|entry| entry.word == word)
            .count();
        assert!(senses >= 2, "{word} needs every sense");
    }
}

#[test]
fn shared_sound_words_never_trigger_notes_alone() {
    let words = [
        "pluck",
        "arp",
        "stab",
        "wobble",
        "dark",
        "bright",
        "warm",
        "wide",
        "thick",
        "airy",
        "epic",
        "cinematic",
        "short",
        "long",
        "deep",
        "soft",
        "fast",
        "slow",
    ];
    for entry in &dictionary().unwrap().entries {
        if words.contains(&entry.word.as_str()) || entry.group == Group::Mood {
            assert!(
                !entry.notes_trigger,
                "{} must not trigger notes alone",
                entry.id
            );
        }
    }
}

#[test]
fn music_ids_never_collide_with_word_ids() {
    let music: BTreeSet<&str> = dictionary()
        .unwrap()
        .entries
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();
    for path in fs::read_dir("dictionary/words").unwrap() {
        let path = path.unwrap().path();
        for entry in load(path.to_str().unwrap()).as_array().unwrap() {
            let id = entry["id"].as_str().unwrap();
            assert!(!music.contains(id), "{id} is a music id and a word id");
        }
    }
}

/// Whether a fragment keeps every pitch in the set on its root.
fn in_set(fragment: &Fragment, offsets: &[u8]) -> bool {
    let root = fragment.root.unwrap_or(0);
    fragment
        .notes
        .iter()
        .all(|note| offsets.contains(&((note.pitch + 12 - root % 12) % 12)))
}

#[test]
fn examples_meet_and_break_the_rules_whose_check_is_simple() {
    for entry in &dictionary().unwrap().entries {
        let (Some(pass), Some(fail)) = (&entry.example_pass, &entry.example_fail) else {
            continue;
        };
        let meets = |fragment: &Fragment| match &entry.rule {
            Rule::PitchSet { offsets, .. } => Some(in_set(fragment, offsets)),
            Rule::ChordTones {
                offsets,
                choose: None,
            } => {
                let classes: BTreeSet<u8> = offsets.iter().map(|offset| offset % 12).collect();
                let root = fragment.root.unwrap_or(0);
                let sounded: BTreeSet<u8> = fragment
                    .notes
                    .iter()
                    .map(|note| (note.pitch + 12 - root % 12) % 12)
                    .collect();
                Some(sounded == classes)
            }
            Rule::Velocity { min, max } => Some(
                fragment
                    .notes
                    .iter()
                    .all(|note| (*min..=*max).contains(&note.vel)),
            ),
            Rule::Register { hard, .. } => Some(
                fragment
                    .notes
                    .iter()
                    .all(|note| (hard[0]..=hard[1]).contains(&note.pitch)),
            ),
            _ => None,
        };
        if let Some(true) = meets(fail)
            && !matches!(entry.rule, Rule::Register { .. })
        {
            panic!("{}: example_fail meets its rule", entry.id);
        }
        assert_ne!(
            meets(pass),
            Some(false),
            "{}: example_pass breaks its rule",
            entry.id
        );
    }
}

#[test]
fn the_oracle_script_reads_no_file() {
    let script = fs::read_to_string("xtask/golden/music_oracle.py").unwrap();
    for call in [
        "open(",
        "pathlib",
        "import os",
        "os.",
        "sys.argv",
        "__file__",
        "glob",
        "read_text",
        "listdir",
        "scandir",
        "shutil",
    ] {
        assert!(!script.contains(call), "music_oracle.py uses `{call}`");
    }
}

/// The committed files that could hold an oracle row.
fn committed_texts() -> Vec<(String, String)> {
    let mut texts = Vec::new();
    for dir in ["dictionary/music", "reference/core"] {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            texts.push((
                path.display().to_string(),
                fs::read_to_string(&path).unwrap(),
            ));
        }
    }
    texts
}

#[test]
fn no_committed_file_holds_an_oracle_row() {
    let mut parser = chordparser::parsing::Parser::new();
    let texts = committed_texts();
    for (symbol, _) in spellings() {
        let Ok(chord) = parser.parse(&symbol) else {
            continue;
        };
        let notes: Vec<String> = chord.note_literals().collect();
        if notes.len() < 3 {
            continue;
        }
        for row in [notes.join(" "), notes.join(", "), chord.to_json()] {
            for (path, text) in &texts {
                assert!(!text.contains(&row), "{path} holds the oracle row `{row}`");
            }
        }
    }
}

#[test]
fn the_python_oracles_cover_every_spelling_and_differ_only_where_pinned() {
    let vector = load("tests/fixtures/golden/music-oracles.json");
    let symbols: BTreeSet<String> = spellings().into_iter().map(|(symbol, _)| symbol).collect();
    let pinned = [
        ("pychord", &["C6/9", "Cm6/9", "C13", "Cmaj13", "CM13"][..]),
        (
            "music21",
            &[
                "C-", "C-7", "C-9", "C-7b5", "C+5", "C-6", "C69", "C-69", "C-11", "C13", "CM13",
                "Cm13", "C-13", "Cmaj#4",
            ][..],
        ),
    ];
    for (oracle, differ) in pinned {
        let lists = &vector["values"][oracle];
        let mut seen = BTreeSet::new();
        for verdict in ["agree", "differ", "unsupported"] {
            for symbol in lists[verdict].as_array().unwrap() {
                let symbol = symbol.as_str().expect("only symbols, never an oracle row");
                assert!(seen.insert(symbol.to_owned()), "{oracle}: {symbol} twice");
            }
        }
        assert_eq!(
            seen, symbols,
            "{oracle}: the chord spellings changed; run `cargo xtask golden`"
        );
        let found: BTreeSet<&str> = lists["differ"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(found, differ.iter().copied().collect(), "{oracle}");
    }
}

#[test]
fn every_roman_alias_parses_to_its_progression_roots() {
    let mut checked = 0;
    for entry in dictionary().unwrap().group(Group::Progression) {
        let Rule::Cycle { roots, .. } = &entry.rule else {
            panic!("{} has no cycle", entry.id)
        };
        for alias in &entry.aliases {
            let Ok(line) = parse_line(alias, None) else {
                continue;
            };
            if line.numerals.len() == roots.len() {
                let parsed: Vec<u8> = line.numerals.iter().map(|numeral| numeral.offset).collect();
                assert!(
                    &parsed == roots || relative_rotation(&parsed, roots),
                    "{}: {alias} gives {parsed:?}, not {roots:?}",
                    entry.id
                );
                checked += 1;
            }
        }
    }
    assert!(checked >= 15, "only {checked} aliases checked");
}

/// Whether `parsed` is the loop `roots` read from the relative key: the
/// same cycle, rotated, with every root a minor third or a major sixth off.
fn relative_rotation(parsed: &[u8], roots: &[u8]) -> bool {
    [3, 9].iter().any(|shift| {
        (0..roots.len()).any(|turn| {
            (0..roots.len()).all(|i| (parsed[i] + shift) % 12 == roots[(i + turn) % roots.len()])
        })
    })
}

/// One line per pinned value, so a changed number shows in the snapshot
/// diff.
fn pins(lines: impl Iterator<Item = String>) -> String {
    lines.collect::<Vec<_>>().join("\n")
}

#[test]
fn stat_values_are_pinned() {
    let dictionary = dictionary().unwrap();
    insta::assert_snapshot!(pins(dictionary.stats.iter().map(|stat| {
        format!(
            "{} {} {:?} {:?}",
            stat.id, stat.share, stat.count, stat.total
        )
    })));
}

#[test]
fn scale_offsets_are_pinned() {
    let dictionary = dictionary().unwrap();
    insta::assert_snapshot!(pins(dictionary.group(Group::Scale).filter_map(|entry| {
        match &entry.rule {
            Rule::PitchSet { offsets, .. } => Some(format!("{} {offsets:?}", entry.id)),
            _ => None,
        }
    })));
}

#[test]
fn onset_sets_and_patterns_are_pinned() {
    let dictionary = dictionary().unwrap();
    insta::assert_snapshot!(pins(dictionary.entries.iter().filter_map(|entry| {
        match &entry.rule {
            Rule::OnsetSet { parts } => {
                let parts: Vec<String> = parts
                    .iter()
                    .map(|part| format!("{} {:?}", part.part, part.slots))
                    .collect();
                Some(format!("{} {}", entry.id, parts.join("; ")))
            }
            Rule::Pattern { k, n, rotation } => {
                Some(format!("{} E({k},{n}) r{rotation}", entry.id))
            }
            _ => None,
        }
    })));
}

#[test]
fn tempos_and_progression_roots_are_pinned() {
    let dictionary = dictionary().unwrap();
    let tempos = dictionary.entries.iter().filter_map(|entry| {
        entry
            .bpm
            .map(|bpm| format!("{} {} {:?} {}", entry.id, bpm.low, bpm.center, bpm.high))
    });
    let roots = dictionary
        .entries
        .iter()
        .filter_map(|entry| match &entry.rule {
            Rule::Cycle { roots, chords, .. } => Some(format!("{} {roots:?} {chords:?}", entry.id)),
            _ => None,
        });
    insta::assert_snapshot!(pins(tempos.chain(roots)));
}
