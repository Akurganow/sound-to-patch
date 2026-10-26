//! The Vital adapter: the `.vital` JSON format, the parameter registry, the
//! built-in template, the capabilities and the laws that span controls.
//!
//! Facts come from the Vital 1.0.6 open source (GPL-3.0), restated in our
//! own words and layout; no code or comment of it is copied. The retail app
//! is newer than that source, so a fact carries the build it was read from.

use std::fmt;
use std::str::FromStr;

use crate::preset::registry::{Provenance, Registry};
use crate::synth::{
    EffectFamily, GlideRule, GlideScaling, LfoSync, OscillatorLayout, SynthCapabilities,
    SynthError, VelocityRule,
};
use crate::units::{Percent, Seconds, Semitones};

mod codec;
pub mod laws;
mod registry;
mod template;

pub use codec::{VitalFormat, VitalModel};
pub use registry::EngineScale;
pub use template::VitalTemplate;

/// The `synth_version` stamp of files built from the built-in template:
/// the version of the open source. A load check on the retail app settles
/// the final stamp.
pub const DEFAULT_STAMP: &str = "1.0.6";

/// Keys that retail builds from 1.5 on write and the open source cannot
/// read. A rewrite keeps them.
pub const RETAIL_ONLY_KEYS: [&str; 5] = [
    "custom_warps",
    "random_values",
    "osc_1_spectral_morph_phase",
    "osc_2_spectral_morph_phase",
    "osc_3_spectral_morph_phase",
];

/// The largest file the reader accepts until the fuzzing task measures
/// real files: 64 MiB. A file of that size made of short numbers can take
/// about 2.4 GB to parse; see [`VitalFormat`].
pub const MAX_PRESET_BYTES: usize = 64 << 20;

/// The Vital format with the default size cap.
pub static FORMAT: VitalFormat = VitalFormat::new(MAX_PRESET_BYTES);

/// The structure of Vital, from the 1.0.6 source. The portamento span is
/// 2^-10 s to 2^4 s; the bend range and the velocity amount are the
/// registry defaults of `pitch_bend_range` and `velocity_track`.
pub static CAPABILITIES: SynthCapabilities = SynthCapabilities {
    oscillators: OscillatorLayout {
        wavetable: 3,
        sample: true,
        sub: false,
        noise: false,
    },
    voice_filters: 2,
    fx_filter: true,
    envelopes: 6,
    lfos: 8,
    random_lfos: 4,
    macros: 4,
    mod_slots: 64,
    unison_voices: (1, 16),
    polyphony_max: 32,
    effects: &[
        EffectFamily::Chorus,
        EffectFamily::Compressor,
        EffectFamily::Delay,
        EffectFamily::Distortion,
        EffectFamily::Eq,
        EffectFamily::Filter,
        EffectFamily::Flanger,
        EffectFamily::Phaser,
        EffectFamily::Reverb,
    ],
    effect_order_free: true,
    effect_repeats: false,
    lfo_sync: &[
        LfoSync::Free,
        LfoSync::Tempo,
        LfoSync::Dotted,
        LfoSync::Triplets,
        LfoSync::KeyTrack,
    ],
    glide: GlideRule {
        min: Seconds::from_const(0.000_976_562_5),
        max: Seconds::from_const(16.0),
        scaling: GlideScaling::Unknown,
    },
    velocity: VelocityRule {
        default_track: Percent::from_const(0.0),
        amp_path_power: 2,
        provenance: Provenance::Interim,
    },
    bend_range_default: Semitones::from_const(2.0),
};

/// The registry of Vital.
pub(crate) fn registry() -> Result<&'static Registry, SynthError> {
    registry::vital_registry().map(|vital| &vital.registry)
}

/// The engine scale of a Vital key, which modulation adds before.
///
/// # Errors
///
/// [`SynthError::BrokenData`] if the built-in registry does not load.
pub fn engine_scale(key: &str) -> Result<Option<EngineScale>, SynthError> {
    registry::vital_registry().map(|vital| vital.scales.get(key).copied())
}

/// A Vital build stamp, `major.minor.patch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VitalVersion {
    /// Major number.
    pub major: u32,
    /// Minor number.
    pub minor: u32,
    /// Patch number.
    pub patch: u32,
}

