//! The render session: the one worker pool that calibration, refine, the
//! bench and the dataset render through. None of them owns a pool.
//!
//! Each worker holds one renderer, and so one plug-in instance in its own
//! host process. A worker starts its renderer at its first job, and starts
//! a new one after [`RESTART_AFTER`] renders, so state a plug-in builds up
//! over many loads cannot reach later renders. A failed render is retried
//! once on a new renderer. A second failure gives a [`RenderFailure`],
//! which a consumer scores [`FAILED_SCORE`]; the session goes on with the
//! next job.
//!
//! Every render starts with the flush of the recipe. That is the
//! renderer's duty, not the session's: every [`Renderer::render`] loads
//! the job's state and plays the pre-roll before the clip.
//!
//! A panic in a renderer counts as a failed try, so it costs one job a
//! retry and never the jobs of the other workers. A
//! [`RenderError::WrongSynth`] is the caller's mistake, not a fault of
//! the plug-in: it fails the job at once and keeps the renderer.
//!
//! # Costs
//!
//! - Each worker holds one plug-in in its own host process. A Vital host
//!   takes about 270 to 325 MB of resident memory, and the session sets no
//!   cap on the worker count, so pick it for the machine.
//! - Run [`RenderSession::start_all`] before [`RenderSession::render_all`].
//!   A plug-in that cannot start otherwise fails every job after two start
//!   time-outs each, every job returns the sentinel, and a search spends
//!   its budget on nothing.
//! - A job that always hangs costs about two render time-outs: one per
//!   try.

use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{RenderError, RenderJob, Rendered, Renderer};

/// How many renders a renderer plays before the worker starts a new one.
pub const RESTART_AFTER: u64 = 256;
/// How many times a failed render is tried again.
pub const RETRIES: usize = 1;
/// The score a consumer gives a failed render: far above any distance a
/// render can score, so a search ranks it last, and finite, so sums and
/// means of scores stay finite.
pub const FAILED_SCORE: f64 = 1.0e9;

/// A renderer a worker can own on its own thread.
pub type BoxedRenderer = Box<dyn Renderer + Send>;

/// Starts a renderer for the worker with the given index.
pub type Spawn = dyn Fn(usize) -> Result<BoxedRenderer, RenderError> + Send + Sync;

/// A render that failed on every try.
#[derive(Debug)]
pub struct RenderFailure {
    /// The error of each try, the first first.
    pub errors: Vec<RenderError>,
}

impl RenderFailure {
    /// The score of the failed render, [`FAILED_SCORE`].
    #[must_use]
    pub fn score(&self) -> f64 {
        FAILED_SCORE
    }
}

/// What the session gives for one job.
#[derive(Debug)]
pub enum RenderOutcome {
    /// The render.
    Rendered(Rendered),
    /// Every try failed.
    Failed(RenderFailure),
}

impl RenderOutcome {
    /// Returns the render, if one came.
    #[must_use]
    pub fn rendered(&self) -> Option<&Rendered> {
        match self {
            Self::Rendered(rendered) => Some(rendered),
            Self::Failed(_) => None,
        }
    }
}

/// One worker: its renderer, if one runs, and its counts.
#[derive(Default)]
struct Worker {
    renderer: Option<BoxedRenderer>,
    /// Renders the current renderer played, failed ones included.
    renders: u64,
    /// Renderers this worker started.
    starts: u64,
}

/// A pool of workers that renders jobs in parallel.
pub struct RenderSession {
    spawn: Box<Spawn>,
    workers: Vec<Mutex<Worker>>,
}

impl std::fmt::Debug for RenderSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderSession")
            .field("workers", &self.workers.len())
            .finish_non_exhaustive()
    }
}

