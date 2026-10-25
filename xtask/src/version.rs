//! Writes the release version. `cog bump` calls this as a pre-bump hook, so
//! the version commit carries the new number in `Cargo.toml` and
//! `Cargo.lock`, and the released binary prints it.

use std::fs;
use std::path::Path;
use std::process::Command;

/// Sets `version` in `[workspace.package]` of `root/Cargo.toml`, then
/// refreshes the workspace entries of `Cargo.lock`.
pub(crate) fn set(root: &Path, version: &str) -> crate::Result<()> {
    let manifest = root.join("Cargo.toml");
    let text = fs::read_to_string(&manifest)?;
    fs::write(&manifest, with_version(&text, version)?)?;
    let status = Command::new(env!("CARGO"))
        .args(["update", "--workspace", "--offline"])
        .current_dir(root)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo update --workspace failed with {status}").into())
    }
}

/// Replaces the `version` line of `[workspace.package]`.
fn with_version(manifest: &str, version: &str) -> crate::Result<String> {
    let valid = !version.is_empty()
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
        && version.split('.').count() == 3;
    if !valid {
        return Err(format!("`{version}` is not a version: give MAJOR.MINOR.PATCH").into());
    }
    let mut in_section = false;
    let mut replaced = false;
    let mut out = String::with_capacity(manifest.len());
    for line in manifest.lines() {
        if line.starts_with('[') {
            in_section = line.trim() == "[workspace.package]";
        }
        if in_section && !replaced && line.starts_with("version = ") {
            out.push_str("version = \"");
            out.push_str(version);
            out.push('"');
            replaced = true;
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    if replaced {
        Ok(out)
    } else {
        Err("Cargo.toml has no `version = ` line in [workspace.package]".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = "[package]\nversion.workspace = true\n\n\
        [workspace.package]\n# note\nversion = \"0.0.0\"\nedition = \"2024\"\n";

    #[test]
    fn replaces_only_the_workspace_version() {
        let out = with_version(MANIFEST, "0.1.0").unwrap();
        assert_eq!(out, MANIFEST.replace("\"0.0.0\"", "\"0.1.0\""));
    }

    #[test]
    fn rejects_a_prefixed_or_partial_version() {
        for bad in ["v0.1.0", "0.1", "0.1.x", ""] {
            assert!(with_version(MANIFEST, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn fails_without_a_workspace_version() {
        assert!(with_version("[package]\nversion = \"1.0.0\"\n", "0.1.0").is_err());
    }
}
