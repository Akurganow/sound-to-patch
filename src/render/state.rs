//! The plug-in state of a preset, and the check of a state the plug-in
//! saved back.
//!
//! Vital's state is the preset JSON as text with a trailing NUL, the form
//! the plug-in saves itself. The builder forces the recipe's oversampling
//! into the controls, because oversampling changes the sound.
//!
//! The builder also gives each state a tuning: the preset's own, or
//! Vital's default. The plug-in keeps its current tuning when a state has
//! none, so one host session would play the next preset in the last tuning.
//!
//! A host call reports success even when the plug-in kept its old sound,
//! so a backend reads the state back and compares it with the sent one:
//! the name, the tuning and every control. The plug-in keeps controls as
//! 32-bit floats, so a number matches within one `f32` step. Structured
//! parts such as the sample, the wavetables and the routes are not
//! compared: Vital 1.0.7 re-encodes the sample and restamps the wavetables
//! when it saves.

use std::fmt;

use serde_json::{Map, Value};

use super::{PresetState, Recipe, RenderError};
use crate::cast;
use crate::preset::format::{FormatError, PresetFormat, Stored};
use crate::preset::vital;
use crate::synth::{Synth, SynthError};
use crate::units::Bpm;

/// The Vital control that sets oversampling.
const OVERSAMPLING: &str = "oversampling";
/// How many `{` bytes [`saved_preset`] tries as the start of the JSON.
const MAX_STARTS: usize = 64;
/// The top-level key of the tuning in a Vital state.
const TUNING: &str = "tuning";
/// The tempo control of a Vital preset, in beats per second despite its
/// name.
const TEMPO: &str = "beats_per_minute";
/// The path of the tempo control in a [`Mismatch`].
pub const TEMPO_PATH: &str = "settings.beats_per_minute";
/// The highest MIDI note.
const MAX_MIDI_NOTE: u64 = 127;
/// How many characters of a value a mismatch message shows.
const SHOWN_CHARS: usize = 40;

/// Builds the plug-in state of `preset`, a preset file of `synth`.
///
/// # Errors
///
/// Returns [`RenderError::Synth`] for a synth without a renderer and
/// [`RenderError::Preset`] for a file that is not a valid preset.
pub fn preset_state(
    synth: Synth,
    preset: &[u8],
    recipe: &Recipe,
) -> Result<PresetState, RenderError> {
    match synth {
        Synth::Vital => vital_state(preset, recipe),
        Synth::Serum2 => Err(RenderError::Synth(SynthError::NotBuilt(synth))),
    }
}

fn vital_state(preset: &[u8], recipe: &Recipe) -> Result<PresetState, RenderError> {
    let mut model = vital::FORMAT.read(preset).map_err(RenderError::Preset)?;
    model
        .set(
            OVERSAMPLING,
            Stored::Float(recipe.vital_oversampling().stored()),
        )
        .map_err(RenderError::Preset)?;
    let bytes = vital::FORMAT
        .write(model.as_ref())
        .map_err(RenderError::Preset)?;
    let mut root: Map<String, Value> =
        serde_json::from_slice(&bytes).map_err(|source| RenderError::State(source.to_string()))?;
    match root.get_mut(TUNING) {
        Some(tuning) => {
            check_tuning(tuning)?;
            // The plug-in saves no empty mapping, so the read-back would
            // miss it.
            if let Some(tuning) = tuning.as_object_mut()
                && tuning
                    .get("mapping")
                    .and_then(Value::as_array)
                    .is_some_and(Vec::is_empty)
            {
                tuning.remove("mapping");
            }
        }
        None => {
            root.insert(TUNING.to_owned(), default_tuning());
        }
    }
    let mut bytes =
        serde_json::to_vec(&root).map_err(|source| RenderError::State(source.to_string()))?;
    bytes.push(0);
    Ok(PresetState::new(Synth::Vital, bytes))
}

/// Vital's default tuning as the plug-in saves it: twelve equal steps an
/// octave, the scale starting at MIDI 60 (`Tuning::stateToJson`, source
/// 1.0.6).
fn default_tuning() -> Value {
    serde_json::json!({
        "scale_start_midi_note": 60,
        "reference_midi_note": 0.0,
        "tuning_name": "",
        "mapping_name": "",
        "default": true,
        "scale": [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0],
    })
}

