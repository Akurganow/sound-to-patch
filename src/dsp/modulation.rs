//! Modulation primitives: band envelopes at 200 Hz, the modulation
//! spectrum, the cumulative-mean-normalised difference, cycle folding and
//! the snap to a note division.
//!
//! The LFO and unison estimators call these. A plain FFT over gappy samples
//! sees the note rhythm as modulation, so gated samples go to the
//! Lomb-Scargle periodogram instead. Drum bleed adds lines at its pattern
//! rates, so the caller computes envelopes on masked cells only.

use realfft::RealFftPlanner;

use crate::audio::{BeatGrid, Division, SampleRate};
use crate::cast;
use crate::confidence::{ConfidenceError, Estimate, Level, Reason, ReasonCode, Signal};
use crate::units::{Bpm, Cents, Hz, Normalized, Seconds};

use super::Window;
use super::analytic::{AnalyticSignal, FilterError, band_pass_zero_phase};

/// The rate of envelope tracks: a 5 ms hop, Nyquist at 100 Hz.
pub const TRACK_RATE_HZ: Hz = Hz::from_const(200.0);
/// The top of the modulation scan. Above it a modulation leaves the
/// envelope domain and shows as sidebands.
pub const SCAN_TOP_HZ: Hz = Hz::from_const(20.0);
/// The bottom of the modulation scan unless the 4-bar rate lies lower.
pub const SCAN_BOTTOM_HZ: Hz = Hz::from_const(0.2);
/// Zero padding of the modulation spectrum.
pub const SPECTRUM_OVERSAMPLE: usize = 8;
/// A rate needs this many full cycles in the span. A pure count, so a
/// bare `f64`.
pub const MIN_CYCLES: f64 = 2.0;
/// The harmonics of the fundamental-picking sum `S(f) = sum X(h f) / sqrt(h)`.
pub const SUM_HARMONICS: usize = 4;
/// The bins of a folded cycle.
pub const FOLD_BINS: usize = 64;
/// The tempo scan of the snap, as a share of the BPM on either side.
pub const BPM_SCAN: Normalized = Normalized::from_const(0.02);

/// A time series at its own rate, such as a band envelope.
#[derive(Clone, Debug, PartialEq)]
pub struct Track {
    values: Vec<f64>,
    rate: Hz,
}

impl Track {
    /// Pairs values with their rate.
    #[must_use]
    pub fn new(values: Vec<f64>, rate: Hz) -> Self {
        Self { values, rate }
    }

    /// Returns the values.
    #[must_use]
    pub fn values(&self) -> &[f64] {
        &self.values
    }

    /// Returns the rate.
    #[must_use]
    pub fn rate(&self) -> Hz {
        self.rate
    }

    /// Returns the span, `len / rate`.
    #[must_use]
    pub fn span(&self) -> Seconds {
        Seconds::clamped(cast::usize_to_f64(self.values.len()) / self.rate.get())
    }
}

/// The scan range at a tempo: from `min(0.2 Hz, the 4-bar rate)` to
/// 20 Hz, so the 2-bar and 4-bar divisions fall inside it.
#[must_use]
pub fn scan_range(bpm: Bpm) -> (Hz, Hz) {
    let four_bars = Division::ALL[0].rate(bpm).get();
    (
        Hz::clamped(SCAN_BOTTOM_HZ.get().min(four_bars)),
        SCAN_TOP_HZ,
    )
}

/// The envelope of one band at 200 Hz: a zero-phase fourth-order
/// Butterworth high-pass at `lo` and low-pass at `hi`, the magnitude of the
/// analytic signal, then the mean of each 5 ms block.
///
/// Pass a note or a passage, not a whole stem: the analytic signal spans
/// the input in one FFT, see [`AnalyticSignal::new`].
///
/// # Errors
///
/// Returns a [`FilterError`] for an edge at 0 Hz or at or above the
/// Nyquist frequency.
pub fn band_envelope(
    samples: &[f64],
    rate: SampleRate,
    lo: Hz,
    hi: Hz,
) -> Result<Track, FilterError> {
    let band = band_pass_zero_phase(samples, rate, lo, hi)?;
    let pad = cast::floor_to_usize(rate.hz().get() / lo.get()).unwrap_or(0) * 4;
    let envelope = AnalyticSignal::new(&band, rate, pad).envelope();
    Ok(Track::new(
        block_means(&envelope, rate.hz().get() / TRACK_RATE_HZ.get()),
        TRACK_RATE_HZ,
    ))
}

