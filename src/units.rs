//! Unit newtypes: a value is checked once, when it is built, and is valid by
//! type afterwards.
//!
//! Every type holds an `f64` that is never NaN or infinite. Each constructor
//! rejects a value outside the type's range with a [`RangeError`] that names
//! the type and the range. Zero is stored as `+0.0`, so `-0.0` never reaches
//! a report.
//!
//! - [`Hz`] and [`Seconds`]: zero or more.
//! - [`Semitones`], [`Cents`], [`Octaves`] and [`Decibels`]: any finite value.
//! - [`Normalized`]: a knob position, 0.0 to 1.0.
//! - [`MidiPitch`]: a fractional MIDI note, 0.0 to 127.0.
//! - [`Percent`]: 0 to 100 as `Percent<Unipolar>`, -100 to 100 as
//!   `Percent<Bipolar>`.
//!
//! A JSON number deserializes through the same check, so a value from
//! `layers.json` is valid by type too.
//!
//! A unit string such as `"1.2 kHz"` parses with [`str::parse`]: a number,
//! optional whitespace and a suffix. Each type takes only its own suffixes,
//! matched case-sensitively. A bare number, an unknown suffix and a suffix
//! of another unit return a [`ParseUnitError`] that lists the accepted
//! suffixes. [`Display`](fmt::Display) prints the first suffix in a form
//! that parses back to the same value.
//!
//! | Type | Suffixes |
//! | :-- | :-- |
//! | [`Hz`] | `Hz`, `kHz` |
//! | [`Seconds`] | `s`, `ms` |
//! | [`Cents`] | `ct` |
//! | [`Semitones`] | `st` |
//! | [`Octaves`] | `oct` |
//! | [`Decibels`] | `dB` |
//! | [`Percent`] | `%` |
//!
//! ```
//! use sound_to_patch::units::{Bipolar, Hz, Percent, Seconds};
//!
//! let cutoff: Hz = "1.2 kHz".parse()?;
//! let pre_delay: Seconds = "20 ms".parse()?;
//! let amount: Percent<Bipolar> = "-40%".parse()?;
//! assert_eq!(cutoff.to_string(), "1200 Hz");
//! assert!("1.2 s".parse::<Hz>().is_err());
//! # Ok::<(), sound_to_patch::units::ParseUnitError>(())
//! ```
//!
//! A conversion between two unit types is a [`From`] impl when every input
//! has a valid output, such as [`Hz::from`] a [`MidiPitch`], and a
//! [`TryFrom`] impl otherwise, such as [`MidiPitch::try_from`] a [`Hz`].
//! A ratio or a gain has no unit type and goes through named methods:
//! [`Cents::to_ratio`], [`Cents::from_ratio`], [`Semitones::to_ratio`],
//! [`Octaves::to_ratio`], [`Decibels::to_amplitude_gain`],
//! [`Decibels::from_amplitude_gain`], [`Decibels::to_power_ratio`] and
//! [`Decibels::from_power_ratio`].
//!
//! No `Milliseconds` type exists: [`Seconds::from_millis`] converts at the
//! boundary. No unit type exists for a knob whose law is not measured, such
//! as the filter cutoff: such a knob takes a [`Normalized`] position.

use std::error::Error;
use std::fmt;
use std::marker::PhantomData;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

mod convert;
mod parse;

pub use parse::ParseUnitError;

/// A value outside the range a unit type or a conversion accepts.
///
/// The message names what failed, the value and the range:
/// `Normalized out of range: got 1.5, expected 0.0 to 1.0`.
#[derive(Clone, Debug, PartialEq)]
pub struct RangeError {
    what: &'static str,
    value: f64,
    range: Range,
}

impl RangeError {
    pub(crate) fn new(what: &'static str, value: f64, range: Range) -> Self {
        Self { what, value, range }
    }
}

impl fmt::Display for RangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `{:?}` keeps the decimal point: `1.0`, not `1`.
        write!(
            f,
            "{} out of range: got {:?}, expected {}",
            self.what, self.value, self.range
        )
    }
}

impl Error for RangeError {}

/// The values a check accepts. Every range excludes NaN and both infinities.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Range {
    Finite,
    AtLeast(f64),
    Above(f64),
    Closed(f64, f64),
}

impl Range {
    pub(crate) fn contains(self, value: f64) -> bool {
        value.is_finite()
            && match self {
                Self::Finite => true,
                Self::AtLeast(min) => value >= min,
                Self::Above(min) => value > min,
                Self::Closed(min, max) => (min..=max).contains(&value),
            }
    }

