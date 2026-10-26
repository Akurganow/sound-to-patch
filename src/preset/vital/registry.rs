//! The Vital parameter registry, built from `registry.json`.
//!
//! The data file holds the control groups as facts in our own layout: one
//! row per control template with its range, default, engine scale, unit and
//! law, and the instance ids of each group. `cargo xtask vital-registry`
//! regenerates it from a checkout of the open source. This module expands
//! the groups to 794 keys and adds what is our judgment, not a fact: the
//! provenance of each law and the tuning notes. The file names the source
//! repository, its commit and its licence.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use serde::Deserialize;

use crate::preset::registry::{
    Build, Law, ParamKind, ParamSpec, Provenance, Registry, UnitKind, UnitLaw,
};
use crate::preset::vital::VitalVersion;
use crate::synth::{Synth, SynthError};

/// The engine transform of a stored value, applied after modulation adds
/// to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineScale {
    /// Integer steps: menus, switches, counts.
    Indexed,
    /// `x`.
    Linear,
    /// `x^2`.
    Quadratic,
    /// `x^3`.
    Cubic,
    /// `x^4`.
    Quartic,
    /// `sqrt(x)`, plus an offset.
    SquareRoot,
    /// `2^x`.
    Exponential,
}

/// Where the extracted facts come from.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    repository: String,
    commit: String,
    licence: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Data {
    source: Source,
    groups: Vec<Group>,
    defaults: BTreeMap<String, f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Group {
    prefix: String,
    label: String,
    ids: Vec<String>,
    #[serde(default)]
    later: Option<Later>,
    controls: Vec<Control>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Later {
    ids: Vec<String>,
    since: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Control {
    name: String,
    label: String,
    since: String,
    kind: ParamKind,
    min: f64,
    max: f64,
    default: f64,
    scale: EngineScale,
    #[serde(default)]
    unit: Option<UnitKind>,
    #[serde(default)]
    law: Option<Law>,
    #[serde(default)]
    choices: Vec<String>,
    #[serde(default)]
    legacy: Legacy,
}

#[derive(Default, Deserialize)]
#[serde(untagged)]
enum Legacy {
    #[default]
    None,
    All(bool),
    Ids(Vec<String>),
}

impl Legacy {
    fn covers(&self, id: &str) -> bool {
        match self {
            Self::None => false,
            Self::All(all) => *all,
            Self::Ids(ids) => ids.iter().any(|legacy| legacy == id),
        }
    }
}

/// The registry with the engine scale of each key.
#[derive(Debug)]
pub(crate) struct VitalRegistry {
    pub(crate) registry: Registry,
    pub(crate) scales: HashMap<String, EngineScale>,
}

const DATA: &str = include_str!("registry.json");

/// The Vital registry, built once.
pub(crate) fn vital_registry() -> Result<&'static VitalRegistry, SynthError> {
    static REGISTRY: OnceLock<Result<VitalRegistry, String>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| build(DATA))
        .as_ref()
        .map_err(|cause| SynthError::BrokenData {
            synth: Synth::Vital,
            cause: cause.clone(),
        })
}

fn build(text: &str) -> Result<VitalRegistry, String> {
    let data: Data =
        serde_json::from_str(text).map_err(|error| format!("registry.json: {error}"))?;
    let mut entries = Vec::new();
    let mut scales = HashMap::new();
    for group in &data.groups {
        for id in &group.ids {
            let later = group
                .later
                .as_ref()
                .filter(|later| later.ids.contains(id))
                .map(|later| later.since.as_str());
            for control in &group.controls {
                let key = if group.prefix.is_empty() {
                    control.name.clone()
                } else {
                    format!("{}_{id}_{}", group.prefix, control.name)
                };
                let label = if group.prefix.is_empty() {
                    control.label.clone()
                } else {
                    format!("{} {} {}", group.label, id.to_uppercase(), control.label)
                };
                let default = data.defaults.get(&key).copied().unwrap_or(control.default);
                let legacy = control.legacy.covers(id);
                let spec = spec(
                    key,
                    label,
                    control,
                    group,
                    later,
                    default,
                    legacy,
                    &data.source,
                )?;
                scales.insert(spec.key.clone(), control.scale);
                entries.push(spec);
            }
        }
    }
    if let Some(key) = data.defaults.keys().find(|key| !scales.contains_key(*key)) {
        return Err(format!("default override for unknown key {key:?}"));
    }
    let registry = Registry::new(Synth::Vital, entries)?;
    Ok(VitalRegistry { registry, scales })
}

#[expect(
    clippy::too_many_arguments,
    reason = "one call site passes the parts of one expanded key"
)]
fn spec(
    key: String,
    label: String,
    control: &Control,
    group: &Group,
    later: Option<&str>,
    default: f64,
    legacy: bool,
    source: &Source,
) -> Result<ParamSpec, String> {
    let law = match (control.unit, control.law.clone()) {
        (Some(unit), Some(law)) => Some(UnitLaw { unit, law }),
        (None, None) => None,
        _ => return Err(format!("{key}: unit and law must come together")),
    };
    let since = match later {
        Some(later) => newer(&control.since, later)?,
        None => control.since.clone(),
    };
    let note = tuning_note(&group.prefix, &control.name, control.kind, law.is_some());
    let mut evidence = format!(
        "Vital open source 1.0.6 ({} at {}, {}): src/common/synth_parameters.cpp",
        source.repository, source.commit, source.licence
    );
    if let Some(extra) = law.as_ref().and_then(law_evidence) {
        evidence.push_str("; ");
        evidence.push_str(extra);
    }
    if note.is_some() {
        evidence.push_str("; the audible effect is not measured");
    }
    Ok(ParamSpec {
        key,
        kind: control.kind,
        stored_min: control.min,
        stored_max: control.max,
        default,
        law,
        label,
        choices: control.choices.clone(),
        provenance: if note.is_some() {
            Provenance::Interim
        } else {
            Provenance::FromFacts
        },
        evidence,
        first_build: Some(Build(since)),
        verified_on: None,
        tuning_note: note.map(str::to_owned),
        legacy,
    })
}

