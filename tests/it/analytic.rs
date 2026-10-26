//! The analytic signal and the heterodyne (#49).

use std::f64::consts::PI;

use sound_to_patch::audio::SampleRate;
use sound_to_patch::dsp::{AnalyticSignal, HeterodyneParams, am_depth, heterodyne};
use sound_to_patch::units::Hz;

use crate::support::golden::{Golden, assert_close, floats};
use crate::support::signals::{sine, times, tones};

const RATE: u32 = 8000;

fn rate() -> SampleRate {
    SampleRate::new(RATE).unwrap()
}

/// The middle half of a track.
fn middle(track: &[f64]) -> &[f64] {
    &track[track.len() / 4..track.len() * 3 / 4]
}

/// A raised-cosine fade over the first and the last second. A cut sine
/// leaves an edge error that decays only as `1 / d`; a smooth onset leaves
/// none worth measuring.
fn faded(x: &[f64]) -> Vec<f64> {
    let edge = f64::from(RATE);
    let len = f64::from(u32::try_from(x.len()).unwrap());
    let ramp = |u: f64| 0.5 - 0.5 * (PI * (u / edge).clamp(0.0, 1.0)).cos();
    x.iter()
        .enumerate()
        .map(|(n, v)| {
            let n = f64::from(u32::try_from(n).unwrap());
            v * ramp(n) * ramp(len - 1.0 - n)
        })
        .collect()
}

#[test]
fn a_noiseless_sine_has_a_flat_envelope_and_its_own_frequency() {
    // 123.4 Hz over 4 s is not a whole number of cycles.
    let x = faded(&sine(123.4, 0.3, f64::from(RATE), 32_000));
    let z = AnalyticSignal::new(&x, rate(), 8000);
    for a in middle(&z.envelope()) {
        assert!((a - 1.0).abs() < 1e-3, "envelope {a}");
    }
    for f in middle(&z.instantaneous_frequency()) {
        assert!((f - 123.4).abs() < 1e-3, "frequency {f}");
    }
}

#[test]
fn a_half_depth_am_tone_returns_depth_one_half() {
    let t = times(f64::from(RATE), 16_000);
    let x: Vec<f64> = t
        .iter()
        .map(|&t| (1.0 + 0.5 * (2.0 * PI * 4.0 * t).sin()) * (2.0 * PI * 500.0 * t).sin())
        .collect();
    let z = AnalyticSignal::new(&x, rate(), 4000);
    let depth = am_depth(middle(&z.envelope())).unwrap().get();
    assert!((depth - 0.5).abs() < 1e-3, "depth {depth}");
}

#[test]
fn a_tone_beside_the_centre_comes_out_at_its_offset() {
    let x = sine(1003.0, 0.0, f64::from(RATE), 32_000);
    let params = HeterodyneParams::new(Hz::new(1000.0).unwrap(), Hz::new(10.0).unwrap());
    let baseband = heterodyne(&x, rate(), params).unwrap();
    assert_eq!(baseband.rate().get(), 40.0);
    // Skip the first second, where the low-pass settles.
    for f in &baseband.instantaneous_frequency()[40..] {
        assert!((f - 3.0).abs() < 1e-3, "offset {f}");
    }
}

#[test]
fn the_analytic_signal_matches_the_reference() {
    // Reference: scipy.signal.hilbert of the zero-padded signal, see hilbert.json.
    let golden = Golden::load("hilbert");
    let recipe = golden.recipe();
    let x = tones(&recipe["signal"]);
    let pad = usize::try_from(recipe["pad"].as_u64().unwrap()).unwrap();
    let z = AnalyticSignal::new(&x, rate(), pad);
    let re: Vec<f64> = z.samples().iter().map(|c| c.re).collect();
    let im: Vec<f64> = z.samples().iter().map(|c| c.im).collect();
    assert_close(&re, &floats(&golden.values()["re"]), golden.tolerance());
    assert_close(&im, &floats(&golden.values()["im"]), golden.tolerance());
    assert_close(
        &z.envelope(),
        &floats(&golden.values()["envelope"]),
        golden.tolerance(),
    );
}
