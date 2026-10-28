//! The tempo of a stem, refined near the given BPM.
//!
//! A tempo taken from a file or folder name is close to the true tempo but
//! rarely exact, and an error of 1 % drifts the notes off the grid within
//! a few bars. [`Tempo::refine`] searches the tempos that the given value
//! lies within [`SEARCH_SHARE`] of, 2.91 % below to 3.09 % above it, in
//! steps of [`BPM_STEP`]. Each candidate scores the autocorrelation of the
//! onset strength summed at 1 to [`MAX_BEAT_LAGS`] beats. The beat offset
//! stays 0: only the period is refined. A window this narrow never reaches
//! half or double the tempo.

use realfft::RealFftPlanner;
use realfft::num_complex::Complex64;

use super::SampleRate;
use crate::cast;
use crate::confidence::{ConfidenceError, Estimate, Level, NoSpread, Reason, ReasonCode, Signal};
use crate::dsp::{Stft, StftParams, low_pass_zero_phase};
use crate::units::{Bpm, Hz};

/// How far the given tempo may lie from the true one: the search covers
/// every tempo `t` with `|given / t - 1| <= SEARCH_SHARE`.
pub const SEARCH_SHARE: f64 = 0.03;
/// The step of the tempo search, in BPM.
pub const BPM_STEP: f64 = 0.01;
/// The most beat lags the score sums.
pub const MAX_BEAT_LAGS: usize = 32;
/// The fewest beat lags that refine a tempo; a shorter stem keeps the
/// given tempo.
pub const MIN_BEAT_LAGS: usize = 4;
/// Beats of the stem that every lag keeps in its overlap, so the longest
/// lag still averages several beats.
pub const OVERLAP_BEATS: usize = 4;
/// The lowest ratio of the best score to the median score over the search
/// that the refined tempo needs before it replaces the given one.
pub const PEAK_RATIO_MIN: f64 = 1.5;
/// The lowest share of the onset strength that repeats at the refined
/// beat.
///
/// The share is the best score over the lags times the zero-lag
/// autocorrelation. Onsets on every beat read near 1, one onset and a
/// steady tone near 0. Generated drum, bass and lead parts read 0.78 to
/// 0.99 and pad chords that change once a bar 0.04 at most; the threshold
/// sits between them.
pub const PERIODICITY_MIN: f64 = 0.4;
/// The lowest share of the onset strength that repeats every 2 or every 4
/// refined beats, for a part whose beat periodicity is too low.
///
/// One onset a bar repeats on a quarter of the beat lags, so its beat
/// periodicity reads near 0.25. Read on every second or fourth lag alone,
/// it reads near 1. Those lags are fewer and line up by chance more
/// often: a drum part whose `--bpm` is 3.5 % off reads 0.47 at a tempo
/// 16/15 of the truth, where every eighth beat meets an eighth note. A
/// tempo ramp of 1 % reads 0.62 at most, and generated parts with one kick
/// a bar or every other beat read 0.875 or more; the threshold sits
/// between them.
pub const SPARSE_PERIODICITY_MIN: f64 = 0.75;
/// The beat spacings of the sparse periodicity.
const SPARSE_BEATS: [usize; 2] = [2, 4];
/// The convention of the refined tempo in `layers.json`.
pub const TEMPO_CONVENTION: &str = "onset strength: the rise of each third-octave band power \
from 30 Hz, compressed as ln(1 + P / knee) with the knee 60 dB below full scale, against the \
frame 10 ms earlier, in 46.4 ms Hann frames on a 1 ms hop, low-passed at 20 Hz, less its \
100 ms mean, below 10 % of its largest rise set to 0; each tempo t with |given / t - 1| at \
most 3 % (2.91 % below to 3.09 % above the given one), in 0.01 BPM steps, scores the unbiased \
autocorrelation summed at 1 to 32 beats; the refined tempo is used when its score is at least \
1.5 times the median score (tempo_peak) and either 0.4 of the lag count times the zero-lag \
autocorrelation (beat_periodicity) or 0.75 of that on every second or fourth beat lag alone \
(sparse_periodicity); cycles_in_span counts the whole beats of the stem";

