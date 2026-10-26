//! Command-line cases, declared as files in `tests/cmd/`.

#[test]
fn cli_cases() {
    trycmd::TestCases::new().case("tests/cmd/*.toml");
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
