//! Preset files of the target synths, behind one adapter per synth.
//!
//! - [`format`](mod@format): the [`PresetFormat`](format::PresetFormat) and
//!   [`PresetModel`](format::PresetModel) traits that read, edit and write
//!   a preset, and the tree that `preset inspect` prints.
//! - [`registry`]: the [`ParamRegistry`](registry::ParamRegistry) of each
//!   synth, with the law between stored and physical values and its
//!   provenance.
//! - [`template`]: where the template of a new preset comes from.
//! - [`vital`]: the Vital adapter.
//!
//! [`inspect`] runs the `preset inspect` command.

use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::preset::format::{DescribeOptions, FormatError, format_for};
use crate::synth::{SynthArg, SynthError};

pub mod format;
pub mod registry;
pub mod template;
pub mod vital;

/// A preset command that failed.
#[derive(Debug)]
pub enum PresetError {
    /// No usable target synth.
    Synth(SynthError),
    /// A file cannot be read.
    Read {
        /// The file.
        path: PathBuf,
        /// Why.
        cause: io::Error,
    },
    /// A file is not a valid preset.
    Format {
        /// The file.
        path: PathBuf,
        /// Why.
        error: FormatError,
    },
}

impl fmt::Display for PresetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Synth(error) => error.fmt(f),
            Self::Read { path, cause } => write!(
                f,
                "cannot read {}: {cause}. Check the path and the permissions",
                path.display()
            ),
            Self::Format { path, error } => write!(f, "{}: {error}", path.display()),
        }
    }
}

impl Error for PresetError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Synth(error) => Some(error),
            Self::Read { cause, .. } => Some(cause),
            Self::Format { error, .. } => Some(error),
        }
    }
}

impl From<SynthError> for PresetError {
    fn from(error: SynthError) -> Self {
        Self::Synth(error)
    }
}

/// The text `preset inspect` prints: the header fields of the adapter, a
/// blank line, then the decoded tree.
///
/// # Errors
///
/// A [`PresetError`] when the synth cannot be resolved, the file cannot be
/// read or it is not a valid preset.
pub fn inspect(
    path: &Path,
    synth: &SynthArg,
    opts: &DescribeOptions,
) -> Result<String, PresetError> {
    let format = format_for(synth.resolve(Some(path))?)?;
    let bytes = fs::read(path).map_err(|cause| PresetError::Read {
        path: path.to_owned(),
        cause,
    })?;
    let model = format.read(&bytes).map_err(|error| PresetError::Format {
        path: path.to_owned(),
        error,
    })?;
    Ok(format!("{}\n{}", model.header(), model.describe(opts)))
}
