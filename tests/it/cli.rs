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
/// folder, which a trycmd case cannot name: the tempo line, the pitch
/// lines, then both written files. The notes come from a file, so the
/// model does not run; the report tests check the notes-step lines. The f0
/// track still runs for the pitch layer.
#[test]
fn analyze_writes_both_files_for_a_generated_wav() {
    let dir = tempfile::tempdir().unwrap();
    // A 0.3 s saw keeps the debug f0 track short.
    let rate = 22_050.0;
    let t = crate::support::signals::times(rate, crate::support::cast::round_to_usize(0.3 * rate));
    let saw = crate::support::signals::bandlimited_saw(220.0, rate, &t, |_| 0.0);
    let path = dir.path().join("lead.wav");
    crate::support::signals::write_wav16(&path, 22_050, &[saw.iter().map(|x| 0.3 * x).collect()]);
    let notes = crate::report::given(dir.path());
    let out = dir.path().join("cli");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sound-to-patch"))
        .arg("analyze")
        .arg(&path)
        .args(["--bpm", "120", "--notes"])
        .arg(&notes)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    // 0.3 s at 120 BPM is shorter than a beat; `report.rs` and `tempo.rs`
    // check the tempo lines of longer stems. The given A1 starts at
    // 0.25 s under the held saw: the band envelope rises at the stem's
    // cut end, a candidate strike, but the level does not rise as a drop
    // there needs.
    assert_eq!(
        stdout,
        format!(
            "tempo: 120 BPM as given; the stem is shorter than one beat\n\
             glides: 0, 0 too short\n\
             pitch drops: 0 of 1 low strikes\n\
             drop skips: 1 no strike\n\
             vibrato: 0 of 0 steady spans\n\
             envelope: 1 of 1 notes read, 1 pooled, 0 with a note-off knee\n\
             envelope: attack none, hold 0 ms, decay none, sustain 0.0 dB\n\
             pump: too few sustained beats\n\
             wrote {}\nwrote {}\n",
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
    let notes = crate::report::given(dir.path());
    let out = dir.path().join("out");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sound-to-patch"))
        .arg("analyze")
        .arg(&path)
        .args(["--bpm", "120", "--notes"])
        .arg(&notes)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stdout.lines().all(|line| !line.contains("packet")),
        "{stdout}"
    );
    assert!(
        stdout.ends_with(&format!(
            "wrote {}\n",
            out.join("spectrogram.png").display()
        )),
        "{stdout}"
    );
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(stderr.starts_with("warning: packet 5 "), "{stderr}");
}

/// Three dry saw notes above 110 Hz, A3, C4 and E4, at 120 BPM: 0.2 s on
/// each eighth note from the second.
pub(crate) fn saw_notes(dir: &std::path::Path) -> (std::path::PathBuf, [(f64, f64); 3]) {
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
        "cleanup voicing: monophonic, as detected",
        "settings: detected, ",
        "decoded: ",
        "octave check: ",
        "ghost merge: ",
        "attack split: ",
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

/// A 55 Hz saw gated with 30 ms gaps: the decoder reads 9 notes for 16, and
/// the output counts the 16 from the attack split on.
#[test]
fn notes_prints_the_count_after_the_attack_split() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("bass.wav");
    let stem = crate::cleanup::gated(&[55.0], 0.005, 0.22, 0.0, -400.0);
    crate::support::signals::write_wav16(&path, 22_050, &[stem]);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sound-to-patch"))
        .arg("notes")
        .arg(&path)
        .args(["--bpm", "120", "--bpm-exact", "--profile", "bass", "--out"])
        .arg(dir.path().join("cli"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    for step in [
        "ghost merge: 9 notes",
        "attack split: 16 notes",
        "split join: 16 notes",
    ] {
        assert!(stdout.contains(step), "no `{step}` in {stdout}");
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

/// Runs `notes` on `stem` into `out` with extra flags.
#[cfg(test)]
fn notes_command(
    stem: &std::path::Path,
    out: &std::path::Path,
    flags: &[&str],
) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_sound-to-patch"))
        .arg("notes")
        .arg(stem)
        .args(["--bpm", "120", "--out"])
        .arg(out)
        .args(flags)
        .output()
        .unwrap()
}

/// `notes.mid` names its stem. A run of another stem into the folder
/// stops before it decodes anything, and so does a report of another
/// stem, each with its own fix.
#[test]
fn notes_refuses_the_files_of_another_stem() {
    use sound_to_patch::notes::{DEFAULT_VELOCITY, Note, NoteList, Velocities, write_named_midi};
    use sound_to_patch::units::{Bpm, MidiPitch, Seconds};
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let other = NoteList::new(vec![
        Note::new(
            Seconds::new(0.0).unwrap(),
            Seconds::new(0.5).unwrap(),
            MidiPitch::new(45.0).unwrap(),
        )
        .unwrap(),
    ]);
    let bytes = write_named_midi(
        &other,
        Bpm::new(120.0).unwrap(),
        Velocities::Constant(DEFAULT_VELOCITY),
        "other.wav",
    )
    .unwrap();
    std::fs::write(out.join("notes.mid"), &bytes).unwrap();
    // No stem file exists: the guard answers first.
    let stem = dir.path().join("lead.wav");
    let output = notes_command(&stem, &out, &[]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        format!(
            "error: {} holds the notes of `other.wav`, not of `lead.wav`. Pass --out <dir> for this stem, or --force to replace it\n",
            out.join("notes.mid").display()
        )
    );
    assert_eq!(std::fs::read(out.join("notes.mid")).unwrap(), bytes);
    std::fs::write(out.join("layers.json"), r#"{"source": "other.wav"}"#).unwrap();
    let output = notes_command(&stem, &out, &[]);
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        format!(
            "error: {} is the report of `other.wav`, not of `lead.wav`. Pass --out <dir> for this stem, or --force to write notes.mid next to it\n",
            out.join("layers.json").display()
        )
    );
}

/// `--force` replaces the notes of another stem, and `--profile` sets the
/// settings and the cleanup voicing while detection still prints.
#[test]
fn notes_with_a_profile_and_force_replaces_the_notes_of_another_stem() {
    let dir = tempfile::tempdir().unwrap();
    let (path, truth) = saw_notes(dir.path());
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    std::fs::write(out.join("layers.json"), r#"{"source": "other.wav"}"#).unwrap();
    std::fs::write(out.join("notes.mid"), b"not ours").unwrap();
    let output = notes_command(&path, &out, &["--profile", "lead", "--force"]);
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    for line in [
        "detected voicing: monophonic",
        "cleanup voicing: monophonic, from --profile lead",
        "settings: profile lead, MIDI 36 to 95,",
    ] {
        assert!(stdout.contains(line), "no `{line}` in {stdout}");
    }
    let bytes = std::fs::read(out.join("notes.mid")).unwrap();
    assert_eq!(
        sound_to_patch::notes::midi_track_name(&bytes).as_deref(),
        Some("lead.wav")
    );
    let notes = sound_to_patch::notes::read_midi(
        &out.join("notes.mid"),
        sound_to_patch::units::Bpm::new(120.0).unwrap(),
    )
    .unwrap();
    assert_eq!(notes.notes().len(), truth.len());
    // The report of the other stem stays: `notes` writes no report.
    assert_eq!(
        std::fs::read_to_string(out.join("layers.json")).unwrap(),
        r#"{"source": "other.wav"}"#
    );
}
