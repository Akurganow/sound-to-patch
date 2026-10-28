//! Glides: pitch moving from one stable pitch to another, read from the f0
//! track, with the curve `s(x) = 1 - (1 - x)^p` fitted between the two.

use super::fit::nelder_mead;
use crate::cast;
use crate::notes::{MODEL_RATE, NoteList, PitchTrack, TRACK_FRAME};
use crate::units::{Cents, Hz, Seconds, Semitones};

/// The slope above which the pitch moves, in cents per second: 0.861
/// semitone per second, the floor of a published portamento detector.
/// Untuned.
pub const GLIDE_SLOPE_MIN: f64 = 86.0;
/// The shortest glide, over its detected span and over its fitted time.
pub const GLIDE_TIME_MIN: Seconds = Seconds::from_const(0.09);
/// Motion shorter than this over its detected span is a step between two
/// notes.
///
/// From here up to [`GLIDE_TIME_MIN`] it is a glide too short to read. Six
/// hops: the moving average alone spreads a jump over five.
pub const GLIDE_STEP_MAX: Seconds = Seconds::from_const(0.035);
/// Two stable pitches this far apart split a steady span, so the vibrato
/// search never reads a step between two notes. A vibrato of ±60 cents
/// moves its stable pitches 120 cents apart and stays whole.
pub const STEADY_SPLIT: Cents = Cents::from_const(150.0);
/// The smallest interval between the two stable pitches.
pub const GLIDE_INTERVAL_MIN: Cents = Cents::from_const(100.0);
/// A stable pitch lasts at least this long.
pub const PLATEAU_MIN: Seconds = Seconds::from_const(0.05);
/// A stable pitch stays within this many cents of its mean.
pub const PLATEAU_RANGE: Cents = Cents::from_const(30.0);
/// A stable pitch belongs to a note whose pitch lies within this many
/// cents of it.
pub const NOTE_MATCH: Cents = Cents::from_const(50.0);
/// Frames of the centred moving average that the slope reads: 5 hops,
/// 29 ms.
pub const SMOOTH_FRAMES: usize = 5;
/// The range of the curve power `p`.
pub const CURVE_RANGE: (f64, f64) = (0.2, 5.0);
/// A fitted `p` within this factor of a bound of [`CURVE_RANGE`] sits on
/// the bound.
pub const CURVE_EDGE: f64 = 1.05;
/// The largest RMS residual of a glide whose `p` sits on the upper bound
/// of [`CURVE_RANGE`]. Exponential portamento, `1 - exp(-t / tau)`, fits
/// the bound within 20 cents; a smeared step on that bound fits worse.
pub const BOUND_MAX_RESIDUAL: Cents = Cents::from_const(20.0);
/// The largest RMS residual of a glide of 90 ms or more. The glides of the
/// generated set fit within 22 cents.
pub const GLIDE_MAX_RESIDUAL: Cents = Cents::from_const(40.0);
/// The longest unvoiced gap between two voiced runs that the glide search
/// bridges. A beating unison can leave the track unvoiced for 75 ms inside
/// a glide.
pub const GAP_BRIDGE_MAX: Seconds = Seconds::from_const(0.1);
/// The smallest jump across a bridged gap, and the smallest motion over
/// the two frames after it: two steps of the tracker's 10-cent grid.
pub const GAP_MOTION_MIN: Cents = Cents::from_const(20.0);
/// A glide from silence needs a sound to start at most this long before
/// its voiced run.
pub const ONSET_LOOKBACK: Seconds = Seconds::from_const(0.1);
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
pub fn track_lag(f0: Hz) -> Seconds {
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
before joins it when the full-band RMS does not rise 6 dB in the gap and the pitch keeps moving \
after the gap the way it jumped; motion of 35 to 90 ms, over the detected span or the fitted \
time, counts as too short; a fit of 90 ms or more with p within 5 % of 0.2, with p within 5 % \
of 5 and an RMS residual above 20 cents, a detected span under 90 ms or a start from silence, \
or with an RMS residual above 40 cents, is no glide; a glide with p within 5 % of 5 reads at low \
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
    /// The number of frames the fit read.
    pub frames: usize,
}

