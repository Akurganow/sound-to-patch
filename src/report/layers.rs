//! The `layers.json` report: the `stem` group and the `pitch` layer group,
//! with a convention for every number.

use serde::Serialize;

use crate::audio::{
    BAND_EDGE_CONVENTION, Band, HARD_LIMITED_CONVENTION, STEREO_CONVENTION, Stem, StereoScreen,
    TEMPO_CONVENTION, Tempo,
};
use crate::confidence::{Confidence, Estimate, NoSpread, Spread};
use crate::dsp::{
    BAND_CONVENTION, Descriptors, FLATNESS_CONVENTION, LEVEL_CONVENTION, SHAPE_CONVENTION, Stats,
};
use crate::estimate::pitch::{
    DROP_CONVENTION, DropFit, GLIDE_CONVENTION, GlideFit, LINE_CONVENTION, PitchFlags, PitchLayer,
    Rated, VIBRATO_CONVENTION, VibratoFit,
};
use crate::notes::TranscriptionRecord;
use crate::synth::Synth;
use crate::units::{
    Bpm, Cents, Correlation, Decibels, Degrees, Hz, Normalized, Ratio, Seconds, Semitones,
};

/// The schema name and version that `layers.json` declares.
pub const SCHEMA: &str = "layers/1";

/// The convention of the statistics over active frames.
const STATISTICS_CONVENTION: &str = "median and interquartile range over active frames, \
quantiles by linear interpolation at q (n - 1) between the sorted values; a frame is active \
when its RMS lies within 40 dB of the loudest frame";

/// Renders the report of a stem as pretty-printed JSON with a final
/// newline. The same stem always gives the same bytes.
///
/// `source` is the stem's file name without its folder, `tempo` the
/// given and refined tempo, `transcription` the record of the
/// transcriber that gave the notes, or `None` when none ran, and `pitch`
/// the pitch layer.
///
/// # Errors
///
/// Returns a [`serde_json::Error`] only if a value cannot be written, which
/// the finite unit types never cause.
pub fn layers_json(
    stem: &Stem,
    descriptors: &Descriptors,
    synth: Synth,
    source: &str,
    tempo: &Tempo,
    transcription: Option<&TranscriptionRecord>,
    pitch: &PitchLayer,
) -> Result<String, serde_json::Error> {
    let report = Layers {
        schema: SCHEMA,
        source,
        synth,
        stem: StemGroup::new(stem, descriptors, tempo),
        transcription,
        pitch: PitchGroup::new(pitch),
        conventions: Conventions::new(descriptors.frame_len),
    };
    let mut text = serde_json::to_string_pretty(&report)?;
    text.push('\n');
    Ok(text)
}

#[derive(Serialize)]
struct Layers<'a> {
    schema: &'static str,
    /// The file name of the stem, without its folder.
    source: &'a str,
    synth: Synth,
    stem: StemGroup<'a>,
    /// The transcriber's model and settings, `null` when none ran.
    transcription: Option<&'a TranscriptionRecord>,
    pitch: PitchGroup<'a>,
    conventions: Conventions,
}

/// The pitch layer as a layer group. The mapping step fills
/// `suggested_edits` in a later release; until then the list is empty.
#[derive(Serialize)]
struct PitchGroup<'a> {
    values: Option<PitchValues<'a>>,
    confidence: &'a Confidence<NoSpread>,
    flags: PitchFlags,
    needs_render: bool,
    suggested_edits: [(); 0],
}

impl<'a> PitchGroup<'a> {
    fn new(layer: &'a PitchLayer) -> Self {
        Self {
            values: layer.confidence.is_estimate().then(|| PitchValues {
                glides: layer.glides.iter().map(GlideRecord::of).collect(),
                drops: layer.drops.iter().map(DropRecord::of).collect(),
                vibrato: layer.vibrato.iter().map(VibratoRecord::of).collect(),
            }),
            confidence: &layer.confidence,
            flags: layer.flags,
            needs_render: false,
            suggested_edits: [],
        }
    }
}

