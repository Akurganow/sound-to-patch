//! A fake renderer for tests: a seeded additive synth with no plug-in.
//!
//! Its patch is JSON in the preset state, under `settings` as in a Vital
//! state, so the load check reads it back the same way. Each note sums the patch's
//! harmonics under a linear attack and an exponential release, and a
//! render draws a random phase per harmonic and a small level jitter, so
//! repeat renders vary the way a synth with random unison phases does.
//! Faults stand in for a plug-in that crashes, hangs or stays silent. A
//! hang is simulated: the render returns [`RenderError::TimedOut`] at once
//! and never waits.
//!
//! The fake keeps no sound between renders, so it plays no pre-roll. It
//! remembers the last state it loaded, which [`Renderer::save_state`]
//! returns; before any load it holds [`FakePatch::init`].

use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};

use super::{
    EventKind, HostIdentity, PresetState, RenderError, RenderJob, RenderRecord, Rendered, Renderer,
    note_events,
};
use crate::audio::Audio;
use crate::cast;
use crate::synth::Synth;
use crate::units::Seconds;

/// The level jitter of a render: each note's level varies by up to this
/// fraction.
const LEVEL_JITTER: f64 = 0.005;
/// The release falls 60 dB over the release time.
const RELEASE_DB: f64 = 60.0;

/// The sound of the fake synth.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FakePatch {
    /// The amplitude of each harmonic, the fundamental first.
    pub harmonics: Vec<f64>,
    /// The attack time.
    pub attack: Seconds,
    /// The time the release takes to fall 60 dB.
    pub release: Seconds,
}

impl FakePatch {
    /// The fake's init patch: a bright tone with a short release.
    #[must_use]
    pub fn init() -> Self {
        Self {
            harmonics: vec![1.0, 0.5, 0.33, 0.25, 0.2],
            attack: Seconds::from_const(0.005),
            release: Seconds::from_const(0.2),
        }
    }

    /// Returns the patch as the state of `synth`.
    ///
    /// # Errors
    ///
    /// Returns the serializer's error. A patch of finite numbers always
    /// serializes.
    pub fn state(&self, synth: Synth) -> Result<PresetState, serde_json::Error> {
        let root = serde_json::json!({ "preset_name": "fake", "settings": self });
        Ok(PresetState::new(synth, serde_json::to_vec(&root)?))
    }

    /// Reads a patch from a state.
    fn of_state(state: &[u8]) -> Result<Self, RenderError> {
        let not_fake = |cause: String| RenderError::State(format!("not a fake patch ({cause})"));
        let root: serde_json::Value =
            serde_json::from_slice(state).map_err(|source| not_fake(source.to_string()))?;
        serde_json::from_value(root["settings"].clone())
            .map_err(|source| not_fake(source.to_string()))
    }
}

/// A fault the fake renderer injects into one render.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// The host process stops.
    Crash,
    /// The host process stops answering. Simulated: the render fails at
    /// once with a time-out.
    Hang,
    /// The plug-in renders silence.
    Silence,
    /// The state does not load: the plug-in keeps and saves its init
    /// patch.
    KeepInit,
    /// The state loads, but the state the plug-in saves names another
    /// preset.
    Rename,
}

/// A seeded additive synth behind the [`Renderer`] trait.
#[derive(Clone, Debug)]
pub struct FakeRenderer {
    synth: Synth,
    seed: u64,
    renders: u64,
    faults: Vec<(u64, Fault)>,
    loaded: Option<Vec<u8>>,
    fixed: Option<FakePatch>,
}

impl FakeRenderer {
    /// Builds a renderer that claims to be `synth`. Two renderers with one
    /// seed give bit-identical renders in the same order.
    #[must_use]
    pub fn new(synth: Synth, seed: u64) -> Self {
        Self {
            synth,
            seed,
            renders: 0,
            faults: Vec::new(),
            loaded: None,
            fixed: None,
        }
    }

    /// Plays `patch` whatever state a job holds, as a plug-in that loads
    /// any preset of its synth. The saved state is still the loaded one.
    #[must_use]
    pub fn with_patch(mut self, patch: FakePatch) -> Self {
        self.fixed = Some(patch);
        self
    }

    /// Injects `fault` into the render with index `at`, counted from 0.
    #[must_use]
    pub fn with_fault(mut self, at: u64, fault: Fault) -> Self {
        self.faults.push((at, fault));
        self
    }

    /// Returns how many renders were asked for, failed ones included.
    #[must_use]
    pub fn renders(&self) -> u64 {
        self.renders
    }
}

impl Renderer for FakeRenderer {
    fn identity(&self) -> HostIdentity {
        HostIdentity {
            synth: self.synth,
            build: "fake".to_owned(),
            host: "fake".to_owned(),
            arch: "fake".to_owned(),
        }
    }

    fn render(&mut self, job: &RenderJob) -> Result<Rendered, RenderError> {
        self.play_job(job, true)
    }

    fn render_init(&mut self, job: &RenderJob) -> Result<Rendered, RenderError> {
        if self.loaded.is_some() {
            return Err(RenderError::Host {
                cause: "the fake already holds a loaded state".to_owned(),
                fix: "Render the init patch first".to_owned(),
            });
        }
        self.play_job(job, false)
    }

    fn save_state(&mut self) -> Result<Vec<u8>, RenderError> {
        match &self.loaded {
            Some(state) => Ok(state.clone()),
            None => Ok(init_state(self.synth)?.bytes().to_vec()),
        }
    }
}

