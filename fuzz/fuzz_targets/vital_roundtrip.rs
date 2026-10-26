//! Every input the `.vital` reader accepts writes, and the written bytes
//! read back to an equal model that writes the same bytes. Comparing the
//! bytes compares floats by their text, so `-0.0` and `0.0` differ.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sound_to_patch::preset::format::PresetFormat;
use sound_to_patch::preset::vital::{FORMAT, VitalModel};

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
    let Ok(again) = again else { return };
    assert_eq!(
        model.as_any().downcast_ref::<VitalModel>(),
        again.as_any().downcast_ref::<VitalModel>()
    );
    assert_eq!(FORMAT.write(again.as_ref()).ok(), Some(written));
});
