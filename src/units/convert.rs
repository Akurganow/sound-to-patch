//! Conversions between units, and between units and plain ratios.
//!
//! A conversion between two unit types is a [`From`] impl when every input
//! has a valid output, and a [`TryFrom`] impl otherwise. A ratio or a gain
//! has no unit type, so it goes in and out through named methods. A method
//! that raises 2 or 10 to a power checks its input first, so the ratio is
//! never infinite or zero.

use super::{Cents, Decibels, Hz, MidiPitch, Octaves, Range, RangeError, Semitones};

/// MIDI note 69 is A4.
const A4_MIDI: f64 = 69.0;
const A4_HZ: f64 = 440.0;

/// Keeps `2^octaves` and `10^(dB / 10)` between `1e-300` and `1e300`, well
/// inside the normal `f64` range.
const MAX_OCTAVES: f64 = 1000.0;
const MAX_POWER_DB: f64 = 3000.0;

impl From<MidiPitch> for Hz {
    /// Converts with equal temperament, A4 = 440 Hz:
    /// `f = 440 * 2^((m - 69) / 12)`. Every MIDI pitch gives a valid
    /// frequency, from about 8.18 Hz to about 12544 Hz.
    fn from(pitch: MidiPitch) -> Self {
        Self(A4_HZ * ((pitch.get() - A4_MIDI) / 12.0).exp2())
    }
}

impl TryFrom<Hz> for MidiPitch {
    type Error = RangeError;

    /// Converts with equal temperament, A4 = 440 Hz:
    /// `m = 69 + 12 * log2(f / 440)`.
    ///
    /// # Errors
    ///
    /// Returns a [`RangeError`] for 0 Hz, which has no pitch, and for a
    /// frequency outside MIDI notes 0 to 127.
    fn try_from(frequency: Hz) -> Result<Self, RangeError> {
        let hz = Range::Above(0.0).check("Hz to MidiPitch", frequency.get())?;
        Self::new(A4_MIDI + 12.0 * (hz / A4_HZ).log2())
    }
}

impl TryFrom<u8> for MidiPitch {
    type Error = RangeError;

    /// Converts a note number from a MIDI file.
    ///
    /// # Errors
    ///
    /// Returns a [`RangeError`] for a number above 127.
    fn try_from(note: u8) -> Result<Self, RangeError> {
        Self::new(f64::from(note))
    }
}

impl Cents {
    /// Returns the frequency ratio `2^(c / 1200)`.
    ///
    /// ```
    /// use sound_to_serum::units::Cents;
    ///
    /// assert_eq!(Cents::new(1200.0)?.to_ratio()?, 2.0);
    /// # Ok::<(), sound_to_serum::units::RangeError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns a [`RangeError`] beyond ±1 200 000 cents, where the ratio
    /// would leave the normal `f64` range.
    pub fn to_ratio(self) -> Result<f64, RangeError> {
        let cents = Range::Closed(-MAX_OCTAVES * 1200.0, MAX_OCTAVES * 1200.0)
            .check("Cents to ratio", self.get())?;
        Ok((cents / 1200.0).exp2())
    }

    /// Converts a frequency ratio: `c = 1200 * log2(ratio)`.
    ///
    /// # Errors
    ///
    /// Returns a [`RangeError`] for a ratio of zero or less, or one that is
    /// not finite.
    pub fn from_ratio(ratio: f64) -> Result<Self, RangeError> {
        let ratio = Range::Above(0.0).check("ratio to Cents", ratio)?;
        Self::new(1200.0 * ratio.log2())
    }
}

impl Semitones {
    /// Returns the frequency ratio `2^(s / 12)`.
    ///
    /// # Errors
    ///
    /// Returns a [`RangeError`] beyond ±12 000 semitones, where the ratio
    /// would leave the normal `f64` range.
    pub fn to_ratio(self) -> Result<f64, RangeError> {
        let semitones = Range::Closed(-MAX_OCTAVES * 12.0, MAX_OCTAVES * 12.0)
            .check("Semitones to ratio", self.get())?;
        Ok((semitones / 12.0).exp2())
    }
}

impl Octaves {
    /// Returns the frequency ratio `2^o`.
    ///
    /// # Errors
    ///
    /// Returns a [`RangeError`] beyond ±1000 octaves, where the ratio would
    /// leave the normal `f64` range.
    pub fn to_ratio(self) -> Result<f64, RangeError> {
        let octaves =
            Range::Closed(-MAX_OCTAVES, MAX_OCTAVES).check("Octaves to ratio", self.get())?;
        Ok(octaves.exp2())
    }
}

