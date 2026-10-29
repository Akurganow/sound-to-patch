//! The repeat-render spread: the noise floor that every later comparison
//! cites.
//!
//! Renders of one preset differ a little. On one Vital instance each
//! render differs from the one before, and a fresh instance renders
//! bit-identically to any other fresh instance: an ignored Vital test pins
//! both. So [`draw`] renders the [`SPREAD_RUNS`] draws of a spread in
//! sequence on one instance and refuses a set with a repeated render.
//!
//! For each output the spread is the 95th percentile of the distances over
//! every pair of renders, 190 pairs for 20 renders, in the output's own
//! unit. The distance of a scalar output is the absolute difference.
//!
//! A clip compares with the renders by the median of its distances from
//! them. Its tolerance comes from the same median taken for each render
//! against the other renders, so the clip and the renders are measured
//! alike: the tolerance is the median at rank [`tolerance_rank`], which a
//! clip of the same sound exceeds 1 time in 20. A floor keeps an output
//! that every render repeats from turning the comparison into an equality
//! test.
//!
//! A recording made elsewhere compares through [`compare_fitted`]: one
//! gain matches its level to the renders, and the verdict covers the
//! outputs of the sound's shape, [`ClipOutput::SHAPE`].
//!
//! The repository keeps no spread of record. The reference recordings page
//! holds the metrics of each recording.

use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::num::NonZeroUsize;

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::recipe::Recipe;
use super::session::{BoxedRenderer, RESTART_AFTER, RenderOutcome, RenderSession};
use super::{HostIdentity, Oversampling, RenderError, RenderJob, Rendered};
use crate::audio::{Audio, AudioError, SampleRate};
use crate::cast;
use crate::dsp::{Summary, quantile};
use crate::units::{Bpm, Decibels};

/// How many renders a spread takes.
pub const SPREAD_RUNS: usize = 20;
/// The fewest renders a spread takes: fewer leave no leave-one-out median
/// of two distances.
pub const MIN_RUNS: usize = 3;
/// The quantile of the pairwise distances.
pub const SPREAD_QUANTILE: f64 = 0.95;
/// The share of clips of the same sound, in percent, that the tolerance
/// covers.
pub const COVERAGE_PERCENT: usize = 95;
/// The name of the spread statistic, as the spread file records it.
pub const STATISTIC: &str = "p95_pairwise_distance";
/// The name of the comparison tolerance, as the spread file records it.
pub const TOLERANCE: &str = "conformal_95_leave_one_out_median";
/// A clip starts at its first sample within this many dB of its peak, so
/// a lead-in of silence or latency does not count.
pub const ONSET_RANGE_DB: Decibels = Decibels::from_const(40.0);
/// The envelope frames per second: one frame is 10 ms.
pub const ENVELOPE_FRAMES_PER_S: u32 = 100;
/// The envelope distance counts the frames within this many dB of the
/// loudest frame of either clip.
pub const ENVELOPE_RANGE_DB: Decibels = Decibels::from_const(40.0);
/// The floor of a tolerance in dB: a level difference below it counts as
/// none.
pub const LEVEL_FLOOR_DB: f64 = 0.01;

/// The level of a silent frame, in dB: far below any level that counts.
const SILENT_DB: f64 = -400.0;

/// A spread that cannot be measured or compared.
#[derive(Debug)]
pub enum SpreadError {
    /// A render failed on both tries.
    Render(String),
    /// A spread asked for too few or too many renders.
    Runs(usize),
    /// Some renders repeat others, so they are not independent draws.
    Repeated {
        /// How many renders differ.
        distinct: usize,
        /// How many renders came back.
        renders: usize,
    },
    /// A render or the clip is silent.
    Silent(String),
    /// Two clips have different sample rates.
    SampleRate {
        /// The clip's rate.
        clip: SampleRate,
        /// The renders' rate.
        renders: SampleRate,
    },
    /// A number of the layer report has a field name with no known unit.
    UnknownUnit(String),
    /// A comparison found no spread row for a clip output.
    MissingRow(&'static str),
    /// A recording could not be scaled by its fitted gain.
    Audio(AudioError),
}

impl fmt::Display for SpreadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Render(cause) => write!(f, "a render of the spread failed: {cause}"),
            Self::Runs(runs) => write!(
                f,
                "a spread takes {MIN_RUNS} to {RESTART_AFTER} renders, not {runs}: fewer leave no leave-one-out median, and more pass the restart, whose fresh instance repeats the first draw. Give a count in that range"
            ),
            Self::Repeated { distinct, renders } => write!(
                f,
                "only {distinct} of {renders} renders differ, so the spread would count repeats as draws. Render the draws in sequence on one plug-in instance, with no restart between them"
            ),
            Self::Silent(what) => {
                write!(f, "{what} is silent: no sample lies above the tail floor")
            }
            Self::SampleRate { clip, renders } => write!(
                f,
                "the clip runs at {clip} and the renders at {renders}. Render at the clip's sample rate"
            ),
            Self::UnknownUnit(path) => write!(
                f,
                "the layer report number {path} has no known unit. Name the field with a unit suffix, or add it to the unitless fields of render::spread"
            ),
            Self::Audio(error) => write!(f, "the recording cannot take its fitted gain: {error}"),
            Self::MissingRow(output) => write!(
                f,
                "the spread has no row for {output}, so the comparison would pass without it. Compare with the rows of clip_rows"
            ),
        }
    }
}

impl std::error::Error for SpreadError {}

