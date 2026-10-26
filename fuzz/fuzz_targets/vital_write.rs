//! Models built through the writers write without error and read back
//! equal. The plan maps arbitrary numbers onto valid inputs, so a writer
//! error is a finding, not noise.

#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use sound_to_patch::preset::format::{PresetFormat, PresetModel, RoutePolarity, Stored};
use sound_to_patch::preset::registry::{ParamKind, ParamSpec, registry_for};
use sound_to_patch::preset::vital::{
    FORMAT, FRAME_SAMPLES, Keyframe, LfoShape, MAX_LFO_POINTS, MAX_POSITION, ROUTE_SOURCES,
    VitalModel, Wavetable,
};
use sound_to_patch::synth::Synth;

#[derive(Arbitrary, Debug)]
struct Plan {
    controls: Vec<(u16, f64)>,
    routes: Vec<(u8, u16, f64, Option<bool>, f64)>,
    lfos: Vec<(u8, u8, Vec<f64>, bool)>,
    tables: Vec<(u8, Vec<u16>, f32)>,
    macros: Vec<(u8, String)>,
}

/// A number in 0 to 1 from any float.
fn unit(value: f64) -> f64 {
    if value.is_finite() {
        value.abs().fract()
    } else {
        0.0
    }
}

/// A valid stored value of `spec` from any float.
fn stored(spec: &ParamSpec, value: f64) -> f64 {
    let raw = spec.stored_min + unit(value) * (spec.stored_max - spec.stored_min);
    match spec.kind {
        ParamKind::Float => raw,
        ParamKind::Flag | ParamKind::Choice | ParamKind::Count => {
            raw.round().clamp(spec.stored_min, spec.stored_max)
        }
    }
}

fn build(plan: &Plan, model: &mut VitalModel) -> Result<(), String> {
    let registry = registry_for(Synth::Vital).map_err(|error| error.to_string())?;
    let controls: Vec<&ParamSpec> = registry.iter().filter(|spec| !spec.legacy).collect();
    let pick = |index: u16| controls[usize::from(index) % controls.len()];
    // A route takes any control but an on-off one.
    let targets: Vec<&ParamSpec> = controls
        .iter()
        .copied()
        .filter(|spec| {
            spec.kind != ParamKind::Flag
                && !["_bipolar", "_stereo", "_bypass"]
                    .iter()
                    .any(|flag| spec.key.starts_with("modulation_") && spec.key.ends_with(flag))
        })
        .collect();
    for &(index, value) in &plan.controls {
        let spec = pick(index);
        model
            .set(&spec.key, Stored::Float(stored(spec, value)))
            .map_err(|error| error.to_string())?;
    }
    for &(source, destination, amount, polarity, power) in plan.routes.iter().take(64) {
        let polarity = polarity.map(|bipolar| {
            if bipolar {
                RoutePolarity::Bipolar
            } else {
                RoutePolarity::Unipolar
            }
        });
        model
            .route(
                ROUTE_SOURCES[usize::from(source) % ROUTE_SOURCES.len()],
                &targets[usize::from(destination) % targets.len()].key,
                unit(amount) * 2.0 - 1.0,
                polarity,
                Some(unit(power) * 20.0 - 10.0),
            )
            .map_err(|error| error.to_string())?;
    }
    for (index, count, values, smooth) in &plan.lfos {
        let count = 2 + usize::from(*count) % (MAX_LFO_POINTS - 1);
        let value = |i: usize| values.get(i).copied().map_or(0.0, unit);
        let last = f64::from(u32::try_from(count - 1).map_err(|error| error.to_string())?);
        let points = (0..count)
            .map(|i| Ok((f64::from(u32::try_from(i)?) / last, value(i))))
            .collect::<Result<Vec<_>, std::num::TryFromIntError>>()
            .map_err(|error| error.to_string())?;
        let powers = (0..count).map(|i| value(count + i) * 20.0 - 10.0).collect();
        let shape = LfoShape {
            name: "Fuzz".to_owned(),
            points,
            powers,
            smooth: *smooth,
        };
        model
            .set_lfo_shape(usize::from(*index % 8), &shape)
            .map_err(|error| error.to_string())?;
    }
    for (index, positions, scale) in plan.tables.iter().take(3) {
        let mut positions: Vec<u16> = positions
            .iter()
            .take(4)
            .map(|position| position % (MAX_POSITION + 1))
            .collect();
        positions.sort_unstable();
        positions.dedup();
        if positions.is_empty() {
            positions.push(0);
        }
        let scale = if scale.is_finite() {
            scale.clamp(-1.0, 1.0)
        } else {
            0.5
        };
        let keyframes = positions
            .into_iter()
            .map(|position| Keyframe {
                position,
                samples: (0..FRAME_SAMPLES)
                    .map(|i| {
                        let i = f32::from(u16::try_from(i).unwrap_or(0));
                        scale * (i / 64.0 + f32::from(position)).sin()
                    })
                    .collect(),
            })
            .collect();
        let table = Wavetable {
            name: "Fuzz".to_owned(),
            author: "sound-to-patch".to_owned(),
            keyframes,
        };
        model
            .set_wavetable(usize::from(*index % 3), &table)
            .map_err(|error| error.to_string())?;
    }
    for (number, label) in &plan.macros {
        model
            .set_macro_label(1 + usize::from(*number % 4), label)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fuzz_target!(|plan: Plan| {
    let Some(template) = sound_to_patch::preset::template::TemplateSource::builtin(
        &sound_to_patch::preset::vital::VitalTemplate,
    ) else {
        return;
    };
    let Ok(base) = FORMAT.read(&template) else {
        return;
    };
    let Some(model) = base.as_any().downcast_ref::<VitalModel>() else {
        return;
    };
    let mut model = model.clone();
    let built = build(&plan, &mut model);
    assert!(built.is_ok(), "a valid plan failed: {built:?}");
    let written = FORMAT.write(&model);
    assert!(written.is_ok(), "a built model does not write: {written:?}");
    let written = written.unwrap_or_default();
    let again = FORMAT.read(&written);
    assert!(
        again.is_ok(),
        "a written model does not read: {:?}",
        again.err()
    );
    let Ok(again) = again else { return };
    assert_eq!(again.as_any().downcast_ref::<VitalModel>(), Some(&model));
    assert_eq!(FORMAT.write(again.as_ref()).ok(), Some(written));
});
