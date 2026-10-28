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
use crate::units::{Bpm, Decibels, MidiPitch, Semitones};

/// The low and high percentiles of the voiced f0 frames that bound the
/// register.
pub const REGISTER_PERCENTILES: (f64, f64) = (1.0, 99.0);
/// The register reads only voiced frames within this many decibels of the
/// reference level, so quiet bleed in a rest cannot widen it.
pub const REGISTER_WINDOW: Decibels = Decibels::from_const(20.0);
/// How far below the reference level a probe note's median frame may lie
/// and still count as a hold candidate. Half the register window, so bleed
/// near -20 dB in a rest does not widen the range.
pub const HOLD_WINDOW: Decibels = Decibels::from_const(10.0);
/// The percentile of the voiced frames' levels that serves as the
/// reference level. A short loud transient does not move it.
pub const REFERENCE_PERCENTILE: f64 = 95.0;
/// How far the range reaches beyond the register on each side.
pub const REGISTER_MARGIN: Semitones = Semitones::from_const(2.0);
/// The share of probe notes that sound with another pitch class from which
/// a part counts as chords. It sits midway between the highest line
/// (0.56) and the lowest chord part (0.79) of the generated set.
pub const CHORD_SHARE: f64 = 0.68;
/// How much of a note's span another pitch class must share with it for
/// the note to sound in a chord.
pub const CHORD_OVERLAP: f64 = 0.5;

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

/// Where the register came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RegisterSource {
    /// The f0 track of a monophonic part.
    F0Track,
    /// The probe notes: a polyphonic part, or a part with no voiced frame.
    ProbeNotes,
    /// The f0 track, widened to the hold candidates outside its range.
    Both,
    /// Neither: no voiced frame and no probe note, so the whole model range.
    None,
}

impl RegisterSource {
    /// Returns the words the command prints.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::F0Track => "f0 track",
            Self::ProbeNotes => "probe notes",
            Self::Both => "f0 track and held probe notes",
            Self::None => "whole model range",
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
    /// The share of probe notes struck and released with a note of another
    /// pitch class, 0 to 1.
    pub chord_share: f64,
    /// The level window of the f0 register.
    pub window: Decibels,
    /// Where the register came from.
    pub source: RegisterSource,
    /// The share of the hold candidates inside the range of the f0
    /// register, if both exist.
    pub probe_held: Option<f64>,
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
    let voicing = voicing_of(chord_share);
    let f0_register = f0_register(track, window);
    let probe_register = probe_register(&probe);
    let probe_pitches = hold_candidates(&probe, track);
    let (range, source, probe_held) =
        choose_range(voicing, f0_register, probe_register, &probe_pitches);
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
        source,
        probe_held,
        f0_register,
        probe_register,
        probe_notes: probe.notes().len(),
    })
}

fn voicing_of(chord_share: f64) -> Voicing {
    if chord_share >= CHORD_SHARE {
        Voicing::Polyphonic
    } else {
        Voicing::Monophonic
    }
}

type Register = Option<(MidiPitch, MidiPitch)>;

