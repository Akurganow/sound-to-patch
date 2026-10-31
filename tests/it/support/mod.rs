//! Helpers shared by the integration tests: the contract suites that every
//! synth adapter must pass, an in-memory adapter that proves the suites
//! pass and fail where they should, seeded noise, golden vectors and test
//! signals.

pub(crate) mod apply;
pub(crate) mod cast;
pub(crate) mod envelope;
pub(crate) mod fixtures;
pub(crate) mod golden;
pub(crate) mod memory;
pub(crate) mod pitch;
pub(crate) mod render;
pub(crate) mod rng;
pub(crate) mod signals;
pub(crate) mod vita;

use std::panic::{self, AssertUnwindSafe};

use sound_to_patch::preset::format::{DescribeOptions, PresetFormat, PresetModel};
use sound_to_patch::preset::mapper::{
    MapContext, Mapped, MotionFxGroups, PitchEnvelopeGroups, SynthMapper, TimbreGroups,
};
use sound_to_patch::preset::registry::{ParamKind, ParamRegistry, Provenance};
use sound_to_patch::preset::template::{Severity, locate_template};
use sound_to_patch::synth::Synth;

/// Runs `check` once for every synth that has an adapter.
pub(crate) fn for_each_ready_synth(mut check: impl FnMut(Synth)) {
    for synth in Synth::READY {
        check(synth);
    }
}

/// The rules every registry meets. Returns the first broken rule.
pub(crate) fn registry_contract(registry: &dyn ParamRegistry) -> Result<(), String> {
    let entries = registry.entries();
    if let Some(pair) = entries.windows(2).find(|pair| pair[0].key >= pair[1].key) {
        return Err(format!(
            "keys are not unique and sorted: {:?} before {:?}",
            pair[0].key, pair[1].key
        ));
    }
    if registry.iter().count() != entries.len() {
        return Err("iter() and entries() differ".into());
    }
    for spec in entries {
        let key = &spec.key;
        if registry.get(key).map(|found| &found.key) != Some(key) {
            return Err(format!("{key}: get() does not find it"));
        }
        if spec.stored_min >= spec.stored_max {
            return Err(format!("{key}: range {} is empty", spec.range_text()));
        }
        if !(spec.stored_min..=spec.stored_max).contains(&spec.default) {
            return Err(format!(
                "{key}: default {} is outside {}",
                spec.default,
                spec.range_text()
            ));
        }
        if spec.label.is_empty() {
            return Err(format!("{key}: no label"));
        }
        if spec.evidence.is_empty() {
            return Err(format!("{key}: no evidence"));
        }
        if spec.provenance == Provenance::Interim && spec.tuning_note.is_none() {
            return Err(format!("{key}: interim without a tuning note"));
        }
        if spec.provenance == Provenance::Measured && spec.verified_on.is_none() {
            return Err(format!("{key}: measured without verified_on"));
        }
        if spec.kind == ParamKind::Float
            && spec.law.is_none()
            && spec.provenance != Provenance::Interim
        {
            return Err(format!("{key}: a float without a unit must be interim"));
        }
        if spec.law.is_some() {
            for (name, bound) in [
                ("min", spec.stored_min),
                ("default", spec.default),
                ("max", spec.stored_max),
            ] {
                // A range end may lie outside the law's domain, such as
                // stored 0 under a decibel law. Any other value must map.
                let Ok(value) = spec.physical(bound) else {
                    if name == "default" {
                        return Err(format!("{key}: the default {bound} has no physical value"));
                    }
                    continue;
                };
                let back = spec.stored(&value).map_err(|error| {
                    format!("{key}: stored {bound} does not round-trip: {error}")
                })?;
                if (back - bound).abs() > 1e-9 * (spec.stored_max - spec.stored_min) {
                    return Err(format!("{key}: stored {bound} came back as {back}"));
                }
            }
        }
        if spec.kind == ParamKind::Choice {
            let labels = spec.stored_max - spec.stored_min + 1.0;
            let count = f64::from(u32::try_from(spec.choices.len()).unwrap());
            if (count - labels).abs() > 0.5 {
                return Err(format!(
                    "{key}: {} labels for {labels} choices",
                    spec.choices.len()
                ));
            }
        }
    }
    Ok(())
}

/// The full describe tree, which compares models bit for bit: every float
/// prints its shortest round-trip text.
fn full(model: &dyn PresetModel) -> String {
    model
        .describe(&DescribeOptions {
            full: true,
            shown: 0,
        })
        .to_string()
}

/// Whether `read` returns an error and does not panic.
fn rejects(format: &dyn PresetFormat, bytes: &[u8]) -> Result<(), String> {
    match panic::catch_unwind(AssertUnwindSafe(|| format.read(bytes).is_err())) {
        Ok(true) => Ok(()),
        Ok(false) => Err(format!(
            "read accepted {} bytes of a cut input",
            bytes.len()
        )),
        Err(_) => Err(format!("read panicked on {} bytes", bytes.len())),
    }
}

/// The rules every format meets on valid sample files. Returns the first
/// broken rule.
pub(crate) fn format_contract(format: &dyn PresetFormat, samples: &[&[u8]]) -> Result<(), String> {
    rejects(format, &[])?;
    for sample in samples {
        let model = format
            .read(sample)
            .map_err(|error| format!("a sample does not read: {error}"))?;
        let written = format
            .write(model.as_ref())
            .map_err(|error| format!("a sample does not write: {error}"))?;
        let again = format
            .read(&written)
            .map_err(|error| format!("a written sample does not read: {error}"))?;
        if model.unknown_keys() != again.unknown_keys() {
            return Err(format!(
                "unknown keys changed: {:?} became {:?}",
                model.unknown_keys(),
                again.unknown_keys()
            ));
        }
        if full(model.as_ref()) != full(again.as_ref()) {
            return Err("read, write, read does not give an equal model".into());
        }
        let header = format
            .read_header(sample)
            .map_err(|error| format!("the header of a sample does not read: {error}"))?;
        if header != model.header() {
            return Err("read_header differs from read().header()".into());
        }
        if header.fields.first().map(|field| field.label.as_str()) != Some("build") {
            return Err("the first header field is not `build`".into());
        }
        rejects(format, &sample[..sample.len() / 2])?;
        rejects(format, &sample[..sample.len() - 1])?;
    }
    let template = locate_template(format.template(), None, format.max_bytes())
        .map_err(|error| format!("no template: {error}"))?;
    let model = format
        .read(&template.bytes)
        .map_err(|error| format!("the template does not read: {error}"))?;
    if let Some(check) = format
        .template()
        .check(model.as_ref())
        .into_iter()
        .find(|check| check.severity == Severity::Error)
    {
        return Err(format!("the template fails its check: {check}"));
    }
    Ok(())
}

/// The rules every mapper meets on the empty inputs: no edit and no
/// `unrepresentable` entry. The mapper tasks add cases with real groups.
pub(crate) fn mapper_contract(
    mapper: &dyn SynthMapper,
    ctx: &MapContext<'_>,
) -> Result<(), String> {
    let empty = Mapped::default();
    for (group, mapped) in [
        (
            "pitch_envelope",
            mapper.pitch_envelope(&PitchEnvelopeGroups::default(), ctx),
        ),
        ("timbre", mapper.timbre(&TimbreGroups::default(), ctx)),
        (
            "motion_fx",
            mapper.motion_fx(&MotionFxGroups::default(), ctx),
        ),
    ] {
        if mapped != empty {
            return Err(format!("{group} maps empty input to {mapped:?}"));
        }
    }
    Ok(())
}
