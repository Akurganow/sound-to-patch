//! The parameter registry of a synth: the key, range, unit and law of
//! every control, with the provenance of each law.
//!
//! A [`ParamSpec`] holds one control. Its [`Law`] converts between the
//! stored value in the preset file and a physical value in an E1 unit.
//! Laws never round: a non-integer stored value for a menu or a count is an
//! error, and choosing the nearest option belongs to the mapper.
//!
//! Each law carries a [`Provenance`]: read from facts, measured on a render
//! or probe, or interim. A [`CalibrationSet`] replaces laws with measured
//! tables without a code change.
//!
//! ```
//! use sound_to_patch::preset::registry::{registry_for, Physical, UnitKind};
//! use sound_to_patch::synth::Synth;
//!
//! let registry = registry_for(Synth::Vital)?;
//! let cutoff = registry.get("filter_1_cutoff").ok_or("no cutoff")?;
//! let stored = cutoff.stored(&Physical::parse(UnitKind::Hz, "261.6255653 Hz")?)?;
//! assert!((stored - 60.0).abs() < 1e-6);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::synth::{Synth, SynthError};
use crate::units::{
    Bipolar, Cents, Decibels, Hz, MidiPitch, Normalized, Octaves, ParseUnitError, Percent, Range,
    RangeError, Seconds, Semitones, Unipolar,
};

/// What a stored value is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamKind {
    /// A real number.
    Float,
    /// An integer.
    Count,
    /// 0 or 1.
    Flag,
    /// An index into the menu labels of the entry.
    Choice,
}

/// The E1 unit type of a physical value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitKind {
    /// [`Hz`].
    Hz,
    /// [`Seconds`].
    Seconds,
    /// [`Semitones`].
    Semitones,
    /// [`Cents`].
    Cents,
    /// [`Octaves`].
    Octaves,
    /// [`Decibels`].
    Decibels,
    /// [`Normalized`].
    Normalized,
    /// [`MidiPitch`].
    MidiPitch,
    /// [`Percent<Unipolar>`].
    PercentUnipolar,
    /// [`Percent<Bipolar>`].
    PercentBipolar,
}

impl UnitKind {
    /// The name of the unit type, as the unit-string parser names it.
    #[must_use]
    pub fn type_name(self) -> &'static str {
        match self {
            Self::Hz => "Hz",
            Self::Seconds => "Seconds",
            Self::Semitones => "Semitones",
            Self::Cents => "Cents",
            Self::Octaves => "Octaves",
            Self::Decibels => "Decibels",
            Self::Normalized => "Normalized",
            Self::MidiPitch => "MidiPitch",
            Self::PercentUnipolar => "Percent (0 to 100)",
            Self::PercentBipolar => "Percent (-100 to 100)",
        }
    }
}

/// A physical value in one of the E1 unit types.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Physical {
    /// A frequency.
    Hz(Hz),
    /// A time.
    Seconds(Seconds),
    /// An interval in semitones.
    Semitones(Semitones),
    /// An interval in cents.
    Cents(Cents),
    /// An interval in octaves.
    Octaves(Octaves),
    /// A level.
    Decibels(Decibels),
    /// A knob position.
    Normalized(Normalized),
    /// A MIDI pitch.
    MidiPitch(MidiPitch),
    /// A percentage from 0 to 100.
    PercentUnipolar(Percent<Unipolar>),
    /// A percentage from -100 to 100.
    PercentBipolar(Percent<Bipolar>),
}

impl Physical {
    /// Builds a value of the unit type `kind` from its base number, as
    /// `get()` returns it: hertz, seconds, percent from 0 to 100, and so on.
    ///
    /// # Errors
    ///
    /// The [`RangeError`] of the unit type.
    pub fn new(kind: UnitKind, value: f64) -> Result<Self, RangeError> {
        Ok(match kind {
            UnitKind::Hz => Self::Hz(Hz::new(value)?),
            UnitKind::Seconds => Self::Seconds(Seconds::new(value)?),
            UnitKind::Semitones => Self::Semitones(Semitones::new(value)?),
            UnitKind::Cents => Self::Cents(Cents::new(value)?),
            UnitKind::Octaves => Self::Octaves(Octaves::new(value)?),
            UnitKind::Decibels => Self::Decibels(Decibels::new(value)?),
            UnitKind::Normalized => Self::Normalized(Normalized::new(value)?),
            UnitKind::MidiPitch => Self::MidiPitch(MidiPitch::new(value)?),
            UnitKind::PercentUnipolar => Self::PercentUnipolar(Percent::new(value)?),
            UnitKind::PercentBipolar => Self::PercentBipolar(Percent::new(value)?),
        })
    }

    /// Parses a unit string such as `"1.2 kHz"` into the unit type `kind`.
    ///
    /// The message of the error names the type, not the parameter. The
    /// caller adds the parameter name.
    ///
    /// # Errors
    ///
    /// The [`ParseUnitError`] of the unit-string parser.
    pub fn parse(kind: UnitKind, text: &str) -> Result<Self, ParseUnitError> {
        Ok(match kind {
            UnitKind::Hz => Self::Hz(text.parse()?),
            UnitKind::Seconds => Self::Seconds(text.parse()?),
            UnitKind::Semitones => Self::Semitones(text.parse()?),
            UnitKind::Cents => Self::Cents(text.parse()?),
            UnitKind::Octaves => Self::Octaves(text.parse()?),
            UnitKind::Decibels => Self::Decibels(text.parse()?),
            UnitKind::Normalized => {
                return Err(ParseUnitError::no_unit_string(text, "Normalized"));
            }
            UnitKind::MidiPitch => Self::MidiPitch(text.parse()?),
            UnitKind::PercentUnipolar => Self::PercentUnipolar(text.parse()?),
            UnitKind::PercentBipolar => Self::PercentBipolar(text.parse()?),
        })
    }

