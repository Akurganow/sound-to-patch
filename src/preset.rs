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
use std::io::Read;
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
    let bytes = read_capped(path, format.max_bytes())?;
    let model = format.read(&bytes).map_err(|error| PresetError::Format {
        path: path.to_owned(),
        error,
    })?;
    Ok(format!("{}\n{}", model.header(), model.describe(opts)))
}

/// Reads at most `max` bytes of a file, so an oversized file is refused
/// without loading it.
fn read_capped(path: &Path, max: usize) -> Result<Vec<u8>, PresetError> {
    let read_error = |cause| PresetError::Read {
        path: path.to_owned(),
        cause,
    };
    let limit = u64::try_from(max).map_or(u64::MAX, |max| max.saturating_add(1));
    let mut bytes = Vec::new();
    fs::File::open(path)
        .and_then(|file| file.take(limit).read_to_end(&mut bytes))
        .map_err(read_error)?;
    if bytes.len() > max {
        return Err(PresetError::Format {
            path: path.to_owned(),
            error: FormatError::new(
                "cannot read the preset",
                format!("the file is larger than the limit of {max} bytes"),
                "Check that the file is a preset of the target synth",
            ),
        });
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_oversized_file_is_refused_unread() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.vital");
        fs::write(&path, vec![b' '; 11]).unwrap();
        let error = read_capped(&path, 10).unwrap_err().to_string();
        assert!(
            error.contains("larger than the limit of 10 bytes"),
            "{error}"
        );
        assert_eq!(read_capped(&path, 11).unwrap().len(), 11);
    }
}