/// Renders `job` `count` times in sequence on one renderer that `spawn`
/// starts.
///
/// A fresh instance renders the same clip as any other fresh one, and each
/// later render on one instance differs. So the draws come from one
/// instance, and the set fails [`distinct`] when a render repeats. A retry
/// after a crash starts a fresh instance, whose first render repeats the
/// first draw, so it fails too. For the same reason `count` stops at
/// [`RESTART_AFTER`], where the session restarts its instance.
///
/// # Errors
///
/// Returns [`SpreadError::Runs`] for a `count` below [`MIN_RUNS`] or above
/// [`RESTART_AFTER`], [`SpreadError::Render`] for a render that failed on
/// both tries and [`SpreadError::Repeated`] for a set with a repeated
/// render.
pub fn draw(
    spawn: impl Fn(usize) -> Result<BoxedRenderer, RenderError> + Send + Sync + 'static,
    job: &RenderJob,
    count: usize,
) -> Result<Vec<Rendered>, SpreadError> {
    if count < MIN_RUNS || !u64::try_from(count).is_ok_and(|count| count <= RESTART_AFTER) {
        return Err(SpreadError::Runs(count));
    }
    let session = RenderSession::new(NonZeroUsize::MIN, spawn);
    let renders = session
        .render_all(&vec![job.clone(); count])
        .into_iter()
        .map(|outcome| match outcome {
            RenderOutcome::Rendered(rendered) => Ok(rendered),
            RenderOutcome::Failed(failure) => Err(SpreadError::Render(
                failure
                    .errors
                    .last()
                    .map_or_else(|| "no error".to_owned(), ToString::to_string),
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    distinct(&renders)?;
    Ok(renders)
}

/// Checks that no two renders hold the same samples, by the SHA-256 of
/// each render's sample rate and samples.
///
/// # Errors
///
/// Returns [`SpreadError::Repeated`] when a render repeats another.
pub fn distinct(renders: &[Rendered]) -> Result<(), SpreadError> {
    let hashes: HashSet<[u8; 32]> = renders
        .iter()
        .map(|rendered| {
            let audio = &rendered.audio;
            let mut hash = Sha256::new();
            hash.update(audio.sample_rate().get().to_le_bytes());
            for channel in audio.channels() {
                for sample in channel {
                    hash.update(sample.to_le_bytes());
                }
            }
            hash.finalize().into()
        })
        .collect();
    if hashes.len() == renders.len() {
        Ok(())
    } else {
        Err(SpreadError::Repeated {
            distinct: hashes.len(),
            renders: renders.len(),
        })
    }
}

/// What a comparison measures of one clip.
///
/// The clip runs from its onset, the first sample within [`ONSET_RANGE_DB`]
/// of the peak, to its end, the last sample above the recipe's tail floor.
/// Both channels count.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipMetrics {
    /// The sample rate of the clip.
    pub sample_rate: SampleRate,
    /// The RMS level from onset to end, in dBFS.
    pub rms_db: f64,
    /// The largest sample magnitude, in dBFS.
    pub peak_db: f64,
    /// From onset to end, in s.
    pub duration_s: f64,
    /// The RMS level of each 10 ms frame from the onset to the end, in
    /// dBFS.
    pub envelope_db: Vec<f64>,
    /// The mean square over the channels of each frame from the onset to
    /// the end, summed: entry `n` covers the first `n` frames.
    energy: Vec<f64>,
}

impl ClipMetrics {
    /// Measures `audio`.
    ///
    /// # Errors
    ///
    /// Returns [`SpreadError::Silent`], naming `what`, for a clip whose
    /// every sample lies at or below the tail floor.
    pub fn of(audio: &Audio, what: &str) -> Result<Self, SpreadError> {
        let silent = || SpreadError::Silent(what.to_owned());
        let channels = audio.channels();
        let magnitude = |frame: usize| {
            channels
                .iter()
                .map(|channel| channel[frame].abs())
                .fold(0.0, f64::max)
        };
        let frames = audio.frames();
        let peak = (0..frames).map(magnitude).fold(0.0, f64::max);
        let floor = Recipe::current().tail_floor_gain();
        if peak <= floor {
            return Err(silent());
        }
        let threshold = peak
            * Decibels::clamped(-ONSET_RANGE_DB.get())
                .to_amplitude_gain()
                .unwrap_or(0.0);
        let onset = (0..frames)
            .find(|frame| magnitude(*frame) >= threshold)
            .ok_or_else(silent)?;
        let end = (onset..frames)
            .rev()
            .find(|frame| magnitude(*frame) > floor)
            .ok_or_else(silent)?
            + 1;
        let count = cast::usize_to_f64(channels.len());
        let mut energy = Vec::with_capacity(end - onset + 1);
        energy.push(0.0);
        let mut sum = 0.0;
        for frame in onset..end {
            sum += channels.iter().map(|c| c[frame] * c[frame]).sum::<f64>() / count;
            energy.push(sum);
        }
        let mean_square =
            |from: usize, to: usize| (energy[to] - energy[from]) / cast::usize_to_f64(to - from);
        let length = end - onset;
        let step = usize::try_from(audio.sample_rate().get() / ENVELOPE_FRAMES_PER_S)
            .unwrap_or(1)
            .max(1);
        let envelope_db = (0..length)
            .step_by(step)
            .map(|from| power_db(mean_square(from, (from + step).min(length))))
            .collect();
        Ok(Self {
            sample_rate: audio.sample_rate(),
            rms_db: power_db(mean_square(0, length)),
            peak_db: 20.0 * peak.log10(),
            duration_s: cast::usize_to_f64(length) / f64::from(audio.sample_rate().get()),
            envelope_db,
            energy,
        })
    }

    /// The RMS level of the first `frames` frames after the onset, in dBFS.
    fn rms_over(&self, frames: usize) -> f64 {
        power_db(self.energy[frames] / cast::usize_to_f64(frames))
    }

    fn frames(&self) -> usize {
        self.energy.len() - 1
    }
}

/// An output of a clip that a spread covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipOutput {
    /// The RMS level. Two clips compare over their common span, the
    /// shorter clip's length from the onset, so a cut tail does not change
    /// the level.
    RmsDb,
    /// [`ClipMetrics::peak_db`].
    PeakDb,
    /// The peak over the RMS level, in dB: the crest factor. Two clips
    /// compare with the RMS level over their common span, as
    /// [`ClipOutput::RmsDb`] does. A gain leaves it unchanged.
    PeakToRmsDb,
    /// [`ClipMetrics::duration_s`].
    DurationS,
    /// The envelope distance, [`envelope_distance`].
    EnvelopeDistanceDb,
}

impl ClipOutput {
    /// Every clip output.
    pub const ALL: [Self; 5] = [
        Self::RmsDb,
        Self::PeakDb,
        Self::PeakToRmsDb,
        Self::DurationS,
        Self::EnvelopeDistanceDb,
    ];

    /// The outputs of the sound's shape, which [`compare_fitted`] compares
    /// after a gain fit: the peak-to-RMS ratio, the duration to the tail
    /// floor and the envelope distance. The RMS level and the peak follow
    /// the level, so the fit takes them out.
    pub const SHAPE: [Self; 3] = [Self::PeakToRmsDb, Self::DurationS, Self::EnvelopeDistanceDb];

    /// The name of the output in a spread file.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::RmsDb => "rms_db",
            Self::PeakDb => "peak_db",
            Self::PeakToRmsDb => "peak_to_rms_db",
            Self::DurationS => "duration_s",
            Self::EnvelopeDistanceDb => "envelope_distance_db",
        }
    }

    /// The unit of the output.
    #[must_use]
    pub fn unit(self) -> &'static str {
        match self {
            Self::DurationS => "s",
            Self::RmsDb | Self::PeakDb | Self::PeakToRmsDb | Self::EnvelopeDistanceDb => "dB",
        }
    }

    /// The value of the output for one clip; `None` for the envelope
    /// distance, which only two clips have. The RMS level is the one from
    /// onset to end, here and in the peak-to-RMS ratio.
    #[must_use]
    pub fn value(self, clip: &ClipMetrics) -> Option<f64> {
        match self {
            Self::RmsDb => Some(clip.rms_db),
            Self::PeakDb => Some(clip.peak_db),
            Self::PeakToRmsDb => Some(clip.peak_db - clip.rms_db),
            Self::DurationS => Some(clip.duration_s),
            Self::EnvelopeDistanceDb => None,
        }
    }

    /// The distance of two clips on the output.
    #[must_use]
    pub fn distance(self, a: &ClipMetrics, b: &ClipMetrics) -> f64 {
        match self {
            Self::RmsDb => {
                let frames = a.frames().min(b.frames());
                (a.rms_over(frames) - b.rms_over(frames)).abs()
            }
            Self::PeakDb => (a.peak_db - b.peak_db).abs(),
            Self::PeakToRmsDb => {
                let frames = a.frames().min(b.frames());
                ((a.peak_db - a.rms_over(frames)) - (b.peak_db - b.rms_over(frames))).abs()
            }
            Self::DurationS => (a.duration_s - b.duration_s).abs(),
            Self::EnvelopeDistanceDb => envelope_distance(a, b),
        }
    }

    /// The floor of the tolerance: [`LEVEL_FLOOR_DB`] for a level and one
    /// sample for the duration.
    #[must_use]
    pub fn floor(self, sample_rate: SampleRate) -> f64 {
        match self {
            Self::DurationS => 1.0 / f64::from(sample_rate.get()),
            Self::RmsDb | Self::PeakDb | Self::PeakToRmsDb | Self::EnvelopeDistanceDb => {
                LEVEL_FLOOR_DB
            }
        }
    }
}