    /// The unit type of the value.
    #[must_use]
    pub fn kind(self) -> UnitKind {
        match self {
            Self::Hz(_) => UnitKind::Hz,
            Self::Seconds(_) => UnitKind::Seconds,
            Self::Semitones(_) => UnitKind::Semitones,
            Self::Cents(_) => UnitKind::Cents,
            Self::Octaves(_) => UnitKind::Octaves,
            Self::Decibels(_) => UnitKind::Decibels,
            Self::Normalized(_) => UnitKind::Normalized,
            Self::MidiPitch(_) => UnitKind::MidiPitch,
            Self::PercentUnipolar(_) => UnitKind::PercentUnipolar,
            Self::PercentBipolar(_) => UnitKind::PercentBipolar,
        }
    }

    /// The base number of the value, as the unit type's `get()` returns it.
    #[must_use]
    pub fn get(self) -> f64 {
        match self {
            Self::Hz(value) => value.get(),
            Self::Seconds(value) => value.get(),
            Self::Semitones(value) => value.get(),
            Self::Cents(value) => value.get(),
            Self::Octaves(value) => value.get(),
            Self::Decibels(value) => value.get(),
            Self::Normalized(value) => value.get(),
            Self::MidiPitch(value) => value.get(),
            Self::PercentUnipolar(value) => value.get(),
            Self::PercentBipolar(value) => value.get(),
        }
    }
}

impl fmt::Display for Physical {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hz(value) => value.fmt(f),
            Self::Seconds(value) => value.fmt(f),
            Self::Semitones(value) => value.fmt(f),
            Self::Cents(value) => value.fmt(f),
            Self::Octaves(value) => value.fmt(f),
            Self::Decibels(value) => value.fmt(f),
            Self::Normalized(value) => write!(f, "{}", value.get()),
            Self::MidiPitch(value) => value.fmt(f),
            Self::PercentUnipolar(value) => value.fmt(f),
            Self::PercentBipolar(value) => value.fmt(f),
        }
    }
}

/// A law between the stored value of a control and its physical value.
///
/// Both directions use the base number of the unit type: hertz, seconds,
/// percent from 0 to 100, decibels. A law is strictly monotone over its
/// domain and never rounds.
pub trait ParamLaw: fmt::Debug + Send + Sync {
    /// The physical value of a stored value.
    ///
    /// # Errors
    ///
    /// A [`RangeError`] when the stored value is outside the domain of the
    /// law or the result is not finite.
    fn to_physical(&self, stored: f64) -> Result<f64, RangeError>;

    /// The stored value of a physical value.
    ///
    /// # Errors
    ///
    /// A [`RangeError`] when the physical value has no stored value under
    /// the law or the result is not finite.
    fn to_stored(&self, physical: f64) -> Result<f64, RangeError>;
}

/// The closed-form laws, a measured table and a law an adapter defines.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Law {
    /// `scale * s + offset`, for any `s`.
    Affine {
        /// Multiplier, not zero.
        scale: f64,
        /// Added after the multiplier.
        offset: f64,
    },
    /// `scale * s^exponent + offset`, for `s >= 0`.
    Power {
        /// Positive exponent.
        exponent: f64,
        /// Multiplier, not zero.
        scale: f64,
        /// Added after the multiplier.
        offset: f64,
    },
    /// `scale * 2^(rate * s)`, for any `s`.
    Exp2 {
        /// Octaves per stored unit, not zero.
        rate: f64,
        /// The value at `s = 0`, positive.
        scale: f64,
    },
    /// `20 * exponent * log10(s)` decibels, for `s > 0`: a stored gain
    /// raised to `exponent` before it reaches the audio.
    DbOfPower {
        /// Positive exponent.
        exponent: f64,
    },
    /// A measured table.
    Table(Table),
    /// A law the closed forms do not cover.
    #[serde(skip)]
    Custom(CustomLaw),
}

/// A law an adapter implements itself, shared between registries.
#[derive(Clone, Debug)]
pub struct CustomLaw(pub Arc<dyn ParamLaw>);

impl PartialEq for CustomLaw {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// The checked result of a law step, named for the error message.
fn finite(what: &'static str, value: f64) -> Result<f64, RangeError> {
    Range::Finite.check(what, value)
}

impl ParamLaw for Law {
    fn to_physical(&self, stored: f64) -> Result<f64, RangeError> {
        let stored = finite("stored value", stored)?;
        match self {
            Self::Affine { scale, offset } => finite("Affine law result", scale * stored + offset),
            Self::Power {
                exponent,
                scale,
                offset,
            } => {
                let base = Range::AtLeast(0.0).check("Power law input", stored)?;
                finite("Power law result", scale * base.powf(*exponent) + offset)
            }
            Self::Exp2 { rate, scale } => finite("Exp2 law result", scale * (rate * stored).exp2()),
            Self::DbOfPower { exponent } => {
                let gain = Range::Above(0.0).check("DbOfPower law input", stored)?;
                finite("DbOfPower law result", 20.0 * exponent * gain.log10())
            }
            Self::Table(table) => table.to_physical(stored),
            Self::Custom(law) => law.0.to_physical(stored),
        }
    }

