//! How far a layer trusts one of its estimates.
//!
//! A [`Confidence`] holds a [`Level`], the [`Reason`]s behind it and an
//! optional spread, or states that the layer made no estimate. The level is
//! stored, not derived: each estimator sets it from its own signals. A
//! confidence is never one float, because the agent that reads the report
//! needs the level and the reasons to decide what to override.
//!
//! The spread is typed: `Confidence<Octaves>` describes a cutoff whose
//! spread is in octaves, `Confidence<Cents>` a unison offset whose spread is
//! in cents. An estimate that has no spread, such as a menu choice, uses
//! [`Confidence<NoSpread>`](NoSpread).
//!
//! In a report a confidence is one JSON object:
//!
//! ```json
//! {"level": "medium", "reasons": [{"code": "active_frames", "value": 412}], "spread": 0.3, "estimate": true}
//! ```
//!
//! `spread` is left out when there is none. A "no estimate" state writes
//! `"estimate": false`, no `level`, no `spread` and at least one reason.
//! Deserialization applies the same rules as the constructors.

use std::error::Error;
use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::units::{
    Cents, Correlation, Decibels, Degrees, Hz, MidiPitch, Normalized, Octaves, Percent, Polarity,
    RangeError, Ratio, Seconds, Semitones,
};

/// How far an estimator trusts its value. The order is `Low < Medium <
/// High`, so a per-layer cap is `level.min(cap)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// The value is a weak guess. The agent should check it first.
    Low,
    /// The value is likely right.
    Medium,
    /// The signals agree on the value.
    High,
}

