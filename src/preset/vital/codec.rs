//! The `.vital` file: one compact JSON text.
//!
//! The model keeps the parsed tree with its key order, unknown keys and
//! the structured parts as read. The reader checks only what the app needs
//! to load a file: the stamp, `settings`, the three fields of
//! `settings.sample` and both names of each modulation entry.
//!
//! A written file keeps every value bit for bit, but not the text of each
//! number: the writer prints the shortest text that reads back to the same
//! value. `1E5` becomes `100000.0`, and an integer of 1e23 becomes `1e+23`.
//! A leading UTF-8 byte order mark is skipped and not written back.

use std::any::Any;
use std::collections::HashSet;
use std::fmt;

use serde::de::{self, Deserializer, IgnoredAny, MapAccess, Visitor};
use serde_json::{Map, Number, Value};

use crate::preset::format::{
    DescribeOptions, Description, FormatError, Header, Node, PresetFormat, PresetModel, Route,
    RoutePolarity, SHOWN_TEXT_CHARS, Status, Stored,
};
use crate::preset::registry::{ParamKind, ParamRegistry};
use crate::preset::template::TemplateSource;
use crate::preset::vital::{RETAIL_ONLY_KEYS, VitalTemplate, VitalVersion, registry, writers};
use crate::synth::Synth;

/// Top-level keys the app reads.
const KNOWN_TOP: [&str; 12] = [
    "synth_version",
    "preset_name",
    "author",
    "comments",
    "preset_style",
    "macro1",
    "macro2",
    "macro3",
    "macro4",
    "settings",
    "license",
    "tuning",
];

/// The structured keys inside `settings`.
const STRUCTURED: [&str; 4] = ["sample", "modulations", "wavetables", "lfos"];

/// The style names the app offers. An empty style is allowed too.
pub(crate) const STYLES: [&str; 9] = [
    "Bass",
    "Lead",
    "Keys",
    "Pad",
    "Percussion",
    "Sequence",
    "Experiment",
    "SFX",
    "Template",
];

/// Modulation slots of a patch.
pub(crate) const SLOTS: usize = 64;

/// The numeric controls each modulation slot owns.
const SLOT_CONTROLS: [&str; 5] = ["amount", "bipolar", "power", "stereo", "bypass"];

/// The `.vital` format.
///
/// A read parses the whole JSON tree. Peak memory depends on the shape of
/// the file: a file of long base64 texts takes about 3 times its size, and
/// a file of short numbers up to about 38 times. Measured in a release
/// build: 60 MB of `0,` items peaked at 2.28 GB, 60 MB in one text at
/// 184 MB.
#[derive(Debug)]
pub struct VitalFormat {
    max_bytes: usize,
}

impl VitalFormat {
    /// A format whose reader refuses inputs larger than `max_bytes`.
    #[must_use]
    pub const fn new(max_bytes: usize) -> Self {
        Self { max_bytes }
    }

    fn check_size(&self, bytes: &[u8]) -> Result<(), FormatError> {
        if bytes.len() > self.max_bytes {
            return Err(read_error(
                format!(
                    "the file has {} bytes, more than the limit of {}",
                    bytes.len(),
                    self.max_bytes
                ),
                "Check that the file is a Vital preset",
            ));
        }
        Ok(())
    }
}

fn read_error(cause: impl Into<String>, fix: impl Into<String>) -> FormatError {
    FormatError::new("cannot read the .vital preset", cause, fix)
}

/// The depth `serde_json` stops at.
const MAX_DEPTH: usize = 128;

fn json_error(error: &serde_json::Error) -> FormatError {
    // serde_json has no error kind for depth; its message is the only sign.
    if error.to_string().starts_with("recursion limit exceeded") {
        return read_error(
            format!(
                "the JSON nests deeper than {MAX_DEPTH} levels at line {} column {}",
                error.line(),
                error.column()
            ),
            "A Vital preset nests a few levels deep; pass a preset saved by Vital",
        );
    }
    read_error(
        format!("the text is not valid JSON: {error}"),
        "Save the preset again from Vital, or fix the JSON at that position",
    )
}

/// The bytes after a UTF-8 byte order mark, which some editors add.
fn without_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)
}

impl PresetFormat for VitalFormat {
    fn synth(&self) -> Synth {
        Synth::Vital
    }

    fn extension(&self) -> &'static str {
        Synth::Vital.extension()
    }

    fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    fn read(&self, bytes: &[u8]) -> Result<Box<dyn PresetModel>, FormatError> {
        self.check_size(bytes)?;
        let value: Value =
            serde_json::from_slice(without_bom(bytes)).map_err(|error| json_error(&error))?;
        let Value::Object(root) = value else {
            return Err(read_error(
                "the top level is not a JSON object",
                "Pass a .vital preset file",
            ));
        };
        validate(&root)?;
        Ok(Box::new(VitalModel { root }))
    }

    fn read_header(&self, bytes: &[u8]) -> Result<Header, FormatError> {
        self.check_size(bytes)?;
        let mut deserializer = serde_json::Deserializer::from_slice(without_bom(bytes));
        let parts = deserializer
            .deserialize_map(PartsVisitor)
            .and_then(|parts| deserializer.end().map(|()| parts))
            .map_err(|error| json_error(&error))?;
        Ok(parts.header())
    }

    fn write(&self, model: &dyn PresetModel) -> Result<Vec<u8>, FormatError> {
        let model = model.as_any().downcast_ref::<VitalModel>().ok_or_else(|| {
            FormatError::new(
                "cannot write the .vital preset",
                "the model belongs to another synth",
                "Write it with the format of its own synth",
            )
        })?;
        // A model can lose a required key, as `reset("sample")` does; the
        // app throws on such a file, so it is never written.
        validate(&model.root)?;
        serde_json::to_vec(&model.root).map_err(|error| {
            FormatError::new(
                "cannot write the .vital preset",
                error.to_string(),
                "Report this; a decoded model always encodes",
            )
        })
    }

    fn absent_means_default(&self, _key: &str) -> bool {
        true
    }

    fn template(&self) -> &dyn TemplateSource {
        &VitalTemplate
    }

    fn route_sources(&self) -> &'static [&'static str] {
        &writers::ROUTE_SOURCES
    }

    fn default_polarity(&self, source: &str) -> RoutePolarity {
        writers::default_polarity(source)
    }

    fn check_value(&self, _key: &str, _stored: f64) -> Result<(), FormatError> {
        // The registry range covers every effect order code.
        Ok(())
    }

    fn scan(&self, model: &dyn PresetModel, edited: &[&str]) -> Result<Vec<String>, FormatError> {
        let model = model.as_any().downcast_ref::<VitalModel>().ok_or_else(|| {
            FormatError::new(
                "cannot check the .vital preset",
                "the model belongs to another synth",
                "Check it with the format of its own synth",
            )
        })?;
        validate(&model.root)?;
        writers::scan(model, edited)
    }

    fn factory_folder(&self) -> &'static [&'static str] {
        &["Vital", "Factory"]
    }
}