    fn to_stored(&self, physical: f64) -> Result<f64, RangeError> {
        let physical = finite("physical value", physical)?;
        match self {
            Self::Affine { scale, offset } => {
                finite("inverse Affine law result", (physical - offset) / scale)
            }
            Self::Power {
                exponent,
                scale,
                offset,
            } => {
                let base = Range::AtLeast(0.0)
                    .check("inverse Power law input", (physical - offset) / scale)?;
                finite("inverse Power law result", base.powf(exponent.recip()))
            }
            Self::Exp2 { rate, scale } => {
                let ratio = Range::Above(0.0).check("inverse Exp2 law input", physical / scale)?;
                finite("inverse Exp2 law result", ratio.log2() / rate)
            }
            Self::DbOfPower { exponent } => finite(
                "inverse DbOfPower law result",
                10f64.powf(physical / (20.0 * exponent)),
            ),
            Self::Table(table) => table.to_stored(physical),
            Self::Custom(law) => law.0.to_stored(physical),
        }
    }
}

/// How a [`Table`] interpolates between its points.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Interp {
    /// Linear in the physical value. Suits decibels and percent.
    Linear,
    /// Linear in the logarithm of the physical value. Suits hertz and
    /// seconds; every physical value must be positive.
    LogPhysical,
}

/// A piecewise law through measured points. It never extrapolates.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "TableData", into = "TableData")]
pub struct Table {
    points: Vec<(f64, f64)>,
    interp: Interp,
}

#[derive(Serialize, Deserialize)]
struct TableData {
    points: Vec<(f64, f64)>,
    interp: Interp,
}

impl TryFrom<TableData> for Table {
    type Error = String;

    fn try_from(data: TableData) -> Result<Self, String> {
        Self::new(data.points, data.interp)
    }
}

impl From<Table> for TableData {
    fn from(table: Table) -> Self {
        Self {
            points: table.points,
            interp: table.interp,
        }
    }
}

impl Table {
    /// Builds a table from `(stored, physical)` points.
    ///
    /// # Errors
    ///
    /// A message when there are fewer than two points, a value is not
    /// finite, the stored values do not strictly increase, the physical
    /// values are not strictly monotone, or a physical value is not
    /// positive under [`Interp::LogPhysical`].
    pub fn new(points: Vec<(f64, f64)>, interp: Interp) -> Result<Self, String> {
        if points.len() < 2 {
            return Err(format!(
                "a table needs at least two points, got {}",
                points.len()
            ));
        }
        if let Some((stored, physical)) = points
            .iter()
            .find(|(stored, physical)| !stored.is_finite() || !physical.is_finite())
        {
            return Err(format!("point ({stored}, {physical}) is not finite"));
        }
        if interp == Interp::LogPhysical
            && let Some((_, physical)) = points.iter().find(|(_, physical)| *physical <= 0.0)
        {
            return Err(format!(
                "physical value {physical} is not positive, which log_physical needs"
            ));
        }
        let rising = points[1].1 > points[0].1;
        for pair in points.windows(2) {
            let ((s0, p0), (s1, p1)) = (pair[0], pair[1]);
            if s1 <= s0 {
                return Err(format!(
                    "stored values must strictly increase, but {s1} follows {s0}"
                ));
            }
            let monotone = if rising { p1 > p0 } else { p1 < p0 };
            if !monotone {
                return Err(format!(
                    "physical values must be strictly monotone, but {p1} follows {p0}"
                ));
            }
        }
        Ok(Self { points, interp })
    }

    /// The `(stored, physical)` points.
    #[must_use]
    pub fn points(&self) -> &[(f64, f64)] {
        &self.points
    }

    /// The interpolation.
    #[must_use]
    pub fn interp(&self) -> Interp {
        self.interp
    }

    fn first(&self) -> (f64, f64) {
        self.points[0]
    }

    fn last(&self) -> (f64, f64) {
        self.points[self.points.len() - 1]
    }

    fn warp(&self, physical: f64) -> f64 {
        match self.interp {
            Interp::Linear => physical,
            Interp::LogPhysical => physical.ln(),
        }
    }

    fn unwarp(&self, value: f64) -> f64 {
        match self.interp {
            Interp::Linear => value,
            Interp::LogPhysical => value.exp(),
        }
    }

    fn to_physical(&self, stored: f64) -> Result<f64, RangeError> {
        let range = Range::Closed(self.first().0, self.last().0);
        let stored = range.check("Table law input", stored)?;
        let index = self
            .points
            .windows(2)
            .position(|pair| stored <= pair[1].0)
            .unwrap_or(self.points.len() - 2);
        let ((s0, p0), (s1, p1)) = (self.points[index], self.points[index + 1]);
        let t = (stored - s0) / (s1 - s0);
        let (w0, w1) = (self.warp(p0), self.warp(p1));
        finite("Table law result", self.unwarp(w0 + t * (w1 - w0)))
    }

    fn to_stored(&self, physical: f64) -> Result<f64, RangeError> {
        let (low, high) = {
            let (a, b) = (self.first().1, self.last().1);
            (a.min(b), a.max(b))
        };
        let physical = Range::Closed(low, high).check("inverse Table law input", physical)?;
        let index = self
            .points
            .windows(2)
            .position(|pair| {
                let (a, b) = (pair[0].1, pair[1].1);
                physical >= a.min(b) && physical <= a.max(b)
            })
            .unwrap_or(self.points.len() - 2);
        let ((s0, p0), (s1, p1)) = (self.points[index], self.points[index + 1]);
        let (w0, w1) = (self.warp(p0), self.warp(p1));
        let t = (self.warp(physical) - w0) / (w1 - w0);
        finite("inverse Table law result", s0 + t * (s1 - s0))
    }
}

