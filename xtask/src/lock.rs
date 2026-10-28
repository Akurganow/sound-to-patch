//! Checks of the lock files. The ONNX Runtime binding of the transcription
//! crate stays out of the build: it would pin an old release candidate and
//! link a native library. The fuzz crate's lock names the crate version of
//! the root lock, so a fuzz build after a release rewrites nothing.

use std::fs;
use std::path::Path;

/// Fails when `Cargo.lock` holds the `ort` crate or one of its `ort-*`
/// companions.
pub(crate) fn check_no_ort(root: &Path) -> crate::Result<()> {
    let lock = fs::read_to_string(root.join("Cargo.lock"))?;
    let found = ort_packages(&lock);
    if found.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Cargo.lock holds {}: a dependency enabled the `ort` features of basic-pitch-rs. Keep its features to `tract` and `bundled-model`",
            found.join(", ")
        )
        .into())
    }
}

/// Fails when `fuzz/Cargo.lock` and `Cargo.lock` name different versions of
/// the `sound-to-patch` crate.
pub(crate) fn check_fuzz_lock(root: &Path) -> crate::Result<()> {
    let version = |path: &str| -> crate::Result<String> {
        let lock = fs::read_to_string(root.join(path))?;
        crate_version(&lock)
            .map(str::to_owned)
            .ok_or_else(|| format!("{path} has no `sound-to-patch` package").into())
    };
    let (main, fuzz) = (version("Cargo.lock")?, version("fuzz/Cargo.lock")?);
    if main == fuzz {
        Ok(())
    } else {
        Err(format!(
            "Cargo.lock names sound-to-patch {main}, but fuzz/Cargo.lock names {fuzz}. Run `cargo update --manifest-path fuzz/Cargo.toml -p sound-to-patch`"
        )
        .into())
    }
}

/// The `version` line that follows `name = "sound-to-patch"`.
fn crate_version(lock: &str) -> Option<&str> {
    let mut lines = lock.lines();
    lines.find(|line| *line == "name = \"sound-to-patch\"")?;
    lines
        .next()?
        .strip_prefix("version = \"")?
        .strip_suffix('"')
}

fn ort_packages(lock: &str) -> Vec<&str> {
    lock.lines()
        .filter_map(|line| line.strip_prefix("name = \"")?.strip_suffix('"'))
        .filter(|name| *name == "ort" || name.starts_with("ort-"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_ort_and_its_companions_count() {
        let lock = "[[package]]\nname = \"ort\"\n\n[[package]]\nname = \"ort-sys\"\n\n[[package]]\nname = \"sort\"\n\n[[package]]\nname = \"tract-onnx\"\ndependencies = [\n \"ort\",\n]\n";
        assert_eq!(ort_packages(lock), vec!["ort", "ort-sys"]);
    }

    #[test]
    fn the_crate_version_follows_its_name() {
        let lock = "[[package]]\nname = \"sound-to-patch-fuzz\"\nversion = \"0.0.0\"\n\n\
                    [[package]]\nname = \"sound-to-patch\"\nversion = \"0.15.0\"\n";
        assert_eq!(crate_version(lock), Some("0.15.0"));
        assert_eq!(crate_version("[[package]]\nname = \"other\"\n"), None);
    }

    #[test]
    fn a_stale_fuzz_lock_names_both_versions() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("fuzz")).unwrap();
        let lock = |version: &str| {
            format!("[[package]]\nname = \"sound-to-patch\"\nversion = \"{version}\"\n")
        };
        fs::write(dir.path().join("Cargo.lock"), lock("0.15.0")).unwrap();
        fs::write(dir.path().join("fuzz/Cargo.lock"), lock("0.12.0")).unwrap();
        let err = check_fuzz_lock(dir.path()).unwrap_err().to_string();
        assert!(err.contains("0.15.0") && err.contains("0.12.0"), "{err}");
        fs::write(dir.path().join("fuzz/Cargo.lock"), lock("0.15.0")).unwrap();
        check_fuzz_lock(dir.path()).unwrap();
    }

    #[test]
    fn the_fuzz_lock_names_the_workspace_version() {
        check_fuzz_lock(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    }

    #[test]
    fn the_workspace_lock_has_no_ort() {
        check_no_ort(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    }
}
