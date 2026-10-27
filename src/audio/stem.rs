//! The stem that every estimator reads: samples, mid and side, the beat
//! grid, the band edge, the flat-top flag and the stereo screen.

use std::fmt;

use crate::cast;
use crate::confidence::{ConfidenceError, Estimate, Level, Reason, ReasonCode, Signal};
use crate::dsp::{ActiveFrames, Stft, StftParams, Summary, Window};
use crate::notes::NoteList;
use crate::units::{Bpm, Decibels, Hz, Normalized, Seconds};

use super::stereo::{StereoScreen, stereo_screen};
use super::{Audio, SampleRate};

/// Harmonic analysis uses 8192-sample frames below this median f0, else
/// 4096: a Hann main lobe is about 4 bins wide, so harmonics `f0` apart
/// need `N >= 4 fs / f0`.
pub const LONG_FRAME_BELOW_HZ: Hz = Hz::from_const(100.0);
/// A fall of at least this much over one semitone marks a cutoff, and the
/// band edge sits at its half-depth point. Real stems with a cutoff fall 30
/// to 44 dB there, natural roll-offs 15 to 22 dB.
pub const BAND_EDGE_STEEP_DROP_DB: Decibels = Decibels::from_const(25.0);
/// The plateau allowance of a steep fall in the band-edge rule.
///
/// A steep fall marks a cut only when the median level over the octave
/// below it lies within this much of the level where it starts: a cut ends
/// a band of content, while the fall above a lone tone does not.
pub const BAND_EDGE_PLATEAU_DB: Decibels = Decibels::from_const(20.0);
/// The margin above the noise floor that the fallback band-edge rule needs.
///
/// The fallback serves a spectrum without a steep fall. Real stems carry a
/// shelf of noise up to 21 dB above the floor beyond their cut, so the
/// margin is 30 dB.
pub const BAND_EDGE_MARGIN_DB: Decibels = Decibels::from_const(30.0);
/// Samples within this share of the peak count as at the ceiling.
pub const CEILING_TOLERANCE: Normalized = Normalized::from_const(1e-5);
/// The shortest run at the ceiling that counts as a flat top.
pub const FLAT_TOP_MIN_RUN_S: Seconds = Seconds::from_const(0.001);
/// The approach share of a flat-top run.
///
/// A run counts only when the samples on both sides of it lie at least this
/// share of the ceiling on the same side: clipping leaves a wave that runs
/// into the ceiling, while a square or pulse wave jumps to it.
pub const CLIP_APPROACH: Normalized = Normalized::from_const(0.5);
/// The share of samples in flat-top runs that marks a stem hard-limited.
pub const FLAT_TOP_MIN_SHARE: Normalized = Normalized::from_const(0.001);

/// The convention of the band edge.
pub const BAND_EDGE_CONVENTION: &str = "long-term power spectrum of the mid over the active \
frames that lie wholly inside the stem, median-smoothed over one semitone on each side; where \
the smoothed level falls at least 25 dB over one semitone above 2 kHz and the median level of \
the octave below lies within 20 dB of the level where the fall starts, the edge is the \
half-depth point of the steepest such fall; otherwise the noise floor is the lowest level above \
1 kHz and the edge is the highest bin at least 30 dB above it, at low confidence, or the Nyquist \
frequency when no bin above 1 kHz is";
/// The convention of the flat-top flag.
pub const HARD_LIMITED_CONVENTION: &str = "true when runs of at least 1 ms of samples \
within 1e-5 of the channel peak, each with its neighbours on both sides at least half the \
peak on the same side, cover at least 0.1 % of a channel";

/// The length of a note division in beats, before its feel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NoteValue {
    /// Four bars of 4/4, 16 beats.
    FourBars,
    /// Two bars of 4/4, 8 beats.
    TwoBars,
    /// A whole note, one bar of 4/4, 4 beats.
    Whole,
    /// A half note, 2 beats.
    Half,
    /// A quarter note, 1 beat.
    Quarter,
    /// An eighth note, half a beat.
    Eighth,
    /// A sixteenth note, a quarter beat.
    Sixteenth,
    /// A thirty-second note, an eighth of a beat.
    ThirtySecond,
}