/// Where the law of an entry comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    /// Read from a cited source, not measured. Needs `evidence`.
    FromFacts,
    /// Fit to a render, a probe or a load check. Needs `evidence` and
    /// `verified_on`.
    Measured,
    /// An estimate, or the audible effect is unmeasured. Needs `evidence`
    /// and a `tuning_note`; an edit of it is marked uncalibrated.
    Interim,
}

/// A synth build, such as `1.5.5`. Each adapter defines what its text
/// holds.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Build(pub String);

impl fmt::Display for Build {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&self.0)
    }
}

/// A unit and the law that converts stored values to it.
#[derive(Clone, Debug, PartialEq)]
pub struct UnitLaw {
    /// The unit type of the physical value.
    pub unit: UnitKind,
    /// The law from the stored value.
    pub law: Law,
}

/// One control of a synth.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamSpec {
    /// The key in the preset file. Unique within a registry.
    pub key: String,
    /// What the stored value is.
    pub kind: ParamKind,
    /// The smallest stored value.
    pub stored_min: f64,
    /// The largest stored value.
    pub stored_max: f64,
    /// The stored value of a new patch, inside the range.
    pub default: f64,
    /// The physical unit and its law, or `None` when no unit is claimed.
    pub law: Option<UnitLaw>,
    /// The on-screen name of the control.
    pub label: String,
    /// Menu labels of a [`ParamKind::Choice`]: `choices[i]` names the
    /// stored value `stored_min + i`.
    pub choices: Vec<String>,
    /// Where the law comes from.
    pub provenance: Provenance,
    /// The source file, document or probe behind the entry.
    pub evidence: String,
    /// The first build that has the control.
    pub first_build: Option<Build>,
    /// The build where a load check or a probe confirmed the entry.
    pub verified_on: Option<Build>,
    /// How to tune an interim control by ear and what to hear.
    pub tuning_note: Option<String>,
    /// The control is in the table, but no module creates it. Readers
    /// accept it and writers refuse it.
    pub legacy: bool,
}

/// A conversion of one control that failed.
///
/// The message names the key, the cause and the fix.
#[derive(Debug)]
pub struct ParamError {
    key: String,
    cause: String,
    fix: String,
    source: Option<Box<dyn Error + Send + Sync>>,
}

impl ParamError {
    fn new(key: &str, cause: String, fix: String) -> Self {
        Self {
            key: key.to_owned(),
            cause,
            fix,
            source: None,
        }
    }

    fn with_source(mut self, source: impl Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// The key of the control.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }
}

impl fmt::Display for ParamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}. {}", self.key, self.cause, self.fix)
    }
}

impl Error for ParamError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source
            .as_deref()
            .map(|source| source as &(dyn Error + 'static))
    }
}

impl ParamSpec {
    /// The physical unit, if the entry claims one.
    #[must_use]
    pub fn unit(&self) -> Option<UnitKind> {
        self.law.as_ref().map(|law| law.unit)
    }

    /// The stored range as text, such as `8.0 to 136.0`.
    #[must_use]
    pub fn range_text(&self) -> String {
        format!("{:?} to {:?}", self.stored_min, self.stored_max)
    }

    /// Whether a stored value lies inside the range and suits the kind.
    #[must_use]
    pub fn accepts(&self, stored: f64) -> bool {
        stored.is_finite()
            && (self.stored_min..=self.stored_max).contains(&stored)
            && (self.kind == ParamKind::Float || stored.fract() == 0.0)
    }

    fn check_stored(&self, stored: f64) -> Result<f64, ParamError> {
        if !stored.is_finite() || !(self.stored_min..=self.stored_max).contains(&stored) {
            return Err(ParamError::new(
                &self.key,
                format!("stored value {stored:?} is outside {}", self.range_text()),
                format!("Use a stored value from {}", self.range_text()),
            ));
        }
        if self.kind != ParamKind::Float && stored.fract() != 0.0 {
            return Err(ParamError::new(
                &self.key,
                format!("stored value {stored:?} is not a whole number"),
                "Pick one whole step; the registry never rounds".to_owned(),
            ));
        }
        Ok(stored)
    }

    fn unit_law(&self) -> Result<&UnitLaw, ParamError> {
        self.law.as_ref().ok_or_else(|| {
            ParamError::new(
                &self.key,
                "the control has no physical unit".to_owned(),
                "Write the stored value instead".to_owned(),
            )
        })
    }

    /// The physical value of a stored value.
    ///
    /// # Errors
    ///
    /// A [`ParamError`] naming the key when the entry has no unit, the
    /// stored value is outside the range or not whole where it must be,
    /// or the law or the unit type rejects it.
    pub fn physical(&self, stored: f64) -> Result<Physical, ParamError> {
        let unit_law = self.unit_law()?;
        let stored = self.check_stored(stored)?;
        let value = unit_law.law.to_physical(stored).map_err(|error| {
            ParamError::new(
                &self.key,
                format!("stored value {stored:?} has no physical value: {error}"),
                format!("Use a stored value from {}", self.range_text()),
            )
            .with_source(error)
        })?;
        Physical::new(unit_law.unit, value).map_err(|error| {
            ParamError::new(
                &self.key,
                format!(
                    "the law gives {value:?}, which {} rejects: {error}",
                    unit_law.unit.type_name()
                ),
                "Report this; the law and the unit disagree".to_owned(),
            )
            .with_source(error)
        })
    }

