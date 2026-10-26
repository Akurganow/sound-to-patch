//! The signal core that every estimator uses: windows, the STFT, peak
//! picking, active frames with their statistics, the analytic signal and
//! decay fits.
//!
//! Conventions that bind every estimator:
//!
//! - Samples and spectra are `f64` at the stem's native rate.
//! - The window is the periodic Hann, `w[n] = 0.5 - 0.5 cos(2 pi n / N)`.
//! - Frames are centred and zero-padded, with hop `N/4`. The layer chooses
//!   `N` from the median f0 of the note list: 8192 below 100 Hz, else 4096.
//!   A Hann main lobe is about 4 bins wide, so harmonics `f0` apart need
//!   `N >= 4 fs / f0`.
//! - A frame is active when its RMS lies within 40 dB of the loudest frame.
//!   Statistics over active frames are the median and the interquartile
//!   range, which resist outliers.
//! - The transforms are not normalised; the caller scales.

mod analytic;
mod decay;
mod frames;
mod peak;
mod stft;
mod window;

pub use analytic::{
    ATTACK_LOW_PASS_HZ, AnalyticSignal, Baseband, FilterError, HeterodyneParams, am_depth,
    heterodyne, low_pass_zero_phase,
};
pub use decay::{Decay, DecayFailure, FitRange, backward_integral};
pub use frames::{ACTIVE_GATE_DB, ActiveFrames, Summary, frame_rms};
pub use peak::{Peak, interpolate, local_maxima, strongest_peak};
pub use realfft::num_complex::Complex64;
pub use stft::{Padding, Stft, StftError, StftParams};
pub use window::Window;
