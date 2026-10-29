//! `cargo xtask render-timing`: times plug-in loads and renders on this
//! machine and writes the table to a local file.

use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use sound_to_patch::render::timing::measure;

use crate::Result;
use crate::plugin::{Plugin, read_preset};

/// Times `preset` on the Vital plug-in and writes the table to `out`.
pub(crate) fn run(preset: &Path, plugin: &Plugin, runs: NonZeroUsize, out: &Path) -> Result<()> {
    let (state, _) = read_preset(preset)?;
    let table = measure(plugin.spawn(), &state, runs)?;
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
