//! The render session (#249) on the fake renderer: a retry after a crash,
//! a sentinel after two failures, a restart after 256 renders, the flush
//! before every render and the order of the outcomes.

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use sound_to_patch::notes::Velocity;
use sound_to_patch::render::fake::{FakeLog, FakePatch, FakeRenderer, FakeStep, Fault};
use sound_to_patch::render::session::{
    BoxedRenderer, FAILED_SCORE, RESTART_AFTER, RenderOutcome, RenderSession,
};
use sound_to_patch::render::{Material, PresetState, RenderError, RenderJob, RenderNote, Renderer};
use sound_to_patch::synth::Synth;
use sound_to_patch::units::{Bpm, Seconds};

fn state(harmonics: Vec<f64>) -> PresetState {
    FakePatch {
        harmonics,
        attack: Seconds::new(0.005).unwrap(),
        release: Seconds::new(0.05).unwrap(),
    }
    .state(Synth::Vital)
    .unwrap()
}

fn job(harmonics: Vec<f64>) -> RenderJob {
    RenderJob::for_material(state(harmonics), Material::Lead, Bpm::new(120.0).unwrap())
}

fn workers(count: usize) -> NonZeroUsize {
    NonZeroUsize::new(count).unwrap()
}

/// A session whose renderers crash on their first render while `crashing`
/// renderers have started, counted over all workers.
fn crashing_session(count: usize, crashing: u64, log: Option<FakeLog>) -> RenderSession {
    let started = AtomicU64::new(0);
    RenderSession::new(workers(count), move |worker| {
        let start = started.fetch_add(1, Ordering::Relaxed);
        let mut fake = FakeRenderer::new(Synth::Vital, start);
        if start < crashing {
            fake = fake.with_fault(0, Fault::Crash);
        }
        if let Some(log) = &log {
            fake = fake.with_log(Arc::clone(log));
        }
        assert!(worker < count);
        Ok(Box::new(fake) as BoxedRenderer)
    })
}

#[test]
fn a_crashed_render_is_tried_once_more_on_a_new_renderer() {
    let session = crashing_session(1, 1, None);
    let outcomes = session.render_all(&[job(vec![1.0])]);
    assert!(outcomes[0].rendered().is_some(), "{outcomes:?}");
    assert_eq!(session.starts(), [2]);
}

#[test]
fn a_render_that_fails_twice_gives_the_sentinel_and_the_session_goes_on() {
    let session = crashing_session(1, 2, None);
    let outcomes = session.render_all(&[job(vec![1.0]), job(vec![1.0, 0.5])]);
    let RenderOutcome::Failed(failure) = &outcomes[0] else {
        panic!("the first job rendered: {outcomes:?}");
    };
    assert_eq!(failure.errors.len(), 2);
    assert!(
        failure
            .errors
            .iter()
            .all(|error| matches!(error, RenderError::Crashed(_)))
    );
    assert_eq!(failure.score(), FAILED_SCORE);
    assert!(FAILED_SCORE.is_finite());
    assert!(outcomes[1].rendered().is_some(), "{outcomes:?}");
    assert_eq!(session.starts(), [3]);
}

#[test]
fn a_renderer_that_does_not_start_is_a_failed_try() {
    let session = RenderSession::new(workers(1), |_| {
        Err(RenderError::Host {
            cause: "no plug-in".to_owned(),
            fix: "Install it".to_owned(),
        })
    });
    let outcomes = session.render_all(&[job(vec![1.0])]);
    let RenderOutcome::Failed(failure) = &outcomes[0] else {
        panic!("{outcomes:?}");
    };
    assert_eq!(failure.errors.len(), 2);
    assert_eq!(session.starts(), [2]);
}

