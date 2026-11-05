//! Turns measured layer values into edits for one synth.
//!
//! Estimators report values in unit types and never round to what a synth
//! offers. A [`SynthMapper`] picks the nearest edit the synth can make and
//! lists what it cannot reach in `unrepresentable`, with the residual.
//! [`fit_param`] is the one helper every mapper shares: it keeps keys in
//! the registry, values in range and output order stable.
//!
//! [`fit_flag`], [`fit_stored`] and [`fit_route`] do the same for a switch,
//! a control without a unit and a modulation route. [`pitch_envelope`]
//! holds the synth-neutral flow of the pitch and envelope groups. The
//! timbre and motion inputs are empty until their estimator epics add
//! their group fields.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::confidence::Level;
use crate::preset::edits::{
    Edit, ParamValue, PolarityText, Record, RouteRecord, stored_in_semitones,
};
use crate::preset::registry::{
    ParamKind, ParamRegistry, ParamSpec, Physical, Provenance, UnitKind,
};
use crate::synth::{Synth, SynthCapabilities, SynthError};
use crate::units::Semitones;

pub mod pitch_envelope;

pub use pitch_envelope::PitchEnvelopeGroups;

/// The timbre groups of the layer report.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct TimbreGroups {}

/// The motion and effect groups of the layer report.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct MotionFxGroups {}

/// What a mapper may know about its synth.
#[derive(Clone, Copy, Debug)]
pub struct MapContext<'a> {
    /// The registry of the target.
    pub registry: &'a dyn ParamRegistry,
    /// The structure of the target.
    pub capabilities: &'a SynthCapabilities,
}

/// Maps layer groups to the edits of one synth.
pub trait SynthMapper: Send + Sync {
    /// The synth it maps to.
    fn synth(&self) -> Synth;

    /// Edits for the pitch and envelope groups.
    fn pitch_envelope(&self, input: &PitchEnvelopeGroups, ctx: &MapContext<'_>) -> Mapped;

    /// Edits for the timbre groups.
    fn timbre(&self, input: &TimbreGroups, ctx: &MapContext<'_>) -> Mapped;

    /// Edits for the motion and effect groups.
    fn motion_fx(&self, input: &MotionFxGroups, ctx: &MapContext<'_>) -> Mapped;
}

/// The output of a mapper.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mapped {
    /// The edits, sorted by registry key, then by route slot.
    pub edits: Vec<SuggestedEdit>,
    /// What the synth cannot reach, with the nearest value it can.
    pub unrepresentable: Vec<Unrepresentable>,
    /// What the mapper leaves unwritten on purpose, with the reason.
    pub unwritten: Vec<Unwritten>,
}

impl Mapped {
    /// Appends the edits and lists of another output.
    pub fn extend(&mut self, other: Self) {
        self.edits.extend(other.edits);
        self.unrepresentable.extend(other.unrepresentable);
        self.unwritten.extend(other.unwritten);
    }
}

/// One edit a mapper suggests.
#[derive(Clone, Debug, PartialEq)]
pub struct SuggestedEdit {
    /// The edit-list record, with the layer name as its reason.
    pub edit: Edit,
    /// The registry law is interim; the explanation reads its tuning note.
    pub uncalibrated: bool,
    /// The level of the layer group, capped.
    pub confidence: Level,
}

/// Why a wanted value has no exact edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnrepresentableReason {
    /// Above the largest value the control reaches.
    AboveRange,
    /// Below the smallest value the control reaches.
    BelowRange,
    /// Between the choices or steps the control offers.
    NoSuchChoice,
    /// The synth has no such control.
    NotInSynth,
    /// Every slot is taken.
    SlotLimit,
}

/// A number in a unit, or a choice name.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Amount {
    /// A number and its unit suffix, or `count` for an integer entry.
    Value {
        /// The number.
        value: f64,
        /// The unit suffix.
        unit: String,
    },
    /// A choice name.
    Choice(String),
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Value { value, unit } => write!(f, "{value} {unit}"),
            Self::Choice(name) => f.write_str(name),
        }
    }
}

/// A wanted value the synth cannot reach exactly.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unrepresentable {
    /// The layer field name, such as `filter1_cutoff_hz`.
    pub field: String,
    /// What the layer measured.
    pub wanted: Amount,
    /// The nearest value the synth reaches, or `None` when it has nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nearest: Option<Amount>,
    /// Why.
    pub reason: UnrepresentableReason,
}

