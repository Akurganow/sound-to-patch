//! Applies resolved edits to a model through the format traits. The
//! applier is synth-neutral: the registry supplies keys, ranges and
//! defaults, and the format supplies the route rules and the final scan.

use crate::preset::edits::{EditError, EditErrors, Resolved};
use crate::preset::format::{FormatError, PresetFormat, PresetModel, Route, RoutePolarity, Stored};
use crate::preset::registry::ParamRegistry;

/// What one edit did.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    /// The key, or `slot N` for a route.
    pub target: String,
    /// The value written, as text.
    pub value: String,
    /// The law of the control is interim.
    pub uncalibrated: bool,
    /// The key is outside the registry.
    pub raw: bool,
    /// Why.
    pub reason: Option<String>,
}

/// An edit that changed nothing, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skipped {
    /// The key.
    pub target: String,
    /// Why it was skipped.
    pub why: String,
}

/// What an edit list did to a model.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ApplyReport {
    /// The changes, in edit order.
    pub changes: Vec<Change>,
    /// The edits that changed nothing.
    pub skipped: Vec<Skipped>,
    /// Warnings of the format scan.
    pub warnings: Vec<String>,
}

fn as_number(value: &Stored) -> Option<f64> {
    match value {
        Stored::Float(number) => Some(*number),
        Stored::Int(number) => Some(crate::cast::i64_to_f64(*number)),
        Stored::Flag(_) | Stored::Text(_) => None,
    }
}

/// Puts a route in the slot that already joins the same source and
/// destination, or in the first free slot.
///
/// # Errors
///
/// A [`FormatError`] that names the slot limit when every slot is taken,
/// or the error of [`PresetModel::set_route`].
pub fn place_route(model: &mut dyn PresetModel, route: Route) -> Result<usize, FormatError> {
    let routes = model.routes();
    let same = routes.iter().position(|slot| {
        slot.as_ref().is_some_and(|have| {
            have.source == route.source && have.destination == route.destination
        })
    });
    let slot = same
        .or_else(|| routes.iter().position(Option::is_none))
        .ok_or_else(|| {
            FormatError::new(
                "cannot add the route",
                format!("all {} modulation slots are taken", routes.len()),
                "Remove a route, or reuse a slot with the same source and destination",
            )
        })?;
    model.set_route(slot, Some(route))?;
    Ok(slot)
}

/// Applies resolved edits in order, then runs the format scan.
///
/// A parameter equal to the value already in the model changes nothing. A
/// parameter equal to its default is skipped when the key is absent and
/// the format reads an absent key as the default.
///
/// # Errors
///
/// [`EditErrors`] with one entry per edit the format refuses, or with the
/// scan error. The model may hold the edits before the failing one.
pub fn apply(
    format: &dyn PresetFormat,
    registry: &dyn ParamRegistry,
    model: &mut dyn PresetModel,
    resolved: &[Resolved],
) -> Result<ApplyReport, EditErrors> {
    let mut report = ApplyReport::default();
    let mut errors = Vec::new();
    for (index, edit) in resolved.iter().enumerate() {
        let result = match edit {
            Resolved::Param {
                key,
                stored,
                uncalibrated,
                reason,
            } => apply_param(format, registry, model, key, *stored, &mut report).map(|changed| {
                if changed {
                    let physical = registry.get(key).map_or_else(String::new, |spec| {
                        shown_value(spec, *stored)
                            .map_or_else(String::new, |text| format!("{text}, "))
                    });
                    report.changes.push(Change {
                        target: key.clone(),
                        value: format!("{physical}stored {stored:?}"),
                        uncalibrated: *uncalibrated,
                        raw: false,
                        reason: reason.clone(),
                    });
                }
            }),
            Resolved::Route {
                source,
                destination,
                amount,
                polarity,
                reason,
            } => apply_route(format, model, source, destination, *amount, *polarity).map(|slot| {
                report.changes.push(Change {
                    target: format!("slot {}", slot + 1),
                    value: format!("{source} -> {destination}, amount {amount:?}"),
                    uncalibrated: false,
                    raw: false,
                    reason: reason.clone(),
                });
            }),
            Resolved::Raw { key, value, reason } => model.set(key, value.clone()).map(|()| {
                report.changes.push(Change {
                    target: key.clone(),
                    value: value.to_string(),
                    uncalibrated: false,
                    raw: true,
                    reason: reason.clone(),
                });
            }),
        };
        if let Err(error) = result {
            errors.push(EditError {
                place: format!("edit {}", index + 1),
                cause: format!("{}: {}", error.what, error.cause),
                fix: error.fix,
            });
        }
    }
    if !errors.is_empty() {
        return Err(EditErrors(errors));
    }
    report.warnings = format.scan(model).map_err(|error| {
        EditErrors(vec![EditError {
            place: "the edited preset".to_owned(),
            cause: format!("{}: {}", error.what, error.cause),
            fix: error.fix,
        }])
    })?;
    Ok(report)
}

/// A stored value as people read it: the physical value, the menu label
/// or on and off.
fn shown_value(spec: &crate::preset::registry::ParamSpec, stored: f64) -> Option<String> {
    use crate::preset::registry::ParamKind;
    match spec.kind {
        ParamKind::Flag => Some(if stored == 0.0 { "off" } else { "on" }.to_owned()),
        ParamKind::Choice => {
            let index = (stored - spec.stored_min).max(0.0);
            spec.choices
                .iter()
                .zip(0u32..)
                .find(|(_, i)| (f64::from(*i) - index).abs() < 0.5)
                .map(|(label, _)| label.clone())
        }
        ParamKind::Float | ParamKind::Count => {
            spec.physical(stored).ok().map(|value| value.to_string())
        }
    }
}

fn apply_param(
    format: &dyn PresetFormat,
    registry: &dyn ParamRegistry,
    model: &mut dyn PresetModel,
    key: &str,
    stored: f64,
    report: &mut ApplyReport,
) -> Result<bool, FormatError> {
    format.check_value(key, stored)?;
    let current = model.get(key);
    if current
        .as_ref()
        .and_then(as_number)
        .is_some_and(|value| value.to_bits() == stored.to_bits())
    {
        report.skipped.push(Skipped {
            target: key.to_owned(),
            why: "equals the template".to_owned(),
        });
        return Ok(false);
    }
    let is_default = registry
        .get(key)
        .is_some_and(|spec| spec.default.to_bits() == stored.to_bits());
    if current.is_none() && is_default && format.absent_means_default(key) {
        report.skipped.push(Skipped {
            target: key.to_owned(),
            why: "equals the default".to_owned(),
        });
        return Ok(false);
    }
    model.set(key, Stored::Float(stored))?;
    Ok(true)
}

fn apply_route(
    format: &dyn PresetFormat,
    model: &mut dyn PresetModel,
    source: &str,
    destination: &str,
    amount: f64,
    polarity: Option<RoutePolarity>,
) -> Result<usize, FormatError> {
    if !format.route_sources().contains(&source) {
        return Err(FormatError::new(
            "cannot add the route",
            format!("{source:?} is not a modulation source"),
            format!("Use one of: {}", format.route_sources().join(", ")),
        ));
    }
    place_route(
        model,
        Route {
            source: source.to_owned(),
            destination: destination.to_owned(),
            amount,
            polarity: polarity.unwrap_or_else(|| format.default_polarity(source)),
            extras: Vec::new(),
        },
    )
}
