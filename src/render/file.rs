//! The `render` command: a preset and a MIDI file to a WAV file and a
//! render record, through the load check.

use std::error::Error;
use std::fmt;
use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use super::check::{LoadCheck, LoadFault, render_checked};
use super::fake::{FakePatch, FakeRenderer};
use super::plugin::{HOST_ENV, PluginRenderer, find_plugin, host_binary};
use super::state::{preset_state, saved_preset};
use super::{HostIdentity, RenderError, RenderJob, RenderRecord, Renderer};
use crate::audio::write_wav_to;
use crate::synth::Synth;
use crate::units::{Bpm, Seconds};

/// What renders a job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Backend {
    /// The synth's plug-in in the host process.
    Plugin,
    /// The fake renderer, for tests of the command.
    Fake,
}

/// A render of a preset file.
#[derive(Clone, Debug)]
pub struct RenderRequest<'a> {
    /// The preset file.
    pub preset: &'a Path,
    /// The MIDI file with the notes.
    pub midi: &'a Path,
    /// The tempo of the MIDI file.
    pub bpm: Bpm,
    /// The WAV file to write. The record goes next to it, with `.json` in
    /// place of `.wav`.
    pub out: &'a Path,
    /// The synth.
    pub synth: Synth,
    /// The plug-in bundle; `None` searches the standard VST3 folders.
    pub plugin: Option<&'a Path>,
    /// Whether an existing output may be replaced.
    pub force: bool,
    /// What renders.
    pub backend: Backend,
}

/// What a render of a preset file did, step by step.
#[derive(Clone, Debug)]
pub struct RenderReport {
    /// The plug-in bundle, for the plug-in backend.
    pub plugin: Option<PathBuf>,
    /// The preset's name, when it has one.
    pub name: Option<String>,
    /// How many notes played.
    pub notes: usize,
    /// The load check.
    pub check: LoadCheck,
    /// The record written next to the audio.
    pub record: RenderRecord,
    /// The WAV file.
    pub wav: PathBuf,
    /// The record file.
    pub record_path: PathBuf,
    /// The tail cap of the job's recipe.
    pub tail_cap: Seconds,
}

impl RenderReport {
    /// The lines that describe each step.
    #[must_use]
    pub fn summary(&self) -> Vec<String> {
        let HostIdentity {
            synth,
            build,
            host,
            arch,
        } = &self.record.identity;
        let from = self
            .plugin
            .as_ref()
            .map(|path| format!(" from {}", path.display()))
            .unwrap_or_default();
        let record = &self.record;
        let tail = if record.tail_capped {
            format!(
                "the tail was still sounding at the cap, {} after the last note-off",
                self.tail_cap
            )
        } else {
            "the tail went quiet".to_owned()
        };
        vec![
            format!(
                "plug-in: {} {build}{from}, in {host} as {arch}",
                synth.name()
            ),
            format!(
                "preset: {}, {} oversampling",
                self.name.as_deref().unwrap_or("(no name)"),
                record.oversampling
            ),
            format!(
                "notes: {}, the last note-off at {:.3} s, {} BPM",
                self.notes,
                record.last_note_off.get(),
                record.bpm.get()
            ),
            format!(
                "init patch: 2 renders, {:.2} dB apart",
                self.check.init_spread_db
            ),
            if self.check.init_like {
                "load check: passed with a warning; the saved state matches".to_owned()
            } else {
                format!(
                    "load check: passed; the saved state matches, and the render lies {:.2} dB from the init patch, beyond the margin of {:.2} dB",
                    self.check.init_distance_db, self.check.margin_db
                )
            },
            format!("clip: {:.3} s, {tail}", record.length.get()),
            format!("wrote {}", self.wav.display()),
            format!("wrote {}", self.record_path.display()),
        ]
    }
}

impl RenderReport {
    /// The warnings of the render: an init-like clip.
    #[must_use]
    pub fn warnings(&self) -> Vec<String> {
        self.check.warning().into_iter().collect()
    }
}

/// A render of a preset file that failed.
#[derive(Debug)]
pub enum RenderFileError {
    /// An input cannot be read.
    Read {
        /// The file.
        path: PathBuf,
        /// Why.
        cause: String,
    },
    /// An output is refused or cannot be written.
    Output {
        /// The file.
        path: PathBuf,
        /// Why.
        cause: String,
        /// How to fix it.
        fix: String,
    },
    /// No plug-in bundle was found.
    NoPlugin(Synth),
    /// The render failed or does not count.
    Load(LoadFault),
}

