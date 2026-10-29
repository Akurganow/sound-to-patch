//! The pYIN tracker behind the f0 track.
//!
//! It follows pyin 1.2.0 (a Rust port of librosa 0.9.1's `pyin`) step by
//! step. A one-off comparison with the crate found the same frames, voiced
//! flags and f0 to the last bit on generated and real stems, so the goldens
//! and the evaluation of the notes step hold. Only the cost differs: the
//! search keeps one row of values and a table of 16-bit back-pointers
//! instead of dense tables of states by frames.
//!
//! What the tests pin: the decoded pitch bin of each frame of three
//! generated stems, taken from pyin 1.2.0, and the banded search against a
//! dense one. They pin no f0 bit pattern and no hash, because the FFT code
//! may differ in the last bit between platforms.
//!
//! The numerics are those of pyin 1.2.0, not of librosa 1.0: a window of
//! half a frame, no pruned moves, and the last bits of its sums and logs. A
//! change to any of them moves frames, and the pinned bins of the three
//! stems catch it. `tests/it/track.rs` and the local evaluation of the notes
//! step check tolerances only.
//!
//! The code ports pyin-rs (MIT) and the `pyin` function of librosa (ISC).
//! Their notices ship in `THIRD-PARTY-LICENSES.html`, from `about.hbs`.
//!
//! The steps, for each centred frame of the stem:
//!
//! 1. The difference function from an FFT autocorrelation over the first
//!    `win` samples of the frame, then the cumulative mean normalisation.
//! 2. Parabolic interpolation of each lag.
//! 3. A probability per trough: the Boltzmann prior over the troughs below
//!    each of 100 thresholds, weighted by the mass of a Beta(2, 18)
//!    distribution between neighbouring thresholds.
//! 4. A sparse list of observations: the troughs mapped to pitch bins.
//!
//! Then one Viterbi decode over the whole stem. A state is a pitch bin with
//! a voiced or an unvoiced flag. A move within a voicing, or across, is
//! allowed to the bins a triangle window around the bin reaches. Every
//! other move costs `ln(f64::MIN_POSITIVE)`, as in the dense model, so a
//! jump of any size stays possible. The banded search takes the best of the
//! band and the best of everything else, which is the dense maximum.

use std::sync::Arc;

use realfft::num_complex::Complex64;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

use crate::cast;

/// Thresholds of the pYIN prior over the troughs.
const N_THRESHOLDS: usize = 100;
/// Shape of the Boltzmann prior: larger values favour short periods.
const BOLTZMANN: f64 = 2.0;
/// Mass added to the global minimum when no trough lies below a threshold.
const NO_TROUGH_PROB: f64 = 0.01;
/// Probability of switching between voiced and unvoiced.
const SWITCH_PROB: f64 = 0.01;
/// Largest pitch move in octaves per second.
const MAX_TRANSITION_RATE: f64 = 35.92;
/// Values below this magnitude in the autocorrelation and the energy are
/// zero.
const ZEROING: f64 = 1e-6;

/// The Beta(2, 18) mass between neighbouring thresholds `i / 100` and
/// `(i + 1) / 100`: the differences of the cumulative distribution that
/// statrs 0.17.1 gives. The table keeps the last bits of that function, which
/// the closed form `1 - (1 - x)^18 (1 + 18 x)` cannot reproduce: the two
/// differ by up to 1e-13 from rounding.
const BETA_MASS: [f64; N_THRESHOLDS] = [
    0.015_273_761_488_895_312,
    0.039_342_188_545_535_02,
    0.055_339_105_649_276_92,
    0.065_127_207_475_369_13,
    0.070_210_531_770_287_02,
    0.071_795_274_838_104_66,
    0.070_841_338_860_712_64,
    0.068_105_890_099_067_7,
    0.064_180_047_014_210_48,
    0.059_519_675_426_893_404,
    0.054_471_142_167_443_31,
    0.049_292_767_582_689_634,
    0.044_172_619_246_746_09,
    0.039_243_202_889_764_68,
    0.034_593_530_664_463_246,
    0.030_278_980_287_729_462,
    0.026_329_300_292_481_483,
    0.022_755_065_699_036_625,
    0.019_552_844_012_670_434,
    0.016_709_292_845_012_658,
    0.014_204_376_958_819_82,
    0.012_013_863_549_232_506,
    0.010_111_229_558_902_535,
    0.008_469_093_296_324_17,
    0.007_060_264_155_201_334_5,
    0.005_858_488_433_591_269,
    0.004_838_955_783_262_433,
    0.003_978_619_379_049_419_5,
    0.003_256_373_216_461_439,
    0.002_653_121_786_139_19,
    0.002_151_770_526_715_446_3,
    0.001_737_159_739_190_863_3,
    0.001_395_959_894_554_099_6,
    0.001_116_542_340_725_557,
    0.000_888_836_191_588_304_7,
    0.000_704_179_552_486_916_5,
    0.000_555_171_109_967_500_5,
    0.000_435_526_408_054_887_07,
    0.000_339_941_779_561_314_66,
    0.000_263_967_839_100_098_8,
    0.000_203_893_623_558_215_6,
    0.000_156_641_842_264_382_27,
    0.000_119_675_236_054_606_06,
    9.091_371_070_391_041e-5,
    6.866_167_960_661_862e-5,
    5.154_490_127_701_194_5e-5,
    3.845_601_106_977_625e-5,
    2.850_790_854_758_145e-5,
    2.099_415_991_618_780_5e-5,
    1.535_559_902_643_424e-5,
    1.115_235_264_537_162_2e-5,
    8.040_569_739_020_675e-6,
    5.753_195_485_014_295e-6,
    4.084_194_844_433_675e-6,
    2.875_694_958_492_936e-6,
    2.007_578_322_027_47e-6,
    1.389_118_165_828_762_1e-6,
    9.523_028_268_176_859e-7,
    6.465_464_913_452_124e-7,
    4.345_293_523_888_571_5e-7,
    2.889_508_310_399_691e-7,
    1.900_152_554_190_626e-7,
    1.235_004_750_688_162_3e-7,
    7.928_666_545_531_371e-8,
    5.024_540_628_006_946e-8,
    3.140_838_689_130_731_6e-8,
    1.935_122_728_724_536_5e-8,
    1.174_125_197_067_610_3e-8,
    7.009_026_470_328_195e-9,
    4.112_380_991_294_628_5e-9,
    2.368_817_253_461_941e-9,
    1.337_925_326_083_677_6e-9,
    7.399_387_769_169_152e-10,
    4.000_907_383_172_603e-10,
    2.111_437_691_354_467_5e-10,
    1.085_488_365_859_532_7e-10,
    5.424_583_005_009_253_6e-11,
    2.628_730_566_556_214_4e-11,
    1.231_881_263_663_581_2e-11,
    5.564_992_910_933_597e-12,
    2.414_735_078_559_715_5e-12,
    1.002_198_324_329_128_8e-12,
    3.959_055_305_813_308e-13,
    1.479_927_291_825_333_7e-13,
    5.206_945_985_491_984e-14,
    1.709_743_457_922_741e-14,
    5.107_025_913_275_72e-15,
    1.443_289_932_012_703_5e-15,
    3.330_669_073_875_469_6e-16,
    1.110_223_024_625_156_5e-16,
    0.0,
    0.0,
    0.0,
    0.0,
    0.0,
    0.0,
    0.0,
    0.0,
    0.0,
    0.0,
];

