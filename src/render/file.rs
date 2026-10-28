//! The `render` command: a preset and a MIDI file to a WAV file and a
//! render record, through the load check.

use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use super::check::{LoadCheck, LoadFault, render_checked};
use super::fake::{FakePatch, FakeRenderer};
use super::plugin::{HOST_ENV, PluginRenderer, find_plugin, host_binary};
use super::state::{preset_state, saved_preset};
use super::{HostIdentity, RenderError, RenderJob, RenderRecord, Renderer};
use crate::audio::write_wav;
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
        if !request.force && path.exists() {
            return Err(RenderFileError::Output {
                path: path.to_owned(),
                cause: "the file exists".to_owned(),
                fix: "Pass --force to replace it, or write to a new file".to_owned(),
            });
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
    // A partial WAV file, or one without its record, would block the next
    // run, so a failed write removes it.
    if let Err(source) = write_wav(out, &output.audio) {
        let _ = std::fs::remove_file(out);
        return Err(RenderFileError::Output {
            path: out.to_owned(),
            cause: source.to_string(),
            fix: "Check that the folder is writable and the disk has room".to_owned(),
        });
    }
    if let Err(source) = std::fs::write(&record_path, json + "\n") {
        let _ = std::fs::remove_file(out);
        return Err(RenderFileError::Output {
            path: record_path,
            cause: source.to_string(),
            fix: "Check that the folder is writable and the disk has room".to_owned(),
        });
    }
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
