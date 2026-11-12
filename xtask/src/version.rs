//! Version numbers outside the code. `cog bump` calls [`set`] as a pre-bump
//! hook, so the version commit carries the new number in `Cargo.toml`,
//! `Cargo.lock` and `fuzz/Cargo.lock`, and the released binary prints it.
//! [`check_rust_version`] keeps the docs on the manifest's `rust-version`.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Sets `version` in `[workspace.package]` of `root/Cargo.toml`, then
/// refreshes the workspace entries of `Cargo.lock` and, when the fuzz crate
/// exists, the entries of the library and its core in `fuzz/Cargo.lock`.
pub(crate) fn set(root: &Path, version: &str) -> crate::Result<()> {
    set_with(root, version, cargo_update)
}

/// [`set`] with the `cargo update` step passed in.
fn set_with(
    root: &Path,
    version: &str,
    mut update: impl FnMut(&Path, &[&str]) -> crate::Result<()>,
) -> crate::Result<()> {
    let manifest = root.join("Cargo.toml");
    let text = fs::read_to_string(&manifest)?;
    let new_text = with_version(&text, version)?;
    // Every file the steps below can change, kept byte for byte.
    let saved: Vec<(PathBuf, Option<Vec<u8>>)> = ["Cargo.toml", "Cargo.lock", "fuzz/Cargo.lock"]
        .into_iter()
        .map(|name| {
            let path = root.join(name);
            let bytes = fs::read(&path).ok();
            (path, bytes)
        })
        .collect();
    fs::write(&manifest, new_text)?;
    update_locks(root, &mut update).map_err(|err| {
        let failed: Vec<String> = saved
            .iter()
            .filter_map(|(path, bytes)| {
                let undone = match bytes {
                    Some(bytes) => fs::write(path, bytes),
                    None => fs::remove_file(path).or_else(|e| match e.kind() {
                        ErrorKind::NotFound => Ok(()),
                        _ => Err(e),
                    }),
                };
                undone.err().map(|e| format!("{}: {e}", path.display()))
            })
            .collect();
        if failed.is_empty() {
            err
        } else {
            format!(
                "{err}; and the old files could not be restored ({}): \
                 run `git checkout Cargo.toml Cargo.lock fuzz/Cargo.lock`",
                failed.join(", ")
            )
            .into()
        }
    })
}

/// Refreshes the lock files after the manifest changed.
fn update_locks(
    root: &Path,
    update: &mut impl FnMut(&Path, &[&str]) -> crate::Result<()>,
) -> crate::Result<()> {
    update(root, &["--workspace", "--offline"])?;
    if root.join("fuzz/Cargo.toml").exists() {
        // Online: the release job has never fetched the fuzz-only crates.
        // `-p` keeps every other entry as locked. From the root, so rustup
        // keeps the release toolchain instead of the nightly that
        // fuzz/rust-toolchain.toml pins.
        update(
            root,
            &[
                "--manifest-path",
                "fuzz/Cargo.toml",
                "-p",
                "sound-to-patch",
                "-p",
                "sound-to-patch-core",
            ],
        )?;
    }
    Ok(())
}

fn cargo_update(root: &Path, args: &[&str]) -> crate::Result<()> {
    let status = Command::new(env!("CARGO"))
        .arg("update")
        .args(args)
        .current_dir(root)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo update {} failed with {status}", args.join(" ")).into())
    }
}

/// Replaces the `version` line of `[workspace.package]`, keeping every line
/// ending as it was.
fn with_version(manifest: &str, version: &str) -> crate::Result<String> {
    check_version(version)?;
    let mut in_section = false;
    let mut replaced = false;
    let mut out = String::with_capacity(manifest.len());
    for raw in manifest.split_inclusive('\n') {
        let line = raw.trim_end_matches(['\r', '\n']);
        let ending = &raw[line.len()..];
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
        out.push_str(ending);
    }
    if replaced {
        Ok(out)
    } else {
        Err("Cargo.toml has no `version = ` line in [workspace.package]".into())
    }
}

/// Accepts the `MAJOR.MINOR.PATCH` numbers that cargo accepts: no leading
/// zero, and each part fits a `u64`.
fn check_version(version: &str) -> crate::Result<()> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(format!("`{version}` is not a version: give MAJOR.MINOR.PATCH").into());
    }
    for part in parts {
        if part.len() > 1 && part.starts_with('0') {
            return Err(format!(
                "`{version}` has a leading zero in `{part}`, which cargo rejects: \
                 write the number without it"
            )
            .into());
        }
        if part.parse::<u64>().is_err() {
            return Err(format!(
                "`{version}` has `{part}`, which does not fit a 64-bit number: \
                 give a smaller number"
            )
            .into());
        }
    }
    Ok(())
}

