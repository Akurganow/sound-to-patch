//! Arbitrary bytes into the `.vital` reader. A read either fails with an
//! error or gives a model whose header equals the fast header read.

#![no_main]

use libfuzzer_sys::fuzz_target;
use sound_to_patch::preset::format::PresetFormat;
use sound_to_patch::preset::vital::FORMAT;

fuzz_target!(|bytes: &[u8]| {
    let header = FORMAT.read_header(bytes);
    if let Ok(model) = FORMAT.read(bytes) {
        assert_eq!(header.ok(), Some(model.header()));
    }
});
