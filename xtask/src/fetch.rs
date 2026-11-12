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

/// The SHA-256 of `bytes` as 64 lower-case hex digits.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256Digest(Sha256::digest(bytes).into()).to_string()
}

/// Downloads `url` to `out` and keeps the file only if its SHA-256 equals
/// `expected`.
///
/// A file already at `out` with the right digest is kept and not fetched
/// again. A download with the wrong digest is deleted. Only `https://` and
/// `file://` URLs work, and a redirect must stay on `https://`. The connect
/// step times out after 30 seconds and the whole transfer after an hour.
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
            "--proto",
            "=https,file",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            CONNECT_SECONDS,
            "--max-time",
            TRANSFER_SECONDS,
            "--output",
        ])
        .arg(&partial)
        .arg("--")
        .arg(url)
        .status()
        .map_err(|err| format!("could not run curl: {err}. Install curl and retry"))?;
    if !status.success() {
        // curl may leave a partial file behind; a missing one is fine.
        let _ = fs::remove_file(&partial);
        return Err(curl_failure(url, &status.to_string(), status.code()).into());
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

/// The seconds curl may take to connect.
const CONNECT_SECONDS: &str = "30";

/// The seconds curl may take for the whole transfer.
const TRANSFER_SECONDS: &str = "3600";

/// The exit code curl gives when a time limit ends the transfer.
const CURL_TIMED_OUT: i32 = 28;

/// The error of a failed curl run, with the cause where the code names one.
fn curl_failure(url: &str, status: &str, code: Option<i32>) -> String {
    if code == Some(CURL_TIMED_OUT) {
        format!(
            "curl timed out on {url}: the connection took over {CONNECT_SECONDS} seconds \
             or the transfer over {TRANSFER_SECONDS}. Check the network and retry on a faster link"
        )
    } else {
        format!("curl failed on {url} with {status}")
    }
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

    #[test]
    fn a_timeout_names_its_cause_and_the_fix() {
        let message = curl_failure("https://x", "exit status: 28", Some(28));
        assert!(message.contains("timed out"), "{message}");
        assert!(message.contains("over 30 seconds"), "{message}");
        assert!(message.contains("over 3600"), "{message}");
        assert!(message.contains("retry"), "{message}");
        let other = curl_failure("https://x", "exit status: 22", Some(22));
        assert_eq!(other, "curl failed on https://x with exit status: 22");
    }

    #[test]
    fn fetch_refuses_a_plain_http_url() {
        use std::io::Write;
        use std::net::TcpListener;

        // A live server that sends the pinned bytes: only the protocol
        // rule can make the call fail.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            while let Ok((mut stream, _)) = listener.accept() {
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc",
                );
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("abc.txt");
        let url = format!("http://127.0.0.1:{port}/x");
        let err = fetch(&url, &ABC.parse().unwrap(), &out).unwrap_err();
        assert!(err.to_string().contains("curl failed"), "{err}");
        assert!(!out.exists());
        assert!(!out.with_extension("part").exists());
    }

    #[test]
    fn fetch_does_not_read_a_url_that_starts_with_a_dash_as_an_option() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("abc.txt");
        let err = fetch("--version", &ABC.parse().unwrap(), &out).unwrap_err();
        assert!(
            err.to_string().contains("curl failed on --version"),
            "{err}"
        );
        assert!(!out.exists());
        assert!(!out.with_extension("part").exists());
    }
}