/// Fails when a doc names a Rust version, as `Rust 1.NN`, other than the
/// `rust-version` of `[workspace.package]`.
pub(crate) fn check_rust_version(root: &Path) -> crate::Result<()> {
    let manifest = fs::read_to_string(root.join("Cargo.toml"))?;
    let msrv = manifest
        .lines()
        .find_map(|line| line.strip_prefix("rust-version = \""))
        .and_then(|rest| rest.strip_suffix('"'))
        .ok_or("Cargo.toml has no `rust-version = \"...\"` line")?;
    let mut docs = vec![root.join("README.md"), root.join("CONTRIBUTING.md")];
    let mut dirs = vec![root.join("docs/src")];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|ext| ext == "md") {
                docs.push(path);
            }
        }
    }
    let mut wrong = Vec::new();
    for doc in docs {
        let text = fs::read_to_string(&doc)?;
        for named in rust_versions(&text) {
            if named != msrv {
                wrong.push(format!("{} names Rust {named}", doc.display()));
            }
        }
    }
    if wrong.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "rust-version is {msrv}, but {}. Update the docs or the manifest",
            wrong.join(", ")
        )
        .into())
    }
}

/// Every `1.NN` that follows the word `Rust `, with at least one digit
/// after the dot.
fn rust_versions(text: &str) -> Vec<&str> {
    text.match_indices("Rust 1.")
        .filter_map(|(at, _)| {
            let version = &text[at + "Rust ".len()..];
            let end = version
                .char_indices()
                .skip(2)
                .find(|&(_, c)| !c.is_ascii_digit())
                .map_or(version.len(), |(i, _)| i);
            (end > 2).then(|| &version[..end])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_rust_versions_in_prose() {
        let text = "Install Rust 1.91 or newer. Rust 1.100 later. Rusty 1.2. Rust 1.x too.";
        assert_eq!(rust_versions(text), ["1.91", "1.100"]);
    }

    #[test]
    fn rust_version_check_names_the_stale_doc() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("docs/src")).unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace.package]\nrust-version = \"1.91\"\n",
        )
        .unwrap();
        fs::write(dir.path().join("README.md"), "Needs Rust 1.91.\n").unwrap();
        fs::write(dir.path().join("CONTRIBUTING.md"), "Needs Rust 1.90.\n").unwrap();
        fs::write(dir.path().join("docs/src/install.md"), "Rust 1.89 works.\n").unwrap();
        fs::write(dir.path().join("docs/src/notes.txt"), "Rust 1.80 is old.\n").unwrap();
        let err = check_rust_version(dir.path()).unwrap_err().to_string();
        assert!(err.contains("CONTRIBUTING.md names Rust 1.90"), "{err}");
        assert!(err.contains("install.md names Rust 1.89"), "{err}");
        assert!(!err.contains("README"), "{err}");
        assert!(!err.contains("notes.txt"), "{err}");
    }

    #[test]
    fn set_writes_the_manifest_and_the_lock_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("src/lib.rs"), "").unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"probe\"\nversion.workspace = true\nedition = \"2024\"\n\n\
             [workspace]\n\n[workspace.package]\nversion = \"0.0.0\"\n",
        )
        .unwrap();
        set(dir.path(), "0.1.0").unwrap();
        let manifest = fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
        let lock = fs::read_to_string(dir.path().join("Cargo.lock")).unwrap();
        assert!(manifest.contains("version = \"0.1.0\""), "{manifest}");
        assert!(
            lock.contains("name = \"probe\"\nversion = \"0.1.0\""),
            "{lock}"
        );
    }

    #[test]
    fn set_writes_the_crate_version_into_the_fuzz_lock() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let write = |path: &str, text: &str| {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        };
        write(
            "Cargo.toml",
            "[package]\nname = \"sound-to-patch\"\nversion.workspace = true\n\
             edition = \"2024\"\n\n[dependencies]\nsound-to-patch-core = { path = \"core\" }\n\n\
             [workspace]\nmembers = [\"core\"]\n\n[workspace.package]\nversion = \"0.0.0\"\n",
        );
        write("src/lib.rs", "");
        write(
            "core/Cargo.toml",
            "[package]\nname = \"sound-to-patch-core\"\nversion.workspace = true\n\
             edition = \"2024\"\n",
        );
        write("core/src/lib.rs", "");
        write(
            "fuzz/Cargo.toml",
            "[package]\nname = \"probe-fuzz\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n\
             [workspace]\n\n[dependencies]\nsound-to-patch = { path = \"..\" }\n\
             probe-dep = \"0.1\"\n",
        );
        write("fuzz/src/lib.rs", "");
        // A vendored registry in place of crates.io, so the fuzz crate
        // locks a registry dependency without the network.
        write(
            ".cargo/config.toml",
            "[source.crates-io]\nreplace-with = \"vendored\"\n\n\
             [source.vendored]\ndirectory = \"vendor\"\n",
        );
        let vendor = |version: &str| {
            let folder = format!("vendor/probe-dep-{version}");
            write(
                &format!("{folder}/Cargo.toml"),
                &format!(
                    "[package]\nname = \"probe-dep\"\nversion = \"{version}\"\nedition = \"2024\"\n"
                ),
            );
            write(&format!("{folder}/src/lib.rs"), "");
            write(
                &format!("{folder}/.cargo-checksum.json"),
                &format!("{{\"files\":{{}},\"package\":\"{}\"}}", "0".repeat(64)),
            );
        };
        vendor("0.1.0");
        set(root, "0.1.0").unwrap();
        // A newer release of the other dependency appears; the version
        // commit leaves its lock entry alone.
        vendor("0.1.1");
        set(root, "0.2.0").unwrap();
        let lock = fs::read_to_string(root.join("fuzz/Cargo.lock")).unwrap();
        for name in ["sound-to-patch", "sound-to-patch-core"] {
            assert!(
                lock.contains(&format!("name = \"{name}\"\nversion = \"0.2.0\"")),
                "{lock}"
            );
        }
        assert!(
            lock.contains("name = \"probe-dep\"\nversion = \"0.1.0\""),
            "{lock}"
        );
    }

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

    #[test]
    fn rejects_a_leading_zero() {
        for bad in ["01.0.0", "0.01.0", "0.0.00", "00.0.0"] {
            let err = with_version(MANIFEST, bad).unwrap_err().to_string();
            assert!(err.contains("leading zero"), "{bad}: {err}");
        }
        for good in ["0.0.0", "10.0.0", "0.10.100"] {
            assert!(with_version(MANIFEST, good).is_ok(), "{good}");
        }
    }

    #[test]
    fn rejects_a_component_that_overflows_u64() {
        for bad in ["99999999999999999999.0.0", "0.18446744073709551616.0"] {
            assert!(with_version(MANIFEST, bad).is_err(), "{bad}");
        }
        assert!(with_version(MANIFEST, "18446744073709551615.0.0").is_ok());
    }

    #[test]
    fn keeps_crlf_line_endings() {
        let crlf = MANIFEST.replace('\n', "\r\n");
        let out = with_version(&crlf, "0.1.0").unwrap();
        assert_eq!(out, crlf.replace("\"0.0.0\"", "\"0.1.0\""));
    }

    #[test]
    fn keeps_a_missing_final_newline() {
        let bare = MANIFEST.trim_end_matches('\n');
        let out = with_version(bare, "0.1.0").unwrap();
        assert_eq!(out, bare.replace("\"0.0.0\"", "\"0.1.0\""));
    }

    #[test]
    fn a_failed_update_leaves_every_file_as_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("fuzz")).unwrap();
        let files = [
            ("Cargo.toml", MANIFEST),
            ("Cargo.lock", "# workspace lock\r\n"),
            ("fuzz/Cargo.toml", "[package]\n"),
            ("fuzz/Cargo.lock", "# fuzz lock"),
        ];
        for (name, text) in files {
            fs::write(root.join(name), text).unwrap();
        }
        let mut calls = 0;
        let err = set_with(root, "0.1.0", |root, _| {
            calls += 1;
            if calls == 1 {
                fs::write(root.join("Cargo.lock"), "new workspace lock\n")?;
                Ok(())
            } else {
                fs::write(root.join("fuzz/Cargo.lock"), "half written")?;
                Err("cargo update failed: no network".into())
            }
        })
        .unwrap_err()
        .to_string();
        assert!(err.contains("no network"), "{err}");
        for (name, text) in files {
            assert_eq!(fs::read_to_string(root.join(name)).unwrap(), text, "{name}");
        }
    }
}