impl Decibels {
    /// Returns the amplitude gain `10^(dB / 20)`, the factor for a
    /// magnitude such as a sample value or a spectral magnitude weight.
    ///
    /// ```
    /// use sound_to_serum::units::Decibels;
    ///
    /// let gain = Decibels::new(-20.0)?.to_amplitude_gain()?;
    /// assert!((gain - 0.1).abs() < 1e-15);
    /// # Ok::<(), sound_to_serum::units::RangeError>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns a [`RangeError`] beyond ±6000 dB, where the gain would leave
    /// the normal `f64` range.
    pub fn to_amplitude_gain(self) -> Result<f64, RangeError> {
        let db = Range::Closed(-2.0 * MAX_POWER_DB, 2.0 * MAX_POWER_DB)
            .check("Decibels to amplitude gain", self.get())?;
        Ok(10_f64.powf(db / 20.0))
    }

    /// Converts an amplitude gain: `dB = 20 * log10(gain)`.
    ///
    /// # Errors
    ///
    /// Returns a [`RangeError`] for a gain of zero or less, which has no
    /// decibel value, or one that is not finite.
    pub fn from_amplitude_gain(gain: f64) -> Result<Self, RangeError> {
        let gain = Range::Above(0.0).check("amplitude gain to Decibels", gain)?;
        Self::new(20.0 * gain.log10())
    }

    /// Returns the power ratio `10^(dB / 10)`, the factor for an energy or a
    /// power, such as the share of a frequency band.
    ///
    /// # Errors
    ///
    /// Returns a [`RangeError`] beyond ±3000 dB, where the ratio would leave
    /// the normal `f64` range.
    pub fn to_power_ratio(self) -> Result<f64, RangeError> {
        let db = Range::Closed(-MAX_POWER_DB, MAX_POWER_DB)
            .check("Decibels to power ratio", self.get())?;
        Ok(10_f64.powf(db / 10.0))
    }

