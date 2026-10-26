//! Any text runs through the edit-list parser, the resolver and the
//! applier on the routes fixture without a panic. An edit list the applier
//! accepts gives a model that writes and reads back.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sound_to_patch::preset::apply::apply;
use sound_to_patch::preset::edits::{self, Path};
use sound_to_patch::preset::format::PresetFormat;
use sound_to_patch::preset::registry::registry_for;
use sound_to_patch::preset::vital::FORMAT;
use sound_to_patch::synth::Synth;

const TEMPLATE: &[u8] = include_bytes!("../../tests/fixtures/vital/routes.vital");

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(list) = edits::parse(text, Path::Agent) else {
        return;
    };
    let Ok(registry) = registry_for(Synth::Vital) else {
        return;
    };
    let Ok(resolved) = edits::resolve(&list, registry) else {
        return;
    };
    let Ok(mut model) = FORMAT.read(TEMPLATE) else {
        return;
    };
    if apply(&FORMAT, registry, model.as_mut(), &resolved).is_err() {
        return;
    }
    let written = FORMAT.write(model.as_ref());
    assert!(
        written.is_ok(),
        "an applied model does not write: {written:?}"
    );
    let written = written.unwrap_or_default();
    let again = FORMAT.read(&written);
    assert!(
        again.is_ok(),
        "a written model does not read: {:?}",
        again.err()
    );
});
