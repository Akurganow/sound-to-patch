//! Stem descriptors (#54) against librosa, the prototype tables and closed
//! forms.

use sound_to_patch::audio::SampleRate;
use sound_to_patch::dsp::{Stft, StftParams, Summary, Window, describe, frame_descriptors};
use sound_to_patch::units::{Decibels, Hz};

use crate::support::golden::{Golden, Tolerance, assert_close, float, floats};
use crate::support::rng::gaussian;
use crate::support::signals::{bandlimited_saw, naive_saw, times, tones};

/// Magnitude frames divided by the window sum.
fn magnitude_frames(x: &[f64], frame_len: usize) -> Vec<Vec<f64>> {
    let params = StftParams::new(frame_len).unwrap();
    let window_sum: f64 = Window::Hann.samples(frame_len).iter().sum();
    Stft::new(params)
        .frames(x)
        .iter()
        .map(|frame| frame.iter().map(|c| c.norm() / window_sum).collect())
        .collect()
}

#[test]
fn frame_descriptors_match_the_reference_features() {
    // Reference: librosa.feature spectral_* on magnitude, see descriptors.json.
    let golden = Golden::load("descriptors");
    let x = tones(&golden.recipe()["signal"]);
    let bin = Hz::new(22_050.0 / 1024.0).unwrap();
    let frames: Vec<_> = magnitude_frames(&x, 1024)
        .iter()
        .map(|a| frame_descriptors(a, bin).unwrap())
        .collect();
    let values = golden.values();
    let column = |pick: fn(&sound_to_patch::dsp::FrameDescriptors) -> f64| {
        frames.iter().map(pick).collect::<Vec<_>>()
    };
    assert_close(
        &column(|d| d.centroid.get()),
        &floats(&values["centroid_hz"]),
        golden.tolerance(),
    );
    assert_close(
        &column(|d| d.spread.get()),
        &floats(&values["spread_hz"]),
        golden.tolerance(),
    );
    assert_close(
        &column(|d| d.rolloff85.get()),
        &floats(&values["rolloff85_hz"]),
        golden.tolerance(),
    );
    assert_close(
        &column(|d| d.rolloff95.get()),
        &floats(&values["rolloff95_hz"]),
        golden.tolerance(),
    );
    assert_close(
        &column(|d| d.flatness.get()),
        &floats(&values["flatness"]),
        golden.tolerance(),
    );
}

#[test]
fn the_sawtooth_table_reproduces_within_one_hertz() {
    let golden = Golden::load("prototype/54-sawtooth");
    let t = times(44_100.0, 88_200);
    let bin = Hz::new(44_100.0 / 4096.0).unwrap();
    for (key, row) in golden.values().as_object().unwrap() {
        let f0: f64 = key.parse().unwrap();
        let frames = magnitude_frames(&bandlimited_saw(f0, 44_100.0, &t, |_| 0.0), 4096);
        // Frames that lie wholly inside the signal.
        let inside: Vec<&Vec<f64>> = frames
            .iter()
            .enumerate()
            .filter(|(m, _)| m * 1024 >= 2048 && m * 1024 + 2048 <= 88_200)
            .map(|(_, frame)| frame)
            .collect();
        let median = |pick: &dyn Fn(&Vec<f64>) -> f64| {
            Summary::of(&inside.iter().map(|frame| pick(frame)).collect::<Vec<_>>())
                .unwrap()
                .median()
        };
        let magnitude = |frame: &Vec<f64>| frame_descriptors(frame, bin).unwrap();
        let power = |frame: &Vec<f64>| {
            frame_descriptors(&frame.iter().map(|a| a * a).collect::<Vec<_>>(), bin).unwrap()
        };
        let actual = [
            median(&|f| magnitude(f).centroid.get()),
            median(&|f| power(f).centroid.get()),
            median(&|f| magnitude(f).rolloff85.get()),
            median(&|f| power(f).rolloff85.get()),
        ];
        let expected = [
            float(&row["centroid_magnitude_hz"]),
            float(&row["centroid_power_hz"]),
            float(&row["rolloff85_magnitude_hz"]),
            float(&row["rolloff85_power_hz"]),
        ];
        assert_close(&actual, &expected, golden.tolerance());
        // The power centroid matches its closed form; the magnitude centroid
        // sits 0.3 % under its own because the window spreads each partial.
        assert!(
            (actual[1] - float(&row["closed_form_centroid_power_hz"])).abs() < 1.0,
            "{key}"
        );
    }
}

#[test]
fn white_noise_flatness_is_0_845_on_magnitude() {
    let golden = Golden::load("prototype/54-noise-flatness");
    let params = StftParams::new(4096).unwrap();
    let mut stft = Stft::new(params);
    let window_sum: f64 = Window::Hann.samples(4096).iter().sum();
    let bin = Hz::new(44_100.0 / 4096.0).unwrap();
    let flatness: f64 = (0..200)
        .map(|m| {
            let noise = gaussian(5400 + m, 4096);
            // Frame 2 covers samples 0 to 4095.
            let a: Vec<f64> = stft
                .frame(&noise, 2)
                .iter()
                .map(|c| c.norm() / window_sum)
                .collect();
            frame_descriptors(&a, bin).unwrap().flatness.get()
        })
        .sum::<f64>()
        / 200.0;
    assert!((flatness - 0.845).abs() < 0.01, "{flatness}");
    // The same noise as the generator.
    assert_close(
        &[flatness],
        &[float(&golden.values()["magnitude"])],
        Tolerance {
            abs: 1e-9,
            rel: 0.0,
        },
    );
}