/// The later of two build texts.
fn newer(a: &str, b: &str) -> Result<String, String> {
    let parse = |text: &str| {
        text.parse::<VitalVersion>()
            .map_err(|error| format!("build {text:?}: {error}"))
    };
    Ok(if parse(a)? >= parse(b)? { a } else { b }.to_owned())
}

/// The source files behind the laws that are not read off the table.
fn law_evidence(law: &UnitLaw) -> Option<&'static str> {
    match (&law.law, law.unit) {
        (Law::Exp2 { .. }, UnitKind::Hz) => Some(
            "note-number frequency law in src/synthesis/framework/utils.h and the rate law in synth_module.cpp",
        ),
        (Law::DbOfPower { .. }, _) => {
            Some("level squared at audio rate in src/synthesis/producers/synth_oscillator.cpp")
        }
        (Law::Power { exponent, .. }, UnitKind::Seconds)
            if (exponent - 4.0).abs() < f64::EPSILON =>
        {
            Some("envelope times in seconds in src/synthesis/modulators/envelope.cpp")
        }
        _ => None,
    }
}

const RESONANCE: &str = "The audible peak depends on the filter model and is not measured. Raise it while a note holds until the peak near the cutoff matches the stem; back off when it starts to whistle.";
const DRIVE: &str = "The drive response inside each filter model is not measured. Raise it until the added harmonics match the stem, then check the level.";
const FILTER_SHAPE: &str = "What this control does depends on the filter model and style. Sweep it slowly while a note holds and stop where the tone matches the stem.";
const DISTORTION: &str = "The distortion curve is not measured. Raise it from zero until the added harmonics match the stem on held notes.";
const COMPRESSOR: &str = "The compressor response is not measured. Compare the attack and the held level of the stem with the patch and set it by ear.";
const EQ: &str = "The EQ shape at this setting is not measured. Sweep it while a note plays and match the brightness of the stem.";
const FEEDBACK: &str = "How much each repeat or sweep keeps is not measured. Raise it until the tail or the ringing of the effect matches the stem.";
const DEPTH: &str =
    "The depth of the sweep is not measured. Raise it until the movement matches the stem.";