impl fmt::Display for RenderFileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, cause } => write!(
                f,
                "cannot read {}: {cause}. Check the path and the permissions",
                path.display()
            ),
            Self::Output { path, cause, fix } => {
                write!(f, "cannot write {}: {cause}. {fix}", path.display())
            }
            Self::NoPlugin(synth) => write!(
                f,
                "cannot find the {synth} plug-in: no bundle in /Library/Audio/Plug-Ins/VST3 or the user's VST3 folder. Install {synth}, or pass --plugin with the path of its .vst3 bundle"
            ),
            Self::Load(fault) => write!(f, "{fault}"),
        }
    }
}

impl Error for RenderFileError {}

impl From<LoadFault> for RenderFileError {
    fn from(fault: LoadFault) -> Self {
        Self::Load(fault)
    }
}

impl From<RenderError> for RenderFileError {
    fn from(error: RenderError) -> Self {
        Self::Load(LoadFault::Render(error))
    }
}

fn exists(path: &Path) -> RenderFileError {
    RenderFileError::Output {
        path: path.to_owned(),
        cause: "the file exists".to_owned(),
        fix: "Pass --force to replace it, or write to a new file".to_owned(),
    }
}

/// Writes one output file.
type WriteOutput<'a> = &'a dyn Fn(&mut dyn Write) -> io::Result<()>;

/// Writes each output to a new temporary file in `folder`, and renames the
/// temporary files over the outputs only once every write succeeded. A
/// failed write removes the temporary files and leaves the outputs as they
/// were.
fn write_outputs(
    folder: &Path,
    outputs: &[(&Path, WriteOutput<'_>)],
    force: bool,
) -> Result<(), RenderFileError> {
    let failed = |path: &Path, cause: io::Error| RenderFileError::Output {
        path: path.to_owned(),
        cause: cause.to_string(),
        fix: "Check that the folder is writable and the disk has room".to_owned(),
    };
    let mut builder = tempfile::Builder::new();
    builder.prefix(".sound-to-patch-").suffix(".partial");
    // The umask applies, as it does to a file that File::create makes.
    #[cfg(unix)]
    builder.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o666));
    let mut written = Vec::with_capacity(outputs.len());
    for &(path, write) in outputs {
        let temporary = builder
            .tempfile_in(folder)
            .and_then(|mut temporary| {
                let mut file = BufWriter::new(&mut temporary);
                write(&mut file)?;
                file.flush()?;
                drop(file);
                temporary.as_file().sync_all()?;
                Ok(temporary)
            })
            .map_err(|cause| failed(path, cause))?;
        written.push((path, temporary));
    }
    let mut placed = Vec::with_capacity(written.len());
    for (path, temporary) in written {
        let persisted = if force {
            temporary.persist(path)
        } else {
            temporary.persist_noclobber(path)
        };
        let Err(error) = persisted else {
            placed.push(path);
            continue;
        };
        // Without force every placed output is new, so removing it restores
        // the folder. shortcut: with force, an output already renamed stays
        // replaced; a rename within one folder fails only when another
        // program changes the folder during the write, because render_file
        // refuses an output that is a folder.
        if !force {
            for path in placed {
                let _ = fs::remove_file(path);
            }
            if error.error.kind() == io::ErrorKind::AlreadyExists {
                return Err(exists(path));
            }
        }
        return Err(failed(path, error.error));
    }
    Ok(())
}

/// The patch the fake backend plays: a dark tone, unlike the fake's init
/// patch.
fn fake_patch() -> FakePatch {
    FakePatch {
        harmonics: vec![1.0, 0.25],
        attack: Seconds::from_const(0.01),
        release: Seconds::from_const(0.3),
    }
}

