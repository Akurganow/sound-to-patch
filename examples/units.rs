//! Parses unit strings, converts between units and writes a confidence as
//! JSON, as the units page of the book shows. Run it with
//! `cargo run --example units`.

use sound_to_serum::confidence::{Confidence, Level, Reason, ReasonCode, Signal};
use sound_to_serum::units::{Bipolar, Cents, Decibels, Hz, MidiPitch, Octaves, Percent, Seconds};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ANCHOR: parse
    let cutoff: Hz = "1.2 kHz".parse()?;
    let pre_delay: Seconds = "20 ms".parse()?;
    let amount: Percent<Bipolar> = "-40%".parse()?;
    // A suffix of another unit is an error that lists the accepted suffixes.
    let wrong = "1.2 s".parse::<Hz>();
    // ANCHOR_END: parse
    println!("{cutoff}, {pre_delay}, {amount}");
    if let Err(error) = wrong {
        println!("{error}");
    }

    // ANCHOR: convert
    let a4 = Hz::from(MidiPitch::new(69.0)?);
    let pitch = MidiPitch::try_from(Hz::new(220.0)?)?;
    let detune = Cents::new(7.0)?.to_ratio()?;
    let gain = Decibels::new(-6.0)?.to_amplitude_gain()?;
    // ANCHOR_END: convert
    println!("{a4}, MIDI {}, ratio {detune}, gain {gain}", pitch.get());

    // ANCHOR: confidence
    let frames = Reason::new(ReasonCode::ActiveFrames, Signal::Count(412))?;
    let cutoff = Confidence::estimate(Level::Medium, Some(Octaves::new(0.3)?), vec![frames])?;
    // {"level":"medium","reasons":[{"code":"active_frames","value":412}],"spread":0.3,"estimate":true}
    println!("{}", serde_json::to_string(&cutoff)?);

    let notes = Reason::new(ReasonCode::TooFewNotes, Signal::Count(2))?;
    let envelope = Confidence::<Seconds>::no_estimate(vec![notes])?;
    // {"reasons":[{"code":"too_few_notes","value":2}],"estimate":false}
    println!("{}", serde_json::to_string(&envelope)?);
    // ANCHOR_END: confidence
    Ok(())
}