impl Unrepresentable {
    /// The signed difference `nearest - wanted` for numbers in one unit.
    #[must_use]
    pub fn residual(&self) -> Option<f64> {
        match (&self.wanted, &self.nearest) {
            (
                Amount::Value {
                    value: wanted,
                    unit,
                },
                Some(Amount::Value {
                    value: nearest,
                    unit: nearest_unit,
                }),
            ) if unit == nearest_unit => Some(nearest - wanted),
            _ => None,
        }
    }
}

/// Why a mapper leaves a measured field unwritten.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnwrittenReason {
    /// The field has no estimate, or a low one.
    LowConfidence,
    /// The field is an upper bound, such as an attack faster than the track
    /// can tell.
    UpperBound,
    /// The report cannot separate the value from what hides it, such as a
    /// release under a heavy tail.
    NotIdentifiable,
    /// The curve stays at its default: no law maps the measured shape yet.
    /// The explanation says "curve not set; tune by ear".
    CurveNotSet,
    /// The control exists, but no probe has shown what it does.
    Unprobed,
}

/// A measured field that the mapper does not write.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unwritten {
    /// The path of the field in the group's `values`, with a dot into a
    /// list's records, such as `attack_s` or `glides.curve`. A curve entry
    /// names the stage whose curve stays at its default.
    pub field: String,
    /// Why.
    pub reason: UnwrittenReason,
}

impl Unwritten {
    /// An entry for a field.
    #[must_use]
    pub fn new(field: &str, reason: UnwrittenReason) -> Self {
        Self {
            field: field.to_owned(),
            reason,
        }
    }
}

/// The suffix of a unit kind in the E1 grammar.
fn suffix(unit: UnitKind) -> &'static str {
    match unit {
        UnitKind::Hz => "Hz",
        UnitKind::Seconds => "s",
        UnitKind::Semitones => "st",
        UnitKind::Cents => "ct",
        UnitKind::Octaves => "oct",
        UnitKind::Decibels => "dB",
        UnitKind::Normalized => "",
        UnitKind::MidiPitch => "midi",
        UnitKind::PercentUnipolar | UnitKind::PercentBipolar => "%",
    }
}

/// The nearest edit of one control to a wanted physical value.
///
/// The value goes through the law of the entry. Outside the range it
/// lands on the nearest bound, and a `Count` or `Choice` entry takes the
/// nearest whole step; either way the miss is listed with the residual. A
/// key outside the registry, a legacy key or a unit that differs gives no
/// edit and a `not_in_synth` entry.
#[must_use]
pub fn fit_param(
    registry: &dyn ParamRegistry,
    field: &str,
    key: &str,
    wanted: Physical,
    reason: &str,
    confidence: Level,
) -> Mapped {
    let unit = wanted.kind();
    let wanted_amount = Amount::Value {
        value: wanted.get(),
        unit: suffix(unit).to_owned(),
    };
    let missing = || not_in_synth(field, wanted_amount.clone());
    let Some(spec) = registry
        .get(key)
        .filter(|spec| !spec.legacy && spec.unit() == Some(unit))
    else {
        return missing();
    };
    let Some(stored) = nearest_stored(spec, wanted.get()) else {
        return missing();
    };
    let Ok(physical) = spec.physical(stored) else {
        return missing();
    };
    let exact = spec
        .stored(&wanted)
        .is_ok_and(|exact| exact.to_bits() == stored.to_bits());
    // The law's round trip adds float noise, such as 0.11999999999999998 s
    // for 0.12 s: an exact fit writes the wanted value, a miss the shortest
    // text that stores the same value to the bit.
    let text = if exact {
        wanted
    } else {
        shortest(spec, physical, stored)
    };
    let mut mapped = one_edit(
        Record::Param {
            param: spec.key.clone(),
            value: ParamValue::Text(text.to_string()),
        },
        reason,
        spec.provenance == Provenance::Interim,
        confidence,
    );
    if !exact {
        // The physical range is the span between the values at both stored
        // bounds, so a falling law needs no special case.
        let ends = [spec.stored_min, spec.stored_max]
            .map(|bound| spec.physical(bound).map_or(physical.get(), Physical::get));
        let reason = miss_side(wanted.get(), ends[0].min(ends[1]), ends[0].max(ends[1]));
        mapped.unrepresentable.push(Unrepresentable {
            field: field.to_owned(),
            wanted: wanted_amount,
            nearest: Some(Amount::Value {
                value: physical.get(),
                unit: suffix(unit).to_owned(),
            }),
            reason,
        });
    }
    mapped
}