impl NoteValue {
    fn beats(self) -> f64 {
        match self {
            Self::FourBars => 16.0,
            Self::TwoBars => 8.0,
            Self::Whole => 4.0,
            Self::Half => 2.0,
            Self::Quarter => 1.0,
            Self::Eighth => 0.5,
            Self::Sixteenth => 0.25,
            Self::ThirtySecond => 0.125,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::FourBars => "4 bars",
            Self::TwoBars => "2 bars",
            Self::Whole => "1/1",
            Self::Half => "1/2",
            Self::Quarter => "1/4",
            Self::Eighth => "1/8",
            Self::Sixteenth => "1/16",
            Self::ThirtySecond => "1/32",
        }
    }
}

/// The feel of a note division.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Feel {
    /// The plain value.
    Straight,
    /// Three in the time of two: the rate times 1.5.
    Triplet,
    /// One and a half times as long: the rate times 2/3.
    Dotted,
}

/// A note division, such as a dotted eighth, that an LFO rate can lock to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Division {
    /// The note value.
    pub value: NoteValue,
    /// The feel.
    pub feel: Feel,
}

impl Division {
    /// The supported divisions: 1/1 to 1/32, each straight, triplet and
    /// dotted, plus 2 and 4 bars, slowest first.
    pub const ALL: [Self; 20] = {
        use Feel::{Dotted, Straight, Triplet};
        use NoteValue::{Eighth, FourBars, Half, Quarter, Sixteenth, ThirtySecond, TwoBars, Whole};
        [
            Self::of(FourBars, Straight),
            Self::of(TwoBars, Straight),
            Self::of(Whole, Dotted),
            Self::of(Whole, Straight),
            Self::of(Half, Dotted),
            Self::of(Whole, Triplet),
            Self::of(Half, Straight),
            Self::of(Quarter, Dotted),
            Self::of(Half, Triplet),
            Self::of(Quarter, Straight),
            Self::of(Eighth, Dotted),
            Self::of(Quarter, Triplet),
            Self::of(Eighth, Straight),
            Self::of(Sixteenth, Dotted),
            Self::of(Eighth, Triplet),
            Self::of(Sixteenth, Straight),
            Self::of(ThirtySecond, Dotted),
            Self::of(Sixteenth, Triplet),
            Self::of(ThirtySecond, Straight),
            Self::of(ThirtySecond, Triplet),
        ]
    };

    const fn of(value: NoteValue, feel: Feel) -> Self {
        Self { value, feel }
    }

    /// Returns the beats one cycle lasts: the note value's beats, times 2/3
    /// for a triplet and 3/2 for a dotted note.
    #[must_use]
    pub fn beats_per_cycle(self) -> f64 {
        let beats = self.value.beats();
        match self.feel {
            Feel::Straight => beats,
            Feel::Triplet => beats * 2.0 / 3.0,
            Feel::Dotted => beats * 1.5,
        }
    }

    /// Returns the rate of one cycle per division at a tempo,
    /// `(BPM / 60) / beats_per_cycle`.
    #[must_use]
    pub fn rate(self, bpm: Bpm) -> Hz {
        let beat_hz = bpm.get() / 60.0;
        Hz::clamped(match self.feel {
            Feel::Straight => beat_hz / self.value.beats(),
            Feel::Triplet => beat_hz / self.value.beats() * 1.5,
            Feel::Dotted => beat_hz / self.value.beats() * 2.0 / 3.0,
        })
    }
}

impl fmt::Display for Division {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let suffix = match self.feel {
            Feel::Straight => "",
            Feel::Triplet => "T",
            Feel::Dotted => "D",
        };
        write!(f, "{}{suffix}", self.value.label())
    }
}

/// The beat grid from the BPM. The BPM gives the beat period, not where the
/// first beat falls, so the offset is given, never estimated; it defaults
/// to 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeatGrid {
    bpm: Bpm,
    offset: Seconds,
}

impl BeatGrid {
    /// A grid at `bpm` whose first beat falls at `offset`.
    #[must_use]
    pub fn new(bpm: Bpm, offset: Seconds) -> Self {
        Self { bpm, offset }
    }

    /// Returns the tempo.
    #[must_use]
    pub fn bpm(self) -> Bpm {
        self.bpm
    }

    /// Returns the time of the first beat.
    #[must_use]
    pub fn offset(self) -> Seconds {
        self.offset
    }

    /// Returns the beat period, `60 / BPM` seconds.
    #[must_use]
    pub fn beat_period(self) -> Seconds {
        Seconds::clamped(60.0 / self.bpm.get())
    }