fn crest(x: &[f64]) -> f64 {
    let rate = SampleRate::new(44_100).unwrap();
    let d = describe(x, rate, StftParams::new(4096).unwrap(), None, false).unwrap();
    d.crest_db.value().map(|db| Decibels::get(*db)).unwrap()
}

#[test]
fn crest_factors_match_closed_forms_and_the_saturated_table() {
    let golden = Golden::load("prototype/54-crest");
    let values = golden.values();
    let t = times(44_100.0, 44_100);
    let sine: Vec<f64> = t
        .iter()
        .map(|&t| (2.0 * std::f64::consts::PI * 110.0 * t).sin())
        .collect();
    let saw = naive_saw(110.0, &t);
    let square: Vec<f64> = sine
        .iter()
        .map(|&v| if v >= 0.0 { 1.0 } else { -1.0 })
        .collect();
    let closed = &values["closed_forms_db"];
    for (x, name) in [(&sine, "sine"), (&saw, "saw"), (&square, "square")] {
        assert!(
            (crest(x) - float(&closed[name])).abs() < 0.02,
            "{name}: {}",
            crest(x)
        );
    }
    let tanh = |x: &[f64]| x.iter().map(|v| (4.0 * v).tanh()).collect::<Vec<_>>();
    assert!((crest(&tanh(&sine)) - float(&values["tanh_sine"])).abs() < 0.05);
    assert!((crest(&tanh(&saw)) - float(&values["tanh_saw"])).abs() < 0.05);
}

#[test]
fn descriptors_of_a_tone_carry_frame_counts_and_band_shares() {
    let rate = SampleRate::new(44_100).unwrap();
    let t = times(44_100.0, 44_100);
    let x = bandlimited_saw(220.0, 44_100.0, &t, |_| 0.0);
    let f0 = Hz::new(220.0).ok();
    let d = describe(&x, rate, StftParams::new(4096).unwrap(), f0, false).unwrap();
    assert_eq!(d.active_frames, 44);
    let shares = d.band_share.value().unwrap();
    let total: f64 = shares.iter().map(|b| b.share.get()).sum();
    assert!((total - 1.0).abs() < 1e-12);
    // No partial lies below 60 Hz; only the start and stop leak there.
    assert!(shares[0].share.get() < 1e-3, "{}", shares[0].share.get());
    let ratio = d.centroid_over_f0.value().unwrap().median().get();
    let centroid = d.centroid.value().unwrap().median().get();
    assert!((ratio - centroid / 220.0).abs() < 1e-9);
}

#[test]
fn too_few_active_frames_give_no_statistic() {
    use sound_to_patch::confidence::ReasonCode;
    use sound_to_patch::dsp::MIN_ACTIVE_FRAMES;
    let rate = SampleRate::new(48_000).unwrap();
    let params = StftParams::new(4096).unwrap();
    let noise: Vec<f64> = gaussian(5, 48_000).iter().map(|v| 0.1 * v).collect();
    // A centred frame every 1024 samples: `k` frames need `(k - 1) 1024 + 1`
    // samples. One sample is one frame; 0.2 s at 48 kHz is 10.
    for frames in [1, 10, MIN_ACTIVE_FRAMES - 1] {
        let d = describe(&noise[..=(frames - 1) * 1024], rate, params, None, false).unwrap();
        assert_eq!(d.active_frames, frames);
        for estimate in [&d.centroid, &d.spread, &d.rolloff85, &d.rolloff95] {
            assert!(estimate.value().is_none(), "{frames} frames");
            let codes: Vec<_> = estimate
                .confidence()
                .reasons()
                .iter()
                .map(|r| r.code())
                .collect();
            assert!(codes.contains(&ReasonCode::TooFewFrames), "{codes:?}");
        }
        assert!(d.flatness.value().is_none() && d.band_share.value().is_none());
        // Levels need no frames.
        assert!(d.peak_dbfs.value().is_some());
    }
    let d = describe(
        &noise[..=(MIN_ACTIVE_FRAMES - 1) * 1024],
        rate,
        params,
        None,
        false,
    )
    .unwrap();
    assert_eq!(d.active_frames, MIN_ACTIVE_FRAMES);
    assert!(d.centroid.value().is_some() && d.band_share.value().is_some());
}

#[test]
fn bands_above_nyquist_are_dropped_and_the_crossing_band_ends_there() {
    let params = StftParams::new(4096).unwrap();
    let noise: Vec<f64> = gaussian(6, 32_000).iter().map(|v| 0.1 * v).collect();
    for (rate, count, top) in [
        (8_000, 4, 4000.0),
        (16_000, 5, 8000.0),
        (44_100, 6, 20_000.0),
    ] {
        let d = describe(&noise, SampleRate::new(rate).unwrap(), params, None, false).unwrap();
        let shares = d.band_share.value().unwrap();
        assert_eq!(shares.len(), count, "{rate} Hz");
        assert!((shares[count - 1].hi.get() - top).abs() < 1e-9, "{rate} Hz");
        let total: f64 = shares.iter().map(|b| b.share.get()).sum();
        assert!((total - 1.0).abs() < 1e-12);
        // White noise puts power in every band that remains.
        assert!(shares.iter().all(|b| b.share.get() > 0.0), "{rate} Hz");
    }
}