/// The value at a stored value with the fewest significant digits that
/// still store exactly that value, so a bound stays inside the range.
fn shortest(spec: &ParamSpec, physical: Physical, stored: f64) -> Physical {
    (0..17)
        .filter_map(|digits| {
            let value = format!("{:.digits$e}", physical.get())
                .parse::<f64>()
                .ok()?;
            Physical::new(physical.kind(), value).ok()
        })
        .find(|short| {
            spec.stored(short)
                .is_ok_and(|back| back.to_bits() == stored.to_bits())
        })
        .unwrap_or(physical)
}

/// A switch set on or off.
///
/// A key outside the registry, a legacy key or a key that is not a switch
/// gives no edit and a `not_in_synth` entry.
#[must_use]
pub fn fit_flag(
    registry: &dyn ParamRegistry,
    field: &str,
    key: &str,
    on: bool,
    reason: &str,
    confidence: Level,
) -> Mapped {
    match registry
        .get(key)
        .filter(|spec| !spec.legacy && spec.kind == ParamKind::Flag)
    {
        Some(spec) => one_edit(
            Record::Param {
                param: spec.key.clone(),
                value: ParamValue::Flag(on),
            },
            reason,
            spec.provenance == Provenance::Interim,
            confidence,
        ),
        None => not_in_synth(field, Amount::Choice(on.to_string())),
    }
}

/// The nearest edit of a control without a unit to a wanted stored value,
/// such as a voice count or a level the adapter's own law computed.
///
/// Outside the range the value lands on the nearest bound, and a `Count`
/// takes the nearest whole number; either way the miss is listed with the
/// residual. A key outside the registry, a legacy key, a key with a unit or
/// a switch or menu gives no edit and a `not_in_synth` entry.
#[must_use]
pub fn fit_stored(
    registry: &dyn ParamRegistry,
    field: &str,
    key: &str,
    wanted: f64,
    reason: &str,
    confidence: Level,
) -> Mapped {
    let spec = registry.get(key).filter(|spec| {
        !spec.legacy
            && spec.unit().is_none()
            && matches!(spec.kind, ParamKind::Float | ParamKind::Count)
    });
    let unit = if spec.is_some_and(|spec| spec.kind == ParamKind::Count) {
        "count"
    } else {
        ""
    };
    let amount = |value| Amount::Value {
        value,
        unit: unit.to_owned(),
    };
    let Some(spec) = spec else {
        return not_in_synth(field, amount(wanted));
    };
    let clamped = wanted.clamp(spec.stored_min, spec.stored_max);
    let stored = if spec.kind == ParamKind::Count {
        clamped.round().clamp(spec.stored_min, spec.stored_max)
    } else {
        clamped
    };
    let mut mapped = one_edit(
        Record::Param {
            param: spec.key.clone(),
            value: ParamValue::Number(stored),
        },
        reason,
        spec.provenance == Provenance::Interim,
        confidence,
    );
    if stored.to_bits() != wanted.to_bits() {
        mapped.unrepresentable.push(Unrepresentable {
            field: field.to_owned(),
            wanted: amount(wanted),
            nearest: Some(amount(stored)),
            reason: miss_side(wanted, spec.stored_min, spec.stored_max),
        });
    }
    mapped
}

