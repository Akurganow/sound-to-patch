//! `cargo xtask render-timing`: times plug-in loads and renders on this
//! machine and writes the table to a local file.

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use sound_to_patch::render::plugin::{HOST_ENV, PluginRenderer, find_plugin, host_binary};
use sound_to_patch::render::recipe::Recipe;
use sound_to_patch::render::session::BoxedRenderer;
use sound_to_patch::render::state::preset_state;
use sound_to_patch::render::timing::measure;
use sound_to_patch::synth::Synth;

use crate::Result;

/// Times `preset` on the Vital plug-in and writes the table to `out`.
pub(crate) fn run(
    preset: &Path,
    plugin: Option<&Path>,
    host: Option<&Path>,
    runs: NonZeroUsize,
    out: &Path,
) -> Result<()> {
    let synth = Synth::Vital;
    let bytes = std::fs::read(preset)
        .map_err(|error| format!("cannot read {}: {error}", preset.display()))?;
    let state = preset_state(synth, &bytes, &Recipe::current())?;
    let plugin: PathBuf = match plugin {
        Some(path) => path.to_owned(),
        None => find_plugin(synth).ok_or("cannot find the Vital plug-in; pass --plugin")?,
    };
    let host: PathBuf = match host {
        Some(path) => path.to_owned(),
        None => host_binary().ok_or_else(|| {
            format!("cannot find sound-to-patch-host; set {HOST_ENV} or pass --host")
        })?,
    };
    let spawn = move |_worker: usize| {
        PluginRenderer::start(synth, &plugin, &host)
            .map(|renderer| Box::new(renderer) as BoxedRenderer)
    };
    let table = measure(spawn, &state, runs)?;
    let json = serde_json::to_string_pretty(&table)?;
    std::fs::write(out, json + "\n")
        .map_err(|error| format!("cannot write {}: {error}", out.display()))?;
    for row in &table.rows {
        let workers = row
            .workers
            .map(|workers| format!(", {workers} workers"))
            .unwrap_or_default();
        println!(
            "{:?}{workers}: median {:.4}, p95 {:.4}",
            row.kind, row.median, row.p95
        );
    }
    println!("wrote {}", out.display());
    Ok(())
}

/// The default preset: the `routes.vital` fixture, from the workspace
/// root, so the task runs from any folder.
pub(crate) fn default_preset() -> PathBuf {
    crate::root().join("tests/fixtures/vital/routes.vital")
}
