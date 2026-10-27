//! Register and voicing detection: the decode settings of a stem without a
//! profile.
//!
//! A probe decode over the model's whole range, A0 to C8, shows whether
//! the part plays one note at a time or chords. The f0 track gives the
//! register of a monophonic part. It follows the fundamental, so an
//! octave-up ghost of the decoder does not widen the range. A chord part
//! takes its register from the probe notes instead: a pitch tracker reads
//! a chord as one low common period.

use super::{
    DEFAULT_MIN_NOTE_FRAMES, DecodeSettings, HIGHEST_MODEL_PITCH, LOWEST_MODEL_PITCH, ModelRun,
    NoteList, PitchRange, PitchTrack, TranscribeError, min_note_frames,
};
use crate::cast;
use crate::units::{Bpm, Decibels, MidiPitch, Seconds, Semitones};

/// The low and high percentiles of the voiced f0 frames that bound the
/// register.
pub const REGISTER_PERCENTILES: (f64, f64) = (1.0, 99.0);
/// The register reads only voiced frames within this many decibels of the
/// loudest frame, so quiet bleed in a rest cannot widen it.
pub const REGISTER_WINDOW: Decibels = Decibels::from_const(20.0);
/// How far the range reaches beyond the register on each side.
pub const REGISTER_MARGIN: Semitones = Semitones::from_const(2.0);
/// The share of probe notes struck with another pitch class from which a
/// part counts as chords.
pub const CHORD_SHARE: f64 = 0.5;
/// How close two onsets lie to count as struck together: the onset
/// tolerance of the evaluation.
pub const CHORD_ONSET_WINDOW: Seconds = Seconds::from_const(0.05);

/// Whether a part plays one note at a time or chords.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Voicing {
    /// One note at a time, as a bass or a lead plays.
    Monophonic,
    /// Two or more notes at once, as a pad or chords play.
    Polyphonic,
}

impl Voicing {
    /// Returns the word the command prints.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Monophonic => "monophonic",
            Self::Polyphonic => "polyphonic",
        }
    }
}

/// What the detection found and the settings it chose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Detection {
    /// The settings for the final decode.
    pub settings: DecodeSettings,
    /// One note at a time, or chords.
    pub voicing: Voicing,
    /// The share of probe notes that start within [`CHORD_ONSET_WINDOW`]
    /// of a note of another pitch class, 0 to 1.
    pub chord_share: f64,
    /// The level window of the f0 register.
    pub window: Decibels,
    /// The low and high percentiles of the voiced f0 frames within the
    /// window, if any such frame is voiced.
    pub f0_register: Option<(MidiPitch, MidiPitch)>,
    /// The lowest and highest probe notes, if the probe found any.
    pub probe_register: Option<(MidiPitch, MidiPitch)>,
    /// The number of notes the probe decode found.
    pub probe_notes: usize,
}

/// The settings of the probe decode: the whole model range, the minimum
/// length from the BPM and the melodia pass.
#[must_use]
pub fn probe_settings(bpm: Bpm) -> DecodeSettings {
    DecodeSettings {
        range: full_range(),
        min_note_frames: min_note_frames(bpm),
        melodia: true,
    }
}

fn full_range() -> PitchRange {
    PitchRange::new(
        MidiPitch::clamped(f64::from(LOWEST_MODEL_PITCH)),
        MidiPitch::clamped(f64::from(HIGHEST_MODEL_PITCH)),
    )
    .unwrap_or(super::Profile::Lead.range())
}

/// Detects the register and voicing of a stem from its model run and its
/// f0 track, and builds the settings for the final decode.
///
/// A monophonic part takes the BPM rule for its minimum length, as the
/// bass and lead profiles do; a polyphonic part keeps the decoder default,
/// as the pad profile does.
///
/// The f0 register reads the voiced frames within [`REGISTER_WINDOW`] of
/// the loudest frame.
///
/// # Errors
///
/// Returns a [`TranscribeError`] when the probe decode fails.
pub fn detect(run: &ModelRun, track: &PitchTrack, bpm: Bpm) -> Result<Detection, TranscribeError> {
    detect_with_window(run, track, bpm, REGISTER_WINDOW)
}