/// A route of a depth in semitones into a destination stored in
/// semitones, such as an envelope into oscillator pitch.
///
/// A depth beyond the destination's stored span lands on the span and the
/// rest is listed. A route amount is an interim law until the calibration
/// set measures it, so the edit is always `uncalibrated`. A destination
/// outside the registry, a legacy one or one not stored in semitones gives
/// no edit and a `not_in_synth` entry; a zero depth gives nothing.
#[must_use]
pub fn fit_route(
    registry: &dyn ParamRegistry,
    field: &str,
    (source, dest): (&str, &str),
    depth: Semitones,
    polarity: Option<PolarityText>,
    reason: &str,
    confidence: Level,
) -> Mapped {
    let amount = |value| Amount::Value {
        value,
        unit: suffix(UnitKind::Semitones).to_owned(),
    };
    let wanted = depth.get();
    let Some(spec) = registry
        .get(dest)
        .filter(|spec| !spec.legacy && stored_in_semitones(spec))
    else {
        return not_in_synth(field, amount(wanted));
    };
    if wanted == 0.0 {
        return Mapped::default();
    }
    let span = spec.stored_max - spec.stored_min;
    let nearest = wanted.clamp(-span, span);
    let mut mapped = one_edit(
        Record::Route(RouteRecord {
            source: source.to_owned(),
            dest: spec.key.clone(),
            amount: Semitones::clamped(nearest).to_string(),
            polarity,
        }),
        reason,
        true,
        confidence,
    );
    if nearest.to_bits() != wanted.to_bits() {
        mapped.unrepresentable.push(Unrepresentable {
            field: field.to_owned(),
            wanted: amount(wanted),
            nearest: Some(amount(nearest)),
            reason: miss_side(wanted, -span, span),
        });
    }
    mapped
}

/// An output of one edit.
fn one_edit(record: Record, reason: &str, uncalibrated: bool, confidence: Level) -> Mapped {
    Mapped {
        edits: vec![SuggestedEdit {
            edit: Edit {
                record,
                reason: Some(reason.to_owned()),
            },
            uncalibrated,
            confidence,
        }],
        ..Mapped::default()
    }
}

/// An output of no edit and one `not_in_synth` entry.
fn not_in_synth(field: &str, wanted: Amount) -> Mapped {
    Mapped {
        unrepresentable: vec![Unrepresentable {
            field: field.to_owned(),
            wanted,
            nearest: None,
            reason: UnrepresentableReason::NotInSynth,
        }],
        ..Mapped::default()
    }
}

/// The side of a range a wanted value misses on, or a step between.
fn miss_side(wanted: f64, low: f64, high: f64) -> UnrepresentableReason {
    if wanted > high {
        UnrepresentableReason::AboveRange
    } else if wanted < low {
        UnrepresentableReason::BelowRange
    } else {
        UnrepresentableReason::NoSuchChoice
    }
}

/// The stored value nearest to a wanted physical value: through the law
/// when it has an inverse there, else the bound on the near side, then the
/// nearest whole step for a whole kind.
fn nearest_stored(spec: &ParamSpec, wanted: f64) -> Option<f64> {
    let law = &spec.law.as_ref()?.law;
    let low = spec.physical(spec.stored_min).ok().map(Physical::get);
    let high = spec.physical(spec.stored_max).ok().map(Physical::get);
    let raw = crate::preset::registry::ParamLaw::to_stored(law, wanted).ok();
    let stored = match raw {
        Some(stored) => stored.clamp(spec.stored_min, spec.stored_max),
        None => {
            // The wanted value is outside the law's domain, such as 0 Hz:
            // take the bound whose physical value is nearer.
            match (low, high) {
                (Some(low), Some(high)) if (wanted - low).abs() <= (wanted - high).abs() => {
                    spec.stored_min
                }
                (Some(_) | None, Some(_)) => spec.stored_max,
                _ => spec.stored_min,
            }
        }
    };
    if spec.kind == ParamKind::Float {
        return Some(stored);
    }
    let (down, up) = (
        stored.floor().max(spec.stored_min),
        stored.ceil().min(spec.stored_max),
    );
    let distance = |step: f64| {
        spec.physical(step)
            .map_or(f64::INFINITY, |value| (value.get() - wanted).abs())
    };
    Some(if distance(up) < distance(down) {
        up
    } else {
        down
    })
}

/// Sorts edits by registry key, then by route slot, as every mapper must.
pub fn sort_edits(mapped: &mut Mapped) {
    mapped
        .edits
        .sort_by(|a, b| a.edit.record.name().cmp(b.edit.record.name()));
}

