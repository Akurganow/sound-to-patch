//! The `layers.json` report: the `stem` group with a convention for every
//! number.

use serde::Serialize;

use crate::audio::{
    BAND_EDGE_CONVENTION, Band, HARD_LIMITED_CONVENTION, STEREO_CONVENTION, Stem, StereoScreen,
};
use crate::confidence::{Confidence, Estimate, NoSpread, Spread};
use crate::dsp::{
    BAND_CONVENTION, Descriptors, FLATNESS_CONVENTION, LEVEL_CONVENTION, SHAPE_CONVENTION, Stats,
};
use crate::synth::Synth;
use crate::units::{Bpm, Correlation, Decibels, Degrees, Hz, Normalized, Ratio, Seconds};

/// The schema name and version that `layers.json` declares.
pub const SCHEMA: &str = "layers/1";

/// The convention of the statistics over active frames.
const STATISTICS_CONVENTION: &str = "median and interquartile range over active frames, \
quantiles by linear interpolation at q (n - 1) between the sorted values; a frame is active \
when its RMS lies within 40 dB of the loudest frame";

/// Renders the report of a stem as pretty-printed JSON with a final
/// newline. The same stem always gives the same bytes.
///
/// # Errors
///
/// Returns a [`serde_json::Error`] only if a value cannot be written, which
/// the finite unit types never cause.
pub fn layers_json(
    stem: &Stem,
    descriptors: &Descriptors,
    synth: Synth,
) -> Result<String, serde_json::Error> {
    let report = Layers {
        schema: SCHEMA,
        synth,
        stem: StemGroup::new(stem, descriptors),
        transcription: (),
        conventions: Conventions::new(descriptors.frame_len),
    };
    let mut text = serde_json::to_string_pretty(&report)?;
    text.push('\n');
    Ok(text)
}

#[derive(Serialize)]
struct Layers<'a> {
    schema: &'static str,
    synth: Synth,
    stem: StemGroup<'a>,
    /// The transcriber's model and settings. No transcriber runs in this
    /// release, and with `--notes` none runs at all, so it is `null`.
    transcription: (),
    conventions: Conventions,
}

#[derive(Serialize)]
struct StemGroup<'a> {
    bpm: Bpm,
    duration_s: Seconds,
    sample_rate_hz: u32,
    band_edge_hz: Value<'a, Hz, Hz>,
    hard_limited: bool,
    peak_dbfs: Value<'a, Decibels, Decibels>,
    rms_dbfs: Value<'a, Decibels, Decibels>,
    crest_db: Value<'a, Decibels, Decibels>,
    centroid_hz: Statistic<'a, Hz>,
    /// `None` without a note list.
    centroid_over_f0: Option<Statistic<'a, Ratio>>,
    spread_hz: Statistic<'a, Hz>,
    rolloff85_hz: Statistic<'a, Hz>,
    rolloff95_hz: Statistic<'a, Hz>,
    flatness: Statistic<'a, Normalized>,
    band_share: BandShares<'a>,
    stereo: Stereo<'a>,
}

impl<'a> StemGroup<'a> {
    fn new(stem: &'a Stem, descriptors: &'a Descriptors) -> Self {
        Self {
            bpm: stem.grid().bpm(),
            duration_s: stem.duration(),
            sample_rate_hz: stem.sample_rate().get(),
            band_edge_hz: Value::of(stem.band_edge()),
            hard_limited: stem.hard_limited(),
            peak_dbfs: Value::of(&descriptors.peak_dbfs),
            rms_dbfs: Value::of(&descriptors.rms_dbfs),
            crest_db: Value::of(&descriptors.crest_db),
            centroid_hz: Statistic::of(&descriptors.centroid),
            centroid_over_f0: stem
                .median_f0()
                .map(|_| Statistic::of(&descriptors.centroid_over_f0)),
            spread_hz: Statistic::of(&descriptors.spread),
            rolloff85_hz: Statistic::of(&descriptors.rolloff85),
            rolloff95_hz: Statistic::of(&descriptors.rolloff95),
            flatness: Statistic::of(&descriptors.flatness),
            band_share: BandShares {
                value: descriptors.band_share.value().map(|shares| {
                    shares
                        .iter()
                        .map(|share| BandPart {
                            lo_hz: share.lo,
                            hi_hz: share.hi,
                            share: share.share,
                        })
                        .collect()
                }),
                confidence: descriptors.band_share.confidence(),
            },
            stereo: Stereo::of(stem.stereo()),
        }
    }
}

