//! The quiet-note guard: a note that sounds only where the stem is far
//! below its loudest passage is bleed or noise, not part of the stem.
//!
//! The model scales each 2 s window by its own level, so a near-silent
//! window with bleed fills its full range and gives notes. The guard
//! drops every note whose span holds no active frame: no frame within the
//! gate of the loudest frame of the stem.

use crate::audio::SampleRate;
use crate::cast;
use crate::dsp::{ActiveFrames, StftParams, frame_rms};
use crate::units::{Decibels, Seconds};

use super::NoteList;

/// The length of a guard frame: 25 ms, short enough to follow a note.
pub const QUIET_FRAME: Seconds = Seconds::from_const(0.025);

/// The gate of the guard: a frame is active within this many decibels of
/// the loudest frame. It is the project's active-frame gate.
pub const QUIET_GATE: Decibels = crate::dsp::ACTIVE_GATE_DB;

/// Drops every note whose span holds no active frame of the stem, and
/// returns the kept notes with the count of dropped ones.
///
/// Frames last [`QUIET_FRAME`] at the stem's own rate, with a quarter
/// frame hop. A frame belongs to a note when its centre lies at or after
/// the onset and before the offset; a note shorter than a hop takes the
/// frame nearest its onset.
#[must_use]
pub fn drop_quiet(
    notes: &NoteList,
    mono: &[f64],
    rate: SampleRate,
    gate: Decibels,
) -> (NoteList, usize) {
    let frame = cast::round_to_usize(QUIET_FRAME.get() * f64::from(rate.get()))
        .unwrap_or(4)
        .max(4);
    let params = StftParams::fixed(frame);
    let active = ActiveFrames::from_rms(&frame_rms(mono, params), gate);
    let frame_of = |time: Seconds| {
        cast::round_to_usize(time.get() * f64::from(rate.get()) / cast::usize_to_f64(params.hop()))
            .unwrap_or(0)
    };
    let before = notes.notes().len();
    let kept: Vec<_> = notes
        .notes()
        .iter()
        .filter(|note| {
            let first = frame_of(note.onset());
            let last = cast::ceil_to_usize(
                note.offset().get() * f64::from(rate.get()) / cast::usize_to_f64(params.hop()),
            )
            .unwrap_or(first)
            .max(first + 1);
            (first..last).any(|m| active.is_active(m))
        })
        .cloned()
        .collect();
    let dropped = before - kept.len();
    (NoteList::new(kept), dropped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::Note;
    use crate::units::MidiPitch;

    fn note(onset: f64, offset: f64) -> Note {
        Note::new(
            Seconds::new(onset).unwrap(),
            Seconds::new(offset).unwrap(),
            MidiPitch::new(60.0).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn a_note_in_a_passage_50_db_down_is_dropped_and_one_at_30_db_kept() {
        let rate = SampleRate::new(8000).unwrap();
        // 1 s at full scale, 1 s at -50 dB, 1 s at -30 dB.
        let mono: Vec<f64> = (0..24_000)
            .map(|n| {
                let level = match n / 8000 {
                    0 => 1.0,
                    1 => 10_f64.powf(-50.0 / 20.0),
                    _ => 10_f64.powf(-30.0 / 20.0),
                };
                let t = f64::from(n) / 8000.0;
                level * (std::f64::consts::TAU * 220.0 * t).sin()
            })
            .collect();
        let notes = NoteList::new(vec![
            note(0.2, 0.8),
            note(1.2, 1.8),
            note(2.2, 2.8),
            note(0.9, 1.5),
        ]);
        let (kept, dropped) = drop_quiet(&notes, &mono, rate, QUIET_GATE);
        assert_eq!(dropped, 1);
        let onsets: Vec<f64> = kept.notes().iter().map(|n| n.onset().get()).collect();
        assert_eq!(onsets, vec![0.2, 0.9, 2.2]);
    }

    #[test]
    fn silence_drops_every_note() {
        let rate = SampleRate::new(8000).unwrap();
        let (kept, dropped) = drop_quiet(
            &NoteList::new(vec![note(0.1, 0.2)]),
            &vec![0.0; 8000],
            rate,
            QUIET_GATE,
        );
        assert!(kept.is_empty());
        assert_eq!(dropped, 1);
    }
}
