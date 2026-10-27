//! Command-line cases, declared as files in `tests/cmd/`.

#[test]
fn cli_cases() {
    trycmd::TestCases::new()
        .case("tests/cmd/*.toml")
        .case("README.md")
        .case("docs/src/guide/preset.md")
        .case("docs/src/reference/cli.md");
}

/// A failed write of the output fails the command. `/dev/full` exists on
/// Linux only; the `test-linux` job runs this.
#[cfg(target_os = "linux")]
#[test]
fn a_failed_output_write_fails_the_command() {
    let full = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sound-to-patch"))
        .args(["preset", "inspect", "tests/fixtures/vital/minimal.vital"])
        .stdout(full)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.starts_with("error: cannot write the output"),
        "{stderr}"
    );
}

/// A reader that closes the pipe early does not fail the command. The
/// pipe closes while the command still starts, before it writes.
#[test]
fn a_closed_pipe_is_not_an_error() {
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_sound-to-patch"))
        .args([
            "preset",
            "inspect",
            "--full",
            "tests/fixtures/vital/minimal.vital",
        ])
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    assert!(child.wait().unwrap().success());
}

/// The success case of `analyze`, on a generated WAV in a temporary
/// folder, which a trycmd case cannot name.
#[test]
fn analyze_writes_both_files_for_a_generated_wav() {
    let dir = tempfile::tempdir().unwrap();
    let path = crate::report::stem(dir.path());
    let out = dir.path().join("cli");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sound-to-patch"))
        .arg("analyze")
        .arg(&path)
        .args(["--bpm", "120", "--out"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout,
        format!(
            "wrote {}\nwrote {}\n",
            out.join("layers.json").display(),
            out.join("spectrogram.png").display()
        )
    );
    assert!(out.join("spectrogram.png").is_file());
}

/// A packet that becomes silence is a warning on standard error; standard
/// output keeps only the written paths.
#[test]
fn analyze_warns_about_corrupt_packets_on_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("glitch.mp3");
    crate::decode::mp3_with_bad_frames(&path, 100, &[5]);
    let out = dir.path().join("out");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sound-to-patch"))
        .arg("analyze")
        .arg(&path)
        .args(["--bpm", "120", "--out"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stdout.lines().all(|line| line.starts_with("wrote ")),
        "{stdout}"
    );
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(stderr.starts_with("warning: packet 5 "), "{stderr}");
}

/// Three dry saw notes above 110 Hz, A3, C4 and E4, at 120 BPM: 0.2 s on
/// each eighth note from the second.
fn saw_notes(dir: &std::path::Path) -> (std::path::PathBuf, [(f64, f64); 3]) {
    let rate = 22_050.0;
    let notes = [(0.25, 57.0), (0.5, 60.0), (0.75, 64.0)];
    let len = crate::support::cast::round_to_usize(1.2 * rate);
    let t = crate::support::signals::times(rate, len);
    let mut mono = vec![0.0; len];
    for (onset, pitch) in notes {
        let f0 = 440.0 * 2_f64.powf((pitch - 69.0) / 12.0);
        let saw = crate::support::signals::bandlimited_saw(f0, rate, &t, |_| 0.0);
        for ((sample, value), time) in mono.iter_mut().zip(saw).zip(&t) {
            if (onset..onset + 0.2).contains(time) {
                *sample += 0.3 * value;
            }
        }
    }
    let path = dir.join("lead.wav");
    crate::support::signals::write_wav16(&path, 22_050, &[mono]);
    (path, notes)
}

/// The success case of `notes`: the file re-reads with the exact notes and
/// the BPM, and the output names each step.
#[test]
fn notes_writes_the_notes_of_a_generated_wav() {
    let dir = tempfile::tempdir().unwrap();
    let (path, truth) = saw_notes(dir.path());
    let out = dir.path().join("cli");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sound-to-patch"))
        .arg("notes")
        .arg(&path)
        .args(["--bpm", "120", "--out"])
        .arg(&out)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let file = out.join("notes.mid");
    assert!(
        stdout.ends_with(&format!("wrote {}\n", file.display())),
        "{stdout}"
    );
    for step in [
        "detected range: ",
        "detected voicing: monophonic",
        "register window: 20 dB",
        "settings: detected, ",
        "decoded: ",
        "octave check: ",
        "ghost merge: ",
        "split join: ",
        "quiet-note guard: 3 notes, 0 dropped below 40 dB",
    ] {
        assert!(stdout.contains(step), "no `{step}` in {stdout}");
    }
    let bpm = sound_to_patch::units::Bpm::new(120.0).unwrap();
    let notes = sound_to_patch::notes::read_midi(&file, bpm).unwrap();
    assert_eq!(notes.notes().len(), truth.len(), "{notes:?}");
    for (note, (onset, pitch)) in notes.notes().iter().zip(truth) {
        assert_eq!(note.pitch().get(), pitch);
        assert!((note.onset().get() - onset).abs() <= 0.05, "{note:?}");
    }
}

/// A silent stem gives no note: exit 1 with the cause and the fix, and
/// nothing written.
#[test]
fn notes_on_a_silent_stem_fails_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("silence.wav");
    crate::support::signals::write_wav16(&path, 22_050, &[vec![0.0; 22_050]]);
    let out = dir.path().join("cli");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sound-to-patch"))
        .arg("notes")
        .arg(&path)
        .args(["--bpm", "120", "--out"])
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(
        stderr,
        "error: `silence.wav` gave no note, so nothing was written: it is silent, or no note sounds within 40 dB of its loudest passage. Check the stem, or pass --profile bass|lead|pad\n"
    );
    assert!(!out.exists());
}
