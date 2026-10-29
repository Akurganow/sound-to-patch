//! The target synth, [`Synth`], and why one cannot be used.
//!
//! Two synths are equal targets, Vital and Serum 2. Only the synths in
//! [`Synth::READY`] have an adapter.

use std::error::Error;
use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A target synth.
///
/// The text form is `vital` or `serum2`. `Display`, `FromStr`, serde and
/// the command line all use it, and parsing is exact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
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

    /// The synth whose preset extension `ext` is, compared without regard
    /// to case.
    #[doc(hidden)]
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
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
    /// The extension of a preset path names a synth without an adapter.
    FileNotReady {
        /// The preset path.
        path: String,
        /// The synth its extension names.
        synth: Synth,
    },
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
            Self::FileNotReady { path, synth } => write!(
                f,
                "cannot read {path:?}: {} files cannot be read yet. Only {} files (.{}) work now",
                synth.name(),
                Synth::READY[0].name(),
                Synth::READY[0].extension()
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
}
