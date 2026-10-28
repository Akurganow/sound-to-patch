//! Keeps the credits complete.
//!
//! The README credits section is the one list; the book includes it. Every
//! external link under a `Sources` heading of a book page or of
//! `ARCHITECTURE.md` must match a credit entry, or a prefix in
//! `docs/cited-references.txt`, the reference documentation we only cite.
//! The pages of the shared `reference/` folder and of the agent plugins
//! under `plugins/` count as book pages, and
//! every `url` in `dictionary/sources.json` counts as a cited source.

use std::fs;
use std::path::{Path, PathBuf};

const BEGIN: &str = "<!-- ANCHOR: credits -->";
const END: &str = "<!-- ANCHOR_END: credits -->";

/// Licence ids whose family is not permissive, matched as whole ids or
/// with a version suffix (`GPL-3.0-or-later`).
const COPYLEFT_IDS: [&str; 4] = ["gpl", "lgpl", "agpl", "mpl"];

/// Creative Commons parts that restrict reuse (`BY-NC`, `BY-SA`).
const CC_PARTS: [&str; 3] = ["nc", "nd", "sa"];

/// Phrases for a missing or closed licence, matched as whole words.
const PHRASES: [&[&str]; 8] = [
    &["no", "license"],
    &["no", "licence"],
    &["not", "stated"],
    &["not", "confirmed"],
    &["in", "copyright"],
    &["non-exclusive"],
    &["non-commercial"],
    &["unclear"],
];

/// Whether a licence text names a non-permissive or unknown licence. Words
/// are compared whole, so `Simplified` is not `MPL`.
fn restricted(licence: &str) -> bool {
    let words: Vec<String> = licence
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '(' | ')' | '"'))
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect();
    let copyleft = |word: &str| {
        COPYLEFT_IDS.iter().any(|id| {
            word == *id
                || word
                    .strip_prefix(id)
                    .is_some_and(|rest| rest.starts_with('-'))
        })
    };
    let cc = |word: &str| word.split('-').any(|part| CC_PARTS.contains(&part));
    words.iter().any(|word| copyleft(word) || cc(word))
        || PHRASES
            .iter()
            .any(|phrase| words.windows(phrase.len()).any(|window| window == *phrase))
}

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
                if restricted(licence) && !entry.contains("Inspiration only.") {
                    problems.push(format!(
                        "credit entry under `{licence}` lacks \"Inspiration only.\": {entry}"
                    ));
                }
                credited.push(normalize(url));
            }
        }
    }

    let mut cited: Vec<(PathBuf, String)> = Vec::new();
    for page in pages(root)? {
        let text = fs::read_to_string(&page)?;
        cited.extend(
            source_links(&text)
                .into_iter()
                .map(|url| (page.clone(), url)),
        );
    }
    let sources = root.join("dictionary/sources.json");
    if sources.is_file() {
        cited.extend(
            dictionary_links(&fs::read_to_string(&sources)?)?
                .into_iter()
                .map(|url| (sources.clone(), url)),
        );
    }
    for (page, url) in cited {
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
    if end < start {
        return Err("README.md has `ANCHOR_END: credits` before `ANCHOR: credits`".into());
    }
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

/// The `url` of every entry of the dictionary source list.
fn dictionary_links(text: &str) -> crate::Result<Vec<String>> {
    let list: serde_json::Value =
        serde_json::from_str(text).map_err(|error| format!("dictionary/sources.json: {error}"))?;
    let entries = list
        .as_array()
        .ok_or("dictionary/sources.json must be a list of sources")?;
    Ok(entries
        .iter()
        .filter_map(|entry| entry.get("url").and_then(serde_json::Value::as_str))
        .map(str::to_owned)
        .collect())
}

/// Book pages, reference and plugin pages, and `ARCHITECTURE.md`. Links
/// are not followed, so a page linked from a plugin counts once.
fn pages(root: &Path) -> crate::Result<Vec<PathBuf>> {
    let mut pages = vec![root.join("ARCHITECTURE.md")];
    let mut dirs = vec![root.join("docs/src")];
    for extra in ["reference", "plugins"] {
        if root.join(extra).is_dir() {
            dirs.push(root.join(extra));
        }
    }
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
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
        while let Some(at) = ["https://", "http://"]
            .iter()
            .filter_map(|scheme| rest.find(scheme))
            .min()
        {
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
    fn checks_plugin_pages_and_the_dictionary_sources() {
        let dir = repo(README, "# Pitch\n");
        fs::create_dir_all(dir.path().join("plugins/p/skills/s")).unwrap();
        fs::write(
            dir.path().join("plugins/p/skills/s/SKILL.md"),
            "# S\n\n## Sources\n\n- https://github.com/other/page\n",
        )
        .unwrap();
        fs::create_dir_all(dir.path().join("dictionary")).unwrap();
        fs::write(
            dir.path().join("dictionary/sources.json"),
            r#"[{"id":"x","url":"https://github.com/other/data"},{"id":"y","url":"https://github.com/owner/tool"}]"#,
        )
        .unwrap();
        let err = check(dir.path()).unwrap_err().to_string();
        assert!(err.contains("github.com/other/page"), "{err}");
        assert!(err.contains("github.com/other/data"), "{err}");
        assert!(!err.contains("owner/tool"), "{err}");
    }

    #[test]
    fn checks_an_http_link_before_an_https_one_on_the_same_line() {
        let page = "# Pitch\n\n## Sources\n\n\
            - Old, http://example.com/a, mirror https://doc.rust-lang.org/book/\n";
        let err = check(repo(README, page).path()).unwrap_err().to_string();
        assert!(err.contains("example.com/a"), "{err}");
    }

    #[test]
    fn fails_on_a_source_without_a_credit() {
        let page = "# Pitch\n\n## Sources\n\n- https://github.com/other/thing\n";
        let err = check(repo(README, page).path()).unwrap_err().to_string();
        assert!(err.contains("github.com/other/thing"), "{err}");
        assert!(err.contains("methods/pitch.md"), "{err}");
    }

    #[test]
    fn matches_licence_ids_as_whole_words() {
        assert!(!restricted("BSD-2-Clause \"Simplified\""));
        assert!(!restricted("MIT code, CC0 1.0 assets"));
        assert!(!restricted("CC BY 4.0"));
        for closed in [
            "GPL-3.0-or-later",
            "MPL-2.0",
            "CC BY-NC-SA 4.0",
            "CC BY-SA 4.0",
            "MIT code, non-commercial weights",
            "no license found",
            "in copyright, free access",
            "arXiv non-exclusive license",
        ] {
            assert!(restricted(closed), "{closed}");
        }
    }

    #[test]
    fn fails_on_anchors_in_the_wrong_order() {
        let readme = "<!-- ANCHOR_END: credits -->\n- x\n<!-- ANCHOR: credits -->\n";
        let err = check(repo(readme, "# Pitch\n").path())
            .unwrap_err()
            .to_string();
        assert!(err.contains("before"), "{err}");
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
