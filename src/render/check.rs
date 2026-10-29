//! The load check: a render counts only when the plug-in kept the preset.
//!
//! A host call reports success even when the plug-in kept its old sound, so
//! the check looks at the result three ways:
//!
//! 1. The clip must sound: a clip whose every sample stays below the tail
//!    floor is silent.
//! 2. The state the plug-in saves after the render must hold the sent
//!    preset: the name, the tuning and every control. When it holds the
//!    init patch instead, the plug-in fell back to init.
//! 3. The clip is compared with the plug-in's own init patch. Each clip is
//!    reduced to its level in octave bands, and the distance is the
//!    largest band difference in dB. A clip within three times the
//!    distance of two init renders is init-like.
//!
//! The first two are faults. The third is a warning: mean band levels miss
//! a change of envelope or modulation, so a preset that loaded can still
//! lie within the margin. The margin stands in for a spread measured over
//! many renders, which a later release stores.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::state::{Mismatch, readback_mismatches, saved_preset};
use super::{PresetState, RenderError, RenderJob, Rendered, Renderer};
use crate::audio::{Audio, SampleRate};
use crate::cast;
use crate::dsp::{Padding, Stft, StftParams, Window};
use crate::preset::registry::registry_for;
use crate::synth::Synth;

/// The STFT frame of the band levels: 4096 samples, about 93 ms at
/// 44.1 kHz, so the lowest band holds several bins.
const FRAME: usize = 4096;
/// The lower edge of the lowest octave band, in Hz. The bands run up to the
/// Nyquist frequency.
const LOWEST_EDGE_HZ: f64 = 44.0;
/// A band quieter than this, in dB below full scale, is left out of the
/// distance, so noise in empty bands does not count.
const BAND_FLOOR_DB: f64 = -90.0;
/// A band more than this many dB below the loudest band of either clip is
/// left out too. Below a note's fundamental a band holds only window
/// leakage, whose level moves by several dB between two renders of one
/// patch.
const BAND_RANGE_DB: f64 = 30.0;
/// The distance between two init renders is scaled by this factor to give
/// the margin: a clip within the margin of the init patch is init-like.
/// Two renders give no distribution, so the factor is a stopgap.
const SPREAD_FACTOR: f64 = 3.0;
/// The smallest margin in dB, for a plug-in whose init renders match
/// almost exactly.
const MIN_MARGIN_DB: f64 = 0.1;
/// How many mismatches a message names.
const SHOWN: usize = 5;

/// The level of a clip in octave bands, in dB of mean power.
#[derive(Clone, Debug, PartialEq)]
pub struct BandLevels(Vec<f64>);

impl BandLevels {
    /// Measures the mono mix of `audio`.
    #[must_use]
    pub fn of(audio: &Audio) -> Self {
        let mono = audio.mono();
        let rate = audio.sample_rate();
        let params = StftParams::new(FRAME)
            .map(|params| params.with_window(Window::Hann).with_padding(Padding::Zero))
            .ok();
        let Some(params) = params else {
            return Self(Vec::new());
        };
        let mut stft = Stft::new(params);
        let bins = params.bins();
        let mut power = vec![0.0; bins];
        let frames = params.frame_count(mono.len()).max(1);
        for frame in 0..frames {
            for (sum, value) in power.iter_mut().zip(stft.frame(&mono, frame)) {
                *sum += value.norm_sqr();
            }
        }
        let scale = 1.0 / cast::usize_to_f64(frames * FRAME * FRAME);
        let bin_hz = params.bin_width(rate).get();
        Self(
            band_edges(rate)
                .windows(2)
                .map(|edge| {
                    let in_band = power.iter().enumerate().filter(|(bin, _)| {
                        let hz = cast::usize_to_f64(*bin) * bin_hz;
                        (edge[0]..edge[1]).contains(&hz)
                    });
                    let (sum, count) =
                        in_band.fold((0.0, 0_usize), |(s, n), (_, p)| (s + p, n + 1));
                    let mean = if count == 0 {
                        0.0
                    } else {
                        sum * scale / cast::usize_to_f64(count)
                    };
                    10.0 * mean.max(1e-30).log10()
                })
                .collect(),
        )
    }