/// The envelope distance of two clips, in dB.
///
/// It is the mean absolute level difference over the frames where either
/// envelope lies within [`ENVELOPE_RANGE_DB`] of the loudest frame of both.
/// A frame below that range, or past the end of the shorter clip, counts
/// at the bottom of the range, so a longer tail adds to the distance. The
/// frames align at the two onsets.
#[must_use]
pub fn envelope_distance(a: &ClipMetrics, b: &ClipMetrics) -> f64 {
    let loudest = a
        .envelope_db
        .iter()
        .chain(&b.envelope_db)
        .fold(SILENT_DB, |x, y| x.max(*y));
    let bottom = loudest - ENVELOPE_RANGE_DB.get();
    let level =
        |envelope: &[f64], frame: usize| envelope.get(frame).copied().unwrap_or(bottom).max(bottom);
    let frames = a.envelope_db.len().max(b.envelope_db.len());
    let (sum, count) = (0..frames)
        .map(|frame| (level(&a.envelope_db, frame), level(&b.envelope_db, frame)))
        .filter(|(x, y)| x.max(*y) > bottom)
        .fold((0.0, 0_usize), |(sum, count), (x, y)| {
            (sum + (x - y).abs(), count + 1)
        });
    if count == 0 {
        0.0
    } else {
        sum / cast::usize_to_f64(count)
    }
}

/// The rank, counted from 1 in ascending order, of the leave-one-out
/// median that is the tolerance of `count` renders: `⌈0.95 (count + 1)⌉`,
/// at most `count`.
///
/// A clip of the same sound and the renders are exchangeable, so the
/// clip's median lies at or below the median at this rank with a
/// probability of the rank over `count + 1`: 20/21 for 20 renders. With
/// fewer than 19 renders the rank stops at `count`, and the coverage falls
/// below 95 %.
#[must_use]
pub fn tolerance_rank(count: usize) -> usize {
    (COVERAGE_PERCENT * (count + 1)).div_ceil(100).min(count)
}

/// The spread of one output.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SpreadRow {
    /// The output: a [`ClipOutput`] name, or the path of a number in the
    /// layer report, such as `stem.rms_dbfs`.
    pub output: String,
    /// The unit of the output, of its spread and of its tolerance.
    pub unit: String,
    /// The [`SPREAD_QUANTILE`] of the distances over every pair of
    /// renders: how far two renders lie apart.
    pub spread: f64,
    /// The leave-one-out median at rank [`tolerance_rank`]: for each
    /// render, the median of its distances from the other renders. A clip
    /// compares with the renders against it.
    pub tolerance: f64,
    /// The least tolerance a comparison uses, so float noise on an output
    /// that every render repeats does not count as a difference. Always
    /// positive.
    pub floor: f64,
    /// How many renders lie within the limit of the other renders, each
    /// held out in turn and compared as a clip: a check of the coverage
    /// that [`tolerance_rank`] states.
    pub held_out_within: usize,
    /// A diagnostic of the draws: the median distance of the renders next
    /// to each other in the sequence over the median distance of all
    /// pairs. Near 1 when a draw does not depend on the one before; absent
    /// when every pair is 0. With 19 adjacent pairs it is noisy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adjacent_ratio: Option<f64>,
    /// A diagnostic of the draws: the lag-1 autocorrelation of the output's
    /// value over the sequence, [`lag1_autocorrelation`]. Absent for an
    /// output of two clips and for a value that every render repeats.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lag1_autocorrelation: Option<f64>,
}