/// Why the tracker cannot run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PyinError(pub(super) &'static str);

/// The settings of the tracker.
#[derive(Clone, Copy, Debug)]
pub(super) struct Settings {
    /// The lowest f0 in Hz.
    pub(super) fmin: f64,
    /// The highest f0 in Hz.
    pub(super) fmax: f64,
    /// The sample rate in Hz.
    pub(super) rate: u32,
    /// Samples per frame.
    pub(super) frame: usize,
    /// Samples between frames.
    pub(super) hop: usize,
    /// Pitch bins per semitone.
    pub(super) bins_per_semitone: usize,
}

/// The sum that ndarray 0.15 takes of a contiguous slice: eight running
/// sums over the stride-8 elements, folded in a fixed order, then the tail.
/// Another order changes the last bits.
fn unrolled_sum(values: &[f64]) -> f64 {
    let mut lanes = [0.0_f64; 8];
    let mut rest = values;
    while let Some((chunk, tail)) = rest.split_first_chunk::<8>() {
        for (lane, x) in lanes.iter_mut().zip(chunk) {
            *lane += x;
        }
        rest = tail;
    }
    let mut total = 0.0;
    total += lanes[0] + lanes[4];
    total += lanes[1] + lanes[5];
    total += lanes[2] + lanes[6];
    total += lanes[3] + lanes[7];
    for x in rest {
        total += x;
    }
    total
}

/// The shift in lags of the parabola through three neighbouring points of
/// the cumulative mean normalised difference. It is zero at both ends and
/// where the vertex lies more than a lag away.
fn parabolic_shifts(yin: &[f64], shifts: &mut [f64]) {
    shifts.fill(0.0);
    for (i, window) in yin.windows(3).enumerate() {
        let a = ((window[0] + window[2]) - window[1] * 2.0) / 2.0;
        let b = (window[2] - window[0]) / 2.0;
        let shift = (-b) / (a * 2.0 + f64::MIN_POSITIVE);
        if shift.abs() <= 1.0 {
            shifts[i + 1] = shift;
        }
    }
}

/// The symmetric triangle window of `size` points that stays above zero at
/// both ends, as scipy's `windows.triang(size)` gives it.
fn triangle(size: usize) -> Vec<f64> {
    let width = cast::usize_to_f64(size);
    let odd = size % 2 == 1;
    let half: Vec<f64> = (1..=size.div_ceil(2))
        .map(|m| {
            let m = cast::usize_to_f64(m);
            if odd {
                m * 2.0 / (width + 1.0)
            } else {
                (m * 2.0 - 1.0) / width
            }
        })
        .collect();
    // An odd window shares its peak between the two halves.
    let mirrored = half.iter().rev().skip(usize::from(odd)).copied();
    half.iter().copied().chain(mirrored).collect()
}

/// The moves allowed between pitch bins, as a band around each bin, and the
/// cost of every other move.
#[derive(Debug)]
struct Transition {
    bins: usize,
    /// The log probability of a move outside every band.
    off_band: f64,
    /// For each bin, the first bin that can move to it.
    first: Vec<usize>,
    /// For each bin, where its predecessors start in `within` and `across`.
    start: Vec<usize>,
    /// For each bin, how many predecessors it has.
    len: Vec<usize>,
    /// The log probability of a move within a voicing, by successor.
    within: Vec<f64>,
    /// The log probability of a move across voicings, by successor.
    across: Vec<f64>,
}

impl Transition {
    /// Builds the moves of `bins` pitch bins with a triangle window of
    /// `width` bins, normalised per row, then scaled by the switch
    /// probability.
    fn new(bins: usize, width: usize) -> Result<Self, PyinError> {
        let rows = Self::rows(bins, width)?;
        // The predecessors of a bin are the rows that reach it.
        let mut first = vec![bins; bins];
        let mut last = vec![0; bins];
        for (k, (begin, weights)) in rows.iter().enumerate() {
            for j in *begin..begin + weights.len() {
                first[j] = first[j].min(k);
                last[j] = last[j].max(k);
            }
        }
        let weight = |k: usize, j: usize| {
            let (begin, weights) = &rows[k];
            j.checked_sub(*begin)
                .and_then(|offset| weights.get(offset))
                .copied()
                .unwrap_or(0.0)
        };
        let stay = 1.0 - SWITCH_PROB;
        let (mut start, mut len) = (Vec::with_capacity(bins), Vec::with_capacity(bins));
        let (mut within, mut across) = (Vec::new(), Vec::new());
        for j in 0..bins {
            let count = if first[j] <= last[j] {
                last[j] - first[j] + 1
            } else {
                0
            };
            start.push(within.len());
            len.push(count);
            for k in first[j]..first[j] + count {
                within.push(((weight(k, j) * stay) + f64::MIN_POSITIVE).ln());
                across.push(((weight(k, j) * SWITCH_PROB) + f64::MIN_POSITIVE).ln());
            }
        }
        Ok(Self {
            bins,
            off_band: (0.0 + f64::MIN_POSITIVE).ln(),
            first,
            start,
            len,
            within,
            across,
        })
    }

    /// The normalised weights of each row: the first bin it reaches and
    /// the weights from there. A row is the triangle window centred on its
    /// bin, cut at both ends of the pitch range, then divided by its sum.
    fn rows(bins: usize, width: usize) -> Result<Vec<(usize, Vec<f64>)>, PyinError> {
        if bins < 2 || width == 0 || width >= bins {
            return Err(PyinError(
                "the window of the moves must be narrower than the pitch range",
            ));
        }
        let window = triangle(width);
        let left = (bins - width) / 2;
        let mut rows = Vec::with_capacity(bins);
        for i in 0..bins {
            let mut row = vec![0.0; bins];
            row[left..left + width].copy_from_slice(&window);
            row.rotate_right((bins / 2 + i + 1) % bins);
            row[(i + width / 2 + 1).min(bins)..].fill(0.0);
            row[..i.saturating_sub(width / 2)].fill(0.0);
            let total = unrolled_sum(&row);
            for x in &mut row {
                *x /= total;
            }
            let first = row.iter().position(|&x| x != 0.0).unwrap_or(0);
            let last = row.iter().rposition(|&x| x != 0.0).unwrap_or(first);
            rows.push((first, row[first..=last].to_vec()));
        }
        Ok(rows)
    }
}