/// What the search found.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GlideRead {
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
/// `struck` tells whether a sound starts or is struck again inside a span
/// of the audio, and `from_silence` whether a glide from silence may start
/// a voiced run whose sound starts inside a span. A voiced run that starts
/// within [`GAP_BRIDGE_MAX`] of the previous one joins it when nothing is
/// struck in the gap and the pitch keeps moving after the gap in the
/// direction of its jump across it: the track can go unvoiced inside a
/// glide, and at a step between two notes.
#[must_use]
pub fn find_glides(
    track: &PitchTrack,
    notes: &NoteList,
    struck: impl Fn(Seconds, Seconds) -> bool,
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
            && !struck(
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
                classify(&mut read, span, || fit_from_silence(&run, *first, span));
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
            let mid = |p: Plateau| p.first + (p.last - p.first) / 2;
            let signed: Vec<f64> = slope.iter().map(|s| s * interval.signum()).collect();
            if let Some(span) = detected(&run, &signed, mid(a), mid(b)) {
                classify(&mut read, span, || fit_between(&run, a, b, span));
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
pub fn steady_spans(track: &PitchTrack, glides: &[GlideFit]) -> Vec<Vec<(Seconds, Hz)>> {
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
        for bounds in cuts.chunks(2) {
            let [from, to] = [bounds[0], *bounds.get(1).unwrap_or(&run.len())];
            let mut span = Vec::new();
            for frame in &run[from..to.max(from)] {
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
/// residual must stay within [`BOUND_MAX_RESIDUAL`], the detected span must
/// last [`GLIDE_TIME_MIN`] or more, the glide must not come from silence,
/// and it is marked: a melody step on a sine and a driven drop read as
/// glides on that bound otherwise. A motion whose fit fails is too short when its detected span
/// is.
fn classify(read: &mut GlideRead, span: (f64, f64), fit: impl FnOnce() -> Option<GlideFit>) {
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
    } else {
        glide.residual <= GLIDE_MAX_RESIDUAL
    };
    if glide.curve > CURVE_EDGE * lo && follows {
        read.glides.push(GlideFit { on_bound, ..glide });
    }
}

/// Whether the pitch keeps moving after an unvoiced gap: the jump from
/// `last` across the gap and the motion over the next two frames of `run`
/// go the same way, each by [`GAP_MOTION_MIN`] or more. At a step the
/// track lands on the new pitch and holds it, or swings back.
fn moves_on(last: &Frame, run: &[Frame]) -> bool {
    let [first, _, third, ..] = run else {
        return false;
    };
    let (jump, then) = (first.cents - last.cents, third.cents - first.cents);
    jump.abs() >= GAP_MOTION_MIN.get()
        && then.abs() >= GAP_MOTION_MIN.get()
        && jump.signum() == then.signum()
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
    let moving: Vec<usize> = (from..=to.min(run.len() - 1))
        .filter(|&k| signed[k] >= GLIDE_SLOPE_MIN)
        .collect();
    Some((run[*moving.first()?].time, run[*moving.last()?].time))
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
    let mut start = (f64::INFINITY, [span.0, length.ln(), 0.0]);
    for &p in &P_GRID {
        for &share in &TIME_GRID {
            for shift in -8..=4 {
                let t0 = span.0 + 0.005 * f64::from(shift);
                let point = [t0, (length * share).ln(), p.ln()];
                let value = sse(window, c0, c1, point[0], point[1], point[2]);
                if value < start.0 {
                    start = (value, point);
                }
            }
        }
    }
    let ([t0, log_time, log_p], value) = nelder_mead(
        |x: &[f64; 3]| sse(window, c0, c1, x[0], x[1], x[2]),
        start.1,
        [0.005, 0.1, 0.1],
        2_000,
        1e-10,
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
    let mut start = (f64::INFINITY, [run[0].cents, length.ln(), 0.0]);
    for &p in &P_GRID {
        for &share in &TIME_GRID {
            let point = [run[0].cents, (length * share).ln(), p.ln()];
            let value = sse(window, point[0], c1, t0, point[1], point[2]);
            if value < start.0 {
                start = (value, point);
            }
        }
    }
    let ([c0, log_time, log_p], value) = nelder_mead(
        |x: &[f64; 3]| sse(window, x[0], c1, t0, x[1], x[2]),
        start.1,
        [10.0, 0.1, 0.1],
        2_000,
        1e-10,
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
        frames,
        on_bound: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::{Note, TrackFrame};
    use crate::units::{Decibels, MidiPitch};

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
