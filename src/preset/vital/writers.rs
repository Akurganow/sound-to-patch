//! Typed writers for the structured parts of a `.vital` file: modulation
//! routes, wavetables, LFO shapes, effect switches and order, and macro
//! labels. Each writer checks its input before it changes the model, so a
//! failed call leaves the model as it was.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Map, Value, json};

use crate::preset::apply::place_route;
use crate::preset::format::{FormatError, PresetModel, Route, RoutePolarity, Stored};
use crate::preset::registry::ParamRegistry;
use crate::preset::vital::codec::VitalModel;
use crate::preset::vital::{DEFAULT_STAMP, registry};
use crate::synth::EffectFamily;

/// The 32 modulation source names.
pub const ROUTE_SOURCES: [&str; 32] = [
    "env_1",
    "env_2",
    "env_3",
    "env_4",
    "env_5",
    "env_6",
    "lfo_1",
    "lfo_2",
    "lfo_3",
    "lfo_4",
    "lfo_5",
    "lfo_6",
    "lfo_7",
    "lfo_8",
    "random_1",
    "random_2",
    "random_3",
    "random_4",
    "macro_control_1",
    "macro_control_2",
    "macro_control_3",
    "macro_control_4",
    "random",
    "stereo",
    "mod_wheel",
    "pitch_wheel",
    "note",
    "note_in_octave",
    "velocity",
    "aftertouch",
    "slide",
    "lift",
];

/// The polarity of a route whose edit names none: bipolar for LFOs, random
/// sources, `stereo` and `pitch_wheel`, unipolar for the rest.
#[must_use]
pub fn default_polarity(source: &str) -> RoutePolarity {
    if source.starts_with("lfo_")
        || source.starts_with("random")
        || source == "stereo"
        || source == "pitch_wheel"
    {
        RoutePolarity::Bipolar
    } else {
        RoutePolarity::Unipolar
    }
}

/// Samples in one wavetable frame.
pub const FRAME_SAMPLES: usize = 2048;

/// The highest keyframe position: a table has 257 frames.
pub const MAX_POSITION: u16 = 256;

/// Base64 characters of one frame: 8192 bytes with padding.
pub const FRAME_BASE64_CHARS: usize = 10_924;

/// The most points an LFO shape holds.
pub const MAX_LFO_POINTS: usize = 100;

/// One keyframe of a wavetable: a frame position and one cycle.
#[derive(Clone, Debug, PartialEq)]
pub struct Keyframe {
    /// The frame index, 0 to 256.
    pub position: u16,
    /// One cycle of 2048 samples.
    pub samples: Vec<f32>,
}

/// A wavetable built from one-cycle keyframes. Frames between keyframes
/// are interpolated by the synth when it loads the table.
#[derive(Clone, Debug, PartialEq)]
pub struct Wavetable {
    /// The table name.
    pub name: String,
    /// The table author.
    pub author: String,
    /// The keyframes, in any order.
    pub keyframes: Vec<Keyframe>,
}

/// An LFO shape: points with one curve power per point.
#[derive(Clone, Debug, PartialEq)]
pub struct LfoShape {
    /// The shape name.
    pub name: String,
    /// `(x, y)` points, x strictly increasing from 0 to 1, y from 0 to 1.
    /// The synth draws `1 - y`, so `y = 0` is the top.
    pub points: Vec<(f64, f64)>,
    /// One curve power per point.
    pub powers: Vec<f64>,
    /// Whether the synth smooths the shape.
    pub smooth: bool,
}

fn write_error(cause: impl Into<String>, fix: impl Into<String>) -> FormatError {
    FormatError::new("cannot write the .vital part", cause, fix)
}

/// The switch control of an effect family.
fn effect_switch(effect: EffectFamily) -> &'static str {
    match effect {
        EffectFamily::Chorus => "chorus_on",
        EffectFamily::Compressor => "compressor_on",
        EffectFamily::Delay => "delay_on",
        EffectFamily::Distortion => "distortion_on",
        EffectFamily::Eq => "eq_on",
        EffectFamily::Filter => "filter_fx_on",
        EffectFamily::Flanger => "flanger_on",
        EffectFamily::Phaser => "phaser_on",
        EffectFamily::Reverb => "reverb_on",
    }
}

