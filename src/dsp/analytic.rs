//! The analytic signal: envelope, phase and instantaneous frequency, plus
//! the band-pass heterodyne that moves one partial to baseband.
//!
//! The analytic signal means something only for one narrowband component
//! (the Bedrosian condition). Band-pass around one partial first; the phase
//! of a full-band stem is meaningless.

use std::error::Error;
use std::f64::consts::{PI, TAU};
use std::fmt;

use biquad::{Biquad, Coefficients, DirectForm2Transposed, ToHertz, Type};
use realfft::num_complex::Complex64;
use rustfft::FftPlanner;

use crate::audio::SampleRate;
use crate::cast;
use crate::units::{Hz, Normalized};

/// The default low-pass for an attack envelope. A 5 Hz smoother smears any
/// attack shorter than tens of milliseconds; 100 to 200 Hz keeps it.
pub const ATTACK_LOW_PASS_HZ: f64 = 150.0;

/// Q values of the two sections of a fourth-order Butterworth low-pass:
/// `1 / (2 cos(pi / 8))` and `1 / (2 cos(3 pi / 8))`.
const BUTTERWORTH_4: [f64; 2] = [0.541_196_100_146_197, 1.306_562_964_876_376_7];

/// The analytic signal `z = x + j H{x}` of a real signal.
#[derive(Clone, Debug, PartialEq)]
pub struct AnalyticSignal {
    z: Vec<Complex64>,
    rate: Hz,
}

impl AnalyticSignal {
    /// Computes the analytic signal by FFT: the negative bins are zeroed,
    /// the positive bins doubled, and the DC bin and, for an even length,
    /// the Nyquist bin keep gain 1.
    ///
    /// The FFT wraps circularly, so `pad` zeros go before and after the
    /// signal and are cut after the transform. The cut ends still carry an
    /// error that decays as `1 / d` with the distance `d` from an end,
    /// about `fs / (2 pi^2 f d)` for a sine of frequency `f`: 0.1 % needs
    /// some 4000 samples at 123 Hz and 8 kHz. Mirroring the signal instead
    /// of zeros is worse, because a reversed partial runs backwards in phase.
    #[must_use]
    pub fn new(samples: &[f64], rate: SampleRate, pad: usize) -> Self {
        let mut buffer: Vec<Complex64> = std::iter::repeat_n(0.0, pad)
            .chain(samples.iter().copied())
            .chain(std::iter::repeat_n(0.0, pad))
            .map(|re| Complex64::new(re, 0.0))
            .collect();
        let len = buffer.len();
        if len > 0 {
            let mut planner = FftPlanner::<f64>::new();
            planner.plan_fft_forward(len).process(&mut buffer);
            let half = len / 2;
            for (bin, value) in buffer.iter_mut().enumerate() {
                let gain = if bin == 0 || (len.is_multiple_of(2) && bin == half) {
                    1.0
                } else if bin <= (len - 1) / 2 {
                    2.0
                } else {
                    0.0
                };
                *value *= gain / cast::usize_to_f64(len);
            }
            planner.plan_fft_inverse(len).process(&mut buffer);
        }
        Self {
            z: buffer[pad..pad + samples.len()].to_vec(),
            rate: rate.hz(),
        }
    }

    /// Returns the complex samples.
    #[must_use]
    pub fn samples(&self) -> &[Complex64] {
        &self.z
    }

    /// Returns the envelope `|z|`.
    #[must_use]
    pub fn envelope(&self) -> Vec<f64> {
        self.z.iter().map(|value| value.norm()).collect()
    }

    /// Returns the unwrapped phase `arg z` in radians.
    #[must_use]
    pub fn phase(&self) -> Vec<f64> {
        unwrap_phase(&self.z)
    }

    /// Returns the instantaneous frequency in hertz between consecutive
    /// samples, `f[n] = (fs / 2 pi) arg(z[n+1] conj(z[n]))`. Avoids
    /// differentiating a noisy phase. One value fewer than samples.
    #[must_use]
    pub fn instantaneous_frequency(&self) -> Vec<f64> {
        instantaneous_frequency(&self.z, self.rate)
    }
}

fn unwrap_phase(z: &[Complex64]) -> Vec<f64> {
    let mut phase = Vec::with_capacity(z.len());
    let mut previous: Option<(f64, f64)> = None;
    for value in z {
        let raw = value.arg();
        let unwrapped = match previous {
            None => raw,
            Some((last_raw, last)) => {
                let mut step = raw - last_raw;
                step -= TAU * ((step + PI) / TAU).floor();
                last + step
            }
        };
        previous = Some((raw, unwrapped));
        phase.push(unwrapped);
    }
    phase
}