/// The decode range from the voicing and the two registers, with its
/// source and the share of hold candidates the f0 range holds.
///
/// A polyphonic part takes the probe span. A monophonic part takes the f0
/// register, widened to every hold candidate outside its range, so a note
/// the track misreads still lands inside.
fn choose_range(
    voicing: Voicing,
    f0: Register,
    probe: Register,
    probe_pitches: &[f64],
) -> (PitchRange, RegisterSource, Option<f64>) {
    let span = |(low, high): (MidiPitch, MidiPitch)| range_of(low.get(), high.get());
    let held = f0.filter(|_| !probe_pitches.is_empty()).map(|register| {
        let range = span(register);
        let (low, high) = (range.lowest().get(), range.highest().get());
        let inside = probe_pitches
            .iter()
            .filter(|&&p| (low..=high).contains(&p))
            .count();
        cast::usize_to_f64(inside) / cast::usize_to_f64(probe_pitches.len())
    });
    match (voicing, f0, probe) {
        (Voicing::Polyphonic, _, Some(probe)) | (_, None, Some(probe)) => {
            (span(probe), RegisterSource::ProbeNotes, held)
        }
        (_, Some(f0), _) if held.is_some_and(|h| h < 1.0) => {
            let low = probe_pitches.iter().copied().fold(f0.0.get(), f64::min);
            let high = probe_pitches.iter().copied().fold(f0.1.get(), f64::max);
            (range_of(low, high), RegisterSource::Both, held)
        }
        (_, Some(f0), _) => (span(f0), RegisterSource::F0Track, held),
        (_, None, None) => (full_range(), RegisterSource::None, held),
    }
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

/// The share of notes that a note of another pitch class sounds with for
/// at least [`CHORD_OVERLAP`] of their span.
///
/// A chord holds its notes together even when they start or end apart: a
/// strum, a broken chord with the sustain held, staggered releases or a
/// voice that moves under held notes. A line holds one note at a time; the
/// decoder's late ends overlap the next note only in part. An octave-up
/// ghost shares its note's pitch class, so it does not count.
/// The voicing of a note list by the rule detection applies to its probe
/// notes: polyphonic when [`CHORD_SHARE`] or more of the notes share at
/// least [`CHORD_OVERLAP`] of their span with a note of another pitch class.
#[must_use]
pub fn voicing_of_notes(notes: &NoteList) -> Voicing {
    voicing_of(chord_share(notes))
}

fn chord_share(notes: &NoteList) -> f64 {
    let notes = notes.notes();
    let class = |pitch: MidiPitch| cast::round_to_usize(pitch.get()).unwrap_or(0) % 12;
    let struck = notes
        .iter()
        .filter(|note| {
            let span = note.offset().get() - note.onset().get();
            notes.iter().any(|other| {
                let shared = other.offset().get().min(note.offset().get())
                    - other.onset().get().max(note.onset().get());
                class(other.pitch()) != class(note.pitch()) && shared >= CHORD_OVERLAP * span
            })
        })
        .count();
    if notes.is_empty() {
        0.0
    } else {
        cast::usize_to_f64(struck) / cast::usize_to_f64(notes.len())
    }
}

/// The pitches of the probe notes that can show a misread f0 track: notes
/// whose median frame lies within [`HOLD_WINDOW`] of the reference level,
/// and that sound alone for at least half their frames.
///
/// A monophonic line plays one note at a time, so a note that shares most
/// of its span with another pitch is an octave ghost or bleed over the
/// line. A weaker note an octave away does not count against it: the
/// decoder gives every note such a twin. A frame spans 93 ms, so bleed in
/// a short gap of the line reads the line's tail in its edge frames; the
/// median skips them.
fn hold_candidates(probe: &NoteList, track: &PitchTrack) -> Vec<f64> {
    let Some(reference) = reference_level(track) else {
        return Vec::new();
    };
    let notes = probe.notes();
    notes
        .iter()
        .filter(|note| {
            let mut levels: Vec<f64> = track
                .frames()
                .iter()
                .filter(|frame| note.sounds_at(frame.time))
                .map(|frame| frame.level.get())
                .collect();
            levels.sort_by(f64::total_cmp);
            quantile(&levels, 0.5).is_some_and(|level| level >= reference - HOLD_WINDOW.get())
        })
        .filter(|note| {
            let (mut frames, mut shared) = (0_usize, 0_usize);
            for frame in track.frames().iter().filter(|f| note.sounds_at(f.time)) {
                frames += 1;
                shared += usize::from(notes.iter().any(|other| {
                    let gap = (other.pitch().get() - note.pitch().get()).abs();
                    let weaker_twin = (gap - 12.0).abs() < 0.5
                        && other
                            .velocity()
                            .zip(note.velocity())
                            .is_some_and(|(twin, this)| twin < this);
                    gap >= 0.5 && !weaker_twin && other.sounds_at(frame.time)
                }));
            }
            shared * 2 <= frames
        })
        .map(|note| note.pitch().get())
        .collect()
}

/// The [`REFERENCE_PERCENTILE`] of the voiced frames' levels.
fn reference_level(track: &PitchTrack) -> Option<f64> {
    let mut levels: Vec<f64> = track
        .frames()
        .iter()
        .filter(|frame| frame.f0.is_some())
        .map(|frame| frame.level.get())
        .collect();
    levels.sort_by(f64::total_cmp);
    quantile(&levels, REFERENCE_PERCENTILE / 100.0)
}

/// The percentiles of the voiced f0 frames whose level lies within
/// `window` of the reference: the [`REFERENCE_PERCENTILE`] of the voiced
/// frames' levels. A loud transient above the window counts no more than a
/// quiet bleed below it.
fn f0_register(track: &PitchTrack, window: Decibels) -> Option<(MidiPitch, MidiPitch)> {
    let reference = reference_level(track)?;
    let voiced: Vec<_> = track
        .frames()
        .iter()
        .filter(|frame| frame.f0.is_some())
        .collect();
    let mut pitches: Vec<f64> = voiced
        .iter()
        .filter(|frame| (frame.level.get() - reference).abs() <= window.get().abs())
        .filter_map(|frame| frame.f0)
        .filter_map(|f0| MidiPitch::try_from(f0).ok())
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
    use crate::units::{Hz, Seconds};

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
    fn each_register_source_has_its_words() {
        let sources = [
            RegisterSource::F0Track,
            RegisterSource::ProbeNotes,
            RegisterSource::Both,
            RegisterSource::None,
        ];
        assert_eq!(
            sources.map(RegisterSource::name),
            [
                "f0 track",
                "probe notes",
                "f0 track and held probe notes",
                "whole model range"
            ]
        );
    }

    #[test]
    fn the_probe_register_spans_the_lowest_and_highest_notes() {
        let notes = NoteList::new(vec![
            note(0.0, 1.0, 57.0),
            note(0.0, 1.0, 52.0),
            note(1.0, 2.0, 64.0),
        ]);
        let (low, high) = probe_register(&notes).unwrap();
        assert_eq!((low.get(), high.get()), (52.0, 64.0));
        assert_eq!(probe_register(&NoteList::new(Vec::new())), None);
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

    fn pitch(value: f64) -> MidiPitch {
        MidiPitch::new(value).unwrap()
    }

    /// Eight bass notes, MIDI 36 to 48, 100 frames each at `level`, then a
    /// transient of `count` frames at 0 dB that reads `transient` (or
    /// unvoiced).
    fn bass_with(level: f64, count: usize, transient: Option<f64>) -> PitchTrack {
        let line = [36.0, 38.0, 40.0, 41.0, 43.0, 45.0, 47.0, 48.0];
        let mut frames: Vec<TrackFrame> = (0..800)
            .map(|i| TrackFrame {
                time: s(cast::usize_to_f64(i) * 0.005),
                f0: Some(Hz::from(pitch(line[i / 100]))),
                level: Decibels::new(level).unwrap(),
            })
            .collect();
        frames.extend((0..count).map(|i| TrackFrame {
            time: s(4.0 + cast::usize_to_f64(i) * 0.005),
            f0: transient.map(|p| Hz::from(pitch(p))),
            level: Decibels::new(0.0).unwrap(),
        }));
        PitchTrack::from_frames(frames)
    }

    fn line_range(track: &PitchTrack) -> (f64, f64) {
        let (low, high) = f0_register(track, REGISTER_WINDOW).unwrap();
        let range = range_of(low.get(), high.get());
        (range.lowest().get(), range.highest().get())
    }

    #[test]
    fn a_loud_burst_ring_or_thump_leaves_the_register_alone() {
        // A noise burst 34 dB above the line that reads as MIDI 19.
        assert_eq!(line_range(&bass_with(-34.0, 20, Some(19.0))), (34.0, 50.0));
        // A 1.5 kHz ring 30 dB above the line.
        assert_eq!(line_range(&bass_with(-30.0, 12, Some(90.5))), (34.0, 50.0));
        // A 60 Hz thump 25 dB above the line.
        assert_eq!(line_range(&bass_with(-25.0, 27, Some(34.6))), (34.0, 50.0));
        // An unvoiced burst changes nothing either.
        assert_eq!(line_range(&bass_with(-40.0, 20, None)), (34.0, 50.0));
    }

    #[test]
    fn the_register_choice_takes_each_source_in_its_case() {
        let line: Vec<f64> = vec![36.0, 38.0, 40.0, 41.0, 43.0, 45.0, 47.0, 48.0];
        let ends = |(range, source, _): (PitchRange, RegisterSource, Option<f64>)| {
            (range.lowest().get(), range.highest().get(), source)
        };
        let f0 = Some((pitch(36.0), pitch(48.0)));
        let probe = Some((pitch(36.0), pitch(60.0)));
        assert_eq!(
            ends(choose_range(Voicing::Polyphonic, f0, probe, &line)),
            (34.0, 62.0, RegisterSource::ProbeNotes)
        );
        assert_eq!(
            ends(choose_range(Voicing::Monophonic, f0, probe, &line)),
            (34.0, 50.0, RegisterSource::F0Track)
        );
        // A misread track that holds none of the hold candidates widens to
        // all of them.
        let misread = Some((pitch(19.0), pitch(19.0)));
        let (range, source, held) = choose_range(Voicing::Monophonic, misread, probe, &line);
        assert_eq!(
            (range.lowest().get(), range.highest().get(), source),
            (21.0, 50.0, RegisterSource::Both)
        );
        assert_eq!(held, Some(0.0));
        assert_eq!(
            ends(choose_range(Voicing::Monophonic, None, probe, &line)),
            (34.0, 62.0, RegisterSource::ProbeNotes)
        );
        assert_eq!(
            ends(choose_range(Voicing::Monophonic, None, None, &[])),
            (21.0, 108.0, RegisterSource::None)
        );
    }

    #[test]
    fn a_hold_candidate_outside_the_f0_range_widens_it() {
        let f0 = Some((pitch(45.0), pitch(52.0)));
        let probe = Some((pitch(45.0), pitch(70.0)));
        let (range, source, held) =
            choose_range(Voicing::Monophonic, f0, probe, &[45.0, 47.0, 50.0, 52.0]);
        assert_eq!(
            (range.highest().get(), source, held),
            (54.0, RegisterSource::F0Track, Some(1.0))
        );
        // A note on 57, as in a fast run that jumps an octave, widens the
        // range to 59; 54 alone would lose it.
        let (range, source, held) = choose_range(
            Voicing::Monophonic,
            f0,
            probe,
            &[45.0, 47.0, 50.0, 52.0, 57.0],
        );
        assert_eq!(
            (range.lowest().get(), range.highest().get(), source, held),
            (43.0, 59.0, RegisterSource::Both, Some(0.8))
        );
    }

    #[test]
    fn a_held_double_stop_above_a_line_falls_outside_the_range() {
        // Known limit: a held 64 over a mono line on 45 to 52 shares its
        // whole span with the line, so it is no hold candidate and the range
        // leaves it out. `--profile pad` keeps it.
        let track = PitchTrack::from_frames(
            (0..400)
                .map(|i| TrackFrame {
                    time: s(cast::usize_to_f64(i) * 0.005),
                    f0: Some(Hz::from(pitch(48.0))),
                    level: Decibels::new(-3.0).unwrap(),
                })
                .collect(),
        );
        let probe = NoteList::new(vec![
            note(0.0, 0.5, 45.0),
            note(0.5, 1.0, 47.0),
            note(1.0, 1.5, 50.0),
            note(1.5, 2.0, 52.0),
            note(0.5, 1.5, 64.0),
        ]);
        let candidates = hold_candidates(&probe, &track);
        // The line notes under the stop share their span too.
        assert_eq!(candidates, vec![45.0, 52.0]);
        let (range, source, _) = choose_range(
            Voicing::Monophonic,
            Some((pitch(45.0), pitch(52.0))),
            Some((pitch(45.0), pitch(64.0))),
            &candidates,
        );
        assert_eq!(source, RegisterSource::F0Track);
        assert!(range.highest().get() < 64.0, "{}", range.highest().get());
    }

    #[test]
    fn ghosts_bleed_and_quiet_notes_do_not_test_the_f0_range() {
        // Line notes at -3 dB from 0 to 4 s with a gap at 3 s, bleed at
        // -30 dB in the gap and from 4 to 5 s. The ghosts at 57 and 69 and
        // the bleed at 72 over the line at 2 s share the frame level of the
        // line, and the bleed in the gap reads it in its edge frames.
        let track = PitchTrack::from_frames(
            (0..1000)
                .map(|i| TrackFrame {
                    time: s(cast::usize_to_f64(i) * 0.005),
                    f0: Some(Hz::from(pitch(45.0))),
                    level: Decibels::new(if i < 800 && !(600..700).contains(&i) {
                        -3.0
                    } else {
                        -30.0
                    })
                    .unwrap(),
                })
                .collect(),
        );
        let probe = NoteList::new(vec![
            note(0.0, 1.0, 45.0),
            note(0.1, 0.4, 57.0),
            note(0.2, 0.5, 69.0),
            note(1.0, 2.0, 47.0),
            note(2.0, 3.0, 48.0),
            note(2.1, 2.4, 72.0),
            note(2.95, 3.55, 64.0),
            note(4.2, 4.8, 72.0),
        ]);
        assert_eq!(hold_candidates(&probe, &track), vec![45.0, 47.0, 48.0]);
    }

    #[test]
    fn a_weaker_octave_twin_leaves_a_note_alone() {
        let track = PitchTrack::from_frames(
            (0..200)
                .map(|i| TrackFrame {
                    time: s(cast::usize_to_f64(i) * 0.005),
                    f0: Some(Hz::from(pitch(45.0))),
                    level: Decibels::new(-3.0).unwrap(),
                })
                .collect(),
        );
        let heard = |onset, offset, key, velocity| {
            note(onset, offset, key).with_velocity(crate::notes::Velocity::new(velocity).unwrap())
        };
        // A true A3 over its weak A2 twin is a hold candidate; the weak twin
        // under the stronger A3 is not.
        let probe = NoteList::new(vec![heard(0.2, 0.4, 57.0, 98), heard(0.19, 0.4, 45.0, 48)]);
        assert_eq!(hold_candidates(&probe, &track), vec![57.0]);
        // An octave of equal strength is a double stop, not a twin.
        let probe = NoteList::new(vec![heard(0.2, 0.4, 57.0, 90), heard(0.2, 0.4, 45.0, 90)]);
        assert_eq!(hold_candidates(&probe, &track), Vec::<f64>::new());
    }

    #[test]
    fn voicing_turns_polyphonic_at_the_chord_share() {
        assert_eq!(voicing_of(CHORD_SHARE), Voicing::Polyphonic);
        assert_eq!(voicing_of(CHORD_SHARE - 1e-9), Voicing::Monophonic);
        assert_eq!(voicing_of(0.0), Voicing::Monophonic);
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
    fn notes_held_together_are_chords_and_ghosts_or_late_ends_are_not() {
        // An octave ghost and a late end over a fifth of the next note.
        let bass = NoteList::new(vec![
            note(0.0, 1.2, 45.0),
            note(0.02, 0.3, 57.0),
            note(1.0, 2.0, 47.0),
        ]);
        assert_eq!(chord_share(&bass), 0.0);
        // A strum, a broken chord held on, and a moving voice under held
        // notes all count; the last note sounds alone for most of its span.
        let chords = NoteList::new(vec![
            note(0.0, 2.0, 57.0),
            note(0.03, 2.0, 61.0),
            note(0.5, 2.1, 64.0),
            note(1.0, 1.9, 59.0),
            note(1.9, 4.0, 62.0),
        ]);
        assert!((chord_share(&chords) - 0.8).abs() < 1e-12);
        // Half the span is enough, just below it is not.
        let pair = |end: f64| {
            chord_share(&NoteList::new(vec![
                note(0.0, 1.0, 57.0),
                note(0.5, end, 60.0),
            ]))
        };
        assert!((pair(1.0) - 1.0).abs() < 1e-12);
        assert!((pair(0.999) - 0.5).abs() < 1e-12);
        assert_eq!(chord_share(&NoteList::default()), 0.0);
    }
}
