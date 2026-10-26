//! Golden numbers: [`generate`] runs the pinned Python generator, and
//! [`check`] verifies the committed vectors against the pins without
//! Python.
//!
//! The generator lives in `xtask/golden/`. `.python-version` pins the
//! interpreter and `requirements.txt` pins every package with its hashes.
//! Each vector under `tests/fixtures/golden/` records the tool versions that
//! made it, so a change of pins without a regeneration fails the check.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// The fields every vector carries.
const FIELDS: [&str; 8] = [
    "name",
    "issue",
    "description",
    "generator",
    "tools",
    "recipe",
    "values",
    "tolerance",
];

/// The prototype tables that the stem-intake tasks quote. Each needs a
/// recipe file, so no quoted figure rests on a script that is gone.
const PROTOTYPE_TABLES: [&str; 9] = [
    "47-stereo-screen",
    "48-peak-accuracy",
    "51-two-saw-peaks",
    "53-drop-time",
    "53-noise-floor",
    "53-release-reverb",
    "54-crest",
    "54-noise-flatness",
    "54-sawtooth",
];

const GOLDEN_DIR: &str = "tests/fixtures/golden";
const GENERATOR_DIR: &str = "xtask/golden";
const VENV: &str = "target/golden-venv";

/// Installs the pinned tools into `target/golden-venv` with `uv` and runs the
/// generator, which rewrites every vector.
pub(crate) fn generate(root: &Path) -> crate::Result<()> {
    let generator = root.join(GENERATOR_DIR);
    let python = fs::read_to_string(generator.join(".python-version"))?;
    let venv = root.join(VENV);
    run(Command::new("uv")
        .args(["venv", "--allow-existing", "--python", python.trim()])
        .arg(&venv))?;
    run(Command::new("uv")
        .args(["pip", "sync", "--require-hashes", "--python"])
        .arg(venv.join("bin/python"))
        .arg(generator.join("requirements.txt")))?;
    run(Command::new(venv.join("bin/python"))
        .arg("-I")
        .arg(generator.join("generate.py"))
        .arg(root))?;
    check(root)
}

fn run(command: &mut Command) -> crate::Result<()> {
    let status = command.status().map_err(|err| {
        format!(
            "cannot run {}: {err}. Install uv from https://docs.astral.sh/uv/",
            command.get_program().display()
        )
    })?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{command:?} failed with {status}").into())
    }
}

/// Fails when a vector lacks a field, names a tool version other than the
/// pin, or when a prototype table has no recipe file.
pub(crate) fn check(root: &Path) -> crate::Result<()> {
    let pins = pins(&root.join(GENERATOR_DIR))?;
    let dir = root.join(GOLDEN_DIR);
    let mut problems = Vec::new();
    let mut files = Vec::new();
    collect(&dir, &mut files)?;
    files.sort();
    for file in &files {
        let shown = file
            .strip_prefix(root)
            .unwrap_or(file)
            .display()
            .to_string();
        match fs::read_to_string(file)
            .map_err(|err| err.to_string())
            .and_then(|text| serde_json::from_str::<Value>(&text).map_err(|err| err.to_string()))
        {
            Ok(vector) => problems.extend(
                vector_problems(&vector, &pins)
                    .into_iter()
                    .map(|p| format!("{shown}: {p}")),
            ),
            Err(err) => problems.push(format!("{shown}: cannot read: {err}")),
        }
    }
    for table in PROTOTYPE_TABLES {
        let path = dir.join("prototype").join(format!("{table}.json"));
        if !path.is_file() {
            problems.push(format!(
                "{GOLDEN_DIR}/prototype/{table}.json is missing: add its recipe to {GENERATOR_DIR}/generate.py and run `cargo xtask golden`"
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("\n").into())
    }
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) -> crate::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect(&path, files)?;
        } else if path.extension().is_some_and(|ext| ext == "json") {
            files.push(path);
        }
    }
    Ok(())
}

/// The pinned versions: `python` from `.python-version`, each package from
/// its `name==version` line in `requirements.txt`.
fn pins(generator: &Path) -> crate::Result<BTreeMap<String, String>> {
    let mut pins = BTreeMap::new();
    let python = fs::read_to_string(generator.join(".python-version"))?;
    pins.insert("python".to_owned(), python.trim().to_owned());
    let requirements = fs::read_to_string(generator.join("requirements.txt"))?;
    for line in requirements.lines() {
        let spec = line.trim_end_matches('\\').trim();
        if let Some((name, version)) = spec.split_once("==") {
            pins.insert(name.trim().to_ascii_lowercase(), version.trim().to_owned());
        }
    }
    Ok(pins)
}

