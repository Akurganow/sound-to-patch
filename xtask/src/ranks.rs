//! `cargo xtask word-ranks`: the ranked producer describing words.
//!
//! Fetches the pinned public inputs by SHA-256 into a folder outside the
//! repository, runs `xtask/golden/word_ranks.py` with the pinned tools and
//! writes `dictionary/words/ranks.json`. The script prints counts of the
//! lexicon words only and fails when its output holds any other string. The
//! Rust tests check the pins and the committed file, so CI needs no Python
//! and no download.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use crate::agreement::outside;
use crate::fetch::fetch;
use crate::golden::{GENERATOR_DIR, toolchain};

const SCRIPT: &str = "word_ranks.py";
const LEXICON: &str = "dictionary/words/lexicon.json";
const OUT: &str = "dictionary/words/ranks.json";

/// One pinned download. `source` is the id in `dictionary/sources.json`;
/// a source may need several files.
struct Input {
    source: &'static str,
    /// File name inside the download folder.
    file: &'static str,
    url: &'static str,
    /// The commit, sha or record revision the URL is pinned to.
    revision: &'static str,
    /// SHA-256 of the file as measured when the pin was set.
    sha256: &'static str,
}

const BANK_A: &str = "cabec31f5a750b3ac7d0ae1be59d32bef4bbf918";
const BANK_B: &str = "c821ad99fb4edbf6fc92967a198db784e034b2d2";
const BANK_C: &str = "a30341be00484d3de6b65869998100ccc6db6945";
const BANK_D: &str = "265e9e227581357efb87ed7217381f8a40c5e2bf";
const BANK_E: &str = "5fafa709315a25ddc3b9925f8471f84b7121e2fc";
const SOUND_TAGS: &str = "5c427d91b3d6a7624941149ece874850b222c838";
const CAPTIONS: &str = "0a51889b340037bb75a9a0858af2e4ece21f7f89";
const BANK_F: &str = "record revision 2";

const INPUTS: [Input; 13] = [
    Input {
        source: "bank-a",
        file: "bank-a.json",
        url: "https://api.github.com/repos/HermannPR/serum2-neurosync/git/trees/cabec31f5a750b3ac7d0ae1be59d32bef4bbf918?recursive=1",
        revision: BANK_A,
        sha256: "dd7d48b26250e8a3a293103f33c4a7bb232bf1cde123455fa1dfd21bd476d376",
    },
    Input {
        source: "bank-b",
        file: "bank-b.tar.gz",
        url: "https://codeload.github.com/ArcerionDev/vital_discord_preset_archive/tar.gz/c821ad99fb4edbf6fc92967a198db784e034b2d2",
        revision: BANK_B,
        sha256: "cf5b9e678550c375093ea1bf38360bd83308643b4e6744c7885373aa6b39b0f2",
    },
    Input {
        source: "bank-c",
        file: "bank-c.tar.gz",
        url: "https://codeload.github.com/surge-synthesizer/surge/tar.gz/a30341be00484d3de6b65869998100ccc6db6945",
        revision: BANK_C,
        sha256: "cd87db6a1e48cf372cec0a7d5fe1dbd4e2a02be425b69256b6ef06fa047a125a",
    },
    Input {
        source: "bank-d",
        file: "bank-d.json",
        url: "https://api.github.com/repos/TheWaveWarden/odin2/git/trees/265e9e227581357efb87ed7217381f8a40c5e2bf?recursive=1",
        revision: BANK_D,
        sha256: "d270691db672f863fbcddfdf51fbef9637884b227870a1576479b3ab22095d34",
    },
    Input {
        source: "bank-e",
        file: "bank-e.json",
        url: "https://api.github.com/repos/mtytel/helm_patches/git/trees/5fafa709315a25ddc3b9925f8471f84b7121e2fc?recursive=1",
        revision: BANK_E,
        sha256: "ef0e10ef42e9057f94932bcd73211573f88aeec6cd7055c7e377845f9ff389ad",
    },
    Input {
        source: "bank-f",
        file: "bank-f.train-name.npy",
        url: "https://zenodo.org/records/7686668/files/train_name.npy?download=1",
        revision: BANK_F,
        sha256: "528d5745837833e9998a425633ed8a7193449c584a1ca4bab8e0f9c133d0fe18",
    },
    Input {
        source: "bank-f",
        file: "bank-f.train-synth.npy",
        url: "https://zenodo.org/records/7686668/files/train_synth.npy?download=1",
        revision: BANK_F,
        sha256: "abde9142c44ebbf58e69e3cc8feec53c41adb829be48173d71afe9b579559dd9",
    },
    Input {
        source: "bank-f",
        file: "bank-f.valid-name.npy",
        url: "https://zenodo.org/records/7686668/files/valid_name.npy?download=1",
        revision: BANK_F,
        sha256: "011104369c19004afbd04646d255925d3d7bbb90cbae6f2c4b74005acfb5043b",
    },
    Input {
        source: "bank-f",
        file: "bank-f.valid-synth.npy",
        url: "https://zenodo.org/records/7686668/files/valid_synth.npy?download=1",
        revision: BANK_F,
        sha256: "8fbaba7db165ea46e465fe117822d7e37c4d90f07fb528078103de2f11e59a00",
    },
    Input {
        source: "bank-f",
        file: "bank-f.test-name.npy",
        url: "https://zenodo.org/records/7686668/files/test_name.npy?download=1",
        revision: BANK_F,
        sha256: "8882fe0ea98f1f7c98b1b37d2d821729900ec37faafffa117df4b56558a5ed58",
    },
    Input {
        source: "bank-f",
        file: "bank-f.test-synth.npy",
        url: "https://zenodo.org/records/7686668/files/test_synth.npy?download=1",
        revision: BANK_F,
        sha256: "02fd7b89c29e8a51b136a2e27c73228ccac9cabc86812b6c1385211e7e822350",
    },
    Input {
        source: "sound-tags",
        file: "sound-tags.parquet",
        url: "https://huggingface.co/datasets/Chr0my/freesound.org/resolve/5c427d91b3d6a7624941149ece874850b222c838/freesound_parquet.parquet",
        revision: SOUND_TAGS,
        sha256: "fdc81269e2ddcf0bffffc05f01c55509d9c772f43b90f6d96fac325f9facdc8d",
    },
    Input {
        source: "captions",
        file: "captions.csv",
        url: "https://huggingface.co/datasets/google/MusicCaps/resolve/0a51889b340037bb75a9a0858af2e4ece21f7f89/musiccaps-public.csv",
        revision: CAPTIONS,
        sha256: "8cc109ac507b5e5aed8ea73a5c8f897a9d0793f7397427e000adf4c05f6021c7",
    },
];