    /// Returns the time of beat `n`, counted from 0 at the offset.
    #[must_use]
    pub fn beat_time(self, beat: usize) -> Seconds {
        Seconds::clamped(self.offset.get() + cast::usize_to_f64(beat) * 60.0 / self.bpm.get())
    }

    /// Returns the rate of a division at the grid's tempo.
    #[must_use]
    pub fn rate(self, division: Division) -> Hz {
        division.rate(self.bpm)
    }
}

/// The stem: native-rate samples with mid and side, the beat grid, the
/// band edge, the flat-top flag and the stereo screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Stem {
    audio: Audio,
    mid: Vec<f64>,
    grid: BeatGrid,
    median_f0: Option<Hz>,
    frame_len: usize,
    band_edge: Estimate<Hz, Hz>,
    hard_limited: bool,
    stereo: StereoScreen,
}

impl Stem {
    /// Builds the stem. The median f0 of `notes` picks the frame length and
    /// feeds the stereo phase; without notes the frame is 4096 samples and
    /// every phase is `None`.
    ///
    /// # Errors
    ///
    /// Never in practice; the reasons are built from counts and flags.
    pub fn new(
        audio: Audio,
        grid: BeatGrid,
        notes: Option<&NoteList>,
    ) -> Result<Self, ConfidenceError> {
        let mid = audio.mono();
        let median_f0 = notes.and_then(median_f0);
        let frame_len = frame_len_for(median_f0);
        let rate = audio.sample_rate();
        let band_edge = band_edge(&mid, rate, StftParams::fixed(frame_len))?;
        let hard_limited = audio
            .channels()
            .iter()
            .any(|channel| is_flat_topped(channel, rate));
        let stereo = match audio.channels() {
            [left, right, ..] => stereo_screen(left, right, rate, median_f0)?,
            _ => StereoScreen::one_channel()?,
        };
        Ok(Self {
            audio,
            mid,
            grid,
            median_f0,
            frame_len,
            band_edge,
            hard_limited,
            stereo,
        })
    }

    /// Returns the decoded audio with every channel.
    #[must_use]
    pub fn audio(&self) -> &Audio {
        &self.audio
    }

    /// Returns the sample rate.
    #[must_use]
    pub fn sample_rate(&self) -> SampleRate {
        self.audio.sample_rate()
    }

    /// Returns the length.
    #[must_use]
    pub fn duration(&self) -> Seconds {
        self.audio.duration()
    }

    /// Returns the mid, `M = (L + R) / 2`: the mono signal of every
    /// non-stereo estimate. A one-channel file's mid is its channel.
    #[must_use]
    pub fn mid(&self) -> &[f64] {
        &self.mid
    }

    /// Returns the side, `S = (L - R) / 2`, computed on demand to spare
    /// memory, or `None` for a one-channel file.
    #[must_use]
    pub fn side(&self) -> Option<Vec<f64>> {
        match self.audio.channels() {
            [left, right, ..] => Some(left.iter().zip(right).map(|(l, r)| (l - r) / 2.0).collect()),
            _ => None,
        }
    }

    /// Returns the beat grid.
    #[must_use]
    pub fn grid(&self) -> BeatGrid {
        self.grid
    }

    /// Returns the median f0 of the note list, if one was given.
    #[must_use]
    pub fn median_f0(&self) -> Option<Hz> {
        self.median_f0
    }

    /// Returns the frame length of harmonic analysis: 8192 below a median
    /// f0 of 100 Hz, else 4096, and 4096 without a note list.
    #[must_use]
    pub fn frame_len(&self) -> usize {
        self.frame_len
    }

    /// Returns the band edge: the highest frequency that stays above the
    /// noise floor. A filter cutoff within one octave of it is never
    /// reported.
    #[must_use]
    pub fn band_edge(&self) -> &Estimate<Hz, Hz> {
        &self.band_edge
    }

    /// Returns whether the stem is flat-topped at its peak, as a limiter or
    /// clipping leaves it.
    #[must_use]
    pub fn hard_limited(&self) -> bool {
        self.hard_limited
    }

    /// Returns the stereo screen of the whole stem.
    #[must_use]
    pub fn stereo(&self) -> &StereoScreen {
        &self.stereo
    }
}

fn median_f0(notes: &NoteList) -> Option<Hz> {
    let frequencies: Vec<f64> = notes
        .notes()
        .iter()
        .map(|note| Hz::from(note.pitch()).get())
        .collect();
    Summary::of(&frequencies).map(|summary| Hz::clamped(summary.median()))
}