fn instantaneous_frequency(z: &[Complex64], rate: Hz) -> Vec<f64> {
    z.windows(2)
        .map(|pair| (pair[1] * pair[0].conj()).arg() * rate.get() / TAU)
        .collect()
}

/// The modulation depth of an envelope, `(max - min) / (max + min)`.
/// Returns `None` for an empty or all-zero envelope.
#[must_use]
pub fn am_depth(envelope: &[f64]) -> Option<Normalized> {
    let max = envelope.iter().copied().reduce(f64::max)?;
    let min = envelope.iter().copied().reduce(f64::min)?;
    if max + min <= 0.0 {
        return None;
    }
    Normalized::new((max - min) / (max + min)).ok()
}

/// Settings of a band-pass heterodyne.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeterodyneParams {
    centre: Hz,
    cutoff: Hz,
    decimation: usize,
}

impl HeterodyneParams {
    /// Shifts `centre` to 0 Hz and keeps `cutoff` hertz on either side. The
    /// decimation factor defaults to the largest that keeps four samples per
    /// cutoff period: `floor(fs / (4 cutoff))`, at least 1.
    #[must_use]
    pub fn new(centre: Hz, cutoff: Hz) -> Self {
        Self {
            centre,
            cutoff,
            decimation: 0,
        }
    }

    /// Sets the decimation factor; 0 restores the default.
    #[must_use]
    pub fn with_decimation(self, decimation: usize) -> Self {
        Self { decimation, ..self }
    }

    fn decimation_for(self, rate: SampleRate) -> usize {
        if self.decimation > 0 {
            return self.decimation;
        }
        cast::floor_to_usize(rate.hz().get() / (4.0 * self.cutoff.get()))
            .unwrap_or(1)
            .max(1)
    }
}

/// One partial moved to baseband: complex samples at a reduced rate.
#[derive(Clone, Debug, PartialEq)]
pub struct Baseband {
    z: Vec<Complex64>,
    rate: Hz,
}

impl Baseband {
    /// Returns the complex baseband samples.
    #[must_use]
    pub fn samples(&self) -> &[Complex64] {
        &self.z
    }

    /// Returns the baseband sample rate, `fs / decimation`.
    #[must_use]
    pub fn rate(&self) -> Hz {
        self.rate
    }

    /// Returns the envelope `|z|`.
    #[must_use]
    pub fn envelope(&self) -> Vec<f64> {
        self.z.iter().map(|value| value.norm()).collect()
    }

    /// Returns the instantaneous frequency offset from the centre in hertz;
    /// negative below the centre.
    #[must_use]
    pub fn instantaneous_frequency(&self) -> Vec<f64> {
        instantaneous_frequency(&self.z, self.rate)
    }
}

/// Moves the band around `centre` to baseband, so a tone at `f_c + d`
/// comes out at `d`.
///
/// It multiplies the signal by `exp(-j 2 pi f_c t)`, low-passes the product
/// with a fourth-order Butterworth at `cutoff`, and keeps every
/// `decimation`-th sample. The first samples carry the filter's start-up
/// transient.
///
/// # Errors
///
/// Returns a [`FilterError`] when the cutoff is zero or not below the
/// Nyquist frequency.
pub fn heterodyne(
    samples: &[f64],
    rate: SampleRate,
    params: HeterodyneParams,
) -> Result<Baseband, FilterError> {
    let mut re = LowPass::new(rate, params.cutoff)?;
    let mut im = LowPass::new(rate, params.cutoff)?;
    let decimation = params.decimation_for(rate);
    let step = TAU * params.centre.get() / rate.hz().get();
    let z = samples
        .iter()
        .enumerate()
        .map(|(n, &x)| {
            // Reduce the phase per sample so it stays exact on long stems.
            let phase = (step * cast::usize_to_f64(n)) % TAU;
            Complex64::new(re.run(x * phase.cos()), im.run(-x * phase.sin()))
        })
        .step_by(decimation)
        .collect();
    Ok(Baseband {
        z,
        rate: Hz::clamped(rate.hz().get() / cast::usize_to_f64(decimation)),
    })
}