/// The mapper of a synth.
///
/// # Errors
///
/// [`SynthError::NotBuilt`] for a synth outside [`Synth::READY`].
pub fn mapper_for(synth: Synth) -> Result<&'static dyn SynthMapper, SynthError> {
    match synth.ready()? {
        Synth::Vital => Ok(&crate::preset::vital::MAPPER),
        Synth::Serum2 => Err(SynthError::NotBuilt(synth)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::edits::{EditList, Path, Resolved, parse, resolve};
    use crate::preset::registry::registry_for;
    use proptest::prelude::*;

    fn vital() -> &'static dyn ParamRegistry {
        registry_for(Synth::Vital).unwrap()
    }

    /// The registry keys with a unit, for the property test.
    fn unit_keys() -> Vec<&'static ParamSpec> {
        vital()
            .iter()
            .filter(|spec| spec.unit().is_some() && !spec.legacy)
            .collect()
    }

    /// Resolves the edits of a mapper output through the edit-list path,
    /// as `preset apply` would.
    fn resolved(mapped: &Mapped) -> Vec<Resolved> {
        let list = EditList {
            synth: Synth::Vital,
            edits: mapped.edits.iter().map(|edit| edit.edit.clone()).collect(),
            ambiguities: Vec::new(),
        };
        let text = serde_json::to_string(&list).unwrap();
        resolve(&parse(&text, Path::Agent).unwrap(), vital()).unwrap()
    }

    proptest! {
        #[test]
        fn fitted_edits_stay_in_range_and_list_every_miss(index in 0usize..10_000, t in -0.5..1.5f64) {
            let keys = unit_keys();
            let spec = keys[index % keys.len()];
            let Ok(low) = spec.physical(spec.stored_min).map(Physical::get) else { return Ok(()) };
            let high = spec.physical(spec.stored_max).unwrap().get();
            let value = low + t * (high - low);
            let Ok(wanted) = Physical::new(spec.unit().unwrap(), value) else { return Ok(()) };
            let mut mapped = fit_param(vital(), "field", &spec.key, wanted, "layer", Level::Medium);
            sort_edits(&mut mapped);
            // Rule 1: the key exists, the value resolves inside the range.
            prop_assert_eq!(mapped.edits.len(), 1);
            for item in resolved(&mapped) {
                let Resolved::Param { key, stored, .. } = item else { panic!("not a param") };
                prop_assert_eq!(&key, &spec.key);
                prop_assert!(spec.accepts(stored), "{} {}", key, stored);
            }
            // Rules 2 and 5: a value outside the range or between steps is
            // listed with the nearest value.
            let inside = (0.0..=1.0).contains(&t);
            let whole = spec.kind != ParamKind::Float;
            if !inside {
                prop_assert_eq!(mapped.unrepresentable.len(), 1, "{} at {}", spec.key, value);
                prop_assert!(mapped.unrepresentable[0].residual().is_some());
            } else if !whole {
                prop_assert!(mapped.unrepresentable.len() <= 1);
            }
        }
    }

    #[test]
    fn a_cutoff_above_the_range_is_written_at_the_top_and_listed() {
        let wanted = Physical::new(UnitKind::Hz, 25_000.0).unwrap();
        let mapped = fit_param(
            vital(),
            "filter1_cutoff_hz",
            "filter_1_cutoff",
            wanted,
            "filter",
            Level::High,
        );
        assert_eq!(mapped.unrepresentable.len(), 1);
        let miss = &mapped.unrepresentable[0];
        assert_eq!(miss.reason, UnrepresentableReason::AboveRange);
        // Top of the range: 8.1757989156 * 2^(136 / 12) Hz.
        let top = 8.175_798_915_6 * (136.0f64 / 12.0).exp2();
        assert!((miss.residual().unwrap() - (top - 25_000.0)).abs() < 1e-6);
        let item = resolved(&mapped).remove(0);
        assert!(matches!(item, Resolved::Param { stored, .. } if (stored - 136.0).abs() < 1e-12));
    }

    #[test]
    fn a_falling_law_names_the_side_it_misses() {
        use crate::preset::registry::Registry;
        use crate::preset::registry::tests::{law_cases, spec};
        let (_, law, min, max, unit) = law_cases()
            .into_iter()
            .find(|(name, ..)| *name == "falling table")
            .unwrap();
        let registry = Registry::new(
            crate::synth::Synth::Vital,
            vec![spec("level", law, min, max, unit)],
        )
        .unwrap();
        // The table falls from 6 dB at stored 0 to -30 dB at stored 1.
        for (wanted, side) in [
            (10.0, UnrepresentableReason::AboveRange),
            (-40.0, UnrepresentableReason::BelowRange),
        ] {
            let wanted = Physical::new(UnitKind::Decibels, wanted).unwrap();
            let mapped = fit_param(&registry, "level", "level", wanted, "level", Level::Low);
            assert_eq!(mapped.unrepresentable[0].reason, side);
        }
    }

    #[test]
    fn a_zero_hz_cutoff_lands_on_the_bottom() {
        let wanted = Physical::new(UnitKind::Hz, 0.0).unwrap();
        let mapped = fit_param(
            vital(),
            "f",
            "filter_1_cutoff",
            wanted,
            "filter",
            Level::Low,
        );
        assert_eq!(
            mapped.unrepresentable[0].reason,
            UnrepresentableReason::BelowRange
        );
    }

    #[test]
    fn a_count_takes_the_nearest_step_and_lists_the_rest() {
        let wanted = Physical::new(UnitKind::Semitones, 12.4).unwrap();
        let mapped = fit_param(
            vital(),
            "osc1_transpose_st",
            "osc_1_transpose",
            wanted,
            "pitch",
            Level::High,
        );
        assert_eq!(
            mapped.unrepresentable[0].reason,
            UnrepresentableReason::NoSuchChoice
        );
        assert!((mapped.unrepresentable[0].residual().unwrap() + 0.4).abs() < 1e-9);
        let exact = Physical::new(UnitKind::Semitones, 12.0).unwrap();
        let mapped = fit_param(vital(), "f", "osc_1_transpose", exact, "pitch", Level::High);
        assert_eq!(mapped.unrepresentable, Vec::new());
    }

    #[test]
    fn a_missing_control_gives_no_edit() {
        let wanted = Physical::new(UnitKind::Hz, 100.0).unwrap();
        for key in ["no_such_key", "osc_1_level", "sub_level"] {
            let mapped = fit_param(vital(), "f", key, wanted, "x", Level::High);
            assert_eq!(mapped.edits, Vec::new(), "{key}");
            assert_eq!(
                mapped.unrepresentable[0].reason,
                UnrepresentableReason::NotInSynth
            );
        }
    }

    #[test]
    fn interim_controls_are_uncalibrated() {
        let wanted = Physical::new(UnitKind::PercentUnipolar, 40.0).unwrap();
        let mapped = fit_param(
            vital(),
            "f",
            "filter_1_resonance",
            wanted,
            "filter",
            Level::High,
        );
        assert!(mapped.edits[0].uncalibrated);
    }

    #[test]
    fn a_fitted_time_prints_without_float_noise() {
        let text = |key: &str, seconds: f64| {
            let wanted = Physical::new(UnitKind::Seconds, seconds).unwrap();
            let mapped = fit_param(vital(), "f", key, wanted, "x", Level::High);
            match &mapped.edits[0].edit.record {
                Record::Param {
                    value: ParamValue::Text(text),
                    ..
                } => text.clone(),
                other => panic!("{other:?}"),
            }
        };
        assert_eq!(text("portamento_time", 0.12), "0.12 s");
        assert_eq!(text("env_1_attack", 0.02), "0.02 s");
        assert_eq!(text("env_1_decay", 0.2), "0.2 s");
        // A clamped time prints the bound: 2^4 s.
        assert_eq!(text("portamento_time", 20.0), "16 s");
    }

    #[test]
    fn a_count_lands_on_a_whole_step_inside_the_range() {
        let mapped = fit_stored(
            vital(),
            "max_polyphony",
            "polyphony",
            40.0,
            "voices",
            Level::High,
        );
        let miss = &mapped.unrepresentable[0];
        assert_eq!(miss.reason, UnrepresentableReason::AboveRange);
        assert_eq!(
            miss.nearest,
            Some(Amount::Value {
                value: 32.0,
                unit: "count".to_owned()
            })
        );
        assert!(
            matches!(resolved(&mapped)[0], Resolved::Param { stored, .. } if (stored - 32.0).abs() < 1e-12)
        );
        let between = fit_stored(vital(), "f", "polyphony", 3.4, "voices", Level::High);
        assert_eq!(
            between.unrepresentable[0].reason,
            UnrepresentableReason::NoSuchChoice
        );
        assert!(
            matches!(resolved(&between)[0], Resolved::Param { stored, .. } if (stored - 3.0).abs() < 1e-12)
        );
        let exact = fit_stored(vital(), "f", "polyphony", 3.0, "voices", Level::High);
        assert_eq!(exact.unrepresentable, Vec::new());
        for key in ["portamento_time", "legato", "no_such_key"] {
            let mapped = fit_stored(vital(), "f", key, 1.0, "x", Level::High);
            assert_eq!(mapped.edits, Vec::new(), "{key}");
            assert_eq!(
                mapped.unrepresentable[0].reason,
                UnrepresentableReason::NotInSynth
            );
        }
    }

    #[test]
    fn a_flag_needs_a_switch() {
        let mapped = fit_flag(
            vital(),
            "joins.legato",
            "legato",
            true,
            "voices",
            Level::Medium,
        );
        assert!(
            matches!(resolved(&mapped)[0], Resolved::Param { stored, .. } if (stored - 1.0).abs() < 1e-12)
        );
        assert_eq!(mapped.edits[0].confidence, Level::Medium);
        let wrong = fit_flag(vital(), "f", "polyphony", true, "voices", Level::High);
        assert_eq!(wrong.edits, Vec::new());
        assert_eq!(
            wrong.unrepresentable[0].reason,
            UnrepresentableReason::NotInSynth
        );
    }

    #[test]
    fn a_route_in_semitones_clamps_at_the_span() {
        let depth = |st: f64| Semitones::new(st).unwrap();
        let route = |st: f64, dest: &str| {
            fit_route(
                vital(),
                "drops.df_st",
                ("env_2", dest),
                depth(st),
                Some(PolarityText::Unipolar),
                "pitch",
                Level::High,
            )
        };
        let amount = |mapped: &Mapped| match resolved(mapped).remove(0) {
            Resolved::Route { amount, .. } => amount,
            other => panic!("{other:?}"),
        };
        let seven = route(7.0, "osc_1_transpose");
        assert!(seven.edits[0].uncalibrated);
        assert!((amount(&seven) - 7.0 / 96.0).abs() < 1e-9);
        assert_eq!(seven.unrepresentable, Vec::new());
        let deep = route(100.0, "osc_1_transpose");
        assert!((amount(&deep) - 1.0).abs() < 1e-12);
        assert_eq!(
            deep.unrepresentable[0].reason,
            UnrepresentableReason::AboveRange
        );
        assert!((deep.unrepresentable[0].residual().unwrap() + 4.0).abs() < 1e-9);
        let rise = route(-100.0, "osc_1_transpose");
        assert_eq!(
            rise.unrepresentable[0].reason,
            UnrepresentableReason::BelowRange
        );
        // A destination stored in cents takes a percent only.
        let cents = route(1.0, "osc_1_tune");
        assert_eq!(cents.edits, Vec::new());
        assert_eq!(
            cents.unrepresentable[0].reason,
            UnrepresentableReason::NotInSynth
        );
        assert_eq!(route(0.0, "osc_1_transpose"), Mapped::default());
    }

    #[test]
    fn unrepresentable_round_trips_strictly() {
        let text = r#"{"field":"filter1_cutoff_hz","wanted":{"value":25000.0,"unit":"Hz"},"nearest":{"value":21096.2,"unit":"Hz"},"reason":"above_range"}"#;
        let entry: Unrepresentable = serde_json::from_str(text).unwrap();
        assert_eq!(serde_json::to_string(&entry).unwrap(), text);
        let choice = r#"{"field":"filter1_type","wanted":"ladder","nearest":"Ladder","reason":"no_such_choice"}"#;
        let entry: Unrepresentable = serde_json::from_str(choice).unwrap();
        assert_eq!(entry.residual(), None);
        assert!(
            serde_json::from_str::<Unrepresentable>(&text.replace("above_range", "too_high"))
                .is_err()
        );
        let extra = r#"{"field":"f","wanted":"a","reason":"not_in_synth","x":1}"#;
        assert!(serde_json::from_str::<Unrepresentable>(extra).is_err());
    }

    #[test]
    fn serum2_has_no_mapper_yet() {
        assert!(matches!(
            mapper_for(Synth::Serum2),
            Err(SynthError::NotBuilt(Synth::Serum2))
        ));
    }
}