    /// The stored value of a physical value.
    ///
    /// # Errors
    ///
    /// A [`ParamError`] naming the key when the unit type differs from the
    /// entry's, the law rejects the value, or the result is outside the
    /// stored range or not whole where it must be.
    pub fn stored(&self, value: &Physical) -> Result<f64, ParamError> {
        let unit_law = self.unit_law()?;
        if value.kind() != unit_law.unit {
            return Err(ParamError::new(
                &self.key,
                format!(
                    "{value} is a {}, but the control takes {}",
                    value.kind().type_name(),
                    unit_law.unit.type_name()
                ),
                format!("Write the value in {}", unit_law.unit.type_name()),
            ));
        }
        let limits = self.physical_limits();
        let nearest = |toward_max: bool| {
            let bound = if toward_max {
                self.stored_max
            } else {
                self.stored_min
            };
            self.physical(bound)
                .map_or_else(|_| format!("stored {bound:?}"), |value| value.to_string())
        };
        let stored = unit_law.law.to_stored(value.get()).map_err(|error| {
            ParamError::new(
                &self.key,
                format!("{value} has no stored value: {error}"),
                format!(
                    "Use a value from {limits}; the nearest is {}",
                    nearest(false)
                ),
            )
            .with_source(error)
        })?;
        // A law round trip can land a few ulps past a bound; that is the bound.
        let slack = 1e-9 * (self.stored_max - self.stored_min);
        let stored = if (self.stored_max..=self.stored_max + slack).contains(&stored) {
            self.stored_max
        } else if (self.stored_min - slack..=self.stored_min).contains(&stored) {
            self.stored_min
        } else {
            stored
        };
        if !(self.stored_min..=self.stored_max).contains(&stored) {
            return Err(ParamError::new(
                &self.key,
                format!(
                    "{value} is stored as {stored:?}, outside {}",
                    self.range_text()
                ),
                format!(
                    "Use a value from {limits}; the nearest is {}",
                    nearest(stored > self.stored_max)
                ),
            ));
        }
        if self.kind != ParamKind::Float && stored.fract() != 0.0 {
            return Err(ParamError::new(
                &self.key,
                format!("{value} is stored as {stored:?}, not a whole number"),
                "Pick one whole step; the registry never rounds".to_owned(),
            ));
        }
        Ok(stored)
    }

    /// The physical range as text, such as `12.978 Hz to 21096 Hz`, or the
    /// stored range when a limit has no physical value.
    fn physical_limits(&self) -> String {
        match (
            self.physical(self.stored_min),
            self.physical(self.stored_max),
        ) {
            (Ok(low), Ok(high)) if low.get() <= high.get() => format!("{low} to {high}"),
            (Ok(low), Ok(high)) => format!("{high} to {low}"),
            _ => format!("stored {}", self.range_text()),
        }
    }
}

/// The parameter registry of one synth.
pub trait ParamRegistry: fmt::Debug + Send + Sync {
    /// The synth the registry describes.
    fn synth(&self) -> Synth;

    /// Every entry, sorted by key.
    fn entries(&self) -> &[ParamSpec];

    /// The entry of a key.
    fn get(&self, key: &str) -> Option<&ParamSpec>;

    /// The calibration tables loaded into the registry.
    fn calibration(&self) -> &CalibrationSet;

    /// Every entry, sorted by key.
    fn iter(&self) -> std::slice::Iter<'_, ParamSpec> {
        self.entries().iter()
    }

    /// Up to `limit` keys close to a wrong key, nearest first.
    ///
    /// A key qualifies when its edit distance is at most
    /// `max(2, len / 3)` of the wrong key, or it contains the wrong text.
    /// Ties sort by key.
    fn close_names(&self, wrong: &str, limit: usize) -> Vec<&str> {
        let threshold = (wrong.chars().count() / 3).max(2);
        let mut ranked: Vec<(usize, &str)> = self
            .iter()
            .filter_map(|spec| {
                let distance = edit_distance(wrong, &spec.key);
                (distance <= threshold || (!wrong.is_empty() && spec.key.contains(wrong)))
                    .then_some((distance, spec.key.as_str()))
            })
            .collect();
        ranked.sort_unstable();
        ranked.into_iter().take(limit).map(|(_, key)| key).collect()
    }
}

/// The Levenshtein distance between two texts, by characters.
fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let next = (diagonal + usize::from(ca != *cb))
                .min(row[j] + 1)
                .min(row[j + 1] + 1);
            diagonal = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}

/// A registry built from a list of entries.
#[derive(Clone, Debug)]
pub struct Registry {
    synth: Synth,
    entries: Vec<ParamSpec>,
    index: HashMap<String, usize>,
    calibration: CalibrationSet,
}

impl Registry {
    /// Sorts the entries by key and indexes them.
    ///
    /// # Errors
    ///
    /// A message naming a key that appears twice.
    pub fn new(synth: Synth, mut entries: Vec<ParamSpec>) -> Result<Self, String> {
        entries.sort_by(|a, b| a.key.cmp(&b.key));
        if let Some(pair) = entries.windows(2).find(|pair| pair[0].key == pair[1].key) {
            return Err(format!("key {:?} appears twice", pair[0].key));
        }
        let index = entries
            .iter()
            .enumerate()
            .map(|(i, spec)| (spec.key.clone(), i))
            .collect();
        Ok(Self {
            synth,
            entries,
            index,
            calibration: CalibrationSet {
                synth,
                tables: Vec::new(),
            },
        })
    }