/// The hop of the onset strength.
const ONSET_HOP_S: f64 = 0.001;
/// The frame of the onset strength, rounded to the nearest power of two.
const ONSET_FRAME_S: f64 = 0.0464;
/// How far apart the two frames of the spectral flux lie.
const FLUX_LAG_S: f64 = 0.01;
/// The low-pass of the onset strength. It removes the ripple that a steady
/// tone leaves in the flux at its period, which reads as a tempo when the
/// period divides the beat (110 Hz is 55 periods per beat at 120 BPM).
const ONSET_LOW_PASS_HZ: f64 = 20.0;
/// The span of the local mean that the onset strength loses.
const ONSET_MEAN_S: f64 = 0.1;
/// The share of the largest rise that a rise of the onset strength needs
/// to count. A noise-free steady tone stays exactly periodic through every
/// earlier step, and its faint ripple repeats at any beat its period
/// divides; the floor removes it.
const ONSET_FLOOR_SHARE: f64 = 0.1;
/// The knee of the log compression, below a full-scale bin. Bins far
/// under it count linearly and barely at all, so the leakage between the
/// partials of a steady tone, which swings with the phase of each frame,
/// leaves no ripple that repeats with the tone's period.
const FLUX_KNEE_DB: f64 = -60.0;

/// The lowest band edge of the onset strength.
const BAND_LOW_HZ: f64 = 30.0;
/// The fewest bins in a band: wider than the main lobe of a Hann window,
/// four bins.
const BAND_MIN_BINS: usize = 6;

/// The tempo of a stem: the given one, the refined one and the one used.
#[derive(Clone, Debug, PartialEq)]
pub struct Tempo {
    given: Bpm,
    refined: Estimate<Bpm, NoSpread>,
    used: Bpm,
}

impl Tempo {
    /// Uses `given` as it is, as `--bpm-exact` asks.
    ///
    /// # Errors
    ///
    /// Never in practice: the reason list is fixed and finite.
    pub fn exact(given: Bpm) -> Result<Self, ConfidenceError> {
        Ok(Self {
            given,
            refined: Estimate::none(vec![Reason::new(ReasonCode::BpmExact, Signal::Flag(true))?])?,
            used: given,
        })
    }

