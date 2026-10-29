//! The render session (#249) on the fake renderer: a retry after a crash,
//! a hang or a panic, a sentinel after two failures, a wrong synth, a
//! restart after 256 renders, one live renderer per worker, the flush
//! before every render and the order of the outcomes.

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use sound_to_patch::notes::Velocity;
use sound_to_patch::render::fake::{FakeLog, FakePatch, FakeRenderer, FakeStep, Fault};
use sound_to_patch::render::session::{
    BoxedRenderer, FAILED_SCORE, RESTART_AFTER, RenderOutcome, RenderSession,
};
use sound_to_patch::render::{
    HostIdentity, Material, PresetState, RenderError, RenderJob, RenderNote, Rendered, Renderer,
};
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

/// Jobs whose single notes end at 0.1 s, 0.2 s and so on, so a clip names
/// its job.
fn numbered(count: u32) -> Vec<RenderJob> {
    (1..=count)
        .map(|n| {
            let end = Seconds::new(0.1 * f64::from(n)).unwrap();
            let note = RenderNote::new(
                60,
                Seconds::new(0.0).unwrap(),
                end,
                Velocity::new(100).unwrap(),
            );
            RenderJob::new(
                state(vec![1.0]),
                vec![note.unwrap()],
                Bpm::new(120.0).unwrap(),
            )
            .unwrap()
        })
        .collect()
}

fn workers(count: usize) -> NonZeroUsize {
    NonZeroUsize::new(count).unwrap()
}

/// A session whose first `failing` renderers, counted over all workers,
/// inject `fault` into their render with index `at`.
fn faulty_session(
    count: usize,
    failing: u64,
    fault: Fault,
    at: u64,
    log: Option<FakeLog>,
) -> RenderSession {
    let started = AtomicU64::new(0);
    RenderSession::new(workers(count), move |worker| {
        let start = started.fetch_add(1, Ordering::Relaxed);
        let mut fake = FakeRenderer::new(Synth::Vital, start);
        if start < failing {
            fake = fake.with_fault(at, fault);
        }
        if let Some(log) = &log {
            fake = fake.with_log(Arc::clone(log));
        }
        assert!(worker < count);
        Ok(Box::new(fake) as BoxedRenderer)
    })
}

fn clips(log: &FakeLog) -> Vec<Seconds> {
    log.lock()
        .unwrap()
        .iter()
        .filter_map(|step| match step {
            FakeStep::Clip(end) => Some(*end),
            _ => None,
        })
        .collect()
}

#[test]
fn a_crashed_render_is_tried_once_more_on_a_new_renderer() {
    let session = faulty_session(1, 1, Fault::Crash, 0, None);
    let outcomes = session.render_all(&[job(vec![1.0])]);
    assert!(outcomes[0].rendered().is_some(), "{outcomes:?}");
    assert_eq!(session.starts(), [2]);
}

#[test]
fn the_retry_renders_the_job_that_failed() {
    let log: FakeLog = Arc::default();
    // The first renderer crashes on its third render, job 2.
    let session = faulty_session(1, 1, Fault::Crash, 2, Some(Arc::clone(&log)));
    let jobs = numbered(5);
    let outcomes = session.render_all(&jobs);
    assert!(outcomes.iter().all(|outcome| outcome.rendered().is_some()));
    let ends: Vec<Seconds> = jobs.iter().map(RenderJob::last_note_off).collect();
    // A crashed render logs no clip, so the retry of job 2 comes next.
    assert_eq!(clips(&log), ends);
    assert_eq!(session.starts(), [2]);
}

#[test]
fn a_hung_render_is_tried_once_more_on_a_new_renderer() {
    let session = faulty_session(1, 1, Fault::Hang, 0, None);
    let outcomes = session.render_all(&[job(vec![1.0])]);
    assert!(outcomes[0].rendered().is_some(), "{outcomes:?}");
    assert_eq!(session.starts(), [2]);
}

#[test]
fn a_render_that_fails_twice_gives_the_sentinel_and_the_session_goes_on() {
    let session = faulty_session(1, 2, Fault::Crash, 0, None);
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
    // Far above any distance a render scores, and still finite.
    assert!(FAILED_SCORE >= 1.0e6 && FAILED_SCORE.is_finite());
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
fn a_wrong_synth_fails_at_once_and_keeps_the_renderer() {
    let session = faulty_session(1, 0, Fault::Crash, 0, None);
    let serum = RenderJob::for_material(
        PresetState::new(Synth::Serum2, b"{}".to_vec()),
        Material::Lead,
        Bpm::new(120.0).unwrap(),
    );
    let outcomes = session.render_all(&[serum, job(vec![1.0])]);
    let RenderOutcome::Failed(failure) = &outcomes[0] else {
        panic!("{outcomes:?}");
    };
    assert!(
        matches!(failure.errors[..], [RenderError::WrongSynth { .. }]),
        "{failure:?}"
    );
    assert!(outcomes[1].rendered().is_some(), "{outcomes:?}");
    assert_eq!(session.starts(), [1]);
}

/// A fake that panics on one render, and counts the live instances.
struct Panicking {
    inner: FakeRenderer,
    panic_at: Option<u64>,
    live: Arc<AtomicUsize>,
}

impl Drop for Panicking {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::Relaxed);
    }
}