#[test]
fn a_worker_starts_a_new_renderer_after_256_renders() {
    assert_eq!(RESTART_AFTER, 256);
    let renders = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&renders);
    let session = RenderSession::new(workers(1), move |_| {
        Ok(Box::new(Counting {
            inner: FakeRenderer::new(Synth::Vital, 0),
            renders: Arc::clone(&seen),
        }) as BoxedRenderer)
    });
    let jobs = vec![job(vec![1.0]); 256];
    assert!(
        session
            .render_all(&jobs)
            .iter()
            .all(|outcome| outcome.rendered().is_some())
    );
    assert_eq!(session.starts(), [1]);
    let outcomes = session.render_all(&jobs[..1]);
    assert!(outcomes[0].rendered().is_some());
    assert_eq!(session.starts(), [2]);
    drop(session);
    // The first renderer played 256 renders, the second one.
    assert_eq!(*renders.lock().unwrap(), [256, 1]);
}

/// A fake that counts its renders in a shared list, one entry per
/// renderer, written when it ends.
struct Counting {
    inner: FakeRenderer,
    renders: Arc<Mutex<Vec<u64>>>,
}

impl Drop for Counting {
    fn drop(&mut self) {
        self.renders
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(self.inner.renders());
    }
}

impl Renderer for Counting {
    fn identity(&self) -> sound_to_patch::render::HostIdentity {
        self.inner.identity()
    }

    fn render(&mut self, job: &RenderJob) -> Result<sound_to_patch::render::Rendered, RenderError> {
        self.inner.render(job)
    }

    fn render_init(
        &mut self,
        job: &RenderJob,
    ) -> Result<sound_to_patch::render::Rendered, RenderError> {
        self.inner.render_init(job)
    }

    fn save_state(&mut self) -> Result<Vec<u8>, RenderError> {
        self.inner.save_state()
    }
}

#[test]
fn every_render_starts_with_the_flush() {
    let log: FakeLog = Arc::default();
    let session = crashing_session(2, 1, Some(Arc::clone(&log)));
    let jobs: Vec<RenderJob> = (1..=6).map(|n| job(vec![1.0; n])).collect();
    let outcomes = session.render_all(&jobs);
    assert!(outcomes.iter().all(|outcome| outcome.rendered().is_some()));
    let steps = log.lock().unwrap().clone();
    let pre_roll = jobs[0].recipe().pre_roll();
    let flush: Vec<FakeStep> = std::iter::once(FakeStep::Load)
        .chain(pre_roll.iter().copied().map(FakeStep::PreRoll))
        .chain([FakeStep::Clip])
        .collect();
    // A crashed render logs nothing, so the log holds one flush and clip
    // per job.
    assert_eq!(steps.len(), flush.len() * jobs.len(), "{steps:?}");
    for render in steps.chunks(flush.len()) {
        assert_eq!(render, flush);
    }
}

#[test]
fn the_outcomes_follow_the_order_of_the_jobs() {
    let session = RenderSession::new(workers(3), |worker| {
        let seed = u64::try_from(worker).unwrap();
        Ok(Box::new(FakeRenderer::new(Synth::Vital, seed)) as BoxedRenderer)
    });
    session.start_all().unwrap();
    assert_eq!(session.starts(), [1, 1, 1]);
    let ends: Vec<Seconds> = (1..=7)
        .map(|n| Seconds::new(0.1 * f64::from(n)).unwrap())
        .collect();
    let jobs: Vec<RenderJob> = ends
        .iter()
        .map(|end| {
            let note = RenderNote::new(
                60,
                Seconds::new(0.0).unwrap(),
                *end,
                Velocity::new(100).unwrap(),
            );
            RenderJob::new(
                state(vec![1.0]),
                vec![note.unwrap()],
                Bpm::new(120.0).unwrap(),
            )
            .unwrap()
        })
        .collect();
    let outcomes = session.render_all(&jobs);
    let rendered: Vec<Seconds> = outcomes
        .iter()
        .map(|outcome| outcome.rendered().unwrap().record.last_note_off)
        .collect();
    assert_eq!(rendered, ends);
    assert_eq!(session.starts(), [1, 1, 1]);
}
