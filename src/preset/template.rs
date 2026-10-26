//! Where the template of a preset comes from: a path on the command line,
//! a file the adapter knows about, or a template built into the binary.
//!
//! [`locate_template`] applies that order. A path the user names that does
//! not exist is an error, never a reason to fall back.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use crate::preset::format::PresetModel;
use crate::preset::read_prefix;

/// The template rules of one synth.
pub trait TemplateSource: Send + Sync {
    /// How a user makes a template. The missing-template error prints it.
    fn help(&self) -> TemplateHelp;

    /// Files searched after the command-line path, in order. May be empty.
    fn candidates(&self) -> Vec<PathBuf>;

    /// A template built into the binary, or `None` when the synth needs a
    /// file saved by the app.
    fn builtin(&self) -> Option<Vec<u8>>;

    /// Errors and warnings about a template.
    fn check(&self, model: &dyn PresetModel) -> Vec<TemplateCheck>;
}

/// The text that tells a user how to make a template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateHelp(pub String);

impl fmt::Display for TemplateHelp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// How serious a [`TemplateCheck`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// The template cannot be used.
    Error,
    /// The template works, but the output may surprise.
    Warning,
}

/// One finding about a template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateCheck {
    /// How serious it is.
    pub severity: Severity,
    /// What was found.
    pub text: String,
    /// What to do about it.
    pub fix: String,
}

impl fmt::Display for TemplateCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(f, "{level}: {}. {}", self.text, self.fix)
    }
}

/// Which step of [`locate_template`] supplied the template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    /// The path on the command line.
    CommandLine(PathBuf),
    /// A file the adapter searches.
    Candidate(PathBuf),
    /// The template built into the binary.
    Builtin,
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CommandLine(path) => write!(f, "{}", path.display()),
            Self::Candidate(path) => write!(f, "{} (found)", path.display()),
            Self::Builtin => write!(f, "built-in default"),
        }
    }
}

/// The bytes of a template and where they came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    /// The file contents.
    pub bytes: Vec<u8>,
    /// Which step supplied them.
    pub origin: Origin,
}

/// No template could be found or read.
#[derive(Debug)]
pub enum TemplateError {
    /// The command-line path cannot be read.
    Unreadable {
        /// The path.
        path: PathBuf,
        /// Why.
        cause: io::Error,
    },
    /// No step supplied a template.
    Missing {
        /// Every path that was tried, in order.
        tried: Vec<PathBuf>,
        /// How to make one.
        help: TemplateHelp,
    },
}

impl fmt::Display for TemplateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { path, cause } => write!(
                f,
                "cannot read the template {}: {cause}. Check the path given with --template",
                path.display()
            ),
            Self::Missing { tried, help } => {
                if tried.is_empty() {
                    return write!(f, "no template found and no path was searched. {help}");
                }
                write!(f, "no template found; tried:")?;
                for path in tried {
                    write!(f, "\n  {}", path.display())?;
                }
                write!(f, "\n{help}")
            }
        }
    }
}

impl Error for TemplateError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Unreadable { cause, .. } => Some(cause),
            Self::Missing { .. } => None,
        }
    }
}

/// Finds the template: the command-line path, then the first existing
/// candidate, then the built-in template.
///
/// # Errors
///
/// [`TemplateError::Unreadable`] when the command-line path cannot be read
/// or holds more than `max` bytes, with no fallback, and
/// [`TemplateError::Missing`] when no step supplies a template. A candidate
/// over `max` bytes is passed over.
pub fn locate_template(
    source: &dyn TemplateSource,
    cli_path: Option<&Path>,
    max: usize,
) -> Result<Template, TemplateError> {
    let read = |path: &Path| {
        let bytes = read_prefix(path, max)?;
        if bytes.len() > max {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("the file is larger than the limit of {max} bytes"),
            ));
        }
        Ok(bytes)
    };
    if let Some(path) = cli_path {
        return read(path)
            .map(|bytes| Template {
                bytes,
                origin: Origin::CommandLine(path.to_owned()),
            })
            .map_err(|cause| TemplateError::Unreadable {
                path: path.to_owned(),
                cause,
            });
    }
    let candidates = source.candidates();
    for path in &candidates {
        if path.is_file()
            && let Ok(bytes) = read(path)
        {
            return Ok(Template {
                bytes,
                origin: Origin::Candidate(path.clone()),
            });
        }
    }
    source
        .builtin()
        .map(|bytes| Template {
            bytes,
            origin: Origin::Builtin,
        })
        .ok_or_else(|| TemplateError::Missing {
            tried: candidates,
            help: source.help(),
        })
}
