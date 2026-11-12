//! Golden numbers: [`generate`] runs the pinned Python generator, and
//! [`check`] verifies the committed vectors against the pins without
//! Python.
//!
//! The generator lives in `xtask/golden/`. `.python-version` pins the
//! interpreter and `requirements.txt` pins every package with its hashes.
//! Each vector under `tests/fixtures/golden/` records the tool versions that
//! made it and the SHA-256 of its script and of `requirements.txt`, so a
//! change of a recipe or a pin without a regeneration fails the check.
//! [`VECTORS`] names every vector, so a deleted or an orphaned one fails
//! too.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;
use sound_to_patch::music::chord::parse_chord;
use sound_to_patch::music::dictionary;
use sound_to_patch::music::entry::Group;

/// The fields every vector carries.
const FIELDS: [&str; 9] = [
    "name",
    "issue",
    "description",
    "generator",
    "sources",
    "tools",
    "recipe",
    "values",
    "tolerance",
];

/// Every vector under `tests/fixtures/golden/`, without `.json`: the keys of
/// `VECTORS` in `generate.py`, then the chord oracles. The prototype tables
/// are the figures that task issues quote, so none may rest on a script
/// that is gone.
const VECTORS: [&str; 29] = [
    "tail-fit",
    "window",
    "stft",
    "hilbert",
    "decay",
    "stereo",
    "descriptors",
    "lombscargle",
    "midi-timing",
    "notes-match",
    "pitch-track",
    "sub-band-line",
    "prototype/48-peak-accuracy",
    "prototype/53-noise-floor",
    "prototype/53-release-reverb",
    "prototype/53-drop-time",
    "prototype/47-stereo-screen",
    "prototype/51-two-saw-peaks",
    "prototype/54-sawtooth",
    "prototype/54-noise-flatness",
    "prototype/54-crest",
    "prototype/59-min-note-length",
    "prototype/61-glide",
    "prototype/66-glide",
    "prototype/67-vibrato",
    "prototype/67-drop-pyin",
    "prototype/69-attack",
    "prototype/71-pump",
    "music-oracles",
];

const GOLDEN_DIR: &str = "tests/fixtures/golden";
const MUSIC_ORACLE: &str = "music_oracle.py";
const MUSIC_VECTOR: &str = "music-oracles.json";
pub(crate) const GENERATOR_DIR: &str = "xtask/golden";
const REQUIREMENTS: &str = "xtask/golden/requirements.txt";
const TESTS: &str = "test_generate.py";
const VENV: &str = "target/golden-venv";

/// Installs the pinned tools into `target/golden-venv` with `uv`, runs the
/// generator's tests and the generator, which rewrites every vector, and
/// prints which values changed. Fails after the rewrite when a
/// `reproduced` flag flipped, so that no flip goes in unseen.
pub(crate) fn generate(root: &Path) -> crate::Result<()> {
    let generator = root.join(GENERATOR_DIR);
    let python = toolchain(root)?;
    let before = vectors(root)?;
    // `-B` leaves no bytecode cache in the repository.
    run(Command::new(&python)
        .args(["-I", "-B"])
        .arg(generator.join(TESTS)))?;
    run(Command::new(&python)
        .arg("-I")
        .arg(generator.join("generate.py"))
        .arg(root))?;
    music_oracles(root, &python, &generator)?;
    let changes = changes(&before, &vectors(root)?);
    for line in &changes.lines {
        println!("{line}");
    }
    check(root)?;
    if changes.flips.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "the vectors are written, but {} `reproduced` flags flipped; check each against its issue before you commit:\n{}",
            changes.flips.len(),
            changes.flips.join("\n")
        )
        .into())
    }
}

/// The SHA-256 of a text file with LF line ends, as the generator writes it.
fn source_hash(path: &Path) -> std::io::Result<String> {
    let text = fs::read(path)?;
    let mut lf = Vec::with_capacity(text.len());
    let mut bytes = text.iter().peekable();
    while let Some(&byte) = bytes.next() {
        if !(byte == b'\r' && bytes.peek() == Some(&&b'\n')) {
            lf.push(byte);
        }
    }
    Ok(crate::fetch::sha256_hex(&lf))
}

/// Every vector file by its name under `tests/fixtures/golden/`, without
/// `.json`.
fn vectors(root: &Path) -> crate::Result<BTreeMap<String, Value>> {
    let dir = root.join(GOLDEN_DIR);
    let mut files = Vec::new();
    if dir.is_dir() {
        collect(&dir, &mut files)?;
    }
    let mut out = BTreeMap::new();
    for file in files {
        let text = fs::read_to_string(&file)?;
        out.insert(vector_name(&dir, &file), serde_json::from_str(&text)?);
    }
    Ok(out)
}

