//! Checks the agent plugins under `plugins/` and the root marketplace file.
//!
//! The limits come from the portable skill specification and the plugin
//! docs of the agent harness: the six frontmatter fields, a description of
//! at most 1,024 characters, a `SKILL.md` under 500 lines. The 300-line cap
//! on reference files and the banned phrases are project rules.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::reference::CONTENTS_ABOVE;

/// The line every plugin manifest description, plugin README and
/// `SKILL.md` carries, as the README states it.
pub(crate) const NON_AFFILIATION: &str = "Vital and Serum 2 are trademarks of their owners. This project is not affiliated with or endorsed by them.";

/// The frontmatter fields of the portable skill specification.
const FIELDS: [&str; 6] = [
    "name",
    "description",
    "license",
    "compatibility",
    "metadata",
    "allowed-tools",
];

const DESCRIPTION_MAX: usize = 1024;
const COMPATIBILITY_MAX: usize = 500;
const SKILL_LINES: usize = 500;
const SKILL_CHARS: usize = 20_000;
const REFERENCE_LINES: usize = 300;

/// Phrases a synth folder must not hold, lowercased. A file about one
/// synth explains it on its own terms.
fn banned(synth: &str) -> &'static [&'static str] {
    match synth {
        "vital" => &["serum", "older version", "previous version", "compared to"],
        "serum2" => &[
            "serum1",
            "serum 1",
            "older version",
            "previous version",
            "compared to",
        ],
        _ => &[],
    }
}

/// Words a synth folder must not hold, compared whole and lowercased.
fn banned_words(synth: &str) -> &'static [&'static str] {
    match synth {
        "serum2" => &["s1"],
        _ => &[],
    }
}

/// Runs every check and lists every problem.
pub(crate) fn check(root: &Path) -> crate::Result<()> {
    let mut problems = Vec::new();
    let plugins = plugin_dirs(root)?;
    check_marketplace(root, &plugins, &mut problems)?;
    for dir in &plugins {
        check_plugin(root, dir, &mut problems)?;
    }
    check_probes(root, &plugins, &mut problems)?;
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n").into())
    }
}

fn plugin_dirs(root: &Path) -> crate::Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    if let Ok(entries) = fs::read_dir(root.join("plugins")) {
        for entry in entries {
            let path = entry?.path();
            if path.is_dir() {
                dirs.push(path);
            }
        }
    }
    dirs.sort();
    Ok(dirs)
}

fn shown(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn json(path: &Path) -> crate::Result<Value> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(serde_json::from_str(&text).map_err(|error| format!("{}: {error}", path.display()))?)
}

fn check_marketplace(
    root: &Path,
    plugins: &[PathBuf],
    problems: &mut Vec<String>,
) -> crate::Result<()> {
    let path = root.join(".claude-plugin/marketplace.json");
    if plugins.is_empty() && !path.exists() {
        return Ok(());
    }
    let market = json(&path)?;
    for field in ["name", "plugins"] {
        if market.get(field).is_none() {
            problems.push(format!("marketplace.json has no `{field}`; add it"));
        }
    }
    if market
        .pointer("/owner/name")
        .and_then(Value::as_str)
        .is_none()
    {
        problems.push("marketplace.json has no `owner.name`; add it".to_owned());
    }
    let mut listed = BTreeSet::new();
    for entry in market
        .get("plugins")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = entry.get("name").and_then(Value::as_str).unwrap_or("");
        let source = entry.get("source").and_then(Value::as_str).unwrap_or("");
        if !source.starts_with("./") || source.contains("..") {
            problems.push(format!(
                "marketplace entry `{name}` has source {source:?}: start it with `./` and stay inside the repository"
            ));
            continue;
        }
        let dir = root.join(&source[2..]);
        let manifest = dir.join(".claude-plugin/plugin.json");
        match json(&manifest) {
            Ok(value) if value.get("name").and_then(Value::as_str) == Some(name) => {}
            Ok(_) => problems.push(format!(
                "marketplace entry `{name}` points at a plugin with another name; make them equal"
            )),
            Err(error) => problems.push(format!("marketplace entry `{name}`: {error}")),
        }
        listed.insert(dir);
    }
    for dir in plugins {
        if !listed.contains(dir) {
            problems.push(format!(
                "{} is not listed in marketplace.json; add an entry",
                shown(root, dir)
            ));
        }
    }
    Ok(())
}

