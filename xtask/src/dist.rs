//! The man page and the shell completions, rendered from the command tree in
//! `sound_to_patch::cli`.

use std::fs;
use std::path::Path;

use clap::CommandFactory;
use clap_complete::Shell;
use sound_to_patch::cli::Cli;

/// The shells a macOS user is likely to run.
const SHELLS: [Shell; 3] = [Shell::Bash, Shell::Zsh, Shell::Fish];

/// Writes `man/sound-to-patch.1` and one completion file per shell into
/// `out_dir/completions/`.
pub(crate) fn write(out_dir: &Path) -> crate::Result<()> {
    let man_dir = out_dir.join("man");
    let completions_dir = out_dir.join("completions");
    fs::create_dir_all(&man_dir)?;
    fs::create_dir_all(&completions_dir)?;

    let mut cmd = Cli::command();
    let mut page = Vec::new();
    clap_mangen::Man::new(cmd.clone()).render(&mut page)?;
    fs::write(man_dir.join(format!("{}.1", cmd.get_name())), page)?;

    let name = cmd.get_name().to_owned();
    for shell in SHELLS {
        clap_complete::generate_to(shell, &mut cmd, &name, &completions_dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;
    use std::process::Command;

    use super::*;

    #[test]
    fn writes_the_man_page_and_three_completions() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path()).unwrap();
        let mut files: Vec<_> = ["man", "completions"]
            .iter()
            .flat_map(|sub| fs::read_dir(dir.path().join(sub)).unwrap())
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        files.sort();
        assert_eq!(
            files,
            [
                "_sound-to-patch",
                "sound-to-patch.1",
                "sound-to-patch.bash",
                "sound-to-patch.fish"
            ]
        );
    }

    // macOS ships mandoc and a `man` without `-l`; Linux ships man-db.
    #[test]
    fn man_page_renders_without_errors() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path()).unwrap();
        let page = dir.path().join("man/sound-to-patch.1");
        let output = match Command::new("mandoc")
            .args(["-T", "utf8", "-W", "error"])
            .arg(&page)
            .output()
        {
            Err(err) if err.kind() == ErrorKind::NotFound => {
                Command::new("man").arg("-l").arg(&page).output()
            }
            other => other,
        }
        .expect("needs mandoc or man-db to render the man page");
        assert!(output.status.success(), "{output:?}");
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("sound-to-patch"), "{text}");
    }
}
