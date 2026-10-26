//! The one download helper for pinned files.
//!
//! Every epic that evaluates on a dataset calls [`fetch`] and writes no
//! fetcher of its own. Downloads go to the git-ignored `data/` directory or
//! another ignored path the caller picks.

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;
use std::process::Command;
use std::str::FromStr;

use sha2::{Digest, Sha256};

/// A SHA-256 digest, parsed from 64 hex digits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Sha256Digest(pub(crate) [u8; 32]);

impl FromStr for Sha256Digest {
    type Err = String;

    fn from_str(hex: &str) -> std::result::Result<Self, Self::Err> {
        let invalid = || format!("`{hex}` is not a SHA-256: give 64 hex digits");
        // from_str_radix alone would accept a sign such as `+f`.
        if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid());
        }
        let mut bytes = [0; 32];
        let (pairs, _) = hex.as_bytes().as_chunks::<2>();
        for (byte, pair) in bytes.iter_mut().zip(pairs) {
            let pair = std::str::from_utf8(pair).map_err(|_| invalid())?;
            *byte = u8::from_str_radix(pair, 16).map_err(|_| invalid())?;
        }
        Ok(Self(bytes))
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

/// Downloads `url` to `out` and keeps the file only if its SHA-256 equals
/// `expected`.
///
/// A file already at `out` with the right digest is kept and not fetched
/// again. A download with the wrong digest is deleted.
pub(crate) fn fetch(url: &str, expected: &Sha256Digest, out: &Path) -> crate::Result<()> {
    if out.exists() && digest_of(out)? == *expected {
        return Ok(());
    }
    if let Some(dir) = out.parent() {
        fs::create_dir_all(dir)?;
    }
    let partial = out.with_extension("part");
    let status = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--output",
        ])
        .arg(&partial)
        .arg(url)
        .status()
        .map_err(|err| format!("could not run curl: {err}. Install curl and retry"))?;
    if !status.success() {
        // curl may leave a partial file behind; a missing one is fine.
        let _ = fs::remove_file(&partial);
        return Err(format!("curl failed on {url} with {status}").into());
    }
    let actual = digest_of(&partial)?;
    if actual != *expected {
        fs::remove_file(&partial)?;
        return Err(format!(
            "{url} has SHA-256 {actual}, expected {expected}. \
             The source changed or the pin is wrong: check the source, then update the pin"
        )
        .into());
    }
    fs::rename(&partial, out)?;
    Ok(())
}

fn digest_of(path: &Path) -> io::Result<Sha256Digest> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0; 1 << 20];
    loop {
        match file.read(&mut buf)? {
            0 => return Ok(Sha256Digest(hasher.finalize().into())),
            n => hasher.update(&buf[..n]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // SHA-256 of the three bytes "abc", from FIPS 180-2 appendix B.1.
    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn source(dir: &Path) -> String {
        let path = dir.join("source.txt");
        fs::write(&path, "abc").unwrap();
        format!("file://{}", path.display())
    }

    #[test]
    fn digest_round_trips_through_hex() {
        assert_eq!(ABC.parse::<Sha256Digest>().unwrap().to_string(), ABC);
    }

    #[test]
    fn digest_rejects_wrong_length_and_non_hex() {
        assert!("abc".parse::<Sha256Digest>().is_err());
        assert!(ABC.replace('b', "g").parse::<Sha256Digest>().is_err());
        let signed = format!("+f{}", "a".repeat(62));
        assert!(signed.parse::<Sha256Digest>().is_err());
    }

    #[test]
    fn fetch_keeps_a_file_with_the_pinned_digest() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("data/abc.txt");
        fetch(&source(dir.path()), &ABC.parse().unwrap(), &out).unwrap();
        assert_eq!(fs::read_to_string(&out).unwrap(), "abc");
    }

    #[test]
    fn fetch_replaces_a_stale_file_at_the_target() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("abc.txt");
        fs::write(&out, "stale").unwrap();
        fetch(&source(dir.path()), &ABC.parse().unwrap(), &out).unwrap();
        assert_eq!(fs::read_to_string(&out).unwrap(), "abc");
    }

    #[test]
    fn fetch_rejects_and_deletes_a_file_with_another_digest() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("abc.txt");
        let wrong = "00".repeat(32).parse().unwrap();
        let err = fetch(&source(dir.path()), &wrong, &out).unwrap_err();
        assert!(err.to_string().contains(ABC), "{err}");
        assert!(!out.exists());
        assert!(!out.with_extension("part").exists());
    }
}
