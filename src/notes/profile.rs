//! Decode settings: the pitch range, the minimum note length and the
//! decoder switches, and the three profiles that fix them per stem type.

use std::error::Error;
use std::fmt;

use basic_pitch::NoteParams;
use basic_pitch::constants::{AUDIO_SAMPLE_RATE, FFT_HOP};

use super::Voicing;
use crate::cast;
use crate::units::{Bpm, Hz, MidiPitch};

/// The lowest pitch the model can give, A0.
pub const LOWEST_MODEL_PITCH: u8 = 21;
/// The highest pitch the model can give, C8.
pub const HIGHEST_MODEL_PITCH: u8 = 108;

/// The decoder's default minimum length in frames, 127.7 ms. A kept note
/// lasts at least one frame more, 139.3 ms.
pub const DEFAULT_MIN_NOTE_FRAMES: usize = 11;

/// The shortest setting the BPM rule gives, in milliseconds.
const RULE_FLOOR_MS: f64 = 58.0;
/// The longest setting the BPM rule gives: the decoder default.
const RULE_CEILING_MS: f64 = 127.7;
/// The fraction of a sixteenth note that the BPM rule takes.
const SIXTEENTH_FRACTION: f64 = 0.5;

/// The length of one model frame in milliseconds, 256 / 22,050 s.
fn frame_ms() -> f64 {
    cast::usize_to_f64(FFT_HOP) / f64::from(AUDIO_SAMPLE_RATE) * 1000.0
}

/// The minimum note length for a BPM, in model frames.
///
/// The rule takes half a sixteenth note, `0.5 * 15000 / BPM` ms, within
/// 58 to 127.7 ms, and rounds it to whole frames, half to even as the
/// decoder does. The decoder drops a note that lasts this many frames or
/// fewer, so a kept note lasts at least one frame more. 60 BPM gives 11
/// frames, 90 BPM 7 and 120 BPM 5.
#[must_use]
pub fn min_note_frames(bpm: Bpm) -> usize {
    let ms = (SIXTEENTH_FRACTION * 15_000.0 / bpm.get()).clamp(RULE_FLOOR_MS, RULE_CEILING_MS);
    cast::floor_to_usize((ms / frame_ms()).round_ties_even()).unwrap_or(DEFAULT_MIN_NOTE_FRAMES)
}

/// A range of whole MIDI pitches the decoder may create, both ends kept,
/// within the model's range A0 to C8.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PitchRange {
    lowest: u8,
    highest: u8,
}

impl PitchRange {
    /// Checks that both ends are whole pitches within 21 to 108 and that
    /// the lowest is not above the highest.
    ///
    /// # Errors
    ///
    /// Returns a [`RangeError`] that names the bad end.
    pub fn new(lowest: MidiPitch, highest: MidiPitch) -> Result<Self, RangeError> {
        let whole = |pitch: MidiPitch| {
            let value = pitch.get();
            (value.fract() == 0.0
                && (f64::from(LOWEST_MODEL_PITCH)..=f64::from(HIGHEST_MODEL_PITCH))
                    .contains(&value))
            .then(|| cast::floor_to_usize(value).and_then(|n| u8::try_from(n).ok()))
            .flatten()
            .ok_or(RangeError::Pitch(pitch))
        };
        let (lowest, highest) = (whole(lowest)?, whole(highest)?);
        if lowest > highest {
            return Err(RangeError::Order { lowest, highest });
        }
        Ok(Self { lowest, highest })
    }

    /// Builds a range from two literals that a test also builds through
    /// [`Self::new`].
    const fn from_const(lowest: u8, highest: u8) -> Self {
        Self { lowest, highest }
    }

    /// Returns the lowest kept pitch.
    #[must_use]
    pub fn lowest(self) -> MidiPitch {
        MidiPitch::clamped(f64::from(self.lowest))
    }

    /// Returns the highest kept pitch.
    #[must_use]
    pub fn highest(self) -> MidiPitch {
        MidiPitch::clamped(f64::from(self.highest))
    }

