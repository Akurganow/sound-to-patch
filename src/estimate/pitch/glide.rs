//! Glides: pitch moving from one stable pitch to another, read from the f0
//! track, with the curve `s(x) = 1 - (1 - x)^p` fitted between the two.

use super::fit::nelder_mead;
use crate::cast;
use crate::notes::{MODEL_RATE, NoteList, PitchTrack, TRACK_FRAME, TRACK_HOP};
use crate::units::{Cents, Hz, Seconds, Semitones};

/// The slope above which the pitch moves, in cents per second: 0.861
/// semitone per second, the floor of a published portamento detector.
/// Untuned.
pub const GLIDE_SLOPE_MIN: f64 = 86.0;
/// The shortest glide, over its detected span and over its fitted time.
pub(crate) const GLIDE_TIME_MIN: Seconds = Seconds::from_const(0.09);
/// Motion shorter than this over its detected span is a step between two
/// notes.
///
/// From here up to [`GLIDE_TIME_MIN`] it is a glide too short to read. Six
/// hops: the moving average alone spreads a jump over five.
pub(crate) const GLIDE_STEP_MAX: Seconds = Seconds::from_const(0.035);
/// Two stable pitches this far apart split a steady span, so the vibrato
/// search never reads a step between two notes. A vibrato of ±60 cents
/// moves its stable pitches 120 cents apart and stays whole.
pub(crate) const STEADY_SPLIT: Cents = Cents::from_const(150.0);
/// The smallest interval between the two stable pitches.
pub(crate) const GLIDE_INTERVAL_MIN: Cents = Cents::from_const(100.0);
/// A stable pitch lasts at least this long.
pub(crate) const PLATEAU_MIN: Seconds = Seconds::from_const(0.05);
/// A stable pitch stays within this many cents of its mean.
pub(crate) const PLATEAU_RANGE: Cents = Cents::from_const(30.0);
/// A stable pitch belongs to a note whose pitch lies within this many
/// cents of it.
pub(crate) const NOTE_MATCH: Cents = Cents::from_const(50.0);
/// Frames of the centred moving average that the slope reads: 5 hops,
/// 29 ms.
pub(crate) const SMOOTH_FRAMES: usize = 5;
/// The range of the curve power `p`.
pub const CURVE_RANGE: (f64, f64) = (0.2, 5.0);
/// A fitted `p` within this factor of a bound of [`CURVE_RANGE`] sits on
/// the bound.
pub(crate) const CURVE_EDGE: f64 = 1.05;
/// The largest RMS residual of a glide whose `p` sits on the upper bound
/// of [`CURVE_RANGE`]. Exponential portamento, `1 - exp(-t / tau)`, fits
/// the bound within 20 cents; a smeared step on that bound fits worse.
pub(crate) const BOUND_MAX_RESIDUAL: Cents = Cents::from_const(20.0);
/// The longest detected span of a glide whose `p` sits on the upper bound
/// of [`CURVE_RANGE`], as a multiple of its fitted time.
///
/// The track smears a step into a fast curve on that bound and keeps
/// drifting after it, so the slope stays up long after the fitted motion
/// ends: 3.3 times its fitted time at a semitone step on one 808
/// oscillator, 5.2 at a step between legato 808 notes, 2.9 and 9.1 at
/// steps on real stems. RC slides and power curves of the generated set
/// read 0.6 to 1.9, small slides on real stems 0.4 to 2.3.
pub(crate) const BOUND_SPAN_MAX: f64 = 2.5;
/// The largest RMS residual of a glide of 90 ms or more. The glides of the
/// generated set fit within 22 cents.
pub(crate) const GLIDE_MAX_RESIDUAL: Cents = Cents::from_const(40.0);
/// The longest unvoiced gap between two voiced runs that the glide search
/// bridges. A beating unison can leave the track unvoiced for 75 ms inside
/// a glide.
pub(crate) const GAP_BRIDGE_MAX: Seconds = Seconds::from_const(0.1);
/// The smallest jump across a bridged gap, and the smallest motion over
/// the two frames after it: two steps of the tracker's 10-cent grid.
pub(crate) const GAP_MOTION_MIN: Cents = Cents::from_const(20.0);
/// The fastest jump across a bridged gap, in cents per second.
///
/// A 50 ms glide of 7 semitones jumps 20 cents per millisecond across its two
/// unvoiced frames, a unison whose voices start in phase 3.6 across its
/// 75 ms gap; a note struck again 10 semitones lower jumps about 40 across
/// three unvoiced frames.
pub(crate) const GAP_SLOPE_MAX: f64 = 30_000.0;
/// A gap whose jump carries more than this share of the interval between
/// two stable pitches, or a jump against it, is a step: the track lands
/// on the new pitch and the settle after it reads as motion.
///
/// Unison glides whose voices beat the track unvoiced for 75 ms jump 0.53
/// and 0.54 of their interval across the gap; a step between two 808
/// notes jumped 0.72 across 58 ms, and a fast-start unison 0.76.
pub(crate) const GAP_SHARE_MAX: f64 = 0.6;
/// A glide from silence needs a sound to start at most this long before
/// its voiced run.
pub(crate) const ONSET_LOOKBACK: Seconds = Seconds::from_const(0.1);
/// The fit reads this much of each stable pitch next to the motion.
const FIT_MARGIN: f64 = 0.05;

