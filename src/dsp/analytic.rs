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
pub const ATTACK_LOW_PASS_HZ: Hz = Hz::from_const(150.0);

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
    ///
    /// It is meant for note-sized segments. The transform holds the padded
    /// signal as complex values, 16 bytes a sample, and an FFT of a length
    /// that is not a power of two needs several times that in scratch: a
    /// whole 10-minute stem at 48 kHz peaks near 2 GB. Heterodyne a long
    /// signal instead.
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
        buffer.truncate(pad + samples.len());
        buffer.drain(..pad);
        Self {
            z: buffer,
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
    /// differentiating a noisy phase. One value fewer than samples. The
    /// per-sample track stays bare `f64` hertz: a value can be negative,
    /// which [`Hz`] rejects.
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
    /// negative below the centre, so the per-sample track is bare `f64`.
    #[must_use]
    pub fn instantaneous_frequency(&self) -> Vec<f64> {
        instantaneous_frequency(&self.z, self.rate)
    }
}

/// Moves the band around `centre` to baseband, so a tone at `f_c + d`
/// comes out at `d`.
///
/// It multiplies the signal by `2 exp(-j 2 pi f_c t)`, low-passes the
/// product with a fourth-order Butterworth at `cutoff`, and keeps every
/// `decimation`-th sample. A partial `A cos(2 pi f t + phi)` comes out as
/// `A exp(j (2 pi (f - f_c) t + phi))`, so the envelope reads `A`. The
/// first samples carry the filter's start-up transient.
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
            // The factor 2 restores the amplitude that the mixing halves.
            let mixed = 2.0 * x;
            Complex64::new(re.run(mixed * phase.cos()), im.run(-mixed * phase.sin()))
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
/// The two passes square the magnitude response, so the gain at `cutoff` is
/// -6 dB, not -3 dB, and the slope is 48 dB per octave. As `filtfilt` does,
/// each end is extended by an odd reflection, `2 x[0] - x[k]`, of three cutoff
/// periods, and each pass starts in the steady state of its first input. A
/// constant or a ramp then passes without a start-up or an end transient,
/// so a release at the end of a stem keeps its level.
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
    let pad = cast::ceil_to_usize(3.0 * rate.hz().get() / cutoff.get()).unwrap_or(0);
    zero_phase(samples, pad, Reflection::Odd, || LowPass::new(rate, cutoff))
}

/// How [`zero_phase`] extends the ends of a signal.
#[derive(Clone, Copy)]
enum Reflection {
    /// `2 x[0] - x[k]`: keeps the value and the slope, for a smooth signal.
    Odd,
    /// `x[k]`: keeps the value only. For a signal with a fast component that
    /// the filter removes, an odd reflection would turn that component's end
    /// value into a step the filter passes.
    Even,
}

/// Runs a filter forward and backward over `samples` extended at each end
/// by a reflection of up to `pad` samples.
fn zero_phase(
    samples: &[f64],
    pad: usize,
    reflection: Reflection,
    filter: impl Fn() -> Result<LowPass, FilterError>,
) -> Result<Vec<f64>, FilterError> {
    let (mut forward, mut backward) = (filter()?, filter()?);
    let (Some(&first), Some(&last)) = (samples.first(), samples.last()) else {
        return Ok(Vec::new());
    };
    let pad = pad.min(samples.len() - 1);
    let mirror = |end: f64, x: f64| match reflection {
        Reflection::Odd => 2.0 * end - x,
        Reflection::Even => x,
    };
    let mut out: Vec<f64> = samples[1..=pad]
        .iter()
        .rev()
        .map(|&x| mirror(first, x))
        .chain(samples.iter().copied())
        .chain(
            samples[samples.len() - 1 - pad..samples.len() - 1]
                .iter()
                .rev()
                .map(|&x| mirror(last, x)),
        )
        .collect();
    for pass in [&mut forward, &mut backward] {
        if let Some(&start) = out.first() {
            pass.settle(start);
        }
        for value in &mut out {
            *value = pass.run(*value);
        }
        out.reverse();
    }
    out.truncate(out.len() - pad);
    out.drain(..pad);
    Ok(out)
}

/// Band-passes a signal with a fourth-order Butterworth high-pass at `lo`
/// and low-pass at `hi`, each run forward and backward, so the band has no
/// delay.
///
/// Both edges get the ends of [`low_pass_zero_phase`]: an odd reflection of
/// three periods of `lo` and steady-state starts.
///
/// # Errors
///
/// Returns a [`FilterError`] when an edge is zero or not below the Nyquist
/// frequency.
pub fn band_pass_zero_phase(
    samples: &[f64],
    rate: SampleRate,
    lo: Hz,
    hi: Hz,
) -> Result<Vec<f64>, FilterError> {
    let pad = cast::ceil_to_usize(3.0 * rate.hz().get() / lo.get()).unwrap_or(0);
    let high = zero_phase(samples, pad, Reflection::Odd, || {
        LowPass::of(Type::HighPass, rate, lo)
    })?;
    zero_phase(&high, pad, Reflection::Odd, || LowPass::new(rate, hi))
}

