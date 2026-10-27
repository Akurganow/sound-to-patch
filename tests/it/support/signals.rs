//! Test signals built from the recipes that the golden generator also reads.

use std::f64::consts::PI;

use serde_json::Value;

use super::rng::gaussian;

/// `n / rate` for each sample index.
pub(crate) fn times(rate: f64, length: usize) -> Vec<f64> {
    (0..length)
        .map(|n| f64::from(u32::try_from(n).unwrap()) / rate)
        .collect()
}

/// A sum of sines plus optional seeded Gaussian noise, as the `tones` recipe
/// of the generator describes.
pub(crate) fn tones(recipe: &Value) -> Vec<f64> {
    let rate = recipe["sample_rate"].as_f64().unwrap();
    let length = usize::try_from(recipe["length"].as_u64().unwrap()).unwrap();
    let t = times(rate, length);
    let mut x = vec![0.0; length];
    for tone in recipe["tones"].as_array().unwrap() {
        let amplitude = tone["amplitude"].as_f64().unwrap();
        let frequency = tone["frequency_hz"].as_f64().unwrap();
        let phase = tone["phase"].as_f64().unwrap();
        for (sample, &time) in x.iter_mut().zip(&t) {
            *sample += amplitude * (2.0 * PI * frequency * time + phase).sin();
        }
    }
    if let Some(noise) = recipe.get("noise") {
        let std = noise["std"].as_f64().unwrap();
        for (sample, n) in x
            .iter_mut()
            .zip(gaussian(noise["seed"].as_u64().unwrap(), length))
        {
            *sample += std * n;
        }
    }
    x
}

/// A sine of unit amplitude.
pub(crate) fn sine(frequency: f64, phase: f64, rate: f64, length: usize) -> Vec<f64> {
    times(rate, length)
        .into_iter()
        .map(|t| (2.0 * PI * frequency * t + phase).sin())
        .collect()
}

/// Linear convolution by FFT, cut to the length of `x`.
pub(crate) fn convolve(x: &[f64], h: &[f64]) -> Vec<f64> {
    use realfft::RealFftPlanner;
    let len = (x.len() + h.len()).next_power_of_two();
    let mut planner = RealFftPlanner::<f64>::new();
    let forward = planner.plan_fft_forward(len);
    let inverse = planner.plan_fft_inverse(len);
    let spectrum = |signal: &[f64]| {
        let mut input = vec![0.0; len];
        input[..signal.len()].copy_from_slice(signal);
        let mut output = forward.make_output_vec();
        forward.process(&mut input, &mut output).unwrap();
        output
    };
    let mut product: Vec<_> = spectrum(x)
        .iter()
        .zip(spectrum(h))
        .map(|(a, b)| a * b)
        .collect();
    let mut out = vec![0.0; len];
    inverse.process(&mut product, &mut out).unwrap();
    let scale = f64::from(u32::try_from(len).unwrap());
    out.truncate(x.len());
    out.iter().map(|v| v / scale).collect()
}

/// A band-limited sawtooth as the generator builds it: harmonic `h` at
/// amplitude `1 / h` below the Nyquist frequency, `sin(2 pi h f0 t - shift(h))`.
pub(crate) fn bandlimited_saw(
    f0: f64,
    rate: f64,
    t: &[f64],
    shift: impl Fn(f64) -> f64,
) -> Vec<f64> {
    let mut x = vec![0.0; t.len()];
    let mut h = 1.0;
    while h * f0 < rate / 2.0 {
        let s = shift(h);
        for (sample, &time) in x.iter_mut().zip(t) {
            *sample += (2.0 * PI * h * f0 * time - s).sin() / h;
        }
        h += 1.0;
    }
    x
}

/// A naive sawtooth ramp, `2 (f t mod 1) - 1`.
pub(crate) fn naive_saw(f: f64, t: &[f64]) -> Vec<f64> {
    t.iter()
        .map(|&time| 2.0 * (f * time).rem_euclid(1.0) - 1.0)
        .collect()
}