/// Checks a preset's tuning against what `Tuning::jsonToState` reads. The
/// plug-in answers a missing key or a wrong type with a dialog box, and an
/// out-of-range note or mapping with a read out of bounds, so such a
/// tuning never reaches it.
fn check_tuning(tuning: &Value) -> Result<(), RenderError> {
    let fail = |cause: String| {
        Err(RenderError::Preset(FormatError::new(
            "cannot render the .vital preset",
            cause,
            "Remove `tuning` to play in the default tuning, or load the tuning in Vital and save the preset again",
        )))
    };
    let Value::Object(tuning) = tuning else {
        return fail("`tuning` is not a JSON object".to_owned());
    };
    if tuning
        .get("scale_start_midi_note")
        .and_then(Value::as_u64)
        .is_none_or(|note| note > MAX_MIDI_NOTE)
    {
        return fail("`tuning.scale_start_midi_note` is not a MIDI note from 0 to 127".to_owned());
    }
    if !tuning
        .get("reference_midi_note")
        .is_some_and(Value::is_number)
    {
        return fail("`tuning.reference_midi_note` is missing or not a number".to_owned());
    }
    for key in ["tuning_name", "mapping_name"] {
        if !tuning.get(key).is_some_and(Value::is_string) {
            return fail(format!("`tuning.{key}` is missing or not a text"));
        }
    }
    if tuning
        .get("default")
        .is_some_and(|value| !value.is_boolean())
    {
        return fail("`tuning.default` is not true or false".to_owned());
    }
    let Some(scale) = tuning
        .get("scale")
        .and_then(Value::as_array)
        .filter(|scale| scale.iter().all(Value::is_number))
    else {
        return fail("`tuning.scale` is missing or not a list of numbers".to_owned());
    };
    if let Some(mapping) = tuning.get("mapping") {
        let fits = mapping.as_array().is_some_and(|mapping| {
            mapping.iter().all(|step| {
                step.as_u64()
                    .and_then(|step| usize::try_from(step).ok())
                    .is_some_and(|step| step < scale.len())
            })
        });
        if !fits {
            return fail(
                "`tuning.mapping` is not a list of positions in `tuning.scale`".to_owned(),
            );
        }
    }
    Ok(())
}

/// Finds the preset JSON inside a state: the first JSON object that holds
/// `settings`. A state the plug-in saved wraps the JSON in binary headers
/// and a trailer; a state the builder made is the JSON itself.
#[must_use]
pub fn saved_preset(state: &[u8]) -> Option<Map<String, Value>> {
    state
        .iter()
        .enumerate()
        .filter(|(_, byte)| **byte == b'{')
        .take(MAX_STARTS)
        .find_map(|(start, _)| {
            let mut values = serde_json::Deserializer::from_slice(&state[start..]).into_iter();
            match values.next() {
                Some(Ok(Value::Object(root))) if root.contains_key("settings") => Some(root),
                _ => None,
            }
        })
}

/// A part of the sent state that the saved state does not hold.
#[derive(Clone, Debug, PartialEq)]
pub struct Mismatch {
    /// The path of the value, such as `settings.filter_1_cutoff`.
    pub path: String,
    /// The sent value. For the tempo control it is the value expected
    /// from the render tempo, in beats per second.
    pub sent: Value,
    /// The saved value, or `None` when the key is missing.
    pub saved: Option<Value>,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.saved {
            Some(saved) => write!(
                f,
                "{} is {}, not {}",
                self.path,
                short(saved),
                short(&self.sent)
            ),
            None => write!(f, "{} is missing", self.path),
        }
    }
}

/// A value as JSON, cut to [`SHOWN_CHARS`] characters.
fn short(value: &Value) -> String {
    let text = value.to_string();
    match text.char_indices().nth(SHOWN_CHARS) {
        Some((end, _)) => format!("{}... ({} characters)", &text[..end], text.chars().count()),
        None => text,
    }
}