/// How far the track reads behind the audio at `f0`.
///
/// The tracker compares the first half of each centred frame with the
/// samples one period later,
/// so a frame reads the audio from a quarter frame before its centre to one
/// period after it: the lag is a quarter frame, 23.2 ms, less half a
/// period, so 18.7 ms at 110 Hz and 14.1 ms at 55 Hz.
#[must_use]
pub(crate) fn track_lag(f0: Hz) -> Seconds {
    let rate = MODEL_RATE.hz().get();
    let quarter = cast::usize_to_f64(TRACK_FRAME / 4) / rate;
    let half_period = if f0.get() > 0.0 { 0.5 / f0.get() } else { 0.0 };
    Seconds::clamped(quarter - half_period)
}

/// [`track_lag`] at a pitch in cents.
fn lag_at(cents: f64) -> f64 {
    track_lag(hz(cents)).get()
}

/// The frequency of a pitch in cents (MIDI pitch times 100).
fn hz(cents: f64) -> Hz {
    Hz::clamped(440.0 * 2f64.powf((cents - 6900.0) / 1200.0))
}

/// The convention of a glide, as `layers.json` names it.
pub const GLIDE_CONVENTION: &str = "pitch in cents of the voiced f0 frames; a stable pitch is a \
span of 50 ms or more within 30 cents of its running mean whose least-squares slope stays at 86 \
cents per second or less; a glide joins two stable pitches 100 cents or more apart that match \
notes of different pitch within 50 cents, where the slope of the 5-frame moving average stays \
at 86 cents per second or more, in the direction of the motion, over 90 ms or more; s(x) = 1 - \
(1 - x)^p is fitted between the two stable pitches by least squares over the motion and 50 ms of \
each; start_s is the fitted start less the track's lag of a quarter frame, 23.2 ms, less half \
a period; time_s is the full 0 to 100 % time; from_silence marks a glide into the first stable \
pitch of a voiced run whose sound starts, the full-band RMS rising 20 dB, up to 100 ms before \
it, and the fit also finds its start pitch; a voiced run that starts within 100 ms of the one \
before joins it when the full-band RMS does not rise 6 dB in the gap, its 10 ms RMS stays within \
20 dB of the 20 ms before or after the gap and within 40 dB of the loudest frame of the stem, \
and the pitch keeps moving after the gap the way it jumped, at 30 cents per millisecond or \
slower; two stable pitches with a bridged gap between them whose jump goes against the interval \
by 20 cents or more, or carries more than 60 % of it, join no glide; motion of 35 to 90 ms, over \
the detected span or the fitted time, counts as too short; a fit of 90 ms or more with p within \
5 % of 0.2, with p within 5 % of 5 and an RMS residual above 20 cents, a detected span under 90 \
ms or over 2.5 times the fitted time, or a start from silence, or with an RMS residual above 40 \
cents, is no glide; a glide with p within 5 % of 5 reads at low \
confidence with the reason curve_bound";

/// A fitted glide.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlideFit {
    /// The start of the motion in the audio.
    pub start: Seconds,
    /// The interval from the start pitch to the end pitch.
    pub interval: Semitones,
    /// The full 0 to 100 % time.
    pub time: Seconds,
    /// The curve power `p`: above 1 starts fast, below 1 starts slow.
    pub curve: f64,
    /// The glide starts a voiced run, so its start pitch comes from the fit.
    pub from_silence: bool,
    /// The span where the slope stayed above [`GLIDE_SLOPE_MIN`].
    pub detected: Seconds,
    /// The RMS residual of the fit.
    pub residual: Cents,
    /// The RMS deviation of the stable pitches from their medians.
    pub jitter: Cents,
    /// The fitted `p` sits on the upper bound of [`CURVE_RANGE`]: a glide
    /// that approaches its target exponentially, as an RC portamento does,
    /// runs past the curve family.
    pub on_bound: bool,
}

/// What the search found.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct GlideRead {
    /// The glides, in time order.
    pub glides: Vec<GlideFit>,
    /// Motions between two stable pitches that lasted from
    /// [`GLIDE_STEP_MAX`] up to [`GLIDE_TIME_MIN`].
    pub too_short: usize,
}

/// One voiced frame of a run: time and pitch in cents (MIDI pitch times
/// 100).
#[derive(Clone, Copy, Debug)]
struct Frame {
    time: f64,
    cents: f64,
}

