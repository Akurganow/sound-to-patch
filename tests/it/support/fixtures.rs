//! Helpers for the fixture tests.

use sound_to_patch::preset::format::{PresetFormat, PresetModel};
use sound_to_patch::preset::vital::{FORMAT, VitalModel, VitalVersion};

pub(crate) fn read(bytes: &[u8]) -> VitalModel {
    let model = FORMAT.read(bytes).unwrap();
    model.as_any().downcast_ref::<VitalModel>().unwrap().clone()
}

pub(crate) fn stamp(model: &VitalModel) -> VitalVersion {
    model.header().fields[0].value.parse().unwrap()
}
