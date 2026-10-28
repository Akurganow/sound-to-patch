//! The contract suite that every renderer passes: the fake in CI, the
//! plug-in host and the open engine as ignored tests.

use sound_to_patch::notes::Velocity;
use sound_to_patch::render::{
    Material, PresetState, RECIPE_VERSION, RenderJob, RenderNote, Renderer,
};
use sound_to_patch::units::{Bpm, Seconds};

/// The peak a clip must exceed to count as sound: about -80 dBFS.
const SOUND: f64 = 1e-4;

fn peak(samples: &[f64]) -> f64 {
    samples.iter().fold(0.0, |a, s| a.max(s.abs()))
}

/// Checks `renderer` with a preset `state` that sounds within 10 ms of a
/// note-on. `single_note` limits the suite to one note per job, for the
/// open engine.
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
        assert!(
            peak(&audio.channels()[0]) > SOUND,
            "{material:?}: the clip is silent"
        );

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
            let end = audio
                .channels()
                .iter()
                .map(|c| peak(&c[hold..]))
                .fold(0.0, f64::max);
            assert!(
                end < SOUND,
                "{material:?}: the clip ends while the tail still sounds"
            );
        }
    }

    // A note at 1 s: silence before it, sound after it.
    let note = RenderNote::new(
        60,
        Seconds::new(1.0).unwrap(),
        Seconds::new(2.0).unwrap(),
        Velocity::new(100).unwrap(),
    )
    .unwrap();
    let job = RenderJob::new(state.clone(), vec![note], bpm).unwrap();
    let output = renderer.render(&job).unwrap();
    let left = &output.audio.channels()[0];
    let rate = output.audio.sample_rate().get() as usize;
    assert!(
        peak(&left[..rate * 99 / 100]) < SOUND,
        "sound before the note-on"
    );
    assert!(
        peak(&left[rate..rate * 3 / 2]) > SOUND,
        "no sound after the note-on"
    );
}