/// Checks the one effect order code the writer knows how to encode.
pub(crate) fn check_effect_order(code: f64) -> Result<(), FormatError> {
    if code == 0.0 {
        Ok(())
    } else {
        Err(write_error(
            format!("effect_chain_order {code} is not the default order"),
            "Keep code 0; the encoding of other orders is unverified",
        ))
    }
}

impl VitalModel {
    /// Adds a route, or updates the slot that joins the same source and
    /// destination, so a route never fires twice. It writes the amount,
    /// the bipolar flag, the curve power (0 unless given), and the stereo
    /// and bypass flags (0).
    ///
    /// # Errors
    ///
    /// A [`FormatError`] for an unknown source (it lists the 32 names), a
    /// destination outside the registry (it lists close names), an amount
    /// outside -1 to 1, a power outside -10 to 10, or a full matrix (it
    /// names the 64-slot limit).
    pub fn route(
        &mut self,
        source: &str,
        destination: &str,
        amount: f64,
        polarity: Option<RoutePolarity>,
        power: Option<f64>,
    ) -> Result<usize, FormatError> {
        if !ROUTE_SOURCES.contains(&source) {
            return Err(write_error(
                format!("{source:?} is not a modulation source"),
                format!("Use one of: {}", ROUTE_SOURCES.join(", ")),
            ));
        }
        let registry = registry().map_err(|error| write_error(error.to_string(), "Report this"))?;
        if registry.get(destination).is_none_or(|spec| spec.legacy) {
            return Err(write_error(
                format!("{destination:?} is not a control of Vital"),
                format!(
                    "Close names: {}",
                    registry.close_names(destination, 10).join(", ")
                ),
            ));
        }
        if !(-1.0..=1.0).contains(&amount) {
            return Err(write_error(
                format!("route amount {amount} is outside -1 to 1"),
                "Use a fraction of the destination's range from -1 to 1",
            ));
        }
        let power = power.unwrap_or(0.0);
        if !(-10.0..=10.0).contains(&power) {
            return Err(write_error(
                format!("route power {power} is outside -10 to 10"),
                "Use a curve power from -10 to 10",
            ));
        }
        let mut route = Route {
            source: source.to_owned(),
            destination: destination.to_owned(),
            amount,
            polarity: polarity.unwrap_or_else(|| default_polarity(source)),
            extras: Vec::new(),
        };
        let slot = self
            .routes()
            .iter()
            .position(|have| {
                have.as_ref()
                    .is_some_and(|have| have.source == source && have.destination == destination)
            })
            .or_else(|| self.routes().iter().position(Option::is_none));
        if let Some(slot) = slot {
            route.extras.push((
                format!("modulation_{}_power", slot + 1),
                Stored::Float(power),
            ));
        }
        place_route(self, route)
    }

    /// Empties a slot and resets its five controls to their defaults.
    ///
    /// # Errors
    ///
    /// A [`FormatError`] for a slot outside 0 to 63.
    pub fn clear_route(&mut self, slot: usize) -> Result<(), FormatError> {
        self.set_route(slot, None)
    }