/// Means of consecutive blocks of `ratio` samples; block `j` covers samples
/// `floor(j ratio)` to `floor((j + 1) ratio) - 1`, so a fractional ratio
/// keeps the track rate exact.
fn block_means(values: &[f64], ratio: f64) -> Vec<f64> {
    let blocks = cast::floor_to_usize(cast::usize_to_f64(values.len()) / ratio).unwrap_or(0);
    (0..blocks)
        .filter_map(|j| {
            let start = cast::floor_to_usize(cast::usize_to_f64(j) * ratio)?;
            let end = cast::floor_to_usize(cast::usize_to_f64(j + 1) * ratio)?.min(values.len());
            let block = values.get(start..end).filter(|block| !block.is_empty())?;
            Some(block.iter().sum::<f64>() / cast::usize_to_f64(block.len()))
        })
        .collect()
}

/// Removes the least-squares line from a track.
fn detrend(values: &[f64]) -> Vec<f64> {
    let n = cast::usize_to_f64(values.len());
    if values.len() < 2 {
        return vec![0.0; values.len()];
    }
    let mean_x = (n - 1.0) / 2.0;
    let mean_y = values.iter().sum::<f64>() / n;
    let (mut sxx, mut sxy) = (0.0, 0.0);
    for (i, y) in values.iter().enumerate() {
        let dx = cast::usize_to_f64(i) - mean_x;
        sxx += dx * dx;
        sxy += dx * (y - mean_y);
    }
    let slope = sxy / sxx;
    values
        .iter()
        .enumerate()
        .map(|(i, y)| y - mean_y - slope * (cast::usize_to_f64(i) - mean_x))
        .collect()
}

/// The magnitude spectrum of a detrended, Hann-windowed track, zero-padded
/// 8x. Its resolution stays `1 / T`; the padding samples it finer.
#[derive(Clone, Debug, PartialEq)]
pub struct ModulationSpectrum {
    magnitudes: Vec<f64>,
    bin_hz: f64,
    span: Seconds,
}

impl ModulationSpectrum {
    /// Computes the spectrum of a track.
    #[must_use]
    pub fn of(track: &Track) -> Self {
        let len = track.values().len();
        let window = Window::Hann.samples(len);
        let fft_len = (len * SPECTRUM_OVERSAMPLE).max(2);
        let mut input: Vec<f64> = detrend(track.values())
            .iter()
            .zip(&window)
            .map(|(v, w)| v * w)
            .collect();
        input.resize(fft_len, 0.0);
        let fft = RealFftPlanner::<f64>::new().plan_fft_forward(fft_len);
        let mut output = fft.make_output_vec();
        let result = fft.process(&mut input, &mut output);
        debug_assert!(result.is_ok(), "buffers come from the plan: {result:?}");
        Self {
            magnitudes: output.iter().map(|value| value.norm()).collect(),
            bin_hz: track.rate().get() / cast::usize_to_f64(fft_len),
            span: track.span(),
        }
    }

    /// Returns the magnitudes, one per bin from 0 Hz.
    #[must_use]
    pub fn magnitudes(&self) -> &[f64] {
        &self.magnitudes
    }

    /// Returns the bin spacing.
    #[must_use]
    pub fn bin_width(&self) -> Hz {
        Hz::clamped(self.bin_hz)
    }

    /// Returns the magnitude at a frequency, linearly interpolated between
    /// bins; 0 beyond the last bin.
    #[must_use]
    pub fn magnitude_at(&self, frequency: f64) -> f64 {
        let position = frequency / self.bin_hz;
        let Some(lower) = cast::floor_to_usize(position) else {
            return 0.0;
        };
        match (self.magnitudes.get(lower), self.magnitudes.get(lower + 1)) {
            (Some(a), Some(b)) => a + (position - cast::usize_to_f64(lower)) * (b - a),
            (Some(a), None) => *a,
            _ => 0.0,
        }
    }

