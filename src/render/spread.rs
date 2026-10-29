//! The repeat-render spread: the noise floor that every later comparison
//! cites.
//!
//! Renders of one preset differ a little, because a synth starts its
//! voices at random phases. One preset rendered [`SPREAD_RUNS`] times with
//! one recipe shows how far two renders of the same sound lie apart. For
//! each output the spread is the 95th percentile of the absolute
//! differences over every pair of renders, 190 pairs for 20 renders, in
//! the output's own unit. For a distance between two clips, such as the
//! envelope distance, it is the same statistic over the distances of the
//! pairs.
//!
//! A comparison of a clip with renders counts as a match when its
//! difference lies within the spread. The spread file is a local output
//! that consumers read; the repository holds the method and no measured
//! numbers.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{HostIdentity, Oversampling};
use crate::audio::Audio;
use crate::cast;
use crate::dsp::quantile;
use crate::units::Bpm;

/// How many renders a spread takes.
pub const SPREAD_RUNS: usize = 20;
/// The quantile of the pairwise differences.
pub const SPREAD_QUANTILE: f64 = 0.95;
/// The name of the statistic, as the spread file records it.
pub const STATISTIC: &str = "p95_abs_pairwise";
/// A clip starts at its first sample within this many dB of its peak, so
/// a lead-in of silence or latency does not count.
pub const ONSET_RANGE_DB: f64 = 40.0;
/// A clip ends at its last sample above this level, in dBFS: the tail
/// floor of the recipe.
pub const END_FLOOR_DB: f64 = -96.0;
/// The envelope frames per second: one frame is 10 ms.
pub const ENVELOPE_FRAMES_PER_S: u32 = 100;
/// The envelope distance counts the frames within this many dB of the
/// loudest frame of either clip.
pub const ENVELOPE_RANGE_DB: f64 = 40.0;

/// The level of a silent frame, in dB: far below any level that counts.
const SILENT_DB: f64 = -400.0;

/// What a comparison measures of one clip.
///
/// The clip runs from its onset, the first sample within [`ONSET_RANGE_DB`]
/// of the peak, to its end, the last sample above [`END_FLOOR_DB`]. Both
/// channels count.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ClipMetrics {
    /// The RMS level from onset to end, in dBFS.
    pub rms_db: f64,
    /// The largest sample magnitude, in dBFS.
    pub peak_db: f64,
    /// From onset to end, in s.
    pub duration_s: f64,
    /// The RMS level of each 10 ms frame from the onset to the end, in
    /// dBFS.
    pub envelope_db: Vec<f64>,
}

impl ClipMetrics {
    /// Measures `audio`. Returns `None` for a clip whose every sample lies
    /// at or below [`END_FLOOR_DB`].
    #[must_use]
    pub fn of(audio: &Audio) -> Option<Self> {
        let channels = audio.channels();
        let magnitude = |frame: usize| {
            channels
                .iter()
                .map(|channel| channel[frame].abs())
                .fold(0.0, f64::max)
        };
        let frames = audio.frames();
        let peak = (0..frames).map(magnitude).fold(0.0, f64::max);
        let floor = gain(END_FLOOR_DB);
        if peak <= floor {
            return None;
        }
        let threshold = peak * gain(-ONSET_RANGE_DB);
        let onset = (0..frames).find(|frame| magnitude(*frame) >= threshold)?;
        let end = (onset..frames)
            .rev()
            .find(|frame| magnitude(*frame) > floor)?
            + 1;
        let power = |range: std::ops::Range<usize>| {
            let count = cast::usize_to_f64(range.len() * channels.len());
            let sum: f64 = channels
                .iter()
                .map(|channel| channel[range.clone()].iter().map(|x| x * x).sum::<f64>())
                .sum();
            sum / count
        };
        let frame = usize::try_from(audio.sample_rate().get() / ENVELOPE_FRAMES_PER_S)
            .unwrap_or(1)
            .max(1);
        let envelope_db = (onset..end)
            .step_by(frame)
            .map(|start| power_db(power(start..(start + frame).min(end))))
            .collect();
        Some(Self {
            rms_db: power_db(power(onset..end)),
            peak_db: 20.0 * peak.log10(),
            duration_s: cast::usize_to_f64(end - onset) / f64::from(audio.sample_rate().get()),
            envelope_db,
        })
    }

