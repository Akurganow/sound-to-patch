//! Keeps the code map in `ARCHITECTURE.md` from losing a module.
//!
//! The check reads names only. The descriptions get a manual review twice a
//! year instead.

use std::fs;
use std::path::Path;

/// Fails when a top-level module of `src/` is not named in `ARCHITECTURE.md`
/// as `` `name` ``.
pub(crate) fn check(root: &Path) -> crate::Result<()> {
    let map = fs::read_to_string(root.join("ARCHITECTURE.md"))?;
    let missing: Vec<_> = modules(&root.join("src"))?
        .into_iter()
        .filter(|name| !map.contains(&format!("`{name}`")))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "ARCHITECTURE.md does not name these modules of src/: {}. Add each to the code map",
            missing.join(", ")
        )
        .into())
    }
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
        fs::write(dir.path().join("ARCHITECTURE.md"), map).unwrap();
        dir
    }

    #[test]
    fn passes_when_every_module_is_named() {
        let dir = tree("`cli` parses, `preset` writes.");
        check(dir.path()).unwrap();
    }

    #[test]
    fn names_the_missing_module() {
        let dir = tree("`cli` parses.");
        let err = check(dir.path()).unwrap_err().to_string();
        assert!(err.contains("preset"), "{err}");
        assert!(!err.contains("cli"), "{err}");
    }
}