/// Backs `cargo xtask word-ranks --data <folder>`: fetches the inputs into
/// `data`, recomputes the ranks and rewrites `dictionary/words/ranks.json`.
///
/// Three banks are counted by file name only, so their pin is the directory
/// listing of the commit and not the archive, which keeps the download
/// small. `data` must lie outside the repository.
pub(crate) fn run(root: &Path, data: &Path) -> crate::Result<()> {
    outside(root, data)?;
    for input in &INPUTS {
        fetch(input.url, &input.sha256.parse()?, &data.join(input.file))?;
    }
    let python = toolchain(root)?;
    let output = Command::new(python)
        .arg("-I")
        .arg(root.join(GENERATOR_DIR).join(SCRIPT))
        .args(script_args(root, data))
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
    fs::write(root.join(OUT), render(&report, &INPUTS)?)?;
    Ok(())
}

/// The script's options: the lexicon, then one option per source with the
/// files of that source in the order the script reads them.
fn script_args(root: &Path, data: &Path) -> Vec<std::ffi::OsString> {
    let mut args = vec!["--lexicon".into(), root.join(LEXICON).into()];
    let mut sources: Vec<&str> = INPUTS.iter().map(|input| input.source).collect();
    sources.dedup();
    for source in sources {
        args.push(format!("--{source}").into());
        args.extend(
            INPUTS
                .iter()
                .filter(|input| input.source == source)
                .map(|input| data.join(input.file).into_os_string()),
        );
    }
    args
}