impl SpreadRow {
    /// The spread row of `count` renders whose pairwise distances
    /// `distance` gives, with the output's `values` in draw order when it
    /// has one per render.
    ///
    /// Returns `None` for fewer than [`MIN_RUNS`] renders, for a `floor`
    /// that is not positive and finite, which would let a difference over
    /// a limit of 0 be no number, and for a distance that is not finite.
    #[must_use]
    pub fn of(
        output: &str,
        unit: &str,
        count: usize,
        distance: impl Fn(usize, usize) -> f64,
        values: Option<&[f64]>,
        floor: f64,
    ) -> Option<Self> {
        if count < MIN_RUNS || !(floor.is_finite() && floor > 0.0) {
            return None;
        }
        let mut matrix = vec![0.0; count * count];
        for i in 0..count {
            for j in i + 1..count {
                let d = distance(i, j);
                matrix[i * count + j] = d;
                matrix[j * count + i] = d;
            }
        }
        let at = |i: usize, j: usize| matrix[i * count + j];
        let all: Vec<usize> = (0..count).collect();
        let pairs: Vec<f64> = (0..count)
            .flat_map(|i| (i + 1..count).map(move |j| (i, j)))
            .map(|(i, j)| at(i, j))
            .collect();
        let tolerance = tolerance_of(&all, at)?;
        let mut held_out_within = 0;
        for held in 0..count {
            let rest: Vec<usize> = (0..count).filter(|i| *i != held).collect();
            let difference = median(&rest.iter().map(|j| at(held, *j)).collect::<Vec<_>>())?;
            if difference <= tolerance_of(&rest, at)?.max(floor) {
                held_out_within += 1;
            }
        }
        let adjacent = median(&(1..count).map(|i| at(i - 1, i)).collect::<Vec<_>>())?;
        let typical = median(&pairs)?;
        Some(Self {
            output: output.to_owned(),
            unit: unit.to_owned(),
            spread: sorted_quantile(pairs, SPREAD_QUANTILE)?,
            tolerance,
            floor,
            held_out_within,
            adjacent_ratio: (typical > 0.0).then(|| adjacent / typical),
            lag1_autocorrelation: values.and_then(lag1_autocorrelation),
        })
    }

    /// The spread row of a scalar output from its value in each render, in
    /// draw order. Returns `None` as [`SpreadRow::of`] does.
    #[must_use]
    pub fn of_values(output: &str, unit: &str, values: &[f64], floor: f64) -> Option<Self> {
        Self::of(
            output,
            unit,
            values.len(),
            |i, j| (values[i] - values[j]).abs(),
            Some(values),
            floor,
        )
    }

    /// The tolerance a comparison uses: the larger of
    /// [`SpreadRow::tolerance`] and [`SpreadRow::floor`].
    #[must_use]
    pub fn limit(&self) -> f64 {
        self.tolerance.max(self.floor)
    }
}

