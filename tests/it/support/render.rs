//! The contract suite that every renderer passes: the fake in CI, the
//! plug-in host and the open engine as ignored tests.

use sound_to_patch::audio::Audio;
use sound_to_patch::notes::Velocity;
use sound_to_patch::render::{
    Material, PresetState, RECIPE_VERSION, RenderJob, RenderNote, Renderer,
};
use sound_to_patch::units::{Bpm, Seconds};

/// The minimal Vital fixture named `Probe`, with `edit` applied to the
/// whole preset, as a render state. Both Vital engines render from it.
pub(crate) fn probe(edit: impl FnOnce(&mut serde_json::Value)) -> PresetState {
    let mut preset: serde_json::Value =
        serde_json::from_slice(&std::fs::read("tests/fixtures/vital/minimal.vital").unwrap())
            .unwrap();
    preset["preset_name"] = "Probe".into();
    edit(&mut preset);
    sound_to_patch::render::state::preset_state(
        sound_to_patch::synth::Synth::Vital,
        &serde_json::to_vec(&preset).unwrap(),
        &sound_to_patch::render::Recipe::current(),
    )
    .unwrap()
}

/// The peak a clip must exceed to count as sound: about -80 dBFS.
const SOUND: f64 = 1e-4;

/// The peak of each channel of `audio` over the frames `range`.
fn peaks(audio: &Audio, range: std::ops::Range<usize>) -> Vec<f64> {
    audio
        .channels()
        .iter()
        .map(|channel| {
            channel[range.clone()]
                .iter()
                .fold(0.0_f64, |a, s| a.max(s.abs()))
        })
        .collect()
}

fn note(key: u8, onset: f64, offset: f64) -> RenderNote {
    RenderNote::new(
        key,
        Seconds::new(onset).unwrap(),
        Seconds::new(offset).unwrap(),
        Velocity::new(100).unwrap(),
    )
    .unwrap()
}

/// Checks `renderer` with a preset `state` that sounds within 10 ms of a
/// note-on in both channels. `single_note` limits the suite to one note
/// per job, for the open engine.
pub(crate) fn check_renderer(renderer: &mut dyn Renderer, state: &PresetState, single_note: bool) {
    let bpm = Bpm::new(120.0).unwrap();
    let identity = renderer.identity();
    assert_eq!(
        identity.synth,
        state.synth(),
        "the renderer renders another synth"
    );

    let materials: &[Material] = if single_note {
        &[Material::Bass, Material::Lead]
    } else {
        &[Material::Bass, Material::Lead, Material::Pad]
    };
    for &material in materials {
        let job = RenderJob::for_material(state.clone(), material, bpm);
        let output = renderer.render(&job).unwrap();
        let audio = &output.audio;
        let rate = job.recipe().sample_rate();
        assert_eq!(
            audio.sample_rate(),
            rate,
            "{material:?}: the rate is not the recipe's"
        );
        assert_eq!(
            audio.channels().len(),
            2,
            "{material:?}: the clip is not stereo"
        );
        let length = rate.seconds(audio.frames());
        assert!(
            length >= job.recipe().clip(),
            "{material:?}: the clip is shorter than the recipe's"
        );
        for (channel, peak) in peaks(audio, 0..audio.frames()).into_iter().enumerate() {
            assert!(peak > SOUND, "{material:?}: channel {channel} is silent");
        }

        let record = &output.record;
        assert_eq!(record.identity, identity);
        assert_eq!(record.recipe_version, RECIPE_VERSION);
        assert_eq!(record.sample_rate, rate.get());
        assert_eq!(record.bpm, bpm);
        assert_eq!(
            record.length, length,
            "{material:?}: the record names another length"
        );
        assert_eq!(record.last_note_off, job.last_note_off());
        if !record.tail_capped {
            let hold = audio.frames() - rate.get() as usize / 2;
            for (channel, peak) in peaks(audio, hold..audio.frames()).into_iter().enumerate() {
                assert!(
                    peak < SOUND,
                    "{material:?}: channel {channel} still sounds at the end"
                );
            }
        }
    }

    // A note at 1 s: silence before it, sound after it, in both channels.
    let job = RenderJob::new(state.clone(), vec![note(60, 1.0, 2.0)], bpm).unwrap();
    let output = renderer.render(&job).unwrap();
    let rate = output.audio.sample_rate().get() as usize;
    for (channel, peak) in peaks(&output.audio, 0..rate * 99 / 100)
        .into_iter()
        .enumerate()
    {
        assert!(peak < SOUND, "channel {channel} sounds before the note-on");
    }
    for (channel, peak) in peaks(&output.audio, rate..rate * 3 / 2)
        .into_iter()
        .enumerate()
    {
        assert!(
            peak > SOUND,
            "channel {channel} is silent after the note-on"
        );
    }

    if !single_note {
        // Overlapping notes of one key, and a note shorter than a frame:
        // every note ends, so the tail ends before the cap.
        let notes = vec![
            note(60, 0.0, 1.0),
            note(60, 0.5, 1.5),
            note(60, 1.0, 1.000_01),
            note(60, 1.000_01, 2.0),
        ];
        let job = RenderJob::new(state.clone(), notes, bpm).unwrap();
        let output = renderer.render(&job).unwrap();
        assert!(
            !output.record.tail_capped,
            "a note of a repeated key never ended"
        );
    }
}
