//! Render and load timing, the table refine sets its budget from.
//!
//! It holds how long a plug-in load, a state load and one 4 s render
//! take, and how many renders per second the session gives with 1, 2, 4
//! and 8 workers.
//!
//! The table measures one machine, so it stays a local file. The
//! repository holds the method and no numbers.

use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::session::{BoxedRenderer, RenderOutcome, RenderSession};
use super::{HostIdentity, Material, PresetState, RenderError, RenderJob};
use crate::cast;
use crate::dsp::quantile;
use crate::units::Bpm;

/// The worker counts of the throughput rows.
pub const WORKER_COUNTS: [usize; 4] = [1, 2, 4, 8];
/// The tempo of the timed renders.
const BPM: f64 = 120.0;

/// What a row measures.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimingKind {
    /// Starting a renderer: the host process and the plug-in load, in s.
    PluginLoad,
    /// Loading a preset state into the plug-in, in s, as the host
    /// measures it.
    StateLoad,
    /// One render of the recipe's lead note, held 3 s in a clip of at
    /// least 4 s, state load and pre-roll included, in s.
    Render,
    /// The length of the timed clip, in s: 4 s, or longer while the tail
    /// sounds.
    ClipLength,
    /// Renders per second of a session.
    RendersPerSecond,
}

/// One row of the table: the median and the 95th percentile of `runs`
/// measurements.
///
/// For [`TimingKind::RendersPerSecond`] each measurement is one batch of
/// one job per worker, and the rate is the jobs over the batch time. Its
/// `p95` is the rate at the 95th percentile of the batch time, the slow
/// end.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimingRow {
    /// What the row measures.
    pub kind: TimingKind,
    /// The workers of a throughput row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workers: Option<usize>,
    /// The median.
    pub median: f64,
    /// The 95th percentile, linear between the two nearest measurements.
    pub p95: f64,
}

/// The timing of one synth build on one machine.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimingTable {
    /// What rendered: the synth, its build, the host and the architecture.
    pub identity: HostIdentity,
    /// The recipe version of the renders.
    pub recipe_version: u32,
    /// How many measurements each row holds.
    pub runs: usize,
    /// The rows.
    pub rows: Vec<TimingRow>,
}

/// Measures every row `runs` times with renderers that `spawn` starts.
///
/// The renders play the recipe's lead note with `state`. The throughput
/// rows start every worker before they time a batch, so no batch waits for
/// a plug-in load.
///
/// # Errors
///
/// Returns the [`RenderError`] of a renderer that does not start or a
/// render that fails, and [`RenderError::Host`] for a backend that does
/// not measure its state loads.
pub fn measure(
    spawn: impl Fn(usize) -> Result<BoxedRenderer, RenderError> + Send + Sync + 'static,
    state: &PresetState,
    runs: NonZeroUsize,
) -> Result<TimingTable, RenderError> {
    let runs = runs.get();
    let spawn = Arc::new(spawn);
    let job = RenderJob::for_material(state.clone(), Material::Lead, Bpm::from_const(BPM));
    let mut loads = Vec::with_capacity(runs);
    let mut identity = None;
    for _ in 0..runs {
        let start = Instant::now();
        let renderer = spawn(0)?;
        loads.push(start.elapsed().as_secs_f64());
        identity.get_or_insert_with(|| renderer.identity());
    }
    let mut renderer = spawn(0)?;
    let mut states = Vec::with_capacity(runs);
    let mut renders = Vec::with_capacity(runs);
    let mut clips = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        let clip = renderer.render(&job)?;
        renders.push(start.elapsed().as_secs_f64());
        clips.push(clip.record.length.get());
        let load = renderer.last_state_load().ok_or_else(|| {
            timing_error(
                "the renderer does not measure its state loads",
                "Time the plug-in backend or the fake",
            )
        })?;
        states.push(load.get());
    }
    drop(renderer);
    let mut rows = vec![
        row(TimingKind::PluginLoad, None, &loads),
        row(TimingKind::StateLoad, None, &states),
        row(TimingKind::Render, None, &renders),
        row(TimingKind::ClipLength, None, &clips),
    ];
    for workers in WORKER_COUNTS {
        let count = NonZeroUsize::new(workers).unwrap_or(NonZeroUsize::MIN);
        let session = RenderSession::new(count, {
            let spawn = Arc::clone(&spawn);
            move |worker| spawn(worker)
        });
        session.start_all()?;
        let batch = vec![job.clone(); workers];
        let mut times = Vec::with_capacity(runs);
        for _ in 0..runs {
            let start = Instant::now();
            let outcomes = session.render_all(&batch);
            times.push(start.elapsed().as_secs_f64());
            if let Some(RenderOutcome::Failed(failure)) = outcomes
                .into_iter()
                .find(|outcome| matches!(outcome, RenderOutcome::Failed(_)))
            {
                return Err(failure
                    .errors
                    .into_iter()
                    .last()
                    .unwrap_or_else(|| timing_error("a render failed", "Time it again")));
            }
        }
        let time = row(TimingKind::RendersPerSecond, Some(workers), &times);
        let rate = |seconds: f64| cast::usize_to_f64(workers) / seconds;
        rows.push(TimingRow {
            median: rate(time.median),
            p95: rate(time.p95),
            ..time
        });
    }
    Ok(TimingTable {
        identity: identity.ok_or_else(|| timing_error("no renderer started", "Time it again"))?,
        recipe_version: job.recipe().version(),
        runs,
        rows,
    })
}

fn row(kind: TimingKind, workers: Option<usize>, values: &[f64]) -> TimingRow {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    TimingRow {
        kind,
        workers,
        median: quantile(&sorted, 0.5).unwrap_or(f64::NAN),
        p95: quantile(&sorted, 0.95).unwrap_or(f64::NAN),
    }
}

fn timing_error(cause: impl Into<String>, fix: &str) -> RenderError {
    RenderError::Host {
        cause: cause.into(),
        fix: fix.to_owned(),
    }
}