    /// The largest level difference in a band where either clip is above
    /// the band floor and within 30 dB of the loudest band.
    /// Two clips with no such band are 0 dB apart.
    #[must_use]
    pub fn distance(&self, other: &Self) -> f64 {
        let loudest = self
            .0
            .iter()
            .chain(&other.0)
            .fold(f64::NEG_INFINITY, |a, b| a.max(*b));
        let floor = BAND_FLOOR_DB.max(loudest - BAND_RANGE_DB);
        self.0
            .iter()
            .zip(&other.0)
            .filter(|(a, b)| a.max(**b) > floor)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max)
    }
}

/// The octave band edges from [`LOWEST_EDGE_HZ`] up to the Nyquist
/// frequency.
fn band_edges(rate: SampleRate) -> Vec<f64> {
    let nyquist = rate.nyquist().get();
    std::iter::successors(Some(LOWEST_EDGE_HZ), |edge| Some(edge * 2.0))
        .take_while(|edge| *edge < nyquist)
        .chain(std::iter::once(nyquist))
        .collect()
}

/// What the load check measured for a render that passed.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct LoadCheck {
    /// The distance of the clip from the init patch, in dB.
    pub init_distance_db: f64,
    /// The distance between two init renders, in dB.
    pub init_spread_db: f64,
    /// The margin the distance had to exceed, in dB.
    pub margin_db: f64,
    /// Whether the clip lies within the margin of the init patch. The
    /// render still counts, because the read-back matched.
    pub init_like: bool,
}

impl LoadCheck {
    /// The warning of an init-like render, if this one is.
    #[must_use]
    pub fn warning(&self) -> Option<String> {
        self.init_like.then(|| {
            format!(
                "the render lies {:.2} dB from the init patch, within the margin of {:.2} dB. The saved state matches, so the preset loaded; octave-band levels miss changes of envelope and modulation. Listen to the clip to check that it differs from the init patch",
                self.init_distance_db, self.margin_db
            )
        })
    }
}

/// Why a render does not count.
#[derive(Debug)]
pub enum LoadFault {
    /// The plug-in or its host failed.
    Render(RenderError),
    /// The clip is silent.
    Silent {
        /// What in the preset can explain the silence. Empty when nothing
        /// known does.
        reasons: Vec<String>,
    },
    /// The plug-in fell back to its init patch: the state it saved is the
    /// init state.
    InitFallback,
    /// The state the plug-in saved differs from the sent one.
    Readback(Vec<Mismatch>),
}

impl fmt::Display for LoadFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Render(error) => write!(f, "{error}"),
            Self::Silent { reasons } if reasons.is_empty() => write!(
                f,
                "the render is silent: every sample stays below -96 dBFS, and nothing known in the preset explains it. Open the preset in the synth and check that it sounds"
            ),
            Self::Silent { reasons } => write!(
                f,
                "the render is silent: {}. Fix the preset so that it sounds",
                reasons.join("; ")
            ),
            Self::InitFallback => write!(
                f,
                "the plug-in did not load the preset: after the render it saves its init patch. Check that the preset loads in the synth and that its build is not newer than the plug-in"
            ),
            Self::Readback(mismatches) => {
                let shown: Vec<String> = mismatches
                    .iter()
                    .take(SHOWN)
                    .map(ToString::to_string)
                    .collect();
                let more = mismatches.len().saturating_sub(SHOWN);
                write!(
                    f,
                    "the plug-in did not keep the preset: {}{}. Check that the preset loads in the synth with these values",
                    shown.join("; "),
                    if more > 0 {
                        format!(" and {more} more")
                    } else {
                        String::new()
                    }
                )
            }
        }
    }
}

impl std::error::Error for LoadFault {}

impl From<RenderError> for LoadFault {
    fn from(error: RenderError) -> Self {
        Self::Render(error)
    }
}

