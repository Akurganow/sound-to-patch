//! The `render` command's file handling (#252) on the fake backend: the
//! output name, an existing output, a missing folder, a folder in place of
//! an output, and writes that fail.

use std::path::{Path, PathBuf};

use serde_json::Value;

use sound_to_patch::render::file::{Backend, RenderFileError, RenderRequest, render_file};
use sound_to_patch::synth::Synth;
use sound_to_patch::units::Bpm;

fn render(out: &Path, force: bool) -> Result<PathBuf, RenderFileError> {
    render_file(&RenderRequest {
        preset: Path::new("tests/fixtures/vital/routes.vital"),
        midi: Path::new("tests/cmd/data/three-notes.mid"),
        bpm: Bpm::new(120.0).unwrap(),
        out,
        synth: Synth::Vital,
        plugin: None,
        force,
        backend: Backend::Fake,
    })
    .map(|report| report.record_path)
}

fn message(result: Result<PathBuf, RenderFileError>) -> String {
    result.unwrap_err().to_string()
}

#[test]
fn the_output_must_end_in_wav_in_any_case() {
    let dir = tempfile::tempdir().unwrap();
    let error = message(render(&dir.path().join("clip.mp3"), false));
    assert!(error.contains("does not end in .wav"), "{error}");
    let record = render(&dir.path().join("clip.WAV"), false).unwrap();
    assert_eq!(record, dir.path().join("clip.json"));
}

#[test]
fn an_existing_output_needs_force() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("clip.wav");
    render(&out, false).unwrap();
    let error = message(render(&out, false));
    assert!(error.contains("the file exists"), "{error}");
    assert!(error.contains("Pass --force"), "{error}");
    std::fs::remove_file(&out).unwrap();
    let error = message(render(&out, false));
    assert!(error.contains("clip.json"), "{error}");
    render(&out, true).unwrap();
}

#[test]
fn a_missing_folder_is_refused_before_the_render() {
    let dir = tempfile::tempdir().unwrap();
    let error = message(render(&dir.path().join("gone").join("clip.wav"), false));
    assert!(error.contains("does not exist"), "{error}");
    assert!(error.contains("Create the folder"), "{error}");
}

#[test]
fn a_folder_in_place_of_an_output_is_refused_before_the_render() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("clip.wav");
    std::fs::create_dir(&out).unwrap();
    let error = message(render(&out, true));
    assert!(error.contains("clip.wav: it is a folder"), "{error}");
    assert!(!dir.path().join("clip.json").exists());
    std::fs::remove_dir(&out).unwrap();
    std::fs::create_dir(dir.path().join("clip.json")).unwrap();
    let error = message(render(&out, true));
    assert!(error.contains("clip.json: it is a folder"), "{error}");
    assert!(!out.exists());
}

#[test]
fn force_replaces_a_read_only_wav_file() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("clip.wav");
    std::fs::write(&out, "old").unwrap();
    let mut permissions = std::fs::metadata(&out).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&out, permissions).unwrap();
    render(&out, true).unwrap();
    assert!(std::fs::read(&out).unwrap().starts_with(b"RIFF"));
    assert!(dir.path().join("clip.json").exists());
}

#[cfg(unix)]
#[test]
fn a_failed_write_keeps_the_old_outputs() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("clip.wav");
    let record = dir.path().join("clip.json");
    std::fs::write(&out, "old clip").unwrap();
    std::fs::write(&record, "old record").unwrap();
    let mode = |mode| std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(dir.path(), mode(0o555)).unwrap();
    let result = render(&out, true);
    std::fs::set_permissions(dir.path(), mode(0o755)).unwrap();
    let error = message(result);
    assert!(error.contains("clip.wav"), "{error}");
    assert!(error.contains("the disk has room"), "{error}");
    assert_eq!(std::fs::read(&out).unwrap(), b"old clip");
    assert_eq!(std::fs::read(&record).unwrap(), b"old record");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
}

/// Clears the immutable flag of a file when it drops, so the temporary
/// folder can be removed even when an assert fails.
#[cfg(target_os = "macos")]
struct Immutable(PathBuf);

#[cfg(target_os = "macos")]
impl Immutable {
    fn set(path: &Path) -> Self {
        chflags("uchg", path);
        Self(path.to_owned())
    }
}

#[cfg(target_os = "macos")]
impl Drop for Immutable {
    fn drop(&mut self) {
        chflags("nouchg", &self.0);
    }
}

#[cfg(target_os = "macos")]
fn chflags(flag: &str, path: &Path) {
    let status = std::process::Command::new("chflags")
        .arg(flag)
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success(), "chflags {flag} failed");
}

#[cfg(target_os = "macos")]
#[test]
fn a_forced_write_that_cannot_replace_the_record_names_the_mixed_pair() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("clip.wav");
    let record = dir.path().join("clip.json");
    render(&out, false).unwrap();
    let old_record = std::fs::read(&record).unwrap();
    let _immutable = Immutable::set(&record);
    let error = message(render(&out, true));
    assert!(error.contains("clip.json"), "{error}");
    assert!(
        error.contains("clip.wav now holds the new render"),
        "{error}"
    );
    assert!(
        error.contains("clip.json still holds the old one"),
        "{error}"
    );
    assert!(error.contains("no longer match"), "{error}");
    assert!(error.contains("render again with --force"), "{error}");
    assert_eq!(std::fs::read(&record).unwrap(), old_record);
}

#[test]
fn the_written_record_matches_the_committed_schema_with_its_load_check() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("clip.wav");
    let path = render(&out, false).unwrap();
    let record: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    assert!(record["load_check"].is_object(), "{record}");
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string("schemas/render-record.schema.json").unwrap(),
    )
    .unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let errors: Vec<String> = validator
        .iter_errors(&record)
        .map(|error| error.to_string())
        .collect();
    assert_eq!(errors, Vec::<String>::new(), "{record}");
}
