//! `cargo xtask render-spread` and `cargo xtask render-reference`: the
//! repeat-render spread of a preset on the Vital plug-in, and the
//! comparison of reference recordings with automatic renders. Both write
//! local files.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use sound_to_patch::audio::{Audio, decode, write_wav};
use sound_to_patch::notes::Transcriber;
use sound_to_patch::render::spread::{
    ClipMetrics, STATISTIC, SpreadTable, clip_rows, compare, layer_values, value_row,
};
use sound_to_patch::render::state::readback_mismatches;
use sound_to_patch::render::{PresetState, RenderJob, Rendered};
use sound_to_patch::report::{AnalyzeRequest, analyze};
use sound_to_patch::synth::Synth;
use sound_to_patch::units::Bpm;

use crate::Result;
use crate::plugin::{Plugin, read_preset, sha256_hex};

/// What a spread run renders.
pub(crate) struct SpreadRun<'a> {
    /// The preset file.
    pub(crate) preset: &'a Path,
    /// The MIDI file.
    pub(crate) midi: &'a Path,
    /// Its tempo.
    pub(crate) bpm: Bpm,
    /// How many renders.
    pub(crate) runs: NonZeroUsize,
    /// How many workers render them.
    pub(crate) workers: NonZeroUsize,
}

/// Measures the spread of one preset and writes it to `out`. The renders
/// and their layer reports stay in the folder [`renders_folder`] names.
pub(crate) fn run_spread(spread: &SpreadRun<'_>, plugin: &Plugin, out: &Path) -> Result<()> {
    let (state, preset_sha256) = read_preset(spread.preset)?;
    let job = RenderJob::from_midi(state, spread.midi, spread.bpm)?;
    plugin.check(&job)?;
    let renders = plugin.render(&job, spread.runs.get(), spread.workers)?;
    let folder = renders_folder(out);
    std::fs::create_dir_all(&folder)?;
    let transcriber = Transcriber::new()?;
    let mut clips = Vec::with_capacity(renders.len());
    let mut layers: BTreeMap<String, (String, Vec<f64>)> = BTreeMap::new();
    for (index, rendered) in renders.iter().enumerate() {
        let wav = folder.join(format!("render-{index:02}.wav"));
        clips.push(stored_metrics(&wav, rendered)?);
        let report = folder.join(format!("render-{index:02}"));
        analyze(
            &AnalyzeRequest {
                stem: &wav,
                bpm: spread.bpm,
                bpm_exact: true,
                synth: Synth::Vital,
                notes: Some(spread.midi),
                profile: None,
                out: Some(&report),
                force: true,
            },
            &transcriber,
        )?;
        let text = std::fs::read_to_string(report.join("layers.json"))?;
        for (path, unit, value) in layer_values(&serde_json::from_str(&text)?) {
            layers
                .entry(path)
                .or_insert((unit, Vec::new()))
                .1
                .push(value);
        }
    }
    let mut outputs = clip_rows(&clips);
    // An output that some render left without an estimate has no spread.
    outputs.extend(layers.iter().filter_map(|(path, (unit, values))| {
        (values.len() == renders.len()).then(|| value_row(path, unit, values))?
    }));
    let first = renders.first().ok_or("no render came back")?;
    let table = SpreadTable {
        identity: first.record.identity.clone(),
        recipe_version: first.record.recipe_version,
        oversampling: first.record.oversampling,
        n: renders.len(),
        statistic: STATISTIC.to_owned(),
        preset_sha256,
        midi_sha256: first.record.midi_sha256.clone(),
        bpm: spread.bpm,
        outputs,
    };
    write_json(out, &serde_json::to_value(&table)?)?;
    for row in &table.outputs {
        println!("{}: {:.4} {}", row.output, row.spread, row.unit);
    }
    println!("wrote {}", out.display());
    Ok(())
}

/// Writes a render as a WAV file and measures the stored file, as a
/// consumer reads it.
fn stored_metrics(wav: &Path, rendered: &Rendered) -> Result<ClipMetrics> {
    write_wav(wav, &rendered.audio)?;
    ClipMetrics::of(&decode(wav)?.into_audio())
        .ok_or_else(|| format!("{} is silent", wav.display()).into())
}

/// What a reference run reads and renders.
pub(crate) struct ReferenceRun<'a> {
    /// The folder the manifest's paths start from.
    pub(crate) root: &'a Path,
    /// The manifest: one JSON object per line with `preset_id`,
    /// `preset_file`, `midi_file`, `bpm` and `render_file`, and
    /// `state_file` when the recording host exported the state it played.
    pub(crate) manifest: &'a Path,
    /// How many renders per reference.
    pub(crate) runs: NonZeroUsize,
    /// How many workers render them.
    pub(crate) workers: NonZeroUsize,
    /// A gain in dB applied to every recording before the comparison: a
    /// diagnostic that separates a level offset of the recording chain
    /// from a difference of the sound. The report records it.
    pub(crate) recording_gain_db: f64,
}