    /// The value of a scalar output.
    #[must_use]
    pub fn value(&self, output: ClipOutput) -> f64 {
        match output {
            ClipOutput::RmsDb => self.rms_db,
            ClipOutput::PeakDb => self.peak_db,
            ClipOutput::DurationS => self.duration_s,
        }
    }
}

/// A scalar output of [`ClipMetrics`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipOutput {
    /// [`ClipMetrics::rms_db`].
    RmsDb,
    /// [`ClipMetrics::peak_db`].
    PeakDb,
    /// [`ClipMetrics::duration_s`].
    DurationS,
}

impl ClipOutput {
    /// Every scalar output.
    pub const ALL: [Self; 3] = [Self::RmsDb, Self::PeakDb, Self::DurationS];

    /// The name of the output in a spread file.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::RmsDb => "rms_db",
            Self::PeakDb => "peak_db",
            Self::DurationS => "duration_s",
        }
    }

    /// The unit of the output.
    #[must_use]
    pub fn unit(self) -> &'static str {
        match self {
            Self::RmsDb | Self::PeakDb => "dB",
            Self::DurationS => "s",
        }
    }
}

/// The name of the envelope distance in a spread file.
pub const ENVELOPE_DISTANCE: &str = "envelope_distance_db";

/// The envelope distance of two clips, in dB.
///
/// It is the mean absolute level difference over the frames where either
/// envelope lies within [`ENVELOPE_RANGE_DB`] of the loudest frame of
/// both. A frame below that
/// range, or past the end of the shorter clip, counts at the bottom of the
/// range, so a longer tail adds to the distance. The frames align at the
/// two onsets.
#[must_use]
pub fn envelope_distance(a: &ClipMetrics, b: &ClipMetrics) -> f64 {
    let loudest = a
        .envelope_db
        .iter()
        .chain(&b.envelope_db)
        .fold(SILENT_DB, |x, y| x.max(*y));
    let bottom = loudest - ENVELOPE_RANGE_DB;
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

/// The [`SPREAD_QUANTILE`] of `distance` over every pair of `count` items,
/// linear between the nearest pairs. Returns `None` for fewer than two
/// items.
#[must_use]
pub fn pairwise_quantile(count: usize, distance: impl Fn(usize, usize) -> f64) -> Option<f64> {
    let mut pairs: Vec<f64> = (0..count)
        .flat_map(|i| (i + 1..count).map(move |j| (i, j)))
        .map(|(i, j)| distance(i, j))
        .collect();
    pairs.sort_by(f64::total_cmp);
    quantile(&pairs, SPREAD_QUANTILE)
}

/// The spread of one output.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpreadRow {
    /// The output: a [`ClipOutput`] name, [`ENVELOPE_DISTANCE`], or the
    /// path of a number in the layer report, such as `stem.rms_dbfs`.
    pub output: String,
    /// The unit of the output and of its spread.
    pub unit: String,
    /// The spread.
    pub spread: f64,
}

/// The spread rows of the clip outputs and the envelope distance. Returns
/// no row for fewer than two clips.
#[must_use]
pub fn clip_rows(clips: &[ClipMetrics]) -> Vec<SpreadRow> {
    let scalar = ClipOutput::ALL.into_iter().filter_map(|output| {
        let spread = pairwise_quantile(clips.len(), |i, j| {
            (clips[i].value(output) - clips[j].value(output)).abs()
        })?;
        Some(SpreadRow {
            output: output.name().to_owned(),
            unit: output.unit().to_owned(),
            spread,
        })
    });
    let envelope = pairwise_quantile(clips.len(), |i, j| envelope_distance(&clips[i], &clips[j]))
        .map(|spread| SpreadRow {
            output: ENVELOPE_DISTANCE.to_owned(),
            unit: "dB".to_owned(),
            spread,
        });
    scalar.chain(envelope).collect()
}