fn vector_name(dir: &Path, file: &Path) -> String {
    file.strip_prefix(dir)
        .unwrap_or(file)
        .with_extension("")
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// What a regeneration changed: one line per changed vector and value, and
/// the `reproduced` flags that flipped.
#[derive(Debug, Default)]
struct Changes {
    lines: Vec<String>,
    flips: Vec<String>,
}

/// Compares the vectors before and after a regeneration. A changed hash in
/// `sources` alone is no change of a value, so it is counted, not listed.
fn changes(before: &BTreeMap<String, Value>, after: &BTreeMap<String, Value>) -> Changes {
    let mut out = Changes::default();
    let mut sources_only = 0;
    for name in before
        .keys()
        .chain(after.keys().filter(|k| !before.contains_key(*k)))
    {
        match (before.get(name), after.get(name)) {
            (Some(_), None) => out.lines.push(format!("{name}: removed")),
            (None, Some(_)) => out.lines.push(format!("{name}: new")),
            (Some(old), Some(new)) => {
                let mut paths = Vec::new();
                differ(old, new, String::new(), &mut paths);
                let (sources, values): (Vec<_>, Vec<_>) =
                    paths.into_iter().partition(|p| p.starts_with("/sources"));
                if values.is_empty() {
                    sources_only += usize::from(!sources.is_empty());
                    continue;
                }
                out.lines
                    .push(format!("{name}: {} values changed", values.len()));
                for path in values {
                    let (a, b) = (old.pointer(&path), new.pointer(&path));
                    out.lines
                        .push(format!("  {path}: {} -> {}", shown(a), shown(b)));
                    if path.ends_with("/reproduced") && a != b {
                        out.flips
                            .push(format!("{name}: {path}: {} -> {}", shown(a), shown(b)));
                    }
                }
            }
            (None, None) => {}
        }
    }
    if out.lines.is_empty() {
        out.lines.push("no value changed".to_owned());
    }
    if sources_only > 0 {
        out.lines.push(format!(
            "{sources_only} vectors changed only their source hashes"
        ));
    }
    out
}

fn shown(value: Option<&Value>) -> String {
    value.map_or_else(|| "(none)".to_owned(), Value::to_string)
}

/// The JSON pointers below `at` where `old` and `new` differ, down to the
/// leaves.
fn differ(old: &Value, new: &Value, at: String, out: &mut Vec<String>) {
    match (old, new) {
        (Value::Object(a), Value::Object(b)) => {
            for key in a.keys().chain(b.keys().filter(|k| !a.contains_key(*k))) {
                let at = format!("{at}/{}", key.replace('~', "~0").replace('/', "~1"));
                match (a.get(key), b.get(key)) {
                    (Some(x), Some(y)) => differ(x, y, at, out),
                    _ => out.push(at),
                }
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                differ(x, y, format!("{at}/{i}"), out);
            }
        }
        _ if old != new => out.push(at),
        _ => {}
    }
}

/// Installs the pinned tools into `target/golden-venv` and returns its
/// Python.
pub(crate) fn toolchain(root: &Path) -> crate::Result<PathBuf> {
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
    Ok(venv.join("bin/python"))
}

/// Runs the chord oracles on every chord spelling of the music dictionary
/// and writes which spellings agree. The oracle gets the spellings on stdin
/// and runs in the temp folder, so it reads no file of the repository, and
/// the vector holds symbol lists only, never an oracle row.
fn music_oracles(root: &Path, python: &Path, generator: &Path) -> crate::Result<()> {
    let request = serde_json::json!({ "chords": chord_request()? });
    let mut child = Command::new(python)
        .arg("-I")
        .arg(generator.join(MUSIC_ORACLE))
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("the oracle has no stdin")?
        .write_all(request.to_string().as_bytes())?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(format!("{MUSIC_ORACLE} failed with {}", output.status).into());
    }
    let report: Value = serde_json::from_slice(&output.stdout)?;
    let document = serde_json::json!({
        "name": "music-oracles",
        "issue": 264,
        "description": "Which chord spellings of the music dictionary get the same pitch classes from each oracle library, which differ and which it cannot parse.",
        "generator": format!("{GENERATOR_DIR}/{MUSIC_ORACLE}"),
        "sources": {
            format!("{GENERATOR_DIR}/{MUSIC_ORACLE}"): source_hash(&generator.join(MUSIC_ORACLE))?,
            REQUIREMENTS: source_hash(&root.join(REQUIREMENTS))?,
        },
        "tools": report["tools"],
        "recipe": { "input": "every chord spelling of dictionary/music/chords.json on root C, with the pitch classes of the dictionary's parser, passed on stdin" },
        "values": report["oracles"],
        "tolerance": { "abs": 0.0, "rel": 0.0 },
    });
    fs::write(
        root.join(GOLDEN_DIR).join(MUSIC_VECTOR),
        serde_json::to_string_pretty(&document)? + "\n",
    )?;
    Ok(())
}