/// The ranks file: the pins, the group sizes, then one word per line, so a
/// diff shows which words moved.
fn render(report: &Value, inputs: &[Input]) -> crate::Result<String> {
    let field = |key: &str| {
        report
            .get(key)
            .ok_or_else(|| format!("{SCRIPT} printed no `{key}`"))
    };
    let mut text = String::from("{\n  \"inputs\": [\n");
    let pins: Vec<String> = inputs
        .iter()
        .map(|input| {
            let pin = serde_json::json!({
                "source": input.source,
                "revision": input.revision,
                "sha256": input.sha256,
            });
            serde_json::to_string(&pin).map(|line| format!("    {line}"))
        })
        .collect::<Result<_, _>>()?;
    text.push_str(&pins.join(",\n"));
    text.push_str("\n  ],\n");
    for key in [
        "tools",
        "presets",
        "banks",
        "sounds",
        "uploaders",
        "captions",
    ] {
        let _ = writeln!(
            text,
            "  \"{key}\": {},",
            serde_json::to_string(field(key)?)?
        );
    }
    for (index, key) in ["describing", "style", "technique"].into_iter().enumerate() {
        let words = field(key)?
            .as_array()
            .ok_or_else(|| format!("{SCRIPT} printed no `{key}` list"))?;
        let lines: Vec<String> = words
            .iter()
            .map(|word| serde_json::to_string(word).map(|line| format!("    {line}")))
            .collect::<Result<_, _>>()?;
        let comma = if index == 2 { "" } else { "," };
        let _ = writeln!(text, "  \"{key}\": [\n{}\n  ]{comma}", lines.join(",\n"));
    }
    text.push_str("}\n");
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> Value {
        serde_json::json!({
            "tools": {"python": "3.14.7"},
            "presets": 2, "banks": {"bank-e": 2}, "sounds": 3, "uploaders": 2, "captions": 1,
            "describing": [
                {"word": "simple", "preset_hits": 2, "rank": 1},
                {"word": "analog", "preset_hits": 1, "rank": 2},
            ],
            "style": [{"label": "house", "preset_hits": 1}],
            "technique": [{"label": "noise", "preset_hits": 1}],
        })
    }

    #[test]
    fn renders_one_word_per_line_and_parses_back() {
        let text = render(&report(), &INPUTS).unwrap();
        assert!(
            text.contains("\n    {\"word\":\"simple\",\"preset_hits\":2,\"rank\":1},\n"),
            "{text}"
        );
        let back: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back["describing"], report()["describing"]);
        assert_eq!(back["inputs"].as_array().unwrap().len(), INPUTS.len());
    }

    #[test]
    fn a_report_without_a_list_is_refused() {
        let mut report = report();
        report.as_object_mut().unwrap().remove("style");
        let err = render(&report, &INPUTS).unwrap_err().to_string();
        assert!(err.contains("`style`"), "{err}");
    }

    #[test]
    fn every_pin_is_complete_and_no_file_repeats() {
        let mut files: Vec<&str> = INPUTS.iter().map(|input| input.file).collect();
        files.sort_unstable();
        files.dedup();
        assert_eq!(files.len(), INPUTS.len());
        for input in &INPUTS {
            assert!(input.sha256.parse::<crate::fetch::Sha256Digest>().is_ok());
            assert!(!input.revision.is_empty() && input.url.starts_with("https://"));
        }
    }

    #[test]
    fn the_script_gets_each_source_once_with_its_files_in_order() {
        let args: Vec<String> = script_args(Path::new("/repo"), Path::new("/data"))
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        let at = args.iter().position(|a| a == "--bank-f").unwrap();
        // The lexicon plus the eight sources, each named once.
        assert_eq!(args.iter().filter(|a| a.starts_with("--")).count(), 9);
        assert!(args[at + 1].ends_with("train-name.npy"));
        assert!(args[at + 2].ends_with("train-synth.npy"));
        assert!(args[at + 6].ends_with("test-synth.npy"));
        assert!(args[at + 7].starts_with("--"));
    }

    #[test]
    fn the_committed_ranks_cover_the_lexicon() {
        let root = crate::root();
        let read = |path: &str| -> Value {
            serde_json::from_str(&fs::read_to_string(root.join(path)).unwrap()).unwrap()
        };
        let (lexicon, ranks) = (read(LEXICON), read(OUT));
        for (key, name) in [
            ("describing", "word"),
            ("style", "label"),
            ("technique", "label"),
        ] {
            let mut want: Vec<&str> = lexicon[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| entry[name].as_str().unwrap())
                .collect();
            let mut have: Vec<&str> = ranks[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| entry[name].as_str().unwrap())
                .collect();
            want.sort_unstable();
            have.sort_unstable();
            assert_eq!(want, have, "{key} of {OUT} differs from {LEXICON}");
        }
    }
}