/// [`detect`] with another level window for the f0 register, for the
/// evaluation of the window.
///
/// # Errors
///
/// Returns a [`TranscribeError`] when the probe decode fails.
pub fn detect_with_window(
    run: &ModelRun,
    track: &PitchTrack,
    bpm: Bpm,
    window: Decibels,
) -> Result<Detection, TranscribeError> {
    let probe = run.decode(&probe_settings(bpm))?;
    let chord_share = chord_share(&probe);
    let voicing = if chord_share >= CHORD_SHARE {
        Voicing::Polyphonic
    } else {
        Voicing::Monophonic
    };
    let f0_register = f0_register(track, window);
    let probe_register = probe_register(&probe);
    let register = match (voicing, f0_register, probe_register) {
        (Voicing::Polyphonic, _, Some((low, high)))
        | (Voicing::Monophonic, Some((low, high)), _)
        | (Voicing::Monophonic, None, Some((low, high))) => Some((low.get(), high.get())),
        _ => None,
    };
    let range = register.map_or_else(full_range, |(low, high)| range_of(low, high));
    Ok(Detection {
        settings: DecodeSettings {
            range,
            min_note_frames: match voicing {
                Voicing::Monophonic => min_note_frames(bpm),
                Voicing::Polyphonic => DEFAULT_MIN_NOTE_FRAMES,
            },
            melodia: true,
        },
        voicing,
        chord_share,
        window,
        f0_register,
        probe_register,
        probe_notes: probe.notes().len(),
    })
}

/// The decode range of a register: [`REGISTER_MARGIN`] beyond each end,
/// widened to whole pitches and clamped to the model's range.
fn range_of(low: f64, high: f64) -> PitchRange {
    let margin = REGISTER_MARGIN.get();
    let clamp = |pitch: f64| {
        pitch.clamp(
            f64::from(LOWEST_MODEL_PITCH),
            f64::from(HIGHEST_MODEL_PITCH),
        )
    };
    PitchRange::new(
        MidiPitch::clamped(clamp((low - margin).floor())),
        MidiPitch::clamped(clamp((high + margin).ceil())),
    )
    .unwrap_or_else(|_| full_range())
}

/// The share of notes that start within [`CHORD_ONSET_WINDOW`] of a note
/// of another pitch class. Chords strike their notes together; a line
/// does not, even where the decoder's late ends overlap the next note. An
/// octave-up ghost shares its note's pitch class, so it does not count.
fn chord_share(notes: &NoteList) -> f64 {
    let notes = notes.notes();
    let class = |pitch: MidiPitch| cast::round_to_usize(pitch.get()).unwrap_or(0) % 12;
    let window = CHORD_ONSET_WINDOW.get();
    let struck = notes
        .iter()
        .filter(|note| {
            notes.iter().any(|other| {
                class(other.pitch()) != class(note.pitch())
                    && (other.onset().get() - note.onset().get()).abs() <= window
            })
        })
        .count();
    if notes.is_empty() {
        0.0
    } else {
        cast::usize_to_f64(struck) / cast::usize_to_f64(notes.len())
    }
}

/// The percentiles of the voiced f0 frames within the window.
fn f0_register(track: &PitchTrack, window: Decibels) -> Option<(MidiPitch, MidiPitch)> {
    let mut pitches: Vec<f64> = track
        .voiced_pitches_within(window)
        .into_iter()
        .map(MidiPitch::get)
        .collect();
    pitches.sort_by(f64::total_cmp);
    let (low, high) = REGISTER_PERCENTILES;
    Some((
        MidiPitch::clamped(quantile(&pitches, low / 100.0)?),
        MidiPitch::clamped(quantile(&pitches, high / 100.0)?),
    ))
}

fn probe_register(notes: &NoteList) -> Option<(MidiPitch, MidiPitch)> {
    let pitches = notes.notes().iter().map(|note| note.pitch().get());
    let low = pitches.clone().reduce(f64::min)?;
    let high = pitches.reduce(f64::max)?;
    Some((MidiPitch::clamped(low), MidiPitch::clamped(high)))
}

