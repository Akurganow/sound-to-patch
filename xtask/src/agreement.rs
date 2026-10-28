//! `cargo xtask agreement`: the listener agreement of the producer words.
//!
//! Fetches the EQ and reverb tables of the pinned listener data (source
//! `crowd-eq`) by SHA-256 into a folder outside the repository, runs `xtask/golden/word_agreement.py` with the
//! pinned tools and writes `dictionary/words/agreement.json`. The script
//! prints derived numbers only and fails when its output holds a source
//! row. The audio files of the dataset are never fetched. The Rust tests
//! recompute class and flag from the stored numbers, so CI needs no Python.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use crate::fetch::fetch;
use crate::golden::{GENERATOR_DIR, toolchain};

const REVISION: &str = "d7e266cac1526ae7db52ea16c744595a05b9df27";
const BASE: &str = "https://huggingface.co/datasets/seungheondoh/socialfx-original/resolve";
/// The table files and their SHA-256, the LFS object ids of the revision.
const TABLES: [(&str, &str); 2] = [
    (
        "eq-00000-of-00001.parquet",
        "43802a449efa8445285ce844719e3a20562750cbfbb5088eed46ada6659ad83a",
    ),
    (
        "reverb-00000-of-00001.parquet",
        "36a8e56839c30fd87a15b2f653235ba19b8d156bd437bb82b02a36ae0c75c66c",
    ),
];
const SCRIPT: &str = "word_agreement.py";
const OUT: &str = "dictionary/words/agreement.json";

pub(crate) fn run(root: &Path, data: &Path) -> crate::Result<()> {
    outside(root, data)?;
    let mut tables = Vec::new();
    for (file, sha256) in TABLES {
        let out = data.join(file);
        fetch(
            &format!("{BASE}/{REVISION}/data/{file}"),
            &sha256.parse()?,
            &out,
        )?;
        tables.push(out);
    }
    let python = toolchain(root)?;
    let output = Command::new(python)
        .arg("-I")
        .arg(root.join(GENERATOR_DIR).join(SCRIPT))
        .args(&tables)
        .current_dir(std::env::temp_dir())
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "{SCRIPT} failed with {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let report: Value = serde_json::from_slice(&output.stdout)?;
    fs::write(root.join(OUT), render(&report)?)?;
    Ok(())
}

/// Refuses a download folder inside the repository: the tables are third
/// party data and stay out of it.
pub(crate) fn outside(root: &Path, data: &Path) -> crate::Result<()> {
    let roots = [fs::canonicalize(root)?, std::path::absolute(root)?];
    let data = fs::canonicalize(data).or_else(|_| std::path::absolute(data))?;
    if roots.iter().any(|root| data.starts_with(root)) {
        return Err(format!(
            "{} lies inside the repository; pass a download folder outside it",
            data.display()
        )
        .into());
    }
    fs::create_dir_all(&data)?;
    Ok(())
}

/// The agreement file: the header fields, then one word per line, so a
/// diff shows which words changed.
fn render(report: &Value) -> crate::Result<String> {
    let mut text = String::from("{\n");
    text.push_str("  \"source\": \"crowd-eq\",\n");
    let _ = writeln!(text, "  \"revision\": \"{REVISION}\",");
    for key in [
        "tools",
        "eq_curves",
        "reverb_rows",
        "reverb_distinct_settings",
    ] {
        let value = report
            .get(key)
            .ok_or(format!("{SCRIPT} printed no `{key}`"))?;
        let _ = writeln!(text, "  \"{key}\": {},", serde_json::to_string(value)?);
    }
    for (index, key) in ["eq", "reverb"].into_iter().enumerate() {
        let words = report[key]
            .as_array()
            .ok_or(format!("{SCRIPT} printed no `{key}` list"))?;
        let lines = words
            .iter()
            .map(|word| serde_json::to_string(word).map(|line| format!("    {line}")))
            .collect::<Result<Vec<_>, _>>()?;
        let comma = if index == 0 { "," } else { "" };
        let _ = writeln!(
            text,
            "  \"{key}\": [
{}
  ]{comma}",
            lines.join(
                ",
"
            )
        );
    }
    text.push_str("}\n");
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_one_word_per_line_and_parses_back() {
        let report = serde_json::json!({
            "tools": {"python": "3.14.7"},
            "eq_curves": 3, "reverb_rows": 6, "reverb_distinct_settings": 2,
            "eq": [{"word": "dark", "n": 8, "value": 0.57}, {"word": "warm", "n": 64, "value": 0.25}],
            "reverb": [{"word": "wide", "n": 62}],
        });
        let text = render(&report).unwrap();
        assert!(
            text.contains("\n    {\"word\":\"dark\",\"n\":8,\"value\":0.57},\n"),
            "{text}"
        );
        let back: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back["eq"], report["eq"]);
        assert_eq!(back["revision"], REVISION);
    }

    #[test]
    fn a_download_folder_inside_the_repository_is_refused() {
        let root = tempfile::tempdir().unwrap();
        let err = outside(root.path(), &root.path().join("data")).unwrap_err();
        assert!(err.to_string().contains("inside the repository"), "{err}");
        let elsewhere = tempfile::tempdir().unwrap();
        assert!(outside(root.path(), elsewhere.path()).is_ok());
    }

    #[test]
    fn a_report_without_its_lists_is_refused() {
        let report = serde_json::json!({
            "tools": {}, "eq_curves": 0, "reverb_rows": 0, "reverb_distinct_settings": 0,
        });
        assert!(render(&report).unwrap_err().to_string().contains("`eq`"));
    }
}