    /// A copy of the registry with the laws of a calibration set. Each
    /// table replaces the law of its key, sets the provenance to
    /// [`Provenance::Measured`] and `verified_on` to the table's build.
    ///
    /// # Errors
    ///
    /// A [`CalibrationError`] when the set is for another synth, a key is
    /// unknown or has no unit, or a table leaves the stored range.
    pub fn with_calibration(&self, set: &CalibrationSet) -> Result<Self, CalibrationError> {
        if set.synth != self.synth {
            return Err(CalibrationError(format!(
                "the calibration set is for {}, but the registry is for {}",
                set.synth.name(),
                self.synth.name()
            )));
        }
        let mut calibrated = self.clone();
        for table in &set.tables {
            let Some(&i) = calibrated.index.get(&table.key) else {
                return Err(CalibrationError(format!(
                    "calibration table for unknown key {:?}",
                    table.key
                )));
            };
            let spec = &mut calibrated.entries[i];
            let Some(unit_law) = spec.law.as_mut() else {
                return Err(CalibrationError(format!(
                    "{}: the control claims no physical unit, so a table cannot calibrate it",
                    table.key
                )));
            };
            let law = Table::new(table.points.clone(), table.interp)
                .map_err(|cause| CalibrationError(format!("{}: {cause}", table.key)))?;
            if let Some((stored, _)) = law
                .points()
                .iter()
                .find(|(stored, _)| !(spec.stored_min..=spec.stored_max).contains(stored))
            {
                return Err(CalibrationError(format!(
                    "{}: table point at stored {stored:?} is outside {}",
                    table.key,
                    spec.range_text()
                )));
            }
            unit_law.law = Law::Table(law);
            spec.provenance = Provenance::Measured;
            spec.verified_on = Some(table.build.clone());
            spec.evidence.clone_from(&table.evidence);
            calibrated.calibration.tables.push(table.clone());
        }
        Ok(calibrated)
    }
}

impl ParamRegistry for Registry {
    fn synth(&self) -> Synth {
        self.synth
    }

    fn entries(&self) -> &[ParamSpec] {
        &self.entries
    }

    fn get(&self, key: &str) -> Option<&ParamSpec> {
        self.index.get(key).map(|&i| &self.entries[i])
    }

    fn calibration(&self) -> &CalibrationSet {
        &self.calibration
    }
}

/// A measured law for one key, as a render or probe produced it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationTable {
    /// The key it calibrates.
    pub key: String,
    /// The build it was measured on.
    pub build: Build,
    /// The render or probe behind it.
    pub evidence: String,
    /// The interpolation between points.
    pub interp: Interp,
    /// `(stored, physical)` points, stored values increasing.
    pub points: Vec<(f64, f64)>,
}

/// The calibration tables of one synth.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationSet {
    /// The synth the tables belong to.
    pub synth: Synth,
    /// The tables.
    pub tables: Vec<CalibrationTable>,
}

impl CalibrationSet {
    /// Reads a set from JSON: `{"synth": "vital", "tables": [...]}`.
    /// Unknown fields are an error.
    ///
    /// # Errors
    ///
    /// A [`CalibrationError`] with the reader's message and position.
    pub fn from_json(text: &str) -> Result<Self, CalibrationError> {
        serde_json::from_str(text)
            .map_err(|error| CalibrationError(format!("cannot read the calibration set: {error}")))
    }
}

/// A calibration set that cannot load into a registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalibrationError(String);

impl fmt::Display for CalibrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Error for CalibrationError {}