    /// Returns the value, with `-0.0` stored as `+0.0`, or the error naming
    /// `what`.
    pub(crate) fn check(self, what: &'static str, value: f64) -> Result<f64, RangeError> {
        if self.contains(value) {
            Ok(value + 0.0)
        } else {
            Err(RangeError::new(what, value, self))
        }
    }
}

impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Finite => write!(f, "a finite value"),
            Self::AtLeast(min) => write!(f, "a finite value of at least {min:?}"),
            Self::Above(min) => write!(f, "a finite value above {min:?}"),
            Self::Closed(min, max) => write!(f, "{min:?} to {max:?}"),
        }
    }
}

/// Defines a unit newtype over a private `f64` with a checked constructor,
/// a getter and serde through the constructor.
macro_rules! unit {
    ($(#[$doc:meta])* $name:ident, $range:expr) => {
        $(#[$doc])*
        #[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize, Deserialize)]
        #[serde(try_from = "f64", into = "f64")]
        pub struct $name(f64);

        impl $name {
            const RANGE: Range = $range;

            /// Checks the value against the range of the type.
            ///
            /// # Errors
            ///
            /// Returns a [`RangeError`] for NaN, an infinity or a value
            /// outside the range.
            pub fn new(value: f64) -> Result<Self, RangeError> {
                Self::RANGE.check(stringify!($name), value).map(Self)
            }

            /// Returns the inner value.
            #[must_use]
            pub fn get(self) -> f64 {
                self.0
            }
        }

        impl TryFrom<f64> for $name {
            type Error = RangeError;

            fn try_from(value: f64) -> Result<Self, RangeError> {
                Self::new(value)
            }
        }

        impl From<$name> for f64 {
            fn from(value: $name) -> f64 {
                value.0
            }
        }
    };
}

unit!(
    /// A frequency in hertz: a finite value of zero or more.
    ///
    /// ```
    /// use sound_to_patch::units::Hz;
    ///
    /// let a4 = Hz::new(440.0)?;
    /// assert!(Hz::new(-1.0).is_err());
    /// # Ok::<(), sound_to_patch::units::RangeError>(())
    /// ```
    Hz,
    Range::AtLeast(0.0)
);

unit!(
    /// A duration in seconds: a finite value of zero or more.
    ///
    /// ```
    /// use sound_to_patch::units::Seconds;
    ///
    /// let attack = Seconds::new(0.25)?;
    /// let pre_delay = Seconds::from_millis(20.0)?;
    /// assert!(Seconds::new(-0.1).is_err());
    /// # Ok::<(), sound_to_patch::units::RangeError>(())
    /// ```
    Seconds,
    Range::AtLeast(0.0)
);

unit!(
    /// An interval in semitones: any finite value, sign allowed.
    ///
    /// ```
    /// use sound_to_patch::units::Semitones;
    ///
    /// let octave_down = Semitones::new(-12.0)?;
    /// assert!(Semitones::new(f64::NAN).is_err());
    /// # Ok::<(), sound_to_patch::units::RangeError>(())
    /// ```
    Semitones,
    Range::Finite
);

unit!(
    /// An interval in cents, a hundredth of a semitone: any finite value,
    /// sign allowed.
    ///
    /// ```
    /// use sound_to_patch::units::Cents;
    ///
    /// let detune = Cents::new(-7.0)?;
    /// assert!(Cents::new(f64::INFINITY).is_err());
    /// # Ok::<(), sound_to_patch::units::RangeError>(())
    /// ```
    Cents,
    Range::Finite
);

unit!(
    /// An interval in octaves: any finite value, sign allowed.
    ///
    /// ```
    /// use sound_to_patch::units::Octaves;
    ///
    /// let spread = Octaves::new(0.3)?;
    /// assert!(Octaves::new(f64::NEG_INFINITY).is_err());
    /// # Ok::<(), sound_to_patch::units::RangeError>(())
    /// ```
    Octaves,
    Range::Finite
);

unit!(
    /// A level in decibels: any finite value. A negative value is common,
    /// because levels are often relative to a peak.
    ///
    /// ```
    /// use sound_to_patch::units::Decibels;
    ///
    /// let level = Decibels::new(-6.0)?;
    /// assert!(Decibels::new(f64::NAN).is_err());
    /// # Ok::<(), sound_to_patch::units::RangeError>(())
    /// ```
    Decibels,
    Range::Finite
);

unit!(
    /// A knob position from 0.0 to 1.0 inclusive.
    ///
    /// A value just above 1.0 from float rounding is rejected, not clamped.
    ///
    /// ```
    /// use sound_to_patch::units::Normalized;
    ///
    /// let cutoff_knob = Normalized::new(0.5)?;
    /// assert!(Normalized::new(1.000_000_1).is_err());
    /// # Ok::<(), sound_to_patch::units::RangeError>(())
    /// ```
    Normalized,
    Range::Closed(0.0, 1.0)
);

unit!(
    /// A MIDI note number from 0.0 to 127.0 inclusive. Fractional values
    /// carry a pitch between notes, such as a pitch track gives.
    ///
    /// ```
    /// use sound_to_patch::units::MidiPitch;
    ///
    /// let a4 = MidiPitch::new(69.0)?;
    /// let bent = MidiPitch::new(69.25)?;
    /// assert!(MidiPitch::new(127.5).is_err());
    /// # Ok::<(), sound_to_patch::units::RangeError>(())
    /// ```
    MidiPitch,
    Range::Closed(0.0, 127.0)
);

impl Seconds {
    /// Converts milliseconds, such as a reverb pre-delay, to seconds.
    ///
    /// # Errors
    ///
    /// Returns a [`RangeError`] when the result is not a valid [`Seconds`].
    pub fn from_millis(millis: f64) -> Result<Self, RangeError> {
        Self::new(millis / 1000.0)
    }
}

/// A percentage. The polarity fixes the range: `Percent<Unipolar>` takes 0 to
/// 100, `Percent<Bipolar>` takes -100 to 100.
///
/// A unipolar value widens to a bipolar one with [`From`]; the reverse is a
/// [`TryFrom`].
///
/// ```
/// use sound_to_patch::units::{Bipolar, Percent, Unipolar};
///
/// let drive = Percent::<Unipolar>::new(25.0)?;
/// let amount = Percent::<Bipolar>::new(-100.0)?;
/// assert!(Percent::<Unipolar>::new(-1.0).is_err());
/// assert!(Percent::<Bipolar>::new(-101.0).is_err());
/// # Ok::<(), sound_to_patch::units::RangeError>(())
/// ```
pub struct Percent<P: Polarity> {
    value: f64,
    polarity: PhantomData<P>,
}

/// The range of a [`Percent`]: [`Unipolar`] or [`Bipolar`].
pub trait Polarity: sealed::Polarity {}

/// 0 to 100 percent, for controls that go one way from zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unipolar {}

/// -100 to 100 percent, for controls that go both ways, such as a
/// modulation amount.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bipolar {}

mod sealed {
    pub trait Polarity {
        const MIN: f64;
    }
}

impl sealed::Polarity for Unipolar {
    const MIN: f64 = 0.0;
}

impl sealed::Polarity for Bipolar {
    const MIN: f64 = -100.0;
}

impl Polarity for Unipolar {}
impl Polarity for Bipolar {}

impl<P: Polarity> Percent<P> {
    /// Checks the value against the range of the polarity.
    ///
    /// # Errors
    ///
    /// Returns a [`RangeError`] for NaN, an infinity or a value outside the
    /// range.
    pub fn new(value: f64) -> Result<Self, RangeError> {
        Range::Closed(P::MIN, 100.0)
            .check("Percent", value)
            .map(|value| Self {
                value,
                polarity: PhantomData,
            })
    }

    /// Returns the inner value, in percent.
    #[must_use]
    pub fn get(self) -> f64 {
        self.value
    }
}

// Derives would bound `P` itself, which never needs these traits.
impl<P: Polarity> Clone for Percent<P> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<P: Polarity> Copy for Percent<P> {}

impl<P: Polarity> fmt::Debug for Percent<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Percent").field(&self.value).finish()
    }
}