/// Defines [`ReasonCode`] and the list of all its variants together, so a
/// new code cannot miss the list that the tests draw from.
macro_rules! reason_codes {
    ($($(#[$doc:meta])* $variant:ident,)+) => {
        /// Why an estimator chose its level, or why it made no estimate.
        ///
        /// Each code is a stable `snake_case` identifier in the report. Each
        /// estimator adds the codes of its own signals here.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum ReasonCode {
            $($(#[$doc])* $variant,)+
        }

        impl ReasonCode {
            /// Every code, in declaration order.
            #[cfg(test)]
            pub(crate) const ALL: &[Self] = &[$(Self::$variant,)+];
        }
    };
}

reason_codes! {
    /// The number of analysis frames with signal above the noise floor.
    ActiveFrames,
    /// Too few notes to fit the value.
    TooFewNotes,
    /// The pitch may be an octave off.
    OctaveUncertain,
    /// The stem is digital silence.
    SilentStem,
    /// The file has one channel, so no stereo value exists.
    OneChannel,
    /// No note list was given, so a value that needs the f0 has none.
    NoNoteList,
    /// The number of full cycles of a rate in the span. Below two, no rate
    /// is reported.
    CyclesInSpan,
    /// The stem is flat-topped at its peak, so its level and crest are
    /// capped by a limiter or by clipping.
    HardLimited,
    /// The stereo screen cannot see an anti-phase stereo chorus or a light
    /// reverb.
    StereoBlindSpot,
    /// The steepest fall of the smoothed spectrum over one semitone above
    /// 2 kHz, in dB. Below the steep-drop threshold the band edge comes from
    /// the margin rule.
    BandEdgeDrop,
    /// The strongest line of a modulation scan over the median magnitude of
    /// the scan. Below the prominence threshold no rate is reported.
    LineProminence,
    /// Fewer active frames than a statistic over frames needs; the value is
    /// the minimum.
    TooFewFrames,
    /// No bin of the spectrum above 1 kHz rises the band-edge margin above
    /// its floor, so the band edge is the Nyquist frequency at low
    /// confidence.
    NoBandEdge,
    /// The best score of the tempo search over its median score. Below the
    /// trust threshold the given tempo stays; 0 when the stem has no
    /// onsets.
    TempoPeak,
    /// The share of the onset strength that repeats at the refined beat: the
    /// best score of the tempo search over its lag count times the
    /// zero-lag autocorrelation. Below the trust threshold the given tempo
    /// stays.
    BeatPeriodicity,
    /// The share of the onset strength that repeats every 2 or every 4
    /// refined beats, the better of the two, as [`Self::BeatPeriodicity`]
    /// reads it on those lags alone. Onsets on every other beat or once a
    /// bar read near 1. It trusts a sparse part when the beat periodicity
    /// does not; 0 when the stem holds too few beats for it. At most 1.
    SparsePeriodicity,
    /// The share of the energy of the raw onset strength, before its
    /// low-pass, within 25 ms of its peaks. Hits and plucks read near 1; a
    /// swell, such as a pad under a slow tremolo, reads low. A refined
    /// tempo is trusted only with sharp onsets.
    OnsetSharpness,
    /// The peaks of the onset strength per beat of the slowest tempo
    /// searched. The sparse periodicity trusts a tempo only for a part
    /// this sparse, because a dense grid, such as sixteenth notes, lines
    /// up with any spacing that is a whole number of its steps.
    OnsetsPerBeat,
    /// `--bpm-exact` was given, so the tempo was not refined.
    BpmExact,
    /// The number of voiced frames of the f0 track.
    VoicedFrames,
    /// The line plays chords, so a method for one voice at a time made no
    /// estimate.
    PolyphonicLine,
    /// How long a pitch motion moved faster than the glide floor, in
    /// seconds. A short motion reads a glide's time less well.
    MotionSpan,
    /// The RMS residual of a fit: in cents for a glide, as a share of the
    /// drop for a pitch drop.
    FitResidual,
    /// The RMS deviation of the stable pitches around a glide from their
    /// medians, in cents. A unison beats and the track wanders, which
    /// skews the glide's time.
    PitchJitter,
    /// The share of a contour's power that a fitted sine explains.
    SineShare,
    /// The kurtosis of a vibrato's contour after its line, the fourth
    /// moment over the squared second: 1.5 for a sine, 1.0 for a square
    /// wave such as a trill between two notes.
    ContourKurtosis,
    /// The time the band envelope of a strike takes from 10 % to 90 % of its
    /// peak, in seconds. A slow attack hides the start of a pitch drop.
    AttackTime,
    /// A glide's fitted curve power sits on the upper bound of its range:
    /// an exponential approach, read with less certainty.
    CurveBound,
    /// The share of the track frames inside the notes that are voiced
    /// within 100 cents of a sounding note. Noise or DC under a note list
    /// reads near 0.
    NoteFrames,
    /// The interquartile range of a value over the notes, divided by the
    /// absolute value of its median. Above 0.5 the notes disagree too much
    /// for a preset.
    RelativeSpread,
    /// The share of the pooled notes whose attack is an upper bound: its
    /// rise is shorter than 1.5 RMS windows, or for an overlapping note
    /// twice the rise of its band filter on an instant step.
    UpperBoundShare,
    /// The interquartile range of a value over the notes whose median is 0,
    /// so the range has no ratio to the median. The value reads low.
    ZeroMedian,
}

/// The value of the signal behind a [`ReasonCode`].
///
/// In JSON a count is an integer, a number has a decimal point, and a flag
/// is a bool.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Signal {
    /// A count, such as a number of frames or notes.
    Count(u64),
    /// A measured value. [`Reason::new`] rejects NaN and both infinities.
    Number(f64),
    /// A yes-or-no signal, such as a saturation flag.
    Flag(bool),
}

/// A reason code with the value of its signal.
///
/// ```
/// use sound_to_patch::confidence::{Reason, ReasonCode, Signal};
///
/// let reason = Reason::new(ReasonCode::ActiveFrames, Signal::Count(412))?;
/// assert!(Reason::new(ReasonCode::ActiveFrames, Signal::Number(f64::NAN)).is_err());
/// # Ok::<(), sound_to_patch::confidence::ConfidenceError>(())
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reason {
    code: ReasonCode,
    value: Signal,
}

impl Reason {
    /// Pairs a code with its signal value.
    ///
    /// # Errors
    ///
    /// Returns a [`ConfidenceError`] for a [`Signal::Number`] that is NaN or
    /// infinite.
    pub fn new(code: ReasonCode, value: Signal) -> Result<Self, ConfidenceError> {
        if let Signal::Number(number) = value {
            check_signal(number).map_err(Kind::Signal)?;
        }
        Ok(Self { code, value })
    }

    /// Returns the reason code.
    #[must_use]
    pub fn code(self) -> ReasonCode {
        self.code
    }

    /// Returns the signal value.
    #[must_use]
    pub fn value(self) -> Signal {
        self.value
    }
}

fn check_signal(number: f64) -> Result<f64, RangeError> {
    if number.is_finite() {
        Ok(number)
    } else {
        Err(RangeError::new(
            "Reason signal",
            number,
            crate::units::Range::Finite,
        ))
    }
}

impl Serialize for Signal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match *self {
            Self::Count(count) => serializer.serialize_u64(count),
            Self::Number(number) => serializer.serialize_f64(number),
            Self::Flag(flag) => serializer.serialize_bool(flag),
        }
    }
}