fn frame_len_for(median_f0: Option<Hz>) -> usize {
    match median_f0 {
        Some(f0) if f0 < LONG_FRAME_BELOW_HZ => 8192,
        _ => 4096,
    }
}

/// The band edge of a mono signal: see [`BAND_EDGE_CONVENTION`].
///
/// # Errors
///
/// Never in practice; the reasons are built from counts and flags.
pub fn band_edge(
    samples: &[f64],
    rate: SampleRate,
    params: StftParams,
) -> Result<Estimate<Hz, Hz>, ConfidenceError> {
    let params = params.with_window(Window::Hann);
    let active = ActiveFrames::of(samples, params);
    let frames = Reason::new(
        ReasonCode::ActiveFrames,
        Signal::Count(u64::try_from(active.count()).unwrap_or(u64::MAX)),
    )?;
    if active.count() == 0 {
        return Estimate::none(vec![
            Reason::new(ReasonCode::SilentStem, Signal::Flag(true))?,
            frames,
        ]);
    }
    // A frame that reaches into the padding sees the signal start or stop as
    // a step, whose broadband energy would hide a low-pass. Such frames count
    // only when no frame lies wholly inside.
    let inside: Vec<usize> = active
        .indices()
        .filter(|&frame| {
            let start = params.frame_start(frame);
            start >= 0
                && usize::try_from(start)
                    .is_ok_and(|start| start + params.frame_len() <= samples.len())
        })
        .collect();
    let frames_used = if inside.is_empty() {
        active.indices().collect()
    } else {
        inside
    };
    let (smoothed, bin_hz) = smoothed_spectrum(samples, rate, params, &frames_used);
    let steepest = steepest_edge(&smoothed, bin_hz);
    let depth = steepest.map_or(0.0, |(_, depth)| depth);
    let mut reasons = vec![
        frames,
        Reason::new(ReasonCode::BandEdgeDrop, Signal::Number(depth))?,
    ];
    // Without a steep fall the margin rule only bounds where the content
    // ends, so its edge is Low. A spectrum that never rises the margin above
    // its floor has no edge below the Nyquist frequency.
    let (edge, level) = match steepest {
        Some((edge, depth)) if depth >= BAND_EDGE_STEEP_DROP_DB.get() => (edge, Level::High),
        _ => {
            if let Some(edge) = margin_edge(&smoothed, bin_hz) {
                (edge, Level::Low)
            } else {
                reasons.push(Reason::new(ReasonCode::NoBandEdge, Signal::Flag(true))?);
                (smoothed.len().saturating_sub(1), Level::Low)
            }
        }
    };
    Estimate::new(
        Hz::clamped(cast::usize_to_f64(edge) * bin_hz),
        level,
        Some(Hz::clamped(bin_hz)),
        reasons,
    )
}

/// The long-term power spectrum in dB over `frames`, median-smoothed over
/// one semitone, and the bin width.
fn smoothed_spectrum(
    samples: &[f64],
    rate: SampleRate,
    params: StftParams,
    frames: &[usize],
) -> (Vec<f64>, f64) {
    let mut stft = Stft::new(params);
    let mut power = vec![0.0; params.bins()];
    for &frame in frames {
        for (sum, value) in power.iter_mut().zip(stft.frame(samples, frame)) {
            *sum += value.norm_sqr();
        }
    }
    let peak = power.iter().copied().fold(0.0, f64::max);
    // A floor far below any real noise keeps the logarithm finite.
    let levels: Vec<f64> = power
        .iter()
        .map(|p| 10.0 * (p + peak * 1e-30).log10())
        .collect();
    (semitone_median(&levels), params.bin_width(rate).get())
}

/// The fallback rule: the highest bin at least [`BAND_EDGE_MARGIN_DB`]
/// above the floor, the lowest smoothed level above 1 kHz. `None` when no
/// bin above 1 kHz rises that far.
fn margin_edge(smoothed: &[f64], bin_hz: f64) -> Option<usize> {
    let first_above_1k = cast::ceil_to_usize(1000.0 / bin_hz).unwrap_or(1).max(1);
    let floor = smoothed
        .get(first_above_1k..)
        .and_then(|band| band.iter().copied().reduce(f64::min))
        .unwrap_or(f64::NEG_INFINITY);
    smoothed
        .iter()
        .rposition(|&level| level >= floor + BAND_EDGE_MARGIN_DB.get())
        .filter(|&edge| edge >= first_above_1k)
}

