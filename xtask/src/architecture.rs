//! Keeps the code map in `ARCHITECTURE.md` from losing a module.
//!
//! The check reads names only. The descriptions get a manual review twice a
//! year instead.

use std::fs;
use std::path::Path;

/// Fails when a top-level module of `src/` or a member crate of the
/// workspace is not named in `ARCHITECTURE.md` as `` `name` ``.
pub(crate) fn check(root: &Path) -> crate::Result<()> {
    let map = fs::read_to_string(root.join("ARCHITECTURE.md"))?;
    let unnamed = |names: Vec<String>| -> Vec<String> {
        names
            .into_iter()
            .filter(|name| !map.contains(&format!("`{name}`")))
            .collect()
    };
    let modules = unnamed(modules(&root.join("src"))?);
    let members = unnamed(members(&fs::read_to_string(root.join("Cargo.toml"))?)?);
    let mut missing = Vec::new();
    if !modules.is_empty() {
        missing.push(format!("these modules of src/: {}", modules.join(", ")));
    }
    if !members.is_empty() {
        missing.push(format!("these workspace crates: {}", members.join(", ")));
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "ARCHITECTURE.md does not name {}. Add each to the code map",
            missing.join(", nor ")
        )
        .into())
    }
}

/// The member folders of the workspace, from the one-line `members` list
/// of the root manifest.
fn members(manifest: &str) -> crate::Result<Vec<String>> {
    let list = manifest
        .lines()
        .find_map(|line| line.strip_prefix("members = ["))
        .and_then(|rest| rest.strip_suffix(']'))
        .ok_or("Cargo.toml has no one-line `members = [...]` list in [workspace]")?;
    Ok(list
        .split(',')
        .map(|item| item.trim().trim_matches('"').to_owned())
        .filter(|item| !item.is_empty())
        .collect())
}

/// Top-level modules: `src/<name>.rs` and `src/<name>/mod.rs`, without the
/// crate roots `lib.rs` and `main.rs`.
fn modules(src: &Path) -> crate::Result<Vec<String>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(src)? {
        let path = entry?.path();
        let name = if path.is_dir() && path.join("mod.rs").is_file() {
            path.file_name()
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            path.file_stem()
        } else {
            None
        };
        if let Some(name) = name.and_then(|name| name.to_str())
            && name != "lib"
            && name != "main"
        {
            names.push(name.to_owned());
        }
    }
    names.sort();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(map: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/preset")).unwrap();
        for file in [
            "src/lib.rs",
            "src/main.rs",
            "src/cli.rs",
            "src/preset/mod.rs",
        ] {
            fs::write(dir.path().join(file), "").unwrap();
        }
        // Neither a directory without mod.rs nor a non-Rust file is a module.
        fs::create_dir_all(dir.path().join("src/bin")).unwrap();
        fs::write(dir.path().join("src/notes.txt"), "").unwrap();
        fs::write(dir.path().join("ARCHITECTURE.md"), map).unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"core\", \"host\"]\n",
        )
        .unwrap();
        dir
    }

    #[test]
    fn passes_when_every_module_is_named() {
        let dir = tree("`cli` parses, `preset` writes, `core` and `host` are crates.");
        check(dir.path()).unwrap();
    }

    #[test]
    fn names_the_missing_module() {
        let dir = tree("`cli` parses, `core` and `host` are crates.");
        let err = check(dir.path()).unwrap_err().to_string();
        assert!(err.contains("preset"), "{err}");
        assert!(!err.contains("cli"), "{err}");
    }

    #[test]
    fn names_the_missing_workspace_crate() {
        let dir = tree("`cli` parses, `preset` writes, `host` is a crate.");
        let err = check(dir.path()).unwrap_err().to_string();
        assert!(err.contains("these workspace crates: core"), "{err}");
        assert!(!err.contains("modules"), "{err}");
    }
}