/// A stable pitch: frames `first..=last` of a run.
#[derive(Clone, Copy, Debug)]
struct Plateau {
    first: usize,
    last: usize,
    median: f64,
}

/// Finds the glides of a monophonic line.
///
/// `breaks` tells whether the sound breaks inside a span of the audio, as
/// a strike or a dip of the level does, `from_silence` whether a glide
/// from silence may start a voiced run whose sound starts inside a span.
/// A voiced run that starts within [`GAP_BRIDGE_MAX`] of the
/// previous one joins it when the sound does not break in the gap and the
/// pitch keeps moving after the gap in the direction of its jump across
/// it: the track can go unvoiced inside a glide, and at a step between two
/// notes.
#[must_use]
pub(crate) fn find_glides(
    track: &PitchTrack,
    notes: &NoteList,
    breaks: impl Fn(Seconds, Seconds) -> bool,
    from_silence: impl Fn(Seconds, Seconds) -> bool,
) -> GlideRead {
    let mut read = GlideRead::default();
    let audio = |frame: &Frame| frame.time - lag_at(frame.cents);
    let mut runs: Vec<Vec<Frame>> = Vec::new();
    for run in voiced_runs(track) {
        if let Some(previous) = runs.last_mut()
            && let Some(last) = previous.last()
            && run[0].time - last.time <= GAP_BRIDGE_MAX.get()
            && moves_on(last, &run)
            && !breaks(
                Seconds::clamped(audio(last)),
                Seconds::clamped(audio(&run[0])),
            )
        {
            previous.extend(run);
        } else {
            runs.push(run);
        }
    }
    for run in runs {
        let smooth = moving_average(&run);
        let slope = slopes(&run, &smooth);
        let plateaus = plateaus(&run, &smooth);
        let note_of = |plateau: &Plateau| {
            let middle = f64::midpoint(run[plateau.first].time, run[plateau.last].time);
            let audio = Seconds::clamped(middle - lag_at(plateau.median));
            notes
                .notes()
                .iter()
                .filter(|note| note.sounds_at(audio))
                .map(|note| note.pitch().get())
                .find(|pitch| (100.0 * pitch - plateau.median).abs() <= NOTE_MATCH.get())
                .map(f64::round)
        };
        if let Some(first) = plateaus.first()
            && first.first > 0
            && from_silence(
                Seconds::clamped(audio(&run[0]) - ONSET_LOOKBACK.get()),
                Seconds::clamped(audio(&run[0])),
            )
            && note_of(first).is_some()
        {
            let motion = detected(
                &run,
                &slope,
                0,
                first.first + (first.last - first.first) / 2,
            );
            let lead = run[0].cents - first.median;
            if lead.abs() >= GLIDE_INTERVAL_MIN.get()
                && let Some(span) = motion
            {
                classify(
                    &mut read,
                    span,
                    || fit_from_silence(&run, *first, span),
                    false,
                );
            }
        }
        for pair in plateaus.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let interval = b.median - a.median;
            if interval.abs() < GLIDE_INTERVAL_MIN.get() {
                continue;
            }
            let (Some(from), Some(to)) = (note_of(&a), note_of(&b)) else {
                continue;
            };
            if (from - to).abs() < 0.5 {
                continue;
            }
            let stepped = steps_across_gap(&run[a.last..=b.first], interval);
            let mid = |p: Plateau| p.first + (p.last - p.first) / 2;
            let signed: Vec<f64> = slope.iter().map(|s| s * interval.signum()).collect();
            if let Some(span) = detected(&run, &signed, mid(a), mid(b)) {
                classify(&mut read, span, || fit_between(&run, a, b, span), stepped);
            }
        }
    }
    read.glides
        .sort_by(|x, y| x.start.get().total_cmp(&y.start.get()));
    read
}

/// The steady spans of the track: its voiced runs, split where two stable
/// pitches lie [`STEADY_SPLIT`] or more apart and without the frames of
/// each glide, as `(time, f0)` frames in track time.
#[must_use]
pub(crate) fn steady_spans(track: &PitchTrack, glides: &[GlideFit]) -> Vec<Vec<(Seconds, Hz)>> {
    let in_glide = |frame: &Frame| {
        let audio = frame.time - lag_at(frame.cents);
        glides
            .iter()
            .any(|g| (g.start.get()..=g.start.get() + g.time.get()).contains(&audio))
    };
    let mut spans = Vec::new();
    for run in voiced_runs(track) {
        let smooth = moving_average(&run);
        let mut cuts = vec![0];
        for pair in plateaus(&run, &smooth).windows(2) {
            if (pair[1].median - pair[0].median).abs() >= STEADY_SPLIT.get() {
                cuts.push(pair[0].last + 1);
                cuts.push(pair[1].first);
            }
        }
        cuts.push(run.len());
        // The cuts come in pairs: each split ends one span and starts the
        // next, between 0 and the end of the run.
        for &[from, to] in cuts.as_chunks::<2>().0 {
            let mut span = Vec::new();
            for frame in &run[from..to] {
                if in_glide(frame) {
                    if !span.is_empty() {
                        spans.push(std::mem::take(&mut span));
                    }
                } else {
                    span.push((Seconds::clamped(frame.time), hz(frame.cents)));
                }
            }
            if !span.is_empty() {
                spans.push(span);
            }
        }
    }
    spans
}

