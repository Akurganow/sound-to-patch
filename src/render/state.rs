//! The plug-in state of a preset, and the check of a state the plug-in
//! saved back.
//!
//! Vital's state is the preset JSON as text with a trailing NUL, the form
//! the plug-in saves itself. The builder forces the recipe's oversampling
//! into the controls, because oversampling changes the sound.
//!
//! A host call reports success even when the plug-in kept its old sound,
//! so a backend reads the state back and compares it with the sent one:
//! the name and every control. The plug-in keeps controls as 32-bit floats,
//! so a number matches within one `f32` step.

use std::fmt;

use serde_json::{Map, Value};

use super::{PresetState, Recipe, RenderError};
use crate::cast;
use crate::preset::format::{PresetFormat, Stored};
use crate::preset::vital;
use crate::synth::{Synth, SynthError};

/// The Vital control that sets oversampling.
const OVERSAMPLING: &str = "oversampling";
/// How many `{` bytes [`saved_preset`] tries as the start of the JSON.
const MAX_STARTS: usize = 64;

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
    let mut bytes = vital::FORMAT
        .write(model.as_ref())
        .map_err(RenderError::Preset)?;
    bytes.push(0);
    Ok(PresetState::new(Synth::Vital, bytes))
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
    /// The sent value.
    pub sent: Value,
    /// The saved value, or `None` when the key is missing.
    pub saved: Option<Value>,
}

impl fmt::Display for Mismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.saved {
            Some(saved) => write!(f, "{} is {saved}, not {}", self.path, self.sent),
            None => write!(f, "{} is missing", self.path),
        }
    }
}

/// Compares the preset JSON of a sent state with the one in a saved
/// state: the name and every part under `settings`. Keys the plug-in adds
/// on save are ignored.
///
/// # Errors
///
/// Returns [`RenderError::State`] when either state holds no preset JSON.
pub fn readback_mismatches(sent: &PresetState, saved: &[u8]) -> Result<Vec<Mismatch>, RenderError> {
    let sent = saved_preset(sent.bytes())
        .ok_or_else(|| RenderError::State("the sent state holds no preset JSON".to_owned()))?;
    let saved = saved_preset(saved).ok_or_else(|| {
        RenderError::State("the state the plug-in saved holds no preset JSON".to_owned())
    })?;
    let mut mismatches = Vec::new();
    for key in ["preset_name", "settings"] {
        if let Some(value) = sent.get(key) {
            compare(key, value, saved.get(key), &mut mismatches);
        }
    }
    Ok(mismatches)
}

fn compare(path: &str, sent: &Value, saved: Option<&Value>, out: &mut Vec<Mismatch>) {
    let fail = |out: &mut Vec<Mismatch>| {
        out.push(Mismatch {
            path: path.to_owned(),
            sent: sent.clone(),
            saved: saved.cloned(),
        });
    };
    match (sent, saved) {
        (Value::Object(sent), Some(Value::Object(saved))) => {
            for (key, value) in sent {
                compare(&format!("{path}.{key}"), value, saved.get(key), out);
            }
        }
        (Value::Array(sent), Some(Value::Array(saved))) if sent.len() == saved.len() => {
            for (index, (value, other)) in sent.iter().zip(saved).enumerate() {
                compare(&format!("{path}[{index}]"), value, Some(other), out);
            }
        }
        (Value::Number(a), Some(Value::Number(b))) => {
            if !a
                .as_f64()
                .zip(b.as_f64())
                .is_some_and(|(a, b)| same_f32(a, b))
            {
                fail(out);
            }
        }
        (sent, Some(saved)) if sent == saved => {}
        _ => fail(out),
    }
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
        saved["tuning"] = serde_json::json!({ "default": true });
        saved["settings"]["filter_1_cutoff"] = f64::from(cast::f64_to_f32(60.1)).into();
        assert_eq!(readback_mismatches(&sent, &wrapped(&saved)).unwrap(), []);
    }

    #[test]
    fn a_changed_name_or_control_is_named() {
        let sent = state();
        let mut saved = Value::Object(saved_preset(sent.bytes()).unwrap());
        saved["preset_name"] = "Other".into();
        saved["settings"][OVERSAMPLING] = 0.0.into();
        saved["settings"].as_object_mut().unwrap().remove("volume");
        let paths: Vec<String> = readback_mismatches(&sent, &wrapped(&saved))
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

    #[test]
    fn a_state_with_no_json_is_an_error() {
        assert!(matches!(
            readback_mismatches(&state(), b"VstW\0\0{"),
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
