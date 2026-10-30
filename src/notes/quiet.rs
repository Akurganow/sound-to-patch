//! The quiet-note guard: a note that sounds only where the stem is far
//! below its loudest passage is bleed or noise, not part of the stem.
//!
//! The model scales each 2 s window by its own level, so a near-silent
//! window with bleed fills its full range and gives notes. The guard
//! drops every note whose span holds no active frame: no frame within the
//! gate of the loudest frame of the stem.

use std::ops::Range;

use crate::audio::SampleRate;
use crate::cast;
use crate::dsp::{ActiveFrames, StftParams, Window, frame_rms};
use crate::units::{Decibels, Hz, MidiPitch, Seconds};

use super::NoteList;

/// The length of a guard frame: 25 ms, short enough to follow a note.
pub const QUIET_FRAME: Seconds = Seconds::from_const(0.025);

/// The gate of the guard: a frame is active within this many decibels of
/// the loudest frame. It is the project's active-frame gate.
pub const QUIET_GATE: Decibels = crate::dsp::ACTIVE_GATE_DB;

/// The stem's level in [`QUIET_FRAME`] frames at its own rate, with a
/// quarter-frame hop, frame `m` centred at `m` hops. The quiet-note guard,
/// the attack split and the split join read the same frames.
#[derive(Clone, Debug)]
pub struct StemLevels<'a> {
    mono: &'a [f64],
    rate: SampleRate,
    params: StftParams,
    rms: Vec<f64>,
    window: Vec<f64>,
}

impl<'a> StemLevels<'a> {
    /// Measures the RMS of every frame of `mono` at `rate`.
    #[must_use]
    pub fn new(mono: &'a [f64], rate: SampleRate) -> Self {
        let frame = cast::round_to_usize(QUIET_FRAME.get() * f64::from(rate.get()))
            .unwrap_or(4)
            .max(4);
        let params = StftParams::fixed(frame);
        Self {
            mono,
            rate,
            params,
            rms: frame_rms(mono, params),
            window: Window::Hann.samples(params.frame_len()),
        }
    }

    /// Returns the RMS of each frame.
    #[must_use]
    pub fn rms(&self) -> &[f64] {
        &self.rms
    }

    /// Returns the RMS of frame `frame` about its own mean: the level with
    /// any DC offset removed.
    pub(crate) fn ac_rms(&self, frame: usize) -> f64 {
        self.ac_level(frame * self.params.hop(), self.params.frame_len())
    }

    /// Returns the time at which frame `frame` is centred.
    pub(crate) fn frame_time(&self, frame: usize) -> Seconds {
        Seconds::clamped(cast::usize_to_f64(frame * self.params.hop()) / f64::from(self.rate.get()))
    }

    fn hops(&self, time: Seconds) -> f64 {
        time.get() * f64::from(self.rate.get()) / cast::usize_to_f64(self.params.hop())
    }

    /// Returns sample `offset` of the `len` samples centred at sample
    /// `centre`. Samples outside the stem count as silence.
    fn sample(&self, centre: usize, offset: usize, len: usize) -> f64 {
        (centre + offset)
            .checked_sub(len / 2)
            .and_then(|index| self.mono.get(index))
            .copied()
            .unwrap_or(0.0)
    }

    /// Returns the RMS about its own mean of the `len` samples centred at
    /// sample `centre`.
    fn ac_level(&self, centre: usize, len: usize) -> f64 {
        let (sum, energy) = (0..len)
            .map(|offset| self.sample(centre, offset, len))
            .fold((0.0, 0.0), |(sum, energy), x| (sum + x, energy + x * x));
        let count = cast::usize_to_f64(len);
        let mean = sum / count;
        (energy / count - mean * mean).max(0.0).sqrt()
    }

    /// Returns the RMS about its own mean of the `window` of samples
    /// centred at `time`: the envelope with any DC offset removed. Samples
    /// outside the stem count as silence.
    #[must_use]
    pub fn envelope(&self, time: Seconds, window: Seconds) -> f64 {
        let rate = f64::from(self.rate.get());
        let len = cast::round_to_usize(window.get() * rate)
            .unwrap_or(1)
            .max(1);
        self.ac_level(cast::round_to_usize(time.get() * rate).unwrap_or(0), len)
    }

