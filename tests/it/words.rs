//! The word dictionary: coverage of the ranked and draft words, the
//! senses and their questions, and the listener numbers.

use std::collections::BTreeSet;
use std::fs;

use serde_json::Value;
use sound_to_patch::words::{Class, Words, dictionary};

fn words() -> &'static Words {
    dictionary().unwrap()
}

/// Every lead word and alias of the dictionary.
fn names() -> BTreeSet<String> {
    words()
        .entries
        .iter()
        .flat_map(|entry| std::iter::once(&entry.word).chain(&entry.aliases))
        .cloned()
        .collect()
}

/// Tiers W1 and W2 of the published ranked list (ranks 1 to 60).
const PUBLISHED: [&str; 60] = [
    "simple",
    "analog",
    "digital",
    "dark",
    "soft",
    "deep",
    "fast",
    "short",
    "distorted",
    "slow",
    "groovy",
    "hard",
    "weird",
    "punchy",
    "big",
    "lofi",
    "shimmery",
    "glitchy",
    "long",
    "rhythmic",
    "energetic",
    "wide",
    "alien",
    "cinematic",
    "bright",
    "retro",
    "happy",
    "heavy",
    "experimental",
    "strange",
    "fun",
    "dirty",
    "mellow",
    "robotic",
    "classic",
    "loud",
    "glassy",
    "noisy",
    "scary",
    "funky",
    "suspenseful",
    "buzzy",
    "fat",
    "full",
    "vintage",
    "sweet",
    "emotional",
    "boomy",
    "upbeat",
    "repetitive",
    "bouncy",
    "smooth",
    "metallic",
    "warm",
    "futuristic",
    "sad",
    "chill",
    "creepy",
    "aggressive",
    "percussive",
];

/// The words of the draft rows (tier W3), each named in a task draft of
/// issue 101.
const DRAFT: [&str; 62] = [
    "bright",
    "dark",
    "dull",
    "muffled",
    "warm",
    "cold",
    "cool",
    "mellow",
    "round",
    "crisp",
    "sharp",
    "harsh",
    "tinny",
    "thin",
    "airy",
    "glassy",
    "silky",
    "present",
    "fat",
    "thick",
    "full",
    "deep",
    "heavy",
    "huge",
    "big",
    "boomy",
    "muddy",
    "tight",
    "punchy",
    "gritty",
    "crunchy",
    "distorted",
    "clean",
    "clear",
    "smooth",
    "aggressive",
    "noisy",
    "lo-fi",
    "vintage",
    "analog",
    "digital",
    "buzzy",
    "hollow",
    "nasal",
    "woody",
    "metallic",
    "vowel",
    "resonant",
    "pure",
    "subby",
    "wide",
    "spacious",
    "wet",
    "dry",
    "distant",
    "echo",
    "wobble",
    "evolving",
    "plucky",
    "bouncy",
    "percussive",
    "sustained",
];

/// The words with incompatible senses.
const MULTI: [&str; 11] = [
    "hollow", "warm", "punchy", "thin", "wobble", "hard", "big", "clean", "airy", "deep", "fat",
];

#[test]
fn the_dictionary_loads_and_meets_every_rule() {
    if let Err(error) = dictionary() {
        panic!("{error}");
    }
}

#[test]
fn every_ranked_word_of_tiers_w1_and_w2_has_an_entry() {
    let names = names();
    let mut tiered: BTreeSet<String> = PUBLISHED.iter().map(|word| (*word).to_owned()).collect();
    let ranks: Value =
        serde_json::from_str(&fs::read_to_string("dictionary/words/ranks.json").unwrap()).unwrap();
    for row in ranks["describing"].as_array().unwrap() {
        if row["tier"].is_string() {
            tiered.insert(row["word"].as_str().unwrap().to_owned());
        }
    }
    let gaps: Vec<&String> = tiered
        .iter()
        .filter(|word| !names.contains(*word))
        .collect();
    assert!(gaps.is_empty(), "ranked words without an entry: {gaps:?}");
}

#[test]
fn every_draft_word_has_an_entry() {
    let names = names();
    let gaps: Vec<&&str> = DRAFT
        .iter()
        .filter(|word| !names.contains(**word))
        .collect();
    assert!(gaps.is_empty(), "draft words without an entry: {gaps:?}");
}

#[test]
fn each_multi_meaning_word_has_several_senses_and_one_default() {
    for word in MULTI {
        let senses = words().senses(word);
        assert!(senses.len() >= 2, "{word} has {} sense", senses.len());
        assert_eq!(
            senses.iter().filter(|entry| entry.default).count(),
            1,
            "{word}"
        );
        assert!(words().question(word).is_some(), "{word} has no question");
    }
}

#[test]
fn the_questions_of_hollow_warm_and_hard() {
    let questions: Vec<String> = ["hollow", "warm", "hard"]
        .into_iter()
        .map(|word| words().question(word).unwrap())
        .collect();
    insta::assert_snapshot!(questions.join("\n"));
}

/// Issue 270 asked for four words; only three have a role a source
/// supports (big, thin, wobble), so the floor is three.
#[test]
fn at_least_three_multi_meaning_words_pick_a_sense_by_role_and_by_default() {
    let by_role: Vec<&str> = MULTI
        .into_iter()
        .filter(|word| {
            words().senses(word).iter().any(|entry| {
                !entry.default
                    && entry.roles.iter().any(|role| {
                        words().pick(word, Some(role)).map(|picked| &picked.id) == Some(&entry.id)
                    })
            })
        })
        .collect();
    assert!(by_role.len() >= 3, "only {by_role:?} pick a sense by role");
    assert!(words().pick("punchy", Some("drums")).unwrap().default);
}