/// Counts a motion as a step, a glide too short, or a glide.
///
/// A motion whose detected span is shorter than [`GLIDE_STEP_MAX`] is a
/// step. Otherwise the fitted time decides, with the same bounds: the
/// track smears a step between two 808 notes over a detected span of 100
/// to 450 ms, while the fit finds the motion short. A fit of
/// [`GLIDE_TIME_MIN`] or more whose `p` sits on the lower bound of
/// [`CURVE_RANGE`], or whose residual exceeds [`GLIDE_MAX_RESIDUAL`], does
/// not follow the curve, so the motion is no glide. On the upper bound the
/// residual must stay within [`BOUND_MAX_RESIDUAL`], the detected span
/// must last [`GLIDE_TIME_MIN`] or more and at most [`BOUND_SPAN_MAX`]
/// times the fitted time, the glide must not come from silence, and it is
/// marked: a melody step on a sine, a driven drop and a smeared step read
/// as glides on that bound otherwise. A `stepped` motion, a step across a
/// gap, is no glide either. A motion whose fit fails is too short when its
/// detected span is.
fn classify(
    read: &mut GlideRead,
    span: (f64, f64),
    fit: impl FnOnce() -> Option<GlideFit>,
    stepped: bool,
) {
    let length = span.1 - span.0;
    if length < GLIDE_STEP_MAX.get() {
        return;
    }
    let Some(glide) = fit() else {
        if length < GLIDE_TIME_MIN.get() {
            read.too_short += 1;
        }
        return;
    };
    if glide.time < GLIDE_STEP_MAX {
        return;
    }
    if glide.time < GLIDE_TIME_MIN {
        read.too_short += 1;
        return;
    }
    let (lo, hi) = CURVE_RANGE;
    let on_bound = glide.curve >= hi / CURVE_EDGE;
    let follows = if on_bound {
        glide.residual <= BOUND_MAX_RESIDUAL
            && !glide.from_silence
            && length >= GLIDE_TIME_MIN.get()
            && length <= BOUND_SPAN_MAX * glide.time.get()
    } else {
        glide.residual <= GLIDE_MAX_RESIDUAL
    };
    let glide = GlideFit { on_bound, ..glide };
    if glide.curve > CURVE_EDGE * lo && follows && !stepped {
        read.glides.push(glide);
    }
}

/// Whether the pitch keeps moving after an unvoiced gap: the jump from
/// `last` across the gap and the motion over the next two frames of `run`
/// go the same way, each by [`GAP_MOTION_MIN`] or more, and the jump is no
/// faster than [`GAP_SLOPE_MAX`]. At a step the track lands on the new
/// pitch and holds it, or swings back.
fn moves_on(last: &Frame, run: &[Frame]) -> bool {
    let [first, _, third, ..] = run else {
        return false;
    };
    let (jump, then) = (first.cents - last.cents, third.cents - first.cents);
    jump.abs() >= GAP_MOTION_MIN.get()
        && jump.abs() <= GAP_SLOPE_MAX * (first.time - last.time)
        && then.abs() >= GAP_MOTION_MIN.get()
        && jump.signum() == then.signum()
}

/// Whether the pitch steps across a bridged gap among `frames`, in time
/// order: it jumps against `interval` by [`GAP_MOTION_MIN`] or more, or by
/// more than [`GAP_SHARE_MAX`] of it.
fn steps_across_gap(frames: &[Frame], interval: f64) -> bool {
    let hop = cast::usize_to_f64(TRACK_HOP) / MODEL_RATE.hz().get();
    frames.windows(2).any(|pair| {
        let jump = pair[1].cents - pair[0].cents;
        pair[1].time - pair[0].time > 1.5 * hop
            && (jump * interval < 0.0 && jump.abs() >= GAP_MOTION_MIN.get()
                || jump.abs() > GAP_SHARE_MAX * interval.abs())
    })
}

/// Splits the track into runs of consecutive voiced frames.
fn voiced_runs(track: &PitchTrack) -> Vec<Vec<Frame>> {
    let mut runs = Vec::new();
    let mut run = Vec::new();
    for frame in track.frames() {
        if let Some(f0) = frame.f0.filter(|f0| f0.get() > 0.0) {
            run.push(Frame {
                time: frame.time.get(),
                cents: 6900.0 + 1200.0 * (f0.get() / 440.0).log2(),
            });
        } else if !run.is_empty() {
            runs.push(std::mem::take(&mut run));
        }
    }
    if !run.is_empty() {
        runs.push(run);
    }
    runs
}