/// Compares the preset JSON of a sent state with the one in a saved
/// state: the name, the tuning and every control, a number under
/// `settings`. Keys the plug-in adds on save are ignored.
///
/// The tempo control, `beats_per_minute` in beats per second, is compared
/// with `bpm` instead of the sent value. A plug-in in a host sets it from
/// the host's tempo on every block, so the saved value shows the tempo the
/// render played at. The expected value is the one Vital computes,
/// [`vital_beats_per_second`].
///
/// The comparison is Vital-shaped: the tempo key is a Vital key. A Serum 2
/// read-back needs its own rule.
///
/// # Errors
///
/// Returns [`RenderError::State`] when either state holds no preset JSON.
pub fn readback_mismatches(
    sent: &PresetState,
    saved: &[u8],
    bpm: Bpm,
) -> Result<Vec<Mismatch>, RenderError> {
    let sent = saved_preset(sent.bytes())
        .ok_or_else(|| RenderError::State("the sent state holds no preset JSON".to_owned()))?;
    let saved = saved_preset(saved).ok_or_else(|| {
        RenderError::State("the state the plug-in saved holds no preset JSON".to_owned())
    })?;
    let mut mismatches = Vec::new();
    let mut check = |path: String, sent: &Value, saved: Option<&Value>| {
        let same = match (sent, saved) {
            (Value::Number(a), Some(Value::Number(b))) => a
                .as_f64()
                .zip(b.as_f64())
                .is_some_and(|(a, b)| same_f32(a, b)),
            (sent, Some(saved)) => sent == saved,
            (_, None) => false,
        };
        if !same {
            mismatches.push(Mismatch {
                path,
                sent: sent.clone(),
                saved: saved.cloned(),
            });
        }
    };
    if let Some(name) = sent.get("preset_name") {
        check("preset_name".to_owned(), name, saved.get("preset_name"));
    }
    let tuning = |root: &Map<String, Value>| root.get(TUNING).and_then(Value::as_object).cloned();
    let saved_tuning = tuning(&saved).unwrap_or_default();
    for (key, value) in tuning(&sent).unwrap_or_default() {
        let path = format!("{TUNING}.{key}");
        match (&value, saved_tuning.get(&key)) {
            (Value::Array(list), Some(Value::Array(other))) if list.len() == other.len() => {
                for (index, (value, other)) in list.iter().zip(other).enumerate() {
                    check(format!("{path}[{index}]"), value, Some(other));
                }
            }
            (value, other) => check(path, value, other),
        }
    }
    let controls =
        |root: &Map<String, Value>| root.get("settings").and_then(Value::as_object).cloned();
    let saved_controls = controls(&saved).unwrap_or_default();
    let sent_controls = controls(&sent).unwrap_or_default();
    for (key, value) in &sent_controls {
        if value.is_number() && key != TEMPO {
            check(format!("settings.{key}"), value, saved_controls.get(key));
        }
    }
    if sent_controls.contains_key(TEMPO) || saved_controls.contains_key(TEMPO) {
        let beats_per_second = Value::from(vital_beats_per_second(bpm));
        check(
            format!("settings.{TEMPO}"),
            &beats_per_second,
            saved_controls.get(TEMPO),
        );
    }
    Ok(mismatches)
}

/// The tempo Vital stores for a host tempo of `bpm`, in beats per second.
///
/// `SoundEngine::setBpm` divides by 60 in 32-bit floats. The shipped
/// 1.0.7 plug-in computes it as `f32(bpm) * f32(1 / 60)`, a multiply by the
/// reciprocal, which can lie one or two `f32` steps from the correctly
/// rounded `bpm / 60` on a fractional tempo.
#[must_use]
pub fn vital_beats_per_second(bpm: Bpm) -> f64 {
    f64::from(cast::f64_to_f32(bpm.get()) * cast::f64_to_f32(1.0 / 60.0))
}