/// The envelope of the band from `lo` to `hi`.
///
/// The signal is mixed down by `2 exp(-j 2 pi f_c t)` at the centre `f_c`,
/// low-passed forward and backward at half the bandwidth, and the magnitude
/// taken. A partial of amplitude `A` inside the band reads `A`.
///
/// Each low-pass extends the ends by an even reflection of three periods of
/// its cutoff, so the ends carry no transient, and memory stays linear in
/// the length, unlike the FFT of [`AnalyticSignal`].
///
/// # Errors
///
/// Returns a [`FilterError`] when `lo` is not above 0 Hz, `hi` not below
/// the Nyquist frequency, or `hi` not above `lo`.
pub fn band_magnitude(
    samples: &[f64],
    rate: SampleRate,
    lo: Hz,
    hi: Hz,
) -> Result<Vec<f64>, FilterError> {
    let nyquist = rate.nyquist().get();
    if lo.get() <= 0.0 || lo.get() >= nyquist {
        return Err(FilterError { cutoff: lo, rate });
    }
    if hi.get() <= lo.get() || hi.get() >= nyquist {
        return Err(FilterError { cutoff: hi, rate });
    }
    let centre = f64::midpoint(lo.get(), hi.get());
    let half = Hz::clamped(0.5 * (hi.get() - lo.get()));
    let step = TAU * centre / rate.hz().get();
    let (re, im): (Vec<f64>, Vec<f64>) = samples
        .iter()
        .enumerate()
        .map(|(n, &x)| {
            // Reduce the phase per sample so it stays exact on long stems.
            let (sin, cos) = ((step * cast::usize_to_f64(n)) % TAU).sin_cos();
            (2.0 * x * cos, -2.0 * x * sin)
        })
        .unzip();
    let pad = cast::ceil_to_usize(3.0 * rate.hz().get() / half.get()).unwrap_or(0);
    let filter = || LowPass::new(rate, half);
    let re = zero_phase(&re, pad, Reflection::Even, filter)?;
    let im = zero_phase(&im, pad, Reflection::Even, filter)?;
    Ok(re.iter().zip(&im).map(|(a, b)| a.hypot(*b)).collect())
}

/// A fourth-order Butterworth low-pass or high-pass as two biquad sections.
struct LowPass([(DirectForm2Transposed<f64>, Coefficients<f64>); 2]);

impl LowPass {
    fn new(rate: SampleRate, cutoff: Hz) -> Result<Self, FilterError> {
        Self::of(Type::LowPass, rate, cutoff)
    }

    fn of(kind: Type<f64>, rate: SampleRate, cutoff: Hz) -> Result<Self, FilterError> {
        let fail = || FilterError { cutoff, rate };
        if cutoff.get() <= 0.0 || cutoff.get() >= rate.nyquist().get() {
            return Err(fail());
        }
        let fs = rate.hz().get().hz();
        let f0 = cutoff.get().hz();
        let section = |q| {
            Coefficients::<f64>::from_params(kind, fs, f0, q)
                .map(|c| (DirectForm2Transposed::<f64>::new(c), c))
                .map_err(|_| fail())
        };
        Ok(Self([
            section(BUTTERWORTH_4[0])?,
            section(BUTTERWORTH_4[1])?,
        ]))
    }

    /// Sets the state that a constant input `x` holds forever, as
    /// `lfilter_zi` does.
    fn settle(&mut self, x: f64) {
        let mut input = x;
        for (section, c) in &mut self.0 {
            let output = input * (c.b0 + c.b1 + c.b2) / (1.0 + c.a1 + c.a2);
            section.s2 = c.b2 * input - c.a2 * output;
            section.s1 = section.s2 + c.b1 * input - c.a1 * output;
            input = output;
        }
    }

    fn run(&mut self, x: f64) -> f64 {
        let [(first, _), (second, _)] = &mut self.0;
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
            "a filter edge at {} cannot run at {}: it must lie above 0 Hz and below {}",
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

    #[test]
    fn the_attack_low_pass_constant_passes_the_unit_check() {
        assert_eq!(ATTACK_LOW_PASS_HZ, Hz::new(150.0).unwrap());
    }

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
            "a filter edge at 4000 Hz cannot run at 8000 Hz: it must lie above 0 Hz and below 4000 Hz"
        );
    }

    #[test]
    fn the_zero_phase_low_pass_passes_dc_and_keeps_its_length() {
        let out = low_pass_zero_phase(&vec![1.0; 4000], rate(), Hz::new(150.0).unwrap()).unwrap();
        assert_eq!(out.len(), 4000);
        assert!((out[2000] - 1.0).abs() < 1e-9, "{}", out[2000]);
    }

    #[test]
    fn the_zero_phase_low_pass_keeps_a_constant_and_a_ramp_at_both_ends() {
        let cutoff = Hz::new(150.0).unwrap();
        let ones = low_pass_zero_phase(&vec![1.0; 4000], rate(), cutoff).unwrap();
        assert!(ones.iter().all(|v| (v - 1.0).abs() < 1e-9), "{ones:?}");
        let ramp: Vec<f64> = (0..4000).map(|n| f64::from(n) / 4000.0).collect();
        let out = low_pass_zero_phase(&ramp, rate(), cutoff).unwrap();
        for (x, y) in ramp.iter().zip(&out) {
            assert!((x - y).abs() < 1e-5, "{x} {y}");
        }
    }

    #[test]
    fn the_zero_phase_low_pass_takes_one_or_no_sample() {
        let cutoff = Hz::new(150.0).unwrap();
        assert_eq!(
            low_pass_zero_phase(&[], rate(), cutoff).unwrap(),
            Vec::<f64>::new()
        );
        let one = low_pass_zero_phase(&[0.5], rate(), cutoff).unwrap();
        assert!((one[0] - 0.5).abs() < 1e-12);
    }

    #[test]
    fn default_decimation_keeps_four_samples_per_cutoff_period() {
        let params = HeterodyneParams::new(Hz::new(1000.0).unwrap(), Hz::new(10.0).unwrap());
        assert_eq!(params.decimation_for(rate()), 200);
        assert_eq!(params.with_decimation(7).decimation_for(rate()), 7);
    }
}
