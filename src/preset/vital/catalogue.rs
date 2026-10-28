//! The layer of each Vital control in the agent reference.
//!
//! The map is our own layout over the key names of the registry. A key
//! that no rule places returns `None`, so a new registry key fails the
//! coverage test until it gets a layer here.

use crate::preset::catalogue::{Layer, Placement};

/// Groups whose keys read `<group>_<number>_<control>`.
const NUMBERED: [&str; 6] = ["env", "filter", "lfo", "modulation", "osc", "random"];

/// Oscillator controls that shape the unison stack.
const UNISON: [&str; 9] = [
    "detune_power",
    "detune_range",
    "frame_spread",
    "spectral_unison",
    "stack_style",
    "stereo_spread",
    "unison_blend",
    "unison_detune",
    "unison_voices",
];

/// Controls of the whole patch that belong to no layer.
const GLOBAL: [&str; 5] = [
    "beats_per_minute",
    "bypass",
    "oversampling",
    "view_spectrogram",
    "volume",
];

/// Where a Vital key sits in the reference, or `None` for a key no rule
/// places.
pub(crate) fn placement(key: &str) -> Option<Placement> {
    let mut parts = key.splitn(3, '_');
    let (group, second, rest) = (parts.next()?, parts.next(), parts.next());
    if let (Some(id), Some(control)) = (second, rest)
        && NUMBERED.contains(&group)
        && !id.is_empty()
        && id.bytes().all(|byte| byte.is_ascii_digit())
    {
        let layer = match group {
            "env" => Layer::Envelope,
            "filter" => Layer::Filter,
            "osc" if UNISON.contains(&control) => Layer::Unison,
            "osc" => Layer::Oscillator,
            _ => Layer::Modulation,
        };
        return Some(Placement {
            layer: Some(layer),
            pattern: format!("{group}_N_{control}"),
            instance: Some(id.to_owned()),
        });
    }
    let layer = if GLOBAL.contains(&key) {
        None
    } else {
        Some(match group {
            "chorus" | "compressor" | "delay" | "effect" | "eq" | "flanger" | "phaser"
            | "reverb" => Layer::Space,
            "distortion" => Layer::Distortion,
            "filter" => Layer::Filter,
            "sample" | "sub" => Layer::Oscillator,
            "stereo" => Layer::Unison,
            "macro" | "mod" => Layer::Modulation,
            "velocity" => Layer::Envelope,
            "voice" if key == "voice_amplitude" => Layer::Envelope,
            "legato" | "mpe" | "pitch" | "polyphony" | "portamento" | "voice" => Layer::Pitch,
            _ => return None,
        })
    };
    Some(Placement {
        layer,
        pattern: key.to_owned(),
        instance: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preset::registry::registry_for;
    use crate::synth::Synth;

    fn layer(key: &str) -> Option<Layer> {
        placement(key).unwrap().layer
    }

    #[test]
    fn numbered_keys_share_a_pattern() {
        let place = placement("modulation_12_amount").unwrap();
        assert_eq!(place.pattern, "modulation_N_amount");
        assert_eq!(place.instance.as_deref(), Some("12"));
        assert_eq!(place.layer, Some(Layer::Modulation));
    }

    #[test]
    fn the_effect_filter_and_numbered_globals_are_their_own_patterns() {
        for key in ["filter_fx_cutoff", "macro_control_1", "chorus_delay_2"] {
            let place = placement(key).unwrap();
            assert_eq!(place.pattern, key);
            assert_eq!(place.instance, None);
        }
        assert_eq!(layer("filter_fx_cutoff"), Some(Layer::Filter));
    }

    #[test]
    fn layers_follow_the_rules() {
        assert_eq!(layer("osc_1_unison_detune"), Some(Layer::Unison));
        assert_eq!(layer("osc_1_level"), Some(Layer::Oscillator));
        assert_eq!(layer("portamento_time"), Some(Layer::Pitch));
        assert_eq!(layer("voice_amplitude"), Some(Layer::Envelope));
        assert_eq!(layer("voice_tune"), Some(Layer::Pitch));
        assert_eq!(layer("effect_chain_order"), Some(Layer::Space));
        assert_eq!(layer("volume"), None);
    }

    #[test]
    fn an_unknown_family_has_no_place() {
        assert_eq!(placement("zz_new"), None);
    }

    #[test]
    fn the_registry_forms_248_patterns() {
        let registry = registry_for(Synth::Vital).unwrap();
        let mut patterns: Vec<String> = registry
            .iter()
            .map(|spec| placement(&spec.key).unwrap().pattern)
            .collect();
        patterns.sort_unstable();
        patterns.dedup();
        assert_eq!(patterns.len(), 248);
    }
}