    /// The decoder's lower bound: the frequency of the lowest pitch. The
    /// decoder rounds a bound to the nearest semitone and keeps it.
    #[must_use]
    pub fn minimum_frequency(self) -> Hz {
        Hz::from(self.lowest())
    }

    /// The decoder's upper bound: the frequency one semitone above the
    /// highest pitch, because the decoder excludes its upper bound.
    #[must_use]
    pub fn maximum_frequency(self) -> Hz {
        Hz::from(MidiPitch::clamped(f64::from(self.highest) + 1.0))
    }
}

/// Everything the decoder needs besides its thresholds, which keep their
/// defaults: the pitch range, the minimum note length and the melodia pass.
/// Pitch bends stay off, because the notes file writes none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeSettings {
    /// The pitches the decoder may create.
    pub range: PitchRange,
    /// The decoder drops a note of this many frames or fewer.
    pub min_note_frames: usize,
    /// Whether the decoder also builds notes without a clear onset from
    /// the energy left over, the melodia pass.
    pub melodia: bool,
}

impl DecodeSettings {
    /// The decoder parameters. The minimum length goes in as the middle of
    /// its frame's rounding interval, so it never sits on a tie.
    #[must_use]
    pub fn note_params(&self) -> NoteParams {
        NoteParams {
            minimum_note_length_ms: cast::usize_to_f64(self.min_note_frames) * frame_ms(),
            minimum_frequency: Some(self.range.minimum_frequency().get()),
            maximum_frequency: Some(self.range.maximum_frequency().get()),
            melodia_trick: self.melodia,
            include_pitch_bends: false,
            ..NoteParams::default()
        }
    }
}

/// A fixed set of decode settings for one kind of stem.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, clap::ValueEnum)]
pub enum Profile {
    /// B0 to F#4, with the minimum length from the BPM.
    Bass,
    /// C2 to B6, with the minimum length from the BPM. Plucks use it too.
    Lead,
    /// C2 to B6, with the decoder's default minimum length. Chords use it
    /// too.
    Pad,
}

impl Profile {
    /// Every profile.
    pub const ALL: [Self; 3] = [Self::Bass, Self::Lead, Self::Pad];

    /// Returns the name the command line uses.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Bass => "bass",
            Self::Lead => "lead",
            Self::Pad => "pad",
        }
    }

    /// Returns the pitch range.
    #[must_use]
    pub fn range(self) -> PitchRange {
        match self {
            // B0 clears the A0 bin edge with margin and covers a
            // five-string bass. Above F#4 a bass has no fundamental.
            Self::Bass => PitchRange::from_const(23, 66),
            Self::Lead | Self::Pad => PitchRange::from_const(36, 95),
        }
    }

    /// Returns the voicing the cleanup assumes: chords for `pad`, one note
    /// at a time otherwise.
    #[must_use]
    pub fn voicing(self) -> Voicing {
        match self {
            Self::Bass | Self::Lead => Voicing::Monophonic,
            Self::Pad => Voicing::Polyphonic,
        }
    }

    /// Returns the decode settings for a stem at a BPM.
    #[must_use]
    pub fn settings(self, bpm: Bpm) -> DecodeSettings {
        DecodeSettings {
            range: self.range(),
            min_note_frames: match self {
                Self::Bass | Self::Lead => min_note_frames(bpm),
                Self::Pad => DEFAULT_MIN_NOTE_FRAMES,
            },
            melodia: true,
        }
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A pitch range that the decoder cannot use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RangeError {
    /// An end that is not a whole pitch within 21 to 108.
    Pitch(MidiPitch),
    /// A lowest pitch above the highest.
    Order {
        /// The lowest pitch.
        lowest: u8,
        /// The highest pitch.
        highest: u8,
    },
}

impl fmt::Display for RangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pitch(pitch) => write!(
                f,
                "pitch {} is not a whole MIDI pitch from {LOWEST_MODEL_PITCH} to {HIGHEST_MODEL_PITCH}, the range of the transcription model",
                pitch.get()
            ),
            Self::Order { lowest, highest } => write!(
                f,
                "the lowest pitch {lowest} lies above the highest pitch {highest}"
            ),
        }
    }
}

