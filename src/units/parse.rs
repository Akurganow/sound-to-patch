//! Unit strings: `FromStr` and `Display` for the unit types. The grammar
//! and the suffix table are in the docs of the parent module.

use std::error::Error;
use std::fmt;
use std::str::FromStr;

use super::{Cents, Decibels, Hz, Octaves, Percent, Polarity, RangeError, Seconds, Semitones};

/// A unit string that does not parse into its unit type.
///
/// The message names the input, the target type, the cause and the
/// accepted suffixes:
/// `cannot read "1.2 s" as Hz: unknown suffix "s". Write a number followed by one of: Hz, kHz`.
#[derive(Clone, Debug, PartialEq)]
pub struct ParseUnitError {
    input: String,
    unit: &'static str,
    accepted: Vec<&'static str>,
    cause: Cause,
}

#[derive(Clone, Debug, PartialEq)]
enum Cause {
    NoNumber,
    NoSuffix,
    UnknownSuffix(String),
    BadNumber(String),
    Range(RangeError),
}

impl fmt::Display for ParseUnitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot read {:?} as {}: ", self.input, self.unit)?;
        match &self.cause {
            Cause::NoNumber => write!(f, "no number before the suffix")?,
            Cause::NoSuffix => write!(f, "no unit suffix")?,
            Cause::UnknownSuffix(suffix) => write!(f, "unknown suffix {suffix:?}")?,
            Cause::BadNumber(number) => write!(f, "{number:?} is not a number")?,
            Cause::Range(error) => return write!(f, "{error}"),
        }
        write!(
            f,
            ". Write a number followed by one of: {}",
            self.accepted.join(", ")
        )
    }
}

impl Error for ParseUnitError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.cause {
            Cause::Range(error) => Some(error),
            _ => None,
        }
    }
}

/// A suffix, the power of ten it scales the number by, and the constructor.
type Suffix<T> = (&'static str, i32, fn(f64) -> Result<T, RangeError>);

fn parse<T>(input: &str, unit: &'static str, suffixes: &[Suffix<T>]) -> Result<T, ParseUnitError> {
    let error = |cause| ParseUnitError {
        input: input.to_owned(),
        unit,
        accepted: suffixes.iter().map(|(suffix, _, _)| *suffix).collect(),
        cause,
    };
    let trimmed = input.trim();
    // Rust float syntax ends in a digit, a dot or an alphabetic `inf`/`NaN`
    // that then fails the range check, so the suffix is the trailing run of
    // letters and `%`.
    let suffix_start = trimmed
        .trim_end_matches(|c: char| c.is_alphabetic() || c == '%')
        .len();
    let (number, suffix) = trimmed.split_at(suffix_start);
    let number = number.trim_end();
    if number.is_empty() {
        return Err(error(Cause::NoNumber));
    }
    if suffix.is_empty() {
        return Err(error(Cause::NoSuffix));
    }
    let Some((_, shift, build)) = suffixes.iter().find(|(name, _, _)| *name == suffix) else {
        return Err(error(Cause::UnknownSuffix(suffix.to_owned())));
    };
    let value =
        parse_scaled(number, *shift).ok_or_else(|| error(Cause::BadNumber(number.to_owned())))?;
    build(value).map_err(|range| error(Cause::Range(range)))
}

/// Parses `number * 10^shift` by moving the decimal exponent in the text,
/// so the value rounds once: `"1.001 kHz"` equals `"1001 Hz"`, where
/// `1.001 * 1000.0` would be one ulp off.
fn parse_scaled(number: &str, shift: i32) -> Option<f64> {
    if shift == 0 {
        return f64::from_str(number).ok();
    }
    let (mantissa, exponent) = match number.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().ok()?),
        None => (number, 0),
    };
    f64::from_str(&format!("{mantissa}e{}", exponent.checked_add(shift)?)).ok()
}

/// Implements `FromStr` from a suffix table and `Display` with the first
/// suffix of the table.
macro_rules! unit_string {
    ($name:ident, $separator:literal, [$(($suffix:literal, $shift:literal, $build:expr)),+ $(,)?]) => {
        impl FromStr for $name {
            type Err = ParseUnitError;

            fn from_str(input: &str) -> Result<Self, ParseUnitError> {
                parse(input, stringify!($name), &[$(($suffix, $shift, $build)),+])
            }
        }

        impl fmt::Display for $name {
            /// Prints the shortest number that parses back to the same
            /// value, then the first suffix.
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let suffixes: &[&str] = &[$($suffix),+];
                f.pad(&format!("{}{}{}", self.get(), $separator, suffixes[0]))
            }
        }
    };
}