impl VitalVersion {
    /// Whether an app of build `app` loads a file with this stamp: the
    /// app refuses a file whose major and minor are newer than its own,
    /// and ignores the patch.
    #[must_use]
    pub fn accepted_by(&self, app: &Self) -> bool {
        (self.major, self.minor) <= (app.major, app.minor)
    }
}

impl FromStr for VitalVersion {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        let parts: Vec<&str> = text.split('.').collect();
        let number = |part: &str| {
            if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            part.parse::<u32>().ok()
        };
        match parts.as_slice() {
            [major, minor, patch] => match (number(major), number(minor), number(patch)) {
                (Some(major), Some(minor), Some(patch)) => Ok(Self {
                    major,
                    minor,
                    patch,
                }),
                _ => Err(format!("{text:?} is not three whole numbers such as 1.5.5")),
            },
            _ => Err(format!("{text:?} is not three whole numbers such as 1.5.5")),
        }
    }
}

impl fmt::Display for VitalVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::registry::{ParamRegistry, Physical};

    fn version(text: &str) -> VitalVersion {
        text.parse().unwrap()
    }

    #[test]
    fn the_version_gate_ignores_the_patch() {
        assert!(version("1.5.5").accepted_by(&version("1.5.1")));
        assert!(!version("1.6.0").accepted_by(&version("1.5.5")));
        assert!(!version("1.5.5").accepted_by(&version("1.0.6")));
        assert!(version("1.0.9").accepted_by(&version("1.0.6")));
        assert!(version("1.0.6").accepted_by(&version("1.5.5")));
    }

    #[test]
    fn version_text_needs_three_numbers() {
        for text in ["1.5", "a.b.c", "1.5.5.1", "", "1..5", "1.5.-1", "1.5.+1"] {
            assert!(text.parse::<VitalVersion>().is_err(), "{text}");
        }
        assert_eq!(version("1.5.5").to_string(), "1.5.5");
        assert!(version("1.10.0") > version("1.9.9"));
    }

    #[test]
    fn capabilities_hold_and_match_the_registry() {
        CAPABILITIES.check().unwrap();
        let registry = registry().unwrap();
        let default = |key: &str| {
            registry
                .get(key)
                .unwrap()
                .physical(registry.get(key).unwrap().default)
                .unwrap()
        };
        assert_eq!(
            default("pitch_bend_range"),
            Physical::Semitones(CAPABILITIES.bend_range_default)
        );
        assert_eq!(
            default("velocity_track"),
            Physical::PercentBipolar(CAPABILITIES.velocity.default_track)
        );
        let glide = registry.get("portamento_time").unwrap();
        assert_eq!(
            glide.physical(glide.stored_min).unwrap(),
            Physical::Seconds(CAPABILITIES.glide.min)
        );
        assert_eq!(
            glide.physical(glide.stored_max).unwrap(),
            Physical::Seconds(CAPABILITIES.glide.max)
        );
        assert_eq!(
            registry.get("polyphony").unwrap().stored_max,
            f64::from(CAPABILITIES.polyphony_max)
        );
        let unison = registry.get("osc_1_unison_voices").unwrap();
        assert_eq!(
            (unison.stored_min, unison.stored_max),
            (
                f64::from(CAPABILITIES.unison_voices.0),
                f64::from(CAPABILITIES.unison_voices.1)
            )
        );
    }

    #[test]
    fn const_unit_values_pass_their_checks() {
        assert_eq!(
            Seconds::new(0.000_976_562_5).unwrap(),
            CAPABILITIES.glide.min
        );
        assert_eq!(Seconds::new(16.0).unwrap(), CAPABILITIES.glide.max);
        assert_eq!(
            Percent::new(0.0).unwrap(),
            CAPABILITIES.velocity.default_track
        );
        assert_eq!(
            Semitones::new(2.0).unwrap(),
            CAPABILITIES.bend_range_default
        );
    }

    #[test]
    fn engine_scale_names_the_transform() {
        assert_eq!(
            engine_scale("env_1_attack").unwrap(),
            Some(EngineScale::Quartic)
        );
        assert_eq!(engine_scale("nope").unwrap(), None);
    }
}
