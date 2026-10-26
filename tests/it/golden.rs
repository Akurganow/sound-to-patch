//! The golden-vector helper itself.

use sound_to_patch::dsp::Window;

use crate::support::golden::{Golden, check_close, floats};

#[test]
fn a_stored_vector_passes_with_its_tolerance_and_a_wrong_value_fails() {
    let golden = Golden::load("window");
    let expected = floats(&golden.values()["hann"]["64"]);
    let actual = Window::Hann.samples(64);
    check_close(&actual, &expected, golden.tolerance()).unwrap();
    let mut wrong = expected.clone();
    wrong[17] += 1e-9;
    let message = check_close(&actual, &wrong, golden.tolerance()).unwrap_err();
    assert!(message.starts_with("value 17: got"), "{message}");
    assert!(check_close(&actual[1..], &expected, golden.tolerance()).is_err());
}

#[test]
fn every_vector_states_recipe_generator_tools_values_and_tolerance() {
    for name in [
        "window",
        "stft",
        "hilbert",
        "decay",
        "prototype/48-peak-accuracy",
    ] {
        let golden = Golden::load(name);
        assert!(golden.recipe().is_object(), "{name}");
        assert!(!golden.values().is_null(), "{name}");
        let tolerance = golden.tolerance();
        assert!(tolerance.abs >= 0.0 && tolerance.rel >= 0.0, "{name}");
    }
}