/// Low-passes a signal forward and then backward with a fourth-order
/// Butterworth, so the result has no delay: an attack stays where it is.
/// For attack envelopes use [`ATTACK_LOW_PASS_HZ`].
///
/// # Errors
///
/// Returns a [`FilterError`] when the cutoff is zero or not below the
/// Nyquist frequency.
pub fn low_pass_zero_phase(
    samples: &[f64],
    rate: SampleRate,
    cutoff: Hz,
) -> Result<Vec<f64>, FilterError> {
    let mut forward = LowPass::new(rate, cutoff)?;
    let mut backward = LowPass::new(rate, cutoff)?;
    let mut out: Vec<f64> = samples.iter().map(|&x| forward.run(x)).collect();
    for value in out.iter_mut().rev() {
        *value = backward.run(*value);
    }
    Ok(out)
}

/// A fourth-order Butterworth low-pass as two biquad sections.
struct LowPass([DirectForm2Transposed<f64>; 2]);

impl LowPass {
    fn new(rate: SampleRate, cutoff: Hz) -> Result<Self, FilterError> {
        let fail = || FilterError { cutoff, rate };
        if cutoff.get() <= 0.0 || cutoff.get() >= rate.nyquist().get() {
            return Err(fail());
        }
        let fs = rate.hz().get().hz();
        let f0 = cutoff.get().hz();
        let section = |q| {
            Coefficients::<f64>::from_params(Type::LowPass, fs, f0, q)
                .map(DirectForm2Transposed::<f64>::new)
                .map_err(|_| fail())
        };
        Ok(Self([
            section(BUTTERWORTH_4[0])?,
            section(BUTTERWORTH_4[1])?,
        ]))
    }

    fn run(&mut self, x: f64) -> f64 {
        let [first, second] = &mut self.0;
        second.run(first.run(x))
    }
}

/// A filter cutoff that the sample rate cannot carry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FilterError {
    cutoff: Hz,
    rate: SampleRate,
}

impl fmt::Display for FilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a low-pass at {} cannot run at {}: the cutoff must lie above 0 Hz and below {}",
            self.cutoff,
            self.rate,
            self.rate.nyquist()
        )
    }
}

impl Error for FilterError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn rate() -> SampleRate {
        SampleRate::new(8_000).unwrap()
    }

    #[test]
    fn the_analytic_signal_of_a_cosine_is_a_unit_phasor() {
        // An integer number of cycles needs no padding.
        let x: Vec<f64> = (0..800)
            .map(|n| (TAU * 100.0 * f64::from(n) / 8000.0).cos())
            .collect();
        let z = AnalyticSignal::new(&x, rate(), 0);
        assert!(z.envelope().iter().all(|a| (a - 1.0).abs() < 1e-12));
        assert!(
            z.instantaneous_frequency()
                .iter()
                .all(|f| (f - 100.0).abs() < 1e-9)
        );
        let phase = z.phase();
        let expected = TAU * 100.0 * 799.0 / 8000.0;
        assert!((phase[799] - expected).abs() < 1e-9, "{}", phase[799]);
    }

    #[test]
    fn am_depth_of_a_flat_or_empty_envelope() {
        assert_eq!(am_depth(&[2.0, 2.0]).unwrap().get(), 0.0);
        assert!((am_depth(&[0.5, 1.5]).unwrap().get() - 0.5).abs() < 1e-15);
        assert!(am_depth(&[]).is_none());
        assert!(am_depth(&[0.0, 0.0]).is_none());
    }

    #[test]
    fn a_cutoff_at_nyquist_fails_with_the_limit() {
        let error = low_pass_zero_phase(&[0.0], rate(), Hz::new(4000.0).unwrap()).unwrap_err();
        assert_eq!(
            error.to_string(),
            "a low-pass at 4000 Hz cannot run at 8000 Hz: the cutoff must lie above 0 Hz and below 4000 Hz"
        );
    }

    #[test]
    fn the_zero_phase_low_pass_passes_dc_and_keeps_its_length() {
        let out = low_pass_zero_phase(&vec![1.0; 4000], rate(), Hz::new(150.0).unwrap()).unwrap();
        assert_eq!(out.len(), 4000);
        assert!((out[2000] - 1.0).abs() < 1e-9, "{}", out[2000]);
    }

    #[test]
    fn default_decimation_keeps_four_samples_per_cutoff_period() {
        let params = HeterodyneParams::new(Hz::new(1000.0).unwrap(), Hz::new(10.0).unwrap());
        assert_eq!(params.decimation_for(rate()), 200);
        assert_eq!(params.with_decimation(7).decimation_for(rate()), 7);
    }
}