const TOO_MANY_BINS: &str = "too many pitch bins for the search";

/// Whether every state of `bins` pitch bins, voiced and unvoiced, fits the
/// 16-bit pointer of the search.
fn states_fit(bins: usize) -> bool {
    bins.checked_mul(2)
        .and_then(|states| states.checked_sub(1))
        .is_some_and(|last| u16::try_from(last).is_ok())
}

/// A state index as the 16-bit pointer the search stores. The search checks
/// that every state fits.
fn pointer(state: usize) -> u16 {
    u16::try_from(state).unwrap_or(u16::MAX)
}

/// A Viterbi decode over `2 * bins` states: the voiced bins, then the
/// unvoiced bins. It takes the log observation probabilities of one frame
/// at a time.
struct Viterbi<'a> {
    transition: &'a Transition,
    values: Vec<f64>,
    next: Vec<f64>,
    /// The best predecessor of each state, frame by frame from the second.
    back: Vec<u16>,
    /// The values after one move outside every band.
    moved: Vec<f64>,
    /// For each half, the first maximum of `moved` over the bins up to and
    /// including each bin, as a value and a state.
    upto: [Vec<(f64, u16)>; 2],
    /// The same over the bins from each bin on.
    from: [Vec<(f64, u16)>; 2],
}

impl<'a> Viterbi<'a> {
    /// Starts at the first frame. `log_init` is the log probability of each
    /// state before it.
    fn new(
        transition: &'a Transition,
        log_init: &[f64],
        log_obs: &[f64],
    ) -> Result<Self, PyinError> {
        let bins = transition.bins;
        if !states_fit(bins) {
            return Err(PyinError(TOO_MANY_BINS));
        }
        let table = || [vec![(0.0, 0); bins], vec![(0.0, 0); bins]];
        Ok(Self {
            transition,
            values: log_init.iter().zip(log_obs).map(|(a, b)| a + b).collect(),
            next: vec![0.0; bins * 2],
            back: Vec::new(),
            moved: vec![0.0; bins * 2],
            upto: table(),
            from: table(),
        })
    }

    /// Adds a frame: the best way into each state from the values of the
    /// frame before.
    fn push(&mut self, log_obs: &[f64]) {
        let bins = self.transition.bins;
        for (moved, value) in self.moved.iter_mut().zip(&self.values) {
            *moved = value + self.transition.off_band;
        }
        for half in 0..2 {
            let moved = &self.moved[half * bins..(half + 1) * bins];
            let mut top = (f64::NEG_INFINITY, 0);
            for (bin, &value) in moved.iter().enumerate() {
                if value > top.0 {
                    top = (value, pointer(half * bins + bin));
                }
                self.upto[half][bin] = top;
            }
            let mut top = (f64::NEG_INFINITY, 0);
            for (bin, &value) in moved.iter().enumerate().rev() {
                if value >= top.0 {
                    top = (value, pointer(half * bins + bin));
                }
                self.from[half][bin] = top;
            }
        }
        for (state, &log) in log_obs.iter().enumerate() {
            let (value, from) = self.best_into(state / bins, state % bins);
            self.next[state] = log + value;
            self.back.push(from);
        }
        std::mem::swap(&mut self.values, &mut self.next);
    }

    /// The best value and predecessor of a state: the maximum over the
    /// moves inside the band and over the moves outside it, the lowest
    /// state on a tie.
    fn best_into(&self, half: usize, bin: usize) -> (f64, u16) {
        let t = self.transition;
        let (begin, count, first) = (t.start[bin], t.len[bin], t.first[bin]);
        let mut best = (f64::NEG_INFINITY, u16::MAX);
        // Inside the band the states run in order, so a strict comparison
        // keeps the lowest.
        for from_half in 0..2 {
            let costs = if from_half == half {
                &t.within
            } else {
                &t.across
            };
            let base = from_half * t.bins + first;
            let candidates = self.values[base..base + count]
                .iter()
                .zip(&costs[begin..begin + count]);
            for (i, (value, cost)) in candidates.enumerate() {
                let candidate = value + cost;
                if candidate > best.0 {
                    best = (candidate, pointer(base + i));
                }
            }
        }
        let end = first + count;
        for from_half in 0..2 {
            let outside = [
                (first > 0).then(|| self.upto[from_half][first - 1]),
                (end < t.bins).then(|| self.from[from_half][end]),
            ];
            for (value, state) in outside.into_iter().flatten() {
                if value > best.0 || (value >= best.0 && state < best.1) {
                    best = (value, state);
                }
            }
        }
        best
    }

    /// The best state sequence, one state per frame.
    fn finish(self) -> Vec<usize> {
        let states = self.transition.bins * 2;
        let frames = self.back.len() / states + 1;
        let mut path = vec![0; frames];
        let mut top = (f64::NEG_INFINITY, 0);
        for (state, &value) in self.values.iter().enumerate() {
            if value > top.0 {
                top = (value, state);
            }
        }
        path[frames - 1] = top.1;
        for t in (0..frames - 1).rev() {
            path[t] = usize::from(self.back[t * states + path[t + 1]]);
        }
        path
    }
}

/// The tracker for one set of settings.
pub(super) struct Pyin {
    settings: Settings,
    window: usize,
    min_period: usize,
    max_period: usize,
    bins: usize,
    forward: Arc<dyn RealToComplex<f64>>,
    inverse: Arc<dyn ComplexToReal<f64>>,
    transition: Transition,
    /// The upper bound of each of the 100 threshold intervals.
    upper: [f64; N_THRESHOLDS],
    /// `exp(-BOLTZMANN * k)` for `k` up to the number of lags.
    decay: Vec<f64>,
}

/// The buffers of one run.
struct Work {
    frame: Vec<f64>,
    fft_in: Vec<f64>,
    spectrum: Vec<Complex64>,
    reversed: Vec<Complex64>,
    acf: Vec<f64>,
    forward_scratch: Vec<Complex64>,
    inverse_scratch: Vec<Complex64>,
    diff: Vec<f64>,
    cumulative: Vec<f64>,
    means: Vec<f64>,
    yin: Vec<f64>,
    shifts: Vec<f64>,
    troughs: Vec<usize>,
    probs: Vec<f64>,
    /// The troughs by the first threshold they lie under, then the running
    /// count of troughs under each threshold. The last slot only takes the
    /// troughs at or above the top threshold.
    under: [usize; N_THRESHOLDS + 1],
    running: [usize; N_THRESHOLDS],
    fact: [f64; N_THRESHOLDS],
    prior: [f64; N_THRESHOLDS],
    obs: Vec<f64>,
    touched: Vec<usize>,
    log_obs: Vec<f64>,
}

