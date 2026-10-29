//! The open Vital engine as a [`Renderer`], for single notes in tests and
//! CI with no Vital licence.
//!
//! `vita`, the GPL-3.0 Python binding of the open engine, runs only as a
//! separate process: each request starts `tools/vita/render.py` with a
//! Python that has the package, and no code of this crate links it. The
//! engine renders one note per call and takes no MIDI file, so a job with
//! more than one note, or with controller changes, is refused.
//!
//! Each render starts clean: the engine keeps no sound between calls. The
//! renderer loads the job's state, plays the recipe's warm-up note and
//! throws it away, so smoothed parameters settle, then renders the note.
//! The engine has no call that renders silence, so the recipe's settle
//! steps add to the warm-up render's length. A note that starts after 0 s
//! gets that much silence before it. The clip ends where the recipe's
//! [`super::TailWatch`] ends it.
//!
//! The engine reads no `tuning` from a preset, so a state with a custom
//! tuning is refused rather than rendered in the default tuning. A preset
//! the engine does not load, or samples that are not finite, come back as
//! [`RenderError::State`]; a Python that does not run as
//! [`RenderError::Host`].

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

use super::state::saved_preset;
use super::{
    ClipEnd, HostIdentity, PreRoll, RenderError, RenderJob, RenderNote, RenderRecord, Rendered,
    Renderer, frame_of,
};
use crate::audio::{Audio, SampleRate, decode};
use crate::synth::Synth;

/// The environment variable that names a Python with the `vita` package.
pub const VITA_PYTHON_ENV: &str = "VITA_PYTHON";
/// The tempo a state is saved at before any render: 120 BPM.
const DEFAULT_BPM: f64 = 120.0;
/// The exit code of the script for a refusal of the engine.
const REFUSED: i32 = 3;

/// Plays single notes through the open engine in a Python process.
#[derive(Debug)]
pub struct VitaRenderer {
    python: PathBuf,
    script: PathBuf,
    identity: HostIdentity,
    loaded: Option<Vec<u8>>,
    /// The tempo of the last job, which [`Renderer::save_state`] loads at.
    bpm: f64,
    renders: u64,
    scratch: tempfile::TempDir,
}

impl VitaRenderer {
    /// Starts a renderer that runs `script`, `tools/vita/render.py`, with
    /// `python`.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError::Host`] when the script does not run or names
    /// no `vita` version.
    pub fn start(python: &Path, script: &Path) -> Result<Self, RenderError> {
        let scratch = tempfile::tempdir().map_err(|source| {
            vita_error(format!("cannot make a scratch folder ({source})"), FIX_TEMP)
        })?;
        let version = ask(python, script, &json!({ "op": "version" }))?;
        let field = |key: &str| version[key].as_str().unwrap_or_default().to_owned();
        let vita = field("vita");
        if vita.is_empty() {
            return Err(vita_error(
                format!("the script named no vita version: {version}"),
                &fix_python(),
            ));
        }
        Ok(Self {
            python: python.to_owned(),
            script: script.to_owned(),
            identity: HostIdentity {
                synth: Synth::Vital,
                build: format!("open engine, vita {vita}"),
                host: format!("tools/vita/render.py, Python {}", field("python")),
                arch: field("arch"),
            },
            loaded: None,
            bpm: DEFAULT_BPM,
            renders: 0,
            scratch,
        })
    }

    /// Writes `state` as preset JSON to a scratch file and returns its
    /// path.
    fn state_file(&self, state: &[u8]) -> Result<PathBuf, RenderError> {
        let root = saved_preset(state)
            .ok_or_else(|| RenderError::State("the state holds no preset JSON".to_owned()))?;
        if let Some(tuning) = root.get("tuning")
            && tuning.get("default").and_then(Value::as_bool) != Some(true)
        {
            return Err(RenderError::State(
                "the preset has a custom tuning, and the open engine reads no tuning; render it through the plug-in".to_owned(),
            ));
        }
        let path = self.scratch.path().join("state.json");
        let text = serde_json::to_vec(&Value::Object(root))
            .map_err(|source| RenderError::State(source.to_string()))?;
        std::fs::write(&path, text).map_err(|source| {
            vita_error(
                format!("cannot write {} ({source})", path.display()),
                FIX_TEMP,
            )
        })?;
        Ok(path)
    }

