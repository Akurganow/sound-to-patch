//! The target synth: the [`Synth`] enum, the shared `--synth` argument and
//! the structure each synth offers, [`SynthCapabilities`].
//!
//! Two synths are equal targets, Vital and Serum 2. Only the synths in
//! [`Synth::READY`] have an adapter. A command resolves its target with
//! [`SynthArg::resolve`]: the flag, the extension of a preset path on the
//! command line, or Vital when neither names one.
//!
//! ```
//! use std::path::Path;
//! use sound_to_patch::synth::{Synth, SynthArg};
//!
//! let arg = SynthArg::default();
//! assert_eq!(arg.resolve(None)?, Synth::Vital);
//! assert_eq!(arg.resolve(Some(Path::new("bass.vital")))?, Synth::Vital);
//! assert_eq!("serum2".parse::<Synth>()?, Synth::Serum2);
//! # Ok::<(), sound_to_patch::synth::SynthError>(())
//! ```

use std::error::Error;
use std::fmt;
use std::path::Path;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::units::{Bipolar, Percent, Seconds, Semitones};

/// A target synth.
///
/// The text form is `vital` or `serum2`. `Display`, `FromStr`, serde and
/// the command line all use it, and parsing is exact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Synth {
    /// Vital, with `.vital` preset files.
    Vital,
    /// Serum 2, with `.SerumPreset` preset files.
    Serum2,
}

impl Synth {
    /// Every target synth, whether or not its adapter exists.
    pub const ALL: [Self; 2] = [Self::Vital, Self::Serum2];

    /// The synths that have an adapter. Every contract test loops over
    /// this list; a new adapter joins the tests by joining it.
    pub const READY: [Self; 1] = [Self::Vital];

    /// The text form, `vital` or `serum2`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Vital => "vital",
            Self::Serum2 => "serum2",
        }
    }

    /// The name for people, `Vital` or `Serum 2`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Vital => "Vital",
            Self::Serum2 => "Serum 2",
        }
    }

    /// The preset file extension without the dot.
    #[must_use]
    pub fn extension(self) -> &'static str {
        match self {
            Self::Vital => "vital",
            Self::Serum2 => "SerumPreset",
        }
    }

    /// Returns the synth if it has an adapter.
    ///
    /// # Errors
    ///
    /// [`SynthError::NotBuilt`] for a synth outside [`Synth::READY`].
    pub fn ready(self) -> Result<Self, SynthError> {
        if Self::READY.contains(&self) {
            Ok(self)
        } else {
            Err(SynthError::NotBuilt(self))
        }
    }

    /// The structure of the synth: oscillators, filters, modulation
    /// sources, effects and voicing rules.
    ///
    /// # Errors
    ///
    /// [`SynthError::NotBuilt`] for a synth outside [`Synth::READY`],
    /// because its values are not settled yet.
    pub fn capabilities(self) -> Result<&'static SynthCapabilities, SynthError> {
        match self.ready()? {
            Self::Vital => Ok(&crate::preset::vital::CAPABILITIES),
            Self::Serum2 => Err(SynthError::NotBuilt(self)),
        }
    }

    /// The synth whose preset extension `ext` is, compared without regard
    /// to case.
    fn from_extension(ext: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|synth| synth.extension().eq_ignore_ascii_case(ext))
    }
}

impl fmt::Display for Synth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.as_str())
    }
}

impl FromStr for Synth {
    type Err = SynthError;

    fn from_str(text: &str) -> Result<Self, SynthError> {
        Self::ALL
            .into_iter()
            .find(|synth| synth.as_str() == text)
            .ok_or_else(|| SynthError::Unknown(text.to_owned()))
    }
}

/// Why a target synth cannot be used.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SynthError {
    /// The text names no synth.
    Unknown(String),
    /// The synth has no adapter yet.
    NotBuilt(Synth),
    /// `--synth` and the extension of a preset path name different synths.
    Mismatch {
        /// The synth that `--synth` names.
        flag: Synth,
        /// The preset path.
        path: String,
        /// The synth its extension names.
        extension: Synth,
    },
    /// The built-in data of an adapter does not load. This is a bug.
    BrokenData {
        /// The synth whose data failed.
        synth: Synth,
        /// What failed.
        cause: String,
    },
}

