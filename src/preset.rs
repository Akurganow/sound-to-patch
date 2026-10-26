//! Preset files of the target synths, behind one adapter per synth.
//!
//! - [`format`](mod@format): the [`PresetFormat`] and
//!   [`PresetModel`](format::PresetModel) traits that read, edit and write
//!   a preset, and the tree that `preset inspect` prints.
//! - [`registry`]: the [`ParamRegistry`](registry::ParamRegistry) of each
//!   synth, with the law between stored and physical values and its
//!   provenance.
//! - [`template`]: where the template of a new preset comes from.
//! - [`vital`]: the Vital adapter.
//! - [`edits`]: the edit list an agent writes, and its check against the
//!   registry.
//! - [`apply`]: applies checked edits to a model.
//! - [`mapper`]: the [`SynthMapper`](mapper::SynthMapper) trait that turns
//!   layer values into edits.
//!
//! [`inspect`] runs `preset inspect` and [`apply_file`] runs `preset apply`.

use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::preset::edits::EditErrors;
use crate::preset::format::{DescribeOptions, FormatError, PresetFormat, format_for};
use crate::preset::registry::registry_for;
use crate::preset::template::{Severity, TemplateError, locate_template};
use crate::synth::{SynthArg, SynthError};

pub mod apply;
pub mod edits;
pub mod format;
pub mod mapper;
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
    /// The edit list is not valid or does not apply.
    Edits(EditErrors),
    /// No usable template.
    Template(TemplateError),
    /// The output path is refused.
    Output {
        /// The path.
        path: PathBuf,
        /// Why.
        cause: String,
        /// How to fix it.
        fix: String,
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
            Self::Edits(errors) => errors.fmt(f),
            Self::Template(error) => error.fmt(f),
            Self::Output { path, cause, fix } => {
                write!(f, "cannot write {}: {cause}. {fix}", path.display())
            }
        }
    }
}

impl Error for PresetError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Synth(error) => Some(error),
            Self::Read { cause, .. } => Some(cause),
            Self::Format { error, .. } => Some(error),
            Self::Edits(errors) => Some(errors),
            Self::Template(error) => Some(error),
            Self::Output { .. } => None,
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

/// Reads at most `max + 1` bytes of a file, so a caller can refuse an
/// oversized file without loading it.
pub(crate) fn read_prefix(path: &Path, max: usize) -> io::Result<Vec<u8>> {
    let limit = u64::try_from(max).map_or(u64::MAX, |max| max.saturating_add(1));
    let mut bytes = Vec::new();
    fs::File::open(path).and_then(|file| file.take(limit).read_to_end(&mut bytes))?;
    Ok(bytes)
}

/// Reads at most `max` bytes of a file, so an oversized file is refused
/// without loading it.
fn read_capped(path: &Path, max: usize) -> Result<Vec<u8>, PresetError> {
    let bytes = read_prefix(path, max).map_err(|cause| PresetError::Read {
        path: path.to_owned(),
        cause,
    })?;
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

/// The inputs of `preset apply`.
#[derive(Clone, Debug)]
pub struct ApplyRequest<'a> {
    /// The edit list file.
    pub edits: &'a Path,
    /// The output preset file.
    pub out: &'a Path,
    /// A template path, or `None` for the synth's own lookup.
    pub template: Option<&'a Path>,
    /// The target synth.
    pub synth: &'a SynthArg,
    /// The preset name, or `None` for the output file stem.
    pub name: Option<&'a str>,
    /// Whether an existing output file may be replaced.
    pub force: bool,
}

/// Whether two paths name the same file. Paths that do not exist yet are
/// compared as written.
fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Refuses an output path with the wrong extension, one that is an input,
/// an existing file without `force`, or one inside the factory folder of
/// the synth.
fn check_output(request: &ApplyRequest<'_>, format: &dyn PresetFormat) -> Result<(), PresetError> {
    let out = request.out;
    let synth = format.synth();
    let refuse = |cause: String, fix: String| PresetError::Output {
        path: out.to_owned(),
        cause,
        fix,
    };
    let extension = out.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    if !extension.eq_ignore_ascii_case(format.extension()) {
        return Err(refuse(
            format!(
                "a {} preset needs the .{} extension",
                synth.name(),
                format.extension()
            ),
            format!("Name the output like bass.{}", format.extension()),
        ));
    }
    if same_file(out, request.edits)
        || request
            .template
            .is_some_and(|template| same_file(out, template))
    {
        return Err(refuse(
            "it is the edit list or the template".to_owned(),
            "Write to a new file; the inputs stay as they are".to_owned(),
        ));
    }
    if !request.force && fs::symlink_metadata(out).is_ok() {
        return Err(refuse(
            "the file exists".to_owned(),
            "Pass --force to replace it, or write to a new file".to_owned(),
        ));
    }
    // The folder as the system resolves it: `-o x.vital` inside a factory
    // folder, a link into one and `Factory/../x.vital` all count by where
    // the file lands. A folder that does not resolve is taken as written.
    let parent = out
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent).unwrap_or_else(|_| parent.to_owned());
    let factory = format.factory_folder();
    let inside = !factory.is_empty()
        && parent.ancestors().any(|dir| {
            let tail: Vec<_> = dir.iter().rev().take(factory.len()).collect();
            tail.len() == factory.len()
                && tail.iter().zip(factory.iter().rev()).all(|(have, want)| {
                    have.to_str()
                        .is_some_and(|have| have.eq_ignore_ascii_case(want))
                })
        });
    if inside {
        return Err(refuse(
            format!(
                "it is inside {}, the factory folder of {}",
                factory.join("/"),
                synth.name()
            ),
            "Write into your own preset folder; factory presets stay untouched".to_owned(),
        ));
    }
    Ok(())
}