/// The centred moving average over [`SMOOTH_FRAMES`], shorter at the ends.
fn moving_average(run: &[Frame]) -> Vec<f64> {
    let half = SMOOTH_FRAMES / 2;
    (0..run.len())
        .map(|i| {
            let window = &run[i.saturating_sub(half)..(i + half + 1).min(run.len())];
            window.iter().map(|f| f.cents).sum::<f64>() / cast::usize_to_f64(window.len())
        })
        .collect()
}

/// The central difference of the smoothed pitch, in cents per second.
fn slopes(run: &[Frame], smooth: &[f64]) -> Vec<f64> {
    (0..run.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(1), (i + 1).min(run.len() - 1));
            let dt = run[b].time - run[a].time;
            if dt > 0.0 {
                (smooth[b] - smooth[a]) / dt
            } else {
                0.0
            }
        })
        .collect()
}

/// The stable pitches of a run, in time order.
fn plateaus(run: &[Frame], smooth: &[f64]) -> Vec<Plateau> {
    let mut found = Vec::new();
    let mut i = 0;
    while i < run.len() {
        let mut fit = LineSums::default();
        fit.add(run[i].time, smooth[i]);
        let mut j = i;
        while j + 1 < run.len() {
            let mut next = fit;
            next.add(run[j + 1].time, smooth[j + 1]);
            let within = (smooth[j + 1] - fit.mean()).abs() <= PLATEAU_RANGE.get();
            let long = run[j + 1].time - run[i].time >= PLATEAU_MIN.get();
            if !within || (long && next.slope().abs() > GLIDE_SLOPE_MIN) {
                break;
            }
            fit = next;
            j += 1;
        }
        if run[j].time - run[i].time >= PLATEAU_MIN.get() {
            let mut cents: Vec<f64> = run[i..=j].iter().map(|f| f.cents).collect();
            cents.sort_by(f64::total_cmp);
            found.push(Plateau {
                first: i,
                last: j,
                median: cents[cents.len() / 2],
            });
            i = j + 1;
        } else {
            i += 1;
        }
    }
    found
}

/// Running sums of a least-squares line.
#[derive(Clone, Copy, Debug, Default)]
struct LineSums {
    n: f64,
    t: f64,
    y: f64,
    tt: f64,
    ty: f64,
}

impl LineSums {
    fn add(&mut self, t: f64, y: f64) {
        self.n += 1.0;
        self.t += t;
        self.y += y;
        self.tt += t * t;
        self.ty += t * y;
    }

    fn mean(&self) -> f64 {
        self.y / self.n
    }

    fn slope(&self) -> f64 {
        let det = self.n * self.tt - self.t * self.t;
        if det <= 0.0 {
            0.0
        } else {
            (self.n * self.ty - self.t * self.y) / det
        }
    }
}

/// The first and last time between frames `from` and `to` whose signed
/// slope reaches [`GLIDE_SLOPE_MIN`], or `None` when none does.
fn detected(run: &[Frame], signed: &[f64], from: usize, to: usize) -> Option<(f64, f64)> {
    let moving = |&k: &usize| signed[k] >= GLIDE_SLOPE_MIN;
    let mut span = from..=to.min(run.len() - 1);
    Some((
        run[span.clone().find(moving)?].time,
        run[span.rfind(moving)?].time,
    ))
}

/// The curve between `c0` and `c1` at time `t` for a start `t0`, a full
/// time `time` and a power `p`.
fn model(t: f64, c0: f64, c1: f64, t0: f64, time: f64, p: f64) -> f64 {
    let x = ((t - t0) / time).clamp(0.0, 1.0);
    c0 + (c1 - c0) * (1.0 - (1.0 - x).powf(p))
}

/// The squared error of the curve over `frames`, and a large value outside
/// the bounds of `p` and the time.
fn sse(frames: &[Frame], c0: f64, c1: f64, t0: f64, log_time: f64, log_p: f64) -> f64 {
    let p = log_p.exp();
    if !(CURVE_RANGE.0..=CURVE_RANGE.1).contains(&p) || !(-5.0..=1.0).contains(&log_time) {
        return f64::INFINITY;
    }
    let time = log_time.exp();
    frames
        .iter()
        .map(|f| (f.cents - model(f.time, c0, c1, t0, time, p)).powi(2))
        .sum()
}

/// The coarse grid of curve powers, times (as shares of the detected span)
/// and start shifts that the simplex starts from.
const P_GRID: [f64; 9] = [0.25, 0.35, 0.5, 0.7, 1.0, 1.4, 2.0, 2.8, 4.0];
const TIME_GRID: [f64; 8] = [0.7, 0.85, 1.0, 1.2, 1.45, 1.75, 2.1, 2.5];

