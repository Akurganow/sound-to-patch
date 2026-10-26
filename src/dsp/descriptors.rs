//! Global spectral and level descriptors of a stem, each with one named
//! convention.
//!
//! Shape descriptors use magnitude weights, band shares use power. The
//! published tools disagree on weights, floors and rolloff arguments, and
//! the conventions differ by up to 15x on the same sound, so a value is
//! comparable only together with its convention.

use crate::audio::SampleRate;
use crate::cast;
use crate::confidence::{ConfidenceError, Estimate, Level, Reason, ReasonCode, Signal, Spread};
use crate::units::{Decibels, Hz, Normalized, Ratio};

use super::frames::{ActiveFrames, Stats, Summary};
use super::{Stft, StftParams, Window};

/// The convention of the shape descriptors.
pub const SHAPE_CONVENTION: &str = "magnitude weights |X_k| divided by the window sum, \
periodic Hann frames, statistics over active frames as median and interquartile range";
/// The convention of the band shares.
pub const BAND_CONVENTION: &str =
    "power |X_k|^2 summed over active frames per band, divided by the power in all bands";
/// The convention of the flatness.
pub const FLATNESS_CONVENTION: &str = "geometric over arithmetic mean of the magnitude, \
floored at 1e-5 after the window-sum scale, bins from 100 Hz to 10 kHz";
/// The convention of the levels.
pub const LEVEL_CONVENTION: &str = "peak and RMS of the mono mix over the whole stem in dB \
relative to full scale 1.0; a full-scale sine reads -3.01 dB RMS; crest is peak minus RMS";

/// The band edges of the band shares in hertz. The edges are a project
/// choice.
pub const BAND_EDGES_HZ: [Hz; 7] = [
    Hz::from_const(20.0),
    Hz::from_const(60.0),
    Hz::from_const(250.0),
    Hz::from_const(1000.0),
    Hz::from_const(4000.0),
    Hz::from_const(10_000.0),
    Hz::from_const(20_000.0),
];

/// The flatness floor, applied after the window-sum scale.
pub const FLATNESS_FLOOR: Normalized = Normalized::from_const(1e-5);

const FLATNESS_BAND_HZ: (f64, f64) = (100.0, 10_000.0);

/// The descriptors of one frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameDescriptors {
    /// `sum f_k p_k` with `p_k = a_k / sum a_k`.
    pub centroid: Hz,
    /// `sqrt(sum (f_k - centroid)^2 p_k)`.
    pub spread: Hz,
    /// The first bin whose cumulative magnitude reaches 85 % of the total.
    pub rolloff85: Hz,
    /// The same at 95 %.
    pub rolloff95: Hz,
    /// Geometric over arithmetic mean of the floored magnitude in 100 Hz to
    /// 10 kHz.
    pub flatness: Normalized,
}

/// Computes the descriptors of one magnitude frame, already divided by the
/// window sum, with bins `bin_hz` apart. Returns `None` for an all-zero
/// frame.
#[must_use]
pub fn frame_descriptors(magnitudes: &[f64], bin_hz: Hz) -> Option<FrameDescriptors> {
    let total: f64 = magnitudes.iter().sum();
    if total <= 0.0 {
        return None;
    }
    let width = bin_hz.get();
    let frequency = |k: usize| cast::usize_to_f64(k) * width;
    let centroid = magnitudes
        .iter()
        .enumerate()
        .map(|(k, a)| frequency(k) * a)
        .sum::<f64>()
        / total;
    let spread = (magnitudes
        .iter()
        .enumerate()
        .map(|(k, a)| (frequency(k) - centroid).powi(2) * a)
        .sum::<f64>()
        / total)
        .sqrt();
    let rolloff = |share: f64| {
        let mut cumulative = 0.0;
        let bin = magnitudes
            .iter()
            .position(|a| {
                cumulative += a;
                cumulative >= share * total
            })
            .unwrap_or(magnitudes.len() - 1);
        Hz::clamped(frequency(bin))
    };
    let band: Vec<f64> = magnitudes
        .iter()
        .enumerate()
        .filter(|&(k, _)| (FLATNESS_BAND_HZ.0..=FLATNESS_BAND_HZ.1).contains(&frequency(k)))
        .map(|(_, a)| a.max(FLATNESS_FLOOR.get()))
        .collect();
    let flatness = if band.is_empty() {
        1.0
    } else {
        let count = cast::usize_to_f64(band.len());
        let log_mean = band.iter().map(|a| a.ln()).sum::<f64>() / count;
        let mean = band.iter().sum::<f64>() / count;
        log_mean.exp() / mean
    };
    Some(FrameDescriptors {
        centroid: Hz::clamped(centroid),
        spread: Hz::clamped(spread),
        rolloff85: rolloff(0.85),
        rolloff95: rolloff(0.95),
        flatness: Normalized::clamped(flatness),
    })
}

/// The share of power in one band.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BandShare {
    /// The lower edge, inclusive.
    pub lo: Hz,
    /// The upper edge, exclusive.
    pub hi: Hz,
    /// The share of the power in all bands.
    pub share: Normalized,
}

