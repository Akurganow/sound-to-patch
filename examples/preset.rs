//! Writes a Vital preset from an edit list and reads it back, as the
//! preset guide of the book shows. Run it with
//! `cargo run --example preset`; it writes into the system temp folder.

use std::fs;

use sound_to_patch::preset::format::DescribeOptions;
use sound_to_patch::preset::{ApplyRequest, apply_file, inspect};
use sound_to_patch::synth::{Synth, SynthArg};

const EDITS: &str = r#"{
  "synth": "vital",
  "edits": [
    { "param": "filter_1_on", "value": true, "reason": "send the oscillators through filter 1" },
    { "param": "filter_1_cutoff", "value": "400 Hz", "reason": "a dark bass" },
    { "param": "env_1_attack", "value": "5 ms", "reason": "a quick attack" }
  ]
}"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::temp_dir().join("sound-to-patch-example");
    fs::create_dir_all(&dir)?;
    let edits = dir.join("bass.json");
    fs::write(&edits, EDITS)?;
    let out = dir.join("Bass.vital");
    // `--synth vital`: without it, the extension of `out` picks Vital too.
    let synth = SynthArg {
        synth: Some(Synth::Vital),
    };
    let summary = apply_file(&ApplyRequest {
        edits: &edits,
        out: &out,
        template: None,
        synth: &synth,
        name: None,
        // `--force`: a second run replaces the file of the first.
        force: true,
    })?;
    print!("{summary}");
    let shown = inspect(&out, &synth, &DescribeOptions::default())?;
    let header: Vec<&str> = shown.lines().take_while(|line| !line.is_empty()).collect();
    println!("{}", header.join("\n"));
    Ok(())
}
