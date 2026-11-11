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
//!   layer values into edits, and the synth-neutral plan of the pitch and
//!   envelope groups.
//! - [`catalogue`]: the layer and pattern of each control in the agent
//!   reference.
//!
//! [`inspect`] runs `preset inspect` and [`apply_file`] runs `preset apply`.

use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::preset::edits::{EditError, EditErrors};
use crate::preset::format::{DescribeOptions, FormatError, PresetFormat, format_for};
use crate::preset::registry::registry_for;
use crate::preset::template::{Severity, TemplateError, locate_template};
use crate::synth::{SynthArg, SynthError};

pub mod apply;
pub mod catalogue;
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
    Edits {
        /// The edit list file.
        path: PathBuf,
        /// Every problem.
        errors: EditErrors,
    },
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
            Self::Edits { path, errors } => write!(f, "{}: {errors}", path.display()),
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
            Self::Edits { errors, .. } => Some(errors),
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

/// The largest edit list `preset apply` reads. A list that sets every
/// control of a synth takes about 100 kB.
const EDIT_LIST_MAX_BYTES: usize = 1 << 20;

/// Reads the edit list as text. A file larger than
/// [`EDIT_LIST_MAX_BYTES`] is refused without loading it, and a file that
/// is not UTF-8 is refused with the offset of the first bad byte.
fn read_edit_list(path: &Path) -> Result<String, PresetError> {
    let refuse = |cause: String, fix: &str| PresetError::Edits {
        path: path.to_owned(),
        errors: EditErrors(vec![EditError {
            place: "the edit list".to_owned(),
            cause,
            fix: fix.to_owned(),
        }]),
    };
    let bytes = read_prefix(path, EDIT_LIST_MAX_BYTES).map_err(|cause| PresetError::Read {
        path: path.to_owned(),
        cause,
    })?;
    if bytes.len() > EDIT_LIST_MAX_BYTES {
        return Err(refuse(
            format!("the file is larger than the limit of {EDIT_LIST_MAX_BYTES} bytes"),
            "Pass the edit list itself; a list holds one short record per edit",
        ));
    }
    String::from_utf8(bytes).map_err(|error| {
        refuse(
            format!(
                "byte {} is not UTF-8 text",
                error.utf8_error().valid_up_to()
            ),
            "Save the file as UTF-8",
        )
    })
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
        return Err(exists(out));
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
/// and must not exist unless the request sets `force`. The file is
/// written to a temporary file next to it and then published: renamed
/// with `force`, hard-linked without it, so a file that appears meanwhile
/// is never replaced. A run that returns an error leaves no file, and a
/// run killed during the write leaves at most the temporary `.partial`
/// file next to the output. A folder without hard links, such as one on
/// exFAT, gets the file through an exclusive create and a write in place
/// instead: the file is visible while it is written, an error removes it,
/// and a run killed during the write leaves a partial output file.
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
    let edit_errors = |errors: EditErrors| PresetError::Edits {
        path: request.edits.to_owned(),
        errors,
    };
    let text = read_edit_list(request.edits)?;
    let list = edits::parse(&text, edits::Path::Agent).map_err(edit_errors)?;
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
    let resolved = edits::resolve(&list, registry).map_err(edit_errors)?;
    let report = apply::apply(format, registry, model.as_mut(), &resolved).map_err(edit_errors)?;
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
    write_atomically(out, format.extension(), &bytes, request.force)?;
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

/// The error for an output file that exists without `--force`.
fn exists(out: &Path) -> PresetError {
    PresetError::Output {
        path: out.to_owned(),
        cause: "the file exists".to_owned(),
        fix: "Pass --force to replace it, or write to a new file".to_owned(),
    }
}

/// Writes a file through a new temporary sibling, so a failed write leaves
/// no partial file. With `force` a rename replaces `out`. Without it a
/// hard link publishes the file and fails if `out` exists, so a file that
/// appears after the check is never replaced.
fn write_atomically(
    out: &Path,
    extension: &str,
    bytes: &[u8],
    force: bool,
) -> Result<(), PresetError> {
    publish(out, extension, bytes, force, |from, to| {
        fs::hard_link(from, to)
    })
}

/// [`write_atomically`] with the hard link as a parameter, so a test can
/// stand in for a file system without links.
fn publish(
    out: &Path,
    extension: &str,
    bytes: &[u8],
    force: bool,
    link: impl Fn(&Path, &Path) -> io::Result<()>,
) -> Result<(), PresetError> {
    static RUN: AtomicUsize = AtomicUsize::new(0);
    let temporary = out.with_extension(format!(
        "{extension}.{}-{}.partial",
        std::process::id(),
        RUN.fetch_add(1, Ordering::Relaxed)
    ));
    let output_error = |cause: io::Error| PresetError::Output {
        path: out.to_owned(),
        cause: cause.to_string(),
        fix: "Check that the folder exists, is writable and has free space".to_owned(),
    };
    // A failed create_new made no file, and the path may be another's.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(output_error)?;
    let failed = |cause: io::Error| {
        let _ = fs::remove_file(&temporary);
        output_error(cause)
    };
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(failed)?;
    drop(file);
    if force {
        return fs::rename(&temporary, out).map_err(failed);
    }
    let linked = link(&temporary, out);
    let _ = fs::remove_file(&temporary);
    match linked {
        Ok(()) => Ok(()),
        Err(cause) if cause.kind() == io::ErrorKind::AlreadyExists => Err(exists(out)),
        // FAT, exFAT and some network folders have no hard links. An
        // exclusive create still never replaces a file, but the file is
        // visible while it is written. An error removes it; a kill during
        // the write leaves it partial.
        Err(_) => {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(out)
                .map_err(|cause| match cause.kind() {
                    io::ErrorKind::AlreadyExists => exists(out),
                    _ => output_error(cause),
                })?;
            file.write_all(bytes)
                .and_then(|()| file.sync_all())
                .map_err(|cause| {
                    let _ = fs::remove_file(out);
                    output_error(cause)
                })
        }
    }
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
    fn a_file_that_appears_after_the_check_is_not_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("Out.vital");
        fs::write(&out, "theirs").unwrap();
        let error = write_atomically(&out, "vital", b"ours", false)
            .unwrap_err()
            .to_string();
        assert!(error.contains("the file exists. Pass --force"), "{error}");
        assert_eq!(fs::read_to_string(&out).unwrap(), "theirs");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        write_atomically(&out, "vital", b"ours", true).unwrap();
        assert_eq!(fs::read_to_string(&out).unwrap(), "ours");
        let new = dir.path().join("New.vital");
        write_atomically(&new, "vital", b"new", false).unwrap();
        assert_eq!(fs::read_to_string(&new).unwrap(), "new");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn a_folder_without_hard_links_still_gets_a_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let no_links = |_: &Path, _: &Path| Err(io::Error::from(io::ErrorKind::Unsupported));
        let new = dir.path().join("New.vital");
        publish(&new, "vital", b"new", false, no_links).unwrap();
        assert_eq!(fs::read_to_string(&new).unwrap(), "new");
        let error = publish(&new, "vital", b"again", false, no_links)
            .unwrap_err()
            .to_string();
        assert!(error.contains("the file exists. Pass --force"), "{error}");
        assert_eq!(fs::read_to_string(&new).unwrap(), "new");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn an_edit_list_is_refused_as_an_edit_list() {
        let dir = tempfile::tempdir().unwrap();
        let big = dir.path().join("big.json");
        fs::write(&big, vec![b' '; EDIT_LIST_MAX_BYTES + 1]).unwrap();
        let error = read_edit_list(&big).unwrap_err().to_string();
        assert_eq!(
            error,
            format!(
                "{}: the edit list has 1 problem(s):\n  the edit list: the file is larger than the limit of 1048576 bytes. Pass the edit list itself; a list holds one short record per edit",
                big.display()
            )
        );
        fs::write(&big, vec![b' '; EDIT_LIST_MAX_BYTES]).unwrap();
        assert_eq!(read_edit_list(&big).unwrap().len(), EDIT_LIST_MAX_BYTES);
        let latin = dir.path().join("latin.json");
        fs::write(&latin, b"{\"x\": \"caf\xe9\"}").unwrap();
        let error = read_edit_list(&latin).unwrap_err().to_string();
        assert!(
            error.ends_with("the edit list: byte 10 is not UTF-8 text. Save the file as UTF-8"),
            "{error}"
        );
    }

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