    /// Returns the frequency of the largest bin between `lo` and `hi`, with
    /// no interpolation.
    #[must_use]
    pub fn peak(&self, lo: Hz, hi: Hz) -> Option<Hz> {
        self.bins_in(lo, hi)
            .max_by(|&a, &b| self.magnitudes[a].total_cmp(&self.magnitudes[b]))
            .map(|bin| Hz::clamped(cast::usize_to_f64(bin) * self.bin_hz))
    }

    /// Returns the fundamental rate between `lo` and `hi`.
    ///
    /// The candidates are the local maxima with at least
    /// [`CANDIDATE_SHARE`] of the largest magnitude in the range. The pick
    /// maximises `S(f) = sum_{h=1..4} X(h f) / sqrt(h)`: saw and square
    /// shapes put energy at `2 f` and `3 f`, and the sum picks `f`.
    #[must_use]
    pub fn fundamental(&self, lo: Hz, hi: Hz) -> Option<Hz> {
        let peaks: Vec<(f64, f64)> = self
            .bins_in(lo, hi)
            .map(|bin| (cast::usize_to_f64(bin) * self.bin_hz, self.magnitudes[bin]))
            .collect();
        pick_fundamental(&peaks, |f| self.magnitude_at(f)).map(Hz::clamped)
    }

    fn bins_in(&self, lo: Hz, hi: Hz) -> impl Iterator<Item = usize> + '_ {
        let first = cast::ceil_to_usize(lo.get() / self.bin_hz)
            .unwrap_or(0)
            .max(1);
        let last = cast::floor_to_usize(hi.get() / self.bin_hz)
            .unwrap_or(0)
            .min(self.magnitudes.len().saturating_sub(1));
        first..=last
    }

    /// The modulation rate between `lo` and `hi`, by the harmonic sum.
    /// No estimate when the span holds fewer than two cycles of it. The
    /// spread is the resolution, `1 / T`.
    ///
    /// # Errors
    ///
    /// Never in practice; the reasons are built from counts.
    pub fn rate(&self, lo: Hz, hi: Hz) -> Result<Estimate<Hz, Hz>, ConfidenceError> {
        rate_estimate(self.fundamental(lo, hi), self.span)
    }
}

/// A fundamental candidate needs at least this share of the largest
/// amplitude in the range. Without it a gap pattern's alias lines lift the
/// harmonic sum of a subharmonic over the true rate. A proposal.
pub const CANDIDATE_SHARE: Normalized = Normalized::from_const(0.5);

/// Picks the fundamental from `(frequency, amplitude)` points in a range.
///
/// The candidates are the local maxima with at least half the largest
/// amplitude. The pick maximises `S(f) = sum_{h=1..4} X(h f) / sqrt(h)`,
/// with `amplitude_at` reading `X` at any frequency. Saw and square shapes
/// put energy at `2 f` and `3 f`; the sum picks `f`.
pub(crate) fn pick_fundamental(
    points: &[(f64, f64)],
    amplitude_at: impl Fn(f64) -> f64,
) -> Option<f64> {
    let largest = points.iter().map(|p| p.1).fold(0.0, f64::max);
    if largest <= 0.0 {
        return None;
    }
    let score = |f: f64| {
        (1..=SUM_HARMONICS)
            .map(|h| {
                let h = cast::usize_to_f64(h);
                amplitude_at(h * f) / h.sqrt()
            })
            .sum::<f64>()
    };
    (0..points.len())
        .filter(|&i| {
            let amplitude = points[i].1;
            let left = i.checked_sub(1).map_or(0.0, |j| points[j].1);
            let right = points.get(i + 1).map_or(0.0, |p| p.1);
            amplitude >= CANDIDATE_SHARE.get() * largest && amplitude >= left && amplitude >= right
        })
        .map(|i| points[i].0)
        .max_by(|&a, &b| score(a).total_cmp(&score(b)))
}