fn init_state(synth: Synth) -> Result<PresetState, RenderError> {
    FakePatch::init()
        .state(synth)
        .map_err(|source| RenderError::State(source.to_string()))
}

impl FakeRenderer {
    fn play_job(&mut self, job: &RenderJob, load: bool) -> Result<Rendered, RenderError> {
        let index = self.renders;
        self.renders += 1;
        let fault = self
            .faults
            .iter()
            .find(|(at, _)| *at == index)
            .map(|(_, fault)| *fault);
        match fault {
            Some(Fault::Crash) => return Err(RenderError::Crashed("fake crash".to_owned())),
            Some(Fault::Hang) => {
                return Err(RenderError::TimedOut {
                    after: Seconds::from_const(30.0),
                    output: "fake hang".to_owned(),
                });
            }
            _ => {}
        }
        if job.state().synth() != self.synth {
            return Err(RenderError::WrongSynth {
                state: job.state().synth(),
                renderer: self.synth,
            });
        }
        let patch = if load && fault != Some(Fault::KeepInit) {
            let patch = match &self.fixed {
                Some(patch) => patch.clone(),
                None => FakePatch::of_state(job.state().bytes())?,
            };
            let mut saved = job.state().bytes().to_vec();
            if fault == Some(Fault::Rename) {
                let mut root: serde_json::Value = serde_json::from_slice(&saved)
                    .map_err(|source| RenderError::State(source.to_string()))?;
                root["preset_name"] = "renamed".into();
                saved = serde_json::to_vec(&root)
                    .map_err(|source| RenderError::State(source.to_string()))?;
            }
            self.loaded = Some(saved);
            patch
        } else {
            if load {
                self.loaded = Some(init_state(self.synth)?.bytes().to_vec());
            }
            FakePatch::init()
        };
        let mut rng = SplitMix::for_render(self.seed, index);
        let (mono, end) = play(&patch, job, &mut rng, fault == Some(Fault::Silence))?;
        let audio = Audio::new(job.recipe().sample_rate(), vec![mono.clone(), mono])?;
        Ok(Rendered {
            audio,
            record: RenderRecord::new(self.identity(), job, end),
        })
    }
}

/// One sounding note: its key, frequency, gain, phases and frames.
struct Voice {
    key: u8,
    hz: f64,
    gain: f64,
    phases: Vec<f64>,
    on: usize,
    off: Option<usize>,
}

/// Renders the mono clip and where it ends.
fn play(
    patch: &FakePatch,
    job: &RenderJob,
    rng: &mut SplitMix,
    silent: bool,
) -> Result<(Vec<f64>, super::ClipEnd), RenderError> {
    let recipe = job.recipe();
    let rate = f64::from(recipe.sample_rate().get());
    let mut voices: Vec<Voice> = Vec::new();
    let events = note_events(job.notes(), recipe.sample_rate());
    let mut next = 0;
    let mut watch = recipe.tail_watch(job.notes());
    let attack = patch.attack.get() * rate;
    let release = patch.release.get() * rate;
    let mut out = Vec::new();
    loop {
        let frame = out.len();
        while let Some(event) = events.get(next).filter(|e| e.frame <= frame) {
            match event.kind {
                EventKind::On => voices.push(Voice {
                    key: event.key,
                    hz: 440.0 * 2_f64.powf((f64::from(event.key) - 69.0) / 12.0),
                    gain: f64::from(event.velocity.get()) / 127.0
                        * (1.0 + LEVEL_JITTER * (2.0 * rng.unit() - 1.0)),
                    phases: patch.harmonics.iter().map(|_| TAU * rng.unit()).collect(),
                    on: frame,
                    off: None,
                }),
                EventKind::Off => {
                    if let Some(voice) = voices
                        .iter_mut()
                        .find(|v| v.off.is_none() && v.key == event.key)
                    {
                        voice.off = Some(frame);
                    }
                }
            }
            next += 1;
        }
        let t = cast::usize_to_f64(frame) / rate;
        let sample: f64 = if silent {
            0.0
        } else {
            voices
                .iter()
                .map(|voice| {
                    let age = cast::usize_to_f64(frame - voice.on);
                    let rise = if attack > 0.0 {
                        (age / attack).min(1.0)
                    } else {
                        1.0
                    };
                    let fall = voice.off.map_or(1.0, |off| {
                        let since = cast::usize_to_f64(frame - off);
                        10_f64.powf(-RELEASE_DB / 20.0 * since / release.max(1.0))
                    });
                    let tone: f64 = patch
                        .harmonics
                        .iter()
                        .zip(&voice.phases)
                        .enumerate()
                        .map(|(k, (amp, phase))| {
                            amp * (TAU * voice.hz * cast::usize_to_f64(k + 1) * t + phase).sin()
                        })
                        .sum();
                    voice.gain * rise * fall * tone
                })
                .sum()
        };
        out.push(sample);
        if let Some(end) = watch.push(&[sample])? {
            return Ok((out, end));
        }
    }
}

/// The SplitMix64 generator: small, seeded and the same on every platform.
struct SplitMix(u64);

impl SplitMix {
    /// Seeds the generator of one render from the pair (seed, render
    /// index): each part goes through the mixer, so nearby pairs give
    /// unrelated streams.
    fn for_render(seed: u64, index: u64) -> Self {
        let first = Self(seed).next();
        Self(first ^ Self(index).next().rotate_left(32))
    }

    /// Returns the next 64 random bits.
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Returns a uniform value in `[0, 1)`.
    fn unit(&mut self) -> f64 {
        cast::unit_interval(self.next())
    }
}