fn vector_problems(vector: &Value, pins: &BTreeMap<String, String>) -> Vec<String> {
    let mut problems: Vec<String> = FIELDS
        .iter()
        .filter(|field| vector.get(**field).is_none_or(Value::is_null))
        .map(|field| format!("lacks `{field}`"))
        .collect();
    if let Some(tools) = vector.get("tools") {
        match tools.as_object() {
            Some(tools) => {
                for (tool, version) in tools {
                    match (pins.get(tool), version.as_str()) {
                        (Some(pin), Some(version)) if pin == version => {}
                        (Some(pin), shown) => problems.push(format!(
                            "made with {tool} {}, but the pin is {pin}: run `cargo xtask golden`",
                            shown.unwrap_or("of no stated version")
                        )),
                        (None, _) => problems.push(format!("names {tool}, which has no pin")),
                    }
                }
            }
            None => problems.push("`tools` is not an object".to_owned()),
        }
    }
    if let Some(tolerance) = vector.get("tolerance") {
        for bound in ["abs", "rel"] {
            if !tolerance
                .get(bound)
                .and_then(Value::as_f64)
                .is_some_and(|v| v >= 0.0)
            {
                problems.push(format!("`tolerance.{bound}` is not a number of 0 or more"));
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    const VECTOR: &str = r#"{"name": "x", "issue": 48, "description": "d", "generator": "g",
        "tools": {"python": "3.14.7", "numpy": "2.5.4"}, "recipe": {}, "values": [1.0],
        "tolerance": {"abs": 1e-9, "rel": 0.0}}"#;

    fn tree(vector: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let generator = dir.path().join(GENERATOR_DIR);
        fs::create_dir_all(&generator).unwrap();
        fs::write(generator.join(".python-version"), "3.14.7\n").unwrap();
        fs::write(
            generator.join("requirements.txt"),
            "numpy==2.5.4 \\\n    --hash=sha256:00\n# via x\nscipy==1.18.1 \\\n",
        )
        .unwrap();
        let prototype = dir.path().join(GOLDEN_DIR).join("prototype");
        fs::create_dir_all(&prototype).unwrap();
        for table in PROTOTYPE_TABLES {
            fs::write(prototype.join(format!("{table}.json")), VECTOR).unwrap();
        }
        fs::write(dir.path().join(GOLDEN_DIR).join("stft.json"), vector).unwrap();
        dir
    }

    #[test]
    fn passes_when_every_vector_matches_the_pins() {
        check(tree(VECTOR).path()).unwrap();
    }

    #[test]
    fn fails_on_a_tool_version_other_than_the_pin() {
        let dir = tree(&VECTOR.replace("2.5.4", "2.5.3"));
        let err = check(dir.path()).unwrap_err().to_string();
        assert!(
            err.contains("stft.json: made with numpy 2.5.3, but the pin is 2.5.4"),
            "{err}"
        );
    }

    #[test]
    fn fails_on_a_missing_field_and_names_it() {
        let dir = tree(&VECTOR.replace(r#""recipe": {},"#, ""));
        let err = check(dir.path()).unwrap_err().to_string();
        assert!(err.contains("stft.json: lacks `recipe`"), "{err}");
    }

    #[test]
    fn fails_on_an_unpinned_tool_and_a_bad_tolerance() {
        let dir = tree(
            &VECTOR
                .replace(r#""numpy": "2.5.4""#, r#""torch": "2.0""#)
                .replace("1e-9", "-1"),
        );
        let err = check(dir.path()).unwrap_err().to_string();
        assert!(err.contains("names torch, which has no pin"), "{err}");
        assert!(
            err.contains("`tolerance.abs` is not a number of 0 or more"),
            "{err}"
        );
    }

    #[test]
    fn fails_when_a_prototype_table_has_no_recipe_file() {
        let dir = tree(VECTOR);
        fs::remove_file(
            dir.path()
                .join(GOLDEN_DIR)
                .join("prototype/53-drop-time.json"),
        )
        .unwrap();
        let err = check(dir.path()).unwrap_err().to_string();
        assert!(
            err.contains("prototype/53-drop-time.json is missing"),
            "{err}"
        );
    }

    #[test]
    fn the_committed_vectors_match_the_pins() {
        check(crate::root()).unwrap();
    }
}