/// The descriptors of a stem.
#[derive(Clone, Debug, PartialEq)]
pub struct Descriptors {
    /// The frame length `N` of the analysis.
    pub frame_len: usize,
    /// The number of active frames.
    pub active_frames: usize,
    /// The spectral centroid, magnitude weights.
    pub centroid: Estimate<Stats<Hz>, Hz>,
    /// The centroid over the median f0 of the note list.
    pub centroid_over_f0: Estimate<Stats<Ratio>, Ratio>,
    /// The spectral spread around the centroid, magnitude weights.
    pub spread: Estimate<Stats<Hz>, Hz>,
    /// The 85 % magnitude rolloff.
    pub rolloff85: Estimate<Stats<Hz>, Hz>,
    /// The 95 % magnitude rolloff.
    pub rolloff95: Estimate<Stats<Hz>, Hz>,
    /// The spectral flatness, magnitude, 100 Hz to 10 kHz.
    pub flatness: Estimate<Stats<Normalized>, Normalized>,
    /// The power shares of the bands between [`BAND_EDGES_HZ`].
    pub band_share: Estimate<Vec<BandShare>, Normalized>,
    /// The peak level of the mono mix in dB relative to full scale.
    pub peak_dbfs: Estimate<Decibels, Decibels>,
    /// The RMS level of the mono mix in dB relative to full scale.
    pub rms_dbfs: Estimate<Decibels, Decibels>,
    /// Peak over RMS in dB.
    pub crest_db: Estimate<Decibels, Decibels>,
}

/// Computes the descriptors of a mono signal.
///
/// Frames are periodic Hann of `params.frame_len()` samples with hop
/// `frame_len / 4`. `f0` is the median fundamental of the note list, if one
/// was given. `hard_limited` is the flat-top flag of the stem; the levels
/// carry it as a reason.
///
/// # Errors
///
/// Returns a [`ConfidenceError`] only if a reason cannot be built, which
/// the counts here never cause.
pub fn describe(
    samples: &[f64],
    rate: SampleRate,
    params: StftParams,
    f0: Option<Hz>,
    hard_limited: bool,
) -> Result<Descriptors, ConfidenceError> {
    let params = params.with_window(Window::Hann);
    let active = ActiveFrames::of(samples, params);
    let count = u64::try_from(active.count()).unwrap_or(u64::MAX);
    let evidence = Evidence {
        frames: Reason::new(ReasonCode::ActiveFrames, Signal::Count(count))?,
        silent: Reason::new(ReasonCode::SilentStem, Signal::Flag(true))?,
    };
    let (per_frame, band_power) = frame_values(samples, rate, params, &active);
    let pick = |f: fn(&FrameDescriptors) -> f64| per_frame.iter().map(f).collect::<Vec<_>>();
    let ratios = f0.filter(|f| f.get() > 0.0).map(|f0| {
        pick(|d| d.centroid.get())
            .iter()
            .map(|c| c / f0.get())
            .collect::<Vec<_>>()
    });
    let centroid_over_f0 = match ratios {
        Some(ratios) => evidence.stats(&ratios, Level::Medium, Ratio::clamped)?,
        None => Estimate::none(vec![Reason::new(
            ReasonCode::NoNoteList,
            Signal::Flag(true),
        )?])?,
    };
    let peak = samples.iter().fold(0.0_f64, |max, x| max.max(x.abs()));
    let rms = if samples.is_empty() {
        0.0
    } else {
        (samples.iter().map(|x| x * x).sum::<f64>() / cast::usize_to_f64(samples.len())).sqrt()
    };
    let peak_db = Decibels::from_amplitude_gain(peak).ok();
    let rms_db = Decibels::from_amplitude_gain(rms).ok();
    let crest = peak_db
        .zip(rms_db)
        .map(|(p, r)| Decibels::clamped(p.get() - r.get()));
    Ok(Descriptors {
        frame_len: params.frame_len(),
        active_frames: active.count(),
        centroid: evidence.stats(&pick(|d| d.centroid.get()), Level::High, Hz::clamped)?,
        centroid_over_f0,
        spread: evidence.stats(&pick(|d| d.spread.get()), Level::Medium, Hz::clamped)?,
        rolloff85: evidence.stats(&pick(|d| d.rolloff85.get()), Level::Medium, Hz::clamped)?,
        rolloff95: evidence.stats(&pick(|d| d.rolloff95.get()), Level::Medium, Hz::clamped)?,
        flatness: evidence.stats(
            &pick(|d| d.flatness.get()),
            Level::Medium,
            Normalized::clamped,
        )?,
        band_share: evidence.band_share(&band_power)?,
        peak_dbfs: evidence.level(peak_db, hard_limited)?,
        rms_dbfs: evidence.level(rms_db, hard_limited)?,
        crest_db: evidence.level(crest, hard_limited)?,
    })
}

