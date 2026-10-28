//! The `render` command's file handling (#252) on the fake backend: the
//! output name, an existing output, a missing folder and a record that
//! cannot be written.

use std::path::{Path, PathBuf};

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
fn a_record_that_cannot_be_written_removes_the_clip() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("clip.wav");
    std::fs::create_dir(dir.path().join("clip.json")).unwrap();
    let error = message(render(&out, true));
    assert!(error.contains("clip.json"), "{error}");
    assert!(!out.exists());
}

#[test]
fn a_wav_file_that_cannot_be_written_leaves_no_record() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("clip.wav");
    std::fs::create_dir(&out).unwrap();
    let error = message(render(&out, true));
    assert!(error.contains("clip.wav"), "{error}");
    assert!(error.contains("the disk has room"), "{error}");
    assert!(!dir.path().join("clip.json").exists());
}