    /// Renders the job's note, with its state or with the engine's init
    /// patch.
    fn play(&mut self, job: &RenderJob, load: bool) -> Result<Rendered, RenderError> {
        let note = single_note(job)?;
        let preset = if load {
            let path = self.state_file(job.state().bytes())?;
            Value::from(path.to_string_lossy().into_owned())
        } else {
            Value::Null
        };
        let recipe = job.recipe();
        let rate = recipe.sample_rate();
        let mut watch = recipe.tail_watch(job.notes());
        let onset = frame_of(note.onset(), rate);
        let last_off = frame_of(note.offset(), rate).max(onset + 1);
        let until =
            (last_off + frame_of(recipe.tail_cap(), rate)).max(frame_of(recipe.clip(), rate));
        self.renders += 1;
        let out = self
            .scratch
            .path()
            .join(format!("clip-{}.wav", self.renders));
        let render_frames = until - onset;
        let reply = ask(
            &self.python,
            &self.script,
            &json!({
                "op": "render",
                "preset": preset,
                "bpm": job.bpm().get(),
                "sample_rate": rate.get(),
                "key": note.key(),
                "velocity": note.velocity().get(),
                "note_frames": last_off - onset,
                "render_frames": render_frames,
                "warm_up": warm_up(recipe.pre_roll(), rate),
                "out": out.to_string_lossy(),
            }),
        )?;
        if load {
            self.loaded = Some(job.state().bytes().to_vec());
        }
        self.bpm = job.bpm().get();
        let decoded = decode(&out).map_err(|source| {
            RenderError::Crashed(format!(
                "the script wrote a clip that does not decode ({source})"
            ))
        });
        let _ = std::fs::remove_file(&out);
        let note_audio = decoded?.into_audio();
        let replied = reply["frames"].as_u64();
        if replied != u64::try_from(render_frames).ok()
            || u64::try_from(note_audio.frames()).ok() != replied
        {
            return Err(RenderError::Crashed(format!(
                "the script was asked for {render_frames} frames, replied {}, and wrote {}",
                reply["frames"],
                note_audio.frames()
            )));
        }
        let mut channels: Vec<Vec<f64>> = note_audio
            .channels()
            .iter()
            .map(|channel| {
                std::iter::repeat_n(0.0, onset)
                    .chain(channel.iter().copied())
                    .collect()
            })
            .collect();
        let frames = channels.first().map_or(0, Vec::len);
        let mut end = ClipEnd {
            frames,
            capped: true,
        };
        let mut frame = Vec::with_capacity(channels.len());
        for index in 0..frames {
            frame.clear();
            frame.extend(channels.iter().map(|channel| channel[index]));
            if let Some(found) = watch.push(&frame)? {
                end = found;
                break;
            }
        }
        for channel in &mut channels {
            channel.truncate(end.frames);
        }
        Ok(Rendered {
            audio: Audio::new(rate, channels)?,
            record: RenderRecord::new(self.identity(), job, end),
        })
    }
}

/// The one note of `job`, or the refusal of a job the engine cannot play:
/// another synth's state, more than one note, or controller changes.
fn single_note(job: &RenderJob) -> Result<&RenderNote, RenderError> {
    if job.state().synth() != Synth::Vital {
        return Err(RenderError::WrongSynth {
            state: job.state().synth(),
            renderer: Synth::Vital,
        });
    }
    let [note] = job.notes() else {
        return Err(RenderError::Note(format!(
            "the open engine plays one note per render, and the job has {}; render a MIDI file with one note, or render through the plug-in",
            job.notes().len()
        )));
    };
    if !job.controls().is_empty() {
        return Err(RenderError::Control(
            "the open engine takes no controller changes; render through the plug-in".to_owned(),
        ));
    }
    Ok(note)
}