/// The descriptors of each active frame and the power per band summed over
/// them.
fn frame_values(
    samples: &[f64],
    rate: SampleRate,
    params: StftParams,
    active: &ActiveFrames,
) -> (Vec<FrameDescriptors>, Vec<f64>) {
    let window_sum: f64 = Window::Hann.samples(params.frame_len()).iter().sum();
    let bin_hz = params.bin_width(rate);
    let mut stft = Stft::new(params);
    let mut per_frame = Vec::with_capacity(active.count());
    let mut band_power = vec![0.0; BAND_EDGES_HZ.len() - 1];
    for frame in active.indices() {
        let magnitudes: Vec<f64> = stft
            .frame(samples, frame)
            .iter()
            .map(|value| value.norm() / window_sum)
            .collect();
        for (k, a) in magnitudes.iter().enumerate() {
            let f = cast::usize_to_f64(k) * bin_hz.get();
            if let Some(band) = BAND_EDGES_HZ
                .windows(2)
                .position(|edge| (edge[0].get()..edge[1].get()).contains(&f))
            {
                band_power[band] += a * a;
            }
        }
        if let Some(descriptors) = frame_descriptors(&magnitudes, bin_hz) {
            per_frame.push(descriptors);
        }
    }
    (per_frame, band_power)
}

/// The reasons every descriptor cites.
struct Evidence {
    frames: Reason,
    silent: Reason,
}

impl Evidence {
    /// The median and IQR of per-frame values, the IQR as the spread; no
    /// estimate without a frame.
    fn stats<T: Spread>(
        &self,
        values: &[f64],
        level: Level,
        unit: fn(f64) -> T,
    ) -> Result<Estimate<Stats<T>, T>, ConfidenceError> {
        match Summary::of(values) {
            Some(summary) => Estimate::new(
                summary.to_stats(unit),
                level,
                Some(unit(summary.iqr())),
                vec![self.frames],
            ),
            None => Estimate::none(vec![self.silent, self.frames]),
        }
    }

    fn band_share(
        &self,
        band_power: &[f64],
    ) -> Result<Estimate<Vec<BandShare>, Normalized>, ConfidenceError> {
        let total: f64 = band_power.iter().sum();
        if total <= 0.0 {
            return Estimate::none(vec![self.silent, self.frames]);
        }
        let shares = BAND_EDGES_HZ
            .windows(2)
            .zip(band_power)
            .map(|(edge, power)| BandShare {
                lo: edge[0],
                hi: edge[1],
                share: Normalized::clamped(power / total),
            })
            .collect();
        Estimate::new(shares, Level::High, None, vec![self.frames])
    }

    fn level(
        &self,
        value: Option<Decibels>,
        hard_limited: bool,
    ) -> Result<Estimate<Decibels, Decibels>, ConfidenceError> {
        match value {
            Some(db) => Estimate::new(
                db,
                Level::High,
                None,
                vec![Reason::new(
                    ReasonCode::HardLimited,
                    Signal::Flag(hard_limited),
                )?],
            ),
            None => Estimate::none(vec![self.silent]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_constants_pass_the_unit_check() {
        let edges =
            [20.0, 60.0, 250.0, 1000.0, 4000.0, 10_000.0, 20_000.0].map(|v| Hz::new(v).unwrap());
        assert_eq!(BAND_EDGES_HZ, edges);
        assert_eq!(FLATNESS_FLOOR, Normalized::new(1e-5).unwrap());
    }

    #[test]
    fn a_frame_with_a_zero_bin_has_a_finite_flatness() {
        let mut magnitudes = vec![0.01; 1025];
        magnitudes[300] = 0.0;
        let d = frame_descriptors(&magnitudes, Hz::new(21.533).unwrap()).unwrap();
        assert!(d.flatness.get() > 0.0 && d.flatness.get().is_finite());
    }

    #[test]
    fn a_single_bin_puts_centroid_and_rolloffs_on_it() {
        let mut magnitudes = vec![0.0; 100];
        magnitudes[10] = 1.0;
        let d = frame_descriptors(&magnitudes, Hz::new(10.0).unwrap()).unwrap();
        assert_eq!(d.centroid.get(), 100.0);
        assert_eq!(d.spread.get(), 0.0);
        assert_eq!((d.rolloff85.get(), d.rolloff95.get()), (100.0, 100.0));
        assert!(frame_descriptors(&[0.0; 4], Hz::new(10.0).unwrap()).is_none());
    }

    #[test]
    fn silence_has_no_estimate_with_a_reason() {
        let rate = SampleRate::new(44_100).unwrap();
        let d = describe(
            &vec![0.0; 44_100],
            rate,
            StftParams::new(4096).unwrap(),
            None,
            false,
        )
        .unwrap();
        assert_eq!(d.active_frames, 0);
        assert!(d.centroid.value().is_none());
        assert!(d.peak_dbfs.value().is_none());
        assert!(d.crest_db.value().is_none());
        assert_eq!(
            d.centroid_over_f0.confidence().reasons()[0].code(),
            ReasonCode::NoNoteList
        );
    }
}
