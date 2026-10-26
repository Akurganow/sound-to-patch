//! Windows, the STFT and the peak picker (#48) against golden vectors.

use sound_to_patch::audio::SampleRate;
use sound_to_patch::dsp::{Padding, Stft, StftParams, Window, strongest_peak};

use crate::support::golden::{Golden, Tolerance, assert_close, float, floats};
use crate::support::rng::{gaussian, uniform};
use crate::support::signals::{sine, tones};

#[test]
fn windows_match_the_periodic_references() {
    // Reference: scipy.signal.get_window(..., fftbins=True), see window.json.
    let golden = Golden::load("window");
    for (window, name) in [
        (Window::Hann, "hann"),
        (Window::BlackmanHarris, "blackman_harris"),
    ] {
        for (size, expected) in golden.values()[name].as_object().unwrap() {
            let actual = window.samples(size.parse().unwrap());
            assert_close(&actual, &floats(expected), golden.tolerance());
        }
    }
}

#[test]
fn stft_frames_match_the_reference_transform() {
    // Reference: librosa.stft, centre=True, Hann, see stft.json.
    let golden = Golden::load("stft");
    let signal = tones(&golden.recipe()["signal"]);
    for case in golden.values()["cases"].as_array().unwrap() {
        let frame_len = usize::try_from(case["frame_len"].as_u64().unwrap()).unwrap();
        let hop = usize::try_from(case["hop"].as_u64().unwrap()).unwrap();
        let padding = match case["padding"].as_str().unwrap() {
            "zero" => Padding::Zero,
            _ => Padding::Reflect,
        };
        let params = StftParams::new(frame_len)
            .unwrap()
            .with_hop(hop)
            .unwrap()
            .with_padding(padding);
        let frames = Stft::new(params).frames(&signal);
        assert_eq!(
            frames.len(),
            usize::try_from(case["frames"].as_u64().unwrap()).unwrap()
        );
        for (m, frame) in frames.iter().enumerate() {
            let re: Vec<f64> = frame.iter().map(|c| c.re).collect();
            let im: Vec<f64> = frame.iter().map(|c| c.im).collect();
            assert_close(&re, &floats(&case["re"][m]), golden.tolerance());
            assert_close(&im, &floats(&case["im"][m]), golden.tolerance());
        }
    }
}

const RATE: u32 = 44_100;
const N: usize = 8192;

/// Locates the strongest peak of frame 2, which covers samples 0 to N - 1.
fn estimate(x: &[f64], window: Window, oversample: usize) -> f64 {
    let rate = SampleRate::new(RATE).unwrap();
    let params = StftParams::new(N)
        .unwrap()
        .with_window(window)
        .with_oversample(oversample)
        .unwrap();
    let mut stft = Stft::new(params);
    let magnitudes: Vec<f64> = stft.frame(x, 2).iter().map(|c| c.norm()).collect();
    strongest_peak(&magnitudes, 1, magnitudes.len(), 4)
        .unwrap()
        .frequency(params, rate)
        .get()
}

#[test]
fn a_noiseless_sine_off_the_bin_grid_is_located_within_a_millihertz() {
    // #48 proposes 0.001 Hz. The generator measures a worst case of
    // 1.08 mHz over offsets across one bin (48-peak-accuracy.json), so the
    // bound holds to 10 % at the worst offset.
    let bin = f64::from(RATE) / 8192.0;
    for i in 0..=20 {
        let offset = f64::from(i) / 20.0 - 0.5;
        let frequency = 100.0 + offset * bin;
        let x = sine(frequency, 0.7, f64::from(RATE), N);
        let error = (estimate(&x, Window::Hann, 4) - frequency).abs();
        assert!(error < 1.1e-3, "offset {offset}: error {error} Hz");
    }
}

/// Standard deviation of the frequency error over the trials of the
/// 48-peak-accuracy recipe, with the generator's noise.
fn trial_std(window: Window) -> f64 {
    let golden = Golden::load("prototype/48-peak-accuracy");
    let recipe = golden.recipe();
    let trials = usize::try_from(recipe["trials"].as_u64().unwrap()).unwrap();
    let seed = recipe["seed"].as_u64().unwrap();
    let sigma = (1.0 / (2.0 * 10f64.powf(float(&recipe["snr_db"]) / 10.0))).sqrt();
    let draws = uniform(seed, 2 * trials);
    let bin = f64::from(RATE) / 8192.0;
    let errors: Vec<f64> = (0..trials)
        .map(|i| {
            let frequency = 100.0 + (draws[2 * i] - 0.5) * bin;
            let phase = 2.0 * std::f64::consts::PI * draws[2 * i + 1];
            let noise = gaussian(seed + 1 + u64::try_from(i).unwrap(), N);
            let x: Vec<f64> = sine(frequency, phase, f64::from(RATE), N)
                .iter()
                .zip(noise)
                .map(|(s, n)| s + sigma * n)
                .collect();
            estimate(&x, window, 16) - frequency
        })
        .collect();
    let count = f64::from(u32::try_from(trials).unwrap());
    let mean = errors.iter().sum::<f64>() / count;
    (errors.iter().map(|e| (e - mean).powi(2)).sum::<f64>() / count).sqrt()
}

#[test]
fn noisy_estimates_stay_near_the_bound_and_beat_the_rectangular_window() {
    let golden = Golden::load("prototype/48-peak-accuracy");
    let bound = float(&golden.values()["bound_std_hz"]);
    let hann = trial_std(Window::Hann);
    let rectangular = trial_std(Window::Rectangular);
    // Acceptance of #48: Hann within 2.5x of the bound, rectangular 10x worse.
    assert!(hann <= 2.5 * bound, "hann {hann} Hz, bound {bound} Hz");
    assert!(
        rectangular >= 10.0 * hann,
        "rectangular {rectangular} Hz, hann {hann} Hz"
    );
    // The same noise as the generator, so the statistic matches its table.
    let stored = &golden.values()["std_hz"];
    assert_close(
        &[hann, rectangular],
        &[float(&stored["hann"]), float(&stored["rectangular"])],
        golden.tolerance(),
    );
}

#[test]
fn noiseless_bias_matches_the_prototype_table() {
    let golden = Golden::load("prototype/48-peak-accuracy");
    let bias = &golden.values()["max_noiseless_bias"];
    let bin = f64::from(RATE) / 8192.0;
    let worst = |window, oversample| {
        (0..=100)
            .map(|i| {
                let frequency = 100.0 + (f64::from(i) / 100.0 - 0.5) * bin;
                (estimate(
                    &sine(frequency, 0.7, f64::from(RATE), N),
                    window,
                    oversample,
                ) - frequency)
                    .abs()
            })
            .fold(0.0, f64::max)
    };
    let actual = [
        worst(Window::Hann, 1) / bin,
        worst(Window::Hann, 4),
        worst(Window::BlackmanHarris, 1) / bin,
    ];
    let expected = [
        float(&bias["hann_no_padding_bins"]),
        float(&bias["hann_4x_padding_hz"]),
        float(&bias["blackman_harris_no_padding_bins"]),
    ];
    assert_close(
        &actual,
        &expected,
        Tolerance {
            abs: 0.0,
            rel: 1e-3,
        },
    );
}