    /// Refines `given` from the onsets of `mono`.
    ///
    /// The refined tempo replaces the given one when its score is at least
    /// [`PEAK_RATIO_MIN`] times the median of the search, and either at
    /// least [`PERIODICITY_MIN`] of the score that onsets on every beat
    /// give or at least [`SPARSE_PERIODICITY_MIN`] of it on every second
    /// or fourth beat lag. Otherwise it is reported at low confidence and
    /// the given tempo stays. A stem shorter than [`MIN_BEAT_LAGS`] plus
    /// [`OVERLAP_BEATS`] beats, or one without onsets, keeps the given
    /// tempo and reports no refined one.
    ///
    /// # Errors
    ///
    /// Never in practice: every signal value is finite.
    pub fn refine(mono: &[f64], rate: SampleRate, given: Bpm) -> Result<Self, ConfidenceError> {
        let keep = |reason: Reason| -> Result<Self, ConfidenceError> {
            Ok(Self {
                given,
                refined: Estimate::none(vec![reason])?,
                used: given,
            })
        };
        let hop = cast::round_to_usize(f64::from(rate.get()) * ONSET_HOP_S)
            .unwrap_or(1)
            .max(1);
        let fps = f64::from(rate.get()) / cast::usize_to_f64(hop);
        let candidates = candidates(given);
        let (Some(&slowest), Some(_)) = (candidates.first(), candidates.last()) else {
            return keep(Reason::new(ReasonCode::TempoPeak, Signal::Number(0.0))?);
        };
        let onsets = smooth(&onset_strength(mono, rate, hop), fps);
        let beats = cast::usize_to_f64(onsets.len()) / fps * slowest / 60.0;
        let whole_beats = cast::floor_to_usize(beats).unwrap_or(0);
        let lags = whole_beats.saturating_sub(OVERLAP_BEATS).min(MAX_BEAT_LAGS);
        if lags < MIN_BEAT_LAGS {
            return keep(Reason::new(
                ReasonCode::CyclesInSpan,
                Signal::Count(u64::try_from(whole_beats).unwrap_or(u64::MAX)),
            )?);
        }
        let longest = cast::usize_to_f64(lags) * 60.0 / slowest * fps;
        let acf = autocorrelation(&onsets, cast::ceil_to_usize(longest).unwrap_or(0) + 2);
        let scores: Vec<f64> = candidates
            .iter()
            .map(|&bpm| {
                let period = 60.0 / bpm * fps;
                (1..=lags)
                    .map(|k| interpolate(&acf, cast::usize_to_f64(k) * period))
                    .sum()
            })
            .collect();
        let Some((best_at, &best)) = scores.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1))
        else {
            return keep(Reason::new(ReasonCode::TempoPeak, Signal::Number(0.0))?);
        };
        let median = median(&scores);
        if !(best > 0.0 && median > 0.0) {
            return keep(Reason::new(ReasonCode::TempoPeak, Signal::Number(0.0))?);
        }
        let ratio = best / median;
        let periodicity = best / (cast::usize_to_f64(lags) * acf[0]);
        let period = 60.0 / candidates[best_at] * fps;
        let sparse = SPARSE_BEATS
            .iter()
            .filter(|&&every| lags / every >= MIN_BEAT_LAGS)
            .map(|&every| {
                let count = lags / every;
                let sum: f64 = (1..=count)
                    .map(|j| interpolate(&acf, cast::usize_to_f64(j * every) * period))
                    .sum();
                sum / (cast::usize_to_f64(count) * acf[0])
            })
            .fold(0.0, f64::max);
        let refined_bpm = Bpm::clamped(candidates[best_at]);
        let trusted = ratio >= PEAK_RATIO_MIN
            && (periodicity >= PERIODICITY_MIN || sparse >= SPARSE_PERIODICITY_MIN);
        let reasons = vec![
            Reason::new(ReasonCode::TempoPeak, Signal::Number(ratio))?,
            Reason::new(ReasonCode::BeatPeriodicity, Signal::Number(periodicity))?,
            Reason::new(ReasonCode::SparsePeriodicity, Signal::Number(sparse))?,
            Reason::new(
                ReasonCode::CyclesInSpan,
                Signal::Count(u64::try_from(whole_beats).unwrap_or(u64::MAX)),
            )?,
        ];
        Ok(Self {
            given,
            refined: Estimate::new(
                refined_bpm,
                if trusted { Level::High } else { Level::Low },
                None,
                reasons,
            )?,
            used: if trusted { refined_bpm } else { given },
        })
    }

    /// Returns the tempo given with `--bpm`.
    #[must_use]
    pub fn given(&self) -> Bpm {
        self.given
    }

    /// Returns the refined tempo with its confidence; no value under
    /// `--bpm-exact`, for a short stem or for one without onsets.
    #[must_use]
    pub fn refined(&self) -> &Estimate<Bpm, NoSpread> {
        &self.refined
    }

    /// Returns the tempo every later step uses.
    #[must_use]
    pub fn used(&self) -> Bpm {
        self.used
    }

    /// Returns the refined tempo when it is trusted, so it replaced the
    /// given one.
    #[must_use]
    pub fn trusted(&self) -> Option<Bpm> {
        (self.refined.confidence().level() == Some(Level::High))
            .then(|| self.refined.value().copied())
            .flatten()
    }

    /// Returns one line that says which tempo is used and why.
    #[must_use]
    pub fn summary(&self) -> String {
        let given = self.given.get();
        let reasons = self.refined.confidence().reasons();
        let signal = |code| {
            reasons
                .iter()
                .find(|reason| reason.code() == code)
                .map(|reason| reason.value())
        };
        match (self.refined.value(), signal(ReasonCode::TempoPeak)) {
            (Some(refined), Some(Signal::Number(ratio))) if self.trusted().is_some() => format!(
                "tempo: {} BPM, refined from --bpm {given} (peak ratio {ratio:.2})",
                refined.get()
            ),
            (Some(refined), Some(Signal::Number(ratio))) => {
                let share = |code| match signal(code) {
                    Some(Signal::Number(share)) => share,
                    _ => 0.0,
                };
                format!(
                    "tempo: {given} BPM as given; the refined {} BPM is not trusted (peak ratio {ratio:.2}, needs {PEAK_RATIO_MIN}; beat periodicity {:.2}, needs {PERIODICITY_MIN}, or {:.2} at 2 or 4 beats, needs {SPARSE_PERIODICITY_MIN})",
                    refined.get(),
                    share(ReasonCode::BeatPeriodicity),
                    share(ReasonCode::SparsePeriodicity),
                )
            }
            _ if signal(ReasonCode::BpmExact).is_some() => {
                format!("tempo: {given} BPM as given (--bpm-exact)")
            }
            _ => match signal(ReasonCode::CyclesInSpan) {
                Some(Signal::Count(0)) => {
                    format!("tempo: {given} BPM as given; the stem is shorter than one beat")
                }
                Some(Signal::Count(beats)) => format!(
                    "tempo: {given} BPM as given; the stem holds {beats} beat{}, too few to refine it",
                    if beats == 1 { "" } else { "s" }
                ),
                _ => format!("tempo: {given} BPM as given; the stem has no onsets to refine it"),
            },
        }
    }
}

