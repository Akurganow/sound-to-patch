//! The man pages and the shell completions, rendered from the command tree in
//! `sound_to_patch::cli`.

use std::fs;
use std::path::Path;

use clap::CommandFactory;
use clap_complete::Shell;
use sound_to_patch::cli::Cli;

/// The shells a macOS user is likely to run.
const SHELLS: [Shell; 3] = [Shell::Bash, Shell::Zsh, Shell::Fish];

/// Writes `man/sound-to-patch.1`, one `man/sound-to-patch-<subcommand>.1`
/// per subcommand, and one completion file per shell into
/// `out_dir/completions/`.
pub(crate) fn write(out_dir: &Path) -> crate::Result<()> {
    let man_dir = out_dir.join("man");
    let completions_dir = out_dir.join("completions");
    fs::create_dir_all(&man_dir)?;
    fs::create_dir_all(&completions_dir)?;

    let mut cmd = Cli::command();
    // The top page lists each subcommand as `sound-to-patch-<name>(1)` and
    // no option of it, so every subcommand needs a page of its own.
    clap_mangen::generate_to(cmd.clone(), &man_dir)?;

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

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    /// The page names of a command and of its visible subcommands, the way
    /// `man` looks them up.
    fn pages(cmd: &clap::Command, prefix: &str, out: &mut Vec<String>) {
        let name = if prefix.is_empty() {
            cmd.get_name().to_owned()
        } else {
            format!("{prefix}-{}", cmd.get_name())
        };
        for sub in cmd.get_subcommands().filter(|sub| !sub.is_hide_set()) {
            if sub.get_name() != "help" {
                pages(sub, &name, out);
            }
        }
        out.push(format!("{name}.1"));
    }

    #[test]
    fn writes_the_man_pages_and_three_completions() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path()).unwrap();
        assert_eq!(
            names(&dir.path().join("completions")),
            [
                "_sound-to-patch",
                "sound-to-patch.bash",
                "sound-to-patch.fish"
            ]
        );
        let mut expected = Vec::new();
        pages(&Cli::command(), "", &mut expected);
        expected.sort();
        assert!(expected.len() > 1, "{expected:?}");
        assert_eq!(names(&dir.path().join("man")), expected);
    }

    #[test]
    fn every_subcommand_page_lists_its_options() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path()).unwrap();
        let page = fs::read_to_string(dir.path().join("man/sound-to-patch-analyze.1")).unwrap();
        for option in ["bpm", "notes", "profile"] {
            assert!(page.contains(&format!("\\-\\-{option}")), "{option}");
        }
    }

    // macOS ships mandoc and a `man` without `-l`; Linux ships man-db.
    #[test]
    fn man_pages_render_without_errors() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path()).unwrap();
        for name in names(&dir.path().join("man")) {
            render(&dir.path().join("man").join(name));
        }
    }

    fn render(page: &Path) {
        let output = match Command::new("mandoc")
            .args(["-T", "utf8", "-W", "error"])
            .arg(page)
            .output()
        {
            Err(err) if err.kind() == ErrorKind::NotFound => {
                Command::new("man").arg("-l").arg(page).output()
            }
            other => other,
        }
        .expect("needs mandoc or man-db to render the man pages");
        assert!(output.status.success(), "{output:?}");
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(text.contains("sound-to-patch"), "{text}");
    }
}