/// The tolerance of the renders `of`: the leave-one-out median at rank
/// [`tolerance_rank`], each median over the distances `at` gives from one
/// render to the others.
fn tolerance_of(of: &[usize], at: impl Fn(usize, usize) -> f64) -> Option<f64> {
    let mut medians = of
        .iter()
        .map(|i| {
            median(
                &of.iter()
                    .filter(|j| *j != i)
                    .map(|j| at(*i, *j))
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Option<Vec<_>>>()?;
    medians.sort_by(f64::total_cmp);
    medians
        .get(tolerance_rank(of.len()).checked_sub(1)?)
        .copied()
}

/// The lag-1 autocorrelation of `values` in draw order: the sum of
/// products of each deviation from the mean with the next one, over the
/// sum of squared deviations.
///
/// A diagnostic: for independent draws it lies near `-1 / n`, within about
/// `2 / √n`, so ±0.45 for 20 renders. A drift over the sequence takes it
/// towards 1. Returns `None` for fewer than two values or values that do
/// not vary.
#[must_use]
pub fn lag1_autocorrelation(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let mean = values.iter().sum::<f64>() / cast::usize_to_f64(values.len());
    let deviation: Vec<f64> = values.iter().map(|value| value - mean).collect();
    let variance: f64 = deviation.iter().map(|d| d * d).sum();
    let lagged: f64 = deviation.windows(2).map(|pair| pair[0] * pair[1]).sum();
    (variance > 0.0 && variance.is_finite()).then(|| lagged / variance)
}

/// The spread rows of the clip outputs, one per [`ClipOutput`].
///
/// # Errors
///
/// Returns [`SpreadError::Runs`] for fewer than [`MIN_RUNS`] clips and
/// [`SpreadError::SampleRate`] when the clips' sample rates differ.
pub fn clip_rows(clips: &[ClipMetrics]) -> Result<Vec<SpreadRow>, SpreadError> {
    let Some(rate) = shared_rate(clips)?.filter(|_| clips.len() >= MIN_RUNS) else {
        return Err(SpreadError::Runs(clips.len()));
    };
    ClipOutput::ALL
        .into_iter()
        .map(|output| {
            let values: Option<Vec<f64>> = clips.iter().map(|clip| output.value(clip)).collect();
            SpreadRow::of(
                output.name(),
                output.unit(),
                clips.len(),
                |i, j| output.distance(&clips[i], &clips[j]),
                values.as_deref(),
                output.floor(rate),
            )
            .ok_or(SpreadError::MissingRow(output.name()))
        })
        .collect()
}

fn shared_rate(clips: &[ClipMetrics]) -> Result<Option<SampleRate>, SpreadError> {
    let Some(first) = clips.first() else {
        return Ok(None);
    };
    match clips
        .iter()
        .find(|clip| clip.sample_rate != first.sample_rate)
    {
        Some(other) => Err(SpreadError::SampleRate {
            clip: other.sample_rate,
            renders: first.sample_rate,
        }),
        None => Ok(Some(first.sample_rate)),
    }
}

/// The spread rows of the numbers of layer reports, one report per render.
///
/// A number that some report lacks, or holds as `null`, gets no row: it has
/// no value to compare in that render. The floor of a level in dB is
/// [`LEVEL_FLOOR_DB`]; any other unit takes the float32 resolution of the
/// largest value, as the reports carry estimates made in float32 audio,
/// and at least the smallest normal float32, for a value that every render
/// holds at 0.
///
/// # Errors
///
/// Returns [`SpreadError::UnknownUnit`] for a field with no known unit.
pub fn layer_rows(reports: &[Value]) -> Result<Vec<SpreadRow>, SpreadError> {
    let mut paths: BTreeMap<String, (String, Vec<f64>)> = BTreeMap::new();
    for report in reports {
        for (path, unit, value) in layer_values(report)? {
            paths
                .entry(path)
                .or_insert_with(|| (unit, Vec::new()))
                .1
                .push(value);
        }
    }
    Ok(paths
        .iter()
        .filter(|(_, (_, values))| values.len() == reports.len())
        .filter_map(|(path, (unit, values))| {
            let floor = if unit == "dB" {
                LEVEL_FLOOR_DB
            } else {
                (values.iter().fold(0.0, |max: f64, v| max.max(v.abs())) * f64::from(f32::EPSILON))
                    .max(f64::from(f32::MIN_POSITIVE))
            };
            SpreadRow::of_values(path, unit, values, floor)
        })
        .collect())
}

/// The numbers of a layer report that a spread covers, with their units:
/// each estimate and each median under `stem` and the groups later
/// releases add.
///
/// The path drops the `value` or `median` key, such as `stem.rms_dbfs` or
/// `stem.band_share[1].share`. Confidences, IQRs, band edges and the
/// inputs (`bpm`, `bpm_given`, `bpm_refined` and `sample_rate_hz`) are left
/// out, and so is a `null`. The unit comes from the field name's suffix
/// (`_hz`, `_s`, `_db`, `_dbfs`, `_deg`). A ratio has a name of
/// [`UNITLESS`].
///
/// # Errors
///
/// Returns [`SpreadError::UnknownUnit`] for a field that has neither.
pub fn layer_values(report: &Value) -> Result<Vec<(String, String, f64)>, SpreadError> {
    let mut values = Vec::new();
    if let Some(root) = report.as_object() {
        for (key, group) in root {
            if !NON_LAYERS.contains(&key.as_str()) {
                walk(key, key, group, &mut values)?;
            }
        }
    }
    Ok(values)
}

/// The fields of a layer report that hold a ratio and have no unit suffix.
pub const UNITLESS: [&str; 4] = ["centroid_over_f0", "flatness", "rho", "share"];

/// The top-level keys of a layer report that hold no estimates.
const NON_LAYERS: [&str; 5] = ["schema", "source", "synth", "transcription", "conventions"];
/// Keys that hold no estimate, or an input of the run.
const SKIPPED: [&str; 8] = [
    "confidence",
    "iqr",
    "lo_hz",
    "hi_hz",
    "bpm",
    "bpm_given",
    "bpm_refined",
    "sample_rate_hz",
];

fn walk(
    path: &str,
    name: &str,
    value: &Value,
    out: &mut Vec<(String, String, f64)>,
) -> Result<(), SpreadError> {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                if SKIPPED.contains(&key.as_str()) {
                    continue;
                }
                if key == "value" || key == "median" {
                    walk(path, name, inner, out)?;
                } else {
                    walk(&format!("{path}.{key}"), key, inner, out)?;
                }
            }
        }
        Value::Array(items) => {
            for (index, inner) in items.iter().enumerate() {
                walk(&format!("{path}[{index}]"), name, inner, out)?;
            }
        }
        Value::Number(number) => {
            if let Some(number) = number.as_f64() {
                let unit =
                    unit_of(name).ok_or_else(|| SpreadError::UnknownUnit(path.to_owned()))?;
                out.push((path.to_owned(), unit.to_owned(), number));
            }
        }
        _ => {}
    }
    Ok(())
}

fn unit_of(name: &str) -> Option<&'static str> {
    if UNITLESS.contains(&name) {
        return Some("ratio");
    }
    [
        ("_dbfs", "dB"),
        ("_db", "dB"),
        ("_hz", "Hz"),
        ("_deg", "deg"),
        ("_s", "s"),
    ]
    .iter()
    .find(|(suffix, _)| name.ends_with(suffix))
    .map(|(_, unit)| *unit)
}

/// The spread of one preset on one synth build and quality setting.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SpreadTable {
    /// What rendered: the synth, its build, the host and the architecture.
    pub identity: HostIdentity,
    /// The recipe version.
    pub recipe_version: u32,
    /// The quality setting: the oversampling of every render.
    pub oversampling: Oversampling,
    /// How many renders, each a distinct draw.
    pub n: usize,
    /// The spread statistic, [`STATISTIC`].
    pub statistic: String,
    /// The comparison tolerance, [`TOLERANCE`].
    pub tolerance: String,
    /// The share of clips of the same sound that the tolerance covers: the
    /// [`tolerance_rank`] of `n` over `n + 1`.
    pub coverage: f64,
    /// The SHA-256 of the preset file.
    pub preset_sha256: String,
    /// The SHA-256 of the MIDI file the notes came from, when they came
    /// from one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub midi_sha256: Option<String>,
    /// The tempo.
    pub bpm: Bpm,
    /// The spread of each output.
    pub outputs: Vec<SpreadRow>,
}

impl SpreadTable {
    /// The table of `renders`, the draws of one preset, with `outputs`.
    ///
    /// # Errors
    ///
    /// Returns [`SpreadError::Render`] when no render came back.
    pub fn new(
        renders: &[Rendered],
        preset_sha256: String,
        bpm: Bpm,
        outputs: Vec<SpreadRow>,
    ) -> Result<Self, SpreadError> {
        let first = renders
            .first()
            .ok_or_else(|| SpreadError::Render("no render came back".to_owned()))?;
        Ok(Self {
            identity: first.record.identity.clone(),
            recipe_version: first.record.recipe_version,
            oversampling: first.record.oversampling,
            n: renders.len(),
            statistic: STATISTIC.to_owned(),
            tolerance: TOLERANCE.to_owned(),
            coverage: cast::usize_to_f64(tolerance_rank(renders.len()))
                / cast::usize_to_f64(renders.len() + 1),
            preset_sha256,
            midi_sha256: first.record.midi_sha256.clone(),
            bpm,
            outputs,
        })
    }
}