/// Checks what the app needs to load a file without throwing.
fn validate(root: &Map<String, Value>) -> Result<(), FormatError> {
    match root.get("synth_version") {
        Some(Value::String(_)) => {}
        Some(_) => {
            return Err(read_error(
                "`synth_version` is not a text",
                "Write the build stamp as a text such as \"1.5.5\"",
            ));
        }
        None => {
            return Err(read_error(
                "the file has no `synth_version`",
                "Add the build stamp, such as \"synth_version\": \"1.0.6\"",
            ));
        }
    }
    let Some(settings) = root.get("settings") else {
        return Err(read_error(
            "the file has no `settings`",
            "Save the preset again from Vital; every preset has a settings object",
        ));
    };
    let Value::Object(settings) = settings else {
        return Err(read_error(
            "`settings` is not a JSON object",
            "Save the preset again from Vital",
        ));
    };
    let Some(Value::Object(sample)) = settings.get("sample") else {
        return Err(read_error(
            "`settings.sample` is missing or not an object",
            "Add a sample object with `length`, `sample_rate` and `samples`, or start from the built-in template",
        ));
    };
    for (field, is_ok) in [
        ("length", sample.get("length").is_some_and(Value::is_number)),
        (
            "sample_rate",
            sample.get("sample_rate").is_some_and(Value::is_number),
        ),
        (
            "samples",
            sample.get("samples").is_some_and(Value::is_string),
        ),
    ] {
        if !is_ok {
            return Err(read_error(
                format!("`settings.sample.{field}` is missing or has the wrong type"),
                "Vital throws on load without it; save the preset again from Vital",
            ));
        }
    }
    if let Some(modulations) = settings.get("modulations") {
        let Value::Array(modulations) = modulations else {
            return Err(read_error(
                "`settings.modulations` is not a list",
                "Write a list of {source, destination} objects",
            ));
        };
        for (slot, entry) in modulations.iter().enumerate() {
            for field in ["source", "destination"] {
                if !entry.get(field).is_some_and(Value::is_string) {
                    return Err(read_error(
                        format!("`settings.modulations[{slot}].{field}` is missing or not a text"),
                        "Vital throws on load without it; write an empty text for an unused slot",
                    ));
                }
            }
        }
    }
    Ok(())
}

/// The inputs of the header, read the same way from a model and from raw
/// bytes.
#[derive(Debug, Default)]
struct HeaderParts {
    version: Option<String>,
    name: Option<String>,
    author: Option<String>,
    style: Option<String>,
    top_keys: Vec<String>,
    settings_keys: Vec<String>,
}

/// A JSON value as header text: a string as it is, anything else as JSON.
fn text_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

impl HeaderParts {
    fn from_root(root: &Map<String, Value>) -> Self {
        let text = |key: &str| root.get(key).map(text_of);
        Self {
            version: text("synth_version"),
            name: text("preset_name"),
            author: text("author"),
            style: text("preset_style"),
            top_keys: root.keys().cloned().collect(),
            settings_keys: match root.get("settings") {
                Some(Value::Object(settings)) => settings.keys().cloned().collect(),
                _ => Vec::new(),
            },
        }
    }

    fn header(&self) -> Header {
        let mut header = Header::default();
        let (stamp, status) = match &self.version {
            None => ("(missing)".to_owned(), Status::Bad),
            Some(text) => match text.parse::<VitalVersion>() {
                Err(_) => (text.clone(), Status::Bad),
                Ok(version) if version.major < 1 => (text.clone(), Status::Warn),
                Ok(_) if self.settings_keys.iter().any(|key| key == "sub_octave") => {
                    (text.clone(), Status::Warn)
                }
                Ok(_) => (text.clone(), Status::Ok),
            },
        };
        header.push("build", stamp, Some(status));
        let or_none = |value: &Option<String>| match value.as_deref() {
            None | Some("") => "(none)".to_owned(),
            Some(text) => text.to_owned(),
        };
        header.push("name", or_none(&self.name), None);
        header.push("author", or_none(&self.author), None);
        header.push("style", or_none(&self.style), None);
        let known = registry().ok();
        let controls = self
            .settings_keys
            .iter()
            .filter(|key| known.is_some_and(|registry| registry.get(key).is_some()))
            .count();
        header.push("controls", controls.to_string(), None);
        let unknown = unknown(known, &self.top_keys, &self.settings_keys);
        let status = if unknown.is_empty() {
            Status::Ok
        } else {
            Status::Warn
        };
        header.push("unknown keys", unknown.len().to_string(), Some(status));
        let retail: Vec<&str> = RETAIL_ONLY_KEYS
            .into_iter()
            .filter(|key| self.settings_keys.iter().any(|have| have == key))
            .collect();
        if !retail.is_empty() {
            header.push("retail-only keys", retail.join(", "), Some(Status::Warn));
        }
        header
    }
}

