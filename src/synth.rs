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

use std::path::Path;

#[doc(inline)]
pub use sound_to_patch_core::synth::{Synth, SynthError};

use crate::music::pitch::OctaveLabel;
use crate::preset::registry::Provenance;
use crate::units::{Bipolar, Percent, Seconds, Semitones};

/// The structure of `synth`: oscillators, filters, modulation sources,
/// effects and voicing rules.
///
/// # Errors
///
/// [`SynthError::NotBuilt`] for a synth outside [`Synth::READY`],
/// because its values are not settled yet.
pub fn capabilities(synth: Synth) -> Result<&'static SynthCapabilities, SynthError> {
    match synth.ready()? {
        Synth::Vital => Ok(&crate::preset::vital::CAPABILITIES),
        Synth::Serum2 => Err(SynthError::NotBuilt(synth)),
    }
}

/// The shared `--synth` option. A command flattens it when it needs a
/// target synth.
#[derive(Clone, Debug, Default, PartialEq, Eq, clap::Args)]
pub struct SynthArg {
    /// The target synth [default: the extension of the preset file, else
    /// vital]
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
    /// [`SynthError::FileNotReady`] when the extension names a synth
    /// without an adapter, and [`SynthError::NotBuilt`] for a synth without
    /// an adapter.
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
            // Suggesting another --synth would only end in a mismatch.
            (_, Some((path, extension))) if !Synth::READY.contains(&extension) => {
                return Err(SynthError::FileNotReady {
                    path: path.display().to_string(),
                    synth: extension,
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
    /// The octave label the synth's own note display gives MIDI note 60;
    /// none until its source or a probe settles it.
    pub middle_c: Option<OctaveLabel>,
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
    /// Where the rule comes from. A reading of the source stays interim
    /// until a render confirms it.
    pub provenance: Provenance,
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // 3. The extension picks the synth. A file of a synth without an
        // adapter says that only Vital files work, with or without a flag.
        for (arg, name) in [
            (&none, "lead.SerumPreset"),
            (&none, "lead.serumpreset"),
            (&serum, "lead.SerumPreset"),
        ] {
            let error = arg.resolve(Some(Path::new(name))).unwrap_err();
            assert_eq!(
                error,
                SynthError::FileNotReady {
                    path: name.to_owned(),
                    synth: Synth::Serum2
                }
            );
            let text = error.to_string();
            assert!(text.contains("Serum 2 files cannot be read yet"), "{text}");
            assert!(
                text.contains("Only Vital files (.vital) work now"),
                "{text}"
            );
            assert!(!text.contains("--synth"), "{text}");
        }
        // 4. Neither: Vital, also with an unknown extension.
        assert_eq!(none.resolve(None), Ok(Synth::Vital));
        assert_eq!(none.resolve(Some(Path::new("notes.txt"))), Ok(Synth::Vital));
        assert_eq!(
            serum.resolve(None),
            Err(SynthError::NotBuilt(Synth::Serum2))
        );
    }

    /// Breaks one rule of a capabilities value.
    type Break = fn(&mut SynthCapabilities);

    /// A test-only value that meets every rule.
    fn sample() -> SynthCapabilities {
        SynthCapabilities {
            oscillators: OscillatorLayout {
                wavetable: 2,
                sample: false,
                sub: true,
                noise: true,
            },
            voice_filters: 1,
            fx_filter: false,
            envelopes: 4,
            lfos: 4,
            random_lfos: 0,
            macros: 8,
            mod_slots: 16,
            unison_voices: (1, 8),
            polyphony_max: 16,
            effects: &[EffectFamily::Delay, EffectFamily::Reverb],
            effect_order_free: true,
            effect_repeats: false,
            lfo_sync: &[LfoSync::Free, LfoSync::Tempo],
            glide: GlideRule {
                min: Seconds::new(0.0).unwrap(),
                max: Seconds::new(10.0).unwrap(),
                scaling: GlideScaling::PerNote,
            },
            velocity: VelocityRule {
                default_track: Percent::new(100.0).unwrap(),
                amp_path_power: 1,
                provenance: Provenance::Interim,
            },
            bend_range_default: Semitones::new(2.0).unwrap(),
            middle_c: Some(OctaveLabel::C3),
        }
    }

    #[test]
    fn each_broken_capability_rule_is_rejected() {
        sample().check().unwrap();
        let broken: [(&str, Break); 6] = [
            ("voice_filters", |c| c.voice_filters = 0),
            ("mod_slots", |c| c.mod_slots = 0),
            ("unison_voices", |c| c.unison_voices = (9, 8)),
            ("glide", |c| c.glide.max = c.glide.min),
            ("effects must not be empty", |c| c.effects = &[]),
            ("appears twice", |c| {
                c.effects = &[EffectFamily::Delay, EffectFamily::Delay];
            }),
        ];
        for (needle, breaks) in broken {
            let mut value = sample();
            breaks(&mut value);
            let message = value.check().unwrap_err();
            assert!(message.contains(needle), "{needle}: {message}");
        }
        let mut repeats = sample();
        repeats.effects = &[EffectFamily::Delay, EffectFamily::Delay];
        repeats.effect_repeats = true;
        repeats.check().unwrap();
    }

    #[test]
    fn capabilities_exist_only_for_ready_synths() {
        for synth in Synth::READY {
            capabilities(synth).unwrap().check().unwrap();
        }
        assert_eq!(
            capabilities(Synth::Serum2),
            Err(SynthError::NotBuilt(Synth::Serum2))
        );
    }
}