impl<'de> Deserialize<'de> for Signal {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(SignalVisitor)
    }
}

struct SignalVisitor;

impl Visitor<'_> for SignalVisitor {
    type Value = Signal;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "a count, a finite number or a bool")
    }

    fn visit_u64<E: de::Error>(self, count: u64) -> Result<Signal, E> {
        Ok(Signal::Count(count))
    }

    // A negative integer is a number; within `i32` it converts exactly.
    fn visit_i64<E: de::Error>(self, number: i64) -> Result<Signal, E> {
        if let Ok(count) = u64::try_from(number) {
            return Ok(Signal::Count(count));
        }
        i32::try_from(number)
            .map(|number| Signal::Number(f64::from(number)))
            .map_err(|_| E::invalid_value(de::Unexpected::Signed(number), &self))
    }

    fn visit_f64<E: de::Error>(self, number: f64) -> Result<Signal, E> {
        check_signal(number).map(Signal::Number).map_err(E::custom)
    }

    fn visit_bool<E: de::Error>(self, flag: bool) -> Result<Signal, E> {
        Ok(Signal::Flag(flag))
    }
}

/// A unit a spread can be given in: every unit type except
/// [`Bpm`](crate::units::Bpm), whose range excludes zero, or [`NoSpread`].
///
/// The trait is sealed.
pub trait Spread: Copy + fmt::Debug + PartialEq + sealed::Spread {}

/// The spread type of an estimate that has no spread, such as a menu
/// choice. It has no values, so a `Confidence<NoSpread>` never holds a
/// spread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoSpread {}

mod sealed {
    use super::ConfidenceError;

    pub trait Spread: Sized {
        fn to_f64(self) -> f64;
        fn from_f64(value: f64) -> Result<Self, ConfidenceError>;
    }
}

macro_rules! spread {
    ($($unit:ty),+) => {$(
        impl sealed::Spread for $unit {
            fn to_f64(self) -> f64 {
                self.get()
            }

            fn from_f64(value: f64) -> Result<Self, ConfidenceError> {
                Ok(Self::new(value).map_err(Kind::Spread)?)
            }
        }

        impl Spread for $unit {}
    )+};
}

spread!(
    Hz,
    Seconds,
    Semitones,
    Cents,
    Octaves,
    Decibels,
    Normalized,
    MidiPitch,
    Correlation,
    Degrees,
    Ratio
);

impl<P: Polarity> sealed::Spread for Percent<P> {
    fn to_f64(self) -> f64 {
        self.get()
    }

    fn from_f64(value: f64) -> Result<Self, ConfidenceError> {
        Ok(Self::new(value).map_err(Kind::Spread)?)
    }
}

impl<P: Polarity> Spread for Percent<P> {}

impl sealed::Spread for NoSpread {
    fn to_f64(self) -> f64 {
        match self {}
    }

    fn from_f64(_: f64) -> Result<Self, ConfidenceError> {
        Err(Kind::SpreadNotAllowed.into())
    }
}

impl Spread for NoSpread {}

