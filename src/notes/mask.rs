//! Harmonic masks from a note list, and the energy outside them.

use crate::audio::SampleRate;
use crate::cast;
use crate::dsp::{Stft, StftParams};
use crate::units::{Hz, Normalized, Seconds};

use super::NoteList;

/// Settings of the harmonic masks.
///
/// The mask of note `n` at frequency `f` and time `t` is
/// `M_n(f, t) = sum_{h=1..H} exp(-(f - h f_n(t))^2 / (2 sigma^2))`, where
/// `f_n(t)` follows the note's contour. A cell lies inside a mask when the
/// value reaches the threshold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaskParams {
    harmonics: usize,
    sigma: Option<Hz>,
    threshold: f64,
}

impl MaskParams {
    /// 20 harmonics, `sigma` of one unpadded bin, `fs / frame_len`, and a
    /// threshold of `exp(-2)`, the mask value two `sigma` from a harmonic.
    /// That covers the 4-bin main lobe of a Hann window. Zero padding narrows
    /// the bins but not the lobe, so `sigma` ignores the padding.
    #[must_use]
    pub fn new() -> Self {
        Self {
            harmonics: 20,
            sigma: None,
            threshold: (-2.0_f64).exp(),
        }
    }

    /// Sets the number of harmonics `H`. Harmonics at or above the Nyquist
    /// frequency are skipped.
    #[must_use]
    pub fn with_harmonics(self, harmonics: usize) -> Self {
        Self { harmonics, ..self }
    }

    /// Sets `sigma`, for example to the unison spread when it is known.
    #[must_use]
    pub fn with_sigma(self, sigma: Hz) -> Self {
        Self {
            sigma: Some(sigma),
            ..self
        }
    }

    /// Sets the mask value a cell needs to count as inside.
    #[must_use]
    pub fn with_threshold(self, threshold: Normalized) -> Self {
        Self {
            threshold: threshold.get(),
            ..self
        }
    }
}

impl Default for MaskParams {
    fn default() -> Self {
        Self::new()
    }
}

/// The masks of one frame: for each bin the note that claims it and
/// whether a second note claims it too.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameMask {
    owner: Vec<Option<usize>>,
    best: Vec<f64>,
    claims: Vec<u8>,
}

impl FrameMask {
    /// An empty mask for a frame of `bins` bins.
    #[must_use]
    pub fn new(bins: usize) -> Self {
        Self {
            owner: vec![None; bins],
            best: vec![0.0; bins],
            claims: vec![0; bins],
        }
    }

    /// Returns the index into the note list of the note with the largest
    /// mask value at a bin, or `None` outside every mask.
    #[must_use]
    pub fn owner(&self, bin: usize) -> Option<usize> {
        self.owner.get(bin).copied().flatten()
    }

    /// Returns whether the bin lies inside some mask.
    #[must_use]
    pub fn is_masked(&self, bin: usize) -> bool {
        self.owner(bin).is_some()
    }

    /// Returns whether two or more notes claim the bin, as octaves and
    /// fifths do with their shared partials.
    #[must_use]
    pub fn is_shared(&self, bin: usize) -> bool {
        self.claims.get(bin).is_some_and(|&claims| claims >= 2)
    }

    fn clear(&mut self) {
        self.owner.fill(None);
        self.best.fill(0.0);
        self.claims.fill(0);
    }
}

/// Builds the masks of a note list on the bins of an STFT layout.
#[derive(Clone, Debug)]
pub struct Masker<'a> {
    notes: &'a NoteList,
    harmonics: usize,
    threshold: f64,
    sigma: f64,
    bin_hz: f64,
    nyquist: f64,
    scratch: Vec<f64>,
}

impl<'a> Masker<'a> {
    /// Prepares the masks of `notes` for frames of `stft` at `rate`.
    #[must_use]
    pub fn new(
        notes: &'a NoteList,
        params: MaskParams,
        stft: StftParams,
        rate: SampleRate,
    ) -> Self {
        let bin_hz = stft.bin_width(rate).get();
        let lobe_bin = rate.hz().get() / cast::usize_to_f64(stft.frame_len());
        Self {
            notes,
            harmonics: params.harmonics,
            threshold: params.threshold,
            sigma: params.sigma.map_or(lobe_bin, Hz::get),
            bin_hz,
            nyquist: rate.nyquist().get(),
            scratch: vec![0.0; stft.bins()],
        }
    }

    /// Fills `out` with the masks of the notes that sound at `time`, the
    /// centre of a frame.
    pub fn frame(&mut self, time: Seconds, out: &mut FrameMask) {
        out.clear();
        let bins = out.owner.len().min(self.scratch.len());
        // Beyond 4 sigma a Gaussian term is below 3.4e-4 of its peak.
        let reach = 4.0 * self.sigma;
        for (index, note) in self.notes.notes().iter().enumerate() {
            if !note.sounds_at(time) {
                continue;
            }
            let f0 = note.f0_at(time).get();
            let mut touched = (usize::MAX, 0);
            for h in 1..=self.harmonics {
                let centre = cast::usize_to_f64(h) * f0;
                if centre >= self.nyquist {
                    break;
                }
                let lo = cast::ceil_to_usize((centre - reach) / self.bin_hz).unwrap_or(0);
                let hi = cast::floor_to_usize((centre + reach) / self.bin_hz)
                    .unwrap_or(0)
                    .min(bins.saturating_sub(1));
                for bin in lo..=hi {
                    let distance = cast::usize_to_f64(bin) * self.bin_hz - centre;
                    self.scratch[bin] +=
                        (-distance * distance / (2.0 * self.sigma * self.sigma)).exp();
                }
                touched = (touched.0.min(lo), touched.1.max(hi));
            }
            if touched.0 > touched.1 {
                continue;
            }
            for bin in touched.0..=touched.1 {
                let value = std::mem::take(&mut self.scratch[bin]);
                if value >= self.threshold {
                    out.claims[bin] = out.claims[bin].saturating_add(1);
                    if value > out.best[bin] {
                        out.best[bin] = value;
                        out.owner[bin] = Some(index);
                    }
                }
            }
        }
    }
}