impl fmt::Display for SynthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(text) => write!(
                f,
                "unknown synth {text:?}. Write one of: {}",
                Synth::ALL.map(Synth::as_str).join(", ")
            ),
            Self::NotBuilt(synth) => write!(
                f,
                "{} support is not built yet. Use --synth {}.",
                synth.name(),
                Synth::READY[0]
            ),
            Self::Mismatch {
                flag,
                path,
                extension,
            } => write!(
                f,
                "--synth {flag} does not match {path:?}, a {} preset. Pass --synth {extension} or a .{} file",
                extension.name(),
                flag.extension()
            ),
            Self::BrokenData { synth, cause } => write!(
                f,
                "the built-in {} data does not load: {cause}. This is a bug; report it with the command you ran",
                synth.name()
            ),
        }
    }
}

impl Error for SynthError {}

/// The shared `--synth` option. A command flattens it when it needs a
/// target synth.
#[derive(Clone, Debug, Default, PartialEq, Eq, clap::Args)]
pub struct SynthArg {
    /// The target synth. Without it, the extension of the preset file picks
    /// the synth, and otherwise Vital.
    #[arg(long, value_enum, value_name = "SYNTH")]
    pub synth: Option<Synth>,
}

impl SynthArg {
    /// Resolves the target synth from the flag and an optional preset path
    /// on the command line.
    ///
    /// 1. The flag and no hint: the flag.
    /// 2. The flag and a hint with a known extension: both must agree.
    /// 3. A hint with a known extension and no flag: the extension.
    /// 4. Neither: Vital.
    ///
    /// The result must also be in [`Synth::READY`].
    ///
    /// # Errors
    ///
    /// [`SynthError::Mismatch`] when the flag and the extension disagree,
    /// and [`SynthError::NotBuilt`] for a synth without an adapter.
    pub fn resolve(&self, hint: Option<&Path>) -> Result<Synth, SynthError> {
        let from_hint = hint.and_then(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .and_then(Synth::from_extension)
                .map(|synth| (path, synth))
        });
        let synth = match (self.synth, from_hint) {
            (Some(flag), Some((path, extension))) if flag != extension => {
                return Err(SynthError::Mismatch {
                    flag,
                    path: path.display().to_string(),
                    extension,
                });
            }
            (Some(flag), _) => flag,
            (None, Some((_, extension))) => extension,
            (None, None) => Synth::Vital,
        };
        synth.ready()
    }
}

/// What a synth offers in structure: the sources, filters, modulation,
/// effects and voicing rules that a mapper chooses from.
///
/// Parameter laws and ranges live in the registry, not here. No estimator
/// reads these values; only mappers do.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct SynthCapabilities {
    /// The oscillator sources.
    pub oscillators: OscillatorLayout,
    /// Filters in the voice path.
    pub voice_filters: u8,
    /// Whether a filter sits in the effect chain.
    pub fx_filter: bool,
    /// Envelopes.
    pub envelopes: u8,
    /// Shaped LFOs.
    pub lfos: u8,
    /// Random LFOs.
    pub random_lfos: u8,
    /// Macro controls.
    pub macros: u8,
    /// Slots of the modulation matrix.
    pub mod_slots: u16,
    /// The smallest and the largest unison voice count.
    pub unison_voices: (u8, u8),
    /// The largest polyphony.
    pub polyphony_max: u8,
    /// The effects, by neutral family name.
    pub effects: &'static [EffectFamily],
    /// Whether the order of the effects can change.
    pub effect_order_free: bool,
    /// Whether one effect family can appear twice.
    pub effect_repeats: bool,
    /// The ways an LFO rate can be set.
    pub lfo_sync: &'static [LfoSync],
    /// The portamento time span and how the time scales with the interval.
    pub glide: GlideRule,
    /// How velocity reaches the level.
    pub velocity: VelocityRule,
    /// The pitch-bend range of a new patch.
    pub bend_range_default: Semitones,
}

impl SynthCapabilities {
    /// Checks the rules every synth must meet.
    ///
    /// # Errors
    ///
    /// Returns the first broken rule as text.
    pub fn check(&self) -> Result<(), String> {
        if self.voice_filters < 1 {
            return Err("voice_filters must be at least 1".into());
        }
        if self.mod_slots < 1 {
            return Err("mod_slots must be at least 1".into());
        }
        if self.unison_voices.0 > self.unison_voices.1 {
            return Err(format!(
                "unison_voices minimum {} exceeds maximum {}",
                self.unison_voices.0, self.unison_voices.1
            ));
        }
        if self.glide.min >= self.glide.max {
            return Err(format!(
                "glide minimum {} is not below maximum {}",
                self.glide.min, self.glide.max
            ));
        }
        if self.effects.is_empty() {
            return Err("effects must not be empty".into());
        }
        if !self.effect_repeats {
            for (index, family) in self.effects.iter().enumerate() {
                if self.effects[..index].contains(family) {
                    return Err(format!(
                        "effect {family:?} appears twice, but effect_repeats is false"
                    ));
                }
            }
        }
        Ok(())
    }
}