/// The confidence of one estimate: a level with an optional spread, or no
/// estimate. Either state carries at least one [`Reason`].
///
/// The constructors keep the parts together: an estimate always has a
/// level, and a "no estimate" state never has a level or a spread.
///
/// ```
/// use sound_to_patch::confidence::{Confidence, Level, Reason, ReasonCode, Signal};
/// use sound_to_patch::units::Octaves;
///
/// let frames = Reason::new(ReasonCode::ActiveFrames, Signal::Count(412))?;
/// let cutoff = Confidence::estimate(Level::Medium, Some(Octaves::new(0.3)?), vec![frames])?;
/// assert_eq!(cutoff.level(), Some(Level::Medium));
///
/// let notes = Reason::new(ReasonCode::TooFewNotes, Signal::Count(2))?;
/// let envelope = Confidence::<Octaves>::no_estimate(vec![notes])?;
/// assert_eq!(envelope.level(), None);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// A "no estimate" state takes no level:
///
/// ```compile_fail,E0061
/// use sound_to_patch::confidence::{Confidence, Level, NoSpread};
///
/// let _ = Confidence::<NoSpread>::no_estimate(Level::High, Vec::new());
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Confidence<S: Spread> {
    state: State<S>,
    reasons: Vec<Reason>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum State<S> {
    Estimate { level: Level, spread: Option<S> },
    NoEstimate,
}

impl<S: Spread> Confidence<S> {
    /// Builds the confidence of an estimate.
    ///
    /// # Errors
    ///
    /// Returns a [`ConfidenceError`] for an empty reason list or a
    /// negative spread.
    pub fn estimate(
        level: Level,
        spread: Option<S>,
        reasons: Vec<Reason>,
    ) -> Result<Self, ConfidenceError> {
        if let Some(spread) = spread {
            crate::units::Range::AtLeast(0.0)
                .check("Confidence spread", spread.to_f64())
                .map_err(Kind::Spread)?;
        }
        Self::with_reasons(State::Estimate { level, spread }, reasons)
    }

    /// Builds the state of a layer that had too little evidence to report a
    /// value.
    ///
    /// # Errors
    ///
    /// Returns a [`ConfidenceError`] for an empty reason list.
    pub fn no_estimate(reasons: Vec<Reason>) -> Result<Self, ConfidenceError> {
        Self::with_reasons(State::NoEstimate, reasons)
    }

    fn with_reasons(state: State<S>, reasons: Vec<Reason>) -> Result<Self, ConfidenceError> {
        if reasons.is_empty() {
            return Err(Kind::NoReasons.into());
        }
        Ok(Self { state, reasons })
    }

    /// Returns `true` unless this is a "no estimate" state.
    #[must_use]
    pub fn is_estimate(&self) -> bool {
        matches!(self.state, State::Estimate { .. })
    }

    /// Returns the level, or `None` for a "no estimate" state.
    #[must_use]
    pub fn level(&self) -> Option<Level> {
        match self.state {
            State::Estimate { level, .. } => Some(level),
            State::NoEstimate => None,
        }
    }

    /// Returns the spread, if the estimate has one.
    #[must_use]
    pub fn spread(&self) -> Option<S> {
        match self.state {
            State::Estimate { spread, .. } => spread,
            State::NoEstimate => None,
        }
    }

    /// Returns the reasons, at least one.
    #[must_use]
    pub fn reasons(&self) -> &[Reason] {
        &self.reasons
    }
}

#[derive(Serialize)]
struct WireOut<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    level: Option<Level>,
    reasons: &'a [Reason],
    #[serde(skip_serializing_if = "Option::is_none")]
    spread: Option<f64>,
    estimate: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WireIn {
    level: Option<Level>,
    reasons: Vec<Reason>,
    spread: Option<f64>,
    estimate: bool,
}

impl<S: Spread> Serialize for Confidence<S> {
    fn serialize<Ser: Serializer>(&self, serializer: Ser) -> Result<Ser::Ok, Ser::Error> {
        WireOut {
            level: self.level(),
            reasons: &self.reasons,
            spread: self.spread().map(sealed::Spread::to_f64),
            estimate: self.is_estimate(),
        }
        .serialize(serializer)
    }
}

impl<'de, S: Spread> Deserialize<'de> for Confidence<S> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = WireIn::deserialize(deserializer)?;
        Self::from_wire(wire).map_err(de::Error::custom)
    }
}