/// Fits a glide between two stable pitches.
fn fit_between(run: &[Frame], a: Plateau, b: Plateau, span: (f64, f64)) -> Option<GlideFit> {
    let window = window(
        run,
        run[a.last].time - FIT_MARGIN,
        run[b.first].time + FIT_MARGIN,
    );
    let (c0, c1) = (a.median, b.median);
    let length = span.1 - span.0;
    let grid = P_GRID.iter().flat_map(|&p| {
        TIME_GRID.iter().flat_map(move |&share| {
            (-8..=4).map(move |shift| {
                [
                    span.0 + 0.005 * f64::from(shift),
                    (length * share).ln(),
                    p.ln(),
                ]
            })
        })
    });
    let ([t0, log_time, log_p], value) = seeded_fit(
        |x: &[f64; 3]| sse(window, c0, c1, x[0], x[1], x[2]),
        [span.0, length.ln(), 0.0],
        grid,
        [0.005, 0.1, 0.1],
    );
    let jitter = jitter(&run[a.first..=a.last], c0, &run[b.first..=b.last], c1);
    glide(
        t0,
        c0,
        c1 - c0,
        log_time,
        log_p,
        value,
        window.len(),
        false,
        length,
        jitter,
    )
}

/// Fits a glide from the start of a voiced run into its first stable
/// pitch: the run's first frame fixes the start, and the start pitch is a
/// free parameter.
fn fit_from_silence(run: &[Frame], b: Plateau, span: (f64, f64)) -> Option<GlideFit> {
    let window = window(run, run[0].time, run[b.first].time + FIT_MARGIN);
    let c1 = b.median;
    let t0 = run[0].time;
    let length = span.1 - t0;
    let grid = P_GRID.iter().flat_map(|&p| {
        TIME_GRID
            .iter()
            .map(move |&share| [run[0].cents, (length * share).ln(), p.ln()])
    });
    let ([c0, log_time, log_p], value) = seeded_fit(
        |x: &[f64; 3]| sse(window, x[0], c1, t0, x[1], x[2]),
        [run[0].cents, length.ln(), 0.0],
        grid,
        [10.0, 0.1, 0.1],
    );
    let jitter = jitter(&[], c1, &run[b.first..=b.last], c1);
    glide(
        t0,
        c0,
        c1 - c0,
        log_time,
        log_p,
        value,
        window.len(),
        true,
        span.1 - span.0,
        jitter,
    )
}

/// Runs a Nelder-Mead simplex from the first point of `grid` with the
/// smallest `objective`, or from `fallback` when none is below infinity,
/// with initial steps `step`.
fn seeded_fit(
    objective: impl Fn(&[f64; 3]) -> f64,
    fallback: [f64; 3],
    grid: impl IntoIterator<Item = [f64; 3]>,
    step: [f64; 3],
) -> ([f64; 3], f64) {
    let mut start = (f64::INFINITY, fallback);
    for point in grid {
        let value = objective(&point);
        if value < start.0 {
            start = (value, point);
        }
    }
    nelder_mead(&objective, start.1, step, 2_000, 1e-10)
}

/// The frames of `run` from `from` to `to`.
fn window(run: &[Frame], from: f64, to: f64) -> &[Frame] {
    let start = run.partition_point(|f| f.time < from);
    let end = run.partition_point(|f| f.time <= to);
    &run[start..end.max(start)]
}

/// The RMS deviation of two stable pitches from their medians.
fn jitter(a: &[Frame], ca: f64, b: &[Frame], cb: f64) -> f64 {
    let squares: f64 = a.iter().map(|f| (f.cents - ca).powi(2)).sum::<f64>()
        + b.iter().map(|f| (f.cents - cb).powi(2)).sum::<f64>();
    (squares / cast::usize_to_f64((a.len() + b.len()).max(1))).sqrt()
}

