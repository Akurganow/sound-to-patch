//! The committed `.vital` fixtures: each one reads and meets the format
//! contract, and each carries the numbers its generator wrote.

use sound_to_patch::preset::format::{PresetModel, RoutePolarity, Stored};
use sound_to_patch::preset::vital::{FORMAT, VitalVersion};

use crate::support::fixtures::{read, stamp};
use crate::support::format_contract;

const MINIMAL: &[u8] = include_bytes!("../fixtures/vital/minimal.vital");
const MARKS: &[u8] = include_bytes!("../fixtures/vital/marks.vital");
const ROUTES: &[u8] = include_bytes!("../fixtures/vital/routes.vital");
const STAMPS: [(&str, &[u8]); 4] = [
    (
        "1.0.6",
        include_bytes!("../fixtures/vital/stamps/1.0.6.vital"),
    ),
    (
        "1.0.9",
        include_bytes!("../fixtures/vital/stamps/1.0.9.vital"),
    ),
    (
        "1.1.0",
        include_bytes!("../fixtures/vital/stamps/1.1.0.vital"),
    ),
    (
        "1.5.5",
        include_bytes!("../fixtures/vital/stamps/1.5.5.vital"),
    ),
];

#[test]
fn every_fixture_meets_the_format_contract() {
    let mut samples = vec![MINIMAL, MARKS, ROUTES];
    samples.extend(STAMPS.map(|(_, bytes)| bytes));
    format_contract(&FORMAT, &samples).unwrap();
}

#[test]
fn the_stamps_pass_the_version_gate() {
    let app = |text: &str| text.parse::<VitalVersion>().unwrap();
    for (text, bytes) in STAMPS {
        let file = stamp(&read(bytes));
        assert_eq!(file.to_string(), text);
        assert_eq!(
            file.accepted_by(&app("1.0.6")),
            text.starts_with("1.0."),
            "{text}"
        );
        assert!(file.accepted_by(&app("1.5.5")), "{text}");
        assert_eq!(file.accepted_by(&app("1.1.0")), text != "1.5.5", "{text}");
    }
}

#[test]
fn the_routes_fixture_holds_its_generated_values() {
    let model = read(ROUTES);
    assert_eq!(stamp(&model).to_string(), "1.0.6");
    for (key, value) in [
        ("filter_1_on", 1.0),
        ("filter_1_cutoff", 72.0),
        ("filter_1_resonance", 0.3),
        ("osc_1_unison_voices", 3.0),
        ("env_2_decay", 0.5),
        ("reverb_on", 1.0),
    ] {
        assert_eq!(model.get(key), Some(Stored::Float(value)), "{key}");
    }
    let routes = model.routes();
    let taken: Vec<_> = routes
        .iter()
        .enumerate()
        .filter_map(|(slot, route)| route.as_ref().map(|route| (slot, route)))
        .map(|(slot, route)| {
            (
                slot,
                route.source.as_str(),
                route.destination.as_str(),
                route.amount,
            )
        })
        .collect();
    assert_eq!(
        taken,
        [
            (0, "lfo_1", "filter_1_cutoff", 0.25),
            (1, "env_2", "osc_1_level", 0.5),
            (2, "macro_control_1", "reverb_dry_wet", 0.75),
        ]
    );
    let power = |slot: usize| {
        routes[slot]
            .as_ref()
            .unwrap()
            .extras
            .iter()
            .find(|(key, _)| key.ends_with("_power"))
            .map(|(_, value)| value.clone())
    };
    assert_eq!(power(1), Some(Stored::Float(-2.0)));
    assert_eq!(routes[0].as_ref().unwrap().polarity, RoutePolarity::Bipolar);
}

#[test]
fn the_routes_wavetable_has_two_full_frames() {
    let model = read(ROUTES);
    let table = model.wavetable(0).unwrap();
    assert_eq!(
        (table.name.as_str(), table.author.as_str()),
        ("Parabola to Square", "sound-to-patch")
    );
    let frames: Vec<_> = table
        .keyframes
        .iter()
        .map(|frame| (frame.position, frame.samples.len()))
        .collect();
    assert_eq!(frames, [(0, 2048), (256, 2048)]);
    // 2048 float32 samples are 8192 bytes, and base64 writes each 3 bytes as
    // 4 characters with padding: 4 * ceil(8192 / 3).
    let text = String::from_utf8(ROUTES.to_vec()).unwrap();
    let lengths: Vec<usize> = text
        .split("\"wave_data\":\"")
        .skip(1)
        .map(|rest| rest.find('"').unwrap())
        .collect();
    assert_eq!(
        lengths.len(),
        2 + 1 + 1,
        "two keyframes of table 1, one of tables 2 and 3"
    );
    assert!(
        lengths
            .iter()
            .all(|&length| length == 4 * 8192_usize.div_ceil(3)),
        "{lengths:?}"
    );
}
