//! `cargo xtask render-spread` and `cargo xtask render-reference`: the
//! repeat-render spread of a preset on the Vital plug-in, and the
//! comparison of reference recordings with automatic renders. Both write
//! local files.

use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{Value, json};
use sound_to_patch::audio::{Audio, decode, write_wav};
use sound_to_patch::notes::Transcriber;
use sound_to_patch::render::spread::{
    ClipMetrics, Comparison, SpreadTable, clip_rows, compare, compare_fitted, layer_rows,
};
use sound_to_patch::render::state::readback_mismatches;
use sound_to_patch::render::{PresetState, RenderJob, Rendered};
use sound_to_patch::report::{AnalyzeRequest, analyze};
use sound_to_patch::synth::Synth;
use sound_to_patch::units::Bpm;

use crate::Result;
use crate::fetch::sha256_hex;
use crate::plugin::{Plugin, read_preset};

/// What a spread run renders.
pub(crate) struct SpreadRun<'a> {
    /// The preset file.
    pub(crate) preset: &'a Path,
    /// The MIDI file.
    pub(crate) midi: &'a Path,
    /// Its tempo.
    pub(crate) bpm: Bpm,
    /// How many renders.
    pub(crate) runs: usize,
}

/// Measures the spread of one preset and writes it to `out`. The renders
/// and their layer reports stay in the folder [`renders_folder`] names.
pub(crate) fn run_spread(spread: &SpreadRun<'_>, plugin: &Plugin, out: &Path) -> Result<()> {
    let (state, preset_sha256) = read_preset(spread.preset)?;
    let job = RenderJob::from_midi(state, spread.midi, spread.bpm)?;
    plugin.check(&job)?;
    let renders = plugin.draw(&job, spread.runs)?;
    let folder = renders_folder(out);
    std::fs::create_dir_all(&folder)?;
    let transcriber = Transcriber::new()?;
    let mut clips = Vec::with_capacity(renders.len());
    let mut reports = Vec::with_capacity(renders.len());
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
        reports.push(serde_json::from_str(&text)?);
    }
    let mut outputs = clip_rows(&clips)?;
    outputs.extend(layer_rows(&reports)?);
    let table = SpreadTable::new(&renders, preset_sha256, spread.bpm, outputs)?;
    crate::write_json(out, &serde_json::to_value(&table)?)?;
    for row in &table.outputs {
        println!(
            "{}: spread {:.4}, tolerance {:.4} {}",
            row.output, row.spread, row.tolerance, row.unit
        );
    }
    println!("wrote {}", out.display());
    Ok(())
}

/// Writes a render as a WAV file and measures the stored file, as a
/// consumer reads it.
fn stored_metrics(wav: &Path, rendered: &Rendered) -> Result<ClipMetrics> {
    write_wav(wav, &rendered.audio)?;
    Ok(ClipMetrics::of(
        &decode(wav)?.into_audio(),
        &wav.display().to_string(),
    )?)
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
    pub(crate) runs: usize,
    /// How many references render at once, each on its own instance.
    pub(crate) workers: NonZeroUsize,
}

/// One manifest row, read and checked before any render.
struct Reference {
    id: String,
    preset_id: String,
    preset_sha256: String,
    recording_sha256: String,
    bpm: Bpm,
    job: RenderJob,
    recording: Audio,
    clip: ClipMetrics,
    state_differences: Value,
}

/// Compares each reference recording with renders of its preset, MIDI
/// file and tempo, against the spread of those renders, and writes the
/// report to `out`. The verdict comes after a gain that matches the
/// recording's level to the renders, on the outputs of the sound's shape;
/// the comparison as recorded is for information. The renders stay in the
/// folder [`renders_folder`] names, one folder per reference.
///
/// Every manifest row is read first: its preset, MIDI file, recording and
/// exported state. Then the references render, [`ReferenceRun::workers`]
/// at once, each drawn in sequence on its own plug-in instance.
pub(crate) fn run_reference(
    reference: &ReferenceRun<'_>,
    plugin: &Plugin,
    out: &Path,
) -> Result<()> {
    let references = read_manifest(reference)?;
    let next = AtomicUsize::new(0);
    // Errors cross threads as text: the task error type is not Send.
    let done: Mutex<Vec<Option<std::result::Result<Value, String>>>> =
        Mutex::new((0..references.len()).map(|_| None).collect());
    let threads = reference.workers.get().min(references.len());
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(entry) = references.get(at) else {
                        break;
                    };
                    let row = render_reference(entry, reference, plugin, out)
                        .map_err(|error| format!("{}: {error}", entry.id));
                    if let Ok(mut done) = done.lock() {
                        done[at] = Some(row);
                    }
                }
            });
        }
    });
    let rows = done
        .into_inner()
        .map_err(|_| "a reference thread panicked")?
        .into_iter()
        .map(|row| row.unwrap_or_else(|| Err("a reference did not render".to_owned())))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let report = json!({ "references": rows });
    crate::write_json(out, &report)?;
    println!("wrote {}", out.display());
    Ok(())
}