#[derive(Serialize)]
struct PitchValues<'a> {
    glides: Vec<GlideRecord<'a>>,
    drops: Vec<DropRecord<'a>>,
    vibrato: Vec<VibratoRecord<'a>>,
}

#[derive(Serialize)]
struct GlideRecord<'a> {
    start_s: Seconds,
    interval_st: Semitones,
    time_s: Seconds,
    curve: f64,
    from_silence: bool,
    confidence: &'a Confidence<NoSpread>,
}

impl<'a> GlideRecord<'a> {
    fn of(glide: &'a Rated<GlideFit>) -> Self {
        let g = &glide.value;
        Self {
            start_s: g.start,
            interval_st: g.interval,
            time_s: g.time,
            curve: g.curve,
            from_silence: g.from_silence,
            confidence: &glide.confidence,
        }
    }
}

#[derive(Serialize)]
struct DropRecord<'a> {
    onset_s: Seconds,
    f_end_hz: Hz,
    df_st: Option<Semitones>,
    df_hz: f64,
    tau_s: Seconds,
    residual: f64,
    confidence: &'a Confidence<NoSpread>,
}

impl<'a> DropRecord<'a> {
    fn of(drop: &'a Rated<DropFit>) -> Self {
        let d = &drop.value;
        Self {
            onset_s: d.onset,
            f_end_hz: d.f_end,
            df_st: d.df(),
            df_hz: d.df_hz,
            tau_s: d.tau,
            residual: d.residual,
            confidence: &drop.confidence,
        }
    }
}

#[derive(Serialize)]
struct VibratoRecord<'a> {
    start_s: Seconds,
    end_s: Seconds,
    rate_hz: Hz,
    extent_ct: Cents,
    extent_raw_ct: Cents,
    extent_factor: f64,
    share: f64,
    confidence: &'a Confidence<NoSpread>,
}

impl<'a> VibratoRecord<'a> {
    fn of(vibrato: &'a Rated<VibratoFit>) -> Self {
        let v = &vibrato.value;
        Self {
            start_s: v.start,
            end_s: v.end,
            rate_hz: v.rate,
            extent_ct: v.extent(),
            extent_raw_ct: v.extent_raw,
            extent_factor: v.factor,
            share: v.share,
            confidence: &vibrato.confidence,
        }
    }
}

#[derive(Serialize)]
struct StemGroup<'a> {
    bpm: Bpm,
    bpm_given: Bpm,
    bpm_refined: Value<'a, Bpm, NoSpread>,
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
    fn new(stem: &'a Stem, descriptors: &'a Descriptors, tempo: &'a Tempo) -> Self {
        Self {
            bpm: stem.grid().bpm(),
            bpm_given: tempo.given(),
            bpm_refined: Value::of(tempo.refined()),
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
#[serde(bound = "V: Serialize")]
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
    bpm_given: &'static str,
    bpm_refined: &'static str,
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
    pitch: &'static str,
    glides: &'static str,
    drops: &'static str,
    sub_band_line: &'static str,
    vibrato: &'static str,
}

impl Conventions {
    fn new(frame_size: usize) -> Self {
        Self {
            frame_size,
            statistics: STATISTICS_CONVENTION,
            spectral: "magnitude weights for shape, power for band shares; periodic Hann frames \
of frame_size samples, hop frame_size / 4, centred and zero-padded; frame_size is 8192 when the \
note list has a median f0 below 100 Hz, else 4096, and 4096 without a note list",
            bpm: "the tempo of the beat grid and of every later step: bpm_refined when its \
confidence is high, else bpm_given",
            bpm_given: "given by --bpm",
            bpm_refined: TEMPO_CONVENTION,
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
            pitch: "the pitch layer of a monophonic line, read from the f0 track, the note list \
and the sub-band line; values is null when the line plays chords or has no notes; every time is \
audio time from the start of the stem",
            glides: GLIDE_CONVENTION,
            drops: DROP_CONVENTION,
            sub_band_line: LINE_CONVENTION,
            vibrato: VIBRATO_CONVENTION,
        }
    }
}
