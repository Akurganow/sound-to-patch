//! The edit list: the JSON an agent writes and `preset apply` reads.
//!
//! ```json
//! {"synth": "vital",
//!  "edits": [
//!    {"param": "filter_1_cutoff", "value": "410 Hz", "reason": "cutoff measured near 410 Hz"},
//!    {"route": {"source": "lfo_1", "dest": "filter_1_cutoff", "amount": "25%"}, "reason": "wobble"},
//!    {"menu": "filter_1_model", "choice": "Ladder", "reason": "warm low-pass"},
//!    {"raw": {"key": "settings_key_not_in_registry", "value": 1.0}, "reason": "newer build"}],
//!  "ambiguities": [{"id": "filter_type", "choice": "ladder", "reason": "smooth slope"}]}
//! ```
//!
//! A unit control takes a unit string such as `"1.2 kHz"`, `"800 ms"`,
//! `"-6 dB"`, `"+12 st"` or `"25%"`, never a bare number. A count takes a
//! whole number, a switch takes `true` or `false`, and a menu takes the
//! label of its choice. A control without a physical unit takes its
//! stored value as a bare number. [`parse`] reads the text strictly and
//! [`resolve`] checks every edit against the registry. Both report every
//! problem, not only the first, and each message names the edit, the
//! cause and the fix.

use std::error::Error;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::preset::format::{RoutePolarity, Stored};
use crate::preset::registry::{Law, ParamKind, ParamRegistry, ParamSpec, Physical, UnitKind};
use crate::synth::Synth;
use crate::units::{Bipolar, Percent, Semitones};

/// A parsed edit list. It serializes to the same JSON form [`parse`]
/// reads.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EditList {
    /// The synth the edits are written for.
    pub synth: Synth,
    /// The edits, in order.
    pub edits: Vec<Edit>,
    /// One decision per ambiguity of the layer report.
    pub ambiguities: Vec<Decision>,
}

/// One edit: a record and the reason for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    /// What to change.
    pub record: Record,
    /// Why. The agent path requires it; the report path fills in the layer
    /// name.
    pub reason: Option<String>,
}

/// The four record kinds.
#[derive(Clone, Debug, PartialEq)]
pub enum Record {
    /// A registry control and its value.
    Param {
        /// The registry key.
        param: String,
        /// The value as written.
        value: ParamValue,
    },
    /// A modulation route.
    Route(RouteRecord),
    /// A menu control and the label of its choice.
    Menu {
        /// The registry key.
        menu: String,
        /// The label of the choice.
        choice: String,
    },
    /// A key the registry does not cover yet, with its value as the file
    /// stores it.
    Raw {
        /// The key.
        key: String,
        /// The value.
        value: Stored,
    },
}

/// A value of a parameter record as written.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ParamValue {
    /// A switch.
    Flag(bool),
    /// A bare number: a count, or the stored value of a control without a
    /// unit.
    Number(f64),
    /// A unit string or a menu label.
    Text(String),
}

impl fmt::Display for ParamValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Flag(flag) => write!(f, "{flag}"),
            Self::Number(number) => write!(f, "{number}"),
            Self::Text(text) => write!(f, "{text:?}"),
        }
    }
}

/// A route record.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteRecord {
    /// The source name.
    pub source: String,
    /// The destination key.
    pub dest: String,
    /// The depth: a percent of the destination's range, or semitones for a
    /// destination stored in semitones.
    pub amount: String,
    /// The polarity; the adapter picks one by source when it is absent.
    #[serde(default)]
    pub polarity: Option<PolarityText>,
}

/// The polarity as an edit list writes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolarityText {
    /// One way from the value.
    Unipolar,
    /// Both ways around the value.
    Bipolar,
}

impl From<PolarityText> for RoutePolarity {
    fn from(text: PolarityText) -> Self {
        match text {
            PolarityText::Unipolar => Self::Unipolar,
            PolarityText::Bipolar => Self::Bipolar,
        }
    }
}

/// The decision on one ambiguity of the layer report.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    /// The ambiguity id from the report.
    pub id: String,
    /// The chosen candidate, or `both` to keep both for refinement.
    pub choice: String,
    /// Why.
    pub reason: String,
}

/// One problem with an edit list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditError {
    /// Where: `edit 3 (filter_1_cutoff)` or `the edit list`.
    pub place: String,
    /// What is wrong.
    pub cause: String,
    /// How to fix it.
    pub fix: String,
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}. {}", self.place, self.cause, self.fix)
    }
}

