//! The cleanup of decoded notes (#60) on the octave ghost that the
//! decoder adds above a low saw.

use sound_to_patch::audio::SampleRate;
use sound_to_patch::notes::{
    MODEL_SHA256, Profile, TRANSCRIBER_VERSION, Transcriber, transcribe_stem,
};
use sound_to_patch::units::Bpm;

use crate::support::signals::{bandlimited_saw, times};

/// One A2 (110 Hz) band-limited saw at amplitude 0.3: 0.5 s from 0.5 s
/// with a 5 ms attack, then a 0.3 s linear release, dry, at 22,050 Hz.
fn a2_saw() -> Vec<f64> {
    let rate = 22_050.0;
    let t = times(rate, crate::support::cast::round_to_usize(1.5 * rate));
    bandlimited_saw(110.0, rate, &t, |_| 0.0)
        .into_iter()
        .zip(&t)
        .map(|(value, &time)| {
            let since = time - 0.5;
            let envelope = if since < 0.0 {
                0.0
            } else {
                (since / 0.005).min(1.0) * (1.0 - (since - 0.5).max(0.0) / 0.3).max(0.0)
            };
            0.3 * envelope * value
        })
        .collect()
}

/// The decoder gives the octave ghost at MIDI 57 with the bass profile; the
/// cleanup leaves the one A2.
#[test]
fn the_octave_ghost_of_a_110_hz_saw_merges_into_its_note() {
    let rate = SampleRate::new(22_050).unwrap();
    let bpm = Bpm::new(120.0).unwrap();
    let found = transcribe_stem(
        &Transcriber::new().unwrap(),
        &a2_saw(),
        rate,
        bpm,
        Some(Profile::Bass),
    )
    .unwrap();
    let pitches = |list: &sound_to_patch::notes::NoteList| {
        list.notes()
            .iter()
            .map(|note| note.pitch().get())
            .collect::<Vec<_>>()
    };
    assert!(
        pitches(&found.decoded).contains(&57.0),
        "no ghost to remove: {:?}",
        pitches(&found.decoded)
    );
    assert_eq!(pitches(&found.notes), vec![45.0]);
    assert_eq!(found.steps.octaves.moved, 1);
    assert_eq!(found.notes.notes()[0].octave_uncertain(), Some(false));
    assert!(found.notes.notes()[0].contour().is_some());
    // The record names the model, the crate, the profile and the decoder
    // parameters of this decode.
    assert_eq!(found.record.model_sha256, MODEL_SHA256);
    assert_eq!(found.record.crate_version, TRANSCRIBER_VERSION);
    assert_eq!(found.record.profile, "bass");
    assert_eq!(
        found.record.note_params,
        Profile::Bass.settings(bpm).note_params()
    );
}