/// An estimate of a rate found in a span: none below two cycles.
pub(crate) fn rate_estimate(
    rate: Option<Hz>,
    span: Seconds,
) -> Result<Estimate<Hz, Hz>, ConfidenceError> {
    let Some(rate) = rate else {
        return Estimate::none(vec![Reason::new(
            ReasonCode::TooFewCycles,
            Signal::Number(0.0),
        )?]);
    };
    let cycles = rate.get() * span.get();
    let reason = Reason::new(ReasonCode::TooFewCycles, Signal::Number(cycles))?;
    if cycles < MIN_CYCLES {
        Estimate::none(vec![reason])
    } else {
        let resolution = Hz::clamped(1.0 / span.get());
        Estimate::new(rate, Level::Medium, Some(resolution), vec![reason])
    }
}

/// The period of a track between `1 / hi` and `1 / lo`.
///
/// The cumulative mean normalised difference picks the first lag whose
/// value dips below 0.1, else the global minimum. Needs two to three
/// cycles; `None` when the track is shorter than two periods of `lo`.
#[must_use]
pub fn cmndf_period(track: &Track, lo: Hz, hi: Hz) -> Option<Seconds> {
    let values = detrend(track.values());
    let rate = track.rate().get();
    let min_lag = cast::ceil_to_usize(rate / hi.get()).unwrap_or(1).max(1);
    let max_lag = cast::floor_to_usize(rate / lo.get())
        .unwrap_or(0)
        .min(values.len() / 2);
    if max_lag <= min_lag {
        return None;
    }
    let window = values.len() - max_lag;
    let difference = |lag: usize| -> f64 {
        values[..window]
            .iter()
            .zip(&values[lag..lag + window])
            .map(|(a, b)| (a - b).powi(2))
            .sum()
    };
    let mut running = 0.0;
    let mut normalised = Vec::with_capacity(max_lag + 1);
    for lag in 1..=max_lag {
        let d = difference(lag);
        running += d;
        normalised.push(if running > 0.0 {
            d * cast::usize_to_f64(lag) / running
        } else {
            1.0
        });
    }
    let candidates = min_lag..=max_lag;
    let dip = candidates.clone().find(|&lag| {
        let value = normalised[lag - 1];
        value < 0.1 && normalised.get(lag).is_none_or(|next| *next >= value)
    });
    let lag = dip
        .or_else(|| candidates.min_by(|&a, &b| normalised[a - 1].total_cmp(&normalised[b - 1])))?;
    Some(Seconds::clamped(cast::usize_to_f64(lag) / rate))
}

/// Folds a track over a period into 64 bins: bin `b` is the mean of the
/// samples whose phase falls in `[b / 64, (b + 1) / 64)`. The result is one
/// cycle of the waveform.
#[must_use]
pub fn fold(track: &Track, period: Seconds) -> Vec<f64> {
    let mut sums = vec![0.0; FOLD_BINS];
    let mut counts = vec![0_usize; FOLD_BINS];
    let samples_per_period = period.get() * track.rate().get();
    if samples_per_period <= 0.0 {
        return sums;
    }
    for (i, value) in track.values().iter().enumerate() {
        let phase = (cast::usize_to_f64(i) / samples_per_period).fract();
        let bin = cast::floor_to_usize(phase * cast::usize_to_f64(FOLD_BINS))
            .unwrap_or(0)
            .min(FOLD_BINS - 1);
        sums[bin] += value;
        counts[bin] += 1;
    }
    sums.iter()
        .zip(&counts)
        .map(|(sum, &count)| {
            if count > 0 {
                sum / cast::usize_to_f64(count)
            } else {
                0.0
            }
        })
        .collect()
}

/// The coarse shape of a folded cycle. A sine and a triangle are never
/// told apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShapeKind {
    /// Rises and falls in about equal time, such as a sine or a triangle.
    SmoothSymmetric,
    /// Rises over most of the cycle and drops fast.
    RampUp,
    /// Rises fast and falls over most of the cycle.
    RampDown,
    /// Holds its top for a large share of the cycle.
    SquareLike,
}

/// The shape of a folded cycle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    /// The share of the cycle from the minimum forward to the maximum.
    pub rise: Normalized,
    /// The share of bins at 90 % of the range or more.
    pub plateau: Normalized,
    /// The class from the two shares.
    pub kind: ShapeKind,
}