/// The spread row of one more output, such as a number of the layer
/// report, from its value in each render. Returns `None` for fewer than
/// two values.
#[must_use]
pub fn value_row(output: &str, unit: &str, values: &[f64]) -> Option<SpreadRow> {
    let spread = pairwise_quantile(values.len(), |i, j| (values[i] - values[j]).abs())?;
    Some(SpreadRow {
        output: output.to_owned(),
        unit: unit.to_owned(),
        spread,
    })
}

/// The numbers of a layer report that a spread covers, with their units:
/// each estimate and each median under `stem` and the groups later
/// releases add.
///
/// The path drops the `value` or `median` key, such as `stem.rms_dbfs` or
/// `stem.band_share[1].share`. Confidences, IQRs, band edges and the
/// inputs (`bpm`, `bpm_given`, `bpm_refined` and `sample_rate_hz`) are left
/// out, and so is a `null`. The unit comes from the field name's suffix
/// (`_hz`, `_s`, `_db`, `_dbfs`, `_deg`); a field without one is a ratio.
#[must_use]
pub fn layer_values(report: &Value) -> Vec<(String, String, f64)> {
    let mut values = Vec::new();
    if let Some(root) = report.as_object() {
        for (key, group) in root {
            if !NON_LAYERS.contains(&key.as_str()) {
                walk(key, key, group, &mut values);
            }
        }
    }
    values
}

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

fn walk(path: &str, name: &str, value: &Value, out: &mut Vec<(String, String, f64)>) {
    match value {
        Value::Object(map) => {
            for (key, inner) in map {
                if SKIPPED.contains(&key.as_str()) {
                    continue;
                }
                if key == "value" || key == "median" {
                    walk(path, name, inner, out);
                } else {
                    walk(&format!("{path}.{key}"), key, inner, out);
                }
            }
        }
        Value::Array(items) => {
            for (index, inner) in items.iter().enumerate() {
                walk(&format!("{path}[{index}]"), name, inner, out);
            }
        }
        Value::Number(number) => {
            if let Some(number) = number.as_f64() {
                out.push((path.to_owned(), unit_of(name).to_owned(), number));
            }
        }
        _ => {}
    }
}

fn unit_of(name: &str) -> &'static str {
    [
        ("_dbfs", "dB"),
        ("_db", "dB"),
        ("_hz", "Hz"),
        ("_deg", "deg"),
        ("_s", "s"),
    ]
    .iter()
    .find(|(suffix, _)| name.ends_with(suffix))
    .map_or("ratio", |(_, unit)| unit)
}

/// The spread of one preset on one synth build and quality setting.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpreadTable {
    /// What rendered: the synth, its build, the host and the architecture.
    pub identity: HostIdentity,
    /// The recipe version.
    pub recipe_version: u32,
    /// The quality setting: the oversampling of every render.
    pub oversampling: Oversampling,
    /// How many renders.
    pub n: usize,
    /// The statistic, [`STATISTIC`].
    pub statistic: String,
    /// The SHA-256 of the preset file.
    pub preset_sha256: String,
    /// The SHA-256 of the MIDI file the notes came from, when they came
    /// from one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub midi_sha256: Option<String>,
    /// The tempo.
    pub bpm: Bpm,
    /// The spread of each output.
    pub outputs: Vec<SpreadRow>,
}

impl SpreadTable {
    /// Returns the row of `output`.
    #[must_use]
    pub fn row(&self, output: &str) -> Option<&SpreadRow> {
        self.outputs.iter().find(|row| row.output == output)
    }
}