/// Renders a preset file and its MIDI file, checks the load, and writes the
/// WAV file and the record.
///
/// # Errors
///
/// Returns a [`RenderFileError`] for an input that cannot be read, an
/// output that exists without `force`, a missing plug-in, and a render
/// that fails or does not pass the load check.
pub fn render_file(request: &RenderRequest<'_>) -> Result<RenderReport, RenderFileError> {
    let out = request.out;
    let is_wav = out
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("wav"));
    if !is_wav {
        return Err(RenderFileError::Output {
            path: out.to_owned(),
            cause: "the name does not end in .wav".to_owned(),
            fix: "Name the output file with .wav; the record goes next to it with .json".to_owned(),
        });
    }
    let record_path = out.with_extension("json");
    let folder = out
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if !folder.is_dir() {
        return Err(RenderFileError::Output {
            path: out.to_owned(),
            cause: format!("the folder {} does not exist", folder.display()),
            fix: "Create the folder, or write to one that exists".to_owned(),
        });
    }
    for path in [out, record_path.as_path()] {
        if path.is_dir() {
            return Err(RenderFileError::Output {
                path: path.to_owned(),
                cause: "it is a folder".to_owned(),
                fix: "Write to a file name that no folder has".to_owned(),
            });
        }
        if !request.force && path.exists() {
            return Err(exists(path));
        }
    }
    let preset = std::fs::read(request.preset).map_err(|source| RenderFileError::Read {
        path: request.preset.to_owned(),
        cause: source.to_string(),
    })?;
    let state = preset_state(request.synth, &preset, &super::Recipe::current())?;
    let name = saved_preset(state.bytes())
        .and_then(|root| {
            root.get("preset_name")
                .and_then(|v| v.as_str())
                .map(str::to_owned)
        })
        .filter(|name| !name.is_empty());
    let job = RenderJob::from_midi(state, request.midi, request.bpm)?;
    let (mut renderer, plugin): (Box<dyn Renderer>, Option<PathBuf>) = match request.backend {
        Backend::Fake => (
            Box::new(FakeRenderer::new(request.synth, 0).with_patch(fake_patch())),
            None,
        ),
        Backend::Plugin => {
            let plugin = match request.plugin {
                Some(path) => path.to_owned(),
                None => {
                    find_plugin(request.synth).ok_or(RenderFileError::NoPlugin(request.synth))?
                }
            };
            let host = host_binary().ok_or_else(|| RenderError::Host {
                cause: "the program cannot find its own folder".to_owned(),
                fix: format!("Set {HOST_ENV} to the path of sound-to-patch-host"),
            })?;
            let renderer = PluginRenderer::start(request.synth, &plugin, &host)?;
            (Box::new(renderer), Some(plugin))
        }
    };
    let (mut output, check) = render_checked(renderer.as_mut(), &job)?;
    output.record.load_check = Some(check);
    let json =
        serde_json::to_string_pretty(&output.record).map_err(|source| RenderFileError::Output {
            path: record_path.clone(),
            cause: source.to_string(),
            fix: "Report it; a record always encodes".to_owned(),
        })?;
    let json = json + "\n";
    write_outputs(
        folder,
        &[
            (out, &|file| write_wav_to(file, &output.audio)),
            (&record_path, &|file| file.write_all(json.as_bytes())),
        ],
        request.force,
    )?;
    Ok(RenderReport {
        plugin,
        name,
        notes: job.notes().len(),
        check,
        record: output.record,
        wav: out.to_owned(),
        record_path,
        tail_cap: job.recipe().tail_cap(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_both(folder: &Path, force: bool) -> Result<(), RenderFileError> {
        write_outputs(
            folder,
            &[
                (&folder.join("clip.wav"), &|file| {
                    file.write_all(b"new clip")
                }),
                (&folder.join("clip.json"), &|_| {
                    Err(io::Error::other("the disk is full"))
                }),
            ],
            force,
        )
    }

    #[test]
    fn a_failed_record_write_leaves_no_new_output() {
        let dir = tempfile::tempdir().unwrap();
        let error = write_both(dir.path(), false).unwrap_err().to_string();
        assert!(error.contains("clip.json: the disk is full"), "{error}");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_failed_record_write_keeps_the_old_outputs() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("clip.wav");
        let record = dir.path().join("clip.json");
        fs::write(&out, "old clip").unwrap();
        fs::write(&record, "old record").unwrap();
        write_both(dir.path(), true).unwrap_err();
        assert_eq!(fs::read(&out).unwrap(), b"old clip");
        assert_eq!(fs::read(&record).unwrap(), b"old record");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}