/// Renders `job` and checks that the plug-in loaded its preset.
///
/// The renderer must not have loaded a state yet: the check renders the
/// init patch twice first, and keeps only the band levels of each clip.
///
/// # Errors
///
/// Returns the [`LoadFault`] that explains why the render does not count.
pub fn render_checked(
    renderer: &mut dyn Renderer,
    job: &RenderJob,
) -> Result<(Rendered, LoadCheck), LoadFault> {
    let init = BandLevels::of(&renderer.render_init(job)?.audio);
    let spread = init.distance(&BandLevels::of(&renderer.render_init(job)?.audio));
    let init_state = renderer.save_state()?;
    let output = renderer.render(job)?;
    let saved = renderer.save_state()?;
    let floor = job.recipe().tail_floor_gain();
    let silent = output
        .audio
        .channels()
        .iter()
        .all(|channel| channel.iter().all(|sample| sample.abs() < floor));
    if silent {
        return Err(LoadFault::Silent {
            reasons: silence_reasons(job.state()),
        });
    }
    let mismatches = readback_mismatches(job.state(), &saved, job.bpm())?;
    if !mismatches.is_empty() {
        let fell_back = matches!(
            (saved_preset(&saved), saved_preset(&init_state)),
            (Some(saved), Some(init)) if saved == init
        );
        return Err(if fell_back {
            LoadFault::InitFallback
        } else {
            LoadFault::Readback(mismatches)
        });
    }
    let distance = BandLevels::of(&output.audio).distance(&init);
    let margin = (SPREAD_FACTOR * spread).max(MIN_MARGIN_DB);
    Ok((
        output,
        LoadCheck {
            init_distance_db: distance,
            init_spread_db: spread,
            margin_db: margin,
            init_like: distance <= margin,
        },
    ))
}

/// What in a preset can make it render silent.
#[must_use]
pub fn silence_reasons(state: &PresetState) -> Vec<String> {
    match state.synth() {
        Synth::Vital => saved_preset(state.bytes())
            .and_then(|root| root.get("settings").and_then(Value::as_object).cloned())
            .map(|settings| vital_silence_reasons(&settings))
            .unwrap_or_default(),
        Synth::Serum2 => Vec::new(),
    }
}

fn vital_silence_reasons(settings: &Map<String, Value>) -> Vec<String> {
    let registry = registry_for(Synth::Vital).ok();
    let value = |key: &str| {
        settings
            .get(key)
            .and_then(Value::as_f64)
            .or_else(|| registry.and_then(|r| r.get(key)).map(|spec| spec.default))
    };
    let mut reasons = Vec::new();
    let sources = ["osc_1_on", "osc_2_on", "osc_3_on", "sample_on"];
    if sources.iter().all(|key| value(key) == Some(0.0)) {
        reasons.push("no oscillator and no sample is on".to_owned());
    }
    if value("volume") == Some(0.0) {
        reasons.push("the master volume is at its minimum".to_owned());
    }
    reasons
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f64, gain: f64) -> Audio {
        let rate = SampleRate::new(44_100).unwrap();
        let samples: Vec<f64> = (0..44_100)
            .map(|n| gain * (std::f64::consts::TAU * hz * cast::usize_to_f64(n) / 44_100.0).sin())
            .collect();
        Audio::new(rate, vec![samples.clone(), samples]).unwrap()
    }

    #[test]
    fn the_bands_run_in_octaves_up_to_nyquist() {
        let edges = band_edges(SampleRate::new(44_100).unwrap());
        assert_eq!(edges.first(), Some(&44.0));
        assert_eq!(edges.last(), Some(&22_050.0));
        assert_eq!(edges.len(), 10);
    }

    #[test]
    fn a_level_change_shows_in_its_band() {
        let a = BandLevels::of(&tone(440.0, 0.5));
        let b = BandLevels::of(&tone(440.0, 0.25));
        assert!((a.distance(&b) - 6.02).abs() < 0.05, "{}", a.distance(&b));
        assert!(a.distance(&a) < 1e-9);
        let c = BandLevels::of(&tone(3_000.0, 0.5));
        assert!(a.distance(&c) > 40.0, "{}", a.distance(&c));
    }

    #[test]
    fn leakage_far_below_the_loudest_band_does_not_count() {
        let a = BandLevels(vec![-80.0, -40.0, -55.0]);
        let b = BandLevels(vec![-72.0, -40.0, -56.0]);
        assert!((a.distance(&b) - 1.0).abs() < 1e-9, "{}", a.distance(&b));
    }

    #[test]
    fn silence_is_explained_by_the_controls() {
        let state = |settings: Value| {
            let root = serde_json::json!({ "synth_version": "1.0.6", "settings": settings });
            PresetState::new(Synth::Vital, serde_json::to_vec(&root).unwrap())
        };
        let off = state(serde_json::json!({ "osc_1_on": 0.0, "volume": 0.0 }));
        assert_eq!(
            silence_reasons(&off),
            [
                "no oscillator and no sample is on",
                "the master volume is at its minimum"
            ]
        );
        // Oscillator 1 is on by default.
        assert_eq!(
            silence_reasons(&state(serde_json::json!({}))),
            Vec::<String>::new()
        );
    }
}