impl RenderSession {
    /// Builds a session of `workers` workers. `spawn` starts the renderer of
    /// a worker; it runs on the worker's thread.
    #[must_use]
    pub fn new(
        workers: NonZeroUsize,
        spawn: impl Fn(usize) -> Result<BoxedRenderer, RenderError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            spawn: Box::new(spawn),
            workers: (0..workers.get())
                .map(|_| Mutex::new(Worker::default()))
                .collect(),
        }
    }

    /// Returns how many renderers each worker started so far.
    #[must_use]
    pub fn starts(&self) -> Vec<u64> {
        self.workers
            .iter()
            .map(|worker| lock(worker).starts)
            .collect()
    }

    /// Starts the renderer of every worker that has none, so a later
    /// [`RenderSession::render_all`] does not wait for plug-in loads.
    ///
    /// # Errors
    ///
    /// Returns the first error of a renderer that does not start.
    pub fn start_all(&self) -> Result<(), RenderError> {
        std::thread::scope(|scope| {
            let handles: Vec<_> = self
                .workers
                .iter()
                .enumerate()
                .map(|(index, worker)| {
                    scope.spawn(move || {
                        let mut worker = lock(worker);
                        if worker.renderer.is_none() {
                            self.start(index, &mut worker, None)?;
                        }
                        Ok(())
                    })
                })
                .collect();
            handles
                .into_iter()
                .try_for_each(|handle| handle.join().unwrap_or_else(|_| Err(panicked())))
        })
    }

    /// Renders every job and returns the outcomes in the order of `jobs`.
    /// The workers take the jobs in turn, each on its own thread.
    #[must_use]
    pub fn render_all(&self, jobs: &[RenderJob]) -> Vec<RenderOutcome> {
        let next = AtomicUsize::new(0);
        let threads = self.workers.len().min(jobs.len());
        let mut done: Vec<(usize, RenderOutcome)> = std::thread::scope(|scope| {
            let handles: Vec<_> = self.workers[..threads]
                .iter()
                .enumerate()
                .map(|(index, worker)| {
                    let next = &next;
                    scope.spawn(move || {
                        let mut worker = lock(worker);
                        let mut done = Vec::new();
                        loop {
                            let at = next.fetch_add(1, Ordering::Relaxed);
                            let Some(job) = jobs.get(at) else {
                                break done;
                            };
                            done.push((at, self.render_one(index, &mut worker, at, job)));
                        }
                    })
                })
                .collect();
            handles
                .into_iter()
                .flat_map(|handle| handle.join().unwrap_or_default())
                .collect()
        });
        done.sort_by_key(|(at, _)| *at);
        // A worker thread that panicked lost its outcomes; its jobs fail.
        let mut done = done.into_iter().peekable();
        (0..jobs.len())
            .map(|at| match done.next_if(|(index, _)| *index == at) {
                Some((_, outcome)) => outcome,
                None => RenderOutcome::Failed(RenderFailure {
                    errors: vec![panicked()],
                }),
            })
            .collect()
    }

    /// Renders job `at` on `worker`, with a new renderer after
    /// [`RESTART_AFTER`] renders and after each failure.
    fn render_one(
        &self,
        index: usize,
        worker: &mut Worker,
        at: usize,
        job: &RenderJob,
    ) -> RenderOutcome {
        let mut errors = Vec::new();
        for _ in 0..=RETRIES {
            if worker.renders >= RESTART_AFTER {
                worker.renderer = None;
            }
            if worker.renderer.is_none()
                && let Err(error) = self.start(index, worker, Some(at))
            {
                errors.push(error);
                continue;
            }
            let Some(renderer) = worker.renderer.as_mut() else {
                continue;
            };
            worker.renders += 1;
            let result = catch_unwind(AssertUnwindSafe(|| renderer.render(job)))
                .unwrap_or_else(|payload| Err(panic_error(Some(at), payload.as_ref())));
            match result {
                Ok(clip) => return RenderOutcome::Rendered(clip),
                Err(error @ RenderError::WrongSynth { .. }) => {
                    // The job is wrong, not the plug-in: a retry gives the
                    // same error, and the renderer stays healthy.
                    errors.push(error);
                    break;
                }
                Err(error) => {
                    errors.push(error);
                    // A failed plug-in may hold a broken state, so the try
                    // after a failure runs on a new renderer.
                    worker.renderer = None;
                }
            }
        }
        RenderOutcome::Failed(RenderFailure { errors })
    }

    fn start(
        &self,
        index: usize,
        worker: &mut Worker,
        at: Option<usize>,
    ) -> Result<(), RenderError> {
        // The old renderer ends before the new one starts, so a worker
        // never holds two plug-in instances.
        worker.renderer = None;
        worker.starts += 1;
        worker.renders = 0;
        let started = catch_unwind(AssertUnwindSafe(|| (self.spawn)(index)))
            .unwrap_or_else(|payload| Err(panic_error(at, payload.as_ref())))?;
        worker.renderer = Some(started);
        Ok(())
    }
}

/// Locks a worker. A worker whose thread panicked is still usable: its
/// renderer is dropped, so the next job starts a new one.
fn lock(worker: &Mutex<Worker>) -> std::sync::MutexGuard<'_, Worker> {
    worker.lock().unwrap_or_else(|poisoned| {
        let mut worker = poisoned.into_inner();
        worker.renderer = None;
        worker
    })
}

fn panicked() -> RenderError {
    RenderError::Crashed("a render worker thread panicked".to_owned())
}

/// The error of a renderer that panicked, naming the job it worked on.
fn panic_error(at: Option<usize>, payload: &(dyn std::any::Any + Send)) -> RenderError {
    let message = payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "no message".to_owned());
    let job = at.map_or_else(
        || "while it started".to_owned(),
        |at| format!("on job {at}"),
    );
    RenderError::Crashed(format!("the renderer panicked {job}: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_poisoned_worker_drops_its_renderer() {
        let worker = Mutex::new(Worker {
            renderer: Some(Box::new(crate::render::fake::FakeRenderer::new(
                crate::synth::Synth::Vital,
                0,
            ))),
            renders: 3,
            starts: 1,
        });
        let poisoned = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _guard = worker.lock().unwrap();
                    panic!("poison the lock");
                })
                .join()
                .is_err()
        });
        assert!(poisoned && worker.is_poisoned());
        let worker = lock(&worker);
        assert!(worker.renderer.is_none());
        assert_eq!((worker.renders, worker.starts), (3, 1));
    }

    #[test]
    fn a_panic_names_the_job_and_its_message() {
        let error = panic_error(Some(6), &"boom");
        assert_eq!(
            error.to_string(),
            RenderError::Crashed("the renderer panicked on job 6: boom".to_owned()).to_string()
        );
        let error = panic_error(None, &String::from("bad"));
        assert!(error.to_string().contains("panicked while it started: bad"));
    }
}