/// The steepest fall of the smoothed level over one semitone above 2 kHz:
/// the bin at the half-depth point of that fall, and its depth in dB.
fn steepest_edge(smoothed: &[f64], bin_hz: f64) -> Option<(usize, f64)> {
    let semitone = 2.0_f64.powf(1.0 / 12.0);
    let first = cast::ceil_to_usize(2000.0 / bin_hz).unwrap_or(1).max(1);
    let (depth, start, stop) = (first..smoothed.len())
        .filter_map(|bin| {
            let stop = cast::round_to_usize(cast::usize_to_f64(bin) * semitone)?;
            let below = *smoothed.get(stop)?;
            Some((smoothed[bin] - below, bin, stop))
        })
        .fold(None, |best: Option<(f64, usize, usize)>, item| match best {
            Some(b) if b.0 >= item.0 => Some(b),
            _ => Some(item),
        })?;
    let half = smoothed[start] - depth / 2.0;
    let edge = (start..=stop).rev().find(|&bin| smoothed[bin] >= half)?;
    // A cut ends a band of content; the edge of a lone tone does not. The
    // octave below the fall must sit within the plateau allowance of its top.
    let octave = Summary::of(&smoothed[start / 2..start])?.median();
    let supported = octave >= smoothed[start] - BAND_EDGE_PLATEAU_DB.get();
    Some((edge, if supported { depth } else { 0.0 }))
}

/// A running median of the levels over one semitone on each side of each
/// bin, at least two bins. A median keeps a step edge where it is.
fn semitone_median(levels: &[f64]) -> Vec<f64> {
    let semitone = 2.0_f64.powf(1.0 / 12.0) - 1.0;
    (0..levels.len())
        .map(|bin| {
            let half = cast::round_to_usize(cast::usize_to_f64(bin) * semitone)
                .unwrap_or(2)
                .max(2);
            let lo = bin.saturating_sub(half);
            let hi = (bin + half + 1).min(levels.len());
            Summary::of(&levels[lo..hi]).map_or(levels[bin], Summary::median)
        })
        .collect()
}

