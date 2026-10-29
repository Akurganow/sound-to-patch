//! The MIDI velocity and the error of a note that breaks a rule.

use std::error::Error;
use std::fmt;

use crate::units::Seconds;

/// A MIDI velocity from 1 to 127. Velocity 0 means a note-off in MIDI, so
/// it is not a velocity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Velocity(u8);

impl Velocity {
    /// Checks the range 1 to 127.
    ///
    /// # Errors
    ///
    /// Returns a [`NoteError`] for 0 or a value above 127.
    pub fn new(value: u8) -> Result<Self, NoteError> {
        if (1..=127).contains(&value) {
            Ok(Self(value))
        } else {
            Err(NoteError::Velocity(value))
        }
    }

    /// Builds a velocity in a `const` item. Every caller passes a literal
    /// that a test also builds through [`Self::new`].
    ///
    /// # Panics
    ///
    /// Panics for a value outside 1 to 127, which in a `const` item fails
    /// the build. Velocity 0 would be a note-off.
    ///
    /// ```
    /// # extern crate sound_to_patch_core as sound_to_patch;
    /// use sound_to_patch::notes::Velocity;
    ///
    /// const LOUD: Velocity = Velocity::from_const(100);
    /// assert_eq!(LOUD, Velocity::new(100)?);
    /// # Ok::<(), sound_to_patch::notes::NoteError>(())
    /// ```
    ///
    /// ```compile_fail
    /// # extern crate sound_to_patch_core as sound_to_patch;
    /// use sound_to_patch::notes::Velocity;
    ///
    /// const NOTE_OFF: Velocity = Velocity::from_const(0);
    /// ```
    #[doc(hidden)]
    #[must_use]
    pub const fn from_const(value: u8) -> Self {
        assert!(
            value >= 1 && value <= 127,
            "Velocity::from_const: a velocity is 1 to 127"
        );
        Self(value)
    }

    /// Returns the velocity.
    #[must_use]
    pub fn get(self) -> u8 {
        self.0
    }
}

/// A note or contour that breaks a rule.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NoteError {
    /// The offset is at or before the onset.
    OffsetNotAfterOnset {
        /// The onset.
        onset: Seconds,
        /// The offset.
        offset: Seconds,
    },
    /// A velocity outside 1 to 127.
    Velocity(u8),
    /// A contour with no point.
    EmptyContour,
    /// A contour point with an f0 of 0 Hz.
    ZeroF0,
    /// Two contour points at the same time.
    DuplicateTime,
}

impl fmt::Display for NoteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OffsetNotAfterOnset { onset, offset } => write!(
                f,
                "a note ends at {offset} but starts at {onset}; the offset must come after the onset"
            ),
            Self::Velocity(value) => write!(f, "velocity {value} is outside 1 to 127"),
            Self::EmptyContour => write!(f, "an f0 contour needs at least one point"),
            Self::ZeroF0 => write!(
                f,
                "an f0 contour point is at 0 Hz; every f0 must be above 0 Hz"
            ),
            Self::DuplicateTime => write!(f, "two f0 contour points share a time"),
        }
    }
}

impl Error for NoteError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_velocity_lies_in_1_to_127() {
        assert_eq!(Velocity::new(1).unwrap().get(), 1);
        assert_eq!(Velocity::new(127).unwrap().get(), 127);
        assert_eq!(Velocity::new(0).unwrap_err(), NoteError::Velocity(0));
        assert!(Velocity::new(128).is_err());
    }
}