unit_string!(Hz, " ", [("Hz", 0, Hz::new), ("kHz", 3, Hz::new)]);
unit_string!(
    Seconds,
    " ",
    [("s", 0, Seconds::new), ("ms", -3, Seconds::new)]
);
unit_string!(Cents, " ", [("ct", 0, Cents::new)]);
unit_string!(Semitones, " ", [("st", 0, Semitones::new)]);
unit_string!(Octaves, " ", [("oct", 0, Octaves::new)]);
unit_string!(Decibels, " ", [("dB", 0, Decibels::new)]);

impl<P: Polarity> FromStr for Percent<P> {
    type Err = ParseUnitError;

    fn from_str(input: &str) -> Result<Self, ParseUnitError> {
        parse(input, "Percent", &[("%", 0, Self::new)])
    }
}

impl<P: Polarity> fmt::Display for Percent<P> {
    /// Prints the shortest number that parses back to the same value, then
    /// `%`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&format!("{}%", self.get()))
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::finite;
    use super::super::{Bipolar, Unipolar};
    use super::*;
    use approx::assert_relative_eq;
    use proptest::prelude::*;

    #[test]
    fn kilohertz_parse_to_hz() {
        let hz: Hz = "1.2 kHz".parse().unwrap();
        assert_relative_eq!(hz.get(), 1200.0, epsilon = 0.0, max_relative = 1e-12);
        assert_eq!("440 Hz".parse::<Hz>().unwrap().get(), 440.0);
        assert_eq!("440Hz".parse::<Hz>().unwrap().get(), 440.0);
        assert_eq!("  440 Hz ".parse::<Hz>().unwrap().get(), 440.0);
        assert_eq!("1e3 Hz".parse::<Hz>().unwrap().get(), 1000.0);
    }

    #[test]
    fn scaled_suffixes_round_once_like_the_base_unit() {
        let khz: Hz = "1.001 kHz".parse().unwrap();
        let hz: Hz = "1001 Hz".parse().unwrap();
        assert_eq!(khz, hz);
        assert_eq!(
            "1.2e-3 kHz".parse::<Hz>().unwrap(),
            "1.2 Hz".parse().unwrap()
        );
        assert_eq!(
            "1.2E1 kHz".parse::<Hz>().unwrap(),
            "12000 Hz".parse().unwrap()
        );
        let ms: Seconds = "1.001 ms".parse().unwrap();
        let s: Seconds = "0.001001 s".parse().unwrap();
        assert_eq!(ms, s);
        assert!("1e Hz".parse::<Hz>().is_err());
        assert!("1e2147483647 kHz".parse::<Hz>().is_err());
    }

    #[test]
    fn milliseconds_parse_to_seconds() {
        let seconds: Seconds = "800 ms".parse().unwrap();
        assert_relative_eq!(seconds.get(), 0.8, epsilon = 0.0, max_relative = 1e-12);
        assert_eq!("1.5 s".parse::<Seconds>().unwrap().get(), 1.5);
    }

    #[test]
    fn interval_level_and_percent_strings_parse() {
        assert_eq!("7 ct".parse::<Cents>().unwrap().get(), 7.0);
        assert_eq!("+12 st".parse::<Semitones>().unwrap().get(), 12.0);
        assert_eq!("-1 oct".parse::<Octaves>().unwrap().get(), -1.0);
        assert_eq!("-6 dB".parse::<Decibels>().unwrap().get(), -6.0);
        assert_eq!("25%".parse::<Percent<Unipolar>>().unwrap().get(), 25.0);
        assert_eq!("-40%".parse::<Percent<Bipolar>>().unwrap().get(), -40.0);
    }

    #[test]
    fn wrong_unit_unknown_suffix_and_missing_number_list_the_suffixes() {
        assert_eq!(
            "1.2 s".parse::<Hz>().unwrap_err().to_string(),
            "cannot read \"1.2 s\" as Hz: unknown suffix \"s\". \
             Write a number followed by one of: Hz, kHz"
        );
        assert_eq!(
            "1.2 parsecs".parse::<Hz>().unwrap_err().to_string(),
            "cannot read \"1.2 parsecs\" as Hz: unknown suffix \"parsecs\". \
             Write a number followed by one of: Hz, kHz"
        );
        assert_eq!(
            "kHz".parse::<Hz>().unwrap_err().to_string(),
            "cannot read \"kHz\" as Hz: no number before the suffix. \
             Write a number followed by one of: Hz, kHz"
        );
    }