impl Pyin {
    /// Checks the settings and plans the transforms.
    pub(super) fn new(settings: Settings) -> Result<Self, PyinError> {
        let Settings {
            fmin,
            fmax,
            rate,
            frame,
            hop,
            bins_per_semitone,
        } = settings;
        let rate_hz = f64::from(rate);
        if !(0.0 < fmin && fmin < fmax && fmax <= rate_hz / 2.0) {
            return Err(PyinError(
                "fmin and fmax must satisfy 0 < fmin < fmax <= rate / 2",
            ));
        }
        let window = frame / 2;
        if window == 0 || hop == 0 || bins_per_semitone == 0 {
            return Err(PyinError(
                "the frame, the hop and the bins per semitone must be positive",
            ));
        }
        let min_period = cast::floor_to_usize(rate_hz / fmax).map_or(1, |p| p.max(1));
        let longest = cast::ceil_to_usize(rate_hz / fmin)
            .ok_or(PyinError("the longest period does not fit"))?;
        let max_period = longest.min(frame - window - 1);
        if max_period < min_period + 2 {
            return Err(PyinError("the frame is too short for the pitch range"));
        }
        let per_semitone = cast::usize_to_f64(bins_per_semitone);
        let bins = cast::floor_to_usize(12.0 * per_semitone * (fmax / fmin).log2())
            .ok_or(PyinError("the pitch range does not fit"))?
            + 1;
        let semitones =
            cast::round_to_usize(MAX_TRANSITION_RATE * 12.0 * cast::usize_to_f64(hop) / rate_hz)
                .ok_or(PyinError("the hop does not fit"))?;
        // Checked before the moves, which take memory by the square of the bins.
        if !states_fit(bins) {
            return Err(PyinError(TOO_MANY_BINS));
        }
        let transition = Transition::new(bins, semitones * bins_per_semitone + 1)?;
        let mut planner = RealFftPlanner::<f64>::new();
        let step = (1.0 - 0.0) / cast::usize_to_f64(N_THRESHOLDS);
        let lags = max_period - min_period + 1;
        Ok(Self {
            settings,
            window,
            min_period,
            max_period,
            bins,
            forward: planner.plan_fft_forward(frame),
            inverse: planner.plan_fft_inverse(frame),
            transition,
            upper: std::array::from_fn(|j| 0.0 + step * cast::usize_to_f64(j + 1)),
            decay: (0..=lags)
                .map(|k| (-BOLTZMANN * cast::usize_to_f64(k)).exp())
                .collect(),
        })
    }

    fn work(&self) -> Work {
        let frame = self.settings.frame;
        let lags = self.max_period - self.min_period + 1;
        Work {
            frame: vec![0.0; frame],
            fft_in: vec![0.0; frame],
            spectrum: self.forward.make_output_vec(),
            reversed: self.forward.make_output_vec(),
            acf: vec![0.0; frame],
            forward_scratch: self.forward.make_scratch_vec(),
            inverse_scratch: self.inverse.make_scratch_vec(),
            diff: vec![0.0; frame - self.window],
            cumulative: vec![0.0; frame],
            means: Vec::with_capacity(self.max_period),
            yin: vec![0.0; lags],
            shifts: vec![0.0; lags],
            troughs: Vec::new(),
            probs: Vec::new(),
            under: [0; N_THRESHOLDS + 1],
            running: [0; N_THRESHOLDS],
            fact: [0.0; N_THRESHOLDS],
            prior: [0.0; N_THRESHOLDS],
            obs: vec![0.0; self.bins],
            touched: Vec::new(),
            log_obs: vec![0.0; self.bins * 2],
        }
    }

    /// The f0 of each frame, `None` where the frame reads as unvoiced. The
    /// stem is padded with half a frame of zeros on both sides, so frame
    /// `m` is centred on sample `m * hop`. An empty stem gives no frames.
    pub(super) fn run(&self, samples: &[f64]) -> Result<Vec<Option<f64>>, PyinError> {
        let Settings { frame, hop, .. } = self.settings;
        let pad = frame / 2;
        let padded = samples.len() + 2 * pad;
        if samples.is_empty() || padded < frame {
            return Ok(Vec::new());
        }
        let bins = self.bins;
        let off = self.transition.off_band;
        let unvoiced_init = (1.0 / cast::usize_to_f64(bins) + f64::MIN_POSITIVE).ln();
        let log_init: Vec<f64> = (0..bins * 2)
            .map(|state| if state < bins { off } else { unvoiced_init })
            .collect();
        let mut work = self.work();
        let mut search: Option<Viterbi<'_>> = None;
        for m in 0..=(padded - frame) / hop {
            for (offset, slot) in work.frame.iter_mut().enumerate() {
                *slot = (m * hop + offset)
                    .checked_sub(pad)
                    .and_then(|i| samples.get(i))
                    .copied()
                    .unwrap_or(0.0);
            }
            self.observe(&mut work)?;
            if let Some(search) = search.as_mut() {
                search.push(&work.log_obs);
            } else {
                search = Some(Viterbi::new(&self.transition, &log_init, &work.log_obs)?);
            }
        }
        let path = search.map_or_else(Vec::new, Viterbi::finish);
        Ok(path
            .into_iter()
            .map(|state| (state < bins).then(|| self.frequency(state)))
            .collect())
    }

    /// The frequency of a voiced pitch bin.
    fn frequency(&self, bin: usize) -> f64 {
        let per_octave = 12.0 * cast::usize_to_f64(self.settings.bins_per_semitone);
        self.settings.fmin * (cast::usize_to_f64(bin) / per_octave).exp2()
    }

    /// Turns the frame in `work.frame` into the log observation
    /// probabilities of every state, in `work.log_obs`.
    fn observe(&self, work: &mut Work) -> Result<(), PyinError> {
        self.difference(work)?;
        parabolic_shifts(&work.yin, &mut work.shifts);
        self.trough_probabilities(work);
        let bins = self.bins;
        for &bin in &work.touched {
            work.obs[bin] = 0.0;
        }
        work.touched.clear();
        let per_octave = 12.0 * cast::usize_to_f64(self.settings.bins_per_semitone);
        let rate = f64::from(self.settings.rate);
        // Troughs run from the shortest period to the longest, and a later
        // trough overwrites an earlier one in the same bin.
        for (&trough, &probability) in work.troughs.iter().zip(&work.probs) {
            if probability == 0.0 {
                continue;
            }
            let period = cast::usize_to_f64(trough + self.min_period) + work.shifts[trough];
            let bin =
                cast::round_to_usize(((rate / period) / self.settings.fmin).log2() * per_octave)
                    .ok_or(PyinError("a trough has no pitch bin"))?;
            // A bin past the top lands in the unvoiced half, which the
            // frame's unvoiced probability then overwrites.
            if bin < bins {
                work.obs[bin] = probability;
                work.touched.push(bin);
            }
        }
        work.touched.sort_unstable();
        work.touched.dedup();
        work.log_obs[..bins].fill(self.transition.off_band);
        let mut voiced = 0.0;
        for &bin in &work.touched {
            voiced += work.obs[bin];
            work.log_obs[bin] = (work.obs[bin] + f64::MIN_POSITIVE).ln();
        }
        let voiced = voiced.clamp(0.0, 1.0);
        let unvoiced = ((-voiced) + 1.0) / cast::usize_to_f64(bins);
        work.log_obs[bins..].fill((unvoiced + f64::MIN_POSITIVE).ln());
        Ok(())
    }