/// Whether two numbers are the same `f32` or neighbours.
fn same_f32(a: f64, b: f64) -> bool {
    let (a, b) = (cast::f64_to_f32(a), cast::f64_to_f32(b));
    let step = (a.next_up() - a).max(a - a.next_down());
    (a - b).abs() <= step
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &[u8] = include_bytes!("../../tests/fixtures/vital/minimal.vital");

    /// The minimal preset with a name and two controls.
    fn preset() -> Vec<u8> {
        let mut preset: Value = serde_json::from_slice(MINIMAL).unwrap();
        preset["preset_name"] = "Probe".into();
        preset["settings"]["volume"] = 5473.0.into();
        preset["settings"]["filter_1_cutoff"] = 60.1.into();
        serde_json::to_vec(&preset).unwrap()
    }

    fn state() -> PresetState {
        preset_state(Synth::Vital, &preset(), &Recipe::current()).unwrap()
    }

    #[test]
    fn the_vital_state_is_the_preset_json_with_a_nul() {
        let state = state();
        assert_eq!(state.synth(), Synth::Vital);
        assert_eq!(state.bytes().last(), Some(&0));
        let json: Value =
            serde_json::from_slice(&state.bytes()[..state.bytes().len() - 1]).unwrap();
        assert!(json["settings"].is_object());
    }

    #[test]
    fn the_state_forces_the_recipe_oversampling() {
        let mut preset: Value = serde_json::from_slice(&preset()).unwrap();
        preset["settings"][OVERSAMPLING] = 3.0.into();
        let preset = serde_json::to_vec(&preset).unwrap();
        let state = preset_state(Synth::Vital, &preset, &Recipe::current()).unwrap();
        let settings = &saved_preset(state.bytes()).unwrap()["settings"];
        assert_eq!(settings[OVERSAMPLING], 1.0);
    }

    #[test]
    fn serum_2_and_a_broken_preset_are_refused() {
        let recipe = Recipe::current();
        assert!(matches!(
            preset_state(Synth::Serum2, MINIMAL, &recipe),
            Err(RenderError::Synth(SynthError::NotBuilt(Synth::Serum2)))
        ));
        assert!(matches!(
            preset_state(Synth::Vital, b"{", &recipe),
            Err(RenderError::Preset(_))
        ));
    }

    /// A state wrapped the way the plug-in saves it: binary headers with a
    /// stray `{`, the JSON with an added key, a NUL and a trailer.
    fn wrapped(json: &Value) -> Vec<u8> {
        let mut bytes = b"VstW\0\0\0\x08{\0\0\x01CcnK".to_vec();
        bytes.extend_from_slice(&serde_json::to_vec(json).unwrap());
        bytes.extend_from_slice(b"\0\0\0JUCEPrivateData");
        bytes
    }

    #[test]
    fn a_saved_state_matches_within_an_f32_step() {
        let sent = state();
        let mut saved = Value::Object(saved_preset(sent.bytes()).unwrap());
        saved["added_on_save"] = true.into();
        saved["settings"]["filter_1_cutoff"] = f64::from(cast::f64_to_f32(60.1)).into();
        assert_eq!(
            readback_mismatches(&sent, &wrapped(&saved), bpm()).unwrap(),
            []
        );
    }

    #[test]
    fn a_changed_name_or_control_is_named() {
        let sent = state();
        let mut saved = Value::Object(saved_preset(sent.bytes()).unwrap());
        saved["preset_name"] = "Other".into();
        saved["settings"][OVERSAMPLING] = 0.0.into();
        saved["settings"].as_object_mut().unwrap().remove("volume");
        let paths: Vec<String> = readback_mismatches(&sent, &wrapped(&saved), bpm())
            .unwrap()
            .iter()
            .map(|m| m.path.clone())
            .collect();
        assert!(paths.contains(&"preset_name".to_owned()), "{paths:?}");
        assert!(
            paths.contains(&"settings.oversampling".to_owned()),
            "{paths:?}"
        );
    }

    /// The minimal preset with `tuning` set to `tuning`.
    fn tuned(tuning: Value) -> Result<PresetState, RenderError> {
        let mut preset: Value = serde_json::from_slice(&preset()).unwrap();
        preset[TUNING] = tuning;
        preset_state(
            Synth::Vital,
            &serde_json::to_vec(&preset).unwrap(),
            &Recipe::current(),
        )
    }

    fn up_two() -> Value {
        let mut tuning = default_tuning();
        tuning["reference_midi_note"] = 2.into();
        tuning["default"] = false.into();
        tuning
    }

    #[test]
    fn a_preset_without_a_tuning_gets_the_default_one() {
        assert_eq!(
            saved_preset(state().bytes()).unwrap()[TUNING],
            default_tuning()
        );
    }

    #[test]
    fn a_preset_keeps_its_own_tuning() {
        let mut tuning = up_two();
        tuning["mapping"] = serde_json::json!([]);
        let state = tuned(tuning).unwrap();
        assert_eq!(saved_preset(state.bytes()).unwrap()[TUNING], up_two());
    }

    #[test]
    fn a_tuning_the_plug_in_cannot_read_is_refused() {
        let broken: [(&str, Value); 7] = [
            ("scale_start_midi_note", 128.into()),
            ("reference_midi_note", "2".into()),
            ("tuning_name", Value::Null),
            ("default", 1.into()),
            ("scale", serde_json::json!([0, "1"])),
            ("mapping", serde_json::json!([0, 13])),
            ("mapping", serde_json::json!([-1])),
        ];
        for (key, value) in broken {
            let mut tuning = default_tuning();
            tuning[key] = value;
            let error = tuned(tuning).unwrap_err();
            assert!(matches!(error, RenderError::Preset(_)), "{key}: {error}");
            assert!(
                error.to_string().contains(&format!("`tuning.{key}`")),
                "{error}"
            );
        }
        assert!(matches!(tuned(12.into()), Err(RenderError::Preset(_))));
    }

    #[test]
    fn a_tuning_the_plug_in_did_not_load_is_named() {
        let sent = tuned(up_two()).unwrap();
        let mut saved = Value::Object(saved_preset(sent.bytes()).unwrap());
        saved[TUNING] = default_tuning();
        let paths: Vec<String> = readback_mismatches(&sent, &wrapped(&saved), bpm())
            .unwrap()
            .iter()
            .map(|m| m.path.clone())
            .collect();
        assert_eq!(paths, ["tuning.reference_midi_note", "tuning.default"]);
    }

    #[test]
    fn a_long_value_is_cut_in_a_message() {
        let mismatch = Mismatch {
            path: "preset_name".to_owned(),
            sent: "x".repeat(100).into(),
            saved: Some("y".into()),
        };
        assert_eq!(
            mismatch.to_string(),
            format!(
                "preset_name is \"y\", not \"{}... (102 characters)",
                "x".repeat(39)
            )
        );
    }

    fn bpm() -> Bpm {
        Bpm::new(120.0).unwrap()
    }

    /// Vital's tempo for `bpm`: `f32(bpm) * f32(1 / 60)`.
    fn vital(bpm: f64) -> f64 {
        f64::from(cast::f64_to_f32(bpm) * cast::f64_to_f32(1.0 / 60.0))
    }

    #[test]
    fn the_saved_tempo_is_compared_with_the_render_tempo() {
        let sent = state();
        let mut saved = Value::Object(saved_preset(sent.bytes()).unwrap());
        // Vital 1.0.7 read back 2.3333334922790527 at 140 BPM.
        assert_eq!(vital(140.0), 2.333_333_492_279_052_7);
        saved["settings"][TEMPO] = vital(140.0).into();
        let at = |tempo: f64| {
            readback_mismatches(&sent, &wrapped(&saved), Bpm::new(tempo).unwrap()).unwrap()
        };
        assert_eq!(at(140.0), []);
        let mismatches = at(120.0);
        assert_eq!(mismatches.len(), 1, "{mismatches:?}");
        assert_eq!(
            mismatches[0].to_string(),
            "settings.beats_per_minute is 2.3333334922790527, not 2.0"
        );
        let mut sent_root = Value::Object(saved_preset(sent.bytes()).unwrap());
        sent_root["settings"][TEMPO] = 2.0.into();
        let with_tempo = PresetState::new(Synth::Vital, serde_json::to_vec(&sent_root).unwrap());
        let mut lost = saved.clone();
        lost["settings"].as_object_mut().unwrap().remove(TEMPO);
        let mismatches =
            readback_mismatches(&with_tempo, &wrapped(&lost), Bpm::new(140.0).unwrap()).unwrap();
        assert_eq!(
            mismatches[0].to_string(),
            "settings.beats_per_minute is missing"
        );
    }

    #[test]
    fn a_fractional_tempo_matches_the_reciprocal_multiply() {
        let sent = state();
        let mut saved = Value::Object(saved_preset(sent.bytes()).unwrap());
        saved["settings"][TEMPO] = vital(100.3).into();
        // The correctly rounded 100.3 / 60 lies two f32 steps away.
        assert!(!same_f32(vital(100.3), 100.3 / 60.0));
        let mismatches =
            readback_mismatches(&sent, &wrapped(&saved), Bpm::new(100.3).unwrap()).unwrap();
        assert_eq!(mismatches, []);
    }

    #[test]
    fn structured_parts_are_not_compared() {
        let sent = state();
        let mut saved = Value::Object(saved_preset(sent.bytes()).unwrap());
        saved["settings"]["sample"]["samples"] = "AAAA".into();
        assert_eq!(
            readback_mismatches(&sent, &wrapped(&saved), bpm()).unwrap(),
            []
        );
    }

    #[test]
    fn a_state_with_no_json_is_an_error() {
        assert!(matches!(
            readback_mismatches(&state(), b"VstW\0\0{", bpm()),
            Err(RenderError::State(_))
        ));
    }

    #[test]
    fn neighbouring_f32_values_match_and_farther_ones_do_not() {
        let x = 0.1_f32;
        assert!(same_f32(f64::from(x), f64::from(x.next_up())));
        assert!(!same_f32(f64::from(x), f64::from(x.next_up().next_up())));
        assert!(same_f32(0.1, f64::from(x)));
    }
}