impl Error for RangeError {}

#[cfg(test)]
mod tests {
    use basic_pitch::notes::{constrain_frequency, model_output_to_notes};
    use basic_pitch::{Posteriorgram, Posteriorgrams};

    use super::*;

    fn pitch(value: f64) -> MidiPitch {
        MidiPitch::new(value).unwrap()
    }

    fn bpm(value: f64) -> Bpm {
        Bpm::new(value).unwrap()
    }

    /// The pitches whose bins survive the frequency bounds of a range.
    fn kept(range: PitchRange) -> Vec<u8> {
        let ones = Posteriorgram::from_vec(vec![1.0; 88], 88).unwrap();
        let (_, frames) = constrain_frequency(
            &ones,
            &ones,
            Some(range.maximum_frequency().get()),
            Some(range.minimum_frequency().get()),
        )
        .unwrap();
        (0..88_u8)
            .filter(|&bin| frames[(0, usize::from(bin))] > 0.0)
            .map(|bin| bin + LOWEST_MODEL_PITCH)
            .collect()
    }

    #[test]
    fn every_range_keeps_exactly_its_pitches() {
        for lowest in LOWEST_MODEL_PITCH..=HIGHEST_MODEL_PITCH {
            for highest in lowest..=HIGHEST_MODEL_PITCH {
                let range = PitchRange::new(pitch(lowest.into()), pitch(highest.into())).unwrap();
                assert_eq!(
                    kept(range),
                    (lowest..=highest).collect::<Vec<_>>(),
                    "{lowest} to {highest}"
                );
            }
        }
    }

    #[test]
    fn every_profile_keeps_exactly_its_pitches() {
        for (profile, lowest, highest) in [
            (Profile::Bass, 23, 66),
            (Profile::Lead, 36, 95),
            (Profile::Pad, 36, 95),
        ] {
            let range = profile.range();
            assert_eq!(
                range,
                PitchRange::new(pitch(lowest.into()), pitch(highest.into())).unwrap()
            );
            assert_eq!(kept(range), (lowest..=highest).collect::<Vec<_>>());
        }
    }

    #[test]
    fn a_400_hz_ceiling_keeps_f_sharp_4_and_drops_g4() {
        let ones = Posteriorgram::from_vec(vec![1.0; 88], 88).unwrap();
        let (_, frames) = constrain_frequency(&ones, &ones, Some(400.0), None).unwrap();
        assert!(frames[(0, 66 - 21)] > 0.0);
        assert_eq!(frames[(0, 67 - 21)], 0.0);
        assert_eq!(
            Profile::Bass.range().highest(),
            pitch(66.0),
            "the bass ceiling is the 400 Hz example"
        );
    }

    #[test]
    fn a_range_takes_whole_pitches_of_the_model_only() {
        assert!(PitchRange::new(pitch(21.0), pitch(108.0)).is_ok());
        for bad in [20.0, 109.0, 60.5, 0.0, 127.0] {
            assert_eq!(
                PitchRange::new(pitch(bad), pitch(108.0)),
                Err(RangeError::Pitch(pitch(bad)))
            );
        }
        assert_eq!(
            PitchRange::new(pitch(60.0), pitch(59.0))
                .unwrap_err()
                .to_string(),
            "the lowest pitch 60 lies above the highest pitch 59"
        );
        assert_eq!(
            RangeError::Pitch(pitch(60.5)).to_string(),
            "pitch 60.5 is not a whole MIDI pitch from 21 to 108, the range of the transcription model"
        );
    }

    #[test]
    fn the_bpm_rule_gives_the_issue_frame_counts() {
        for (tempo, frames) in [(60.0, 11), (90.0, 7), (120.0, 5), (4.0, 11), (1000.0, 5)] {
            assert_eq!(min_note_frames(bpm(tempo)), frames, "{tempo} BPM");
        }
        // The ceiling holds up to 58.7 BPM and the floor from 129.3 BPM.
        assert_eq!(min_note_frames(bpm(58.7)), 11);
        assert_eq!(min_note_frames(bpm(129.4)), 5);
    }