impl Error for EditError {}

/// Every problem of an edit list, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditErrors(pub Vec<EditError>);

impl fmt::Display for EditErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the edit list has {} problem(s):", self.0.len())?;
        for error in &self.0 {
            write!(f, "\n  {error}")?;
        }
        Ok(())
    }
}

impl Error for EditErrors {}

fn place(index: usize, name: &str) -> String {
    format!("edit {} ({name})", index + 1)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParamData {
    param: String,
    value: ParamValue,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RouteData {
    route: RouteRecord,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MenuData {
    menu: String,
    choice: String,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawData {
    raw: RawValue,
    #[serde(default)]
    reason: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawValue {
    key: String,
    value: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ListData {
    synth: Synth,
    edits: Vec<Value>,
    #[serde(default)]
    ambiguities: Vec<Decision>,
}

/// Whether every edit must carry a reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Path {
    /// The agent writes the list: each edit needs a reason.
    Agent,
    /// The list comes from a layer report, which supplies the reasons.
    Report,
}

/// Reads an edit list.
///
/// # Errors
///
/// [`EditErrors`] with one entry per bad record: unknown fields, a record
/// of no known kind, a wrong value type or, on the agent path, a missing
/// or empty reason.
pub fn parse(text: &str, path: Path) -> Result<EditList, EditErrors> {
    let data: ListData = serde_json::from_str(text).map_err(|error| {
        EditErrors(vec![EditError {
            place: "the edit list".to_owned(),
            cause: format!("it does not read: {error}"),
            fix: "Write one JSON object with `synth`, `edits` and an optional `ambiguities` list"
                .to_owned(),
        }])
    })?;
    let mut edits = Vec::new();
    let mut errors = Vec::new();
    for (index, value) in data.edits.into_iter().enumerate() {
        match parse_edit(&value) {
            Ok(edit) => {
                if path == Path::Agent && edit.reason.as_deref().is_none_or(|r| r.trim().is_empty())
                {
                    errors.push(EditError {
                        place: place(index, edit.record.name()),
                        cause: "the edit has no reason".to_owned(),
                        fix: "Add a `reason` that says what was heard or measured".to_owned(),
                    });
                }
                edits.push(edit);
            }
            Err((name, cause, fix)) => errors.push(EditError {
                place: place(index, &name),
                cause,
                fix,
            }),
        }
    }
    if errors.is_empty() {
        Ok(EditList {
            synth: data.synth,
            edits,
            ambiguities: data.ambiguities,
        })
    } else {
        Err(EditErrors(errors))
    }
}

type RecordError = (String, String, String);

fn parse_edit(value: &Value) -> Result<Edit, RecordError> {
    let kinds = ["param", "route", "menu", "raw"];
    let Value::Object(map) = value else {
        return Err((
            "?".to_owned(),
            format!("the edit is {value}, not an object"),
            "Write an object with one of `param`, `route`, `menu` or `raw`".to_owned(),
        ));
    };
    let found: Vec<&str> = kinds
        .into_iter()
        .filter(|kind| map.contains_key(*kind))
        .collect();
    let name = map
        .get("param")
        .or_else(|| map.get("menu"))
        .and_then(Value::as_str)
        .or_else(|| {
            map.get("raw")
                .and_then(|raw| raw.get("key"))
                .and_then(Value::as_str)
        })
        .unwrap_or_else(|| found.first().copied().unwrap_or("?"))
        .to_owned();
    let invalid = |error: serde_json::Error, example: &str| {
        (
            name.clone(),
            format!("the record does not read: {error}"),
            format!("Write it as {example}"),
        )
    };
    match found.as_slice() {
        ["param"] => serde_json::from_value::<ParamData>(value.clone())
            .map(|data| Edit {
                record: Record::Param {
                    param: data.param,
                    value: data.value,
                },
                reason: data.reason,
            })
            .map_err(|error| invalid(error, r#"{"param": "<key>", "value": "410 Hz", "reason": "..."}"#)),
        ["route"] => serde_json::from_value::<RouteData>(value.clone())
            .map(|data| Edit {
                record: Record::Route(data.route),
                reason: data.reason,
            })
            .map_err(|error| {
                invalid(
                    error,
                    r#"{"route": {"source": "lfo_1", "dest": "<key>", "amount": "25%", "polarity": "bipolar"}, "reason": "..."}"#,
                )
            }),
        ["menu"] => serde_json::from_value::<MenuData>(value.clone())
            .map(|data| Edit {
                record: Record::Menu {
                    menu: data.menu,
                    choice: data.choice,
                },
                reason: data.reason,
            })
            .map_err(|error| invalid(error, r#"{"menu": "<key>", "choice": "<label>", "reason": "..."}"#)),
        ["raw"] => {
            let data = serde_json::from_value::<RawData>(value.clone())
                .map_err(|error| invalid(error, r#"{"raw": {"key": "<key>", "value": 1.0}, "reason": "..."}"#))?;
            let stored = stored_of(&data.raw.value).ok_or_else(|| {
                (
                    name.clone(),
                    format!("the raw value {} is not a number, a flag or a text", data.raw.value),
                    "Write the value as the file stores it".to_owned(),
                )
            })?;
            Ok(Edit {
                record: Record::Raw {
                    key: data.raw.key,
                    value: stored,
                },
                reason: data.reason,
            })
        }
        [] => Err((
            name,
            "the edit has no record kind".to_owned(),
            "Add one of `param`, `route`, `menu` or `raw`".to_owned(),
        )),
        _ => Err((
            name,
            format!("the edit mixes record kinds: {}", found.join(", ")),
            "Write one record kind per edit".to_owned(),
        )),
    }
}

fn stored_of(value: &Value) -> Option<Stored> {
    match value {
        Value::Bool(flag) => Some(Stored::Flag(*flag)),
        Value::String(text) => Some(Stored::Text(text.clone())),
        Value::Number(number) => match (number.as_i64(), number.as_f64()) {
            (Some(int), _) if !number.is_f64() => Some(Stored::Int(int)),
            (_, Some(float)) => Some(Stored::Float(float)),
            _ => None,
        },
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

impl Serialize for Edit {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serde_json::Map::new();
        match &self.record {
            Record::Param { param, value: v } => {
                map.insert("param".into(), Value::String(param.clone()));
                map.insert(
                    "value".into(),
                    serde_json::to_value(v).map_err(serde::ser::Error::custom)?,
                );
            }
            Record::Route(route) => {
                map.insert(
                    "route".into(),
                    serde_json::to_value(route).map_err(serde::ser::Error::custom)?,
                );
            }
            Record::Menu { menu, choice } => {
                map.insert("menu".into(), Value::String(menu.clone()));
                map.insert("choice".into(), Value::String(choice.clone()));
            }
            Record::Raw { key, value: v } => {
                let raw = match v {
                    Stored::Float(number) => serde_json::json!(number),
                    Stored::Int(number) => serde_json::json!(number),
                    Stored::Flag(flag) => Value::Bool(*flag),
                    Stored::Text(text) => Value::String(text.clone()),
                };
                map.insert("raw".into(), serde_json::json!({"key": key, "value": raw}));
            }
        }
        if let Some(reason) = &self.reason {
            map.insert("reason".into(), Value::String(reason.clone()));
        }
        map.serialize(serializer)
    }
}

impl Record {
    /// The key, menu or route destination the record names.
    #[must_use]
    pub fn name(&self) -> &str {
        match self {
            Self::Param { param, .. } => param,
            Self::Route(route) => &route.dest,
            Self::Menu { menu, .. } => menu,
            Self::Raw { key, .. } => key,
        }
    }
}

/// An edit checked against the registry, with values in stored units.
#[derive(Clone, Debug, PartialEq)]
pub enum Resolved {
    /// A control and its stored value.
    Param {
        /// The registry key.
        key: String,
        /// The stored value, inside the registry range.
        stored: f64,
        /// The registry law is interim: the edit is uncalibrated.
        uncalibrated: bool,
        /// Why.
        reason: Option<String>,
    },
    /// A route with its amount as the file stores it.
    Route {
        /// The source name.
        source: String,
        /// The destination key.
        destination: String,
        /// The amount, -1 to 1 of the destination's stored range.
        amount: f64,
        /// The polarity, when the edit gives one.
        polarity: Option<RoutePolarity>,
        /// Why.
        reason: Option<String>,
    },
    /// A key the registry does not cover.
    Raw {
        /// The key.
        key: String,
        /// The value.
        value: Stored,
        /// Why.
        reason: Option<String>,
    },
}

/// Checks every edit against the registry of the list's synth.
///
/// # Errors
///
/// [`EditErrors`] with one entry per edit that fails: the list is for
/// another synth, an unknown key (with up to ten close names), a wrong
/// value kind, a unit string that does not parse, a value out of range
/// (with the range, the unit and the nearest valid value), an unknown
/// menu label or a raw key the registry covers.
pub fn resolve(list: &EditList, registry: &dyn ParamRegistry) -> Result<Vec<Resolved>, EditErrors> {
    if list.synth != registry.synth() {
        return Err(EditErrors(vec![EditError {
            place: "the edit list".to_owned(),
            cause: format!(
                "it is written for {}, but the target is {}",
                list.synth.name(),
                registry.synth().name()
            ),
            fix: format!(
                "Pass --synth {}, or write the list for {}",
                list.synth,
                registry.synth().name()
            ),
        }]));
    }
    let mut resolved = Vec::new();
    let mut errors = Vec::new();
    for (index, edit) in list.edits.iter().enumerate() {
        let name = edit.record.name();
        match resolve_record(&edit.record, registry) {
            Ok(mut item) => {
                match &mut item {
                    Resolved::Param { reason, .. }
                    | Resolved::Route { reason, .. }
                    | Resolved::Raw { reason, .. } => reason.clone_from(&edit.reason),
                }
                resolved.push(item);
            }
            Err((cause, fix)) => errors.push(EditError {
                place: place(index, name),
                cause,
                fix,
            }),
        }
    }
    if errors.is_empty() {
        Ok(resolved)
    } else {
        Err(EditErrors(errors))
    }
}

type Problem = (String, String);

fn spec_of<'a>(registry: &'a dyn ParamRegistry, key: &str) -> Result<&'a ParamSpec, Problem> {
    let spec = registry.get(key).ok_or_else(|| {
        let close = registry.close_names(key, 10);
        (
            format!("{key:?} is not a control of {}", registry.synth().name()),
            if close.is_empty() {
                "Check the key against the synth reference".to_owned()
            } else {
                format!("Close names: {}", close.join(", "))
            },
        )
    })?;
    if spec.legacy {
        return Err((
            format!("{key} is a legacy control that no module of the synth creates"),
            "Leave it out; writers never create legacy controls".to_owned(),
        ));
    }
    Ok(spec)
}

fn resolve_record(record: &Record, registry: &dyn ParamRegistry) -> Result<Resolved, Problem> {
    match record {
        Record::Param { param, value } => {
            let spec = spec_of(registry, param)?;
            let stored = param_stored(spec, value)?;
            Ok(Resolved::Param {
                key: spec.key.clone(),
                stored,
                uncalibrated: spec.provenance == crate::preset::registry::Provenance::Interim,
                reason: None,
            })
        }
        Record::Menu { menu, choice } => {
            let spec = spec_of(registry, menu)?;
            if spec.kind != ParamKind::Choice {
                return Err((
                    format!("{menu} is not a menu"),
                    "Use a `param` record for it".to_owned(),
                ));
            }
            Ok(Resolved::Param {
                key: spec.key.clone(),
                stored: choice_stored(spec, choice)?,
                uncalibrated: false,
                reason: None,
            })
        }
        Record::Route(route) => {
            let spec = spec_of(registry, &route.dest)?;
            Ok(Resolved::Route {
                source: route.source.clone(),
                destination: spec.key.clone(),
                amount: route_amount(spec, &route.amount)?,
                polarity: route.polarity.map(RoutePolarity::from),
                reason: None,
            })
        }
        Record::Raw { key, value } => {
            if registry.get(key).is_some() {
                return Err((
                    format!("{key} is a registry control, not a raw key"),
                    "Use a `param` or `menu` record for it".to_owned(),
                ));
            }
            Ok(Resolved::Raw {
                key: key.clone(),
                value: value.clone(),
                reason: None,
            })
        }
    }
}

/// An example unit string for the fix text of a unit.
fn example(unit: UnitKind) -> &'static str {
    match unit {
        UnitKind::Hz => "\"1.2 kHz\"",
        UnitKind::Seconds => "\"800 ms\"",
        UnitKind::Semitones => "\"+12 st\"",
        UnitKind::Cents => "\"7 ct\"",
        UnitKind::Octaves => "\"-1 oct\"",
        UnitKind::Decibels => "\"-6 dB\"",
        UnitKind::Normalized => "a number from 0 to 1",
        UnitKind::MidiPitch => "\"45 midi\"",
        UnitKind::PercentUnipolar => "\"25%\"",
        UnitKind::PercentBipolar => "\"-40%\"",
    }
}

fn param_stored(spec: &ParamSpec, value: &ParamValue) -> Result<f64, Problem> {
    let key = &spec.key;
    match (spec.kind, spec.unit(), value) {
        (ParamKind::Choice, _, ParamValue::Text(label)) => choice_stored(spec, label),
        // A stored index selects a choice whose label repeats.
        (ParamKind::Choice, _, ParamValue::Number(number)) if spec.accepts(*number) => Ok(*number),
        (ParamKind::Choice, _, _) => Err((
            format!("{key} is a menu, and {value} is not one of its labels or indices"),
            format!(
                "Write one of: {}, or a whole stored index from {}",
                spec.choices.join(", "),
                spec.range_text()
            ),
        )),
        (ParamKind::Flag, _, ParamValue::Flag(flag)) => Ok(if *flag { 1.0 } else { 0.0 }),
        (ParamKind::Flag, _, _) => Err((
            format!("{key} is a switch, and {value} is not true or false"),
            "Write true or false".to_owned(),
        )),
        (_, Some(unit), ParamValue::Text(text)) => {
            let physical = Physical::parse(unit, text).map_err(|error| {
                (
                    format!("{key}: {error}"),
                    format!("Write a value such as {}", example(unit)),
                )
            })?;
            spec.stored(&physical).map_err(|error| {
                (
                    error.to_string(),
                    format!("The control takes {}", unit.type_name()),
                )
            })
        }
        (_, Some(unit), _) => Err((
            format!("{key} takes a unit string, not {value}"),
            format!("Write a value with its unit, such as {}", example(unit)),
        )),
        (_, None, ParamValue::Number(number)) => {
            if spec.accepts(*number) {
                return Ok(*number);
            }
            let whole = spec.kind != ParamKind::Float;
            if whole && (spec.stored_min..=spec.stored_max).contains(number) {
                return Err((
                    format!("{key}: {number} is not a whole number"),
                    format!("Use a whole number; the nearest is {:?}", number.round()),
                ));
            }
            let nearest = number.clamp(spec.stored_min, spec.stored_max);
            let nearest = if whole { nearest.round() } else { nearest };
            Err((
                format!(
                    "{key}: {number} is outside the stored range {}",
                    spec.range_text()
                ),
                format!(
                    "Use a {} from {}; the nearest is {nearest:?}",
                    if whole { "whole number" } else { "number" },
                    spec.range_text()
                ),
            ))
        }
        (_, None, _) => Err((
            format!("{key} has no physical unit, and {value} is not a number"),
            format!(
                "Write the stored value as a bare number from {}",
                spec.range_text()
            ),
        )),
    }
}

fn choice_stored(spec: &ParamSpec, label: &str) -> Result<f64, Problem> {
    let matches: Vec<usize> = spec
        .choices
        .iter()
        .enumerate()
        .filter(|(_, choice)| *choice == label)
        .map(|(index, _)| index)
        .collect();
    match matches.as_slice() {
        [index] => Ok(spec.stored_min + crate::cast::usize_to_f64(*index)),
        [] => Err((
            format!("{label:?} is not a choice of {}", spec.key),
            format!("Write one of: {}", spec.choices.join(", ")),
        )),
        _ => Err((
            format!("{label:?} names several choices of {}", spec.key),
            "Write the stored index as a `param` record; the labels repeat".to_owned(),
        )),
    }
}

/// Whether the stored value of a control is a note number or a semitone
/// count, so that a semitone amount is a stored offset.
fn stored_in_semitones(spec: &ParamSpec) -> bool {
    match spec.law.as_ref().map(|law| (&law.law, law.unit)) {
        Some((Law::Exp2 { rate, .. }, UnitKind::Hz)) => (rate * 12.0 - 1.0).abs() < 1e-12,
        Some((Law::Affine { scale, offset }, UnitKind::Semitones)) => {
            (scale - 1.0).abs() < 1e-12 && offset.abs() < 1e-12
        }
        _ => false,
    }
}

fn route_amount(spec: &ParamSpec, text: &str) -> Result<f64, Problem> {
    let span = spec.stored_max - spec.stored_min;
    let semitones_ok = stored_in_semitones(spec);
    let fix = if semitones_ok {
        "Write the depth as a percent of the range, such as \"25%\", or in semitones, such as \"+12 st\""
    } else {
        "Write the depth as a percent of the destination's range, such as \"25%\""
    };
    let amount = if let Ok(percent) = text.parse::<Percent<Bipolar>>() {
        percent.get() / 100.0
    } else if let (true, Ok(semitones)) = (semitones_ok, text.parse::<Semitones>()) {
        semitones.get() / span
    } else {
        return Err((
            format!("route amount {text:?} into {} does not read", spec.key),
            fix.to_owned(),
        ));
    };
    if (-1.0..=1.0).contains(&amount) {
        Ok(amount)
    } else {
        Err((
            format!(
                "route amount {text:?} is beyond the full range of {}",
                spec.key
            ),
            format!(
                "Keep it within -100% to 100%{}",
                if semitones_ok {
                    format!(", or ±{span} st")
                } else {
                    String::new()
                }
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::registry::registry_for;
    use approx::assert_relative_eq;

    fn vital() -> &'static dyn ParamRegistry {
        registry_for(Synth::Vital).unwrap()
    }

    fn list(edits: &str) -> String {
        format!(r#"{{"synth":"vital","edits":[{edits}]}}"#)
    }

    fn resolve_one(edit: &str) -> Result<Resolved, String> {
        let parsed = parse(&list(edit), Path::Agent).map_err(|e| e.to_string())?;
        resolve(&parsed, vital())
            .map(|mut all| all.remove(0))
            .map_err(|e| e.to_string())
    }

    fn stored(edit: &str) -> f64 {
        match resolve_one(edit).unwrap() {
            Resolved::Param { stored, .. } => stored,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn each_unit_family_parses_to_its_stored_value() {
        // Hz -> note number: 12 * log2(1200 / 8.1757989156)
        assert_relative_eq!(
            stored(r#"{"param":"filter_1_cutoff","value":"1.2 kHz","reason":"r"}"#),
            12.0 * (1200.0f64 / 8.175_798_915_6).log2(),
            max_relative = 1e-12
        );
        // seconds -> fourth root: 0.8^(1/4)
        assert_relative_eq!(
            stored(r#"{"param":"env_1_attack","value":"800 ms","reason":"r"}"#),
            0.8f64.powf(0.25),
            max_relative = 1e-12
        );
        // percent -> stored / 100
        assert_relative_eq!(
            stored(r#"{"param":"osc_1_pan","value":"-40%","reason":"r"}"#),
            -0.4,
            max_relative = 1e-12
        );
        // dB -> 10^(dB / 40)
        assert_relative_eq!(
            stored(r#"{"param":"osc_1_level","value":"-6 dB","reason":"r"}"#),
            10f64.powf(-6.0 / 40.0),
            max_relative = 1e-12
        );
        assert_relative_eq!(
            stored(r#"{"param":"osc_1_transpose","value":"+12 st","reason":"r"}"#),
            12.0,
            max_relative = 1e-12
        );
        assert_relative_eq!(
            stored(r#"{"param":"osc_1_tune","value":"7 ct","reason":"r"}"#),
            0.07,
            max_relative = 1e-12
        );
        assert_relative_eq!(
            stored(r#"{"param":"osc_1_unison_voices","value":5,"reason":"r"}"#),
            5.0,
            max_relative = 1e-12
        );
        assert_relative_eq!(
            stored(r#"{"param":"filter_1_on","value":true,"reason":"r"}"#),
            1.0,
            max_relative = 1e-12
        );
        assert_relative_eq!(
            stored(r#"{"param":"lfo_1_tempo","value":"1/8","reason":"r"}"#),
            9.0,
            max_relative = 1e-12
        );
        assert_relative_eq!(
            stored(r#"{"menu":"filter_1_model","choice":"Ladder","reason":"r"}"#),
            2.0,
            max_relative = 1e-12
        );
        assert_relative_eq!(
            stored(r#"{"param":"env_1_sustain","value":0.5,"reason":"r"}"#),
            0.5,
            max_relative = 1e-12
        );
    }

    #[test]
    fn the_five_failures_name_the_parameter_cause_and_fix() {
        let bare =
            resolve_one(r#"{"param":"filter_1_cutoff","value":410,"reason":"r"}"#).unwrap_err();
        assert!(
            bare.contains("filter_1_cutoff takes a unit string") && bare.contains("\"1.2 kHz\""),
            "{bare}"
        );
        let family =
            resolve_one(r#"{"param":"env_1_attack","value":"5 Hz","reason":"r"}"#).unwrap_err();
        assert!(
            family.contains("env_1_attack: cannot read \"5 Hz\" as Seconds")
                && family.contains("s, ms"),
            "{family}"
        );
        let unknown =
            resolve_one(r#"{"param":"filter_1_cuttoff","value":"5 Hz","reason":"r"}"#).unwrap_err();
        assert!(
            unknown.contains("Close names: filter_1_cutoff"),
            "{unknown}"
        );
        let range = resolve_one(r#"{"param":"filter_1_cutoff","value":"30 kHz","reason":"r"}"#)
            .unwrap_err();
        assert!(
            range.contains("outside 8.0 to 136.0")
                && range.contains("Hz to")
                && range.contains("nearest"),
            "{range}"
        );
        let reason = parse(
            &list(r#"{"param":"filter_1_cutoff","value":"1 kHz"}"#),
            Path::Agent,
        )
        .unwrap_err()
        .to_string();
        assert!(
            reason.contains("edit 1 (filter_1_cutoff): the edit has no reason"),
            "{reason}"
        );
        assert!(
            parse(
                &list(r#"{"param":"filter_1_cutoff","value":"1 kHz"}"#),
                Path::Report
            )
            .is_ok()
        );
    }

    #[test]
    fn a_wrong_suffix_lists_the_accepted_ones() {
        let error =
            resolve_one(r#"{"param":"filter_1_cutoff","value":"1 GHz","reason":"r"}"#).unwrap_err();
        assert!(error.contains("Hz, kHz"), "{error}");
    }

    #[test]
    fn every_problem_is_reported_not_only_the_first() {
        let text = list(
            r#"{"param":"nope","value":"1 Hz","reason":"r"},{"param":"filter_1_cutoff","value":"1 Hz","reason":"r"},{"menu":"filter_1_model","choice":"Moog","reason":"r"}"#,
        );
        let errors = resolve(&parse(&text, Path::Agent).unwrap(), vital()).unwrap_err();
        assert_eq!(errors.0.len(), 3);
        assert!(errors.0[2].fix.contains("Analog, Dirty, Ladder"));
    }

    #[test]
    fn records_reject_unknown_fields_and_mixed_kinds() {
        for (edit, needle) in [
            (
                r#"{"param":"filter_1_cutoff","value":"1 kHz","reason":"r","why":"x"}"#,
                "unknown field",
            ),
            (
                r#"{"param":"filter_1_cutoff","menu":"x","reason":"r"}"#,
                "mixes record kinds",
            ),
            (r#"{"reason":"r"}"#, "no record kind"),
            ("5", "not an object"),
            (
                r#"{"raw":{"key":"k","value":null},"reason":"r"}"#,
                "not a number",
            ),
        ] {
            let error = parse(&list(edit), Path::Agent).unwrap_err().to_string();
            assert!(error.contains(needle), "{needle}: {error}");
        }
        assert!(parse(r#"{"synth":"vital","edits":[],"extra":1}"#, Path::Agent).is_err());
        assert!(parse(r#"{"edits":[]}"#, Path::Agent).is_err());
    }

    #[test]
    fn route_menu_and_raw_records_parse_and_round_trip() {
        let text = list(
            r#"{"route":{"source":"lfo_1","dest":"filter_1_cutoff","amount":"+32 st","polarity":"unipolar"},"reason":"wobble"},{"menu":"filter_1_model","choice":"Ladder","reason":"warm"},{"raw":{"key":"custom_warps_count","value":2},"reason":"newer build"}"#,
        );
        let parsed = parse(&text, Path::Agent).unwrap();
        let resolved = resolve(&parsed, vital()).unwrap();
        assert_eq!(
            resolved[0],
            Resolved::Route {
                source: "lfo_1".into(),
                destination: "filter_1_cutoff".into(),
                amount: 0.25,
                polarity: Some(RoutePolarity::Unipolar),
                reason: Some("wobble".into()),
            }
        );
        assert_eq!(
            resolved[2],
            Resolved::Raw {
                key: "custom_warps_count".into(),
                value: Stored::Int(2),
                reason: Some("newer build".into()),
            }
        );
        let percent = resolve_one(
            r#"{"route":{"source":"env_2","dest":"osc_1_level","amount":"-50%"},"reason":"r"}"#,
        )
        .unwrap();
        assert!(
            matches!(percent, Resolved::Route { amount, polarity: None, .. } if (amount + 0.5).abs() < 1e-12)
        );
        let semis = resolve_one(
            r#"{"route":{"source":"env_2","dest":"osc_1_level","amount":"12 st"},"reason":"r"}"#,
        )
        .unwrap_err();
        assert!(semis.contains("percent of the destination"), "{semis}");
        let raw_known =
            resolve_one(r#"{"raw":{"key":"filter_1_cutoff","value":2},"reason":"r"}"#).unwrap_err();
        assert!(raw_known.contains("registry control"), "{raw_known}");
    }

    #[test]
    fn every_record_kind_round_trips_through_json() {
        let text = list(
            r#"{"param":"filter_1_cutoff","value":"410 Hz","reason":"a"},{"param":"filter_1_on","value":true,"reason":"b"},{"param":"osc_1_unison_voices","value":5.0,"reason":"c"},{"route":{"source":"lfo_1","dest":"filter_1_cutoff","amount":"25%","polarity":"bipolar"},"reason":"d"},{"route":{"source":"env_2","dest":"osc_1_level","amount":"-10%"},"reason":"e"},{"menu":"filter_1_model","choice":"Ladder","reason":"f"},{"raw":{"key":"k","value":2},"reason":"g"},{"raw":{"key":"k2","value":0.5},"reason":"h"}"#,
        );
        let parsed = parse(&text, Path::Agent).unwrap();
        let again = parse(&serde_json::to_string(&parsed).unwrap(), Path::Agent).unwrap();
        assert_eq!(parsed, again);
    }

    #[test]
    fn a_list_for_another_synth_is_rejected() {
        let parsed = parse(r#"{"synth":"serum2","edits":[]}"#, Path::Agent).unwrap();
        let error = resolve(&parsed, vital()).unwrap_err().to_string();
        assert!(
            error.contains("written for Serum 2, but the target is Vital"),
            "{error}"
        );
    }

    #[test]
    fn legacy_controls_and_repeated_labels_are_refused() {
        let legacy = resolve_one(r#"{"param":"sub_on","value":true,"reason":"r"}"#).unwrap_err();
        assert!(legacy.contains("legacy control"), "{legacy}");
        let repeated =
            resolve_one(r#"{"menu":"osc_1_distortion_type","choice":"FM <- Osc","reason":"r"}"#)
                .unwrap_err();
        assert!(repeated.contains("several choices"), "{repeated}");
    }

    #[test]
    fn a_stored_index_selects_a_repeated_label() {
        let spec = vital().get("osc_1_distortion_type").unwrap();
        let index = spec
            .choices
            .iter()
            .position(|label| label == "FM <- Osc")
            .unwrap();
        let stored = spec.stored_min + crate::cast::usize_to_f64(index);
        let resolved = resolve_one(&format!(
            r#"{{"param":"osc_1_distortion_type","value":{stored},"reason":"r"}}"#
        ))
        .unwrap();
        assert!(
            matches!(resolved, Resolved::Param { stored: got, .. } if got.to_bits() == stored.to_bits())
        );
        let outside = resolve_one(r#"{"param":"osc_1_distortion_type","value":99,"reason":"r"}"#)
            .unwrap_err();
        assert!(outside.contains("whole stored index"), "{outside}");
    }

    #[test]
    fn a_count_between_steps_is_not_whole() {
        let error =
            resolve_one(r#"{"param":"osc_1_unison_voices","value":5.5,"reason":"r"}"#).unwrap_err();
        assert!(error.contains("5.5 is not a whole number"), "{error}");
        assert!(error.contains("the nearest is 6.0"), "{error}");
        let error =
            resolve_one(r#"{"param":"osc_1_unison_voices","value":20,"reason":"r"}"#).unwrap_err();
        assert!(error.contains("outside the stored range"), "{error}");
    }

    #[test]
    fn uncalibrated_follows_the_registry() {
        let resolved =
            resolve_one(r#"{"param":"filter_1_resonance","value":"50%","reason":"r"}"#).unwrap();
        assert!(matches!(
            resolved,
            Resolved::Param {
                uncalibrated: true,
                ..
            }
        ));
        let resolved =
            resolve_one(r#"{"param":"filter_1_cutoff","value":"500 Hz","reason":"r"}"#).unwrap();
        assert!(matches!(
            resolved,
            Resolved::Param {
                uncalibrated: false,
                ..
            }
        ));
    }

    #[test]
    fn decisions_parse_strictly() {
        let text = r#"{"synth":"vital","edits":[],"ambiguities":[{"id":"a","choice":"both","reason":"keep both"}]}"#;
        assert_eq!(parse(text, Path::Agent).unwrap().ambiguities.len(), 1);
        let bad = r#"{"synth":"vital","edits":[],"ambiguities":[{"id":"a","choice":"both"}]}"#;
        assert!(parse(bad, Path::Agent).is_err());
    }
}
