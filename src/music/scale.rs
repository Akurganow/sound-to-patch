//! Scale lookup and the snap rule.
//!
//! A note is in a scale when its offset above the root, modulo 12, is one
//! of the scale's offsets. The snap rule keeps a note in the scale and
//! moves any other to the nearest scale pitch, a tie going to the lower.
//!
//! ```
//! use sound_to_patch::music::scale::{offsets, snap};
//!
//! let major = offsets("scale.major")?;
//! assert_eq!(snap(60, 4, major), 59);
//! assert_eq!(snap(61, 4, major), 61);
//! # Ok::<(), sound_to_patch::music::MusicError>(())
//! ```

use crate::music::entry::{Group, Rule};
use crate::music::{MusicError, dictionary};

/// The offsets of a scale entry.
///
/// # Errors
///
/// [`MusicError::UnknownScale`] for an id that is no scale entry, and
/// [`MusicError::BrokenData`] when the dictionary does not load.
pub fn offsets(id: &str) -> Result<&'static [u8], MusicError> {
    let entry = dictionary()?
        .entry(id)
        .filter(|entry| entry.group == Group::Scale)
        .ok_or_else(|| MusicError::UnknownScale(id.to_owned()))?;
    match &entry.rule {
        Rule::PitchSet { offsets, .. } => Ok(offsets),
        _ => Err(MusicError::UnknownScale(id.to_owned())),
    }
}

/// Whether a pitch lies in the scale of `offsets` on `root`.
#[must_use]
pub fn contains(pitch: u8, root: u8, offsets: &[u8]) -> bool {
    let offset = (i16::from(pitch) - i16::from(root)).rem_euclid(12);
    offsets.iter().any(|&scale| i16::from(scale) == offset)
}

/// The pitch moved into the scale: kept when it is in, else the nearest
/// scale pitch below or above, a tie going to the lower. A scale with no
/// offsets keeps every pitch.
#[must_use]
pub fn snap(pitch: u8, root: u8, offsets: &[u8]) -> u8 {
    if offsets.is_empty() || contains(pitch, root, offsets) {
        return pitch;
    }
    let below = (0..=pitch).rev().find(|&p| contains(p, root, offsets));
    let above = (pitch..=127).find(|&p| contains(p, root, offsets));
    match (below, above) {
        (Some(low), Some(high)) if pitch - low <= high - pitch => low,
        (_, Some(high)) => high,
        (Some(low), None) => low,
        (None, None) => pitch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAJOR: [u8; 7] = [0, 2, 4, 5, 7, 9, 11];
    const MINOR_PENTATONIC: [u8; 5] = [0, 3, 5, 7, 10];
    const NATURAL_MINOR: [u8; 7] = [0, 2, 3, 5, 7, 8, 10];

    #[test]
    fn snaps_the_published_cases() {
        assert_eq!(snap(60, 4, &MAJOR), 59);
        assert_eq!(snap(61, 4, &MAJOR), 61);
        assert_eq!(snap(62, 0, &MINOR_PENTATONIC), 63);
        assert_eq!(snap(66, 9, &NATURAL_MINOR), 65);
    }

    #[test]
    fn a_tie_goes_to_the_lower_pitch() {
        assert_eq!(snap(61, 0, &MAJOR), 60);
        assert_eq!(snap(66, 0, &MAJOR), 65);
    }

    #[test]
    fn every_note_in_c_major_moves_by_at_most_one() {
        for pitch in 0..=127 {
            let snapped = snap(pitch, 0, &MAJOR);
            assert!(snapped.abs_diff(pitch) <= 1, "{pitch} -> {snapped}");
            assert!(contains(snapped, 0, &MAJOR));
        }
    }

    #[test]
    fn the_ends_of_midi_snap_inward() {
        assert_eq!(snap(127, 0, &[0]), 120);
        assert_eq!(snap(0, 1, &[0]), 1);
    }
}