/// Reads the shape of a folded cycle, or `None` for a flat one.
///
/// It is square-like when the plateau holds 35 % of the cycle, a ramp up
/// when the rise takes 75 % or more, a ramp down when it takes 25 % or
/// less, else smooth and symmetric. These thresholds are proposals.
#[must_use]
pub fn shape(cycle: &[f64]) -> Option<Shape> {
    let (min_bin, min) = cycle
        .iter()
        .copied()
        .enumerate()
        .min_by(|a, b| a.1.total_cmp(&b.1))?;
    let (max_bin, max) = cycle
        .iter()
        .copied()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(&b.1))?;
    let range = max - min;
    if range <= 0.0 {
        return None;
    }
    let len = cycle.len();
    let rise = cast::usize_to_f64((max_bin + len - min_bin) % len) / cast::usize_to_f64(len);
    let plateau = cast::usize_to_f64(cycle.iter().filter(|&&v| (v - min) / range >= 0.9).count())
        / cast::usize_to_f64(len);
    let kind = if plateau >= 0.35 {
        ShapeKind::SquareLike
    } else if rise >= 0.75 {
        ShapeKind::RampUp
    } else if rise <= 0.25 {
        ShapeKind::RampDown
    } else {
        ShapeKind::SmoothSymmetric
    };
    Some(Shape {
        rise: Normalized::clamped(rise),
        plateau: Normalized::clamped(plateau),
        kind,
    })
}

/// A rate snapped to the nearest note division.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snap {
    /// The nearest division in log distance.
    pub division: Division,
    /// The tempo the division was read at.
    pub bpm: Bpm,
    /// The rate of the division at that tempo.
    pub rate: Hz,
    /// How far the measured rate lies from it.
    pub residual: Cents,
}

/// Snaps a rate to the nearest division at a tempo, in log distance.
#[must_use]
pub fn snap(rate: Hz, bpm: Bpm) -> Snap {
    let grid = BeatGrid::new(bpm, Seconds::clamped(0.0));
    let residual = |division: Division| 1200.0 * (rate.get() / grid.rate(division).get()).log2();
    let division = Division::ALL
        .iter()
        .copied()
        .min_by(|&a, &b| residual(a).abs().total_cmp(&residual(b).abs()))
        .unwrap_or(Division::ALL[0]);
    Snap {
        division,
        bpm,
        rate: grid.rate(division),
        residual: Cents::clamped(residual(division)),
    }
}