fn check_plugin(root: &Path, dir: &Path, problems: &mut Vec<String>) -> crate::Result<()> {
    let folder = dir.file_name().and_then(|name| name.to_str()).unwrap_or("");
    let manifest = dir.join(".claude-plugin/plugin.json");
    match json(&manifest) {
        Ok(value) => {
            let name = value.get("name").and_then(Value::as_str).unwrap_or("");
            if name != folder {
                problems.push(format!(
                    "{}: `name` must be {folder:?}",
                    shown(root, &manifest)
                ));
            }
            if name.starts_with("claude") || name.starts_with("anthropic") {
                problems.push(format!(
                    "{}: a plugin name may not start with a reserved word",
                    shown(root, &manifest)
                ));
            }
            let description = value
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("");
            if !description.contains(NON_AFFILIATION) {
                problems.push(format!(
                    "{}: the description lacks the non-affiliation line; append it",
                    shown(root, &manifest)
                ));
            }
        }
        Err(error) => problems.push(error.to_string()),
    }
    let readme = dir.join("README.md");
    if !fs::read_to_string(&readme).is_ok_and(|text| text.contains(NON_AFFILIATION)) {
        problems.push(format!(
            "{} lacks the non-affiliation line; add it",
            shown(root, &readme)
        ));
    }
    for skill in subdirs(&dir.join("skills"))? {
        check_skill(root, &skill, problems)?;
    }
    Ok(())
}

fn subdirs(dir: &Path) -> crate::Result<Vec<PathBuf>> {
    let mut dirs = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries {
            let path = entry?.path();
            if path.is_dir() {
                dirs.push(path);
            }
        }
    }
    dirs.sort();
    Ok(dirs)
}

