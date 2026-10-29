//! Stem intake: decoding, mono, resampling and the [`Stem`].
//!
//! A file decodes at its native rate. A consumer that needs another rate
//! resamples. The [`Stem`] adds the beat grid, the band edge, the flat-top
//! flag and the stereo screen.
//!
//! Samples are `f64` from the decoder on. Every channel is kept, because the
//! stereo statistics need both. Mono is the channel mean, with no gain
//! normalisation. Only a consumer with a fixed input rate, such as the
//! transcription model, resamples; analysis stays at the native rate.
//!
//! ```no_run
//! use sound_to_patch::audio::{self, SampleRate};
//!
//! let stem = audio::decode("bass.wav".as_ref())?.into_audio();
//! let mono = stem.mono();
//! let model_rate = SampleRate::new(22_050)?;
//! let resampled = audio::resample(&mono, stem.sample_rate(), model_rate)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

mod decode;
mod resample;
mod stem;
mod stereo;
mod tempo;

pub use decode::{DecodeError, Decoded, MAX_SKIPPED_PACKETS, SkippedPacket, decode};
pub use resample::{ResampleError, resample};
#[doc(inline)]
pub use sound_to_patch_core::audio::{
    Audio, AudioError, SampleRate, mono_mean, write_wav, write_wav_to,
};
pub use stem::{
    BAND_EDGE_CONVENTION, BAND_EDGE_MARGIN_DB, BAND_EDGE_PLATEAU_DB, BAND_EDGE_STEEP_DROP_DB,
    BeatGrid, CEILING_TOLERANCE, CLIP_APPROACH, Division, FLAT_TOP_MIN_RUN_S, FLAT_TOP_MIN_SHARE,
    Feel, HARD_LIMITED_CONVENTION, LONG_FRAME_BELOW_HZ, NoteValue, Stem, band_edge, is_flat_topped,
};
pub use stereo::{
    Band, HIGH_EDGE_HZ, SCREEN_FRAME_LEN, SCREEN_HARMONICS, STEREO_CONVENTION, SUB_EDGE_HZ,
    StereoScreen, StereoStats, stereo_screen,
};
pub use tempo::{
    BPM_STEP, MAX_BEAT_LAGS, MIN_BEAT_LAGS, OVERLAP_BEATS, PEAK_RATIO_MIN, PERIODICITY_MIN,
    SEARCH_SHARE, SHARPNESS_MIN, SPARSE_ONSETS_MAX, SPARSE_PERIODICITY_MIN, TEMPO_CONVENTION,
    Tempo,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_decoder_reads_a_written_file_back() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("two.wav");
        let left = vec![0.0, 0.5, -0.25, 1.0];
        let right = vec![-1.0, 0.125, 0.0, 0.75];
        let audio = Audio::new(SampleRate::new(44_100).unwrap(), vec![left, right]).unwrap();
        write_wav(&path, &audio).unwrap();
        assert_eq!(decode(&path).unwrap().into_audio(), audio);
    }
}