    /// Returns the frames centred from `from` to `to`, both included.
    #[must_use]
    pub fn frames_between(&self, from: Seconds, to: Seconds) -> Range<usize> {
        let first = cast::ceil_to_usize(self.hops(from)).unwrap_or(0);
        let end = cast::floor_to_usize(self.hops(to))
            .map_or(0, |last| last + 1)
            .min(self.rms.len());
        first.min(end)..end
    }

    /// Returns the level of frame `frame` at the first [`HARMONICS`]
    /// harmonics of `pitch` below the Nyquist frequency: the root of the
    /// summed power of each, from a Hann window over the frame with the
    /// frame's mean removed. Samples outside the stem count as silence.
    ///
    /// The window's main lobe spans 160 Hz, so below about A2 (110 Hz) it
    /// cannot tell a note's fundamental from a tone a fifth or a tritone
    /// away.
    #[must_use]
    pub fn harmonic_level(&self, frame: usize, pitch: MidiPitch) -> f64 {
        let len = self.params.frame_len();
        let rate = f64::from(self.rate.get());
        let centre = frame * self.params.hop();
        let raw: Vec<f64> = (0..len)
            .map(|offset| self.sample(centre, offset, len))
            .collect();
        let mean = raw.iter().sum::<f64>() / cast::usize_to_f64(len);
        let windowed: Vec<f64> = raw
            .iter()
            .zip(&self.window)
            .map(|(x, weight)| (x - mean) * weight)
            .collect();
        let f0 = Hz::from(pitch).get();
        (1..=HARMONICS)
            .map(|h| f0 * f64::from(h))
            .take_while(|&frequency| frequency < rate / 2.0)
            .map(|frequency| bin_power(&windowed, frequency, rate))
            .sum::<f64>()
            .sqrt()
    }

    /// Returns the level at the fundamental of `pitch` of the one period of
    /// samples centred at `time`, with their mean removed: one DFT bin over
    /// the period, unnormalised, so only ratios of two readings mean
    /// anything. The bin's zeros fall on every harmonic of the note when
    /// the period is a whole number of samples. It is rounded to one, so at
    /// 22,050 Hz and at the worst phase a harmonic leaks about -70 dB at
    /// A1, -46 to -50 dB from A2 to A5, -23 dB at A6 and -17 dB at A7. The
    /// bin rejects only the note's harmonics: a key 1 to 3 semitones up
    /// reads within 1 dB of the note, and a fifth up 2 to 6 dB under it.
    /// Samples outside the stem count as silence.
    #[must_use]
    pub fn fundamental_level(&self, time: Seconds, pitch: MidiPitch) -> f64 {
        let rate = f64::from(self.rate.get());
        let f0 = Hz::from(pitch).get();
        let len = cast::round_to_usize(rate / f0).unwrap_or(1).max(1);
        let centre = cast::round_to_usize(time.get() * rate).unwrap_or(0);
        let raw: Vec<f64> = (0..len)
            .map(|offset| self.sample(centre, offset, len))
            .collect();
        let mean = raw.iter().sum::<f64>() / cast::usize_to_f64(len);
        let centred: Vec<f64> = raw.iter().map(|x| x - mean).collect();
        bin_power(&centred, f0, rate).sqrt()
    }
}