/// Snaps a rate at the BPM and at tempos within 2 % of it, in 0.1 % steps,
/// and returns the snap with the smallest residual. A drifting tempo shows
/// as a best BPM away from the nominal one.
#[must_use]
pub fn snap_scan(rate: Hz, bpm: Bpm) -> Snap {
    let steps = cast::round_to_usize(BPM_SCAN.get() / 0.001).unwrap_or(0);
    (0..=2 * steps)
        .filter_map(|i| {
            let share = (cast::usize_to_f64(i) - cast::usize_to_f64(steps)) * 0.001;
            Bpm::new(bpm.get() * (1.0 + share)).ok()
        })
        .map(|tempo| snap(rate, tempo))
        .min_by(|a, b| a.residual.get().abs().total_cmp(&b.residual.get().abs()))
        .unwrap_or_else(|| snap(rate, bpm))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn the_constants_pass_the_unit_check() {
        assert_eq!(TRACK_RATE_HZ, Hz::new(200.0).unwrap());
        assert_eq!(SCAN_TOP_HZ, Hz::new(20.0).unwrap());
        assert_eq!(SCAN_BOTTOM_HZ, Hz::new(0.2).unwrap());
        assert_eq!(BPM_SCAN, Normalized::new(0.02).unwrap());
        assert_eq!(CANDIDATE_SHARE, Normalized::new(0.5).unwrap());
    }

    fn track(values: Vec<f64>) -> Track {
        Track::new(values, TRACK_RATE_HZ)
    }

    fn periodic(period: usize, cycles: usize, f: impl Fn(f64) -> f64) -> Vec<f64> {
        (0..period * cycles)
            .map(|i| f(cast::usize_to_f64(i % period) / cast::usize_to_f64(period)))
            .collect()
    }

    #[test]
    fn the_scan_starts_at_the_lower_of_02_hz_and_the_4_bar_rate() {
        let (lo, hi) = scan_range(Bpm::new(140.0).unwrap());
        assert!((lo.get() - 140.0 / 60.0 / 16.0).abs() < 1e-12);
        assert_eq!(hi.get(), 20.0);
        assert_eq!(scan_range(Bpm::new(400.0).unwrap()).0.get(), 0.2);
    }

    #[test]
    fn block_means_keep_a_fractional_ratio() {
        assert_eq!(block_means(&[1.0, 3.0, 5.0, 7.0, 9.0], 2.5), vec![2.0, 7.0]);
    }

    #[test]
    fn shapes_read_their_class() {
        let sine = periodic(64, 1, |p| (std::f64::consts::TAU * p).sin());
        let up = periodic(64, 1, |p| p);
        let down = periodic(64, 1, |p| 1.0 - p);
        let square = periodic(64, 1, |p| if p < 0.5 { 1.0 } else { 0.0 });
        assert_eq!(shape(&sine).unwrap().kind, ShapeKind::SmoothSymmetric);
        assert_eq!(shape(&up).unwrap().kind, ShapeKind::RampUp);
        assert_eq!(shape(&down).unwrap().kind, ShapeKind::RampDown);
        assert_eq!(shape(&square).unwrap().kind, ShapeKind::SquareLike);
        assert!(shape(&[1.0; 64]).is_none());
    }

    #[test]
    fn cmndf_finds_the_period_of_a_ramp() {
        let ramp = track(periodic(50, 6, |p| p));
        let period = cmndf_period(&ramp, Hz::new(1.0).unwrap(), Hz::new(20.0).unwrap()).unwrap();
        assert!((period.get() - 0.25).abs() < 1e-12, "{period:?}");
    }

    proptest! {
        #[test]
        fn folding_an_exactly_periodic_envelope_reproduces_it(cycles in 2usize..8, phase in 0.0..1.0f64) {
            // 64 samples per period put one sample in each bin.
            let wave = |p: f64| (std::f64::consts::TAU * (p + phase)).sin() + 0.3 * (2.0 * std::f64::consts::TAU * p).cos();
            let values = periodic(64, cycles, wave);
            let folded = fold(&track(values), Seconds::new(64.0 / TRACK_RATE_HZ.get()).unwrap());
            for (b, value) in folded.iter().enumerate() {
                prop_assert!((value - wave(cast::usize_to_f64(b) / 64.0)).abs() < 1e-12);
            }
        }

        #[test]
        fn a_division_rate_snaps_to_itself(index in 0usize..20, bpm in 40.0..300.0f64) {
            let bpm = Bpm::new(bpm).unwrap();
            let division = Division::ALL[index];
            let snapped = snap(division.rate(bpm), bpm);
            prop_assert_eq!(snapped.division, division);
            prop_assert!(snapped.residual.get().abs() < 1e-9);
        }

        #[test]
        fn doubling_the_bpm_doubles_the_snapped_rate(rate in 0.2..20.0f64, bpm in 40.0..300.0f64) {
            let once = snap(Hz::new(rate).unwrap(), Bpm::new(bpm).unwrap());
            let twice = snap(Hz::new(2.0 * rate).unwrap(), Bpm::new(2.0 * bpm).unwrap());
            prop_assert_eq!(once.division, twice.division);
            prop_assert!((twice.rate.get() / once.rate.get() - 2.0).abs() < 1e-12);
        }

        #[test]
        fn rate_times_beats_per_cycle_is_the_beat_rate(index in 0usize..20, bpm in 4.0..1000.0f64) {
            let division = Division::ALL[index];
            let bpm = Bpm::new(bpm).unwrap();
            let product = division.rate(bpm).get() * division.beats_per_cycle();
            prop_assert!((product / (bpm.get() / 60.0) - 1.0).abs() < 1e-12);
            let doubled = Bpm::new(bpm.get() * 2.0).unwrap_or(bpm);
            if doubled != bpm {
                prop_assert!((division.rate(doubled).get() / division.rate(bpm).get() - 2.0).abs() < 1e-12);
            }
        }
    }
}
