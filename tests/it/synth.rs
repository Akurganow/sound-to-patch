//! The shared `--synth` argument as a command sees it.

use clap::{CommandFactory, Parser};
use sound_to_patch::synth::{Synth, SynthArg, SynthError};

use crate::support::for_each_ready_synth;

/// A command that only flattens the shared argument.
#[derive(Debug, Parser)]
#[command(name = "probe")]
struct Probe {
    #[command(flatten)]
    synth: SynthArg,
}

#[test]
fn absent_flag_resolves_to_vital() {
    let probe = Probe::try_parse_from(["probe"]).unwrap();
    assert_eq!(probe.synth.resolve(None), Ok(Synth::Vital));
}

#[test]
fn serum2_is_not_built_yet() {
    let probe = Probe::try_parse_from(["probe", "--synth", "serum2"]).unwrap();
    let error = probe.synth.resolve(None).unwrap_err();
    assert_eq!(error, SynthError::NotBuilt(Synth::Serum2));
    assert_eq!(
        error.to_string(),
        "Serum 2 support is not built yet. Use --synth vital."
    );
}

#[test]
fn the_flag_takes_only_the_two_names() {
    let error = Probe::try_parse_from(["probe", "--synth", "Vital"]).unwrap_err();
    let message = error.to_string();
    assert!(
        message.contains("vital") && message.contains("serum2"),
        "{message}"
    );
}

#[test]
fn help_names_both_values_and_the_default() {
    insta::assert_snapshot!(Probe::command().render_long_help().to_string());
}

#[test]
fn every_ready_synth_has_capabilities_that_hold() {
    for_each_ready_synth(|synth| {
        synth.capabilities().unwrap().check().unwrap();
    });
}