/// The candidate tempos in BPM, slowest first: every step `t` with
/// `|given / t - 1| <= SEARCH_SHARE`, within the range of [`Bpm`].
fn candidates(given: Bpm) -> Vec<f64> {
    let low = (given.get() / (1.0 + SEARCH_SHARE)).max(4.0);
    let high = (given.get() / (1.0 - SEARCH_SHARE)).min(1000.0);
    // Dividing by the steps per BPM keeps 11962 steps at exactly 119.62.
    let per_bpm = (1.0 / BPM_STEP).round();
    let first = cast::ceil_to_usize(low * per_bpm - 1e-9).unwrap_or(0);
    let last = cast::floor_to_usize(high * per_bpm + 1e-9).unwrap_or(0);
    (first..=last)
        .map(|step| cast::usize_to_f64(step) / per_bpm)
        .collect()
}

/// The onset strength, one value per hop: the summed rise of every band
/// against the frame [`FLUX_LAG_S`] earlier, on the band power compressed
/// as `ln(1 + P / knee)` with the knee [`FLUX_KNEE_DB`] below a full-scale
/// bin.
fn onset_strength(mono: &[f64], rate: SampleRate, hop: usize) -> Vec<f64> {
    let wanted = f64::from(rate.get()) * ONSET_FRAME_S;
    let up = cast::ceil_to_usize(wanted)
        .unwrap_or(4)
        .max(4)
        .next_power_of_two();
    let frame_len = if cast::usize_to_f64(up) - wanted > wanted - cast::usize_to_f64(up / 2) {
        (up / 2).max(4)
    } else {
        up
    };
    let peak = mono.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
    let Ok(params) = StftParams::new(frame_len).and_then(|p| p.with_hop(hop)) else {
        return Vec::new();
    };
    let frames = params.frame_count(mono.len());
    if peak == 0.0 {
        return vec![0.0; frames];
    }
    // A full-scale sine reads peak * N / 4 in its bin under a Hann window.
    let full_scale = peak * cast::usize_to_f64(frame_len) / 4.0;
    let knee = full_scale * full_scale * 10_f64.powf(FLUX_KNEE_DB / 10.0);
    let bands = bands(params.bins(), params.bin_width(rate).get());
    let lag = cast::round_to_usize(FLUX_LAG_S * f64::from(rate.get()) / cast::usize_to_f64(hop))
        .unwrap_or(1)
        .max(1);
    let mut stft = Stft::new(params);
    let mut history: Vec<Vec<f64>> = Vec::with_capacity(lag + 1);
    let mut strength = Vec::with_capacity(frames);
    for frame in 0..frames {
        let spectrum = stft.frame(mono, frame);
        let levels: Vec<f64> = bands
            .iter()
            .map(|&(lo, hi)| {
                let power: f64 = spectrum[lo..hi].iter().map(Complex64::norm_sqr).sum();
                (power / knee).ln_1p()
            })
            .collect();
        let rise = history
            .first()
            .filter(|_| history.len() == lag)
            .map_or(0.0, |earlier| {
                levels
                    .iter()
                    .zip(earlier)
                    .map(|(now, then)| (now - then).max(0.0))
                    .sum()
            });
        strength.push(rise);
        if history.len() == lag {
            history.remove(0);
        }
        history.push(levels);
    }
    strength
}