    /// The cumulative mean normalised difference of the frame, in
    /// `work.yin`, for the lags `min_period..=max_period`.
    fn difference(&self, work: &mut Work) -> Result<(), PyinError> {
        let frame = self.settings.frame;
        let win = self.window;
        let failed = PyinError("a transform failed");
        // The autocorrelation: the frame against its first `win` samples,
        // reversed.
        work.fft_in.copy_from_slice(&work.frame);
        self.forward
            .process_with_scratch(
                &mut work.fft_in,
                &mut work.spectrum,
                &mut work.forward_scratch,
            )
            .map_err(|_| failed)?;
        work.fft_in.fill(0.0);
        for (k, slot) in work.fft_in[..win].iter_mut().enumerate() {
            *slot = work.frame[win - k];
        }
        self.forward
            .process_with_scratch(
                &mut work.fft_in,
                &mut work.reversed,
                &mut work.forward_scratch,
            )
            .map_err(|_| failed)?;
        for (x, y) in work.spectrum.iter_mut().zip(&work.reversed) {
            *x *= y;
        }
        self.inverse
            .process_with_scratch(&mut work.spectrum, &mut work.acf, &mut work.inverse_scratch)
            .map_err(|_| failed)?;
        // The energy of every window of `win` samples, from cumulative sums.
        let mut sum = 0.0;
        for (slot, x) in work.cumulative.iter_mut().zip(&work.frame) {
            sum += x * x;
            *slot = sum;
        }
        let zero_small = |x: f64| if x.abs() >= ZEROING { x } else { 0.0 };
        let energy = |j: usize| zero_small(work.cumulative[win + j] - work.cumulative[j]);
        let lead = energy(0);
        let length = cast::usize_to_f64(frame);
        for j in 0..=self.max_period {
            let correlation = zero_small(work.acf[win + j] / length);
            work.diff[j] = (lead + energy(j)) - correlation * 2.0;
        }
        // The cumulative mean over lags 1 to `max_period`.
        work.means.clear();
        let mut running = 0.0;
        for (i, lag) in (1..=self.max_period).enumerate() {
            running += work.diff[lag];
            work.means.push(running / cast::usize_to_f64(i + 1));
        }
        for (i, slot) in work.yin.iter_mut().enumerate() {
            let lag = self.min_period + i;
            *slot = work.diff[lag] / (work.means[lag - 1] + f64::MIN_POSITIVE);
        }
        Ok(())
    }

