//! Every input the `.vital` reader accepts writes, and the written bytes
//! read back to an equal model that writes the same bytes. Comparing the
//! bytes compares floats by their text, so `-0.0` and `0.0` differ.
//!
//! A read model also goes through every reader of a model that a command
//! runs on a file from outside: the description, the routes, the unknown
//! keys, the scan, the template check and the three wavetables. Each slot
//! then takes its own route back, and the model still writes.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sound_to_patch::preset::format::{DescribeOptions, PresetFormat, PresetModel};
use sound_to_patch::preset::template::TemplateSource;
use sound_to_patch::preset::vital::{FORMAT, VitalModel, VitalTemplate};

fuzz_target!(|bytes: &[u8]| {
    let Ok(model) = FORMAT.read(bytes) else {
        return;
    };
    let written = FORMAT.write(model.as_ref());
    assert!(written.is_ok(), "a read model does not write: {written:?}");
    let written = written.unwrap_or_default();
    let again = FORMAT.read(&written);
    assert!(
        again.is_ok(),
        "a written model does not read: {:?}",
        again.err()
    );
    let Ok(mut again) = again else { return };
    assert_eq!(
        model.as_any().downcast_ref::<VitalModel>(),
        again.as_any().downcast_ref::<VitalModel>()
    );
    assert_eq!(FORMAT.write(again.as_ref()).ok(), Some(written));
    read_everything(model.as_ref());
    reset_routes(again.as_mut());
});

/// Runs every reader of a model; a failure is an answer, a panic is not.
fn read_everything(model: &dyn PresetModel) {
    model.describe(&DescribeOptions::default());
    model.describe(&DescribeOptions {
        full: true,
        shown: 0,
    });
    model.unknown_keys();
    let _ = FORMAT.scan(model, &[]);
    VitalTemplate.check(model);
    if let Some(vital) = model.as_any().downcast_ref::<VitalModel>() {
        for index in 0..3 {
            let _ = vital.wavetable(index);
        }
    }
}

/// Sets each slot to the route it holds. A model that took every route it
/// was given still writes.
fn reset_routes(model: &mut dyn PresetModel) {
    for (slot, route) in model.routes().into_iter().enumerate() {
        let _ = model.set_route(slot, route);
    }
    let written = FORMAT.write(model);
    assert!(
        written.is_ok(),
        "a model with its routes set again does not write: {written:?}"
    );
}
