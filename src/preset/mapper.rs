//! Turns measured layer values into edits for one synth.
//!
//! Estimators report values in unit types and never round to what a synth
//! offers. A [`SynthMapper`] picks the nearest edit the synth can make and
//! lists what it cannot reach in `unrepresentable`, with the residual.
//! [`fit_param`] is the one helper every mapper shares: it keeps keys in
//! the registry, values in range and output order stable.
//!
//! The three input types are empty until the estimator epics add their
//! group fields.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::confidence::Level;
use crate::preset::edits::{Edit, ParamValue, Record};
use crate::preset::registry::{
    ParamKind, ParamRegistry, ParamSpec, Physical, Provenance, UnitKind,
};
use crate::synth::{Synth, SynthCapabilities, SynthError};

/// The pitch and envelope groups of the layer report.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct PitchEnvelopeGroups {}

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
    let missing = |nearest| Mapped {
        edits: Vec::new(),
        unrepresentable: vec![Unrepresentable {
            field: field.to_owned(),
            wanted: wanted_amount.clone(),
            nearest,
            reason: UnrepresentableReason::NotInSynth,
        }],
    };
    let Some(spec) = registry
        .get(key)
        .filter(|spec| !spec.legacy && spec.unit() == Some(unit))
    else {
        return missing(None);
    };
    let Some(stored) = nearest_stored(spec, wanted.get()) else {
        return missing(None);
    };
    let Ok(physical) = spec.physical(stored) else {
        return missing(None);
    };
    let mut mapped = Mapped {
        edits: vec![SuggestedEdit {
            edit: Edit {
                record: Record::Param {
                    param: spec.key.clone(),
                    value: ParamValue::Text(physical.to_string()),
                },
                reason: Some(reason.to_owned()),
            },
            uncalibrated: spec.provenance == Provenance::Interim,
            confidence,
        }],
        unrepresentable: Vec::new(),
    };
    let exact = spec
        .stored(&wanted)
        .is_ok_and(|exact| exact.to_bits() == stored.to_bits());
    if !exact {
        let reason = if stored >= spec.stored_max && wanted.get() > physical.get() {
            UnrepresentableReason::AboveRange
        } else if stored <= spec.stored_min && wanted.get() < physical.get() {
            UnrepresentableReason::BelowRange
        } else {
            UnrepresentableReason::NoSuchChoice
        };
        let reason = match (rising(spec), reason) {
            (false, UnrepresentableReason::AboveRange) => UnrepresentableReason::BelowRange,
            (false, UnrepresentableReason::BelowRange) => UnrepresentableReason::AboveRange,
            (_, other) => other,
        };
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

/// Whether the physical value grows with the stored value.
fn rising(spec: &ParamSpec) -> bool {
    match (
        spec.physical(spec.stored_min),
        spec.physical(spec.stored_max),
    ) {
        (Ok(low), Ok(high)) => high.get() >= low.get(),
        _ => true,
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
