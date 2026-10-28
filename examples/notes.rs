//! Builds a short saw line in code, runs the notes step on it and prints
//! the notes it kept. Run it with `cargo run --release --example notes`;
//! a debug build takes about a minute, most of it in the f0 track.

use std::f64::consts::TAU;

use sound_to_patch::audio::SampleRate;
use sound_to_patch::notes::{Transcriber, transcribe_stem};
use sound_to_patch::units::Bpm;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A3, C4 and E4 as band-limited saws, 0.2 s on each eighth note at
    // 120 BPM, at 22,050 Hz.
    let rate = 22_050_u32;
    let line = [(0.25, 220.0), (0.5, 261.63), (0.75, 329.63)];
    let mono: Vec<f64> = (0..rate * 6 / 5)
        .map(|n| {
            let t = f64::from(n) / f64::from(rate);
            line.iter()
                .filter(|(onset, _)| (*onset..onset + 0.2).contains(&t))
                .map(|(_, f0)| {
                    (1_u32..100)
                        .map(f64::from)
                        .take_while(|h| h * f0 < f64::from(rate) / 2.0)
                        .map(|h| 0.3 * (TAU * h * f0 * t).sin() / h)
                        .sum::<f64>()
                })
                .sum()
        })
        .collect();

    let found = transcribe_stem(
        &Transcriber::new()?,
        &mono,
        SampleRate::new(rate)?,
        Bpm::new(120.0)?,
        None,
    )?;
    println!(
        "{} voicing, MIDI {} to {}",
        found.detection.voicing.name(),
        found.settings.range.lowest().get(),
        found.settings.range.highest().get()
    );
    println!(
        "{} decoded, {} kept",
        found.steps.decoded,
        found.notes.notes().len()
    );
    for note in found.notes.notes() {
        println!(
            "{:.3} s to {:.3} s: MIDI {}",
            note.onset().get(),
            note.offset().get(),
            note.pitch().get()
        );
    }
    Ok(())
}