/// The bands of the onset strength, as bin ranges: a third of an octave
/// from [`BAND_LOW_HZ`] up, and at least [`BAND_MIN_BINS`] bins wide, so
/// each band sums whole main lobes and the energy of a steady partial does
/// not swing with the phase of the frame.
fn bands(bins: usize, bin_hz: f64) -> Vec<(usize, usize)> {
    let mut bands = Vec::new();
    let mut lo = cast::ceil_to_usize(BAND_LOW_HZ / bin_hz)
        .unwrap_or(1)
        .max(1);
    while lo < bins {
        let third =
            cast::ceil_to_usize(cast::usize_to_f64(lo) * 2_f64.powf(1.0 / 3.0)).unwrap_or(bins);
        let hi = third.max(lo + BAND_MIN_BINS).min(bins);
        bands.push((lo, hi));
        lo = hi;
    }
    bands
}

/// The onset strength low-passed at [`ONSET_LOW_PASS_HZ`] with no lag,
/// less its centred mean over [`ONSET_MEAN_S`], floored at 0. A rise
/// under [`ONSET_FLOOR_SHARE`] of the largest one becomes 0, so only
/// onsets remain.
fn smooth(onsets: &[f64], fps: f64) -> Vec<f64> {
    let rate = cast::round_to_usize(fps)
        .and_then(|fps| u32::try_from(fps).ok())
        .and_then(|fps| SampleRate::new(fps).ok());
    let smoothed = rate
        .and_then(|rate| low_pass_zero_phase(onsets, rate, Hz::from_const(ONSET_LOW_PASS_HZ)).ok());
    let smoothed = smoothed.unwrap_or_else(|| onsets.to_vec());
    let half = cast::round_to_usize(ONSET_MEAN_S * fps / 2.0).unwrap_or(0);
    let mut prefix = Vec::with_capacity(smoothed.len() + 1);
    prefix.push(0.0);
    for value in &smoothed {
        prefix.push(prefix[prefix.len() - 1] + value);
    }
    let rises: Vec<f64> = (0..smoothed.len())
        .map(|i| {
            let (lo, hi) = (i.saturating_sub(half), (i + half + 1).min(smoothed.len()));
            let mean = (prefix[hi] - prefix[lo]) / cast::usize_to_f64(hi - lo);
            (smoothed[i] - mean).max(0.0)
        })
        .collect();
    let floor = ONSET_FLOOR_SHARE * rises.iter().fold(0.0_f64, |m, &v| m.max(v));
    rises
        .into_iter()
        .map(|v| if v >= floor { v } else { 0.0 })
        .collect()
}

/// The unbiased autocorrelation of `x` at lags 0 to `lags - 1`: each sum
/// is divided by the number of products in it.
fn autocorrelation(x: &[f64], lags: usize) -> Vec<f64> {
    let lags = lags.min(x.len());
    if lags == 0 {
        return Vec::new();
    }
    let len = (x.len() + lags).next_power_of_two();
    let mut planner = RealFftPlanner::<f64>::new();
    let forward = planner.plan_fft_forward(len);
    let inverse = planner.plan_fft_inverse(len);
    let mut input = forward.make_input_vec();
    input[..x.len()].copy_from_slice(x);
    let mut spectrum = forward.make_output_vec();
    if forward.process(&mut input, &mut spectrum).is_err() {
        return vec![0.0; lags];
    }
    for bin in &mut spectrum {
        *bin = Complex64::new(bin.norm_sqr(), 0.0);
    }
    let mut out = inverse.make_output_vec();
    if inverse.process(&mut spectrum, &mut out).is_err() {
        return vec![0.0; lags];
    }
    let scale = cast::usize_to_f64(len);
    out.iter()
        .take(lags)
        .enumerate()
        .map(|(lag, sum)| sum / scale / cast::usize_to_f64(x.len() - lag))
        .collect()
}

/// `values` at a fractional index, by linear interpolation; 0 past the end.
fn interpolate(values: &[f64], at: f64) -> f64 {
    let Some(index) = cast::floor_to_usize(at) else {
        return 0.0;
    };
    let fraction = at - cast::usize_to_f64(index);
    match (values.get(index), values.get(index + 1)) {
        (Some(a), Some(b)) => a + (b - a) * fraction,
        (Some(a), None) => *a,
        _ => 0.0,
    }
}