    /// Finds the troughs of `work.yin` and the probability the pYIN prior
    /// gives each, in `work.troughs` and `work.probs`.
    fn trough_probabilities(&self, work: &mut Work) {
        let yin = &work.yin;
        let last = yin.len() - 1;
        work.troughs.clear();
        work.probs.clear();
        for i in 0..=last {
            let is_trough = if i == 0 {
                yin[0] < yin[1]
            } else if i == last {
                yin[last] < yin[last - 1]
            } else {
                yin[i] < yin[i - 1] && yin[i] <= yin[i + 1]
            };
            if is_trough {
                work.troughs.push(i);
            }
        }
        if work.troughs.is_empty() {
            return;
        }
        // A trough lies under every threshold from this one on.
        let first_threshold = |value: f64| {
            if value.is_nan() {
                N_THRESHOLDS
            } else {
                self.upper.partition_point(|&u| u <= value)
            }
        };
        work.under.fill(0);
        for &trough in &work.troughs {
            work.under[first_threshold(yin[trough])] += 1;
        }
        let mut total = 0;
        for count in &mut work.under[..N_THRESHOLDS] {
            total += *count;
            *count = total;
        }
        let numerator = 1.0 - (-BOLTZMANN).exp();
        for (fact, &n) in work.fact.iter_mut().zip(&work.under) {
            *fact = numerator / (1.0 - self.decay[n]);
        }
        work.running.fill(0);
        for &trough in &work.troughs {
            work.prior.fill(0.0);
            for (j, mass) in BETA_MASS
                .iter()
                .enumerate()
                .skip(first_threshold(yin[trough]))
            {
                let position = work.running[j];
                work.running[j] += 1;
                work.prior[j] = work.fact[j] * self.decay[position] * mass;
            }
            work.probs.push(unrolled_sum(&work.prior));
        }
        // The thresholds above the lowest trough give it their mass too.
        let mut lowest = 0;
        for (slot, &trough) in work.troughs.iter().enumerate() {
            if yin[trough] < yin[work.troughs[lowest]] {
                lowest = slot;
            }
        }
        let uncovered = first_threshold(yin[work.troughs[lowest]]);
        work.probs[lowest] += NO_TROUGH_PROB * unrolled_sum(&BETA_MASS[..uncovered]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cumulative distribution of Beta(2, 18), `1 - (1 - x)^18 (1 + 18 x)`.
    fn beta_cdf(x: f64) -> f64 {
        1.0 - (1.0 - x).powi(18) * (1.0 + 18.0 * x)
    }

    #[test]
    fn the_beta_mass_is_the_difference_of_the_closed_form() {
        for (i, &mass) in BETA_MASS.iter().enumerate() {
            let low = beta_cdf(cast::usize_to_f64(i) / 100.0);
            let high = beta_cdf(cast::usize_to_f64(i + 1) / 100.0);
            assert!((mass - (high - low)).abs() < 1e-12, "interval {i}");
        }
        let total: f64 = BETA_MASS.iter().sum();
        assert!((total - 1.0).abs() < 1e-15, "{total}");
        // The mode of Beta(2, 18) is 1 / 18, inside the sixth interval.
        let peak = first_max_index(&BETA_MASS);
        assert_eq!(peak, 5);
    }

    fn first_max_index(values: &[f64]) -> usize {
        let mut best = 0;
        for (i, &value) in values.iter().enumerate() {
            if value > values[best] {
                best = i;
            }
        }
        best
    }

    #[test]
    fn the_sum_adds_in_the_order_of_eight_lanes() {
        // Lanes 0 and 4 hold 1e16 and -1e16 and lose the ones added to
        // them. The lanes end at [1e16, 2, 2, 2, -1e16, 2, 2, 2], the fold
        // gives 12 and the two tail elements add 2. A left-to-right sum
        // of the same values gives 13.
        let values = [
            1e16, 1.0, 1.0, 1.0, -1e16, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 1.0,
            1.0,
        ];
        assert_eq!(unrolled_sum(&values), 14.0);
        assert_eq!(values.iter().sum::<f64>(), 13.0);
        assert_eq!(unrolled_sum(&[]), 0.0);
        assert_eq!(unrolled_sum(&[2.0, 3.0]), 5.0);
    }

    #[test]
    fn parabolic_shifts_read_the_vertex_and_leave_the_ends_at_zero() {
        // y = (d - 0.3)^2 around the middle point: the vertex is 0.3 lags on.
        let vertex = 0.3_f64;
        let yin = [
            5.0,
            (1.0 + vertex).powi(2),
            vertex * vertex,
            (1.0 - vertex).powi(2),
            7.0,
        ];
        let mut shifts = [9.0; 5];
        parabolic_shifts(&yin, &mut shifts);
        assert_eq!(shifts[0], 0.0);
        assert_eq!(shifts[4], 0.0);
        // Around the second point the neighbours are 5 and 0.09: not a
        // parabola of vertex 0.3, so only the middle one is checked.
        assert!((shifts[2] - vertex).abs() < 1e-12, "{}", shifts[2]);
    }

    #[test]
    fn a_vertex_more_than_a_lag_away_gives_no_shift() {
        let mut shifts = [9.0; 3];
        // The vertex of this parabola lies 1.5 lags past the last point.
        parabolic_shifts(&[0.0, 1.0, 1.5], &mut shifts);
        assert_eq!(shifts, [0.0; 3]);
        // A flat run has no curvature, and the shift stays zero.
        parabolic_shifts(&[2.0, 2.0, 2.0], &mut shifts);
        assert_eq!(shifts, [0.0; 3]);
    }

    #[test]
    fn the_triangle_window_matches_the_symmetric_one() {
        assert_eq!(triangle(3), [0.5, 1.0, 0.5]);
        assert_eq!(triangle(4), [0.25, 0.75, 0.75, 0.25]);
        let five = triangle(5);
        assert!((five[0] - 1.0 / 3.0).abs() < 1e-16 && (five[1] - 2.0 / 3.0).abs() < 1e-16);
        assert_eq!(five[2], 1.0);
        let wide = triangle(31);
        assert_eq!(wide.len(), 31);
        assert_eq!(wide[15], 1.0);
        assert_eq!(wide[0], 0.0625);
        assert_eq!(wide[30], 0.0625);
    }

    #[test]
    fn the_edge_rows_of_the_band_are_cut_and_renormalised() {
        let rows = Transition::rows(5, 3).unwrap();
        let expect = [
            (0, vec![2.0 / 3.0, 1.0 / 3.0]),
            (0, vec![0.25, 0.5, 0.25]),
            (1, vec![0.25, 0.5, 0.25]),
            (2, vec![0.25, 0.5, 0.25]),
            (3, vec![1.0 / 3.0, 2.0 / 3.0]),
        ];
        assert_eq!(rows.len(), expect.len());
        for (row, (begin, weights)) in rows.iter().zip(&expect) {
            assert_eq!(row.0, *begin);
            assert_eq!(row.1.len(), weights.len());
            for (a, b) in row.1.iter().zip(weights) {
                assert!((a - b).abs() < 1e-15);
            }
        }
        // The tracker's own size: 888 bins and a window of 31.
        let rows = Transition::rows(888, 31).unwrap();
        assert_eq!((rows[0].0, rows[0].1.len()), (0, 16));
        assert_eq!((rows[887].0, rows[887].1.len()), (872, 16));
        assert_eq!((rows[100].0, rows[100].1.len()), (85, 31));
        for (begin, weights) in &rows {
            let total: f64 = weights.iter().sum();
            assert!((total - 1.0).abs() < 1e-14, "row at {begin}");
        }
    }

    #[test]
    fn a_window_as_wide_as_the_range_is_refused() {
        assert!(Transition::new(5, 5).is_err());
        assert!(Transition::new(1, 1).is_err());
        assert!(Transition::new(5, 0).is_err());
    }

    #[test]
    fn the_predecessors_of_a_bin_are_the_rows_that_reach_it() {
        let transition = Transition::new(5, 3).unwrap();
        assert_eq!(transition.first, [0, 0, 1, 2, 3]);
        assert_eq!(transition.len, [2, 3, 3, 3, 2]);
        // A move within the band is likelier than a move across voicings,
        // and both beat the cost of a jump.
        for (within, across) in transition.within.iter().zip(&transition.across) {
            assert!(within > across && *across > transition.off_band);
        }
    }

    /// A seeded stream of values in `[0, 1)`.
    struct Stream(u64);

    impl Stream {
        fn next(&mut self) -> f64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            cast::unit_interval(self.0.rotate_left(29) ^ (self.0 >> 7))
        }
    }

    /// The dense search of pyin 1.2.0 over the same log probabilities, as an
    /// oracle: the maximum over every predecessor, the lowest on a tie.
    fn dense(
        transition: &Transition,
        log_init: &[f64],
        frames: &[Vec<f64>],
    ) -> (Vec<usize>, Vec<f64>) {
        let bins = transition.bins;
        let states = bins * 2;
        let mut log_trans = vec![transition.off_band; states * states];
        for j in 0..bins {
            for i in 0..transition.len[j] {
                let k = transition.first[j] + i;
                let within = transition.within[transition.start[j] + i];
                let across = transition.across[transition.start[j] + i];
                for half in 0..2 {
                    log_trans[(half * bins + k) * states + half * bins + j] = within;
                    log_trans[(half * bins + k) * states + (1 - half) * bins + j] = across;
                }
            }
        }
        let mut values: Vec<f64> = log_init
            .iter()
            .zip(&frames[0])
            .map(|(a, b)| a + b)
            .collect();
        let mut back: Vec<Vec<usize>> = Vec::new();
        for frame in &frames[1..] {
            let mut next = vec![0.0; states];
            let mut pointers = vec![0; states];
            for j in 0..states {
                let mut best = (f64::NEG_INFINITY, 0);
                for k in 0..states {
                    let candidate = values[k] + log_trans[k * states + j];
                    if candidate > best.0 {
                        best = (candidate, k);
                    }
                }
                next[j] = frame[j] + best.0;
                pointers[j] = best.1;
            }
            values = next;
            back.push(pointers);
        }
        let mut state = first_max_index(&values);
        let mut path = vec![state];
        for pointers in back.iter().rev() {
            state = pointers[state];
            path.push(state);
        }
        path.reverse();
        (path, values)
    }

    /// Runs the banded search and returns its path and last values.
    fn banded(
        transition: &Transition,
        log_init: &[f64],
        frames: &[Vec<f64>],
    ) -> (Vec<usize>, Vec<f64>) {
        let mut search = Viterbi::new(transition, log_init, &frames[0]).unwrap();
        for frame in &frames[1..] {
            search.push(frame);
        }
        let values = search.values.clone();
        (search.finish(), values)
    }

    fn random_frames(
        stream: &mut Stream,
        states: usize,
        count: usize,
        steps: f64,
    ) -> Vec<Vec<f64>> {
        (0..count)
            .map(|_| {
                (0..states)
                    .map(|_| {
                        let draw = stream.next() * steps;
                        // Whole steps of 0.5 make many equal values.
                        -(if steps > 0.0 {
                            draw.floor() * 0.5
                        } else {
                            draw
                        })
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn the_banded_search_equals_the_dense_search_on_random_input() {
        let transition = Transition::new(24, 7).unwrap();
        let mut stream = Stream(7);
        let log_init: Vec<f64> = (0..48).map(|s| if s < 24 { -30.0 } else { -3.0 }).collect();
        for steps in [8.0, 1.0e6] {
            for _ in 0..6 {
                let frames = random_frames(&mut stream, 48, 40, steps);
                assert_eq!(
                    banded(&transition, &log_init, &frames),
                    dense(&transition, &log_init, &frames)
                );
            }
        }
    }

    #[test]
    fn the_tie_break_takes_the_lowest_state_in_and_out_of_the_band() {
        // Every move costs the same as a jump, so every predecessor ties
        // with every other in every frame: the lowest state, 0, wins.
        let bins = 6;
        let cost = -5.0;
        let transition = Transition {
            bins,
            off_band: cost,
            first: (0..bins)
                .map(|j| j.saturating_sub(1).min(bins - 3))
                .collect(),
            start: (0..bins).map(|j| j * 3).collect(),
            len: vec![3; bins],
            within: vec![cost; bins * 3],
            across: vec![cost; bins * 3],
        };
        let log_init = vec![0.0; bins * 2];
        let frames = vec![vec![0.0; bins * 2]; 5];
        let (path, _) = banded(&transition, &log_init, &frames);
        assert_eq!(path, dense(&transition, &log_init, &frames).0);
        assert!(path.iter().all(|&state| state == 0), "{path:?}");
        // Whole-number values keep many ties between band and jump.
        let mut stream = Stream(11);
        for _ in 0..8 {
            let frames = random_frames(&mut stream, bins * 2, 30, 4.0);
            assert_eq!(
                banded(&transition, &log_init, &frames),
                dense(&transition, &log_init, &frames)
            );
        }
    }

    #[test]
    fn a_jump_wider_than_the_band_stays_possible() {
        let transition = Transition::new(40, 5).unwrap();
        let states = 80;
        let log_init = vec![0.0; states];
        let frames: Vec<Vec<f64>> = (0..12)
            .map(|t| {
                let target = if t < 6 { 3 } else { 35 };
                (0..states)
                    .map(|s| if s == target { 0.0 } else { -300.0 })
                    .collect()
            })
            .collect();
        let (path, _) = banded(&transition, &log_init, &frames);
        let expected: Vec<usize> = (0..12).map(|t| if t < 6 { 3 } else { 35 }).collect();
        assert_eq!(path, expected);
        assert_eq!(path, dense(&transition, &log_init, &frames).0);
    }

    fn sine(hz: f64, seconds: f64, rate: f64) -> Vec<f64> {
        let count = cast::floor_to_usize(seconds * rate).unwrap();
        (0..count)
            .map(|n| (std::f64::consts::TAU * hz * cast::usize_to_f64(n) / rate).sin() * 0.5)
            .collect()
    }

    fn settings() -> Settings {
        Settings {
            fmin: 25.0,
            fmax: 4200.0,
            rate: 22_050,
            frame: 2048,
            hop: 128,
            bins_per_semitone: 10,
        }
    }

    #[test]
    fn a_sine_reads_at_its_pitch_with_one_frame_per_hop_and_one_more() {
        let tracker = Pyin::new(settings()).unwrap();
        let samples = sine(220.0, 0.4, 22_050.0);
        let frames = tracker.run(&samples).unwrap();
        assert_eq!(frames.len(), samples.len() / 128 + 1);
        let voiced: Vec<f64> = frames.iter().flatten().copied().collect();
        assert!(voiced.len() > frames.len() * 9 / 10);
        for hz in voiced.iter().skip(20).take(voiced.len() - 40) {
            let cents = 1200.0 * (hz / 220.0).log2();
            assert!(cents.abs() < 20.0, "{hz} Hz");
        }
    }

    #[test]
    fn silence_and_an_empty_stem_have_no_voiced_frame() {
        let tracker = Pyin::new(settings()).unwrap();
        assert_eq!(tracker.run(&[]).unwrap().len(), 0);
        let frames = tracker.run(&vec![0.0; 5000]).unwrap();
        assert_eq!(frames.len(), 5000 / 128 + 1);
        assert!(frames.iter().all(Option::is_none));
        // A stem shorter than a hop still gives its one frame.
        assert_eq!(tracker.run(&[0.1; 3]).unwrap().len(), 1);
    }

    #[test]
    fn settings_the_tracker_cannot_meet_are_refused() {
        let base = settings();
        let bad = [
            Settings { fmin: 0.0, ..base },
            Settings { fmax: 20.0, ..base },
            Settings {
                fmax: 12_000.0,
                ..base
            },
            Settings { hop: 0, ..base },
            Settings { frame: 1, ..base },
            Settings { frame: 12, ..base },
            Settings {
                bins_per_semitone: 0,
                ..base
            },
            // A window of the moves as wide as the whole range.
            Settings {
                fmin: 4000.0,
                fmax: 4200.0,
                bins_per_semitone: 1,
                ..base
            },
        ];
        for settings in bad {
            assert!(Pyin::new(settings).is_err(), "{settings:?}");
        }
    }

    /// The decoded pitch path that pyin 1.2.0 gives for three generated
    /// stems, as runs of `(bin, frames)` with `bin = round(120 log2(f0 / 25))`
    /// and `None` for an unvoiced frame. Each stem was also read with a
    /// relative noise of 1e-15 on every sample, 40 times, without a change.
    /// The stems and the runs below are taken from the crate `pyin` 1.2.0.
    const GAPS_AND_NOISE: &[(Option<usize>, usize)] = &[
        (None, 1),
        (Some(260), 67),
        (Some(261), 4),
        (Some(262), 1),
        (Some(263), 1),
        (Some(264), 1),
        (Some(272), 1),
        (None, 19),
        (Some(330), 74),
        (Some(331), 2),
        (None, 19),
        (Some(381), 1),
        (Some(380), 73),
        (Some(381), 1),
        (None, 20),
    ];

    const JUMPS: &[(Option<usize>, usize)] = &[
        (None, 1),
        (Some(261), 1),
        (Some(260), 23),
        (Some(263), 1),
        (Some(262), 1),
        (Some(261), 6),
        (Some(260), 20),
        (None, 4),
        (Some(297), 1),
        (Some(299), 1),
        (Some(300), 20),
        (Some(306), 1),
        (Some(314), 1),
        (Some(317), 1),
        (Some(318), 1),
        (Some(319), 1),
        (Some(320), 20),
        (None, 5),
        (Some(257), 1),
        (Some(260), 1),
        (Some(259), 1),
        (Some(260), 16),
        (Some(259), 1),
        (Some(256), 1),
        (None, 5),
        (Some(320), 20),
        (Some(321), 1),
        (None, 5),
        (Some(380), 20),
        (Some(381), 1),
        (None, 5),
        (Some(299), 1),
        (Some(301), 1),
        (Some(300), 17),
        (Some(301), 1),
    ];

    const GLIDE: &[(Option<usize>, usize)] = &[
        (None, 35),
        (Some(310), 54),
        (Some(311), 1),
        (Some(312), 1),
        (Some(314), 1),
        (Some(317), 1),
        (Some(318), 1),
        (Some(321), 1),
        (Some(323), 1),
        (Some(326), 1),
        (Some(328), 1),
        (Some(330), 1),
        (Some(333), 1),
        (Some(335), 1),
        (Some(337), 1),
        (Some(340), 1),
        (Some(342), 1),
        (Some(344), 1),
        (Some(346), 1),
        (Some(348), 1),
        (Some(351), 1),
        (Some(354), 1),
        (Some(356), 1),
        (Some(358), 1),
        (Some(360), 1),
        (Some(362), 1),
        (Some(365), 1),
        (Some(367), 1),
        (Some(369), 1),
        (Some(371), 1),
        (Some(374), 1),
        (Some(376), 1),
        (Some(378), 1),
        (Some(381), 1),
        (Some(383), 1),
        (Some(386), 1),
        (Some(388), 1),
        (Some(389), 1),
        (Some(393), 1),
        (Some(394), 1),
        (Some(397), 1),
        (Some(399), 1),
        (Some(402), 1),
        (Some(403), 1),
        (Some(406), 1),
        (Some(408), 1),
        (Some(411), 1),
        (Some(413), 1),
        (Some(416), 1),
        (Some(417), 1),
        (Some(420), 1),
        (Some(422), 1),
        (Some(424), 1),
        (Some(427), 1),
        (Some(428), 1),
        (Some(429), 2),
        (Some(430), 46),
    ];

    const SAMPLE_RATE: f64 = 22_050.0;

    /// A tone of `count` samples with the given partial amplitudes.
    fn harmonic(hz: f64, count: usize, partials: &[f64]) -> Vec<f64> {
        (0..count)
            .map(|n| {
                let t = cast::usize_to_f64(n) / SAMPLE_RATE;
                let sum: f64 = partials
                    .iter()
                    .enumerate()
                    .map(|(h, a)| {
                        a * (std::f64::consts::TAU * cast::usize_to_f64(h + 1) * hz * t).sin()
                    })
                    .sum();
                sum * 0.4
            })
            .collect()
    }

    /// The frequency of a pitch bin centre.
    fn bin_hz(bin: f64) -> f64 {
        25.0 * (bin / 120.0).exp2()
    }

    /// Three notes with digital silence between them, over noise at 3 % of
    /// the peak.
    fn gaps_and_noise() -> Vec<f64> {
        let mut out = Vec::new();
        for bin in [260.0, 330.0, 380.0] {
            out.extend(harmonic(bin_hz(bin), 8820, &[1.0, 0.5, 0.33, 0.25]));
            out.extend(vec![0.0; 3300]);
        }
        let mut noise = Stream(3);
        for x in &mut out {
            *x += 0.02 * (noise.next() * 2.0 - 1.0);
        }
        out
    }

    /// Eight notes of 0.15 s that jump by up to 180 bins (1.5 octaves).
    fn jumps() -> Vec<f64> {
        [260.0, 380.0, 300.0, 440.0, 260.0, 440.0, 380.0, 300.0]
            .into_iter()
            .flat_map(|bin| harmonic(bin_hz(bin), 3300, &[1.0, 0.5, 0.33]))
            .collect()
    }

    /// 0.2 s of silence, then 0.3 s on bin 310, a rise of 120 bins in 0.3 s
    /// and 0.3 s on bin 430, with all the harmonics under the Nyquist rate.
    fn glide() -> Vec<f64> {
        let (hold, ramp) = (6615, 6615);
        let start = bin_hz(310.0);
        let harmonics: Vec<f64> = (1..=64)
            .map(cast::usize_to_f64)
            .take_while(|h| h * start * 2.0 < SAMPLE_RATE / 2.0)
            .collect();
        let mut out = vec![0.0; 4410];
        let mut phase = 0.0;
        for i in 0..2 * hold + ramp {
            let step = if i < hold {
                0.0
            } else if i < hold + ramp {
                cast::usize_to_f64(i - hold) / cast::usize_to_f64(ramp)
            } else {
                1.0
            };
            let x: f64 = harmonics
                .iter()
                .map(|h| (std::f64::consts::TAU * h * phase).sin() / h)
                .sum();
            out.push(0.5 * x);
            phase += start * step.exp2() / SAMPLE_RATE;
        }
        out
    }

    /// The runs of the decoded bin of a stem.
    fn runs(samples: &[f64]) -> Vec<(Option<usize>, usize)> {
        let frames = Pyin::new(settings()).unwrap().run(samples).unwrap();
        let mut runs: Vec<(Option<usize>, usize)> = Vec::new();
        for f0 in frames {
            let bin = f0.map(|hz| cast::round_to_usize(120.0 * (hz / 25.0).log2()).unwrap());
            match runs.last_mut() {
                Some((last, count)) if *last == bin => *count += 1,
                _ => runs.push((bin, 1)),
            }
        }
        runs
    }

    #[test]
    fn the_decoded_pitch_path_is_the_one_pyin_1_2_0_gives() {
        assert_eq!(runs(&gaps_and_noise()), GAPS_AND_NOISE);
        assert_eq!(runs(&jumps()), JUMPS);
        assert_eq!(runs(&glide()), GLIDE);
    }

    #[test]
    fn too_many_pitch_bins_fail_before_the_moves_are_built() {
        // 1000 bins per semitone make 88 708 bins, whose moves would take
        // 63 GB, and the search stores its states in 16 bits.
        let too_fine = Settings {
            bins_per_semitone: 1000,
            ..settings()
        };
        assert_eq!(
            Pyin::new(too_fine).err(),
            Some(PyinError("too many pitch bins for the search"))
        );
        let absurd = Settings {
            bins_per_semitone: usize::MAX,
            ..settings()
        };
        assert!(Pyin::new(absurd).is_err());
        assert!(states_fit(32_768));
        assert!(!states_fit(32_769));
        assert!(!states_fit(usize::MAX));
    }
}