/// Compares each reference recording with renders of its preset, MIDI
/// file and tempo, within the spread of those renders, and writes the
/// report to `out`. The renders stay in the folder [`renders_folder`]
/// names, one folder per reference.
pub(crate) fn run_reference(
    reference: &ReferenceRun<'_>,
    plugin: &Plugin,
    out: &Path,
) -> Result<()> {
    let text = std::fs::read_to_string(reference.manifest)?;
    let mut rows = Vec::new();
    let mut header = None;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let entry: Value = serde_json::from_str(line)?;
        let field = |key: &str| {
            entry[key]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("a manifest row has no `{key}`: {line}"))
        };
        let preset_id = field("preset_id")?;
        let preset = reference.root.join(field("preset_file")?);
        let midi = reference.root.join(field("midi_file")?);
        let recording = reference.root.join(field("render_file")?);
        let bpm = Bpm::new(entry["bpm"].as_f64().ok_or("a manifest row has no `bpm`")?)?;
        let id = format!("{preset_id}__{}", file_stem(&midi));
        let (state, preset_sha256) = read_preset(&preset)?;
        let state_differences = match entry["state_file"].as_str() {
            Some(file) => json!(state_differences(&state, &reference.root.join(file), bpm)?),
            None => Value::Null,
        };
        let job = RenderJob::from_midi(state, &midi, bpm)?;
        plugin.check(&job)?;
        let renders = plugin.render(&job, reference.runs.get(), reference.workers)?;
        let folder = renders_folder(out).join(&id);
        std::fs::create_dir_all(&folder)?;
        let clips = renders
            .iter()
            .enumerate()
            .map(|(index, rendered)| {
                stored_metrics(&folder.join(format!("render-{index:02}.wav")), rendered)
            })
            .collect::<Result<Vec<_>>>()?;
        let recording_bytes = std::fs::read(&recording)?;
        let audio = decode(&recording)?.into_audio();
        let gain = 10_f64.powf(reference.recording_gain_db / 20.0);
        let audio = Audio::new(
            audio.sample_rate(),
            audio
                .channels()
                .iter()
                .map(|channel| channel.iter().map(|x| x * gain).collect())
                .collect(),
        )?;
        let clip =
            ClipMetrics::of(&audio).ok_or_else(|| format!("the recording of {id} is silent"))?;
        let spread = clip_rows(&clips);
        let comparisons = compare(&clip, &clips, &spread);
        let within = comparisons.iter().all(|comparison| comparison.within);
        println!(
            "{id}: {}",
            comparisons
                .iter()
                .map(|c| format!(
                    "{} {:.3}/{:.3}{}",
                    c.output,
                    c.difference,
                    c.spread,
                    if c.within { "" } else { " BEYOND" }
                ))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let first = renders.first().ok_or("no render came back")?;
        header.get_or_insert_with(|| {
            json!({
                "identity": first.record.identity,
                "recipe_version": first.record.recipe_version,
                "oversampling": first.record.oversampling,
                "n": renders.len(),
                "statistic": STATISTIC,
                "recording_gain_db": reference.recording_gain_db,
            })
        });
        rows.push(json!({
            "id": id,
            "preset_id": preset_id,
            "bpm": bpm,
            "preset_sha256": preset_sha256,
            "midi_sha256": first.record.midi_sha256,
            "recording_sha256": sha256_hex(&recording_bytes),
            "state_differences": state_differences,
            "spread": spread,
            "comparisons": comparisons,
            "within": within,
        }));
    }
    let mut report = header.ok_or("the manifest names no reference")?;
    report["references"] = Value::Array(rows);
    write_json(out, &report)?;
    println!("wrote {}", out.display());
    Ok(())
}

/// The values that differ between the state of the preset and the state
/// the recording host exported, as the load check compares them at the
/// recording's tempo: the evidence that the recording played the preset.
/// The name and the tuning are left out: the host names the export, and
/// saves no tuning in a preset file.
fn state_differences(state: &PresetState, exported: &Path, bpm: Bpm) -> Result<Vec<String>> {
    let ignored = |path: &str| path == "preset_name" || path.starts_with("tuning");
    Ok(readback_mismatches(state, &std::fs::read(exported)?, bpm)?
        .into_iter()
        .filter(|mismatch| !ignored(&mismatch.path))
        .map(|mismatch| mismatch.to_string())
        .collect())
}

/// The folder beside `out` that keeps the renders: `out` without its
/// extension, plus `-renders`.
fn renders_folder(out: &Path) -> PathBuf {
    let mut name = out.file_stem().unwrap_or_default().to_owned();
    name.push("-renders");
    out.with_file_name(name)
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn write_json(out: &Path, value: &Value) -> Result<()> {
    let text = serde_json::to_string_pretty(value)? + "\n";
    std::fs::write(out, text)
        .map_err(|error| format!("cannot write {}: {error}", out.display()).into())
}

/// The default MIDI file of a spread: three notes at 120 BPM.
pub(crate) fn default_midi() -> PathBuf {
    crate::root().join("tests/cmd/data/three-notes.mid")
}