/// Every chord spelling on root C with the pitch classes the dictionary's
/// parser gives it.
pub(crate) fn chord_request() -> crate::Result<Vec<Value>> {
    let mut chords = Vec::new();
    for entry in dictionary()?.group(Group::Chord) {
        for symbol in &entry.symbols {
            let text = format!("C{symbol}");
            let classes: Vec<u8> = parse_chord(&text)?.classes().into_iter().collect();
            chords.push(serde_json::json!({ "symbol": text, "classes": classes }));
        }
    }
    Ok(chords)
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
/// pin, was made from another version of its script or of the pins, or
/// holds a `reproduced` flag that disagrees with its rows; and when a
/// vector of [`VECTORS`] is missing or a file is in no recipe.
pub(crate) fn check(root: &Path) -> crate::Result<()> {
    let pins = pins(&root.join(GENERATOR_DIR))?;
    let dir = root.join(GOLDEN_DIR);
    let mut problems = Vec::new();
    let mut files = Vec::new();
    collect(&dir, &mut files)?;
    files.sort();
    let mut hashes = BTreeMap::new();
    for file in &files {
        let name = vector_name(&dir, file);
        let shown = format!("{GOLDEN_DIR}/{name}.json");
        if !VECTORS.contains(&name.as_str()) {
            problems.push(format!(
                "{shown} is made by no recipe: delete it, or add its recipe to {GENERATOR_DIR}/generate.py and its name to VECTORS in xtask/src/golden.rs"
            ));
        }
        match fs::read_to_string(file)
            .map_err(|err| err.to_string())
            .and_then(|text| serde_json::from_str::<Value>(&text).map_err(|err| err.to_string()))
        {
            Ok(vector) => problems.extend(
                vector_problems(&vector, &pins, root, &mut hashes)
                    .into_iter()
                    .map(|p| format!("{shown}: {p}")),
            ),
            Err(err) => problems.push(format!("{shown}: cannot read: {err}")),
        }
    }
    for name in VECTORS {
        if !dir.join(format!("{name}.json")).is_file() {
            problems.push(format!(
                "{GOLDEN_DIR}/{name}.json is missing: run `cargo xtask golden`, or drop its name from VECTORS"
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

fn vector_problems(
    vector: &Value,
    pins: &BTreeMap<String, String>,
    root: &Path,
    hashes: &mut BTreeMap<String, Option<String>>,
) -> Vec<String> {
    let mut problems: Vec<String> = FIELDS
        .iter()
        .filter(|field| vector.get(**field).is_none_or(Value::is_null))
        .map(|field| format!("lacks `{field}`"))
        .collect();
    if let Some(sources) = vector.get("sources") {
        problems.extend(source_problems(vector, sources, root, hashes));
    }
    flag_problems(vector, "", &mut problems);
    if let Some(tools) = vector.get("tools") {
        match tools.as_object() {
            Some(tools) if !tools.contains_key("python") => {
                problems.push("`tools` lacks the python version".to_owned());
            }
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

/// Whether `sources` names the vector's script and the pins, each with the
/// hash of the file as it is now.
fn source_problems(
    vector: &Value,
    sources: &Value,
    root: &Path,
    hashes: &mut BTreeMap<String, Option<String>>,
) -> Vec<String> {
    let Some(sources) = sources.as_object() else {
        return vec!["`sources` is not an object".to_owned()];
    };
    let mut problems = Vec::new();
    let script = vector
        .get("generator")
        .and_then(Value::as_str)
        .map(|g| g.split_once(':').map_or(g, |(file, _)| file));
    for needed in script.into_iter().chain([REQUIREMENTS]) {
        if !sources.contains_key(needed) {
            problems.push(format!("`sources` lacks {needed}"));
        }
    }
    for (file, stored) in sources {
        let now = hashes
            .entry(file.clone())
            .or_insert_with(|| source_hash(&root.join(file)).ok());
        match (now, stored.as_str()) {
            (Some(now), Some(stored)) if now == stored => {}
            (Some(_), _) => problems.push(format!(
                "made from another version of {file}: run `cargo xtask golden`"
            )),
            (None, _) => problems.push(format!("names {file}, which cannot be read")),
        }
    }
    problems
}

/// Every `reproduced` flag must be a boolean, and a flag beside `rows`
/// must be true exactly when every row's flag is: a row is a boolean or an
/// object with its own `reproduced`.
fn flag_problems(value: &Value, at: &str, problems: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let Some(flag) = map.get("reproduced") {
                match (flag.as_bool(), map.get("rows")) {
                    (None, _) => problems.push(format!("{at}/reproduced is not a boolean")),
                    (Some(flag), Some(rows)) => {
                        let row_flags: Vec<Option<bool>> = match rows {
                            Value::Array(rows) => rows.iter().map(row_flag).collect(),
                            Value::Object(rows) => rows.values().map(row_flag).collect(),
                            _ => vec![None],
                        };
                        if row_flags.iter().any(Option::is_none) {
                            problems
                                .push(format!("{at}/rows holds a row with no `reproduced` flag"));
                        } else if row_flags.iter().all(|r| *r == Some(true)) != flag {
                            problems.push(format!(
                                "{at}/reproduced is {flag}, but its rows say otherwise"
                            ));
                        }
                    }
                    (Some(_), None) => {}
                }
            }
            for (key, child) in map {
                flag_problems(child, &format!("{at}/{key}"), problems);
            }
        }
        Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                flag_problems(child, &format!("{at}/{i}"), problems);
            }
        }
        _ => {}
    }
}

fn row_flag(row: &Value) -> Option<bool> {
    row.as_bool()
        .or_else(|| row.get("reproduced").and_then(Value::as_bool))
}

#[cfg(test)]
mod tests {
    use super::*;

    const VECTOR: &str = r#"{"name": "x", "issue": 48, "description": "d", "generator": "GENERATOR",
        "sources": SOURCES,
        "tools": {"python": "3.14.7", "numpy": "2.5.4"}, "recipe": {}, "values": [1.0],
        "tolerance": {"abs": 1e-9, "rel": 0.0}}"#;

    /// A vector made by `script` from the files of `root` as they are.
    fn vector(root: &Path, script: &str) -> String {
        let sources = serde_json::json!({
            script: source_hash(&root.join(script)).unwrap(),
            REQUIREMENTS: source_hash(&root.join(REQUIREMENTS)).unwrap(),
        });
        VECTOR
            .replace("GENERATOR", &format!("{script}:f"))
            .replace("SOURCES", &sources.to_string())
    }

    /// A tree with every vector of [`VECTORS`], made from its files, and
    /// `stft.json` replaced by `edit` of it.
    fn tree_with(edit: impl Fn(String) -> String) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let generator = dir.path().join(GENERATOR_DIR);
        fs::create_dir_all(&generator).unwrap();
        fs::write(generator.join(".python-version"), "3.14.7\n").unwrap();
        fs::write(
            generator.join("requirements.txt"),
            "numpy==2.5.4 \\\n    --hash=sha256:00\n# via x\nscipy==1.18.1 \\\n",
        )
        .unwrap();
        fs::write(generator.join("generate.py"), "PEAK = 300\n").unwrap();
        fs::write(generator.join(MUSIC_ORACLE), "ORACLE = 1\n").unwrap();
        let golden = dir.path().join(GOLDEN_DIR);
        fs::create_dir_all(golden.join("prototype")).unwrap();
        for name in VECTORS {
            let script = if name == "music-oracles" {
                format!("{GENERATOR_DIR}/{MUSIC_ORACLE}")
            } else {
                format!("{GENERATOR_DIR}/generate.py")
            };
            let mut text = vector(dir.path(), &script);
            if name == "stft" {
                text = edit(text);
            }
            fs::write(golden.join(format!("{name}.json")), text).unwrap();
        }
        dir
    }

    fn tree(stft: &str) -> tempfile::TempDir {
        let stft = stft.to_owned();
        tree_with(move |made| if stft.is_empty() { made } else { stft.clone() })
    }

    fn edited(from: &str, to: &str) -> tempfile::TempDir {
        let (from, to) = (from.to_owned(), to.to_owned());
        tree_with(move |made| {
            assert!(made.contains(&from), "{from}");
            made.replace(&from, &to)
        })
    }

    fn error(dir: &tempfile::TempDir) -> String {
        check(dir.path()).unwrap_err().to_string()
    }

    #[test]
    fn passes_when_every_vector_matches_the_pins() {
        check(tree("").path()).unwrap();
    }

    #[test]
    fn fails_on_a_tool_version_other_than_the_pin() {
        let err = error(&edited("2.5.4", "2.5.3"));
        assert!(
            err.contains("stft.json: made with numpy 2.5.3, but the pin is 2.5.4"),
            "{err}"
        );
    }

    #[test]
    fn fails_on_tools_without_the_python_version() {
        for tools in ["{}", r#"{"numpy": "2.5.4"}"#] {
            let err = error(&edited(r#"{"python": "3.14.7", "numpy": "2.5.4"}"#, tools));
            assert!(
                err.contains("stft.json: `tools` lacks the python version"),
                "{err}"
            );
        }
    }

    #[test]
    fn fails_on_a_missing_field_and_names_it() {
        let err = error(&edited(r#""recipe": {},"#, ""));
        assert!(err.contains("stft.json: lacks `recipe`"), "{err}");
    }

    #[test]
    fn fails_on_an_unpinned_tool_and_a_bad_tolerance() {
        let dir = tree_with(|made| {
            made.replace(r#""numpy": "2.5.4""#, r#""torch": "2.0""#)
                .replace("1e-9", "-1")
        });
        let err = error(&dir);
        assert!(err.contains("names torch, which has no pin"), "{err}");
        assert!(
            err.contains("`tolerance.abs` is not a number of 0 or more"),
            "{err}"
        );
    }

    #[test]
    fn fails_when_a_recipe_changes_without_a_regeneration() {
        let dir = tree("");
        fs::write(
            dir.path().join(GENERATOR_DIR).join("generate.py"),
            "PEAK = 301\n",
        )
        .unwrap();
        let err = error(&dir);
        assert!(
            err.contains("prototype/48-peak-accuracy.json: made from another version of xtask/golden/generate.py"),
            "{err}"
        );
        // The chord oracles come from another script.
        assert!(!err.contains("music-oracles.json"), "{err}");
    }

    #[test]
    fn fails_when_a_pin_changes_without_a_regeneration() {
        let dir = tree("");
        let requirements = dir.path().join(REQUIREMENTS);
        let text = fs::read_to_string(&requirements).unwrap();
        fs::write(&requirements, text.replace("sha256:00", "sha256:01")).unwrap();
        let err = error(&dir);
        assert!(
            err.contains(
                "music-oracles.json: made from another version of xtask/golden/requirements.txt"
            ),
            "{err}"
        );
    }

    #[test]
    fn a_crlf_checkout_hashes_as_its_lf_text() {
        let dir = tree("");
        let script = dir.path().join(GENERATOR_DIR).join("generate.py");
        fs::write(&script, "PEAK = 300\r\n").unwrap();
        check(dir.path()).unwrap();
    }

    #[test]
    fn fails_when_sources_lack_the_script() {
        let err = error(&edited(
            "\"xtask/golden/generate.py\":",
            "\"xtask/golden/other.py\":",
        ));
        assert!(
            err.contains("stft.json: `sources` lacks xtask/golden/generate.py"),
            "{err}"
        );
        assert!(
            err.contains("names xtask/golden/other.py, which cannot be read"),
            "{err}"
        );
    }

    #[test]
    fn fails_when_a_listed_vector_is_missing() {
        let dir = tree("");
        fs::remove_file(dir.path().join(GOLDEN_DIR).join("stft.json")).unwrap();
        fs::remove_file(
            dir.path()
                .join(GOLDEN_DIR)
                .join("prototype/53-drop-time.json"),
        )
        .unwrap();
        let err = error(&dir);
        assert!(err.contains("golden/stft.json is missing"), "{err}");
        assert!(
            err.contains("prototype/53-drop-time.json is missing"),
            "{err}"
        );
    }

    #[test]
    fn fails_on_a_vector_that_no_recipe_makes() {
        let dir = tree("");
        let made = fs::read_to_string(dir.path().join(GOLDEN_DIR).join("stft.json")).unwrap();
        fs::write(
            dir.path().join(GOLDEN_DIR).join("prototype/50-gone.json"),
            made,
        )
        .unwrap();
        let err = error(&dir);
        assert!(
            err.contains("prototype/50-gone.json is made by no recipe"),
            "{err}"
        );
    }

    #[test]
    fn fails_on_a_reproduced_flag_that_its_rows_contradict() {
        let figures =
            r#""issue_figures": {"rows": [true, {"reproduced": false}], "reproduced": true},"#;
        let err = error(&edited(
            r#""recipe": {},"#,
            &format!(r#""recipe": {{}}, {figures}"#),
        ));
        assert!(
            err.contains(
                "stft.json: /issue_figures/reproduced is true, but its rows say otherwise"
            ),
            "{err}"
        );
        let figures = r#""issue_figures": {"rows": {"a": true}, "reproduced": "yes"},"#;
        let err = error(&edited(
            r#""recipe": {},"#,
            &format!(r#""recipe": {{}}, {figures}"#),
        ));
        assert!(
            err.contains("/issue_figures/reproduced is not a boolean"),
            "{err}"
        );
    }

    #[test]
    fn a_regeneration_lists_changed_values_and_flipped_flags() {
        let old = serde_json::json!({
            "sources": {"g": "1"},
            "values": {"bound": 0.5, "same": [1, 2]},
            "issue_figures": {"rows": [true], "reproduced": true},
        });
        let mut new = old.clone();
        new["sources"]["g"] = "2".into();
        new["values"]["bound"] = 0.7.into();
        new["issue_figures"]["reproduced"] = false.into();
        let mut hashed = old.clone();
        hashed["sources"]["g"] = "2".into();
        let before = BTreeMap::from([
            ("a".to_owned(), old.clone()),
            ("b".to_owned(), old.clone()),
            ("gone".to_owned(), old),
        ]);
        let after = BTreeMap::from([("a".to_owned(), new), ("b".to_owned(), hashed)]);
        let changes = changes(&before, &after);
        assert_eq!(
            changes.lines,
            [
                "a: 2 values changed",
                "  /values/bound: 0.5 -> 0.7",
                "  /issue_figures/reproduced: true -> false",
                "gone: removed",
                "1 vectors changed only their source hashes",
            ]
        );
        assert_eq!(
            changes.flips,
            ["a: /issue_figures/reproduced: true -> false"]
        );
    }

    #[test]
    fn the_vector_list_holds_the_vectors_of_generate_py() {
        let script =
            fs::read_to_string(crate::root().join(GENERATOR_DIR).join("generate.py")).unwrap();
        let table = script
            .split_once("\nVECTORS = {\n")
            .and_then(|(_, rest)| rest.split_once("\n}\n"))
            .unwrap()
            .0;
        let names: Vec<&str> = table
            .lines()
            .filter_map(|line| line.trim().split('"').nth(1))
            .collect();
        assert_eq!(names, VECTORS[..VECTORS.len() - 1]);
        assert_eq!(VECTORS[VECTORS.len() - 1], "music-oracles");
    }

    #[test]
    fn the_peak_bound_is_the_cramer_rao_bound_of_its_recipe() {
        // var(f) >= 12 / ((2 pi)^2 SNR N (N^2 - 1)) cycles^2 per sample^2,
        // with SNR = A^2 / (2 sigma^2) as the recipe defines it.
        let golden: Value = serde_json::from_str(
            &fs::read_to_string(
                crate::root()
                    .join(GOLDEN_DIR)
                    .join("prototype/48-peak-accuracy.json"),
            )
            .unwrap(),
        )
        .unwrap();
        let recipe = &golden["recipe"];
        assert_eq!(recipe["snr_definition"], "A^2 / (2 sigma^2)");
        let snr = 10f64.powf(recipe["snr_db"].as_f64().unwrap() / 10.0);
        let n = recipe["frame_len"].as_f64().unwrap();
        let rate = recipe["sample_rate"].as_f64().unwrap();
        let bound =
            (12.0 / ((std::f64::consts::TAU).powi(2) * snr * n * (n * n - 1.0))).sqrt() * rate;
        let stored = golden["values"]["bound_std_hz"].as_f64().unwrap();
        assert!((stored / bound - 1.0).abs() < 1e-9, "{stored} vs {bound}");
    }

    #[test]
    fn the_pump_table_recovers_to_the_layer_threshold() {
        let golden: Value = serde_json::from_str(
            &fs::read_to_string(
                crate::root()
                    .join(GOLDEN_DIR)
                    .join("prototype/71-pump.json"),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            golden["recipe"]["back_db"].as_f64(),
            Some(sound_to_patch::estimate::envelope::PUMP_BACK.get())
        );
    }

    #[test]
    fn the_committed_vectors_match_the_pins() {
        check(crate::root()).unwrap();
    }
}