/// How a clip compares with renders of the same preset on one output.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Comparison {
    /// The output.
    pub output: String,
    /// The unit.
    pub unit: String,
    /// The clip's value; absent for a distance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clip: Option<f64>,
    /// The median of the renders' values; absent for a distance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub renders: Option<f64>,
    /// The median, over the renders, of the distance between the clip and
    /// one render.
    pub difference: f64,
    /// The tolerance: the larger of the row's tolerance and floor, so
    /// always positive.
    pub limit: f64,
    /// The difference over the limit: at most 1 for a match.
    pub ratio: f64,
    /// Whether the difference lies within the limit.
    pub within: bool,
}

/// Compares `clip` with each of `renders` on every clip output.
///
/// The difference is the median over the renders of the clip's distance
/// from one render. A clip of the same sound lies within the row's limit
/// with the coverage of [`tolerance_rank`].
///
/// # Errors
///
/// Returns [`SpreadError::SampleRate`] when the clip and the renders have
/// different sample rates, [`SpreadError::Runs`] for no render and
/// [`SpreadError::MissingRow`] for a clip output that `rows` lacks.
pub fn compare(
    clip: &ClipMetrics,
    renders: &[ClipMetrics],
    rows: &[SpreadRow],
) -> Result<Vec<Comparison>, SpreadError> {
    compare_on(&ClipOutput::ALL, clip, renders, rows)
}

/// A fitted gain beyond this many dB, either way, is a finding to explain:
/// within it a level difference is no fault, since a comparison of two
/// sounds matches their loudness first.
pub const GAIN_RANGE_DB: f64 = 3.0;

/// How a recording compares with renders of the same preset once a gain
/// matches its level to theirs.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FittedComparison {
    /// The gain applied to the recording, in dB: [`fit_gain`].
    pub gain_db: f64,
    /// Whether the gain lies beyond [`GAIN_RANGE_DB`]: a finding, not a
    /// failure.
    pub gain_beyond_range: bool,
    /// The comparisons of the recording after the gain on the
    /// [`ClipOutput::SHAPE`] outputs.
    pub comparisons: Vec<Comparison>,
    /// Whether every comparison lies within its limit.
    pub within: bool,
}

/// The gain in dB that matches the level of `clip` to `renders`.
///
/// It is the median over the renders of their RMS level minus the clip's,
/// each pair over its common span, as [`ClipOutput::RmsDb`] compares them.
/// Returns `None` for no render.
#[must_use]
pub fn fit_gain(clip: &ClipMetrics, renders: &[ClipMetrics]) -> Option<f64> {
    median(
        &renders
            .iter()
            .map(|render| {
                let frames = clip.frames().min(render.frames());
                render.rms_over(frames) - clip.rms_over(frames)
            })
            .collect::<Vec<_>>(),
    )
}

/// Fits the gain of `recording` to `renders`, applies it and compares the
/// result on the [`ClipOutput::SHAPE`] outputs.
///
/// The gain moves the recording's end too, since the end is the last
/// sample above a fixed tail floor, so the duration compares after it.
///
/// # Errors
///
/// Returns [`SpreadError::Silent`], naming `what`, for a silent
/// recording, [`SpreadError::Audio`] for a recording the gain cannot
/// scale, and the errors of [`compare`].
pub fn compare_fitted(
    recording: &Audio,
    what: &str,
    renders: &[ClipMetrics],
    rows: &[SpreadRow],
) -> Result<FittedComparison, SpreadError> {
    let clip = ClipMetrics::of(recording, what)?;
    check_rate(&clip, renders)?;
    let gain_db = fit_gain(&clip, renders).ok_or(SpreadError::Runs(renders.len()))?;
    let amplitude = Decibels::clamped(gain_db)
        .to_amplitude_gain()
        .unwrap_or(1.0);
    let scaled = Audio::new(
        recording.sample_rate(),
        recording
            .channels()
            .iter()
            .map(|channel| channel.iter().map(|x| x * amplitude).collect())
            .collect(),
    )
    .map_err(SpreadError::Audio)?;
    let fitted = ClipMetrics::of(&scaled, what)?;
    let comparisons = compare_on(&ClipOutput::SHAPE, &fitted, renders, rows)?;
    Ok(FittedComparison {
        gain_db,
        gain_beyond_range: gain_db.abs() > GAIN_RANGE_DB,
        within: comparisons.iter().all(|comparison| comparison.within),
        comparisons,
    })
}

fn check_rate(clip: &ClipMetrics, renders: &[ClipMetrics]) -> Result<(), SpreadError> {
    match shared_rate(renders)? {
        Some(rate) if rate != clip.sample_rate => Err(SpreadError::SampleRate {
            clip: clip.sample_rate,
            renders: rate,
        }),
        _ => Ok(()),
    }
}

fn compare_on(
    outputs: &[ClipOutput],
    clip: &ClipMetrics,
    renders: &[ClipMetrics],
    rows: &[SpreadRow],
) -> Result<Vec<Comparison>, SpreadError> {
    check_rate(clip, renders)?;
    outputs
        .iter()
        .map(|&output| {
            let row = rows
                .iter()
                .find(|row| row.output == output.name())
                .ok_or(SpreadError::MissingRow(output.name()))?;
            let difference = median(
                &renders
                    .iter()
                    .map(|render| output.distance(clip, render))
                    .collect::<Vec<_>>(),
            )
            .ok_or(SpreadError::Runs(renders.len()))?;
            let limit = row.limit();
            Ok(Comparison {
                output: row.output.clone(),
                unit: row.unit.clone(),
                clip: output.value(clip),
                renders: renders
                    .iter()
                    .map(|render| output.value(render))
                    .collect::<Option<Vec<_>>>()
                    .and_then(|values| median(&values)),
                difference,
                limit,
                ratio: difference / limit,
                within: difference <= limit,
            })
        })
        .collect()
}

