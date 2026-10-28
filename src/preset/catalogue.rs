//! Where each control of a synth sits in the agent reference: its layer
//! and the pattern that groups its instances.
//!
//! The reference has one catalogue file per synth layer, plus one file for
//! controls outside the layers. The layer of a key is a fact of one synth,
//! so each adapter supplies its own map; this module only dispatches.
//!
//! ```
//! use sound_to_patch::preset::catalogue::{Layer, placement};
//! use sound_to_patch::synth::Synth;
//!
//! let place = placement(Synth::Vital, "osc_2_unison_detune")?;
//! assert_eq!(place.layer, Some(Layer::Unison));
//! assert_eq!(place.pattern, "osc_N_unison_detune");
//! assert_eq!(place.instance.as_deref(), Some("2"));
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::fmt;

use crate::preset::vital;
use crate::synth::{Synth, SynthError};

/// A synth layer, in signal-path order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Layer {
    /// Pitch, glide and voicing.
    Pitch,
    /// The oscillator source.
    Oscillator,
    /// Unison voices and width.
    Unison,
    /// The filters.
    Filter,
    /// The envelopes.
    Envelope,
    /// LFOs, random sources, macros and the modulation matrix.
    Modulation,
    /// Distortion.
    Distortion,
    /// Space and effects.
    Space,
}

impl Layer {
    /// Every layer, in signal-path order.
    pub const ALL: [Self; 8] = [
        Self::Pitch,
        Self::Oscillator,
        Self::Unison,
        Self::Filter,
        Self::Envelope,
        Self::Modulation,
        Self::Distortion,
        Self::Space,
    ];

    /// The file stem and text form, such as `oscillator`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pitch => "pitch",
            Self::Oscillator => "oscillator",
            Self::Unison => "unison",
            Self::Filter => "filter",
            Self::Envelope => "envelope",
            Self::Modulation => "modulation",
            Self::Distortion => "distortion",
            Self::Space => "space",
        }
    }

    /// The name for people, such as `Pitch and glide`.
    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Self::Pitch => "Pitch and glide",
            Self::Oscillator => "Oscillator source",
            Self::Unison => "Unison and width",
            Self::Filter => "Filter",
            Self::Envelope => "Envelopes",
            Self::Modulation => "Modulation",
            Self::Distortion => "Distortion",
            Self::Space => "Space and effects",
        }
    }
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.as_str())
    }
}

/// Where one key sits in the reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    /// The layer, or `None` for a control outside the layers, such as the
    /// tempo or the oversampling.
    pub layer: Option<Layer>,
    /// The key with its instance replaced by `N`, such as `osc_N_level`.
    /// A key with one instance is its own pattern.
    pub pattern: String,
    /// The instance the key names, such as `2` in `osc_2_level`.
    pub instance: Option<String>,
}

/// Where a key of `synth` sits in the reference.
///
/// # Errors
///
/// [`SynthError::NotBuilt`] for a synth without an adapter, and
/// [`SynthError::BrokenData`] for a key the adapter's map does not place.
pub fn placement(synth: Synth, key: &str) -> Result<Placement, SynthError> {
    match synth.ready()? {
        Synth::Vital => vital::placement(key).ok_or_else(|| SynthError::BrokenData {
            synth,
            cause: format!("the reference map places no layer for {key:?}"),
        }),
        Synth::Serum2 => Err(SynthError::NotBuilt(synth)),
    }
}

/// Keys that files saved by some builds of `synth` hold and the registry
/// does not cover. The reference lists them apart, with no range.
///
/// # Errors
///
/// [`SynthError::NotBuilt`] for a synth without an adapter.
pub fn unread_keys(synth: Synth) -> Result<&'static [&'static str], SynthError> {
    match synth.ready()? {
        Synth::Vital => Ok(&vital::RETAIL_ONLY_KEYS),
        Synth::Serum2 => Err(SynthError::NotBuilt(synth)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::registry::registry_for;

    #[test]
    fn every_key_of_a_ready_synth_has_a_place() {
        for synth in Synth::READY {
            let registry = registry_for(synth).unwrap();
            for spec in registry.iter() {
                let place = placement(synth, &spec.key).unwrap();
                assert!(!place.pattern.is_empty(), "{}", spec.key);
            }
        }
    }

    #[test]
    fn serum2_is_not_ready() {
        assert!(matches!(
            placement(Synth::Serum2, "a"),
            Err(SynthError::NotBuilt(Synth::Serum2))
        ));
    }
}