/// Runs `script` with `python` on one request and reads its reply.
///
/// An exit of [`REFUSED`] is a refusal of the engine, which comes back as
/// [`RenderError::State`] with the script's message. A Python that does
/// not start or a script that fails otherwise is [`RenderError::Host`].
fn ask(python: &Path, script: &Path, request: &Value) -> Result<Value, RenderError> {
    // shortcut: no time-out on the script, which only tests and CI run;
    // add one if a user-facing command renders through it.
    let mut child = Command::new(python)
        .arg("-I")
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|source| {
            vita_error(
                format!("cannot start {} ({source})", python.display()),
                &fix_python(),
            )
        })?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(request.to_string().as_bytes());
    }
    let output = child.wait_with_output().map_err(|source| {
        RenderError::Crashed(format!("the open-engine script did not finish ({source})"))
    })?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let last = stderr
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .to_owned();
    if output.status.code() == Some(REFUSED) {
        return Err(RenderError::State(last));
    }
    if !output.status.success() {
        return Err(vita_error(
            format!("the open-engine script failed ({}): {last}", output.status),
            &fix_python(),
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|source| {
        RenderError::Crashed(format!(
            "the open-engine script gave a reply that is not JSON ({source})"
        ))
    })
}

/// The warm-up request: the recipe's warm-up note, in a render as long as
/// the whole pre-roll, in frames at `rate`.
fn warm_up(pre_roll: &[PreRoll], rate: SampleRate) -> Value {
    let length: usize = pre_roll
        .iter()
        .map(|step| match step {
            PreRoll::Settle { length } | PreRoll::WarmUp { length, .. } => frame_of(*length, rate),
        })
        .sum();
    pre_roll
        .iter()
        .find_map(|step| match step {
            PreRoll::WarmUp {
                key,
                velocity,
                start,
                end,
                ..
            } => Some(json!({
                "key": key,
                "velocity": velocity,
                "note_frames": frame_of(*end, rate) - frame_of(*start, rate),
                "render_frames": length,
            })),
            PreRoll::Settle { .. } => None,
        })
        .unwrap_or(Value::Null)
}

fn fix_python() -> String {
    format!("Set {VITA_PYTHON_ENV} to a Python with the packages of tools/vita/requirements.txt")
}

const FIX_TEMP: &str = "Check that the temporary folder is writable";

fn vita_error(cause: String, fix: &str) -> RenderError {
    RenderError::Host {
        cause,
        fix: fix.to_owned(),
    }
}

impl Renderer for VitaRenderer {
    fn identity(&self) -> HostIdentity {
        self.identity.clone()
    }

    fn render(&mut self, job: &RenderJob) -> Result<Rendered, RenderError> {
        self.play(job, true)
    }

    fn render_init(&mut self, job: &RenderJob) -> Result<Rendered, RenderError> {
        if self.loaded.is_some() {
            return Err(vita_error(
                "the renderer already loaded a state".to_owned(),
                "Render the init patch before the first render of a preset",
            ));
        }
        self.play(job, false)
    }

    fn save_state(&mut self) -> Result<Vec<u8>, RenderError> {
        let preset = match &self.loaded {
            Some(state) => Value::from(self.state_file(state)?.to_string_lossy().into_owned()),
            None => Value::Null,
        };
        // The script loads the state again at the last job's tempo, as the
        // load check compares the saved tempo with that job.
        let reply = ask(
            &self.python,
            &self.script,
            &json!({
                "op": "save",
                "preset": preset,
                "bpm": self.bpm,
                "sample_rate": super::Recipe::current().sample_rate().get(),
            }),
        )?;
        let mut saved: Value = reply["state"]
            .as_str()
            .and_then(|state| serde_json::from_str(state).ok())
            .ok_or_else(|| RenderError::Crashed(format!("the script saved no state: {reply}")))?;
        // The engine saves no tuning, but it plays the default tuning, the
        // only one a loaded state may hold here.
        let tuning = self
            .loaded
            .as_deref()
            .and_then(saved_preset)
            .and_then(|root| root.get("tuning").cloned());
        if let (Some(tuning), Some(root)) = (tuning, saved.as_object_mut()) {
            root.insert("tuning".to_owned(), tuning);
        }
        serde_json::to_vec(&saved).map_err(|source| RenderError::State(source.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::state::preset_state;
    use crate::render::{Control, ControlKind, Material, Recipe, RenderNote};
    use crate::units::{Bpm, Seconds};

    /// A renderer that never started a script: every refusal comes before
    /// the first request, so none reaches the missing Python.
    fn unstarted() -> VitaRenderer {
        VitaRenderer {
            python: PathBuf::from("no-such-python"),
            script: PathBuf::from("no-such-script.py"),
            identity: HostIdentity {
                synth: Synth::Vital,
                build: String::new(),
                host: String::new(),
                arch: String::new(),
            },
            loaded: None,
            bpm: DEFAULT_BPM,
            renders: 0,
            scratch: tempfile::tempdir().unwrap(),
        }
    }

    fn probe(edit: impl FnOnce(&mut Value)) -> crate::render::PresetState {
        let mut preset: Value =
            serde_json::from_slice(include_bytes!("../../tests/fixtures/vital/minimal.vital"))
                .unwrap();
        edit(&mut preset);
        preset_state(
            Synth::Vital,
            &serde_json::to_vec(&preset).unwrap(),
            &Recipe::current(),
        )
        .unwrap()
    }

    fn bpm() -> Bpm {
        Bpm::new(120.0).unwrap()
    }

    fn note(key: u8) -> RenderNote {
        RenderNote::new(
            key,
            Seconds::new(0.0).unwrap(),
            Seconds::new(0.5).unwrap(),
            crate::notes::Velocity::new(100).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn the_four_refusals_come_before_the_script() {
        let mut renderer = unstarted();
        let vital = probe(|_| {});
        let other = crate::render::PresetState::new(Synth::Serum2, vital.bytes().to_vec());
        let error = renderer
            .render(&RenderJob::for_material(other, Material::Lead, bpm()))
            .unwrap_err();
        assert!(matches!(error, RenderError::WrongSynth { .. }), "{error}");
        let two = RenderJob::new(vital.clone(), vec![note(60), note(64)], bpm()).unwrap();
        let error = renderer.render(&two).unwrap_err();
        assert!(matches!(error, RenderError::Note(_)), "{error}");
        assert!(error.to_string().contains("the job has 2"), "{error}");
        let wheel = RenderJob::new(vital, vec![note(60)], bpm())
            .unwrap()
            .with_controls(vec![Control {
                time: Seconds::new(0.1).unwrap(),
                kind: ControlKind::ModWheel(64),
            }])
            .unwrap();
        let error = renderer.render(&wheel).unwrap_err();
        assert!(matches!(error, RenderError::Control(_)), "{error}");
        let tuned = probe(|preset| {
            preset["tuning"] = json!({
                "scale_start_midi_note": 60, "reference_midi_note": 2, "tuning_name": "",
                "mapping_name": "", "default": false,
                "scale": [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0]
            });
        });
        let error = renderer
            .render(&RenderJob::for_material(tuned, Material::Lead, bpm()))
            .unwrap_err();
        assert!(matches!(error, RenderError::State(_)), "{error}");
        assert!(error.to_string().contains("reads no tuning"), "{error}");
        assert_eq!(renderer.loaded, None);
        assert_eq!(renderer.renders, 0);
    }

    /// `python3`, or `None` with a note when it does not run, so the stub
    /// tests skip on a machine without it.
    fn python3() -> Option<&'static Path> {
        let python = Path::new("python3");
        let runs = Command::new(python)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success());
        if !runs {
            eprintln!(
                "skipped: the stub tests run their scripts with python3, which does not run here"
            );
        }
        runs.then_some(python)
    }

    /// A Python script with `body`, in `dir`.
    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        path
    }

    /// A Python script that prints `message` to standard error and exits
    /// with `code`.
    fn stub(dir: &Path, code: i32, message: &str) -> PathBuf {
        script(
            dir,
            &format!("exit-{code}.py"),
            &format!("import sys\nprint({message:?}, file=sys.stderr)\nsys.exit({code})\n"),
        )
    }

    /// A renderer that runs `script` with `python`, never started.
    fn with_script(python: &Path, script: PathBuf) -> VitaRenderer {
        VitaRenderer {
            python: python.to_owned(),
            script,
            ..unstarted()
        }
    }

    #[test]
    fn a_refusal_of_the_engine_is_a_state_error_and_other_failures_a_host_error() {
        let Some(python) = python3() else { return };
        let dir = tempfile::tempdir().unwrap();
        let request = json!({ "op": "render" });
        // The literal exit code of tools/vita/render.py, not the constant.
        let refused = ask(python, &stub(dir.path(), 3, "not loaded"), &request);
        assert!(
            matches!(&refused, Err(RenderError::State(message)) if message == "not loaded"),
            "{refused:?}"
        );
        let failed = ask(python, &stub(dir.path(), 1, "no module"), &request);
        assert!(
            matches!(&failed, Err(RenderError::Host { fix, .. }) if fix.contains(VITA_PYTHON_ENV)),
            "{failed:?}"
        );
        let missing = ask(Path::new("no-such-python"), Path::new("x.py"), &request);
        assert!(
            matches!(&missing, Err(RenderError::Host { cause, .. }) if cause.starts_with("cannot start")),
            "{missing:?}"
        );
    }

    #[test]
    fn a_refused_load_leaves_no_loaded_state_and_no_tempo() {
        let Some(python) = python3() else { return };
        let dir = tempfile::tempdir().unwrap();
        let mut renderer = with_script(python, stub(dir.path(), 3, "not loaded"));
        let job = RenderJob::for_material(probe(|_| {}), Material::Lead, Bpm::new(140.0).unwrap());
        let error = renderer.render(&job).unwrap_err();
        assert!(matches!(error, RenderError::State(_)), "{error}");
        assert_eq!(renderer.loaded, None);
        assert_eq!(renderer.bpm, DEFAULT_BPM);
    }

    #[test]
    fn a_clip_of_another_length_than_asked_is_a_crash() {
        let Some(python) = python3() else { return };
        let dir = tempfile::tempdir().unwrap();
        // Writes a clip of 10 stereo frames and says so, whatever it was
        // asked for.
        let short = script(
            dir.path(),
            "short.py",
            "import json, sys, wave\n\
             request = json.load(sys.stdin)\n\
             with wave.open(request['out'], 'wb') as out:\n\
             \x20   out.setnchannels(2)\n\
             \x20   out.setsampwidth(2)\n\
             \x20   out.setframerate(request['sample_rate'])\n\
             \x20   out.writeframes(bytes(40))\n\
             print(json.dumps({'frames': 10}))\n",
        );
        let mut renderer = with_script(python, short);
        let job = RenderJob::for_material(probe(|_| {}), Material::Lead, bpm());
        let error = renderer.render(&job).unwrap_err();
        assert!(
            matches!(&error, RenderError::Crashed(cause) if cause.contains("replied 10, and wrote 10")),
            "{error}"
        );
    }
}