/// One value and its confidence; `value` is `null` for no estimate.
#[derive(Serialize)]
struct Value<'a, V, S: Spread> {
    value: Option<&'a V>,
    confidence: &'a Confidence<S>,
}

impl<'a, V, S: Spread> Value<'a, V, S> {
    fn of(estimate: &'a Estimate<V, S>) -> Self {
        Self {
            value: estimate.value(),
            confidence: estimate.confidence(),
        }
    }
}

/// The median and IQR over active frames and their confidence; both are
/// `null` for no estimate.
#[derive(Serialize)]
struct Statistic<'a, T: Spread> {
    median: Option<T>,
    iqr: Option<T>,
    confidence: &'a Confidence<T>,
}

impl<'a, T: Spread> Statistic<'a, T> {
    fn of(estimate: &'a Estimate<Stats<T>, T>) -> Self {
        Self {
            median: estimate.value().map(Stats::median),
            iqr: estimate.value().map(Stats::iqr),
            confidence: estimate.confidence(),
        }
    }
}

#[derive(Serialize)]
struct BandShares<'a> {
    value: Option<Vec<BandPart>>,
    confidence: &'a Confidence<Normalized>,
}

#[derive(Serialize)]
struct BandPart {
    lo_hz: Hz,
    hi_hz: Hz,
    share: Normalized,
}

#[derive(Serialize)]
struct Stereo<'a> {
    full: StereoBand,
    sub: StereoBand,
    low_mid: StereoBand,
    high: StereoBand,
    confidence: &'a Confidence<NoSpread>,
}

impl<'a> Stereo<'a> {
    fn of(screen: &'a StereoScreen) -> Self {
        let band = |band| {
            let stats = screen.band(band);
            StereoBand {
                rho: stats.rho,
                sm_db: stats.sm_db,
                ipd_deg: stats.ipd_deg,
            }
        };
        Self {
            full: band(Band::Full),
            sub: band(Band::Sub),
            low_mid: band(Band::LowMid),
            high: band(Band::High),
            confidence: screen.confidence(),
        }
    }
}

#[derive(Serialize)]
struct StereoBand {
    rho: Option<Correlation>,
    sm_db: Option<Decibels>,
    ipd_deg: Option<Degrees>,
}

/// The convention of every number in the `stem` group, keyed by its field.
#[derive(Serialize)]
struct Conventions {
    frame_size: usize,
    statistics: &'static str,
    spectral: &'static str,
    bpm: &'static str,
    duration_s: &'static str,
    sample_rate_hz: &'static str,
    band_edge_hz: &'static str,
    hard_limited: &'static str,
    peak_dbfs: &'static str,
    rms_dbfs: &'static str,
    crest_db: &'static str,
    centroid_hz: &'static str,
    centroid_over_f0: &'static str,
    spread_hz: &'static str,
    rolloff85_hz: &'static str,
    rolloff95_hz: &'static str,
    flatness: &'static str,
    band_share: &'static str,
    stereo: &'static str,
    confidence: &'static str,
}

impl Conventions {
    fn new(frame_size: usize) -> Self {
        Self {
            frame_size,
            statistics: STATISTICS_CONVENTION,
            spectral: "magnitude weights for shape, power for band shares; periodic Hann frames \
of frame_size samples, hop frame_size / 4, centred and zero-padded; frame_size is 8192 when the \
note list has a median f0 below 100 Hz, else 4096, and 4096 without a note list",
            bpm: "given by --bpm, not estimated",
            duration_s: "decoded frames over the sample rate",
            sample_rate_hz: "the native rate of the file; no resampling",
            band_edge_hz: BAND_EDGE_CONVENTION,
            hard_limited: HARD_LIMITED_CONVENTION,
            peak_dbfs: LEVEL_CONVENTION,
            rms_dbfs: LEVEL_CONVENTION,
            crest_db: LEVEL_CONVENTION,
            centroid_hz: SHAPE_CONVENTION,
            centroid_over_f0: "the centroid of each active frame over the median f0 of the note \
list, MIDI pitch to hertz with A4 = 440 Hz; null without a note list",
            spread_hz: SHAPE_CONVENTION,
            rolloff85_hz: SHAPE_CONVENTION,
            rolloff95_hz: SHAPE_CONVENTION,
            flatness: FLATNESS_CONVENTION,
            band_share: BAND_CONVENTION,
            stereo: STEREO_CONVENTION,
            confidence: "level, reasons and spread of each estimate; the spread of a statistic \
is its interquartile range and that of the band edge one bin width, in the unit of the value; \
estimate false means no value, with the reasons why",
        }
    }
}