    /// Writes the wavetable of oscillator `index` (0 to 2), one Wave
    /// Source component with the keyframes sorted by position. The table
    /// asks the synth not to remove DC or normalise, so the frames stay as
    /// written.
    ///
    /// # Errors
    ///
    /// A [`FormatError`] for an index above 2, no keyframe, a position
    /// above 256 or used twice, a frame that is not 2048 samples, or a
    /// sample that is not finite.
    pub fn set_wavetable(&mut self, index: usize, table: &Wavetable) -> Result<(), FormatError> {
        if index > 2 {
            return Err(write_error(
                format!("oscillator {} does not exist", index + 1),
                "Vital has 3 oscillators; use index 0 to 2",
            ));
        }
        if table.keyframes.is_empty() {
            return Err(write_error(
                "the wavetable has no keyframe",
                "Add at least one frame",
            ));
        }
        let mut keyframes: Vec<&Keyframe> = table.keyframes.iter().collect();
        keyframes.sort_by_key(|keyframe| keyframe.position);
        for (i, keyframe) in keyframes.iter().enumerate() {
            if keyframe.position > MAX_POSITION {
                return Err(write_error(
                    format!(
                        "keyframe position {} is above {MAX_POSITION}",
                        keyframe.position
                    ),
                    "Use frame positions from 0 to 256",
                ));
            }
            if i > 0 && keyframes[i - 1].position == keyframe.position {
                return Err(write_error(
                    format!("keyframe position {} appears twice", keyframe.position),
                    "Give each keyframe its own position",
                ));
            }
            if keyframe.samples.len() != FRAME_SAMPLES {
                return Err(write_error(
                    format!("a frame has {} samples", keyframe.samples.len()),
                    "Write one cycle of exactly 2048 samples",
                ));
            }
            if let Some(bad) = keyframe.samples.iter().find(|sample| !sample.is_finite()) {
                return Err(write_error(
                    format!("frame {} holds the sample {bad}", keyframe.position),
                    "Write finite samples only",
                ));
            }
        }
        let frames: Vec<Value> = keyframes
            .iter()
            .map(|keyframe| {
                let bytes: Vec<u8> = keyframe
                    .samples
                    .iter()
                    .flat_map(|sample| sample.to_le_bytes())
                    .collect();
                json!({"position": keyframe.position, "wave_data": STANDARD.encode(bytes)})
            })
            .collect();
        let value = json!({
            "groups": [{
                "components": [{
                    "keyframes": frames,
                    "type": "Wave Source",
                    "interpolation_style": 1,
                    "interpolation": 1,
                }],
            }],
            "name": table.name,
            "author": table.author,
            "version": DEFAULT_STAMP,
            "remove_all_dc": false,
            "full_normalize": false,
        });
        self.set_list_item("wavetables", 3, index, value)
    }

    /// Reads back the Wave Source keyframes of oscillator `index`.
    ///
    /// # Errors
    ///
    /// A [`FormatError`] when the table is missing, holds another component
    /// first, or a `wave_data` text has the wrong length or is not base64.
    /// The length is checked before decoding.
    pub fn wavetable(&self, index: usize) -> Result<Wavetable, FormatError> {
        let missing = || {
            write_error(
                format!("wavetable {index} is missing or not a Wave Source"),
                "Write it with set_wavetable",
            )
        };
        let table = self
            .root()
            .get("settings")
            .and_then(|settings| settings.get("wavetables"))
            .and_then(|tables| tables.get(index))
            .ok_or_else(missing)?;
        let component = table
            .pointer("/groups/0/components/0")
            .filter(|component| {
                component.get("type").and_then(Value::as_str) == Some("Wave Source")
            })
            .ok_or_else(missing)?;
        let mut keyframes = Vec::new();
        for keyframe in component
            .get("keyframes")
            .and_then(Value::as_array)
            .ok_or_else(missing)?
        {
            let position = keyframe
                .get("position")
                .and_then(Value::as_u64)
                .and_then(|position| u16::try_from(position).ok())
                .ok_or_else(missing)?;
            let text = keyframe
                .get("wave_data")
                .and_then(Value::as_str)
                .ok_or_else(missing)?;
            if text.len() != FRAME_BASE64_CHARS {
                return Err(write_error(
                    format!(
                        "wave_data at position {position} has {} characters",
                        text.len()
                    ),
                    "A frame is 10924 base64 characters",
                ));
            }
            let bytes = STANDARD.decode(text).map_err(|error| {
                write_error(
                    format!("wave_data is not base64: {error}"),
                    "Write the frame again",
                )
            })?;
            let samples = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|chunk| f32::from_le_bytes(*chunk))
                .collect();
            keyframes.push(Keyframe { position, samples });
        }
        let text = |key: &str| {
            table
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        Ok(Wavetable {
            name: text("name"),
            author: text("author"),
            keyframes,
        })
    }