    proptest::proptest! {
        #[test]
        fn the_bpm_rule_stays_in_range_and_never_grows_with_tempo(
            a in 4.0..=1000.0_f64,
            b in 4.0..=1000.0_f64,
        ) {
            let (slow, fast) = (a.min(b), a.max(b));
            let frames = min_note_frames(bpm(slow));
            proptest::prop_assert!((5..=11).contains(&frames));
            proptest::prop_assert!(min_note_frames(bpm(fast)) <= frames);
            for profile in Profile::ALL {
                let settings = profile.settings(bpm(slow));
                proptest::prop_assert_eq!(
                    settings.note_params().min_note_len_frames().unwrap(),
                    settings.min_note_frames
                );
            }
        }
    }

    #[test]
    fn profiles_turn_bends_off_and_keep_the_decoder_thresholds() {
        let defaults = NoteParams::default();
        for profile in Profile::ALL {
            let params = profile.settings(bpm(120.0)).note_params();
            assert!(!params.include_pitch_bends, "{profile}");
            assert!(params.melodia_trick, "{profile}");
            assert_eq!(params.onset_threshold, defaults.onset_threshold);
            assert_eq!(params.frame_threshold, defaults.frame_threshold);
            assert_eq!(params.energy_tolerance, defaults.energy_tolerance);
            params.validate().unwrap();
        }
        assert_eq!(Profile::Pad.settings(bpm(200.0)).min_note_frames, 11);
        assert_eq!(Profile::Bass.settings(bpm(200.0)).min_note_frames, 5);
        assert_eq!(Profile::ALL.map(Profile::name), ["bass", "lead", "pad"]);
        let plain = DecodeSettings {
            melodia: false,
            ..Profile::Lead.settings(bpm(120.0))
        };
        assert!(!plain.note_params().melodia_trick);
    }

    /// Posteriorgrams of 60 frames with one note at A3 from frame 10 for
    /// `len` frames, with or without an onset at its first frame.
    fn region(len: usize, onset: bool) -> Posteriorgrams {
        let (frames, bin) = (60, 57 - 21);
        let mut note = Posteriorgram::zeros(frames, 88);
        let mut onsets = Posteriorgram::zeros(frames, 88);
        for t in 10..10 + len {
            note.as_mut_slice()[t * 88 + bin] = 0.8;
        }
        if onset {
            onsets.as_mut_slice()[10 * 88 + bin] = 0.9;
        }
        Posteriorgrams::new(note, onsets, Posteriorgram::zeros(frames, 264)).unwrap()
    }

    #[test]
    fn the_default_length_drops_11_frames_and_keeps_12_with_an_onset() {
        let params = NoteParams {
            infer_onsets: false,
            melodia_trick: false,
            ..NoteParams::default()
        };
        let lengths = |len| {
            model_output_to_notes(&region(len, true), &params)
                .unwrap()
                .iter()
                .map(|note| note.end_frame - note.start_frame)
                .collect::<Vec<_>>()
        };
        assert_eq!(lengths(11), Vec::<usize>::new());
        assert_eq!(lengths(12), vec![12]);
    }

    #[test]
    fn the_melodia_pass_is_one_frame_stricter() {
        let params = NoteParams {
            infer_onsets: false,
            ..NoteParams::default()
        };
        let lengths = |len| {
            model_output_to_notes(&region(len, false), &params)
                .unwrap()
                .iter()
                .map(|note| note.end_frame - note.start_frame)
                .collect::<Vec<_>>()
        };
        assert_eq!(lengths(12), Vec::<usize>::new());
        // A 13-frame region becomes a note one frame shorter: the pass
        // stores an end frame one lower.
        assert_eq!(lengths(13), vec![12]);
    }
}