/// The oscillator sources of a synth.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OscillatorLayout {
    /// Wavetable oscillators.
    pub wavetable: u8,
    /// Whether a sample oscillator exists.
    pub sample: bool,
    /// Whether a separate sub oscillator exists.
    pub sub: bool,
    /// Whether a separate noise oscillator exists.
    pub noise: bool,
}

/// An effect family by its neutral name. A report never uses a synth's own
/// menu text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum EffectFamily {
    /// Chorus.
    Chorus,
    /// Dynamic range compressor.
    Compressor,
    /// Delay.
    Delay,
    /// Distortion: clipping, folding, bit and rate reduction.
    Distortion,
    /// Equalizer.
    Eq,
    /// A filter in the effect chain.
    Filter,
    /// Flanger.
    Flanger,
    /// Phaser.
    Phaser,
    /// Reverb.
    Reverb,
}

/// A way to set an LFO rate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LfoSync {
    /// A free rate in hertz.
    Free,
    /// A note division of the tempo.
    Tempo,
    /// A dotted note division.
    Dotted,
    /// A triplet note division.
    Triplets,
    /// A rate that follows the played note.
    KeyTrack,
}

/// The portamento time span.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlideRule {
    /// The shortest glide time.
    pub min: Seconds,
    /// The longest glide time.
    pub max: Seconds,
    /// Whether the time is per note or per octave of interval.
    pub scaling: GlideScaling,
}

/// How the glide time relates to the interval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlideScaling {
    /// The time is the same for any interval.
    PerNote,
    /// The time is per octave of interval.
    PerOctave,
    /// Not settled by facts or a probe.
    Unknown,
}

/// How velocity reaches the audio level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VelocityRule {
    /// The velocity-to-level amount of a new patch.
    pub default_track: Percent<Bipolar>,
    /// The power applied to envelope times velocity on the way to the
    /// audio gain.
    pub amp_path_power: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_form_round_trips_for_every_synth() {
        for synth in Synth::ALL {
            assert_eq!(synth.to_string().parse::<Synth>(), Ok(synth));
            let json = serde_json::to_string(&synth).unwrap();
            assert_eq!(json, format!("\"{synth}\""));
            assert_eq!(serde_json::from_str::<Synth>(&json).unwrap(), synth);
        }
    }

    #[test]
    fn parsing_is_exact_and_lists_both_values() {
        for text in ["Vital", "serum 2", "VITAL", " vital", ""] {
            let message = text.parse::<Synth>().unwrap_err().to_string();
            assert!(message.contains("vital, serum2"), "{message}");
        }
    }

    #[test]
    fn not_built_message_names_the_fix() {
        assert_eq!(
            Synth::Serum2.ready().unwrap_err().to_string(),
            "Serum 2 support is not built yet. Use --synth vital."
        );
    }

    #[test]
    fn resolve_follows_the_four_rules() {
        let none = SynthArg::default();
        let vital = SynthArg {
            synth: Some(Synth::Vital),
        };
        let serum = SynthArg {
            synth: Some(Synth::Serum2),
        };
        // 1. Flag, no hint.
        assert_eq!(vital.resolve(None), Ok(Synth::Vital));
        // 2. Flag and a known extension must agree.
        assert_eq!(vital.resolve(Some(Path::new("a.VITAL"))), Ok(Synth::Vital));
        let mismatch = serum
            .resolve(Some(Path::new("a.vital")))
            .unwrap_err()
            .to_string();
        assert!(
            mismatch.contains("serum2") && mismatch.contains("vital"),
            "{mismatch}"
        );
        // 3. The extension picks the synth.
        assert_eq!(
            none.resolve(Some(Path::new("lead.SerumPreset"))),
            Err(SynthError::NotBuilt(Synth::Serum2))
        );
        assert_eq!(
            none.resolve(Some(Path::new("lead.serumpreset"))),
            Err(SynthError::NotBuilt(Synth::Serum2))
        );
        // 4. Neither: Vital, also with an unknown extension.
        assert_eq!(none.resolve(None), Ok(Synth::Vital));
        assert_eq!(none.resolve(Some(Path::new("notes.txt"))), Ok(Synth::Vital));
        assert_eq!(
            serum.resolve(None),
            Err(SynthError::NotBuilt(Synth::Serum2))
        );
    }

    #[test]
    fn capabilities_exist_only_for_ready_synths() {
        for synth in Synth::READY {
            synth.capabilities().unwrap().check().unwrap();
        }
        assert_eq!(
            Synth::Serum2.capabilities(),
            Err(SynthError::NotBuilt(Synth::Serum2))
        );
    }
}