    /// Converts a power ratio: `dB = 10 * log10(ratio)`.
    ///
    /// # Errors
    ///
    /// Returns a [`RangeError`] for a ratio of zero or less, or one that is
    /// not finite.
    pub fn from_power_ratio(ratio: f64) -> Result<Self, RangeError> {
        let ratio = Range::Above(0.0).check("power ratio to Decibels", ratio)?;
        Self::new(10.0 * ratio.log10())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use proptest::prelude::*;

    // Anchors: the MIDI tuning standard puts note 69 at 440 Hz, and each
    // octave (12 notes) doubles the frequency.
    #[test]
    fn midi_anchors_convert_to_hz() {
        for (note, hz) in [(69.0, 440.0), (57.0, 220.0), (81.0, 880.0), (21.0, 27.5)] {
            let got = Hz::from(MidiPitch::new(note).unwrap()).get();
            assert_relative_eq!(got, hz, epsilon = 0.0, max_relative = 1e-15);
        }
    }

    #[test]
    fn hz_anchors_convert_to_midi() {
        for (hz, note) in [(440.0, 69.0), (220.0, 57.0), (880.0, 81.0), (27.5, 21.0)] {
            let got = MidiPitch::try_from(Hz::new(hz).unwrap()).unwrap().get();
            assert_relative_eq!(got, note, epsilon = 1e-12, max_relative = 0.0);
        }
    }

    #[test]
    fn midi_range_ends_convert_to_valid_hz() {
        let lowest = Hz::from(MidiPitch::new(0.0).unwrap()).get();
        let highest = Hz::from(MidiPitch::new(127.0).unwrap()).get();
        assert_relative_eq!(
            lowest,
            440.0 / 2_f64.powf(69.0 / 12.0),
            epsilon = 0.0,
            max_relative = 1e-15
        );
        assert_relative_eq!(
            highest,
            440.0 * 2_f64.powf(58.0 / 12.0),
            epsilon = 0.0,
            max_relative = 1e-15
        );
    }

    #[test]
    fn zero_and_out_of_range_hz_have_no_midi_pitch() {
        assert_eq!(
            MidiPitch::try_from(Hz::new(0.0).unwrap())
                .unwrap_err()
                .to_string(),
            "Hz to MidiPitch out of range: got 0.0, expected a finite value above 0.0"
        );
        let message = MidiPitch::try_from(Hz::new(20_000.0).unwrap())
            .unwrap_err()
            .to_string();
        assert!(
            message.starts_with("MidiPitch out of range: got 135.0"),
            "{message}"
        );
        assert!(MidiPitch::try_from(Hz::new(8.0).unwrap()).is_err());
    }

    #[test]
    fn negative_hz_has_no_midi_pitch() {
        // `Hz` itself rejects a negative value, so no negative frequency
        // reaches the conversion.
        assert!(Hz::new(-440.0).is_err());
    }

    #[test]
    fn midi_note_numbers_above_127_are_rejected() {
        assert_eq!(MidiPitch::try_from(127_u8).unwrap().get(), 127.0);
        assert_eq!(MidiPitch::try_from(0_u8).unwrap().get(), 0.0);
        assert_eq!(
            MidiPitch::try_from(128_u8).unwrap_err().to_string(),
            "MidiPitch out of range: got 128.0, expected 0.0 to 127.0"
        );
    }

    // Anchors: an octave is a ratio of 2, 12 semitones or 1200 cents.
    #[test]
    fn interval_anchors_convert_to_ratios() {
        assert_eq!(Cents::new(1200.0).unwrap().to_ratio().unwrap(), 2.0);
        assert_eq!(Cents::new(0.0).unwrap().to_ratio().unwrap(), 1.0);
        assert_eq!(Cents::new(-1200.0).unwrap().to_ratio().unwrap(), 0.5);
        assert_eq!(Octaves::new(1.0).unwrap().to_ratio().unwrap(), 2.0);
        assert_eq!(Semitones::new(12.0).unwrap().to_ratio().unwrap(), 2.0);
        assert_eq!(Semitones::new(-24.0).unwrap().to_ratio().unwrap(), 0.25);
        assert_eq!(Cents::from_ratio(2.0).unwrap().get(), 1200.0);
        assert_eq!(Cents::from_ratio(1.0).unwrap().get(), 0.0);
    }

    #[test]
    fn a_semitone_is_100_cents() {
        let semitone = Semitones::new(1.0).unwrap().to_ratio().unwrap();
        let cents = Cents::new(100.0).unwrap().to_ratio().unwrap();
        assert_relative_eq!(semitone, cents, epsilon = 0.0, max_relative = 1e-15);
    }

    #[test]
    fn ratio_of_zero_or_less_has_no_cents() {
        assert_eq!(
            Cents::from_ratio(0.0).unwrap_err().to_string(),
            "ratio to Cents out of range: got 0.0, expected a finite value above 0.0"
        );
        assert!(Cents::from_ratio(-2.0).is_err());
        assert!(Cents::from_ratio(f64::INFINITY).is_err());
    }

    #[test]
    fn ratios_stay_inside_the_normal_range() {
        assert!(
            Cents::new(1_200_000.0)
                .unwrap()
                .to_ratio()
                .unwrap()
                .is_normal()
        );
        assert!(
            Cents::new(-1_200_000.0)
                .unwrap()
                .to_ratio()
                .unwrap()
                .is_normal()
        );
        assert_eq!(
            Cents::new(1_200_001.0)
                .unwrap()
                .to_ratio()
                .unwrap_err()
                .to_string(),
            "Cents to ratio out of range: got 1200001.0, expected -1200000.0 to 1200000.0"
        );
        assert!(Semitones::new(12_000.0).unwrap().to_ratio().is_ok());
        assert!(Semitones::new(12_001.0).unwrap().to_ratio().is_err());
        assert!(Semitones::new(-12_001.0).unwrap().to_ratio().is_err());
        assert!(Octaves::new(1000.0).unwrap().to_ratio().is_ok());
        assert!(
            Octaves::new(-1000.0)
                .unwrap()
                .to_ratio()
                .unwrap()
                .is_normal()
        );
        assert!(Octaves::new(1001.0).unwrap().to_ratio().is_err());
        assert!(Octaves::new(-1001.0).unwrap().to_ratio().is_err());
    }

    // Anchors: 20 * log10(2) = 6.0206 dB to four decimals, -20 dB is a tenth
    // of the amplitude and 10 dB ten times the power.
    #[test]
    fn decibel_anchors_convert_to_gain_and_power() {
        let six = Decibels::new(6.0206).unwrap().to_amplitude_gain().unwrap();
        // 6.0206 is 20 * log10(2) = 6.02059991... rounded to 1e-4 dB. The
        // rounding, 8.7e-8 dB, is 1.0e-8 relative in gain.
        assert_relative_eq!(six, 2.0, epsilon = 0.0, max_relative = 1e-7);
        let tenth = Decibels::new(-20.0).unwrap().to_amplitude_gain().unwrap();
        assert_relative_eq!(tenth, 0.1, epsilon = 0.0, max_relative = 1e-15);
        let power = Decibels::new(10.0).unwrap().to_power_ratio().unwrap();
        assert_relative_eq!(power, 10.0, epsilon = 0.0, max_relative = 1e-15);
        assert_eq!(
            Decibels::new(0.0).unwrap().to_amplitude_gain().unwrap(),
            1.0
        );
    }

    #[test]
    fn gain_and_power_anchors_convert_to_decibels() {
        let two = Decibels::from_amplitude_gain(2.0).unwrap().get();
        assert_relative_eq!(two, 6.0206, epsilon = 1e-7, max_relative = 0.0);
        let tenth = Decibels::from_amplitude_gain(0.1).unwrap().get();
        assert_relative_eq!(tenth, -20.0, epsilon = 1e-12, max_relative = 0.0);
        let ten = Decibels::from_power_ratio(10.0).unwrap().get();
        assert_relative_eq!(ten, 10.0, epsilon = 1e-12, max_relative = 0.0);
        let hundredth = Decibels::from_power_ratio(0.01).unwrap().get();
        assert_relative_eq!(hundredth, -20.0, epsilon = 1e-12, max_relative = 0.0);
    }

    #[test]
    fn gain_of_zero_or_less_has_no_decibels() {
        assert_eq!(
            Decibels::from_amplitude_gain(0.0).unwrap_err().to_string(),
            "amplitude gain to Decibels out of range: got 0.0, expected a finite value above 0.0"
        );
        assert!(Decibels::from_amplitude_gain(-1.0).is_err());
        assert!(Decibels::from_power_ratio(0.0).is_err());
        assert!(Decibels::from_power_ratio(f64::NAN).is_err());
    }

    #[test]
    fn gains_stay_inside_the_normal_range() {
        assert!(
            Decibels::new(6000.0)
                .unwrap()
                .to_amplitude_gain()
                .unwrap()
                .is_normal()
        );
        assert!(
            Decibels::new(-6000.0)
                .unwrap()
                .to_amplitude_gain()
                .unwrap()
                .is_normal()
        );
        assert!(Decibels::new(6001.0).unwrap().to_amplitude_gain().is_err());
        assert!(Decibels::new(-6001.0).unwrap().to_amplitude_gain().is_err());
        assert!(
            Decibels::new(3000.0)
                .unwrap()
                .to_power_ratio()
                .unwrap()
                .is_normal()
        );
        assert!(
            Decibels::new(-3000.0)
                .unwrap()
                .to_power_ratio()
                .unwrap()
                .is_normal()
        );
        assert!(Decibels::new(3001.0).unwrap().to_power_ratio().is_err());
        assert!(Decibels::new(-3001.0).unwrap().to_power_ratio().is_err());
    }

    proptest! {
        // The issue asks for 20 Hz to 20 kHz. MIDI pitch ends at note 127,
        // about 12 544 Hz, so the round trip stops there.
        #[test]
        fn hz_to_midi_to_hz_round_trips(hz in 20.0..=12_543.0f64) {
            let pitch = MidiPitch::try_from(Hz::new(hz).unwrap()).unwrap();
            let back = Hz::from(pitch).get();
            prop_assert!((back - hz).abs() <= 1e-9 * hz, "{hz} -> {back}");
        }

        #[test]
        fn cents_to_ratio_to_cents_round_trips(cents in -2400.0..=2400.0f64) {
            let ratio = Cents::new(cents).unwrap().to_ratio().unwrap();
            let back = Cents::from_ratio(ratio).unwrap().get();
            prop_assert!((back - cents).abs() <= 1e-9, "{cents} -> {back}");
        }

        #[test]
        fn db_to_gain_to_db_round_trips(db in -120.0..=24.0f64) {
            let gain = Decibels::new(db).unwrap().to_amplitude_gain().unwrap();
            let back = Decibels::from_amplitude_gain(gain).unwrap().get();
            prop_assert!((back - db).abs() <= 1e-9, "{db} -> {back}");
        }

        #[test]
        fn db_to_power_to_db_round_trips(db in -120.0..=24.0f64) {
            let ratio = Decibels::new(db).unwrap().to_power_ratio().unwrap();
            let back = Decibels::from_power_ratio(ratio).unwrap().get();
            prop_assert!((back - db).abs() <= 1e-9, "{db} -> {back}");
        }
    }
}