    #[test]
    fn suffixes_are_case_sensitive() {
        assert!("1.2 khz".parse::<Hz>().is_err());
        assert!("-6 db".parse::<Decibels>().is_err());
        assert!("5 MS".parse::<Seconds>().is_err());
    }

    #[test]
    fn a_bare_number_is_an_error() {
        assert_eq!(
            "5".parse::<Cents>().unwrap_err().to_string(),
            "cannot read \"5\" as Cents: no unit suffix. Write a number followed by one of: ct"
        );
        assert!("5".parse::<Hz>().is_err());
        assert!("5".parse::<Percent<Unipolar>>().is_err());
        assert!("".parse::<Hz>().is_err());
    }

    #[test]
    fn a_bad_number_is_an_error() {
        assert_eq!(
            "1.2.3 Hz".parse::<Hz>().unwrap_err().to_string(),
            "cannot read \"1.2.3 Hz\" as Hz: \"1.2.3\" is not a number. \
             Write a number followed by one of: Hz, kHz"
        );
        assert!("1.2 k Hz".parse::<Hz>().is_err());
    }

    #[test]
    fn a_unit_string_passes_the_range_check() {
        let error = "-5 Hz".parse::<Hz>().unwrap_err();
        assert_eq!(
            error.to_string(),
            "cannot read \"-5 Hz\" as Hz: \
             Hz out of range: got -5.0, expected a finite value of at least 0.0"
        );
        assert!(error.source().is_some());
        assert!("NaN Hz".parse::<Hz>().is_err());
        assert!("inf Hz".parse::<Hz>().unwrap_err().source().is_some());
        assert!("inf dB".parse::<Decibels>().is_err());
        assert!("-40%".parse::<Percent<Unipolar>>().is_err());
        assert!("101%".parse::<Percent<Bipolar>>().is_err());
        assert!("-1 ms".parse::<Seconds>().is_err());
    }

    #[test]
    fn a_suffix_error_has_no_source() {
        assert!("5".parse::<Hz>().unwrap_err().source().is_none());
    }

    #[test]
    fn display_prints_the_first_suffix() {
        assert_eq!(Hz::new(1200.0).unwrap().to_string(), "1200 Hz");
        assert_eq!(Seconds::new(0.8).unwrap().to_string(), "0.8 s");
        assert_eq!(Cents::new(-7.5).unwrap().to_string(), "-7.5 ct");
        assert_eq!(Semitones::new(12.0).unwrap().to_string(), "12 st");
        assert_eq!(Octaves::new(-1.0).unwrap().to_string(), "-1 oct");
        assert_eq!(Decibels::new(-6.0).unwrap().to_string(), "-6 dB");
        assert_eq!(Percent::<Unipolar>::new(25.0).unwrap().to_string(), "25%");
    }

    #[test]
    fn display_honours_width_and_alignment() {
        assert_eq!(format!("{:>10}", Hz::new(440.0).unwrap()), "    440 Hz");
        assert_eq!(
            format!("{:<6}|", Percent::<Bipolar>::new(-5.0).unwrap()),
            "-5%   |"
        );
    }

    fn reparses<T>(value: T) -> Result<(), TestCaseError>
    where
        T: fmt::Display + FromStr<Err = ParseUnitError> + PartialEq + fmt::Debug,
    {
        prop_assert_eq!(value.to_string().parse::<T>().unwrap(), value);
        Ok(())
    }

    proptest! {
        #[test]
        fn display_parses_back_exactly(
            any in finite(),
            unipolar in 0.0..=100.0f64,
            bipolar in -100.0..=100.0f64,
        ) {
            reparses(Hz::new(any.abs()).unwrap())?;
            reparses(Seconds::new(any.abs()).unwrap())?;
            reparses(Cents::new(any).unwrap())?;
            reparses(Semitones::new(any).unwrap())?;
            reparses(Octaves::new(any).unwrap())?;
            reparses(Decibels::new(any).unwrap())?;
            reparses(Percent::<Unipolar>::new(unipolar).unwrap())?;
            reparses(Percent::<Bipolar>::new(bipolar).unwrap())?;
        }
    }
}
