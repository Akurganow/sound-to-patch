//! The render core on the fake renderer: the contract suite and its
//! negative cases, repeat renders, faults, the record and the recipe
//! version.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};
use sound_to_patch::render::fake::{FakePatch, FakeRenderer, Fault};
use sound_to_patch::render::{
    HostIdentity, Material, PresetState, Recipe, RenderError, RenderJob, RenderRecord, Rendered,
    Renderer,
};
use sound_to_patch::synth::Synth;
use sound_to_patch::units::{Bpm, Seconds};

use crate::support::render::check_renderer;

fn patch(release: f64) -> FakePatch {
    FakePatch {
        harmonics: vec![0.5, 0.25, 0.125],
        attack: Seconds::new(0.005).unwrap(),
        release: Seconds::new(release).unwrap(),
    }
}

fn state(release: f64) -> PresetState {
    patch(release).state(Synth::Vital).unwrap()
}

fn job(release: f64) -> RenderJob {
    RenderJob::for_material(state(release), Material::Lead, Bpm::new(120.0).unwrap())
}

#[test]
fn the_fake_passes_the_renderer_contract() {
    for synth in Synth::READY {
        let mut renderer = FakeRenderer::new(synth, 7);
        check_renderer(&mut renderer, &patch(0.3).state(synth).unwrap(), false);
    }
}

#[test]
#[should_panic(expected = "is silent")]
fn the_contract_fails_a_silent_renderer() {
    let mut renderer = (0..16).fold(FakeRenderer::new(Synth::Vital, 7), |fake, at| {
        fake.with_fault(at, Fault::Silence)
    });
    check_renderer(&mut renderer, &state(0.3), false);
}

/// A renderer whose right channel is silent.
struct RightSilent(FakeRenderer);

impl Renderer for RightSilent {
    fn identity(&self) -> HostIdentity {
        self.0.identity()
    }

    fn render(&mut self, job: &RenderJob) -> Result<Rendered, RenderError> {
        let rendered = self.0.render(job)?;
        let left = rendered.audio.channels()[0].clone();
        let right = vec![0.0; left.len()];
        Ok(Rendered {
            audio: sound_to_patch::audio::Audio::new(
                rendered.audio.sample_rate(),
                vec![left, right],
            )?,
            record: rendered.record,
        })
    }
}

#[test]
#[should_panic(expected = "channel 1 is silent")]
fn the_contract_fails_a_renderer_with_a_silent_channel() {
    check_renderer(
        &mut RightSilent(FakeRenderer::new(Synth::Vital, 7)),
        &state(0.3),
        false,
    );
}

#[test]
fn one_seed_gives_bit_identical_renders_and_repeats_vary() {
    let first = FakeRenderer::new(Synth::Vital, 7)
        .render(&job(0.3))
        .unwrap();
    let mut renderer = FakeRenderer::new(Synth::Vital, 7);
    let again = renderer.render(&job(0.3)).unwrap();
    assert_eq!(first.audio, again.audio);
    let repeat = renderer.render(&job(0.3)).unwrap();
    assert_ne!(again.audio, repeat.audio);
    let other = FakeRenderer::new(Synth::Vital, 8)
        .render(&job(0.3))
        .unwrap();
    assert_ne!(first.audio, other.audio);
}

#[test]
fn a_tail_heavy_preset_is_not_cut() {
    // The note peaks near 0.6 (velocity 100/127 times three harmonics) and
    // falls 60 dB per 6 s after its note-off at 3 s. It crosses -96 dBFS
    // about 91 dB down, 9.1 s later at 12.1 s, and the quiet second ends
    // the clip near 13.1 s.
    let rendered = FakeRenderer::new(Synth::Vital, 7)
        .render(&job(6.0))
        .unwrap();
    assert!(!rendered.record.tail_capped);
    let length = rendered.record.length.get();
    assert!((12.8..13.4).contains(&length), "{length}");
}

#[test]
fn faults_fail_the_render_they_hit_and_no_other() {
    let mut renderer = FakeRenderer::new(Synth::Vital, 7)
        .with_fault(0, Fault::Crash)
        .with_fault(1, Fault::Hang)
        .with_fault(2, Fault::Silence);
    assert!(matches!(
        renderer.render(&job(0.3)),
        Err(RenderError::Crashed(_))
    ));
    assert!(matches!(
        renderer.render(&job(0.3)),
        Err(RenderError::TimedOut(_))
    ));
    let silent = renderer.render(&job(0.3)).unwrap();
    assert!(silent.audio.channels().iter().flatten().all(|s| *s == 0.0));
    assert!(renderer.render(&job(0.3)).is_ok());
    assert_eq!(renderer.renders(), 4);
}

#[test]
fn a_state_for_another_synth_or_not_a_patch_is_refused() {
    let bpm = Bpm::new(120.0).unwrap();
    let mut renderer = FakeRenderer::new(Synth::Vital, 7);
    let other = RenderJob::for_material(
        patch(0.3).state(Synth::Serum2).unwrap(),
        Material::Lead,
        bpm,
    );
    assert!(matches!(
        renderer.render(&other),
        Err(RenderError::WrongSynth {
            state: Synth::Serum2,
            renderer: Synth::Vital
        })
    ));
    let garbage = RenderJob::for_material(
        PresetState::new(Synth::Vital, b"{}".to_vec()),
        Material::Lead,
        bpm,
    );
    assert!(matches!(
        renderer.render(&garbage),
        Err(RenderError::State(_))
    ));
}

#[test]
fn the_record_survives_a_json_round_trip() {
    let record = FakeRenderer::new(Synth::Vital, 7)
        .render(&job(0.3))
        .unwrap()
        .record;
    assert_eq!(record.identity.arch, "fake");
    let json = serde_json::to_string(&record).unwrap();
    let back: RenderRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(back, record);
}

/// The SHA-256 of the recipe's JSON at each version. A changed field
/// changes the hash: raise `RECIPE_VERSION`, accept the new snapshot and add
/// a row here. Old rows stay as the record of old recipes.
const RECIPE_HASHES: &[(u32, &str)] = &[(
    1,
    "a612dc91900caf260157dd0c89905f0c860e562fe2e9eb07a04e84ecd8916cb9",
)];

fn recipe_json() -> String {
    serde_json::to_string_pretty(&Recipe::current()).unwrap()
}

/// Any change to a recipe field changes this snapshot. The snapshot name
/// carries the version, so the old snapshot stays as the record of the old
/// recipe.
#[test]
fn the_recipe_matches_the_snapshot_of_its_version() {
    insta::assert_snapshot!(
        format!("recipe_v{}", Recipe::current().version()),
        recipe_json()
    );
}

#[test]
fn the_recipe_hash_is_the_one_committed_for_its_version() {
    let version = Recipe::current().version();
    let hash = Sha256::digest(recipe_json())
        .iter()
        .fold(String::new(), |mut hex, byte| {
            write!(hex, "{byte:02x}").unwrap();
            hex
        });
    let committed = RECIPE_HASHES
        .iter()
        .find(|(v, _)| *v == version)
        .map(|(_, h)| *h);
    assert_eq!(
        committed,
        Some(hash.as_str()),
        "the recipe of version {version} changed; raise RECIPE_VERSION"
    );
}
