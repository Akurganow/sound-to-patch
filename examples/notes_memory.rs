//! Runs one stage of the notes step on the 60 s stem of the notes bench,
//! so a tool that reports a process's peak memory can measure each stage:
//! `cargo run --release --example notes_memory -- <stage>`, with `stem`
//! (the stem alone), `model`, `track`, `decode` (the model run, then the
//! decode and the cleanup on an empty track) or `step` (the whole step).

use std::error::Error;
use std::f64::consts::TAU;

use sound_to_patch::audio::SampleRate;
use sound_to_patch::notes::{PitchTrack, Profile, Transcriber, Voicing, clean_up, transcribe_stem};
use sound_to_patch::units::Bpm;

/// The model rate, so neither the model nor the track resamples.
const RATE: u32 = 22_050;

/// The bench stem: 60 s at 120 BPM, an eighth note every quarter, saws and
/// sines in turn over two octaves from A2.
fn stem() -> Vec<f64> {
    let rate = f64::from(RATE);
    let pitches = [45.0, 48.0, 52.0, 55.0, 57.0, 60.0, 64.0, 67.0];
    let beat = RATE / 2;
    (0..RATE * 60)
        .map(|n| {
            if n % beat >= beat / 2 {
                return 0.0;
            }
            let t = f64::from(n) / rate;
            let index = usize::try_from(n / beat).unwrap_or(0) % pitches.len();
            let f0 = 440.0 * 2_f64.powf((pitches[index] - 69.0) / 12.0);
            if index.is_multiple_of(2) {
                (1_u32..100)
                    .map(f64::from)
                    .take_while(|h| h * f0 < rate / 2.0)
                    .map(|h| 0.3 * (TAU * h * f0 * t).sin() / h)
                    .sum()
            } else {
                0.3 * (TAU * f0 * t).sin()
            }
        })
        .collect()
}

fn main() -> Result<(), Box<dyn Error>> {
    let stage = std::env::args()
        .nth(1)
        .ok_or("name a stage: stem, model, track, decode or step")?;
    let mono = stem();
    let rate = SampleRate::new(RATE)?;
    let bpm = Bpm::new(120.0)?;
    let count = match stage.as_str() {
        "stem" => mono.len(),
        "model" => Transcriber::new()?
            .run(&mono, rate)?
            .posteriorgrams()
            .note
            .n_frames(),
        "track" => PitchTrack::of(&mono, rate)?.frames().len(),
        "decode" => {
            let run = Transcriber::new()?.run(&mono, rate)?;
            let decoded = run.decode(&Profile::Lead.settings(bpm))?;
            let empty = PitchTrack::default();
            let (kept, _) = clean_up(&decoded, &run, &empty, Voicing::Monophonic, &mono, rate);
            kept.notes().len()
        }
        "step" => transcribe_stem(&Transcriber::new()?, &mono, rate, bpm, None)?
            .notes
            .notes()
            .len(),
        other => return Err(format!("unknown stage `{other}`").into()),
    };
    println!("{stage}: {count}");
    Ok(())
}
