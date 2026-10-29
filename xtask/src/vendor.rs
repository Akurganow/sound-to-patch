//! Check of the vendored `pyin` copy. Its library source must equal the
//! crates.io archive, so a reader trusts the copy without a diff.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::fetch::{Sha256Digest, digest_of};

const COPY: &str = "vendor/pyin";
const LIST: &str = "UPSTREAM.sha256";

/// Fails when a file under `vendor/pyin/src` or `vendor/pyin/LICENSE` differs
/// from the hash in `vendor/pyin/UPSTREAM.sha256`, is missing, or has no hash.
pub(crate) fn check(root: &Path) -> crate::Result<()> {
    check_copy(&root.join(COPY))
}

fn check_copy(dir: &Path) -> crate::Result<()> {
    let list = fs::read_to_string(dir.join(LIST))?;
    let pinned = parse(&list)?;
    let mut problems = Vec::new();
    for (path, expected) in &pinned {
        match digest_of(&dir.join(path)) {
            Ok(actual) if actual == *expected => {}
            Ok(actual) => {
                problems.push(format!("{path} has SHA-256 {actual}, expected {expected}"));
            }
            Err(err) => problems.push(format!("{path} cannot be read: {err}")),
        }
    }
    let mut files = Vec::new();
    collect(&dir.join("src"), &mut files)?;
    for file in files {
        let path = file.strip_prefix(dir)?.to_string_lossy().replace('\\', "/");
        if !pinned.contains_key(&path) {
            problems.push(format!("{path} has no hash in {LIST}"));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{COPY} differs from the pyin archive:\n  {}\nRestore the file from the crates.io archive. \
             Update {LIST} only for a new pyin release",
            problems.join("\n  ")
        )
        .into())
    }
}

fn parse(list: &str) -> crate::Result<BTreeMap<String, Sha256Digest>> {
    let mut pinned = BTreeMap::new();
    for line in list
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let (hash, path) = line
            .split_once(' ')
            .ok_or_else(|| format!("{LIST}: `{line}` is not `<hash> <path>`"))?;
        pinned.insert(path.to_owned(), hash.parse::<Sha256Digest>()?);
    }
    Ok(pinned)
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) -> crate::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // SHA-256 of the three bytes "abc", from FIPS 180-2 appendix B.1.
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn copy(dir: &Path) {
        fs::create_dir(dir.join("src")).unwrap();
        fs::write(dir.join("src/lib.rs"), "abc").unwrap();
        fs::write(dir.join("LICENSE"), "abc").unwrap();
        fs::write(
            dir.join(LIST),
            format!("# a comment\n{ABC} src/lib.rs\n{ABC} LICENSE\n"),
        )
        .unwrap();
    }

    #[test]
    fn a_copy_equal_to_its_list_passes() {
        let dir = tempfile::tempdir().unwrap();
        copy(dir.path());
        check_copy(dir.path()).unwrap();
    }

    #[test]
    fn a_changed_byte_fails() {
        let dir = tempfile::tempdir().unwrap();
        copy(dir.path());
        fs::write(dir.path().join("src/lib.rs"), "abd").unwrap();
        let err = check_copy(dir.path()).unwrap_err().to_string();
        assert!(err.contains("src/lib.rs has SHA-256"), "{err}");
    }

    #[test]
    fn a_missing_or_extra_file_fails() {
        let dir = tempfile::tempdir().unwrap();
        copy(dir.path());
        fs::remove_file(dir.path().join("LICENSE")).unwrap();
        fs::write(dir.path().join("src/extra.rs"), "").unwrap();
        let err = check_copy(dir.path()).unwrap_err().to_string();
        assert!(err.contains("LICENSE cannot be read"), "{err}");
        assert!(err.contains("src/extra.rs has no hash"), "{err}");
    }

    #[test]
    fn the_committed_copy_equals_upstream() {
        check(Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).unwrap();
    }
}