    /// Writes the shape of LFO `index` (0 to 7).
    ///
    /// # Errors
    ///
    /// A [`FormatError`] for an index above 7, fewer than 2 or more than
    /// 100 points, a power count that differs from the point count, x not
    /// strictly increasing within 0 to 1, y outside 0 to 1, or a value
    /// that is not finite.
    pub fn set_lfo_shape(&mut self, index: usize, shape: &LfoShape) -> Result<(), FormatError> {
        if index > 7 {
            return Err(write_error(
                format!("LFO {} does not exist", index + 1),
                "Vital has 8 LFOs; use index 0 to 7",
            ));
        }
        let count = shape.points.len();
        if !(2..=MAX_LFO_POINTS).contains(&count) {
            return Err(write_error(
                format!("the shape has {count} points"),
                "Use 2 to 100 points",
            ));
        }
        if shape.powers.len() != count {
            return Err(write_error(
                format!("{} powers for {count} points", shape.powers.len()),
                "Give one power per point",
            ));
        }
        for (i, &(x, y)) in shape.points.iter().enumerate() {
            let rising = i == 0 || shape.points[i - 1].0 < x;
            if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) || !rising {
                return Err(write_error(
                    format!("point {} is ({x}, {y})", i + 1),
                    "Keep x strictly increasing within 0 to 1 and y within 0 to 1",
                ));
            }
        }
        if let Some(power) = shape.powers.iter().find(|power| !power.is_finite()) {
            return Err(write_error(
                format!("a power is {power}"),
                "Write finite powers",
            ));
        }
        let points: Vec<f64> = shape.points.iter().flat_map(|&(x, y)| [x, y]).collect();
        let value = json!({
            "num_points": count,
            "points": points,
            "powers": shape.powers,
            "name": shape.name,
            "smooth": shape.smooth,
        });
        self.set_list_item("lfos", 8, index, value)
    }

    /// Switches an effect on or off.
    ///
    /// # Errors
    ///
    /// A [`FormatError`] if the model has no settings.
    pub fn set_effect_on(&mut self, effect: EffectFamily, on: bool) -> Result<(), FormatError> {
        self.set(
            effect_switch(effect),
            Stored::Float(if on { 1.0 } else { 0.0 }),
        )
    }

    /// Sets the order of the effects. Only the default order, code 0, is
    /// written until the encoding of other codes is verified.
    ///
    /// # Errors
    ///
    /// A [`FormatError`] for any other code.
    pub fn set_effect_order(&mut self, code: u32) -> Result<(), FormatError> {
        let code = f64::from(code);
        check_effect_order(code)?;
        self.set("effect_chain_order", Stored::Float(code))
    }

    /// Sets the label of macro `number` (1 to 4).
    ///
    /// # Errors
    ///
    /// A [`FormatError`] for a number outside 1 to 4.
    pub fn set_macro_label(&mut self, number: usize, label: &str) -> Result<(), FormatError> {
        if !(1..=4).contains(&number) {
            return Err(write_error(
                format!("macro {number} does not exist"),
                "Vital has macros 1 to 4",
            ));
        }
        self.set_top_text(&format!("macro{number}"), label);
        Ok(())
    }

    /// Replaces item `index` of a structured list, first filling the list
    /// to `len` with copies of the template's defaults if it is shorter.
    fn set_list_item(
        &mut self,
        list: &str,
        len: usize,
        index: usize,
        value: Value,
    ) -> Result<(), FormatError> {
        let settings = self.settings_map_mut()?;
        let items = settings
            .entry(list)
            .or_insert_with(|| Value::Array(Vec::new()));
        let Value::Array(items) = items else {
            return Err(write_error(
                format!("`settings.{list}` is not a list"),
                "Read the preset again from a valid file",
            ));
        };
        let fill = super::template::default_item(list);
        while items.len() < len {
            items.push(fill.clone());
        }
        items[index] = value;
        Ok(())
    }
}