impl<S: Spread> Confidence<S> {
    fn from_wire(wire: WireIn) -> Result<Self, ConfidenceError> {
        match (wire.estimate, wire.level) {
            (true, Some(level)) => {
                let spread = wire.spread.map(S::from_f64).transpose()?;
                Self::estimate(level, spread, wire.reasons)
            }
            (true, None) => Err(Kind::EstimateWithoutLevel.into()),
            (false, Some(_)) => Err(Kind::LevelWithoutEstimate.into()),
            (false, None) if wire.spread.is_some() => Err(Kind::SpreadWithoutEstimate.into()),
            (false, None) => Self::no_estimate(wire.reasons),
        }
    }
}

/// A [`Confidence`] or a [`Reason`] whose parts do not fit together.
///
/// The message names the cause and the fix.
#[derive(Clone, Debug, PartialEq)]
pub struct ConfidenceError(Kind);

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    NoReasons,
    Spread(RangeError),
    Signal(RangeError),
    SpreadNotAllowed,
    EstimateWithoutLevel,
    LevelWithoutEstimate,
    SpreadWithoutEstimate,
}

impl From<Kind> for ConfidenceError {
    fn from(kind: Kind) -> Self {
        Self(kind)
    }
}

impl fmt::Display for ConfidenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Kind::NoReasons => write!(
                f,
                "Confidence has no reasons: add at least one reason code with its signal value"
            ),
            Kind::Spread(error) | Kind::Signal(error) => write!(f, "{error}"),
            Kind::SpreadNotAllowed => write!(
                f,
                "Confidence has a spread, but this estimate has none: remove \"spread\""
            ),
            Kind::EstimateWithoutLevel => write!(
                f,
                "Confidence with \"estimate\": true has no level: \
                 add \"level\" or set \"estimate\": false"
            ),
            Kind::LevelWithoutEstimate => write!(
                f,
                "Confidence with \"estimate\": false has a level: \
                 remove \"level\" or set \"estimate\": true"
            ),
            Kind::SpreadWithoutEstimate => write!(
                f,
                "Confidence with \"estimate\": false has a spread: \
                 remove \"spread\" or set \"estimate\": true"
            ),
        }
    }
}

impl Error for ConfidenceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.0 {
            Kind::Spread(error) | Kind::Signal(error) => Some(error),
            _ => None,
        }
    }
}

/// A value with its confidence. The value is present exactly when the
/// confidence is an estimate.
///
/// ```
/// use sound_to_patch::confidence::{Estimate, Level, Reason, ReasonCode, Signal};
/// use sound_to_patch::units::Hz;
///
/// let frames = Reason::new(ReasonCode::ActiveFrames, Signal::Count(412))?;
/// let centroid = Estimate::new(Hz::new(1520.0)?, Level::High, Some(Hz::new(310.0)?), vec![frames])?;
/// assert_eq!(centroid.value().map(|hz| hz.get()), Some(1520.0));
///
/// let silent = Reason::new(ReasonCode::SilentStem, Signal::Flag(true))?;
/// let none = Estimate::<Hz, Hz>::none(vec![silent])?;
/// assert!(none.value().is_none());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Estimate<V, S: Spread> {
    value: Option<V>,
    confidence: Confidence<S>,
}

impl<V, S: Spread> Estimate<V, S> {
    /// A value with the confidence of an estimate.
    ///
    /// # Errors
    ///
    /// Returns a [`ConfidenceError`] as [`Confidence::estimate`] does.
    pub fn new(
        value: V,
        level: Level,
        spread: Option<S>,
        reasons: Vec<Reason>,
    ) -> Result<Self, ConfidenceError> {
        Ok(Self {
            value: Some(value),
            confidence: Confidence::estimate(level, spread, reasons)?,
        })
    }