#[expect(clippy::too_many_arguments, reason = "the fields of one record")]
fn glide(
    t0: f64,
    c0: f64,
    interval: f64,
    log_time: f64,
    log_p: f64,
    sse: f64,
    frames: usize,
    from_silence: bool,
    detected: f64,
    jitter: f64,
) -> Option<GlideFit> {
    if frames == 0 || !sse.is_finite() {
        return None;
    }
    Some(GlideFit {
        start: Seconds::new(t0 - lag_at(c0)).ok()?,
        interval: Semitones::new(interval / 100.0).ok()?,
        time: Seconds::new(log_time.exp()).ok()?,
        curve: log_p.exp(),
        from_silence,
        detected: Seconds::new(detected).ok()?,
        residual: Cents::new((sse / cast::usize_to_f64(frames)).sqrt()).ok()?,
        jitter: Cents::new(jitter).ok()?,
        on_bound: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::{Note, TrackFrame};
    use crate::units::{Decibels, MidiPitch};

    #[test]
    fn the_convention_states_the_constants() {
        use super::super::{
            DIP_SIDE, DIP_WINDOW, GAP_DIP_DB, RUN_START_RISE, STRIKE_RISE, num, ratio_db, says,
        };
        use crate::dsp::ACTIVE_GATE_DB;
        let ms = |s: f64| num(1000.0 * s);
        let quarter = 1000.0 * cast::usize_to_f64(TRACK_FRAME / 4) / MODEL_RATE.hz().get();
        says(
            GLIDE_CONVENTION,
            &[
                format!(
                    "a span of {} ms or more within {} cents of its running mean whose \
                     least-squares slope stays at {} cents per second or less",
                    ms(PLATEAU_MIN.get()),
                    num(PLATEAU_RANGE.get()),
                    num(GLIDE_SLOPE_MIN)
                ),
                format!(
                    "{} cents or more apart that match notes of different pitch within {} cents, \
                     where the slope of the {}-frame moving average stays at {} cents per second",
                    num(GLIDE_INTERVAL_MIN.get()),
                    num(NOTE_MATCH.get()),
                    SMOOTH_FRAMES,
                    num(GLIDE_SLOPE_MIN)
                ),
                format!("over {} ms or more;", ms(GLIDE_TIME_MIN.get())),
                format!("over the motion and {} ms of each", ms(FIT_MARGIN)),
                format!("a quarter frame, {quarter:.1} ms"),
                format!(
                    "the full-band RMS rising {}, up to {} ms before it",
                    ratio_db(RUN_START_RISE),
                    ms(ONSET_LOOKBACK.get())
                ),
                format!(
                    "within {} ms of the one before joins it when the full-band RMS does not \
                     rise {} in the gap, its {} ms RMS stays within {} dB of the {} ms before or \
                     after the gap and within {} dB of the loudest frame",
                    ms(GAP_BRIDGE_MAX.get()),
                    ratio_db(STRIKE_RISE),
                    ms(DIP_WINDOW),
                    num(GAP_DIP_DB.get()),
                    ms(DIP_SIDE),
                    num(ACTIVE_GATE_DB.get())
                ),
                format!(
                    "at {} cents per millisecond or slower",
                    num(GAP_SLOPE_MAX / 1000.0)
                ),
                format!(
                    "against the interval by {} cents or more, or carries more than {} % of it",
                    num(GAP_MOTION_MIN.get()),
                    num(100.0 * GAP_SHARE_MAX)
                ),
                format!(
                    "motion of {} to {} ms",
                    ms(GLIDE_STEP_MAX.get()),
                    ms(GLIDE_TIME_MIN.get())
                ),
                format!(
                    "a fit of {} ms or more with p within {} % of {}",
                    ms(GLIDE_TIME_MIN.get()),
                    num(100.0 * (CURVE_EDGE - 1.0)),
                    num(CURVE_RANGE.0)
                ),
                format!(
                    "with p within {} % of {} and an RMS residual above {} cents, a detected \
                     span under {} ms or over {} times the fitted time",
                    num(100.0 * (CURVE_EDGE - 1.0)),
                    num(CURVE_RANGE.1),
                    num(BOUND_MAX_RESIDUAL.get()),
                    ms(GLIDE_TIME_MIN.get()),
                    num(BOUND_SPAN_MAX)
                ),
                format!(
                    "an RMS residual above {} cents, is no glide",
                    num(GLIDE_MAX_RESIDUAL.get())
                ),
            ],
        );
    }

    const HOP: f64 = 128.0 / 22_050.0;

    /// A track whose pitch follows `cents(t)` from 0 to `length`, voiced
    /// where `cents` returns a value, read a quarter frame late and on the
    /// tracker's grid of 10 cents.
    fn track(length: f64, cents: impl Fn(f64) -> Option<f64>) -> PitchTrack {
        let count = cast::floor_to_usize(length / HOP).unwrap();
        PitchTrack::from_frames(
            (0..count)
                .map(|k| {
                    let t = cast::usize_to_f64(k) * HOP;
                    let guess = t - track_lag(Hz::new(150.0).unwrap()).get();
                    let lag = cents(guess).map_or(0.0, lag_at);
                    let audio = t - lag;
                    TrackFrame {
                        time: Seconds::new(t).unwrap(),
                        f0: cents(audio).map(|c| {
                            let c = (c / 10.0).round() * 10.0;
                            Hz::new(440.0 * 2f64.powf((c - 6900.0) / 1200.0)).unwrap()
                        }),
                        level: Decibels::new(0.0).unwrap(),
                    }
                })
                .collect(),
        )
    }

    fn notes(list: &[(f64, f64, f64)]) -> NoteList {
        NoteList::new(
            list.iter()
                .map(|&(on, off, pitch)| {
                    Note::new(
                        Seconds::new(on).unwrap(),
                        Seconds::new(off).unwrap(),
                        MidiPitch::new(pitch).unwrap(),
                    )
                    .unwrap()
                })
                .collect(),
        )
    }

    #[test]
    fn a_jump_across_a_gap_faster_than_30_cents_per_ms_is_no_glide_motion() {
        let frame = |time: f64, cents: f64| Frame { time, cents };
        let last = frame(0.0, 0.0);
        // Four hops apart: 1000 cents is 43 per ms, 300 cents 13.
        for (jump, bridged) in [(-1000.0, false), (-300.0, true)] {
            let run = [
                frame(4.0 * HOP, jump),
                frame(5.0 * HOP, jump - 30.0),
                frame(6.0 * HOP, jump - 60.0),
            ];
            assert_eq!(moves_on(&last, &run), bridged, "{jump}");
        }
    }

    #[test]
    fn a_jump_across_a_gap_that_carries_most_of_the_interval_is_a_step() {
        let frame = |time: f64, cents: f64| Frame { time, cents };
        let gap = [
            frame(0.0, 6900.0),
            frame(HOP, 6890.0),
            frame(5.0 * HOP, 6440.0),
            frame(6.0 * HOP, 6200.0),
        ];
        // 450 cents is 0.64 of a fall of 700 and 0.5 of 900; against a
        // rise it is a step at any share.
        assert!(steps_across_gap(&gap, -700.0));
        assert!(!steps_across_gap(&gap, -900.0));
        assert!(steps_across_gap(&gap, 900.0));
        // One hop apart the frames hold no gap.
        let close = [frame(0.0, 6900.0), frame(HOP, 6440.0)];
        assert!(!steps_across_gap(&close, -700.0));
    }

    fn glide_track(full: f64, p: f64) -> PitchTrack {
        track(1.6, |t| {
            (0.0..1.5 + full).contains(&t).then(|| {
                let x = ((t - 0.5) / full).clamp(0.0, 1.0);
                4500.0 + 700.0 * (1.0 - (1.0 - x).powf(p))
            })
        })
    }

    #[test]
    fn a_curve_on_the_tracker_grid_reads_its_time_and_power() {
        let line = notes(&[(0.0, 0.55, 45.0), (0.55, 1.6, 52.0)]);
        for (full, p) in [(0.15, 1.0), (0.15, 2.5), (0.4, 1.0), (0.4, 2.5), (0.3, 0.5)] {
            let read = find_glides(&glide_track(full, p), &line, |_, _| false, |_, _| true);
            assert_eq!(read.glides.len(), 1, "{full} {p}: {read:?}");
            let glide = read.glides[0];
            assert!(
                (glide.time.get() - full).abs() < 0.05 * full,
                "{full} {p}: {glide:?}"
            );
            assert!((glide.curve - p).abs() < 0.2 * p, "{full} {p}: {glide:?}");
            assert!(
                (glide.start.get() - 0.5).abs() < 0.006,
                "{full} {p}: {glide:?}"
            );
            assert!((glide.interval.get() - 7.0).abs() < 0.1, "{glide:?}");
            assert!(!glide.from_silence);
        }
    }

    #[test]
    fn a_step_reports_nothing_and_a_short_glide_is_too_short() {
        let line = notes(&[(0.0, 0.5, 45.0), (0.5, 1.6, 52.0)]);
        let step = find_glides(&glide_track(0.001, 1.0), &line, |_, _| false, |_, _| true);
        assert_eq!(step, GlideRead::default());
        let short = find_glides(&glide_track(0.05, 1.0), &line, |_, _| false, |_, _| true);
        assert!(short.glides.is_empty(), "{short:?}");
        assert_eq!(short.too_short, 1);
    }

    #[test]
    fn motion_inside_one_note_is_not_a_glide() {
        let vibrato = track(1.6, |t| {
            Some(5700.0 + 60.0 * (std::f64::consts::TAU * 3.0 * t).sin())
        });
        let read = find_glides(
            &vibrato,
            &notes(&[(0.0, 1.6, 57.0)]),
            |_, _| false,
            |_, _| true,
        );
        assert_eq!(read, GlideRead::default());
    }

    #[test]
    fn a_glide_into_the_first_note_comes_from_silence() {
        let rising = track(1.0, |t| {
            (0.2..1.0).contains(&t).then(|| {
                let x = ((t - 0.2) / 0.2).clamp(0.0, 1.0);
                4500.0 + 500.0 * x
            })
        });
        let line = notes(&[(0.2, 1.0, 50.0)]);
        let read = find_glides(&rising, &line, |_, _| false, |_, _| true);
        assert_eq!(read.glides.len(), 1, "{read:?}");
        let glide = read.glides[0];
        assert!(glide.from_silence);
        assert!((glide.interval.get() - 5.0).abs() < 0.3, "{glide:?}");
        let skipped = find_glides(&rising, &line, |_, _| false, |_, _| false);
        assert_eq!(skipped.glides, []);
    }
}
