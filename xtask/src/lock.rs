//! A check of the lock file: the ONNX Runtime binding of the transcription
//! crate stays out of the build. It would pin an old release candidate and
//! link a native library.

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
    fn the_workspace_lock_has_no_ort() {
        check_no_ort(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    }
}