/// Every file with `extension` below `dir`, sorted.
fn files(dir: &Path, extension: &str) -> crate::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut dirs = vec![dir.to_owned()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            let path = entry?.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|ext| ext == extension) {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

/// The top-level frontmatter fields and their one-line values.
fn frontmatter(text: &str) -> Option<Vec<(String, String)>> {
    let rest = text.strip_prefix("---\n")?;
    let end = rest.find("\n---\n")?;
    Some(
        rest[..end]
            .lines()
            .filter(|line| !line.starts_with([' ', '\t']) && !line.trim().is_empty())
            .filter_map(|line| line.split_once(':'))
            .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
            .collect(),
    )
}

fn check_skill(root: &Path, dir: &Path, problems: &mut Vec<String>) -> crate::Result<()> {
    let folder = dir.file_name().and_then(|name| name.to_str()).unwrap_or("");
    let path = dir.join("SKILL.md");
    let at = shown(root, &path);
    let Ok(text) = fs::read_to_string(&path) else {
        problems.push(format!("{at} is missing; every skill folder needs one"));
        return Ok(());
    };
    match frontmatter(&text) {
        None => problems.push(format!("{at} has no `---` frontmatter; add one")),
        Some(fields) => {
            for (key, _) in &fields {
                if !FIELDS.contains(&key.as_str()) {
                    problems.push(format!(
                        "{at}: frontmatter field `{key}` is outside the six spec fields; remove it"
                    ));
                }
            }
            let get = |want: &str| {
                fields
                    .iter()
                    .find(|(key, _)| key == want)
                    .map(|(_, value)| value.as_str())
            };
            let name = get("name").unwrap_or("");
            let valid = !name.is_empty()
                && name.len() <= 64
                && name.split('-').all(|part| {
                    !part.is_empty()
                        && part
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                });
            if name != folder || !valid {
                problems.push(format!(
                    "{at}: `name` must equal the folder name {folder:?}, in lowercase letters, digits and single hyphens"
                ));
            }
            let description = get("description").unwrap_or("");
            let length = description.chars().count();
            if length == 0 || length > DESCRIPTION_MAX {
                problems.push(format!("{at}: the description has {length} characters; keep it within 1 to {DESCRIPTION_MAX} on one line"));
            }
            if get("compatibility").is_some_and(|value| value.chars().count() > COMPATIBILITY_MAX) {
                problems.push(format!(
                    "{at}: `compatibility` is over {COMPATIBILITY_MAX} characters; shorten it"
                ));
            }
        }
    }
    let lines = text.lines().count();
    let chars = text.chars().count();
    if lines >= SKILL_LINES || chars > SKILL_CHARS {
        problems.push(format!(
            "{at} has {lines} lines and {chars} characters; keep it under {SKILL_LINES} lines and {SKILL_CHARS} characters, and move detail into reference files"
        ));
    }
    if !text.contains(NON_AFFILIATION) {
        problems.push(format!("{at} lacks the non-affiliation line; add it"));
    }
    for file in files(&dir.join("reference"), "md")? {
        check_reference(root, dir, &file, problems)?;
    }
    for file in files(&dir.join("examples"), "json")? {
        check_banned(root, dir, &file, &fs::read_to_string(&file)?, problems);
    }
    Ok(())
}

fn check_reference(
    root: &Path,
    skill: &Path,
    file: &Path,
    problems: &mut Vec<String>,
) -> crate::Result<()> {
    let text = fs::read_to_string(file)?;
    let at = shown(root, file);
    let lines = text.lines().count();
    if lines > REFERENCE_LINES {
        problems.push(format!(
            "{at} has {lines} lines; split it to stay within {REFERENCE_LINES}"
        ));
    }
    if lines > CONTENTS_ABOVE {
        let headings: Vec<&str> = text
            .lines()
            .filter(|line| line.starts_with("## "))
            .collect();
        if headings.first() != Some(&"## Contents") {
            problems.push(format!(
                "{at} has {lines} lines; open it with a `## Contents` list before the first section"
            ));
        }
    }
    check_banned(root, skill, file, &text, problems);
    Ok(())
}

/// The synth folder of a file: the first part of its path below `reference`
/// or `examples` of the skill.
fn synth_of(skill: &Path, file: &Path) -> Option<String> {
    let mut parts = file.strip_prefix(skill).ok()?.iter();
    parts.next()?;
    let synth = parts.next()?.to_str()?;
    parts.next().map(|_| synth.to_owned())
}

fn check_banned(root: &Path, skill: &Path, file: &Path, text: &str, problems: &mut Vec<String>) {
    let Some(synth) = synth_of(skill, file) else {
        return;
    };
    let lower = text.to_lowercase();
    let at = shown(root, file);
    for phrase in banned(&synth) {
        if lower.contains(phrase) {
            problems.push(format!("{at} holds {phrase:?}; a {synth} file names no other synth and compares no versions"));
        }
    }
    let words: BTreeSet<&str> = lower.split(|c: char| !c.is_alphanumeric()).collect();
    for word in banned_words(&synth) {
        if words.contains(word) {
            problems.push(format!("{at} holds the word {word:?}; remove it"));
        }
    }
}

/// Each reference file has one probe whose answer appears in it and in no
/// other reference file.
fn check_probes(root: &Path, plugins: &[PathBuf], problems: &mut Vec<String>) -> crate::Result<()> {
    let mut texts = Vec::new();
    for plugin in plugins {
        for skill in subdirs(&plugin.join("skills"))? {
            for file in files(&skill.join("reference"), "md")? {
                let text = fs::read_to_string(&file)?;
                texts.push((file.strip_prefix(root).unwrap_or(&file).to_owned(), text));
            }
        }
    }
    let path = root.join("dictionary/probes.json");
    if texts.is_empty() && !path.exists() {
        return Ok(());
    }
    let probes = json(&path)?;
    let mut covered = BTreeSet::new();
    for probe in probes.as_array().into_iter().flatten() {
        let file = probe.get("file").and_then(Value::as_str).unwrap_or("");
        let answer = probe.get("answer").and_then(Value::as_str).unwrap_or("");
        if probe
            .get("question")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
            || answer.is_empty()
        {
            problems.push(format!("probe for {file:?} needs a question and an answer"));
            continue;
        }
        let holders: Vec<&Path> = texts
            .iter()
            .filter(|(_, text)| text.contains(answer))
            .map(|(path, _)| path.as_path())
            .collect();
        if holders != [Path::new(file)] {
            problems.push(format!(
                "the answer {answer:?} of the probe for {file} must appear in that file only; it appears in {holders:?}"
            ));
        }
        if !covered.insert(file.to_owned()) {
            problems.push(format!("{file} has two probes; keep one"));
        }
    }
    for (path, _) in &texts {
        if !covered.contains(&path.display().to_string()) {
            problems.push(format!(
                "{} has no probe in dictionary/probes.json; add one",
                path.display()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SKILL: &str = "---\nname: synth-reference\ndescription: A reference. Use when writing presets.\nlicense: MIT\nmetadata:\n  project: x\n---\n\n# Ref\n\nVital and Serum 2 are trademarks of their owners. This project is not affiliated with or endorsed by them.\n";

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let plugin = root.join("plugins/ref");
        let skill = plugin.join("skills/synth-reference");
        fs::create_dir_all(root.join(".claude-plugin")).unwrap();
        fs::create_dir_all(plugin.join(".claude-plugin")).unwrap();
        fs::create_dir_all(skill.join("reference/vital")).unwrap();
        fs::create_dir_all(root.join("dictionary")).unwrap();
        fs::write(
            root.join(".claude-plugin/marketplace.json"),
            r#"{"name":"m","owner":{"name":"o"},"plugins":[{"name":"ref","source":"./plugins/ref"}]}"#,
        )
        .unwrap();
        fs::write(
            plugin.join(".claude-plugin/plugin.json"),
            format!(r#"{{"name":"ref","description":"A plugin. {NON_AFFILIATION}"}}"#),
        )
        .unwrap();
        fs::write(
            plugin.join("README.md"),
            format!("# ref\n\n{NON_AFFILIATION}\n"),
        )
        .unwrap();
        fs::write(skill.join("SKILL.md"), SKILL).unwrap();
        fs::write(
            skill.join("reference/vital/filter.md"),
            "# Filter\n\n`filter_N_cutoff`\n",
        )
        .unwrap();
        fs::write(
            root.join("dictionary/probes.json"),
            r#"[{"file":"plugins/ref/skills/synth-reference/reference/vital/filter.md","question":"Which key sets the cutoff?","answer":"filter_N_cutoff"}]"#,
        )
        .unwrap();
        dir
    }

    fn fails_with(dir: &tempfile::TempDir, text: &str) {
        let err = check(dir.path()).unwrap_err().to_string();
        assert!(err.contains(text), "{err}");
    }

    #[test]
    fn a_good_plugin_passes() {
        check(repo().path()).unwrap();
    }

    #[test]
    fn the_real_plugins_pass() {
        check(crate::root()).unwrap();
    }

    #[test]
    fn an_extra_frontmatter_field_fails() {
        let dir = repo();
        let path = dir
            .path()
            .join("plugins/ref/skills/synth-reference/SKILL.md");
        fs::write(
            &path,
            SKILL.replace("license: MIT\n", "license: MIT\nuser-invocable: false\n"),
        )
        .unwrap();
        fails_with(&dir, "user-invocable");
    }

    #[test]
    fn a_long_description_fails() {
        let dir = repo();
        let path = dir
            .path()
            .join("plugins/ref/skills/synth-reference/SKILL.md");
        let long = format!("description: {}\n", "a".repeat(1025));
        fs::write(
            &path,
            SKILL.replace(
                "description: A reference. Use when writing presets.\n",
                &long,
            ),
        )
        .unwrap();
        fails_with(&dir, "1025 characters");
    }

    #[test]
    fn a_long_skill_file_fails() {
        let dir = repo();
        let path = dir
            .path()
            .join("plugins/ref/skills/synth-reference/SKILL.md");
        fs::write(&path, format!("{SKILL}{}", "line\n".repeat(500))).unwrap();
        fails_with(&dir, "under 500 lines");
    }

    #[test]
    fn a_missing_non_affiliation_line_fails() {
        let dir = repo();
        fs::write(dir.path().join("plugins/ref/README.md"), "# ref\n").unwrap();
        fails_with(&dir, "README.md lacks the non-affiliation line");
    }

    #[test]
    fn a_long_reference_file_needs_contents_and_a_cap() {
        let dir = repo();
        let path = dir
            .path()
            .join("plugins/ref/skills/synth-reference/reference/vital/filter.md");
        fs::write(
            &path,
            format!("# F\n\n`filter_N_cutoff`\n\n## A\n{}", "x\n".repeat(120)),
        )
        .unwrap();
        fails_with(&dir, "## Contents");
        fs::write(
            &path,
            format!("# F\n\n`filter_N_cutoff`\n{}", "x\n".repeat(301)),
        )
        .unwrap();
        fails_with(&dir, "within 300");
    }

    #[test]
    fn a_vital_file_names_no_other_synth() {
        let dir = repo();
        let path = dir
            .path()
            .join("plugins/ref/skills/synth-reference/reference/vital/filter.md");
        fs::write(&path, "# F\n\n`filter_N_cutoff`, as in Serum.\n").unwrap();
        fails_with(&dir, "\"serum\"");
        fs::write(
            &path,
            "# F\n\n`filter_N_cutoff`, compared to the older version.\n",
        )
        .unwrap();
        fails_with(&dir, "\"older version\"");
    }

    #[test]
    fn a_probe_answer_must_be_unique_and_every_file_needs_a_probe() {
        let dir = repo();
        let other = dir
            .path()
            .join("plugins/ref/skills/synth-reference/reference/vital/space.md");
        fs::write(&other, "# Space\n\n`filter_N_cutoff` again\n").unwrap();
        fails_with(&dir, "must appear in that file only");
        fails_with(&dir, "space.md has no probe");
        fs::remove_file(&other).unwrap();
        fs::remove_file(
            dir.path()
                .join("plugins/ref/skills/synth-reference/reference/vital/filter.md"),
        )
        .unwrap();
        fails_with(&dir, "must appear in that file only");
    }

    #[test]
    fn a_marketplace_source_outside_the_repository_fails() {
        let dir = repo();
        fs::write(
            dir.path().join(".claude-plugin/marketplace.json"),
            r#"{"name":"m","owner":{"name":"o"},"plugins":[{"name":"ref","source":"../ref"}]}"#,
        )
        .unwrap();
        fails_with(&dir, "start it with `./`");
        fails_with(&dir, "is not listed");
    }
}