/// The power of one DFT bin of `samples` at `frequency`, at `rate`, by the
/// Goertzel recurrence, which reads any frequency.
fn bin_power(samples: &[f64], frequency: f64, rate: f64) -> f64 {
    let coefficient = 2.0 * (std::f64::consts::TAU * frequency / rate).cos();
    let (mut s1, mut s2) = (0.0, 0.0);
    for x in samples {
        let s0 = x + coefficient * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    (s1 * s1 + s2 * s2 - coefficient * s1 * s2).max(0.0)
}

/// The harmonics of a note that [`StemLevels::harmonic_level`] sums.
pub const HARMONICS: u8 = 8;

/// Drops every note whose span holds no active frame of the stem, and
/// returns the kept notes with the count of dropped ones.
///
/// A note's frames run from the frame nearest its onset, which can be
/// centred up to half a hop before it, to the last frame centred before
/// its offset; a note shorter than a hop takes the frame nearest its
/// onset.
#[must_use]
pub fn drop_quiet(
    notes: &NoteList,
    mono: &[f64],
    rate: SampleRate,
    gate: Decibels,
) -> (NoteList, usize) {
    drop_quiet_with(notes, &StemLevels::new(mono, rate), gate)
}

/// Drops the notes as [`drop_quiet`] does, over levels already measured.
#[must_use]
pub fn drop_quiet_with(
    notes: &NoteList,
    levels: &StemLevels<'_>,
    gate: Decibels,
) -> (NoteList, usize) {
    let active = ActiveFrames::from_rms(levels.rms(), gate);
    let before = notes.notes().len();
    let kept: Vec<_> = notes
        .notes()
        .iter()
        .filter(|note| {
            let first = cast::round_to_usize(levels.hops(note.onset())).unwrap_or(0);
            let last = cast::ceil_to_usize(levels.hops(note.offset()))
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

    /// 1 s of silence, then 1 s of a 220 Hz sine at full scale, at 8 kHz:
    /// frames of 200 samples, a hop of 6.25 ms.
    fn silence_then_tone() -> Vec<f64> {
        (0..16_000)
            .map(|n| {
                let t = f64::from(n) / 8000.0;
                if n < 8000 {
                    0.0
                } else {
                    (std::f64::consts::TAU * 220.0 * t).sin()
                }
            })
            .collect()
    }

    #[test]
    fn a_note_that_reaches_into_a_loud_passage_is_kept() {
        let rate = SampleRate::new(8000).unwrap();
        let notes = NoteList::new(vec![note(0.5, 1.03), note(0.5, 0.9)]);
        let (kept, dropped) = drop_quiet(&notes, &silence_then_tone(), rate, QUIET_GATE);
        assert_eq!(dropped, 1);
        assert!((kept.notes()[0].offset().get() - 1.03).abs() < 1e-9);
    }

    #[test]
    fn a_note_in_a_10_ms_gap_is_kept_by_the_25_ms_frames_over_it() {
        let rate = SampleRate::new(8000).unwrap();
        // The tone, 10 ms of silence from 1 s, then the tone again.
        let tone = silence_then_tone().split_off(8000);
        let mut mono = tone.clone();
        mono.extend(std::iter::repeat_n(0.0, 80));
        mono.extend(&tone);
        let notes = NoteList::new(vec![note(1.001, 1.008)]);
        let (kept, dropped) = drop_quiet(&notes, &mono, rate, QUIET_GATE);
        assert_eq!((kept.notes().len(), dropped), (1, 0));
    }

    #[test]
    fn a_note_shorter_than_a_hop_takes_the_frame_at_its_onset() {
        let rate = SampleRate::new(8000).unwrap();
        // From 0.6 to 0.9 of hop 240, inside the tone: the onset rounds up
        // to frame 241, and the offset reaches no further.
        let hop = 0.006_25;
        let notes = NoteList::new(vec![note(240.6 * hop, 240.9 * hop)]);
        let (kept, dropped) = drop_quiet(&notes, &silence_then_tone(), rate, QUIET_GATE);
        assert_eq!((kept.notes().len(), dropped), (1, 0));
    }

    /// A sine of amplitude 1 at `frequency` from sample `start` of 1 s at
    /// `rate`.
    fn sine_from(rate: u32, frequency: f64, start: usize) -> Vec<f64> {
        (0..rate)
            .map(|n| {
                let t = f64::from(n) / f64::from(rate);
                if (n as usize) < start {
                    0.0
                } else {
                    (std::f64::consts::TAU * frequency * t).sin()
                }
            })
            .collect()
    }

    #[test]
    fn frames_between_holds_both_ends() {
        let rate = SampleRate::new(8000).unwrap();
        let mono = vec![0.0; 8000];
        let levels = StemLevels::new(&mono, rate);
        let s = |value| Seconds::new(value).unwrap();
        assert_eq!(levels.frames_between(s(0.0), s(0.0125)), 0..3);
        assert_eq!(levels.frames_between(s(0.001), s(0.0125)), 1..3);
        assert_eq!(
            levels.frames_between(s(0.99), s(2.0)).end,
            levels.rms().len()
        );
    }

    #[test]
    fn the_harmonic_level_reads_each_harmonic_in_its_own_frame() {
        // At 8 kHz a frame holds 200 samples: a full sine of amplitude 1
        // reads half the Hann window's sum, 50.
        let rate = SampleRate::new(8000).unwrap();
        let a4 = MidiPitch::new(69.0).unwrap();
        for frequency in [440.0, 1320.0] {
            let mono = sine_from(8000, frequency, 4000);
            let levels = StemLevels::new(&mono, rate);
            // Frame 80 is centred on sample 4000, where the sine starts.
            assert!(levels.harmonic_level(78, a4).abs() < 1e-12);
            let level = levels.harmonic_level(82, a4);
            assert!((level - 50.0).abs() < 0.5, "{frequency} Hz reads {level}");
        }
    }

    #[test]
    fn the_envelope_reads_the_rms_of_its_window_about_its_mean() {
        // At 8 kHz 10 ms hold 80 samples, 4.4 periods of 440 Hz: close to
        // the RMS of a sine, 0.707, with or without a DC offset.
        let rate = SampleRate::new(8000).unwrap();
        let s = |value| Seconds::new(value).unwrap();
        let mono = sine_from(8000, 440.0, 4000);
        let offset: Vec<f64> = mono.iter().map(|x| x + 0.5).collect();
        for stem in [&mono, &offset] {
            let levels = StemLevels::new(stem, rate);
            let full = levels.envelope(s(0.75), s(0.01));
            assert!(
                (full - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.02,
                "{full}"
            );
        }
        // Centred on the start of the sine, half the window is silence.
        let half = StemLevels::new(&mono, rate).envelope(s(0.5), s(0.01));
        assert!((half - 0.5).abs() < 0.03, "{half}");
    }

    #[test]
    fn a_dc_offset_leaves_the_harmonic_level_alone() {
        let rate = SampleRate::new(8000).unwrap();
        let a1 = MidiPitch::new(33.0).unwrap();
        let mono = sine_from(8000, 55.0, 0);
        let offset: Vec<f64> = mono.iter().map(|x| x + 0.5).collect();
        let clean = StemLevels::new(&mono, rate).harmonic_level(80, a1);
        let shifted = StemLevels::new(&offset, rate).harmonic_level(80, a1);
        assert!(
            (shifted / clean - 1.0).abs() < 0.05,
            "{shifted} against {clean}"
        );
    }

    #[test]
    fn below_a2_the_harmonic_level_hears_a_fifth_as_the_note() {
        // At A1 a tone a fifth up, at 82.4 Hz, reads within 3 dB of A1
        // itself: the window cannot resolve them.
        let rate = SampleRate::new(8000).unwrap();
        let a1 = MidiPitch::new(33.0).unwrap();
        let level = |frequency| {
            let mono = sine_from(8000, frequency, 0);
            StemLevels::new(&mono, rate).harmonic_level(80, a1)
        };
        let ratio = level(82.407) / level(55.0);
        assert!(
            ratio > 10_f64.powf(-3.0 / 20.0),
            "the fifth reads {ratio} of A1"
        );
    }

    #[test]
    fn the_harmonic_level_stops_below_the_nyquist_frequency() {
        // At 7040 Hz the eighth harmonic of A4 lies on the Nyquist frequency.
        let nyquist: Vec<f64> = (0..7040)
            .map(|n| if n % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let levels = StemLevels::new(&nyquist, SampleRate::new(7040).unwrap());
        assert!(levels.harmonic_level(80, MidiPitch::new(69.0).unwrap()) < 1.0);
        // At 8 kHz the fifth harmonic of 1250 Hz lies above it, and the
        // sixth would alias to 1750 Hz.
        let pitch = MidiPitch::new(69.0 + 12.0 * (1250.0_f64 / 440.0).log2()).unwrap();
        let mono = sine_from(8000, 1750.0, 0);
        let levels = StemLevels::new(&mono, SampleRate::new(8000).unwrap());
        assert!(levels.harmonic_level(80, pitch) < 1.0);
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
