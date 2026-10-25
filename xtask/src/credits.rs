//! Keeps the credits complete.
//!
//! The README credits section is the one list; the book includes it. Every
//! external link under a `Sources` heading of a book page or of
//! `ARCHITECTURE.md` must match a credit entry, or a prefix in
//! `docs/cited-references.txt`, the reference documentation we only cite.

use std::fs;
use std::path::{Path, PathBuf};

const BEGIN: &str = "<!-- ANCHOR: credits -->";
const END: &str = "<!-- ANCHOR_END: credits -->";

/// Licence words that make a work non-permissive, so its entry must say
/// "Inspiration only."
const NOT_PERMISSIVE: [&str; 13] = [
    "GPL",
    "MPL",
    "-NC",
    "-ND",
    "-SA",
    "non-commercial",
    "no license",
    "no licence",
    "not stated",
    "not confirmed",
    "in copyright",
    "arXiv non-exclusive",
    "unclear",
];

/// Fails on a credit entry without a licence, on a non-permissive entry
/// without "Inspiration only.", and on a cited source without a credit.
pub(crate) fn check(root: &Path) -> crate::Result<()> {
    let readme = fs::read_to_string(root.join("README.md"))?;
    let entries = entries(&readme)?;
    let allowed = fs::read_to_string(root.join("docs/cited-references.txt"))?;
    let allowed: Vec<_> = allowed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(normalize)
        .collect();

    let mut problems = Vec::new();
    let mut credited = Vec::new();
    for entry in &entries {
        match link_and_licence(entry) {
            None => problems.push(format!(
                "credit entry has no `[name](url) (licence)`: {entry}"
            )),
            Some((url, licence)) => {
                let restricted = NOT_PERMISSIVE
                    .iter()
                    .any(|word| licence.to_lowercase().contains(&word.to_lowercase()));
                if restricted && !entry.contains("Inspiration only.") {
                    problems.push(format!(
                        "credit entry under `{licence}` lacks \"Inspiration only.\": {entry}"
                    ));
                }
                credited.push(normalize(url));
            }
        }
    }

    for page in pages(root)? {
        let text = fs::read_to_string(&page)?;
        for url in source_links(&text) {
            let url = normalize(&url);
            let known = credited
                .iter()
                .chain(&allowed)
                .any(|prefix| is_prefix(prefix, &url));
            if !known {
                problems.push(format!(
                    "{} cites {url} with no credit entry: add one to the README credits, \
                     or add reference documentation to docs/cited-references.txt",
                    page.strip_prefix(root).unwrap_or(&page).display()
                ));
            }
        }
    }

    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n").into())
    }
}

/// The bullets between the credits anchors of the README.
fn entries(readme: &str) -> crate::Result<Vec<&str>> {
    let start = readme
        .find(BEGIN)
        .ok_or("README.md has no `<!-- ANCHOR: credits -->` line")?;
    let end = readme
        .find(END)
        .ok_or("README.md has no `<!-- ANCHOR_END: credits -->` line")?;
    Ok(readme[start..end]
        .lines()
        .filter_map(|line| line.strip_prefix("- "))
        .collect())
}

/// The URL of the first link and the parenthesised licence right after it.
fn link_and_licence(entry: &str) -> Option<(&str, &str)> {
    let open = entry.find("](")?;
    let rest = &entry[open + 2..];
    let close = rest.find(')')?;
    let licence = rest[close + 1..].strip_prefix(" (")?;
    let licence = &licence[..licence.find(')')?];
    (!licence.trim().is_empty()).then_some((&rest[..close], licence))
}

/// Book pages and `ARCHITECTURE.md`.
fn pages(root: &Path) -> crate::Result<Vec<PathBuf>> {
    let mut pages = vec![root.join("ARCHITECTURE.md")];
    let mut dirs = vec![root.join("docs/src")];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir)? {
            let path = entry?.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|ext| ext == "md") {
                pages.push(path);
            }
        }
    }
    pages.sort();
    Ok(pages)
}