/// Returns the share of the signal's power that lies outside every mask.
///
/// That share is the bleed of other instruments plus any note the list
/// missed. Power is `|X|^2` over all STFT cells; a frame where no note
/// sounds counts as outside. Returns `None` for digital silence.
#[must_use]
pub fn out_of_mask_fraction(
    samples: &[f64],
    rate: SampleRate,
    stft: StftParams,
    notes: &NoteList,
    params: MaskParams,
) -> Option<Normalized> {
    let mut transform = Stft::new(stft);
    let mut masker = Masker::new(notes, params, stft, rate);
    let mut mask = FrameMask::new(stft.bins());
    let (mut outside, mut total) = (0.0, 0.0);
    for frame in 0..stft.frame_count(samples.len()) {
        masker.frame(stft.frame_time(frame, rate), &mut mask);
        for (bin, value) in transform.frame(samples, frame).iter().enumerate() {
            let power = value.norm_sqr();
            total += power;
            if !mask.is_masked(bin) {
                outside += power;
            }
        }
    }
    (total > 0.0)
        .then(|| Normalized::new(outside / total).ok())
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::Note;
    use crate::units::MidiPitch;

    fn notes(pitches: &[f64]) -> NoteList {
        NoteList::new(
            pitches
                .iter()
                .map(|&pitch| {
                    Note::new(
                        Seconds::new(0.0).unwrap(),
                        Seconds::new(10.0).unwrap(),
                        MidiPitch::new(pitch).unwrap(),
                    )
                    .unwrap()
                })
                .collect(),
        )
    }

    fn mask_of(list: &NoteList) -> FrameMask {
        mask_with(list, MaskParams::new())
    }

    /// The mask at 1 s with bins of 10.77 Hz, 44.1 kHz over 4096 samples.
    fn mask_with(list: &NoteList, params: MaskParams) -> FrameMask {
        let rate = SampleRate::new(44_100).unwrap();
        let stft = StftParams::new(4096).unwrap();
        let mut masker = Masker::new(list, params, stft, rate);
        let mut mask = FrameMask::new(stft.bins());
        masker.frame(Seconds::new(1.0).unwrap(), &mut mask);
        mask
    }

    #[test]
    fn an_octave_pair_shares_cells_and_a_single_note_shares_none() {
        let single = mask_of(&notes(&[45.0]));
        assert!((0..2049).all(|bin| !single.is_shared(bin)));
        // 110 Hz sits at bin 10.2 of 10.77 Hz.
        assert_eq!(single.owner(10), Some(0));
        let octave = mask_of(&notes(&[45.0, 57.0]));
        // 220 Hz is harmonic 2 of A2 and harmonic 1 of A3.
        assert!(octave.is_shared(20));
        assert!(!octave.is_shared(10));
    }

    #[test]
    fn a_silent_frame_clears_the_mask_of_the_frame_before() {
        let list = notes(&[45.0]);
        let rate = SampleRate::new(44_100).unwrap();
        let stft = StftParams::new(4096).unwrap();
        let mut masker = Masker::new(&list, MaskParams::new(), stft, rate);
        let mut mask = FrameMask::new(stft.bins());
        masker.frame(Seconds::new(1.0).unwrap(), &mut mask);
        assert!(mask.is_masked(10));
        masker.frame(Seconds::new(11.0).unwrap(), &mut mask);
        assert!((0..stft.bins()).all(|bin| !mask.is_masked(bin)));
    }

    #[test]
    fn each_parameter_narrows_the_mask_of_a2() {
        // 110 Hz lies 2.33 Hz above the centre of bin 10.
        let a2 = notes(&[45.0]);
        let default = mask_of(&a2);
        assert!(default.is_masked(10) && default.is_masked(20));
        let fundamental = mask_with(&a2, MaskParams::new().with_harmonics(1));
        assert!(fundamental.is_masked(10) && !fundamental.is_masked(20));
        let narrow = mask_with(&a2, MaskParams::new().with_sigma(Hz::new(1.0).unwrap()));
        assert!(!narrow.is_masked(10));
        let strict = mask_with(
            &a2,
            MaskParams::new().with_threshold(Normalized::new(0.99).unwrap()),
        );
        assert!(!strict.is_masked(10));
    }

    #[test]
    fn a_note_on_one_bin_masks_that_bin() {
        let bin_hz = 44_100.0 / 4096.0;
        let pitch = MidiPitch::try_from(Hz::new(10.0 * bin_hz).unwrap()).unwrap();
        let list = notes(&[pitch.get()]);
        let params = MaskParams::new()
            .with_harmonics(1)
            .with_sigma(Hz::new(0.1).unwrap());
        let mask = mask_with(&list, params);
        assert_eq!(mask.owner(10), Some(0));
        assert!(!mask.is_masked(9) && !mask.is_masked(11));
    }

    #[test]
    fn of_two_equal_notes_the_first_owns_the_bin() {
        let mask = mask_of(&notes(&[45.0, 45.0]));
        assert_eq!(mask.owner(10), Some(0));
        assert!(mask.is_shared(10));
    }
}