    /// No value, and the reasons why.
    ///
    /// # Errors
    ///
    /// Returns a [`ConfidenceError`] as [`Confidence::no_estimate`] does.
    pub fn none(reasons: Vec<Reason>) -> Result<Self, ConfidenceError> {
        Ok(Self {
            value: None,
            confidence: Confidence::no_estimate(reasons)?,
        })
    }

    /// Returns the value, or `None` for no estimate.
    #[must_use]
    pub fn value(&self) -> Option<&V> {
        self.value.as_ref()
    }

    /// Returns the confidence.
    #[must_use]
    pub fn confidence(&self) -> &Confidence<S> {
        &self.confidence
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use serde_json::json;

    fn frames(count: u64) -> Reason {
        Reason::new(ReasonCode::ActiveFrames, Signal::Count(count)).unwrap()
    }

    fn octaves(value: f64) -> Octaves {
        Octaves::new(value).unwrap()
    }

    #[test]
    fn estimate_serializes_in_the_report_layout() {
        let confidence =
            Confidence::estimate(Level::Medium, Some(octaves(0.3)), vec![frames(412)]).unwrap();
        assert_eq!(
            serde_json::to_value(&confidence).unwrap(),
            json!({"level": "medium", "reasons": [{"code": "active_frames", "value": 412}], "spread": 0.3, "estimate": true})
        );
        assert_eq!(
            serde_json::to_string(&confidence).unwrap(),
            r#"{"level":"medium","reasons":[{"code":"active_frames","value":412}],"spread":0.3,"estimate":true}"#
        );
    }

    #[test]
    fn estimate_without_spread_leaves_the_field_out() {
        let confidence = Confidence::<Cents>::estimate(Level::High, None, vec![frames(1)]).unwrap();
        assert_eq!(
            serde_json::to_value(&confidence).unwrap(),
            json!({"level": "high", "reasons": [{"code": "active_frames", "value": 1}], "estimate": true})
        );
    }

    #[test]
    fn no_estimate_has_no_level_no_spread_and_a_reason() {
        let reason = Reason::new(ReasonCode::TooFewNotes, Signal::Count(2)).unwrap();
        let confidence = Confidence::<Octaves>::no_estimate(vec![reason]).unwrap();
        assert!(!confidence.is_estimate());
        assert_eq!(confidence.level(), None);
        assert_eq!(confidence.spread(), None);
        assert_eq!(confidence.reasons(), &[reason]);
        let value = serde_json::to_value(&confidence).unwrap();
        assert_eq!(
            value,
            json!({"reasons": [{"code": "too_few_notes", "value": 2}], "estimate": false})
        );
        let back: Confidence<Octaves> = serde_json::from_value(value).unwrap();
        assert_eq!(back, confidence);
    }

    #[test]
    fn level_reasons_and_signals_stay_together() {
        let reasons = vec![
            frames(412),
            Reason::new(ReasonCode::OctaveUncertain, Signal::Flag(true)).unwrap(),
        ];
        let confidence =
            Confidence::estimate(Level::Low, Some(octaves(1.5)), reasons.clone()).unwrap();
        assert!(confidence.is_estimate());
        assert_eq!(confidence.level(), Some(Level::Low));
        assert_eq!(confidence.spread(), Some(octaves(1.5)));
        assert_eq!(confidence.reasons(), reasons.as_slice());
        assert_eq!(confidence.reasons()[1].code(), ReasonCode::OctaveUncertain);
        assert_eq!(confidence.reasons()[1].value(), Signal::Flag(true));
    }

    #[test]
    fn every_state_needs_a_reason() {
        let error = Confidence::<Octaves>::estimate(Level::High, None, Vec::new()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Confidence has no reasons: add at least one reason code with its signal value"
        );
        assert!(error.source().is_none());
        assert!(Confidence::<Octaves>::no_estimate(Vec::new()).is_err());
        let json = json!({"reasons": [], "estimate": false});
        assert!(serde_json::from_value::<Confidence<Octaves>>(json).is_err());
    }

    #[test]
    fn spread_in_percent_serializes_as_a_number() {
        let spread = Percent::<crate::units::Unipolar>::new(12.5).unwrap();
        let confidence = Confidence::estimate(Level::Low, Some(spread), vec![frames(3)]).unwrap();
        let value = serde_json::to_value(&confidence).unwrap();
        assert_eq!(value["spread"], json!(12.5));
        assert_eq!(
            serde_json::from_value::<Confidence<_>>(value).unwrap(),
            confidence
        );
    }

    #[test]
    fn negative_spread_is_rejected() {
        let error =
            Confidence::estimate(Level::High, Some(octaves(-0.3)), vec![frames(1)]).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Confidence spread out of range: got -0.3, expected a finite value of at least 0.0"
        );
        assert!(error.source().is_some());
        assert!(Confidence::estimate(Level::High, Some(octaves(0.0)), vec![frames(1)]).is_ok());
    }