impl Renderer for Panicking {
    fn identity(&self) -> HostIdentity {
        self.inner.identity()
    }

    fn render(&mut self, job: &RenderJob) -> Result<Rendered, RenderError> {
        assert!(
            self.panic_at != Some(self.inner.renders()),
            "the fake plug-in fell over"
        );
        self.inner.render(job)
    }

    fn render_init(&mut self, job: &RenderJob) -> Result<Rendered, RenderError> {
        self.inner.render_init(job)
    }

    fn save_state(&mut self) -> Result<Vec<u8>, RenderError> {
        self.inner.save_state()
    }
}

/// A session whose first renderer panics on its render with index `at`,
/// and the most renderers that lived at once.
fn panicking_session(count: usize, at: u64) -> (RenderSession, Arc<AtomicUsize>) {
    let started = AtomicU64::new(0);
    let live = Arc::new(AtomicUsize::new(0));
    let most = Arc::new(AtomicUsize::new(0));
    let peak = Arc::clone(&most);
    let session = RenderSession::new(workers(count), move |_| {
        let start = started.fetch_add(1, Ordering::Relaxed);
        let now = live.fetch_add(1, Ordering::Relaxed) + 1;
        peak.fetch_max(now, Ordering::Relaxed);
        Ok(Box::new(Panicking {
            inner: FakeRenderer::new(Synth::Vital, start),
            panic_at: (start == 0).then_some(at),
            live: Arc::clone(&live),
        }) as BoxedRenderer)
    });
    (session, most)
}

#[test]
fn a_panic_costs_one_try_and_every_other_job_renders() {
    for count in [1_u64, 2] {
        let (session, _) = panicking_session(usize::try_from(count).unwrap(), 3);
        let outcomes = session.render_all(&numbered(11));
        assert!(
            outcomes.iter().all(|outcome| outcome.rendered().is_some()),
            "{count} workers: {outcomes:?}"
        );
        assert_eq!(session.starts().iter().sum::<u64>(), count + 1);
    }
}

#[test]
fn a_panic_on_both_tries_names_the_job() {
    // Every renderer panics on its first render, so every job fails twice.
    let session = RenderSession::new(workers(1), |_| {
        Ok(Box::new(Panicking {
            inner: FakeRenderer::new(Synth::Vital, 0),
            panic_at: Some(0),
            live: Arc::new(AtomicUsize::new(1)),
        }) as BoxedRenderer)
    });
    let outcomes = session.render_all(&numbered(3));
    for (at, outcome) in outcomes.iter().enumerate() {
        let RenderOutcome::Failed(failure) = outcome else {
            panic!("job {at} rendered");
        };
        assert_eq!(failure.errors.len(), 2);
        for error in &failure.errors {
            let message = error.to_string();
            let named = format!("the renderer panicked on job {at}: the fake plug-in fell over");
            assert!(message.contains(&named), "{message}");
        }
    }
    assert_eq!(session.starts(), [6]);
}

#[test]
fn a_worker_never_holds_two_renderers() {
    let (session, most) = panicking_session(1, 0);
    assert!(
        session
            .render_all(&numbered(4))
            .iter()
            .all(|outcome| outcome.rendered().is_some())
    );
    assert_eq!(session.starts(), [2]);
    assert_eq!(most.load(Ordering::Relaxed), 1);
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
    fn identity(&self) -> HostIdentity {
        self.inner.identity()
    }

    fn render(&mut self, job: &RenderJob) -> Result<Rendered, RenderError> {
        self.inner.render(job)
    }

    fn render_init(&mut self, job: &RenderJob) -> Result<Rendered, RenderError> {
        self.inner.render_init(job)
    }

    fn save_state(&mut self) -> Result<Vec<u8>, RenderError> {
        self.inner.save_state()
    }
}

#[test]
fn every_render_starts_with_the_flush() {
    let log: FakeLog = Arc::default();
    let session = faulty_session(2, 1, Fault::Crash, 0, Some(Arc::clone(&log)));
    let jobs = numbered(6);
    let outcomes = session.render_all(&jobs);
    assert!(outcomes.iter().all(|outcome| outcome.rendered().is_some()));
    let steps = log.lock().unwrap().clone();
    let pre_roll = jobs[0].recipe().pre_roll();
    let flush: Vec<FakeStep> = std::iter::once(FakeStep::Load)
        .chain(pre_roll.iter().copied().map(FakeStep::PreRoll))
        .collect();
    // A crashed render logs nothing, so the log holds one flush and clip
    // per job.
    assert_eq!(steps.len(), (flush.len() + 1) * jobs.len(), "{steps:?}");
    for render in steps.chunks(flush.len() + 1) {
        assert_eq!(render[..flush.len()], flush);
        assert!(
            matches!(render[flush.len()], FakeStep::Clip(_)),
            "{render:?}"
        );
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
    let jobs = numbered(7);
    let outcomes = session.render_all(&jobs);
    let rendered: Vec<Seconds> = outcomes
        .iter()
        .map(|outcome| outcome.rendered().unwrap().record.last_note_off)
        .collect();
    let ends: Vec<Seconds> = jobs.iter().map(RenderJob::last_note_off).collect();
    assert_eq!(rendered, ends);
    assert_eq!(session.starts(), [1, 1, 1]);
}