/// Reads every manifest row: the preset state, the job, the recording and
/// its metrics, and the exported state's differences. Refuses a row whose
/// id another row already holds, since both would write one renders
/// folder.
fn read_manifest(reference: &ReferenceRun<'_>) -> Result<Vec<Reference>> {
    let text = std::fs::read_to_string(reference.manifest)?;
    let mut ids = HashSet::new();
    let mut references = Vec::new();
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
        if !ids.insert(id.clone()) {
            return Err(format!(
                "two manifest rows have the id {id}: one preset and one MIDI file. Remove the repeated row"
            )
            .into());
        }
        let (state, preset_sha256) = read_preset(&preset)?;
        let state_differences = match entry["state_file"].as_str() {
            Some(file) => json!(state_differences(&state, &reference.root.join(file), bpm)?),
            None => Value::Null,
        };
        let job = RenderJob::from_midi(state, &midi, bpm)?;
        let recording_bytes = std::fs::read(&recording)
            .map_err(|error| format!("cannot read {}: {error}", recording.display()))?;
        let audio = decode(&recording)?.into_audio();
        let clip = ClipMetrics::of(&audio, &format!("the recording of {id}"))?;
        references.push(Reference {
            id,
            preset_id,
            preset_sha256,
            recording_sha256: sha256_hex(&recording_bytes),
            bpm,
            job,
            recording: audio,
            clip,
            state_differences,
        });
    }
    if references.is_empty() {
        return Err("the manifest names no reference".into());
    }
    Ok(references)
}

/// Renders one reference, compares its recording and returns its report
/// row.
fn render_reference(
    entry: &Reference,
    reference: &ReferenceRun<'_>,
    plugin: &Plugin,
    out: &Path,
) -> Result<Value> {
    plugin.check(&entry.job)?;
    let renders = plugin.draw(&entry.job, reference.runs)?;
    let folder = renders_folder(out).join(&entry.id);
    std::fs::create_dir_all(&folder)?;
    let clips = renders
        .iter()
        .enumerate()
        .map(|(index, rendered)| {
            stored_metrics(&folder.join(format!("render-{index:02}.wav")), rendered)
        })
        .collect::<Result<Vec<_>>>()?;
    let spread = SpreadTable::new(
        &renders,
        entry.preset_sha256.clone(),
        entry.bpm,
        clip_rows(&clips)?,
    )?;
    let as_recorded = compare(&entry.clip, &clips, &spread.outputs)?;
    let fitted = compare_fitted(
        &entry.recording,
        &format!("the recording of {}", entry.id),
        &clips,
        &spread.outputs,
    )?;
    println!(
        "{}: gain {:+.2} dB, {}",
        entry.id,
        fitted.gain_db,
        summary(&fitted.comparisons)
    );
    Ok(json!({
        "id": entry.id,
        "preset_id": entry.preset_id,
        "recording_sha256": entry.recording_sha256,
        "state_differences": entry.state_differences,
        "spread": spread,
        "gain_db": fitted.gain_db,
        "gain_beyond_range": fitted.gain_beyond_range,
        "comparisons": fitted.comparisons,
        "within": fitted.within,
        "as_recorded": {
            "comparisons": as_recorded,
            "within": as_recorded.iter().all(|comparison| comparison.within),
        },
    }))
}

fn summary(comparisons: &[Comparison]) -> String {
    comparisons
        .iter()
        .map(|c| {
            format!(
                "{} {:.3}/{:.3}{}",
                c.output,
                c.difference,
                c.limit,
                if c.within { "" } else { " BEYOND" }
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
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

/// The default MIDI file of a spread: three notes at 120 BPM.
pub(crate) fn default_midi() -> PathBuf {
    crate::root().join("tests/cmd/data/three-notes.mid")
}