fn median(values: &[f64]) -> Option<f64> {
    Summary::of(values).map(Summary::median)
}

fn sorted_quantile(mut values: Vec<f64>, q: f64) -> Option<f64> {
    values.sort_by(f64::total_cmp);
    quantile(&values, q)
}

fn power_db(power: f64) -> f64 {
    Decibels::from_power_ratio(power).map_or(SILENT_DB, Decibels::get)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(gain: f64, seconds: f64, lead: usize) -> Audio {
        let rate = 44_100.0;
        let frames = cast::round_to_usize(seconds * rate).unwrap();
        let samples: Vec<f64> = (0..lead)
            .map(|_| 0.0)
            .chain((0..frames).map(|n| {
                gain * (std::f64::consts::TAU * 440.0 * cast::usize_to_f64(n) / rate).sin()
            }))
            .collect();
        Audio::new(
            SampleRate::new(44_100).unwrap(),
            vec![samples.clone(), samples],
        )
        .unwrap()
    }

    fn metrics(gain: f64, seconds: f64, lead: usize) -> ClipMetrics {
        ClipMetrics::of(&tone(gain, seconds, lead), "the tone").unwrap()
    }

    #[test]
    fn a_sine_has_its_level_peak_and_length() {
        let metrics = metrics(0.5, 1.0, 4410);
        // A sine of amplitude 0.5 has an RMS of 0.5 / √2: -9.03 dBFS.
        assert!((metrics.rms_db + 9.03).abs() < 0.01, "{}", metrics.rms_db);
        assert!((metrics.peak_db + 6.02).abs() < 0.01, "{}", metrics.peak_db);
        // The lead-in of 0.1 s does not count.
        assert!(
            (metrics.duration_s - 1.0).abs() < 0.001,
            "{}",
            metrics.duration_s
        );
        assert_eq!(metrics.envelope_db.len(), 100);
        let silent = ClipMetrics::of(&tone(0.0, 1.0, 0), "the clip").unwrap_err();
        assert!(
            silent.to_string().starts_with("the clip is silent"),
            "{silent}"
        );
    }

    #[test]
    fn the_envelope_distance_aligns_onsets_and_counts_a_longer_tail() {
        let a = metrics(0.5, 1.0, 0);
        let late = metrics(0.5, 1.0, 22_050);
        assert!(envelope_distance(&a, &late) < 1e-6);
        let quieter = metrics(0.25, 1.0, 0);
        assert!((envelope_distance(&a, &quieter) - 6.02).abs() < 0.01);
        let longer = metrics(0.5, 2.0, 0);
        // Half the counted frames lie past the shorter clip, 40 dB apart.
        assert!((envelope_distance(&a, &longer) - 20.0).abs() < 0.5);
    }

    #[test]
    fn the_rms_level_compares_over_the_common_span() {
        // A clip whose second half is 20 dB quieter has a lower RMS from
        // onset to end, but over the first half it matches a clip cut there.
        let rate = 44_100;
        let full: Vec<f64> = (0..rate)
            .map(|n| {
                let gain = if n < rate / 2 { 0.5 } else { 0.05 };
                gain * (std::f64::consts::TAU * 440.0 * f64::from(n) / f64::from(rate)).sin()
            })
            .collect();
        let rate = SampleRate::new(44_100).unwrap();
        let long = ClipMetrics::of(&Audio::new(rate, vec![full.clone()]).unwrap(), "a").unwrap();
        let cut = ClipMetrics::of(
            &Audio::new(rate, vec![full[..22_050].to_vec()]).unwrap(),
            "b",
        )
        .unwrap();
        assert!(cut.rms_db - long.rms_db > 2.0);
        assert!(ClipOutput::RmsDb.distance(&long, &cut) < 1e-9);
    }

    #[test]
    fn a_layer_report_gives_its_estimates_with_units() {
        let report = serde_json::json!({
            "schema": "layers/1",
            "conventions": { "frame_size": 2048 },
            "stem": {
                "bpm": 120.0,
                "duration_s": 4.5,
                "rms_dbfs": { "value": -16.6, "confidence": { "spread": 1.0 } },
                "centroid_hz": { "median": 238.2, "iqr": 115.8 },
                "band_share": { "value": [{ "lo_hz": 20.0, "hi_hz": 60.0, "share": 0.1 }] },
                "stereo": { "full": { "rho": 0.49, "ipd_deg": 67.1 } },
                "crest_db": { "value": null }
            }
        });
        let values = layer_values(&report).unwrap();
        let named: Vec<(&str, &str)> = values
            .iter()
            .map(|(path, unit, _)| (path.as_str(), unit.as_str()))
            .collect();
        assert_eq!(
            named,
            [
                ("stem.duration_s", "s"),
                ("stem.rms_dbfs", "dB"),
                ("stem.centroid_hz", "Hz"),
                ("stem.band_share[0].share", "ratio"),
                ("stem.stereo.full.rho", "ratio"),
                ("stem.stereo.full.ipd_deg", "deg"),
            ]
        );
    }

    #[test]
    fn a_field_with_no_known_unit_is_refused() {
        let report = serde_json::json!({ "stem": { "notes": { "value": 3 } } });
        let error = layer_values(&report).unwrap_err();
        assert!(
            error.to_string().contains("stem.notes has no known unit"),
            "{error}"
        );
    }

    #[test]
    fn a_layer_number_that_one_report_lacks_gets_no_row() {
        let report = |rms: f64, centroid: Option<f64>| {
            serde_json::json!({
                "stem": { "rms_dbfs": { "value": rms }, "centroid_hz": { "median": centroid } }
            })
        };
        let reports = [
            report(-16.0, Some(200.0)),
            report(-16.2, None),
            report(-16.1, Some(210.0)),
        ];
        let rows = layer_rows(&reports).unwrap();
        let names: Vec<&str> = rows.iter().map(|row| row.output.as_str()).collect();
        assert_eq!(names, ["stem.rms_dbfs"]);
        assert_eq!(rows[0].floor, LEVEL_FLOOR_DB);
        assert!((rows[0].spread - 0.19).abs() < 1e-9, "{:?}", rows[0]);
    }

    #[test]
    fn the_spread_is_the_95th_percentile_of_the_pairs() {
        let values: Vec<f64> = (0..20).map(f64::from).collect();
        // 190 pairs; |i - j| = d occurs 20 - d times.
        let mut pairs: Vec<f64> = Vec::new();
        for i in 0..20 {
            for j in i + 1..20 {
                pairs.push(f64::from(j - i));
            }
        }
        assert_eq!(pairs.len(), 190);
        pairs.sort_by(f64::total_cmp);
        let expected = quantile(&pairs, 0.95).unwrap();
        let row = SpreadRow::of_values("x", "dB", &values, 1e-9).unwrap();
        assert!((row.spread - expected).abs() < 1e-12);
        assert!(SpreadRow::of_values("x", "dB", &values[..2], 1e-9).is_none());
    }

    #[test]
    fn the_tolerance_rank_covers_95_percent_of_n_plus_one() {
        // ⌈0.95 (n + 1)⌉: the largest of 19 or 20 medians, the 39th of 40.
        assert_eq!(tolerance_rank(20), 20);
        assert_eq!(tolerance_rank(19), 19);
        assert_eq!(tolerance_rank(40), 39);
        assert_eq!(tolerance_rank(100), 96);
        // Below 19 renders the rank stops at n.
        assert_eq!(tolerance_rank(3), 3);
    }

    #[test]
    fn the_tolerance_is_the_leave_one_out_median_at_the_rank() {
        // Values 0..5: the medians of each value's distances to the others
        // are 2.5, 1.5, 1.5, 1.5, 2.5, and the rank of 5 is 5.
        let values: Vec<f64> = (0..5).map(f64::from).collect();
        let row = SpreadRow::of_values("x", "dB", &values, 1e-9).unwrap();
        assert_eq!(row.tolerance, 2.5);
        // Held out, the end values lie 2.5 from the other four, whose
        // largest median is 2: only the middle three lie within.
        assert_eq!(row.held_out_within, 3);
        // Adjacent values lie 1 apart; the median of all ten pairs is 2.
        assert_eq!(row.adjacent_ratio, Some(0.5));
    }

    #[test]
    fn a_drift_over_the_draws_shows_in_both_diagnostics() {
        // The same 20 values, once rising in draw order and once
        // interleaved, so the spread and the tolerance stay alike.
        let rising: Vec<f64> = (0..20).map(f64::from).collect();
        let interleaved: Vec<f64> = (0..20)
            .map(|i| f64::from(if i % 2 == 0 { i / 2 } else { 19 - i / 2 }))
            .collect();
        let drift = SpreadRow::of_values("x", "dB", &rising, 1e-9).unwrap();
        let mixed = SpreadRow::of_values("x", "dB", &interleaved, 1e-9).unwrap();
        assert_eq!(
            (drift.spread, drift.tolerance),
            (mixed.spread, mixed.tolerance)
        );
        let drift_r = drift.lag1_autocorrelation.unwrap();
        let mixed_r = mixed.lag1_autocorrelation.unwrap();
        assert!(drift_r > 0.8, "{drift_r}");
        assert!(mixed_r < -0.8, "{mixed_r}");
        assert!(drift.adjacent_ratio.unwrap() < 0.2, "{drift:?}");
        assert!(mixed.adjacent_ratio.unwrap() > 1.5, "{mixed:?}");
        assert_eq!(lag1_autocorrelation(&[1.0; 5]), None);
    }

    #[test]
    fn a_floor_of_zero_gives_no_row() {
        assert!(SpreadRow::of_values("x", "ratio", &[0.0; 4], 0.0).is_none());
        let zero = || serde_json::json!({ "stem": { "flatness": { "value": 0.0 } } });
        let rows = layer_rows(&[zero(), zero(), zero()]).unwrap();
        assert!(rows[0].floor > 0.0, "{rows:?}");
        assert!(rows[0].limit() > 0.0, "{rows:?}");
    }

    #[test]
    fn too_few_clips_or_a_missing_row_fail_the_comparison() {
        let clips: Vec<ClipMetrics> = (0..2).map(|_| metrics(0.5, 0.2, 0)).collect();
        let error = clip_rows(&clips).unwrap_err();
        assert!(matches!(error, SpreadError::Runs(2)), "{error}");
        let clips: Vec<ClipMetrics> = (0..3).map(|_| metrics(0.5, 0.2, 0)).collect();
        let mut rows = clip_rows(&clips).unwrap();
        assert_eq!(rows.len(), ClipOutput::ALL.len());
        rows.remove(1);
        let error = compare(&clips[0], &clips, &rows).unwrap_err();
        assert!(
            matches!(error, SpreadError::MissingRow("peak_db")),
            "{error}"
        );
    }

    #[test]
    fn a_repeated_output_compares_against_the_floor() {
        let row = SpreadRow::of_values("x", "dB", &[1.0; 4], LEVEL_FLOOR_DB).unwrap();
        assert_eq!((row.spread, row.tolerance), (0.0, 0.0));
        assert_eq!(row.adjacent_ratio, None);
        assert_eq!(row.limit(), LEVEL_FLOOR_DB);
        let one_sample = ClipOutput::DurationS.floor(SampleRate::new(48_000).unwrap());
        assert!((one_sample - 1.0 / 48_000.0).abs() < 1e-15);
    }

    #[test]
    fn a_clip_at_another_sample_rate_does_not_compare() {
        let renders: Vec<ClipMetrics> = (0..3).map(|_| metrics(0.5, 0.2, 0)).collect();
        let rows = clip_rows(&renders).unwrap();
        let samples = vec![0.5; 4800];
        let other = ClipMetrics::of(
            &Audio::new(SampleRate::new(48_000).unwrap(), vec![samples]).unwrap(),
            "c",
        )
        .unwrap();
        let error = compare(&other, &renders, &rows).unwrap_err();
        assert!(matches!(error, SpreadError::SampleRate { .. }), "{error}");
    }
}