const MIX: &str = "The balance law between the dry and the wet sound is not measured. Raise it until the effect is as loud as in the stem.";
const REVERB: &str = "The size, tone and motion of the reverb are not measured. Match the length and the colour of the tail by ear.";
const WARP: &str = "The sound change of this oscillator warp is not measured. Sweep it while a note holds and stop where the timbre matches the stem.";
const CURVE: &str = "The value bends a curve; negative and positive values bend opposite ways. Match the shape of the stem by ear.";
const GENERIC: &str = "No physical unit is claimed for this control. Sweep it while a note holds and stop where the sound matches the stem.";

/// The tuning note of an interim control, or `None` for a law from facts.
///
/// Menus, switches and counts are facts. A real-valued control is interim
/// when it claims no unit, or when its unit is a display scale whose
/// audible effect no source states.
fn tuning_note(prefix: &str, name: &str, kind: ParamKind, has_unit: bool) -> Option<&'static str> {
    if kind != ParamKind::Float {
        return None;
    }
    let family = match (prefix, name) {
        (_, name)
            if name.starts_with("eq_")
                && (name.ends_with("_gain") || name.ends_with("_resonance")) =>
        {
            Some(EQ)
        }
        ("filter", "resonance") | ("", "distortion_filter_resonance") => Some(RESONANCE),
        ("filter", "drive") => Some(DRIVE),
        ("filter", name) if name.starts_with("blend") || name.starts_with("formant_") => {
            Some(FILTER_SHAPE)
        }
        ("filter", "mix") => Some(MIX),
        ("", "distortion_drive" | "distortion_mix" | "distortion_filter_blend") => Some(DISTORTION),
        ("", name) if name.starts_with("compressor_") => Some(COMPRESSOR),
        (
            "",
            "reverb_size"
            | "reverb_chorus_amount"
            | "reverb_high_shelf_gain"
            | "reverb_low_shelf_gain",
        ) => Some(REVERB),
        (_, name) if name.ends_with("_feedback") => Some(FEEDBACK),
        (_, name) if name.ends_with("_mod_depth") => Some(DEPTH),
        (_, name) if name.ends_with("_dry_wet") => Some(MIX),
        (
            "osc",
            "distortion_amount"
            | "distortion_spread"
            | "spectral_morph_amount"
            | "spectral_morph_spread",
        ) => Some(WARP),
        _ => None,
    };
    family.or_else(|| match (has_unit, name.ends_with("_power")) {
        (true, _) => None,
        (false, true) => Some(CURVE),
        (false, false) => Some(GENERIC),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::registry::{ParamLaw, ParamRegistry, Physical};
    use approx::assert_relative_eq;

    fn vital() -> &'static VitalRegistry {
        vital_registry().unwrap()
    }

    fn spec(key: &str) -> &'static ParamSpec {
        vital().registry.get(key).unwrap()
    }

    /// The physical value of a stored value, as a plain number.
    fn physical(key: &str, stored: f64) -> f64 {
        spec(key).physical(stored).unwrap().get()
    }

    #[test]
    fn the_table_has_794_keys_and_22_legacy() {
        let registry = &vital().registry;
        assert_eq!(registry.entries().len(), 794);
        let legacy: Vec<_> = registry
            .iter()
            .filter(|spec| spec.legacy)
            .map(|spec| spec.key.as_str())
            .collect();
        assert_eq!(legacy.len(), 22, "{legacy:?}");
        for key in [
            "sub_direct_out",
            "sub_waveform",
            "filter_1_osc1_input",
            "filter_fx_sample_input",
            "filter_fx_filter_input",
            "compressor_low_band_unused",
        ] {
            assert!(spec(key).legacy, "{key}");
        }
        assert!(!spec("filter_1_filter_input").legacy);
    }

    #[test]
    fn group_and_scale_counts_match_the_source() {
        let count = |prefix: &str| {
            vital()
                .registry
                .iter()
                .filter(|spec| spec.key.starts_with(prefix))
                .count()
        };
        assert_eq!(count("env_"), 54);
        assert_eq!(count("lfo_"), 96);
        assert_eq!(count("random_"), 32);
        assert_eq!(count("osc_"), 87);
        assert_eq!(count("filter_1_") + count("filter_2_"), 40);
        assert_eq!(count("filter_fx_"), 20);
        assert_eq!(count("modulation_"), 320);
        let mut scales: HashMap<EngineScale, usize> = HashMap::new();
        for scale in vital().scales.values() {
            *scales.entry(*scale).or_default() += 1;
        }
        assert_eq!(scales.get(&EngineScale::Indexed), Some(&370));
        assert_eq!(scales.get(&EngineScale::Linear), Some(&351));
        assert_eq!(scales.get(&EngineScale::Quartic), Some(&30));
        assert_eq!(scales.get(&EngineScale::Exponential), Some(&30));
        assert_eq!(scales.get(&EngineScale::Quadratic), Some(&12));
        assert_eq!(scales.get(&EngineScale::SquareRoot), Some(&1));
        assert_eq!(scales.get(&EngineScale::Cubic), None);
    }

    #[test]
    fn later_instances_carry_their_first_build() {
        let build = |key: &str| spec(key).first_build.clone().unwrap().0;
        assert_eq!(build("osc_3_level"), "0.5.0");
        assert_eq!(build("osc_1_level"), "0.0.0");
        assert_eq!(build("modulation_33_amount"), "0.6.1");
        assert_eq!(build("modulation_32_amount"), "0.0.0");
    }

    #[test]
    fn overridden_defaults_hold() {
        assert_eq!(spec("osc_1_on").default, 1.0);
        assert_eq!(spec("osc_2_on").default, 0.0);
        assert_eq!(spec("osc_2_destination").default, 1.0);
        assert_eq!(spec("osc_3_destination").default, 3.0);
    }

    // Golden values: each expected number is computed from the formula
    // beside it, as the facts state the law.

    #[test]
    fn cutoff_follows_the_note_law() {
        // Hz = 8.1757989156 * 2^(stored / 12)
        for (stored, hz) in [
            (8.0, 8.175_798_915_6 * 2f64.powf(8.0 / 12.0)),
            (36.0, 8.175_798_915_6 * 8.0),
            (60.0, 8.175_798_915_6 * 32.0),
            (84.0, 8.175_798_915_6 * 128.0),
            (100.0, 8.175_798_915_6 * 2f64.powf(100.0 / 12.0)),
            (136.0, 8.175_798_915_6 * 2f64.powf(136.0 / 12.0)),
        ] {
            for key in ["filter_1_cutoff", "filter_2_cutoff", "filter_fx_cutoff"] {
                assert_relative_eq!(
                    physical(key, stored),
                    hz,
                    epsilon = 0.0,
                    max_relative = 1e-6
                );
            }
        }
        // The facts round these to 12.98, 65.4, 261.63, 1046.5, 2637, 21.1k.
        assert_relative_eq!(physical("filter_1_cutoff", 8.0), 12.98, max_relative = 1e-3);
        assert_relative_eq!(physical("filter_1_cutoff", 60.0), 261.63, epsilon = 0.005);
        assert_relative_eq!(
            physical("filter_1_cutoff", 136.0),
            21_096.0,
            max_relative = 1e-3
        );
        for key in [
            "delay_filter_cutoff",
            "distortion_filter_cutoff",
            "chorus_cutoff",
            "phaser_center",
            "flanger_center",
            "eq_low_cutoff",
            "eq_band_cutoff",
            "eq_high_cutoff",
            "reverb_pre_low_cutoff",
            "reverb_high_shelf_cutoff",
        ] {
            assert_eq!(spec(key).unit(), Some(UnitKind::Hz), "{key}");
            assert_relative_eq!(physical(key, 60.0), 261.625_565_3, max_relative = 1e-6);
        }
    }

    #[test]
    fn envelope_times_are_fourth_powers() {
        // seconds = stored^4
        assert_relative_eq!(
            physical("env_1_attack", 2.378_42),
            2.378_42f64.powi(4),
            max_relative = 1e-9
        );
        assert_relative_eq!(physical("env_3_decay", 2.378_42), 32.0, max_relative = 1e-5);
        assert_relative_eq!(physical("env_6_release", 1.0), 1.0, max_relative = 1e-12);
        assert_relative_eq!(
            physical("env_2_hold", spec("env_2_hold").stored_max),
            4.0,
            max_relative = 1e-9
        );
        assert_relative_eq!(
            physical("env_1_delay", std::f64::consts::SQRT_2),
            4.0,
            max_relative = 1e-9
        );
    }

    #[test]
    fn rates_and_times_are_powers_of_two() {
        // Hz = 2^stored; seconds = 2^stored
        assert_relative_eq!(
            physical("lfo_1_frequency", -7.0),
            0.007_812_5,
            max_relative = 1e-12
        );
        assert_relative_eq!(
            physical("lfo_8_frequency", 9.0),
            512.0,
            max_relative = 1e-12
        );
        assert_relative_eq!(
            physical("random_4_frequency", 1.0),
            2.0,
            max_relative = 1e-12
        );
        for key in [
            "delay_frequency",
            "phaser_frequency",
            "flanger_frequency",
            "chorus_frequency",
        ] {
            assert_eq!(spec(key).unit(), Some(UnitKind::Hz), "{key}");
            assert_relative_eq!(physical(key, 0.0), 1.0, max_relative = 1e-12);
        }
        assert_relative_eq!(
            physical("portamento_time", -10.0),
            2f64.powi(-10),
            max_relative = 1e-12
        );
        assert_relative_eq!(
            physical("reverb_decay_time", 0.0),
            1.0,
            max_relative = 1e-12
        );
        assert_eq!(spec("chorus_delay_1").unit(), Some(UnitKind::Seconds));
        assert_relative_eq!(
            physical("chorus_delay_1", -9.0),
            2f64.powi(-9),
            max_relative = 1e-12
        );
    }

    #[test]
    fn levels_pans_tunes_and_detune_follow_their_laws() {
        // dB = 40 * log10(stored)
        let default = spec("osc_1_level").default;
        assert_relative_eq!(
            physical("osc_1_level", default),
            40.0 * default.log10(),
            max_relative = 1e-9
        );
        assert_relative_eq!(
            physical("osc_1_level", default),
            -6.0206,
            max_relative = 1e-4
        );
        assert_relative_eq!(physical("sample_level", 1.0), 0.0, epsilon = 1e-12);
        // dB = sqrt(stored) - 80
        assert_relative_eq!(
            physical("volume", 5_473.040_4),
            5_473.040_4f64.sqrt() - 80.0,
            max_relative = 1e-9
        );
        assert_relative_eq!(physical("volume", 5_473.040_4), -6.02, max_relative = 1e-3);
        assert_relative_eq!(physical("volume", 7_399.440_4), 6.02, max_relative = 1e-3);
        // cents = 100 * stored; percent = 100 * stored
        assert_relative_eq!(physical("osc_2_tune", 0.5), 50.0, max_relative = 1e-12);
        assert_eq!(spec("osc_2_tune").unit(), Some(UnitKind::Cents));
        assert_relative_eq!(physical("osc_1_pan", -0.5), -50.0, max_relative = 1e-12);
        assert_eq!(spec("osc_1_pan").unit(), Some(UnitKind::PercentBipolar));
        assert_relative_eq!(
            physical("filter_1_keytrack", 1.0),
            100.0,
            max_relative = 1e-12
        );
        // percent = stored^2
        assert_relative_eq!(
            physical("osc_1_unison_detune", 4.472_135_955),
            20.0,
            max_relative = 1e-9
        );
        assert_relative_eq!(
            physical("osc_1_unison_detune", 10.0),
            100.0,
            max_relative = 1e-12
        );
        // semitones = stored, whole steps
        assert_eq!(spec("osc_1_transpose").kind, ParamKind::Count);
        assert_relative_eq!(
            physical("osc_1_transpose", -48.0),
            -48.0,
            max_relative = 1e-12
        );
    }

    #[test]
    fn every_law_round_trips_at_its_range_and_default() {
        for spec in vital().registry.iter() {
            let Some(unit_law) = &spec.law else { continue };
            let middle = f64::midpoint(spec.stored_min, spec.stored_max);
            for stored in [spec.stored_min, spec.default, middle, spec.stored_max] {
                let Ok(value) = unit_law.law.to_physical(stored) else {
                    // Only a zero gain has no decibel value.
                    assert_eq!(
                        (stored, &unit_law.law),
                        (0.0, &Law::DbOfPower { exponent: 2.0 }),
                        "{}",
                        spec.key
                    );
                    continue;
                };
                let back = unit_law.law.to_stored(value).unwrap();
                assert!(
                    (back - stored).abs() <= 1e-9 * stored.abs().max(1e-9),
                    "{}: {stored} came back as {back}",
                    spec.key
                );
            }
        }
    }

    #[test]
    fn interim_entries_are_listed_with_notes() {
        let interim: Vec<_> = vital()
            .registry
            .iter()
            .filter(|spec| spec.provenance == Provenance::Interim)
            .collect();
        for key in [
            "filter_1_resonance",
            "filter_2_resonance",
            "filter_fx_resonance",
        ] {
            assert!(interim.iter().any(|spec| spec.key == key), "{key}");
            assert_eq!(spec(key).tuning_note.as_deref(), Some(RESONANCE));
        }
        for spec in &interim {
            assert!(spec.tuning_note.is_some(), "{}", spec.key);
        }
        for key in [
            "filter_1_drive",
            "distortion_drive",
            "eq_low_gain",
            "compressor_band_gain",
            "delay_feedback",
            "reverb_size",
            "env_1_sustain",
        ] {
            assert_eq!(spec(key).provenance, Provenance::Interim, "{key}");
        }
        for key in [
            "filter_1_cutoff",
            "env_1_attack",
            "lfo_1_frequency",
            "osc_1_level",
            "volume",
            "osc_1_tune",
            "filter_1_model",
            "osc_1_unison_voices",
        ] {
            assert_eq!(spec(key).provenance, Provenance::FromFacts, "{key}");
        }
    }

    #[test]
    fn choices_cover_their_range() {
        assert_eq!(spec("filter_1_model").choices.len(), 8);
        assert_eq!(spec("filter_1_model").choices[0], "Analog");
        assert_eq!(spec("filter_1_on").kind, ParamKind::Flag);
        let tempo = spec("delay_tempo");
        assert_eq!(tempo.kind, ParamKind::Choice);
        assert_eq!(tempo.stored_min, 4.0);
        assert_eq!(tempo.choices.first().map(String::as_str), Some("4/1"));
    }

    #[test]
    fn hz_edits_reach_stored_cutoffs_exactly() {
        let cutoff = spec("filter_1_cutoff");
        let stored = cutoff
            .stored(&Physical::parse(UnitKind::Hz, "1046.502261 Hz").unwrap())
            .unwrap();
        assert_relative_eq!(stored, 84.0, max_relative = 1e-8);
        let message = cutoff
            .stored(&Physical::parse(UnitKind::Hz, "5 Hz").unwrap())
            .unwrap_err()
            .to_string();
        assert!(
            message.starts_with("filter_1_cutoff: 5 Hz is stored as"),
            "{message}"
        );
    }

    #[test]
    fn broken_data_is_an_error() {
        assert!(build("{}").is_err());
        assert!(build(r#"{"source":{"repository":"r","commit":"c","licence":"l"},"groups":[],"defaults":{"nope":1.0}}"#).is_err());
    }

    #[test]
    fn newer_compares_builds() {
        assert_eq!(newer("0.0.0", "0.5.0").unwrap(), "0.5.0");
        assert_eq!(newer("0.7.4", "0.5.0").unwrap(), "0.7.4");
        assert!(newer("x", "0.5.0").is_err());
    }
}