/// Keys outside the registry and the known structure, as paths.
fn unknown(
    registry: Option<&crate::preset::registry::Registry>,
    top_keys: &[String],
    settings_keys: &[String],
) -> Vec<String> {
    let top = top_keys
        .iter()
        .filter(|key| !KNOWN_TOP.contains(&key.as_str()))
        .cloned();
    let settings = settings_keys
        .iter()
        .filter(|key| {
            !STRUCTURED.contains(&key.as_str())
                && registry.is_none_or(|registry| registry.get(key).is_none())
        })
        .map(|key| format!("settings.{key}"));
    top.chain(settings).collect()
}

/// Reads the header parts and skips every value it does not need, so a
/// large embedded sample is scanned once and never copied.
struct PartsVisitor;

impl<'de> Visitor<'de> for PartsVisitor {
    type Value = HeaderParts;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a .vital preset object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<HeaderParts, A::Error> {
        let mut parts = HeaderParts::default();
        let mut seen = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "synth_version" => parts.version = Some(text_of(&map.next_value()?)),
                "preset_name" => parts.name = Some(text_of(&map.next_value()?)),
                "author" => parts.author = Some(text_of(&map.next_value()?)),
                "preset_style" => parts.style = Some(text_of(&map.next_value()?)),
                "settings" => parts.settings_keys = map.next_value::<SettingsKeys>()?.0,
                _ => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
            if seen.insert(key.clone()) {
                parts.top_keys.push(key);
            }
        }
        Ok(parts)
    }
}

/// The keys of `settings`, with every value skipped.
struct SettingsKeys(Vec<String>);

impl<'de> de::Deserialize<'de> for SettingsKeys {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct KeysVisitor;

        impl<'de> Visitor<'de> for KeysVisitor {
            type Value = SettingsKeys;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("the settings object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<SettingsKeys, A::Error> {
                let mut keys: Vec<String> = Vec::new();
                let mut seen = HashSet::new();
                while let Some(key) = map.next_key::<String>()? {
                    map.next_value::<IgnoredAny>()?;
                    if seen.insert(key.clone()) {
                        keys.push(key);
                    }
                }
                Ok(SettingsKeys(keys))
            }

            fn visit_unit<E: de::Error>(self) -> Result<SettingsKeys, E> {
                Ok(SettingsKeys(Vec::new()))
            }
        }

        deserializer.deserialize_any(KeysVisitor)
    }
}

/// A decoded `.vital` preset.
///
/// Duplicate keys in one object keep the first position and the last
/// value, as the JSON reader does.
#[derive(Clone, Debug, PartialEq)]
pub struct VitalModel {
    root: Map<String, Value>,
}

impl VitalModel {
    /// The JSON tree as read, in file order.
    #[must_use]
    pub fn root(&self) -> &Map<String, Value> {
        &self.root
    }

    fn settings(&self) -> Option<&Map<String, Value>> {
        self.root.get("settings").and_then(Value::as_object)
    }

    fn settings_mut(&mut self) -> Result<&mut Map<String, Value>, FormatError> {
        self.root
            .get_mut("settings")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| {
                FormatError::new(
                    "cannot edit the .vital preset",
                    "the model has no `settings` object",
                    "Read the preset again from a valid file",
                )
            })
    }

    pub(crate) fn settings_map_mut(&mut self) -> Result<&mut Map<String, Value>, FormatError> {
        self.settings_mut()
    }

    pub(crate) fn set_top_text(&mut self, key: &str, text: &str) {
        self.set_text(key, text);
    }

    fn set_text(&mut self, key: &str, text: &str) {
        self.root
            .insert(key.to_owned(), Value::String(text.to_owned()));
    }

    fn number(&self, key: &str) -> Option<f64> {
        self.settings()?.get(key)?.as_f64()
    }
}