/// The effective value of a control: the file's number, or the registry
/// default when the key is absent.
fn effective(settings: &Map<String, Value>, key: &str) -> f64 {
    settings
        .get(key)
        .and_then(Value::as_f64)
        .unwrap_or_else(|| {
            registry()
                .ok()
                .and_then(|registry| registry.get(key))
                .map_or(0.0, |spec| spec.default)
        })
}

/// The switch that turns off the module a control belongs to.
fn module_switch(key: &str) -> Option<String> {
    let parts: Vec<&str> = key.splitn(3, '_').collect();
    match parts.as_slice() {
        ["osc" | "filter", id, _] => Some(format!("{}_{id}_on", parts[0])),
        [family, ..]
            if [
                "chorus",
                "compressor",
                "delay",
                "distortion",
                "eq",
                "flanger",
                "phaser",
                "reverb",
                "sample",
            ]
            .contains(family) =>
        {
            Some(format!("{family}_on"))
        }
        _ => None,
    }
}

/// Checks a model after edits and returns its warnings.
pub(crate) fn scan(model: &VitalModel) -> Result<Vec<String>, FormatError> {
    let registry = registry().map_err(|error| write_error(error.to_string(), "Report this"))?;
    let settings = model
        .root()
        .get("settings")
        .and_then(Value::as_object)
        .ok_or_else(|| write_error("the model has no settings", "Read the preset again"))?;
    for (key, value) in settings {
        let Some(spec) = registry.get(key) else {
            continue;
        };
        let Some(number) = value.as_f64() else {
            return Err(write_error(
                format!("control {key} holds {value}, not a number"),
                "Vital throws on load; write a number",
            ));
        };
        if !spec.accepts(number) {
            return Err(write_error(
                format!(
                    "control {key} holds {number}, outside {}",
                    spec.range_text()
                ),
                "Vital applies it unclamped; write a value inside the range",
            ));
        }
    }
    let mut warnings = Vec::new();
    for (slot, route) in model.routes().iter().enumerate() {
        let Some(route) = route else { continue };
        if let Some(switch) = module_switch(&route.destination)
            && effective(settings, &switch) == 0.0
        {
            warnings.push(format!(
                "slot {}: {} belongs to a module that is off ({switch} = 0), so the route does nothing",
                slot + 1,
                route.destination
            ));
        }
    }
    for (filter, targets) in [(1, [0.0, 2.0]), (2, [1.0, 2.0])] {
        if effective(settings, &format!("filter_{filter}_on")) == 0.0 {
            continue;
        }
        let fed = (1..=3).any(|osc| {
            effective(settings, &format!("osc_{osc}_on")) != 0.0
                && targets.contains(&effective(settings, &format!("osc_{osc}_destination")))
        }) || (effective(settings, "sample_on") != 0.0
            && targets.contains(&effective(settings, "sample_destination")))
            || (filter == 2 && effective(settings, "filter_2_filter_input") != 0.0);
        if !fed {
            warnings.push(format!(
                "filter {filter} is on, but no oscillator or sample routes into it, so it does nothing"
            ));
        }
    }
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::format::PresetFormat;
    use crate::preset::template::TemplateSource;
    use crate::preset::vital::{FORMAT, VitalTemplate};

    fn template() -> VitalModel {
        let model = FORMAT.read(&VitalTemplate.builtin().unwrap()).unwrap();
        model.as_any().downcast_ref::<VitalModel>().unwrap().clone()
    }

    fn empty() -> VitalModel {
        let model = FORMAT
            .read(br#"{"synth_version":"1.0.6","settings":{"sample":{"length":1,"sample_rate":44100,"samples":"AAA="}}}"#)
            .unwrap();
        model.as_any().downcast_ref::<VitalModel>().unwrap().clone()
    }

    fn saw(position: u16, offset: f32) -> Keyframe {
        Keyframe {
            position,
            samples: (0..2048u16)
                .map(|i| f32::from(i) / 1024.0 - 1.0 + offset)
                .collect(),
        }
    }

    #[test]
    fn a_route_sets_the_slot_and_its_five_controls() {
        let mut model = empty();
        let slot = model
            .route("lfo_1", "filter_1_cutoff", 0.25, None, None)
            .unwrap();
        assert_eq!(slot, 0);
        let route = model.routes()[0].clone().unwrap();
        assert_eq!(
            (route.source.as_str(), route.destination.as_str()),
            ("lfo_1", "filter_1_cutoff")
        );
        assert_eq!(route.polarity, RoutePolarity::Bipolar);
        for (name, value) in [
            ("amount", 0.25),
            ("bipolar", 1.0),
            ("power", 0.0),
            ("stereo", 0.0),
            ("bypass", 0.0),
        ] {
            assert_eq!(
                model.get(&format!("modulation_1_{name}")),
                Some(Stored::Float(value)),
                "{name}"
            );
        }
        assert_eq!(
            model
                .route("lfo_1", "filter_1_cutoff", -0.5, None, Some(2.0))
                .unwrap(),
            0
        );
        assert_eq!(model.get("modulation_1_amount"), Some(Stored::Float(-0.5)));
        assert_eq!(model.get("modulation_1_power"), Some(Stored::Float(2.0)));
    }

    #[test]
    fn the_65th_route_names_the_limit() {
        let mut model = empty();
        let registry = registry().unwrap();
        let keys: Vec<&str> = registry
            .iter()
            .filter(|spec| !spec.legacy && spec.key.ends_with("_amount"))
            .map(|spec| spec.key.as_str())
            .take(65)
            .collect();
        assert_eq!(keys.len(), 65);
        for key in &keys[..64] {
            model.route("env_2", key, 0.1, None, None).unwrap();
        }
        let message = model
            .route("env_2", keys[64], 0.1, None, None)
            .unwrap_err()
            .to_string();
        assert!(message.contains("all 64 modulation slots"), "{message}");
    }

    #[test]
    fn bad_routes_list_the_fix() {
        let mut model = empty();
        let message = model
            .route("lfo_9", "filter_1_cutoff", 0.1, None, None)
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("env_1, env_2") && message.contains("lift"),
            "{message}"
        );
        let message = model
            .route("lfo_1", "filter_1_cuttoff", 0.1, None, None)
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("Close names: filter_1_cutoff"),
            "{message}"
        );
        assert!(
            model
                .route("lfo_1", "filter_1_cutoff", 1.5, None, None)
                .is_err()
        );
        assert!(
            model
                .route("lfo_1", "filter_1_cutoff", 0.5, None, Some(11.0))
                .is_err()
        );
    }

    #[test]
    fn the_polarity_default_follows_the_source_family() {
        for source in ["lfo_3", "random_2", "random", "stereo", "pitch_wheel"] {
            assert_eq!(default_polarity(source), RoutePolarity::Bipolar, "{source}");
        }
        for source in [
            "env_1",
            "macro_control_2",
            "mod_wheel",
            "note",
            "velocity",
            "aftertouch",
            "slide",
            "lift",
            "note_in_octave",
        ] {
            assert_eq!(
                default_polarity(source),
                RoutePolarity::Unipolar,
                "{source}"
            );
        }
    }

    #[test]
    fn a_cleared_slot_restores_the_model() {
        let before = template();
        let mut model = before.clone();
        model
            .route("env_2", "filter_1_cutoff", 0.25, None, Some(-3.0))
            .unwrap();
        assert_ne!(model, before);
        model.clear_route(0).unwrap();
        assert_eq!(model, before);
    }

    #[test]
    fn a_wavetable_round_trips_bit_for_bit() {
        let mut model = template();
        let table = Wavetable {
            name: "Fitted".into(),
            author: "sound-to-patch".into(),
            keyframes: vec![saw(128, 0.0), saw(0, 0.25), saw(256, -0.125)],
        };
        model.set_wavetable(1, &table).unwrap();
        let written = FORMAT.write(&model).unwrap();
        let read = FORMAT.read(&written).unwrap();
        let read = read.as_any().downcast_ref::<VitalModel>().unwrap();
        let back = read.wavetable(1).unwrap();
        let mut expected = table.keyframes.clone();
        expected.sort_by_key(|keyframe| keyframe.position);
        assert_eq!(back.keyframes.len(), 3);
        for (got, want) in back.keyframes.iter().zip(&expected) {
            assert_eq!(got.position, want.position);
            let got: Vec<u32> = got.samples.iter().map(|s| s.to_bits()).collect();
            let want: Vec<u32> = want.samples.iter().map(|s| s.to_bits()).collect();
            assert_eq!(got, want);
        }
        let data =
            &read.root()["settings"]["wavetables"][1]["groups"][0]["components"][0]["keyframes"];
        for keyframe in data.as_array().unwrap() {
            assert_eq!(
                keyframe["wave_data"].as_str().unwrap().len(),
                FRAME_BASE64_CHARS
            );
        }
    }

    #[test]
    fn bad_wavetables_fail_and_leave_the_model() {
        let mut model = template();
        let before = model.clone();
        let twice = Wavetable {
            name: "x".into(),
            author: "x".into(),
            keyframes: vec![saw(3, 0.0), saw(3, 0.0)],
        };
        assert!(
            model
                .set_wavetable(0, &twice)
                .unwrap_err()
                .to_string()
                .contains("appears twice")
        );
        let mut nan = saw(0, 0.0);
        nan.samples[7] = f32::NAN;
        let nan = Wavetable {
            name: "x".into(),
            author: "x".into(),
            keyframes: vec![nan],
        };
        assert!(
            model
                .set_wavetable(0, &nan)
                .unwrap_err()
                .to_string()
                .contains("NaN")
        );
        let far = Wavetable {
            name: "x".into(),
            author: "x".into(),
            keyframes: vec![saw(257, 0.0)],
        };
        assert!(model.set_wavetable(0, &far).is_err());
        assert!(
            model
                .set_wavetable(
                    3,
                    &Wavetable {
                        name: "x".into(),
                        author: "x".into(),
                        keyframes: vec![saw(0, 0.0)]
                    }
                )
                .is_err()
        );
        assert_eq!(model, before);
    }

    #[test]
    fn a_short_wave_data_is_refused_before_decoding() {
        let mut model = template();
        model
            .set_wavetable(
                0,
                &Wavetable {
                    name: "x".into(),
                    author: "x".into(),
                    keyframes: vec![saw(0, 0.0)],
                },
            )
            .unwrap();
        let mut root = model.root().clone();
        root["settings"]["wavetables"][0]["groups"][0]["components"][0]["keyframes"][0]["wave_data"] =
            json!("AAAA");
        let text = serde_json::to_vec(&root).unwrap();
        let read = FORMAT.read(&text).unwrap();
        let read = read.as_any().downcast_ref::<VitalModel>().unwrap();
        assert!(
            read.wavetable(0)
                .unwrap_err()
                .to_string()
                .contains("4 characters")
        );
    }

    #[test]
    fn lfo_shapes_take_up_to_100_points() {
        let shape = |n: usize| LfoShape {
            name: "Ramp".into(),
            points: (0..n)
                .map(|i| {
                    (
                        crate::cast::usize_to_f64(i) / crate::cast::usize_to_f64(n - 1),
                        0.5,
                    )
                })
                .collect(),
            powers: vec![0.0; n],
            smooth: false,
        };
        let mut model = template();
        model.set_lfo_shape(7, &shape(100)).unwrap();
        assert_eq!(
            model.root()["settings"]["lfos"][7]["num_points"],
            json!(100)
        );
        assert!(
            model
                .set_lfo_shape(0, &shape(101))
                .unwrap_err()
                .to_string()
                .contains("101 points")
        );
        let mut falling = shape(3);
        falling.points[2].0 = 0.2;
        assert!(model.set_lfo_shape(0, &falling).is_err());
        let mut uneven = shape(3);
        uneven.powers.pop();
        assert!(model.set_lfo_shape(0, &uneven).is_err());
        assert!(model.set_lfo_shape(8, &shape(3)).is_err());
    }

    #[test]
    fn only_the_default_effect_order_is_written() {
        let mut model = template();
        model.set_effect_order(0).unwrap();
        assert_eq!(model.get("effect_chain_order"), Some(Stored::Float(0.0)));
        let message = model.set_effect_order(5).unwrap_err().to_string();
        assert!(message.contains("unverified"), "{message}");
        model.set_effect_on(EffectFamily::Reverb, true).unwrap();
        assert_eq!(model.get("reverb_on"), Some(Stored::Float(1.0)));
        model.set_effect_on(EffectFamily::Filter, true).unwrap();
        assert_eq!(model.get("filter_fx_on"), Some(Stored::Float(1.0)));
    }

    #[test]
    fn names_reject_an_unknown_style_and_macros_take_labels() {
        let mut model = template();
        assert!(model.set_tags(&["Wobble".to_owned()]).is_err());
        model.set_macro_label(2, "Wub").unwrap();
        assert_eq!(model.root()["macro2"], json!("Wub"));
        assert!(model.set_macro_label(5, "x").is_err());
    }

    #[test]
    fn the_scan_warns_about_dead_routes_and_unfed_filters() {
        let mut model = template();
        assert_eq!(scan(&model).unwrap(), Vec::<String>::new());
        model
            .route("lfo_1", "filter_2_cutoff", 0.25, None, None)
            .unwrap();
        model
            .route("lfo_1", "reverb_size", 0.25, None, None)
            .unwrap();
        let warnings = scan(&model).unwrap();
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings[0].contains("filter_2_on = 0"));
        assert!(warnings[1].contains("reverb_on = 0"));
        model.set("filter_2_on", Stored::Float(1.0)).unwrap();
        let warnings = scan(&model).unwrap();
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("filter 2 is on, but no oscillator")),
            "{warnings:?}"
        );
        model.set("filter_1_on", Stored::Float(1.0)).unwrap();
        assert!(
            !scan(&model)
                .unwrap()
                .iter()
                .any(|w| w.contains("filter 1 is on"))
        );
        model.set("filter_1_cutoff", Stored::Float(200.0)).unwrap();
        assert!(
            scan(&model)
                .unwrap_err()
                .to_string()
                .contains("outside 8.0 to 136.0")
        );
    }

    #[test]
    fn every_writer_keeps_the_model_loadable() {
        let mut model = empty();
        model
            .route("velocity", "osc_1_level", 0.5, None, None)
            .unwrap();
        model
            .set_wavetable(
                2,
                &Wavetable {
                    name: "x".into(),
                    author: "x".into(),
                    keyframes: vec![saw(0, 0.0)],
                },
            )
            .unwrap();
        model
            .set_lfo_shape(
                3,
                &LfoShape {
                    name: "x".into(),
                    points: vec![(0.0, 0.0), (1.0, 1.0)],
                    powers: vec![0.0, 0.0],
                    smooth: true,
                },
            )
            .unwrap();
        let written = FORMAT.write(&model).unwrap();
        let read = FORMAT.read(&written).unwrap();
        let root: Value = serde_json::from_slice(&written).unwrap();
        assert_eq!(root["settings"]["wavetables"].as_array().unwrap().len(), 3);
        assert_eq!(root["settings"]["lfos"].as_array().unwrap().len(), 8);
        assert_eq!(
            root["settings"]["modulations"].as_array().unwrap().len(),
            64
        );
        let read = read.as_any().downcast_ref::<VitalModel>().unwrap();
        scan(read).unwrap();
    }
}