/// The median, the mean of the middle pair for an even count.
fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    match n {
        0 => 0.0,
        _ if n % 2 == 1 => sorted[n / 2],
        _ => f64::midpoint(sorted[n / 2 - 1], sorted[n / 2]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bpm(value: f64) -> Bpm {
        Bpm::new(value).unwrap()
    }

    #[test]
    fn the_search_holds_every_tempo_the_given_one_lies_within_three_percent_of() {
        let steps = candidates(bpm(120.0));
        assert!((steps[0] - 116.51).abs() < 1e-9, "{}", steps[0]);
        assert!((steps[steps.len() - 1] - 123.71).abs() < 1e-9);
        // 120 is 3 % above 116.505 and 3 % below 123.711.
        assert!(120.0 / steps[0] - 1.0 <= SEARCH_SHARE);
        assert!(1.0 - 120.0 / steps[steps.len() - 1] <= SEARCH_SHARE);
    }

    #[test]
    fn the_search_never_reaches_half_or_double_the_tempo() {
        for given in [4.0, 60.0, 120.0, 173.5, 1000.0] {
            let steps = candidates(bpm(given));
            assert!(steps.iter().all(|&t| t > given * 0.9 && t < given * 1.1));
            assert!(steps.iter().all(|&t| (4.0..=1000.0).contains(&t)));
        }
    }

    #[test]
    fn the_autocorrelation_divides_each_lag_by_its_products() {
        let acf = autocorrelation(&[1.0, 1.0, 1.0, 1.0], 4);
        for value in acf {
            assert!((value - 1.0).abs() < 1e-12, "{value}");
        }
    }

    #[test]
    fn interpolation_reads_between_samples() {
        assert!((interpolate(&[0.0, 2.0], 0.25) - 0.5).abs() < 1e-12);
        assert!(interpolate(&[0.0, 2.0], 5.0).abs() < 1e-12);
    }

    #[test]
    fn exact_keeps_the_given_tempo() {
        let tempo = Tempo::exact(bpm(120.0)).unwrap();
        assert_eq!(tempo.used(), bpm(120.0));
        assert!(tempo.refined().value().is_none());
        assert_eq!(tempo.summary(), "tempo: 120 BPM as given (--bpm-exact)");
    }

    #[test]
    fn silence_keeps_the_given_tempo() {
        let rate = SampleRate::new(8000).unwrap();
        let tempo = Tempo::refine(&vec![0.0; 8000 * 6], rate, bpm(120.0)).unwrap();
        assert_eq!(tempo.used(), bpm(120.0));
        assert!(tempo.refined().value().is_none());
        assert_eq!(
            tempo.summary(),
            "tempo: 120 BPM as given; the stem has no onsets to refine it"
        );
    }

    #[test]
    fn a_short_stem_keeps_the_given_tempo() {
        let rate = SampleRate::new(8000).unwrap();
        // 3 s at 120 BPM is 6 beats: 2 lags after the overlap.
        let clicks: Vec<f64> = (0..8000 * 3)
            .map(|n| if n % 4000 == 0 { 1.0 } else { 0.0 })
            .collect();
        let tempo = Tempo::refine(&clicks, rate, bpm(120.0)).unwrap();
        assert_eq!(tempo.used(), bpm(120.0));
        assert!(
            tempo.summary().contains("too few to refine it"),
            "{}",
            tempo.summary()
        );
    }

    #[test]
    fn a_stem_under_one_beat_says_so() {
        let rate = SampleRate::new(8000).unwrap();
        let tempo = Tempo::refine(&[0.5; 1600], rate, bpm(120.0)).unwrap();
        assert_eq!(
            tempo.summary(),
            "tempo: 120 BPM as given; the stem is shorter than one beat"
        );
    }

    #[test]
    fn an_untrusted_tempo_equal_to_the_given_one_is_not_called_refined() {
        let reasons = vec![
            Reason::new(ReasonCode::TempoPeak, Signal::Number(322.03)).unwrap(),
            Reason::new(ReasonCode::BeatPeriodicity, Signal::Number(0.24)).unwrap(),
            Reason::new(ReasonCode::SparsePeriodicity, Signal::Number(0.5)).unwrap(),
        ];
        let tempo = Tempo {
            given: bpm(120.0),
            refined: Estimate::new(bpm(120.0), Level::Low, None, reasons).unwrap(),
            used: bpm(120.0),
        };
        assert_eq!(tempo.trusted(), None);
        assert_eq!(
            tempo.summary(),
            "tempo: 120 BPM as given; the refined 120 BPM is not trusted (peak ratio 322.03, \
needs 1.5; beat periodicity 0.24, needs 0.4, or 0.50 at 2 or 4 beats, needs 0.75)"
        );
    }
}