fn stored_of(value: &Value) -> Option<Stored> {
    match value {
        Value::Number(number) => Some(match number.as_i64() {
            Some(int) if !number.is_f64() => Stored::Int(int),
            _ => Stored::Float(number.as_f64().unwrap_or(f64::NAN)),
        }),
        Value::Bool(flag) => Some(Stored::Flag(*flag)),
        Value::String(text) => Some(Stored::Text(text.clone())),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

fn edit_error(cause: impl Into<String>, fix: impl Into<String>) -> FormatError {
    FormatError::new("cannot edit the .vital preset", cause, fix)
}

/// Refuses a route without a source or a destination, and one into an
/// on-off control: a switch, or the bipolar, stereo or bypass flag of a
/// slot. Factory presets of Vital 1.0.x route into numbers, counts, menus
/// such as `osc_1_transpose` and `lfo_1_tempo`, and the amount and power
/// of other slots, and never into an on-off control.
fn check_route(slot: usize, route: &Route) -> Result<(), FormatError> {
    if route.source.is_empty() || route.destination.is_empty() {
        return Err(edit_error(
            format!(
                "slot {}: a route needs a source and a destination",
                slot + 1
            ),
            "Name both, or clear the slot",
        ));
    }
    let target = registry()
        .ok()
        .and_then(|registry| registry.get(&route.destination));
    let Some(spec) = target.filter(|spec| {
        spec.kind == ParamKind::Flag
            || (spec.key.starts_with("modulation_")
                && ["_bipolar", "_stereo", "_bypass"]
                    .iter()
                    .any(|flag| spec.key.ends_with(flag)))
    }) else {
        return Ok(());
    };
    Err(edit_error(
        format!(
            "slot {}: `{}` is an on-off control and takes no modulation",
            slot + 1,
            spec.key
        ),
        "Route into a number, count or menu control, or into the amount or power of another slot",
    ))
}

/// A JSON number for a finite control value.
fn number_value(key: &str, value: &Stored) -> Result<Value, FormatError> {
    match value {
        Stored::Float(number) => Number::from_f64(*number).map(Value::Number).ok_or_else(|| {
            edit_error(
                format!("{key}: {number} is not a finite number"),
                "Write a finite number; JSON cannot hold NaN or an infinity",
            )
        }),
        Stored::Int(number) => Ok(Value::from(*number)),
        Stored::Flag(_) | Stored::Text(_) => Err(edit_error(
            format!("{key}: a control holds a number, not {value}"),
            "Write a number; Vital throws on a flag or a text in a control",
        )),
    }
}

impl PresetModel for VitalModel {
    fn header(&self) -> Header {
        HeaderParts::from_root(&self.root).header()
    }

    fn get(&self, key: &str) -> Option<Stored> {
        self.settings()?.get(key).and_then(stored_of)
    }

    fn set(&mut self, key: &str, value: Stored) -> Result<(), FormatError> {
        if STRUCTURED.contains(&key) {
            return Err(edit_error(
                format!("`{key}` is a structured part, not a control"),
                "Use the writer for that part",
            ));
        }
        let value = number_value(key, &value)?;
        self.settings_mut()?.insert(key.to_owned(), value);
        Ok(())
    }

    fn reset(&mut self, key: &str) {
        if let Ok(settings) = self.settings_mut() {
            settings.shift_remove(key);
        }
    }

    fn routes(&self) -> Vec<Option<Route>> {
        let entries = self
            .settings()
            .and_then(|settings| settings.get("modulations"))
            .and_then(Value::as_array);
        (0..SLOTS)
            .map(|slot| {
                let entry = entries?.get(slot)?;
                let source = entry.get("source")?.as_str()?;
                let destination = entry.get("destination")?.as_str()?;
                if source.is_empty() || destination.is_empty() {
                    return None;
                }
                let control = |name: &str| format!("modulation_{}_{name}", slot + 1);
                let bipolar = self.number(&control("bipolar")).unwrap_or(0.0) >= 0.5;
                let extras = ["power", "stereo", "bypass"]
                    .into_iter()
                    .filter_map(|name| {
                        let key = control(name);
                        self.get(&key).map(|value| (key, value))
                    })
                    .collect();
                Some(Route {
                    source: source.to_owned(),
                    destination: destination.to_owned(),
                    amount: self.number(&control("amount")).unwrap_or(0.0),
                    polarity: if bipolar {
                        RoutePolarity::Bipolar
                    } else {
                        RoutePolarity::Unipolar
                    },
                    extras,
                })
            })
            .collect()
    }

    fn set_route(&mut self, slot: usize, route: Option<Route>) -> Result<(), FormatError> {
        if slot >= SLOTS {
            return Err(edit_error(
                format!("modulation slot {} does not exist", slot + 1),
                format!("Vital has {SLOTS} slots; use one from 1 to {SLOTS}"),
            ));
        }
        let control = |name: &str| format!("modulation_{}_{name}", slot + 1);
        // The same source and destination edit the slot in place: its
        // controls and the extra fields of its entry, such as a drawn
        // `line_mapping` curve, stay. Any other route starts from the
        // defaults, so a new route never inherits a muted or curved slot.
        let current = self.routes().get(slot).cloned().flatten();
        let in_place = matches!((&current, &route), (Some(have), Some(new))
            if have.source == new.source && have.destination == new.destination);
        // Every slot control is written, so a cleared slot holds its
        // defaults and the bipolar flag never relies on the GUI rule.
        let mut numbers: Vec<(String, Value)> = SLOT_CONTROLS
            .into_iter()
            .map(|name| {
                let key = control(name);
                let default = registry()
                    .ok()
                    .and_then(|registry| registry.get(&key))
                    .map_or(0.0, |spec| spec.default);
                let kept = in_place.then(|| self.number(&key)).flatten();
                (key, Value::from(kept.unwrap_or(default)))
            })
            .collect();
        let mut put = |key: String, value: Value| {
            if let Some(slot) = numbers.iter_mut().find(|(have, _)| *have == key) {
                slot.1 = value;
            }
        };
        let (source, destination) = match &route {
            Some(route) => {
                check_route(slot, route)?;
                put(
                    control("amount"),
                    number_value(&control("amount"), &Stored::Float(route.amount))?,
                );
                let bipolar = Stored::Float(match route.polarity {
                    RoutePolarity::Bipolar => 1.0,
                    RoutePolarity::Unipolar => 0.0,
                });
                put(
                    control("bipolar"),
                    number_value(&control("bipolar"), &bipolar)?,
                );
                for (key, value) in &route.extras {
                    if !["power", "stereo", "bypass"]
                        .iter()
                        .any(|name| *key == control(name))
                    {
                        return Err(edit_error(
                            format!("slot {}: `{key}` is not a field of this slot", slot + 1),
                            format!(
                                "Use {}, {} or {}",
                                control("power"),
                                control("stereo"),
                                control("bypass")
                            ),
                        ));
                    }
                    put(key.clone(), number_value(key, value)?);
                }
                (route.source.clone(), route.destination.clone())
            }
            None => (String::new(), String::new()),
        };
        let settings = self.settings_mut()?;
        let modulations = settings
            .entry("modulations")
            .or_insert_with(|| Value::Array(Vec::new()));
        let Value::Array(modulations) = modulations else {
            return Err(edit_error(
                "`settings.modulations` is not a list",
                "Read the preset again from a valid file",
            ));
        };
        while modulations.len() < SLOTS {
            modulations.push(serde_json::json!({"source": "", "destination": ""}));
        }
        match (&mut modulations[slot], in_place) {
            (Value::Object(entry), true) => {
                entry.insert("source".to_owned(), Value::String(source));
                entry.insert("destination".to_owned(), Value::String(destination));
            }
            (entry, _) => {
                let mut fresh = Map::new();
                fresh.insert("source".to_owned(), Value::String(source));
                fresh.insert("destination".to_owned(), Value::String(destination));
                *entry = Value::Object(fresh);
            }
        }
        for (key, value) in numbers {
            settings.insert(key, value);
        }
        Ok(())
    }

    fn set_name(&mut self, name: &str) -> Result<(), FormatError> {
        self.set_text("preset_name", name);
        Ok(())
    }

    fn set_author(&mut self, author: &str) -> Result<(), FormatError> {
        self.set_text("author", author);
        Ok(())
    }

    fn set_description(&mut self, description: &str) -> Result<(), FormatError> {
        self.set_text("comments", description);
        Ok(())
    }

    fn set_tags(&mut self, tags: &[String]) -> Result<(), FormatError> {
        match tags {
            [] => {
                self.set_text("preset_style", "");
                Ok(())
            }
            [style] if STYLES.contains(&style.as_str()) => {
                self.set_text("preset_style", style);
                Ok(())
            }
            _ => Err(edit_error(
                format!("Vital takes one style, got {tags:?}"),
                format!("Use one of: {}", STYLES.join(", ")),
            )),
        }
    }

    fn describe(&self, opts: &DescribeOptions) -> Description {
        let registry = registry().ok();
        let root = self
            .root
            .iter()
            .map(|(key, value)| {
                let node = match (key.as_str(), value) {
                    ("settings", Value::Object(settings)) => Node::Map(
                        settings
                            .iter()
                            .map(|(key, value)| {
                                // A retail-only part such as `custom_warps`
                                // is a structure; the header names it.
                                let retail_part = RETAIL_ONLY_KEYS.contains(&key.as_str())
                                    && (value.is_array() || value.is_object());
                                let mark = if retail_part {
                                    None
                                } else {
                                    control_mark(registry, key, value)
                                };
                                (key.clone(), marked(value, mark, opts))
                            })
                            .collect(),
                    ),
                    _ if !KNOWN_TOP.contains(&key.as_str()) => {
                        marked(value, Some("unknown key".to_owned()), opts)
                    }
                    _ => describe_value(value, opts),
                };
                (key.clone(), node)
            })
            .collect();
        Description {
            root: Node::Map(root),
        }
    }

    fn unknown_keys(&self) -> Vec<String> {
        let parts = HeaderParts::from_root(&self.root);
        unknown(registry().ok(), &parts.top_keys, &parts.settings_keys)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// The mark of a control value: wrong type, out of range, legacy or
/// unknown.
fn control_mark(
    registry: Option<&crate::preset::registry::Registry>,
    key: &str,
    value: &Value,
) -> Option<String> {
    if STRUCTURED.contains(&key) {
        return None;
    }
    let Some(spec) = registry.and_then(|registry| registry.get(key)) else {
        return Some(if RETAIL_ONLY_KEYS.contains(&key) {
            "retail-only key".to_owned()
        } else {
            "unknown key".to_owned()
        });
    };
    let Some(number) = value.as_f64() else {
        return Some("not a number; Vital throws on load".to_owned());
    };
    if !spec.accepts_saved(number) {
        let whole = if spec.kind == ParamKind::Float {
            ""
        } else {
            " whole numbers"
        };
        return Some(format!("out of range:{whole} {}", spec.range_text()));
    }
    spec.legacy.then(|| "legacy control".to_owned())
}

/// A value with a mark. A list or an object that carries a mark becomes
/// one marked line: its compact JSON in a full view, its size otherwise.
fn marked(value: &Value, mark: Option<String>, opts: &DescribeOptions) -> Node {
    let text = match (value, opts.full) {
        (Value::Array(_) | Value::Object(_), true) => {
            serde_json::to_string(value).unwrap_or_default()
        }
        (Value::Array(items), false) => format!("list of {}", items.len()),
        (Value::Object(map), false) => format!("object of {} keys", map.len()),
        _ => match describe_value(value, opts) {
            Node::Scalar { text, .. } => text,
            node => return node,
        },
    };
    match (mark, value) {
        (None, Value::Array(_) | Value::Object(_)) => describe_value(value, opts),
        (mark, _) => Node::Scalar { text, mark },
    }
}

fn describe_value(value: &Value, opts: &DescribeOptions) -> Node {
    match value {
        Value::Object(map) => Node::Map(
            map.iter()
                .map(|(key, value)| (key.clone(), describe_value(value, opts)))
                .collect(),
        ),
        Value::Array(items) => {
            let shown = if opts.full { items.len() } else { opts.shown };
            Node::List {
                len: items.len(),
                items: items
                    .iter()
                    .take(shown)
                    .map(|item| describe_value(item, opts))
                    .collect(),
            }
        }
        Value::String(text) if !opts.full && text.chars().count() > SHOWN_TEXT_CHARS => {
            Node::scalar(format!("<text of {} characters>", text.chars().count()))
        }
        other => Node::scalar(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"{"synth_version":"1.0.6","settings":{"sample":{"length":1,"sample_rate":44100,"samples":"AAA="}}}"#;

    fn read(text: &str) -> Result<Box<dyn PresetModel>, FormatError> {
        VitalFormat::new(1 << 20).read(text.as_bytes())
    }

    fn model(text: &str) -> VitalModel {
        read(text)
            .unwrap()
            .as_any()
            .downcast_ref::<VitalModel>()
            .unwrap()
            .clone()
    }

    fn write(model: &dyn PresetModel) -> String {
        String::from_utf8(VitalFormat::new(1 << 20).write(model).unwrap()).unwrap()
    }

    #[test]
    fn a_minimal_file_reads_and_writes_compact() {
        let model = model(MINIMAL);
        assert_eq!(write(&model), MINIMAL);
    }

    #[test]
    fn each_missing_part_names_the_key_and_a_fix() {
        let cases = [
            (
                r#"{"settings":{"sample":{"length":1,"sample_rate":1,"samples":""}}}"#,
                "`synth_version`",
            ),
            (r#"{"synth_version":"1.0.6"}"#, "`settings`"),
            (
                r#"{"synth_version":"1.0.6","settings":{}}"#,
                "`settings.sample`",
            ),
            (
                r#"{"synth_version":"1.0.6","settings":{"sample":{"sample_rate":1,"samples":""}}}"#,
                "`settings.sample.length`",
            ),
            (
                r#"{"synth_version":"1.0.6","settings":{"sample":{"length":1,"sample_rate":1,"samples":""},"modulations":[{"destination":"x"}]}}"#,
                "`settings.modulations[0].source`",
            ),
            ("not json", "not valid JSON"),
            ("", "not valid JSON"),
            ("[]", "not a JSON object"),
            (
                r#"{"synth_version":1,"settings":{}}"#,
                "`synth_version` is not a text",
            ),
        ];
        for (text, needle) in cases {
            let message = read(text).unwrap_err().to_string();
            assert!(
                message.starts_with("cannot read the .vital preset: "),
                "{message}"
            );
            assert!(message.contains(needle), "{needle}: {message}");
        }
    }

    #[test]
    fn deep_nesting_names_the_depth_limit() {
        for (open, close) in [("[", "]"), (r#"{"a":"#, "}")] {
            let deep = format!("{}1{}", open.repeat(100_000), close.repeat(100_000));
            let text = format!(
                r#"{{"synth_version":"1.0.6","settings":{{"x":{deep},"sample":{{"length":1,"sample_rate":44100,"samples":"AAA="}}}}}}"#
            );
            let error = read(&text).unwrap_err().to_string();
            assert!(error.contains("nests deeper than 128 levels"), "{error}");
            assert!(error.contains("pass a preset saved by Vital"), "{error}");
            // The header read skips values without recursing, so it ends
            // without a depth error.
            assert!(
                VitalFormat::new(1 << 20)
                    .read_header(text.as_bytes())
                    .is_ok()
            );
        }
    }

    #[test]
    fn lists_and_objects_in_control_slots_are_marked() {
        let text = r#"{"synth_version":"1.0.6","zz_top":{"a":1},"settings":{"filter_1_cutoff":[1,2],"osc_1_level":{"x":1},"zz":[3],"sample":{"length":1,"sample_rate":44100,"samples":"AAA="}}}"#;
        let shown = model(text)
            .describe(&DescribeOptions::default())
            .to_string();
        for line in [
            "filter_1_cutoff: list of 2  (not a number; Vital throws on load)",
            "osc_1_level: object of 1 keys  (not a number; Vital throws on load)",
            "zz: list of 1  (unknown key)",
            "zz_top: object of 1 keys  (unknown key)",
        ] {
            assert!(shown.contains(line), "{line}\n{shown}");
        }
        assert!(shown.contains("sample:\n"), "{shown}");
        let full = model(text)
            .describe(&DescribeOptions {
                full: true,
                shown: 0,
            })
            .to_string();
        assert!(
            full.contains("filter_1_cutoff: [1,2]  (not a number"),
            "{full}"
        );
    }

    #[test]
    fn number_text_is_canonical_and_values_are_kept() {
        let text = MINIMAL.replace(
            r#""length":1"#,
            r#""length":1E5,"x":100000000000000000000000"#,
        );
        let written = write(read(&text).unwrap().as_ref());
        assert!(
            written.contains(r#""length":100000.0,"x":1e+23"#),
            "{written}"
        );
        assert_eq!(model(&written), model(&text));
    }

    #[test]
    fn a_byte_order_mark_is_skipped() {
        let text = format!("\u{feff}{MINIMAL}");
        assert_eq!(write(read(&text).unwrap().as_ref()), MINIMAL);
        let header = VitalFormat::new(1 << 20)
            .read_header(text.as_bytes())
            .unwrap();
        assert_eq!(header.fields[0].value, "1.0.6");
    }

    #[test]
    fn a_header_read_of_many_keys_is_linear() {
        let keys = (0..200_000)
            .map(|i| format!(r#""k{i}":0,"#))
            .collect::<Vec<_>>()
            .concat();
        let text = format!(
            r#"{{{keys}"synth_version":"1.0.6","settings":{{{keys}"sample":{{"length":1,"sample_rate":44100,"samples":"AAA="}}}}}}"#
        );
        let start = std::time::Instant::now();
        let header = VitalFormat::new(64 << 20)
            .read_header(text.as_bytes())
            .unwrap();
        // A quadratic scan of 200,000 keys took 15 s; a linear one takes
        // well under a second, even in a debug build.
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
        let unknown = header
            .fields
            .iter()
            .find(|field| field.label == "unknown keys")
            .unwrap();
        assert_eq!(unknown.value, "400000");
    }

    #[test]
    fn the_size_cap_applies_to_both_reads() {
        let format = VitalFormat::new(10);
        assert!(
            format
                .read(MINIMAL.as_bytes())
                .unwrap_err()
                .to_string()
                .contains("limit of 10")
        );
        assert!(format.read_header(MINIMAL.as_bytes()).is_err());
    }

    #[test]
    fn a_declared_length_allocates_nothing() {
        // 2^28 float32 samples are 1 GiB. The reader keeps the sample as
        // text and never sizes a buffer from `length`.
        let text = r#"{"synth_version":"1.0.6","settings":{"sample":{"length":268435456,"sample_rate":44100,"samples":"AAA="}}}"#;
        assert_eq!(write(read(text).unwrap().as_ref()), text);
    }

    #[test]
    fn hostile_values_read_as_written_or_fail() {
        let wrap = |settings: &str| {
            format!(
                r#"{{"synth_version":"1.0.6","settings":{{{settings}"sample":{{"length":1,"sample_rate":44100,"samples":"AAA="}}}}}}"#
            )
        };
        for bad in [
            wrap(r#""filter_1_cutoff":1e400,"#),
            wrap(r#""x":"\ud800","#),
            "[1]".to_owned(),
            "\"x\"".to_owned(),
        ] {
            assert!(read(&bad).is_err(), "{bad}");
        }
        let negative_zero = model(&wrap(r#""filter_1_cutoff":-0.0,"#));
        assert_eq!(
            negative_zero
                .get("filter_1_cutoff")
                .map(|value| match value {
                    Stored::Float(number) => number.to_bits(),
                    _ => 0,
                }),
            Some((-0.0_f64).to_bits())
        );
        let duplicate = model(&wrap(r#""filter_1_cutoff":1.0,"filter_1_cutoff":2.0,"#));
        assert_eq!(duplicate.get("filter_1_cutoff"), Some(Stored::Float(2.0)));
        let slots = (0..65)
            .map(|_| r#"{"source":"","destination":""}"#)
            .collect::<Vec<_>>()
            .join(",");
        let long = model(&wrap(&format!(r#""modulations":[{slots}],"#)));
        assert_eq!(write(&long), wrap(&format!(r#""modulations":[{slots}],"#)));
    }

    #[test]
    fn unknown_keys_and_order_survive() {
        let text = r#"{"zeta":1,"synth_version":"1.5.5","settings":{"zz_new":2.5,"sample":{"length":1,"sample_rate":44100,"samples":"AAA="},"custom_warps":[],"filter_1_cutoff":60.0},"alpha":"x"}"#;
        let model = model(text);
        assert_eq!(write(&model), text);
        assert_eq!(
            model.unknown_keys(),
            ["zeta", "alpha", "settings.zz_new", "settings.custom_warps"]
        );
    }

    #[test]
    fn set_accepts_finite_numbers_only() {
        let mut model = model(MINIMAL);
        let before = model.clone();
        assert!(model.set("filter_1_cutoff", Stored::Flag(true)).is_err());
        assert!(
            model
                .set("filter_1_cutoff", Stored::Float(f64::NAN))
                .is_err()
        );
        assert!(
            model
                .set("filter_1_cutoff", Stored::Text("60".into()))
                .is_err()
        );
        assert!(model.set("sample", Stored::Float(1.0)).is_err());
        assert_eq!(model, before);
        model.set("filter_1_cutoff", Stored::Float(72.5)).unwrap();
        model.set("osc_1_transpose", Stored::Int(-12)).unwrap();
        assert_eq!(model.get("filter_1_cutoff"), Some(Stored::Float(72.5)));
        assert_eq!(model.get("osc_1_transpose"), Some(Stored::Int(-12)));
        assert!(write(&model).ends_with(r#""filter_1_cutoff":72.5,"osc_1_transpose":-12}}"#));
        model.reset("filter_1_cutoff");
        assert_eq!(model.get("filter_1_cutoff"), None);
    }

    #[test]
    fn get_keeps_absent_and_explicit_default_apart() {
        let model = model(
            r#"{"synth_version":"1.0.6","settings":{"filter_1_cutoff":60.0,"sample":{"length":1,"sample_rate":44100,"samples":"AAA="}}}"#,
        );
        assert_eq!(model.get("filter_1_cutoff"), Some(Stored::Float(60.0)));
        assert_eq!(model.get("filter_2_cutoff"), None);
    }

    #[test]
    fn header_read_matches_the_model_header() {
        let text = r#"{"synth_version":"1.5.5","preset_name":"Bass","author":"a","preset_style":"Bass","extra":{"x":[1,2]},"settings":{"filter_1_cutoff":60.0,"custom_warps":[],"nope":1,"sample":{"length":1,"sample_rate":44100,"samples":"AAA="}}}"#;
        let header = VitalFormat::new(1 << 20)
            .read_header(text.as_bytes())
            .unwrap();
        assert_eq!(header, model(text).header());
        let fields: Vec<(&str, &str)> = header
            .fields
            .iter()
            .map(|field| (field.label.as_str(), field.value.as_str()))
            .collect();
        assert_eq!(
            fields,
            [
                ("build", "1.5.5"),
                ("name", "Bass"),
                ("author", "a"),
                ("style", "Bass"),
                ("controls", "1"),
                ("unknown keys", "3"),
                ("retail-only keys", "custom_warps"),
            ]
        );
    }

    #[test]
    fn a_header_read_skips_a_large_sample() {
        let samples = "A".repeat(32 << 20);
        let text = format!(
            r#"{{"synth_version":"1.0.6","settings":{{"sample":{{"length":1,"sample_rate":44100,"samples":"{samples}"}},"modulations":[]}}}}"#
        );
        let header = VitalFormat::new(64 << 20)
            .read_header(text.as_bytes())
            .unwrap();
        assert_eq!(header.fields[0].value, "1.0.6");
        assert!(
            VitalFormat::new(64 << 20)
                .read_header(&text.as_bytes()[..100])
                .is_err()
        );
    }

    #[test]
    fn old_stamps_and_sub_octave_warn() {
        let old = r#"{"synth_version":"0.9.0","settings":{"sample":{"length":1,"sample_rate":44100,"samples":"AAA="}}}"#;
        assert_eq!(model(old).header().fields[0].status, Some(Status::Warn));
        let sub = r#"{"synth_version":"1.0.6","settings":{"sub_octave":1,"sample":{"length":1,"sample_rate":44100,"samples":"AAA="}}}"#;
        assert_eq!(model(sub).header().fields[0].status, Some(Status::Warn));
        let bad = r#"{"synth_version":"1.5","settings":{"sample":{"length":1,"sample_rate":44100,"samples":"AAA="}}}"#;
        assert_eq!(model(bad).header().fields[0].status, Some(Status::Bad));
    }

    #[test]
    fn describe_marks_odd_controls_and_elides() {
        let text = r#"{"synth_version":"1.0.6","settings":{"filter_1_cutoff":200.0,"filter_1_model":2.5,"osc_1_level":"x","sub_on":0.0,"zz":1,"sample":{"length":1,"sample_rate":44100,"samples":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"},"modulations":[{"source":"","destination":""},{"source":"","destination":""},{"source":"","destination":""},{"source":"","destination":""}]}}"#;
        let shown = model(text)
            .describe(&DescribeOptions::default())
            .to_string();
        assert!(
            shown.contains("filter_1_cutoff: 200.0  (out of range: 8.0 to 136.0)"),
            "{shown}"
        );
        assert!(
            shown.contains("filter_1_model: 2.5  (out of range: whole numbers 0.0 to 7.0)"),
            "{shown}"
        );
        assert!(
            shown.contains("osc_1_level: \"x\"  (not a number; Vital throws on load)"),
            "{shown}"
        );
        assert!(shown.contains("sub_on: 0.0  (legacy control)"), "{shown}");
        assert!(shown.contains("zz: 1  (unknown key)"), "{shown}");
        assert!(
            shown.contains("samples: <text of 72 characters>"),
            "{shown}"
        );
        assert!(shown.contains("modulations: list of 4"), "{shown}");
        assert!(shown.contains("- ... 1 more"), "{shown}");
        let full = model(text)
            .describe(&DescribeOptions {
                full: true,
                shown: 0,
            })
            .to_string();
        assert!(!full.contains("more"), "{full}");
        assert!(full.contains("\"AAAA"), "{full}");
    }

    #[test]
    fn routes_read_and_write_through_the_slot_controls() {
        let mut model = model(MINIMAL);
        assert!(model.routes().iter().all(Option::is_none));
        assert_eq!(model.routes().len(), 64);
        let route = Route {
            source: "lfo_1".into(),
            destination: "filter_1_cutoff".into(),
            amount: 0.25,
            polarity: RoutePolarity::Bipolar,
            extras: vec![("modulation_3_power".into(), Stored::Float(-2.0))],
        };
        model.set_route(2, Some(route.clone())).unwrap();
        let mut expected = route;
        expected.extras.extend([
            ("modulation_3_stereo".to_owned(), Stored::Float(0.0)),
            ("modulation_3_bypass".to_owned(), Stored::Float(0.0)),
        ]);
        assert_eq!(model.routes()[2], Some(expected));
        assert_eq!(model.get("modulation_3_amount"), Some(Stored::Float(0.25)));
        assert_eq!(model.get("modulation_3_bipolar"), Some(Stored::Float(1.0)));
        let written = write(&model);
        assert_eq!(written.matches("\"source\"").count(), 64);
        model.set_route(2, None).unwrap();
        assert!(model.routes()[2].is_none());
        assert_eq!(model.get("modulation_3_amount"), Some(Stored::Float(0.0)));
        assert_eq!(model.get("modulation_3_power"), Some(Stored::Float(0.0)));
        assert!(
            model
                .set_route(64, None)
                .unwrap_err()
                .to_string()
                .contains("64 slots")
        );
        let bad_extra = Route {
            source: "lfo_1".into(),
            destination: "filter_1_cutoff".into(),
            amount: 0.1,
            polarity: RoutePolarity::Unipolar,
            extras: vec![("modulation_9_power".into(), Stored::Float(1.0))],
        };
        assert!(model.set_route(0, Some(bad_extra)).is_err());
    }

    #[test]
    fn a_model_without_a_required_key_does_not_write() {
        let mut model = model(MINIMAL);
        model.reset("sample");
        let error = VitalFormat::new(1 << 20)
            .write(&model)
            .unwrap_err()
            .to_string();
        assert!(error.contains("settings.sample"), "{error}");
    }

    #[test]
    fn a_new_route_clears_the_old_slot_controls() {
        let mut model = model(MINIMAL);
        let muted = Route {
            source: "lfo_1".into(),
            destination: "filter_1_cutoff".into(),
            amount: 0.5,
            polarity: RoutePolarity::Unipolar,
            extras: vec![
                ("modulation_1_power".into(), Stored::Float(-2.0)),
                ("modulation_1_stereo".into(), Stored::Float(1.0)),
                ("modulation_1_bypass".into(), Stored::Float(1.0)),
            ],
        };
        model.set_route(0, Some(muted)).unwrap();
        let plain = Route {
            source: "env_2".into(),
            destination: "osc_1_level".into(),
            amount: 0.25,
            polarity: RoutePolarity::Unipolar,
            extras: Vec::new(),
        };
        model.set_route(0, Some(plain)).unwrap();
        for name in ["power", "stereo", "bypass"] {
            assert_eq!(
                model.get(&format!("modulation_1_{name}")),
                Some(Stored::Float(0.0)),
                "{name}"
            );
        }
    }

    #[test]
    fn a_route_into_an_on_off_control_is_refused() {
        let mut model = model(MINIMAL);
        let route = |destination: &str| Route {
            source: "lfo_1".into(),
            destination: destination.into(),
            amount: 0.5,
            polarity: RoutePolarity::Unipolar,
            extras: Vec::new(),
        };
        for destination in ["osc_1_on", "modulation_3_bipolar", "modulation_3_bypass"] {
            let error = model
                .set_route(0, Some(route(destination)))
                .unwrap_err()
                .to_string();
            assert!(error.contains("is an on-off control"), "{error}");
            assert!(error.contains("number, count or menu control"), "{error}");
        }
        assert!(model.routes()[0].is_none());
        for (slot, destination) in [
            "lfo_1_tempo",
            "osc_1_transpose",
            "osc_1_unison_voices",
            "modulation_3_amount",
            "modulation_3_power",
        ]
        .into_iter()
        .enumerate()
        {
            model.set_route(slot, Some(route(destination))).unwrap();
            assert_eq!(
                model.routes()[slot]
                    .as_ref()
                    .map(|have| have.destination.as_str()),
                Some(destination)
            );
        }
    }

    #[test]
    fn names_and_style_go_to_their_keys() {
        let mut model = model(MINIMAL);
        model.set_name("Wobble").unwrap();
        model.set_author("me").unwrap();
        model.set_description("from a stem").unwrap();
        model.set_tags(&["Bass".to_owned()]).unwrap();
        assert!(
            model
                .set_tags(&["Bass".to_owned(), "Lead".to_owned()])
                .is_err()
        );
        assert!(
            model
                .set_tags(&["Wobbly".to_owned()])
                .unwrap_err()
                .to_string()
                .contains("Bass, Lead")
        );
        let written = write(&model);
        assert!(written.ends_with(r#""preset_name":"Wobble","author":"me","comments":"from a stem","preset_style":"Bass"}"#), "{written}");
    }

    #[test]
    fn integers_and_floats_keep_their_form() {
        let text = r#"{"synth_version":"1.0.6","settings":{"a":1,"b":1.0,"c":-0.0,"d":1e-7,"sample":{"length":1,"sample_rate":44100,"samples":"AAA="}}}"#;
        let model = model(text);
        assert_eq!(model.get("a"), Some(Stored::Int(1)));
        assert_eq!(model.get("b"), Some(Stored::Float(1.0)));
        assert_eq!(write(&model), text);
    }

    #[test]
    fn duplicate_keys_keep_the_last_value() {
        let model = model(
            r#"{"synth_version":"1.0.6","settings":{"filter_1_cutoff":10.0,"sample":{"length":1,"sample_rate":44100,"samples":"AAA="},"filter_1_cutoff":20.0}}"#,
        );
        assert_eq!(model.get("filter_1_cutoff"), Some(Stored::Float(20.0)));
    }
}