/// Runs `preset apply`: reads the edit list, takes the template, applies
/// the edits and writes the preset. Returns the report to print.
///
/// The output must carry the extension of the synth, and must not be the
/// edit list, the template or a file in the factory folder of the synth,
/// and must not exist unless the request sets `force`. The
/// file is written to a temporary file next to it and renamed, so a failed
/// run leaves no partial file.
///
/// # Errors
///
/// A [`PresetError`] for any refused input, edit or path.
pub fn apply_file(request: &ApplyRequest<'_>) -> Result<String, PresetError> {
    let out = request.out;
    let synth = request.synth.resolve(request.template.or(Some(out)))?;
    let format = format_for(synth)?;
    let registry = registry_for(synth)?;
    check_output(request, format)?;
    let text =
        String::from_utf8(read_capped(request.edits, format.max_bytes())?).map_err(|error| {
            PresetError::Read {
                path: request.edits.to_owned(),
                cause: io::Error::new(io::ErrorKind::InvalidData, error),
            }
        })?;
    let list = edits::parse(&text, edits::Path::Agent).map_err(PresetError::Edits)?;
    let template = locate_template(format.template(), request.template, format.max_bytes())
        .map_err(PresetError::Template)?;
    let template_path = match &template.origin {
        template::Origin::CommandLine(path) | template::Origin::Candidate(path) => path.clone(),
        template::Origin::Builtin => PathBuf::from("built-in template"),
    };
    let mut model = format
        .read(&template.bytes)
        .map_err(|error| PresetError::Format {
            path: template_path.clone(),
            error,
        })?;
    let checks = format.template().check(model.as_ref());
    if let Some(error) = checks
        .iter()
        .find(|check| check.severity == Severity::Error)
    {
        return Err(PresetError::Format {
            path: template_path,
            error: FormatError::new(
                "cannot use the template",
                error.text.clone(),
                error.fix.clone(),
            ),
        });
    }
    let resolved = edits::resolve(&list, registry).map_err(PresetError::Edits)?;
    let report =
        apply::apply(format, registry, model.as_mut(), &resolved).map_err(PresetError::Edits)?;
    let name = request
        .name
        .map(str::to_owned)
        .or_else(|| {
            out.file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_owned)
        })
        .unwrap_or_default();
    let write_error = |error: FormatError| PresetError::Format {
        path: out.to_owned(),
        error,
    };
    model.set_name(&name).map_err(write_error)?;
    let bytes = format.write(model.as_ref()).map_err(write_error)?;
    format.read(&bytes).map_err(write_error)?;
    write_atomically(out, format.extension(), &bytes)?;
    let stamp = model
        .header()
        .fields
        .into_iter()
        .find(|field| field.label == "build")
        .map(|field| field.value);
    Ok(apply_summary(
        out,
        &template.origin,
        &name,
        stamp.as_deref(),
        &report,
        &checks,
    ))
}

/// Writes a file through a temporary sibling and a rename, so a failed
/// write leaves no partial file.
fn write_atomically(out: &Path, extension: &str, bytes: &[u8]) -> Result<(), PresetError> {
    let temporary = out.with_extension(format!("{extension}.partial"));
    let failed = |cause: io::Error| {
        // The temporary file may not exist; either way nothing is left.
        let _ = fs::remove_file(&temporary);
        PresetError::Output {
            path: out.to_owned(),
            cause: cause.to_string(),
            fix: "Check that the folder exists, is writable and has free space".to_owned(),
        }
    };
    fs::write(&temporary, bytes).map_err(failed)?;
    fs::rename(&temporary, out).map_err(failed)
}

fn apply_summary(
    out: &Path,
    origin: &template::Origin,
    name: &str,
    stamp: Option<&str>,
    report: &apply::ApplyReport,
    checks: &[template::TemplateCheck],
) -> String {
    use std::fmt::Write as _;
    let mut text = format!("wrote {} ({name:?}) from {origin}\n", out.display());
    if let Some(stamp) = stamp {
        let _ = writeln!(text, "  build {stamp}, kept from the template");
    }
    for change in &report.changes {
        let mut notes = Vec::new();
        if change.uncalibrated {
            notes.push("uncalibrated");
        }
        if change.raw {
            notes.push("raw");
        }
        let notes = if notes.is_empty() {
            String::new()
        } else {
            format!(" [{}]", notes.join(", "))
        };
        let _ = writeln!(text, "  set {} = {}{notes}", change.target, change.value);
    }
    for skipped in &report.skipped {
        let _ = writeln!(text, "  skipped {}: {}", skipped.target, skipped.why);
    }
    for check in checks {
        let _ = writeln!(text, "  {check}");
    }
    for warning in &report.warnings {
        let _ = writeln!(text, "  warning: {warning}");
    }
    text
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