/// How a clip compares with renders of the same preset on one output.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Comparison {
    /// The output.
    pub output: String,
    /// The unit.
    pub unit: String,
    /// The clip's value; absent for a distance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<f64>,
    /// The median of the renders' values; absent for a distance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renders: Option<f64>,
    /// The median, over the renders, of the difference or distance between
    /// the clip and one render.
    pub difference: f64,
    /// The spread of the output.
    pub spread: f64,
    /// Whether the difference lies within the spread.
    pub within: bool,
}

/// Compares `clip` with each of `renders` on every clip output and the
/// envelope distance, against `spread`.
///
/// One pair of clips differs by more
/// than the 95th percentile one time in twenty, so the comparison takes
/// the median over the renders of the pairwise difference. An output with
/// no spread row is left out.
#[must_use]
pub fn compare(
    clip: &ClipMetrics,
    renders: &[ClipMetrics],
    spread: &[SpreadRow],
) -> Vec<Comparison> {
    let spread_of = |output: &str| spread.iter().find(|row| row.output == output);
    let median = |mut values: Vec<f64>| {
        values.sort_by(f64::total_cmp);
        quantile(&values, 0.5)
    };
    let scalar = ClipOutput::ALL.into_iter().filter_map(|output| {
        let row = spread_of(output.name())?;
        let value = clip.value(output);
        let values: Vec<f64> = renders.iter().map(|render| render.value(output)).collect();
        let difference = median(values.iter().map(|v| (value - v).abs()).collect())?;
        Some(Comparison {
            output: row.output.clone(),
            unit: row.unit.clone(),
            clip: Some(value),
            renders: median(values),
            difference,
            spread: row.spread,
            within: difference <= row.spread,
        })
    });
    let envelope = spread_of(ENVELOPE_DISTANCE).and_then(|row| {
        let difference = median(
            renders
                .iter()
                .map(|render| envelope_distance(clip, render))
                .collect(),
        )?;
        Some(Comparison {
            output: row.output.clone(),
            unit: row.unit.clone(),
            clip: None,
            renders: None,
            difference,
            spread: row.spread,
            within: difference <= row.spread,
        })
    });
    scalar.chain(envelope).collect()
}

fn gain(db: f64) -> f64 {
    10_f64.powf(db / 20.0)
}

fn power_db(power: f64) -> f64 {
    if power > 0.0 {
        10.0 * power.log10()
    } else {
        SILENT_DB
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::SampleRate;

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

    #[test]
    fn a_sine_has_its_level_peak_and_length() {
        let metrics = ClipMetrics::of(&tone(0.5, 1.0, 4410)).unwrap();
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
        assert!(ClipMetrics::of(&tone(0.0, 1.0, 0)).is_none());
    }

    #[test]
    fn the_envelope_distance_aligns_onsets_and_counts_a_longer_tail() {
        let a = ClipMetrics::of(&tone(0.5, 1.0, 0)).unwrap();
        let late = ClipMetrics::of(&tone(0.5, 1.0, 22_050)).unwrap();
        assert!(envelope_distance(&a, &late) < 1e-6);
        let quieter = ClipMetrics::of(&tone(0.25, 1.0, 0)).unwrap();
        assert!((envelope_distance(&a, &quieter) - 6.02).abs() < 0.01);
        let longer = ClipMetrics::of(&tone(0.5, 2.0, 0)).unwrap();
        // Half the counted frames lie past the shorter clip, 40 dB apart.
        assert!((envelope_distance(&a, &longer) - 20.0).abs() < 0.5);
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
        let values: Vec<(String, String, f64)> = layer_values(&report);
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
        let row = value_row("x", "dB", &values).unwrap();
        assert!((row.spread - expected).abs() < 1e-12);
        assert!(value_row("x", "dB", &values[..1]).is_none());
    }
}