#[test]
fn a_warm_bass_takes_a_sense_that_lowers_the_brightness() {
    // The worked example "warm bass" of issue 101, a test case of the text
    // path: the bass role picks a warm sense the analyzer checks by the
    // centroid relative to the fundamental going down.
    let warm = words().pick("warm", Some("bass")).unwrap();
    let measure = warm.measure.as_ref().unwrap();
    assert_eq!(
        (measure.key.as_str(), measure.direction.as_str()),
        ("centroid_over_f0", "down"),
        "{}",
        warm.id
    );
    assert_eq!(warm.agreement.class, Class::Medium);
}

#[test]
fn wide_lengthens_the_decay_and_sets_no_stereo_width() {
    let wide = words().pick("wide", None).unwrap();
    let reverb = wide.reverb.unwrap();
    assert!(reverb.effects.decay > 0.1, "{reverb:?}");
    assert!(reverb.effects.stereo_spread.abs() < 0.05, "{reverb:?}");
}

#[test]
fn the_lo_fi_chain_keeps_its_order() {
    let meaning = &words().pick("lo-fi", None).unwrap().meaning;
    let at = |part: &str| {
        meaning
            .find(part)
            .unwrap_or_else(|| panic!("`{part}` missing from: {meaning}"))
    };
    let order = [
        at("muted"),
        at("low-pass"),
        at("saturation"),
        at("wow"),
        at("noise"),
    ];
    assert!(order.is_sorted(), "{meaning}");
}

#[test]
fn the_listener_numbers_reproduce_the_draft_table() {
    // The draft of issue 101 listed these words with n of 8 or more.
    let table: [(&str, u32, f64, Class); 22] = [
        ("tinny", 8, 0.82, Class::High),
        ("dark", 8, 0.57, Class::High),
        ("sharp", 8, 0.50, Class::High),
        ("crisp", 8, 0.47, Class::High),
        ("heavy", 15, 0.46, Class::High),
        ("loud", 26, 0.40, Class::Medium),
        ("bright", 19, 0.34, Class::Medium),
        ("mellow", 8, 0.34, Class::Medium),
        ("hard", 10, 0.32, Class::Medium),
        ("sad", 9, 0.29, Class::Medium),
        ("calm", 13, 0.27, Class::Medium),
        ("warm", 64, 0.25, Class::Medium),
        ("soothing", 17, 0.24, Class::Medium),
        ("smooth", 14, 0.22, Class::Medium),
        ("clear", 12, 0.22, Class::Medium),
        ("soft", 29, 0.20, Class::Medium),
        ("hot", 9, 0.18, Class::Low),
        ("gentle", 8, 0.10, Class::Low),
        ("harsh", 16, 0.08, Class::Low),
        ("happy", 22, 0.08, Class::Low),
        ("cold", 34, 0.05, Class::Low),
        ("cool", 15, -0.03, Class::Low),
    ];
    for (word, n, value, class) in table {
        let row = words().eq(word).unwrap();
        assert_eq!(
            (row.n, row.value, row.class),
            (n, Some(value), class),
            "{word}"
        );
    }
    let wide = words().reverb("wide").unwrap();
    assert_eq!((wide.n, wide.spread_ratio), (62, 0.62));
    assert_eq!(words().listeners.reverb_distinct_settings, 256);
}

#[test]
fn a_listener_row_does_not_fit_the_agreement_schema() {
    let schema: Value =
        serde_json::from_str(&fs::read_to_string("schemas/agreement.schema.json").unwrap())
            .unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let mut file: Value =
        serde_json::from_str(&fs::read_to_string("dictionary/words/agreement.json").unwrap())
            .unwrap();
    assert!(validator.is_valid(&file));
    file["eq"][0]["param_values"] = Value::from(vec![0.1; 40]);
    assert!(!validator.is_valid(&file));
}

#[test]
fn style_and_technique_words_match_the_lexicon_groups() {
    let lexicon: Value =
        serde_json::from_str(&fs::read_to_string("dictionary/words/lexicon.json").unwrap())
            .unwrap();
    for (key, list) in [("style", &words().style), ("technique", &words().technique)] {
        let groups = lexicon[key].as_array().unwrap();
        assert_eq!(groups.len(), list.len(), "{key}");
        assert_eq!(list.len(), 26, "{key}");
        for (group, row) in groups.iter().zip(list) {
            let label = group["label"].as_str().unwrap();
            let first = |text: &str| text.split([' ', '/']).next().unwrap_or("").to_owned();
            assert_eq!(
                first(label),
                first(&row.word),
                "{key} {}: {label}",
                row.word
            );
        }
    }
}

#[test]
fn every_ranked_input_names_a_counts_only_source() {
    let sources: Value =
        serde_json::from_str(&fs::read_to_string("dictionary/sources.json").unwrap()).unwrap();
    let ranks: Value =
        serde_json::from_str(&fs::read_to_string("dictionary/words/ranks.json").unwrap()).unwrap();
    for input in ranks["inputs"].as_array().unwrap() {
        let id = input["source"].as_str().unwrap();
        let source = sources
            .as_array()
            .unwrap()
            .iter()
            .find(|source| source["id"] == id)
            .unwrap_or_else(|| panic!("{id} is not in dictionary/sources.json"));
        assert_eq!(source["use"], "counts only", "{id}");
        let sha = input["sha256"].as_str().unwrap();
        assert!(
            source["revision"].as_str().unwrap().contains(sha),
            "{id}: the pin {sha} is not in its revision field"
        );
    }
}