/// Whether a channel is flat-topped: see [`HARD_LIMITED_CONVENTION`].
#[must_use]
pub fn is_flat_topped(channel: &[f64], rate: SampleRate) -> bool {
    let peak = channel.iter().fold(0.0_f64, |max, x| max.max(x.abs()));
    if peak <= 0.0 || channel.is_empty() {
        return false;
    }
    let ceiling = peak * (1.0 - CEILING_TOLERANCE.get());
    let min_run = cast::ceil_to_usize(FLAT_TOP_MIN_RUN_S.get() * rate.hz().get())
        .unwrap_or(1)
        .max(2);
    // A clipped wave runs into the ceiling and out of it on the same side; a
    // square or pulse wave jumps to it from the other side.
    let approaches = |index: Option<usize>, sign: f64| {
        index
            .and_then(|i| channel.get(i))
            .is_some_and(|&x| x * sign >= CLIP_APPROACH.get() * ceiling)
    };
    let mut flat = 0_usize;
    let mut start = 0;
    while start < channel.len() {
        let sign = channel[start].signum();
        if channel[start].abs() < ceiling {
            start += 1;
            continue;
        }
        let end = channel[start..]
            .iter()
            .position(|&x| x.abs() < ceiling || x.is_sign_positive() != sign.is_sign_positive())
            .map_or(channel.len(), |offset| start + offset);
        let run = end - start;
        if run >= min_run && approaches(start.checked_sub(1), sign) && approaches(Some(end), sign) {
            flat += run;
        }
        start = end;
    }
    cast::usize_to_f64(flat) >= FLAT_TOP_MIN_SHARE.get() * cast::usize_to_f64(channel.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_constants_pass_the_unit_check() {
        assert_eq!(LONG_FRAME_BELOW_HZ, Hz::new(100.0).unwrap());
        assert_eq!(BAND_EDGE_MARGIN_DB, Decibels::new(30.0).unwrap());
        assert_eq!(BAND_EDGE_STEEP_DROP_DB, Decibels::new(25.0).unwrap());
        assert_eq!(BAND_EDGE_PLATEAU_DB, Decibels::new(20.0).unwrap());
        assert_eq!(CEILING_TOLERANCE, Normalized::new(1e-5).unwrap());
        assert_eq!(FLAT_TOP_MIN_RUN_S, Seconds::new(0.001).unwrap());
        assert_eq!(FLAT_TOP_MIN_SHARE, Normalized::new(0.001).unwrap());
        assert_eq!(CLIP_APPROACH, Normalized::new(0.5).unwrap());
    }

    fn bpm(value: f64) -> Bpm {
        Bpm::new(value).unwrap()
    }

    fn rate() -> SampleRate {
        SampleRate::new(44_100).unwrap()
    }

    #[test]
    fn at_120_bpm_beats_fall_every_half_second_and_divisions_have_their_rates() {
        let grid = BeatGrid::new(bpm(120.0), Seconds::new(0.0).unwrap());
        assert_eq!(grid.beat_period().get(), 0.5);
        assert_eq!(grid.beat_time(3).get(), 1.5);
        let straight = |value| Division {
            value,
            feel: Feel::Straight,
        };
        assert_eq!(grid.rate(straight(NoteValue::Half)).get(), 1.0);
        assert_eq!(grid.rate(straight(NoteValue::Quarter)).get(), 2.0);
        assert_eq!(grid.rate(straight(NoteValue::Eighth)).get(), 4.0);
        assert_eq!(grid.rate(straight(NoteValue::Sixteenth)).get(), 8.0);
        assert_eq!(grid.rate(straight(NoteValue::Whole)).get(), 0.5);
        let quarter = |feel| Division {
            value: NoteValue::Quarter,
            feel,
        };
        assert_eq!(grid.rate(quarter(Feel::Triplet)).get(), 3.0);
        assert!((grid.rate(quarter(Feel::Dotted)).get() - 4.0 / 3.0).abs() < 1e-15);
    }

    #[test]
    fn divisions_are_sorted_slowest_first_and_named() {
        let rates: Vec<f64> = Division::ALL
            .iter()
            .map(|d| d.rate(bpm(120.0)).get())
            .collect();
        assert!(rates.windows(2).all(|pair| pair[0] < pair[1]), "{rates:?}");
        assert_eq!(Division::ALL[10].to_string(), "1/8D");
        assert_eq!(Division::ALL[0].to_string(), "4 bars");
    }

    #[test]
    fn a_one_channel_stem_has_no_side_and_no_stereo_values() {
        let samples: Vec<f64> = (0..44_100)
            .map(|n| (cast::usize_to_f64(n) * 0.05).sin())
            .collect();
        let audio = Audio::new(rate(), vec![samples.clone()]).unwrap();
        let stem = Stem::new(
            audio,
            BeatGrid::new(bpm(120.0), Seconds::new(0.0).unwrap()),
            None,
        )
        .unwrap();
        assert_eq!(stem.mid(), samples.as_slice());
        assert_eq!(stem.side(), None);
        assert!(!stem.stereo().confidence().is_estimate());
        assert_eq!(stem.frame_len(), 4096);
    }

    #[test]
    fn equal_channels_have_zero_side_and_opposite_ones_zero_mid() {
        let x: Vec<f64> = (0..8820)
            .map(|n| (cast::usize_to_f64(n) * 0.05).sin())
            .collect();
        let negated: Vec<f64> = x.iter().map(|v| -v).collect();
        let grid = BeatGrid::new(bpm(120.0), Seconds::new(0.0).unwrap());
        let same = Stem::new(
            Audio::new(rate(), vec![x.clone(), x.clone()]).unwrap(),
            grid,
            None,
        )
        .unwrap();
        assert!(same.side().unwrap().iter().all(|&s| s == 0.0));
        let opposite =
            Stem::new(Audio::new(rate(), vec![x, negated]).unwrap(), grid, None).unwrap();
        assert!(opposite.mid().iter().all(|&m| m == 0.0));
    }

    #[test]
    fn a_clipped_sine_is_flat_topped_and_clean_or_saturated_ones_are_not() {
        let sine: Vec<f64> = (0..44_100)
            .map(|n| (std::f64::consts::TAU * 110.0 * cast::usize_to_f64(n) / 44_100.0).sin())
            .collect();
        let clipped: Vec<f64> = sine.iter().map(|v| v.clamp(-0.5, 0.5)).collect();
        let saturated: Vec<f64> = sine.iter().map(|v| (4.0 * v).tanh()).collect();
        assert!(is_flat_topped(&clipped, rate()));
        assert!(!is_flat_topped(&sine, rate()));
        assert!(!is_flat_topped(&saturated, rate()));
        assert!(!is_flat_topped(&[0.0; 100], rate()));
    }
}