/// The linear quantile of sorted values, the default of `numpy.quantile`.
fn quantile(sorted: &[f64], q: f64) -> Option<f64> {
    let last = sorted.len().checked_sub(1)?;
    let position = q * cast::usize_to_f64(last);
    let below = cast::floor_to_usize(position)?.min(last);
    let above = (below + 1).min(last);
    let fraction = position - cast::usize_to_f64(below);
    Some(sorted[below] + (sorted[above] - sorted[below]) * fraction)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::{Note, TrackFrame};
    use crate::units::Hz;

    fn s(value: f64) -> Seconds {
        Seconds::new(value).unwrap()
    }

    fn note(onset: f64, offset: f64, pitch: f64) -> Note {
        Note::new(s(onset), s(offset), MidiPitch::new(pitch).unwrap()).unwrap()
    }

    /// A track of 1000 frames 5 ms apart at full level: frame `i` reads
    /// `pitch(i)`.
    fn track_of(pitch: impl Fn(usize) -> Option<f64>) -> PitchTrack {
        PitchTrack::from_frames(
            (0..1000)
                .map(|i| TrackFrame {
                    time: s(cast::usize_to_f64(i) * 0.005),
                    f0: pitch(i).map(|p| Hz::from(MidiPitch::new(p).unwrap())),
                    level: Decibels::new(0.0).unwrap(),
                })
                .collect(),
        )
    }

    #[test]
    fn frames_outside_the_window_leave_the_register_alone() {
        // A bass at 40 and quiet bleed at 72, 30 dB down, in every third
        // frame.
        let frames = (0..900)
            .map(|i| TrackFrame {
                time: s(cast::usize_to_f64(i) * 0.005),
                f0: Some(Hz::from(
                    MidiPitch::new(if i % 3 == 0 { 72.0 } else { 40.0 }).unwrap(),
                )),
                level: Decibels::new(if i % 3 == 0 { -30.0 } else { -3.0 }).unwrap(),
            })
            .collect();
        let track = PitchTrack::from_frames(frames);
        let (low, high) = f0_register(&track, REGISTER_WINDOW).unwrap();
        assert!((low.get() - 40.0).abs() < 1e-9 && (high.get() - 40.0).abs() < 1e-9);
        let (_, wide) = f0_register(&track, Decibels::new(40.0).unwrap()).unwrap();
        assert!((wide.get() - 72.0).abs() < 1e-9);
    }

    #[test]
    fn the_register_is_the_1st_to_99th_percentile_of_voiced_frames() {
        // 1000 voiced frames over pitches 40.0 to 49.99, plus unvoiced ones.
        let track = track_of(|i| (i % 3 != 0).then(|| 40.0 + cast::usize_to_f64(i) / 100.0));
        let (low, high) = f0_register(&track, REGISTER_WINDOW).unwrap();
        let mut voiced: Vec<f64> = (0..1000)
            .filter(|i| i % 3 != 0)
            .map(|i| 40.0 + cast::usize_to_f64(i) / 100.0)
            .collect();
        voiced.sort_by(f64::total_cmp);
        assert!((low.get() - quantile(&voiced, 0.01).unwrap()).abs() < 1e-9);
        assert!((high.get() - quantile(&voiced, 0.99).unwrap()).abs() < 1e-9);
        assert!((low.get() - 40.105).abs() < 0.01, "{}", low.get());
        assert!((high.get() - 49.885).abs() < 0.01, "{}", high.get());
        assert_eq!(f0_register(&track_of(|_| None), REGISTER_WINDOW), None);
    }

    #[test]
    fn the_range_reaches_two_semitones_beyond_the_register() {
        let ends = |range: PitchRange| (range.lowest().get(), range.highest().get());
        assert_eq!(ends(range_of(40.105, 49.885)), (38.0, 52.0));
        assert_eq!(ends(range_of(45.0, 45.0)), (43.0, 47.0));
        assert_eq!(ends(range_of(22.0, 107.5)), (21.0, 108.0));
    }

    #[test]
    fn quantiles_interpolate_linearly() {
        let values = [1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(quantile(&values, 0.0), Some(1.0));
        assert_eq!(quantile(&values, 0.5), Some(3.0));
        assert_eq!(quantile(&values, 0.99), Some(4.96));
        assert_eq!(quantile(&values, 1.0), Some(5.0));
        assert_eq!(quantile(&[], 0.5), None);
    }

    #[test]
    fn notes_struck_together_are_chords_and_ghosts_or_overlaps_are_not() {
        let bass = NoteList::new(vec![
            note(0.0, 1.2, 45.0),
            note(0.02, 0.3, 57.0),
            note(1.0, 2.0, 47.0),
        ]);
        assert_eq!(chord_share(&bass), 0.0);
        let chords = NoteList::new(vec![
            note(0.0, 2.0, 57.0),
            note(0.05, 2.0, 60.0),
            note(2.0, 4.0, 57.0),
            note(3.0, 4.0, 62.0),
        ]);
        assert!((chord_share(&chords) - 0.5).abs() < 1e-12);
        assert_eq!(chord_share(&NoteList::default()), 0.0);
    }
}