/// The registry of a synth.
///
/// # Errors
///
/// [`SynthError::NotBuilt`] for a synth outside [`Synth::READY`], and
/// [`SynthError::BrokenData`] if the built-in data does not load.
pub fn registry_for(synth: Synth) -> Result<&'static dyn ParamRegistry, SynthError> {
    match synth.ready()? {
        Synth::Vital => {
            crate::preset::vital::registry().map(|registry| registry as &'static dyn ParamRegistry)
        }
        Synth::Serum2 => Err(SynthError::NotBuilt(synth)),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use proptest::prelude::*;

    /// One entry per law form, for the round-trip and monotone tests.
    pub(crate) fn law_cases() -> Vec<(&'static str, Law, f64, f64, UnitKind)> {
        vec![
            (
                "affine",
                Law::Affine {
                    scale: 100.0,
                    offset: 0.0,
                },
                -1.0,
                1.0,
                UnitKind::PercentBipolar,
            ),
            (
                "power",
                Law::Power {
                    exponent: 4.0,
                    scale: 1.0,
                    offset: 0.0,
                },
                0.0,
                2.378_42,
                UnitKind::Seconds,
            ),
            (
                "exp2",
                Law::Exp2 {
                    rate: 1.0 / 12.0,
                    scale: 8.175_798_915_6,
                },
                8.0,
                136.0,
                UnitKind::Hz,
            ),
            (
                "db_of_power",
                Law::DbOfPower { exponent: 2.0 },
                0.001,
                1.0,
                UnitKind::Decibels,
            ),
            (
                "table",
                Law::Table(
                    Table::new(
                        vec![(0.0, 100.0), (0.5, 400.0), (1.0, 3200.0)],
                        Interp::LogPhysical,
                    )
                    .unwrap(),
                ),
                0.0,
                1.0,
                UnitKind::Hz,
            ),
            (
                "falling table",
                Law::Table(
                    Table::new(vec![(0.0, 6.0), (0.25, 0.0), (1.0, -30.0)], Interp::Linear)
                        .unwrap(),
                ),
                0.0,
                1.0,
                UnitKind::Decibels,
            ),
            (
                "custom",
                Law::Custom(CustomLaw(Arc::new(Cube))),
                0.0,
                1.0,
                UnitKind::Seconds,
            ),
        ]
    }

    /// A law no closed form covers: `10 * s^3` seconds.
    #[derive(Debug)]
    struct Cube;

    impl ParamLaw for Cube {
        fn to_physical(&self, stored: f64) -> Result<f64, RangeError> {
            Ok(10.0 * stored.powi(3))
        }

        fn to_stored(&self, physical: f64) -> Result<f64, RangeError> {
            Ok((physical / 10.0).cbrt())
        }
    }

    pub(crate) fn spec(key: &str, law: Law, min: f64, max: f64, unit: UnitKind) -> ParamSpec {
        ParamSpec {
            key: key.to_owned(),
            kind: ParamKind::Float,
            stored_min: min,
            stored_max: max,
            default: min,
            law: Some(UnitLaw { unit, law }),
            label: key.to_owned(),
            choices: Vec::new(),
            provenance: Provenance::FromFacts,
            evidence: "test".to_owned(),
            first_build: None,
            verified_on: None,
            tuning_note: None,
            legacy: false,
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1000))]
        #[test]
        fn every_law_round_trips(t in 0.0..=1.0f64) {
            for (name, law, min, max, _) in law_cases() {
                let stored = min + t * (max - min);
                let back = law.to_stored(law.to_physical(stored).unwrap()).unwrap();
                prop_assert!(
                    (back - stored).abs() <= 1e-9 * stored.abs().max(1e-9),
                    "{name}: {stored} came back as {back}"
                );
            }
        }

        #[test]
        fn every_law_is_monotone(a in 0.0..=1.0f64, step in 0.0..=1.0f64) {
            // b >= a by construction: rejecting half of the pairs exhausts
            // proptest's global reject limit.
            let b = a + (1.0 - a) * step;
            for (name, law, min, max, _) in law_cases() {
                let (sa, sb) = (min + a * (max - min), min + b * (max - min));
                let (pa, pb) = (law.to_physical(sa).unwrap(), law.to_physical(sb).unwrap());
                let rising = law.to_physical(max).unwrap() > law.to_physical(min).unwrap();
                prop_assert!(if rising { pa <= pb } else { pa >= pb }, "{name}: {sa} -> {pa}, {sb} -> {pb}");
                // Every law case is strictly monotone. Points a few ulps
                // apart may still round to one physical value.
                if sb - sa > 1e-9 * (max - min) {
                    prop_assert!(if rising { pa < pb } else { pa > pb }, "{name}: {sa} -> {pa}, {sb} -> {pb}");
                }
            }
        }
    }

    #[test]
    fn values_outside_the_domain_name_the_key_and_range() {
        for (name, law, min, max, unit) in law_cases() {
            let entry = spec(name, law, min, max, unit);
            let message = entry.physical(max + 1.0).unwrap_err().to_string();
            assert!(message.starts_with(&format!("{name}: ")), "{message}");
            assert!(message.contains(&entry.range_text()), "{message}");
        }
        let power = spec(
            "env_1_attack",
            Law::Power {
                exponent: 4.0,
                scale: 1.0,
                offset: 0.0,
            },
            0.0,
            2.378_42,
            UnitKind::Seconds,
        );
        let message = power
            .stored(&Physical::new(UnitKind::Seconds, 40.0).unwrap())
            .unwrap_err()
            .to_string();
        assert!(
            message.starts_with("env_1_attack: 40 s is stored as"),
            "{message}"
        );
        assert!(message.contains("0 s to 32"), "{message}");
        let wrong_unit = power
            .stored(&Physical::new(UnitKind::Hz, 4.0).unwrap())
            .unwrap_err()
            .to_string();
        assert!(wrong_unit.contains("takes Seconds"), "{wrong_unit}");
    }

    #[test]
    fn domain_edges_are_errors() {
        let power = Law::Power {
            exponent: 2.0,
            scale: 1.0,
            offset: 0.0,
        };
        assert!(power.to_physical(-0.1).is_err());
        assert!(power.to_stored(-1.0).is_err());
        let db = Law::DbOfPower { exponent: 2.0 };
        assert!(db.to_physical(0.0).is_err());
        let exp2 = Law::Exp2 {
            rate: 1.0,
            scale: 1.0,
        };
        assert!(exp2.to_stored(0.0).is_err());
        assert!(exp2.to_physical(f64::NAN).is_err());
        assert!(exp2.to_physical(5000.0).is_err());
    }

    #[test]
    fn tables_never_extrapolate() {
        let table = Table::new(vec![(0.0, 1.0), (1.0, 2.0)], Interp::Linear).unwrap();
        assert_eq!(table.to_physical(0.5).unwrap(), 1.5);
        assert!(table.to_physical(1.01).is_err());
        assert!(table.to_stored(2.5).is_err());
    }

    #[test]
    fn bad_tables_are_rejected() {
        assert!(Table::new(vec![(0.0, 1.0)], Interp::Linear).is_err());
        assert!(Table::new(vec![(0.0, 1.0), (0.0, 2.0)], Interp::Linear).is_err());
        assert!(Table::new(vec![(0.0, 1.0), (1.0, 2.0), (2.0, 1.5)], Interp::Linear).is_err());
        assert!(Table::new(vec![(0.0, 1.0), (1.0, 1.0)], Interp::Linear).is_err());
        assert!(Table::new(vec![(0.0, 0.0), (1.0, 1.0)], Interp::LogPhysical).is_err());
        assert!(Table::new(vec![(0.0, f64::NAN), (1.0, 1.0)], Interp::Linear).is_err());
    }

    #[test]
    fn whole_kinds_never_round() {
        let mut entry = spec(
            "osc_1_transpose",
            Law::Affine {
                scale: 1.0,
                offset: 0.0,
            },
            -48.0,
            48.0,
            UnitKind::Semitones,
        );
        entry.kind = ParamKind::Count;
        assert_eq!(
            entry
                .stored(&Physical::new(UnitKind::Semitones, 12.0).unwrap())
                .unwrap(),
            12.0
        );
        let message = entry
            .stored(&Physical::new(UnitKind::Semitones, 12.5).unwrap())
            .unwrap_err()
            .to_string();
        assert!(message.contains("not a whole number"), "{message}");
        assert!(entry.physical(1.5).is_err());
    }

    #[test]
    fn parse_uses_the_unit_string_parser() {
        assert_eq!(
            Physical::parse(UnitKind::Hz, "1.2 kHz").unwrap(),
            Physical::Hz(Hz::new(1200.0).unwrap())
        );
        let wrong = Physical::parse(UnitKind::Hz, "1.2 s").unwrap_err();
        assert_eq!(wrong, "1.2 s".parse::<Hz>().unwrap_err());
        assert!(Physical::parse(UnitKind::Normalized, "0.5").is_err());
        assert_eq!(
            Physical::parse(UnitKind::PercentBipolar, "-40%")
                .unwrap()
                .get(),
            -40.0
        );
    }

    #[test]
    fn edit_distance_counts_single_changes() {
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("cutoff", "cutoff"), 0);
        assert_eq!(edit_distance("cuttoff", "cutoff"), 1);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
    }

    fn small_registry() -> Registry {
        let entries = law_cases()
            .into_iter()
            .map(|(name, law, min, max, unit)| spec(&name.replace(' ', "_"), law, min, max, unit))
            .chain([spec(
                "filter_1_cutoff",
                Law::Exp2 {
                    rate: 1.0 / 12.0,
                    scale: 8.175_798_915_6,
                },
                8.0,
                136.0,
                UnitKind::Hz,
            )])
            .collect();
        Registry::new(Synth::Vital, entries).unwrap()
    }

    #[test]
    fn close_names_rank_by_distance() {
        let registry = small_registry();
        assert_eq!(
            registry.close_names("filter_1_cuttoff", 10).first(),
            Some(&"filter_1_cutoff")
        );
        assert_eq!(
            registry.close_names("table", 10),
            ["table", "falling_table"]
        );
        assert_eq!(registry.close_names("zzzzzzzzzzzz", 10), Vec::<&str>::new());
        assert_eq!(registry.close_names("filter", 1).len(), 1);
    }

    #[test]
    fn duplicate_keys_are_rejected() {
        let one = small_registry().entries()[0].clone();
        assert!(Registry::new(Synth::Vital, vec![one.clone(), one]).is_err());
    }

    #[test]
    fn calibration_turns_interim_into_measured() {
        let mut registry_entries = small_registry().entries().to_vec();
        for entry in &mut registry_entries {
            entry.provenance = Provenance::Interim;
            entry.tuning_note = Some("by ear".to_owned());
        }
        let registry = Registry::new(Synth::Vital, registry_entries).unwrap();
        let set = CalibrationSet::from_json(
            r#"{"synth":"vital","tables":[{"key":"filter_1_cutoff","build":"1.5.5","evidence":"render 1","interp":"log_physical","points":[[8.0,20.0],[136.0,20000.0]]}]}"#,
        )
        .unwrap();
        let calibrated = registry.with_calibration(&set).unwrap();
        let cutoff = calibrated.get("filter_1_cutoff").unwrap();
        assert_eq!(cutoff.provenance, Provenance::Measured);
        assert_eq!(cutoff.verified_on, Some(Build("1.5.5".to_owned())));
        assert_eq!(cutoff.evidence, "render 1");
        assert!(matches!(cutoff.law.as_ref().unwrap().law, Law::Table(_)));
        assert_eq!(calibrated.calibration().tables.len(), 1);
        assert_eq!(
            registry.get("filter_1_cutoff").unwrap().provenance,
            Provenance::Interim
        );

        let unknown = CalibrationSet::from_json(
            r#"{"synth":"vital","tables":[{"key":"nope","build":"1","evidence":"e","interp":"linear","points":[[0,0],[1,1]]}]}"#,
        )
        .unwrap();
        assert!(
            registry
                .with_calibration(&unknown)
                .unwrap_err()
                .to_string()
                .contains("unknown key")
        );
        let outside = CalibrationSet::from_json(
            r#"{"synth":"vital","tables":[{"key":"filter_1_cutoff","build":"1","evidence":"e","interp":"linear","points":[[0,1],[200,2]]}]}"#,
        )
        .unwrap();
        assert!(registry.with_calibration(&outside).is_err());
        let serum = CalibrationSet {
            synth: Synth::Serum2,
            tables: Vec::new(),
        };
        assert!(registry.with_calibration(&serum).is_err());
    }

    #[test]
    fn calibration_json_is_strict() {
        assert!(CalibrationSet::from_json(r#"{"synth":"vital","tables":[],"extra":1}"#).is_err());
        assert!(
            CalibrationSet::from_json(
                r#"{"synth":"vital","tables":[{"key":"k","build":"1","evidence":"e","interp":"linear","points":[[0,0],[1,1]],"note":""}]}"#
            )
            .is_err()
        );
    }

    #[test]
    fn serum2_has_no_registry_yet() {
        assert_eq!(
            registry_for(Synth::Serum2).unwrap_err(),
            SynthError::NotBuilt(Synth::Serum2)
        );
    }
}