impl<P: Polarity> PartialEq for Percent<P> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<P: Polarity> PartialOrd for Percent<P> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.value.partial_cmp(&other.value)
    }
}

impl<P: Polarity> TryFrom<f64> for Percent<P> {
    type Error = RangeError;

    fn try_from(value: f64) -> Result<Self, RangeError> {
        Self::new(value)
    }
}

impl<P: Polarity> From<Percent<P>> for f64 {
    fn from(value: Percent<P>) -> f64 {
        value.value
    }
}

impl From<Percent<Unipolar>> for Percent<Bipolar> {
    fn from(value: Percent<Unipolar>) -> Self {
        Self {
            value: value.value,
            polarity: PhantomData,
        }
    }
}

impl TryFrom<Percent<Bipolar>> for Percent<Unipolar> {
    type Error = RangeError;

    fn try_from(value: Percent<Bipolar>) -> Result<Self, RangeError> {
        Self::new(value.value)
    }
}

impl<P: Polarity> Serialize for Percent<P> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(self.value)
    }
}

impl<'de, P: Polarity> Deserialize<'de> for Percent<P> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = f64::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use proptest::prelude::*;

    const NON_FINITE: [f64; 3] = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY];

    fn all_reject(value: f64) -> [bool; 10] {
        [
            Hz::new(value).is_err(),
            Seconds::new(value).is_err(),
            Semitones::new(value).is_err(),
            Cents::new(value).is_err(),
            Octaves::new(value).is_err(),
            Decibels::new(value).is_err(),
            Normalized::new(value).is_err(),
            MidiPitch::new(value).is_err(),
            Percent::<Unipolar>::new(value).is_err(),
            Percent::<Bipolar>::new(value).is_err(),
        ]
    }

    #[test]
    fn every_type_rejects_nan_and_both_infinities() {
        for value in NON_FINITE {
            assert_eq!(all_reject(value), [true; 10], "{value:?}");
        }
    }

    #[test]
    fn every_type_accepts_zero() {
        assert_eq!(all_reject(0.0), [false; 10]);
    }

    #[test]
    fn negative_zero_is_stored_as_positive_zero() {
        assert!(Hz::new(-0.0).unwrap().get().is_sign_positive());
        assert!(Cents::new(-0.0).unwrap().get().is_sign_positive());
        assert!(
            Percent::<Bipolar>::new(-0.0)
                .unwrap()
                .get()
                .is_sign_positive()
        );
    }

    #[test]
    fn hz_rejects_negative_and_nan_naming_type_and_range() {
        assert_eq!(
            Hz::new(-1.0).unwrap_err().to_string(),
            "Hz out of range: got -1.0, expected a finite value of at least 0.0"
        );
        assert_eq!(
            Hz::new(f64::NAN).unwrap_err().to_string(),
            "Hz out of range: got NaN, expected a finite value of at least 0.0"
        );
    }

    #[test]
    fn seconds_rejects_negative() {
        assert!(Seconds::new(-f64::MIN_POSITIVE).is_err());
        assert!(Seconds::new(0.0).is_ok());
    }

    #[test]
    fn normalized_accepts_both_ends_and_rejects_rounding_overshoot() {
        assert_eq!(Normalized::new(0.0).unwrap().get(), 0.0);
        assert_eq!(Normalized::new(1.0).unwrap().get(), 1.0);
        assert!(Normalized::new(1.000_000_1).is_err());
        assert!(Normalized::new(-f64::MIN_POSITIVE).is_err());
        assert_eq!(
            Normalized::new(1.5).unwrap_err().to_string(),
            "Normalized out of range: got 1.5, expected 0.0 to 1.0"
        );
    }

    #[test]
    fn unipolar_percent_takes_0_to_100() {
        assert!(Percent::<Unipolar>::new(0.0).is_ok());
        assert!(Percent::<Unipolar>::new(100.0).is_ok());
        assert!(Percent::<Unipolar>::new(101.0).is_err());
        assert!(Percent::<Unipolar>::new(-1.0).is_err());
        assert_eq!(
            Percent::<Unipolar>::new(101.0).unwrap_err().to_string(),
            "Percent out of range: got 101.0, expected 0.0 to 100.0"
        );
    }

    #[test]
    fn bipolar_percent_takes_minus_100_to_100() {
        assert!(Percent::<Bipolar>::new(-100.0).is_ok());
        assert!(Percent::<Bipolar>::new(100.0).is_ok());
        assert!(Percent::<Bipolar>::new(-101.0).is_err());
        assert!(Percent::<Bipolar>::new(101.0).is_err());
    }

    #[test]
    fn percent_widens_to_bipolar_and_narrows_with_a_check() {
        let unipolar = Percent::<Unipolar>::new(25.0).unwrap();
        let bipolar = Percent::<Bipolar>::from(unipolar);
        assert_eq!(bipolar.get(), 25.0);
        assert_eq!(Percent::<Unipolar>::try_from(bipolar).unwrap(), unipolar);
        let negative = Percent::<Bipolar>::new(-40.0).unwrap();
        assert!(Percent::<Unipolar>::try_from(negative).is_err());
    }

    #[test]
    fn percent_compares_and_prints_by_value() {
        let low = Percent::<Unipolar>::new(10.0).unwrap();
        let high = Percent::<Unipolar>::new(20.0).unwrap();
        assert!(low < high);
        assert_ne!(low, high);
        assert_eq!(low.clone(), low);
        assert_eq!(format!("{low:?}"), "Percent(10.0)");
    }

    #[test]
    fn signed_units_take_both_signs() {
        assert!(Decibels::new(-6.0).is_ok());
        assert!(Semitones::new(-12.0).is_ok());
        assert!(Cents::new(7.0).is_ok());
        assert!(Octaves::new(-1.0).is_ok());
        assert!(Decibels::new(f64::MAX).is_ok());
        assert!(Decibels::new(f64::MIN).is_ok());
    }

    #[test]
    fn midi_pitch_takes_0_to_127() {
        assert!(MidiPitch::new(0.0).is_ok());
        assert!(MidiPitch::new(127.0).is_ok());
        assert!(MidiPitch::new(127.5).is_err());
        assert!(MidiPitch::new(-0.5).is_err());
    }

    #[test]
    fn millis_convert_to_seconds() {
        assert_eq!(Seconds::from_millis(250.0).unwrap().get(), 0.25);
        assert!(Seconds::from_millis(-1.0).is_err());
    }

    #[test]
    fn try_from_f64_and_back() {
        assert_eq!(f64::from(Hz::try_from(440.0).unwrap()), 440.0);
        assert!(Hz::try_from(-1.0).is_err());
        assert_eq!(f64::from(Percent::<Bipolar>::try_from(-5.0).unwrap()), -5.0);
    }

    #[test]
    fn json_number_out_of_range_fails_with_type_and_range() {
        for (input, value) in [("1.5", "1.5"), ("-0.1", "-0.1")] {
            let message = serde_json::from_str::<Normalized>(input)
                .unwrap_err()
                .to_string();
            assert_eq!(
                message,
                format!("Normalized out of range: got {value}, expected 0.0 to 1.0")
            );
        }
        let message = serde_json::from_str::<Percent<Unipolar>>("101")
            .unwrap_err()
            .to_string();
        assert_eq!(
            message,
            "Percent out of range: got 101.0, expected 0.0 to 100.0"
        );
    }

    #[test]
    fn json_string_for_a_number_field_fails_with_serde_type_message() {
        let message = serde_json::from_str::<Normalized>("\"0.5\"")
            .unwrap_err()
            .to_string();
        assert!(
            message.starts_with("invalid type: string \"0.5\""),
            "{message}"
        );
        assert!(serde_json::from_str::<Percent<Bipolar>>("\"5\"").is_err());
    }

    // serde_json reads an integer into a float field; a change there shows up
    // here.
    #[test]
    fn json_integer_reads_into_a_float_field() {
        assert_eq!(serde_json::from_str::<Normalized>("1").unwrap().get(), 1.0);
        assert_eq!(serde_json::from_str::<Hz>("440").unwrap().get(), 440.0);
        assert_eq!(
            serde_json::from_str::<Percent<Bipolar>>("-40")
                .unwrap()
                .get(),
            -40.0
        );
    }

    #[test]
    fn json_writes_a_plain_number() {
        let hz = Hz::new(440.5).unwrap();
        assert_eq!(serde_json::to_string(&hz).unwrap(), "440.5");
        let percent = Percent::<Bipolar>::new(-40.0).unwrap();
        assert_eq!(serde_json::to_string(&percent).unwrap(), "-40.0");
    }

    fn round_trips<T>(value: T) -> Result<(), TestCaseError>
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + fmt::Debug,
    {
        let json = serde_json::to_string(&value).unwrap();
        prop_assert_eq!(serde_json::from_str::<T>(&json).unwrap(), value);
        Ok(())
    }

    /// Finite values of both signs, subnormals and zero included.
    pub(crate) fn finite() -> impl Strategy<Value = f64> {
        prop::num::f64::NORMAL | prop::num::f64::SUBNORMAL | prop::num::f64::ZERO
    }

    proptest! {
        #[test]
        fn json_round_trip_is_exact(
            any in finite(),
            unit in 0.0..=1.0f64,
            midi in 0.0..=127.0f64,
            unipolar in 0.0..=100.0f64,
            bipolar in -100.0..=100.0f64,
        ) {
            let magnitude = any.abs();
            round_trips(Hz::new(magnitude).unwrap())?;
            round_trips(Seconds::new(magnitude).unwrap())?;
            round_trips(Semitones::new(any).unwrap())?;
            round_trips(Cents::new(any).unwrap())?;
            round_trips(Octaves::new(any).unwrap())?;
            round_trips(Decibels::new(any).unwrap())?;
            round_trips(Normalized::new(unit).unwrap())?;
            round_trips(MidiPitch::new(midi).unwrap())?;
            round_trips(Percent::<Unipolar>::new(unipolar).unwrap())?;
            round_trips(Percent::<Bipolar>::new(bipolar).unwrap())?;
        }
    }
}