    #[test]
    fn nan_and_infinite_spread_cannot_be_built() {
        // The spread is a unit type, and every unit type rejects them.
        assert!(Octaves::new(f64::NAN).is_err());
        assert!(Octaves::new(f64::INFINITY).is_err());
        let json = r#"{"level": "high", "reasons": [{"code": "active_frames", "value": 1}], "spread": 1e999, "estimate": true}"#;
        assert!(serde_json::from_str::<Confidence<Octaves>>(json).is_err());
    }

    #[test]
    fn json_negative_or_out_of_range_spread_is_rejected() {
        let json = json!({"level": "high", "reasons": [{"code": "active_frames", "value": 1}], "spread": -0.3, "estimate": true});
        let message = serde_json::from_value::<Confidence<Octaves>>(json)
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("Confidence spread out of range"),
            "{message}"
        );
        let json = json!({"level": "high", "reasons": [{"code": "active_frames", "value": 1}], "spread": 1.5, "estimate": true});
        let message = serde_json::from_value::<Confidence<Normalized>>(json)
            .unwrap_err()
            .to_string();
        assert!(message.contains("Normalized out of range"), "{message}");
    }

    #[test]
    fn json_level_on_a_no_estimate_state_is_rejected() {
        let json = json!({"level": "high", "reasons": [{"code": "too_few_notes", "value": 2}], "estimate": false});
        assert_eq!(
            serde_json::from_value::<Confidence<Octaves>>(json)
                .unwrap_err()
                .to_string(),
            "Confidence with \"estimate\": false has a level: \
             remove \"level\" or set \"estimate\": true"
        );
    }

    #[test]
    fn json_spread_on_a_no_estimate_state_is_rejected() {
        let json = json!({"reasons": [{"code": "too_few_notes", "value": 2}], "spread": 0.3, "estimate": false});
        assert_eq!(
            serde_json::from_value::<Confidence<Octaves>>(json)
                .unwrap_err()
                .to_string(),
            "Confidence with \"estimate\": false has a spread: \
             remove \"spread\" or set \"estimate\": true"
        );
    }

    #[test]
    fn json_estimate_without_a_level_is_rejected() {
        let json = json!({"reasons": [{"code": "active_frames", "value": 412}], "estimate": true});
        assert_eq!(
            serde_json::from_value::<Confidence<Octaves>>(json)
                .unwrap_err()
                .to_string(),
            "Confidence with \"estimate\": true has no level: \
             add \"level\" or set \"estimate\": false"
        );
    }

    #[test]
    fn json_spread_for_an_estimate_without_spread_is_rejected() {
        let json = json!({"level": "low", "reasons": [{"code": "active_frames", "value": 1}], "spread": 0.3, "estimate": true});
        assert_eq!(
            serde_json::from_value::<Confidence<NoSpread>>(json)
                .unwrap_err()
                .to_string(),
            "Confidence has a spread, but this estimate has none: remove \"spread\""
        );
    }

    #[test]
    fn json_unknown_field_and_missing_estimate_are_rejected() {
        let json = json!({"level": "low", "reasons": [{"code": "active_frames", "value": 1}], "estimate": true, "score": 0.5});
        assert!(serde_json::from_value::<Confidence<Octaves>>(json).is_err());
        let json = json!({"level": "low", "reasons": [{"code": "active_frames", "value": 1}]});
        assert!(serde_json::from_value::<Confidence<Octaves>>(json).is_err());
        let json = json!({"code": "active_frames", "value": 1, "weight": 2});
        assert!(serde_json::from_value::<Reason>(json).is_err());
    }

    #[test]
    fn unknown_reason_code_and_level_are_rejected() {
        assert!(serde_json::from_value::<Reason>(json!({"code": "vibes", "value": 1})).is_err());
        assert!(serde_json::from_value::<Level>(json!("certain")).is_err());
    }

    #[test]
    fn signals_read_from_json_by_kind() {
        let read = |value| serde_json::from_value::<Signal>(value).unwrap();
        assert_eq!(read(json!(412)), Signal::Count(412));
        assert_eq!(read(json!(-3)), Signal::Number(-3.0));
        assert_eq!(read(json!(2.5)), Signal::Number(2.5));
        assert_eq!(read(json!(true)), Signal::Flag(true));
        assert!(serde_json::from_value::<Signal>(json!(i64::MIN)).is_err());
        assert_eq!(
            serde_json::from_value::<Signal>(json!("412"))
                .unwrap_err()
                .to_string(),
            "invalid type: string \"412\", expected a count, a finite number or a bool"
        );
        assert!(serde_json::from_str::<Signal>("1e999").is_err());
    }

    #[test]
    fn a_signal_must_be_finite() {
        let error = Reason::new(ReasonCode::ActiveFrames, Signal::Number(f64::NAN)).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Reason signal out of range: got NaN, expected a finite value"
        );
        assert!(error.source().is_some());
        assert!(Reason::new(ReasonCode::ActiveFrames, Signal::Number(f64::INFINITY)).is_err());
        assert!(Reason::new(ReasonCode::ActiveFrames, Signal::Number(-2.5)).is_ok());
    }

    #[test]
    fn every_reason_code_has_a_distinct_snake_case_name() {
        let names: Vec<String> = ReasonCode::ALL
            .iter()
            .map(|code| serde_json::to_string(code).unwrap())
            .collect();
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), names.len());
        assert!(names.contains(&"\"octave_uncertain\"".to_owned()));
    }

    #[test]
    fn levels_order_low_to_high() {
        assert!(Level::Low < Level::Medium && Level::Medium < Level::High);
        assert_eq!(Level::High.min(Level::Low), Level::Low);
    }

    /// Finite values of both signs, subnormals and zero included.
    fn finite() -> impl Strategy<Value = f64> {
        prop::num::f64::NORMAL | prop::num::f64::SUBNORMAL | prop::num::f64::ZERO
    }

    fn reason() -> impl Strategy<Value = Reason> {
        let code = prop::sample::select(ReasonCode::ALL);
        let signal = prop_oneof![
            any::<u64>().prop_map(Signal::Count),
            finite().prop_map(Signal::Number),
            any::<bool>().prop_map(Signal::Flag),
        ];
        (code, signal).prop_map(|(code, signal)| Reason::new(code, signal).unwrap())
    }

    fn confidence() -> impl Strategy<Value = Confidence<Cents>> {
        let level = prop_oneof![Just(Level::Low), Just(Level::Medium), Just(Level::High)];
        let spread = prop::option::of(finite().prop_map(|v| Cents::new(v.abs()).unwrap()));
        let reasons = prop::collection::vec(reason(), 1..4);
        (any::<bool>(), level, spread, reasons).prop_map(|(estimate, level, spread, reasons)| {
            if estimate {
                Confidence::estimate(level, spread, reasons).unwrap()
            } else {
                Confidence::no_estimate(reasons).unwrap()
            }
        })
    }

    proptest! {
        #[test]
        fn every_state_round_trips(confidence in confidence()) {
            let json = serde_json::to_string(&confidence).unwrap();
            let back: Confidence<Cents> = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(back, confidence);
        }
    }
}