/// External links in the lines under a `Sources` heading, up to the next
/// heading of any level.
fn source_links(page: &str) -> Vec<String> {
    let mut in_sources = false;
    let mut links = Vec::new();
    for line in page.lines() {
        if line.starts_with('#') {
            in_sources = line.trim_start_matches('#').trim() == "Sources";
            continue;
        }
        if !in_sources {
            continue;
        }
        let mut rest = line;
        while let Some(at) = rest.find("https://").or_else(|| rest.find("http://")) {
            let url = &rest[at..];
            let end = url
                .find(|c: char| c.is_whitespace() || matches!(c, ')' | '>' | ']' | '`' | ','))
                .unwrap_or(url.len());
            links.push(url[..end].trim_end_matches('.').to_owned());
            rest = &url[end..];
        }
    }
    links
}

/// Scheme, `www.` and trailing slashes dropped, so equal targets compare
/// equal.
fn normalize(url: &str) -> String {
    let url = url
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");
    url.trim_end_matches('/').to_owned()
}

/// `prefix` covers `url` when it is equal or a parent path of it.
fn is_prefix(prefix: &str, url: &str) -> bool {
    url == prefix
        || url
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with(['/', '#', '?']))
}

#[cfg(test)]
mod tests {
    use super::*;

    const README: &str = "# Title\n\n<!-- ANCHOR: credits -->\n\
        - [Tool](https://github.com/owner/tool) (MIT): parsing.\n\
        - [Synth](https://github.com/owner/synth) (GPL-3.0): mechanisms. Inspiration only.\n\
        - A. Author, [Paper](https://arxiv.org/abs/2401.00001) (CC BY 4.0), venue 2024: method.\n\
        <!-- ANCHOR_END: credits -->\n";

    fn repo(readme: &str, page: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("docs/src/methods")).unwrap();
        fs::write(dir.path().join("README.md"), readme).unwrap();
        fs::write(dir.path().join("ARCHITECTURE.md"), "# Architecture\n").unwrap();
        fs::write(
            dir.path().join("docs/cited-references.txt"),
            "# Reference documentation\nhttps://doc.rust-lang.org/\n",
        )
        .unwrap();
        fs::write(dir.path().join("docs/src/methods/pitch.md"), page).unwrap();
        dir
    }

    #[test]
    fn passes_when_every_source_is_credited_or_allowed() {
        let page = "# Pitch\n\nSee https://example.com/not-a-source.\n\n## Sources\n\n\
            - Tool, https://github.com/owner/tool/blob/main/src/lib.rs\n\
            - [Paper](https://arxiv.org/abs/2401.00001)\n\
            - Book, https://doc.rust-lang.org/book/\n";
        check(repo(README, page).path()).unwrap();
    }

    #[test]
    fn fails_on_a_source_without_a_credit() {
        let page = "# Pitch\n\n## Sources\n\n- https://github.com/other/thing\n";
        let err = check(repo(README, page).path()).unwrap_err().to_string();
        assert!(err.contains("github.com/other/thing"), "{err}");
        assert!(err.contains("methods/pitch.md"), "{err}");
    }

    #[test]
    fn ignores_a_commented_allowlist_line() {
        let page = "# Pitch\n\n## Sources\n\n- https://github.com/other/thing\n";
        let dir = repo(README, page);
        fs::write(
            dir.path().join("docs/cited-references.txt"),
            "# https://github.com/other/thing\n\nhttps://doc.rust-lang.org/\n",
        )
        .unwrap();
        assert!(check(dir.path()).is_err());
    }

    #[test]
    fn does_not_match_a_sibling_repository_by_prefix() {
        let page = "# Pitch\n\n## Sources\n\n- https://github.com/owner/tool-extra\n";
        assert!(check(repo(README, page).path()).is_err());
    }

    #[test]
    fn fails_on_an_entry_without_a_licence() {
        let readme = README.replace(" (MIT): parsing", ": parsing");
        let err = check(repo(&readme, "# Pitch\n").path())
            .unwrap_err()
            .to_string();
        assert!(err.contains("no `[name](url) (licence)`"), "{err}");
    }

    #[test]
    fn fails_on_a_restricted_entry_without_inspiration_only() {
        let readme = README.replace(" Inspiration only.", "");
        let err = check(repo(&readme, "# Pitch\n").path())
            .unwrap_err()
            .to_string();
        assert!(err.contains("GPL-3.0"), "{err}");
    }
}
