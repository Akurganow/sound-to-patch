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
