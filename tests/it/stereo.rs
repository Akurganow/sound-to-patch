//! The stereo screen (#47) against closed forms and its prototype table.

use std::f64::consts::PI;

use sound_to_patch::audio::{Band, SampleRate, StereoStats, stereo_screen};
use sound_to_patch::units::{Correlation, Decibels, Degrees, Hz};

use crate::support::cast::round_to_usize;
use crate::support::golden::{Golden, assert_close, float};
use crate::support::rng::{gaussian, uniform};
use crate::support::signals::{bandlimited_saw, convolve, times};

const RATE: f64 = 44_100.0;
const LN_1000: f64 = 6.907_755_278_982_137;

fn rate() -> SampleRate {
    SampleRate::new(44_100).unwrap()
}

fn full(left: &[f64], right: &[f64], f0: Option<f64>) -> StereoStats {
    stereo_screen(left, right, rate(), f0.and_then(|f| Hz::new(f).ok()))
        .unwrap()
        .band(Band::Full)
}

fn rms(x: &[f64]) -> f64 {
    (x.iter().map(|v| v * v).sum::<f64>() / f64::from(u32::try_from(x.len()).unwrap())).sqrt()
}

/// Removes the mean and scales to unit RMS.
fn normalise(x: &[f64]) -> Vec<f64> {
    let mean = x.iter().sum::<f64>() / f64::from(u32::try_from(x.len()).unwrap());
    let centred: Vec<f64> = x.iter().map(|v| v - mean).collect();
    let scale = rms(&centred);
    centred.iter().map(|v| v / scale).collect()
}

#[test]
fn independent_noise_channels_are_uncorrelated_with_side_equal_to_mid() {
    let left = normalise(&gaussian(4740, 88_200));
    let right = normalise(&gaussian(4741, 88_200));
    let stats = full(&left, &right, None);
    assert!(stats.rho.unwrap().get().abs() < 0.02, "{stats:?}");
    assert!(stats.sm_db.unwrap().get().abs() < 0.2, "{stats:?}");
}

#[test]
fn a_pure_delay_reads_360_f_tau_degrees() {
    let golden = Golden::load("stereo");
    let delay = &golden.values()["pure_delay"];
    let (f, tau) = (float(&delay["frequency_hz"]), float(&delay["delay_s"]));
    let t = times(RATE, 88_200);
    let left: Vec<f64> = t.iter().map(|&t| (2.0 * PI * f * t).sin()).collect();
    let right: Vec<f64> = t
        .iter()
        .map(|&t| (2.0 * PI * f * (t - tau)).sin())
        .collect();
    let ipd = full(&left, &right, Some(f)).ipd_deg.unwrap().get();
    // Reference: the generator's STFT pipeline in numpy and librosa, see
    // stereo.json; the closed form is 360 f tau.
    assert_close(&[ipd], &[float(&delay["ipd_deg"])], golden.tolerance());
    assert!(
        (ipd - float(&delay["closed_form_ipd_deg"])).abs() < 0.05,
        "{ipd}"
    );
}

#[test]
fn side_over_mid_follows_rho_for_channels_of_equal_rms() {
    for (seed, mix) in [(1_u64, 0.2), (2, 0.5), (3, 0.9)] {
        let a = gaussian(4750 + seed, 44_100);
        let b = gaussian(4760 + seed, 44_100);
        let left = normalise(&a);
        let mixed: Vec<f64> = a
            .iter()
            .zip(&b)
            .map(|(a, b)| mix * a + (1.0 - mix) * b)
            .collect();
        let right = normalise(&mixed);
        let stats = full(&left, &right, None);
        let rho = stats.rho.unwrap().get();
        let ratio = 10f64.powf(stats.sm_db.unwrap().get() / 10.0);
        let identity = (1.0 - rho) / (1.0 + rho);
        assert!(
            (ratio / identity - 1.0).abs() < 1e-6,
            "{ratio} vs {identity}"
        );
    }
}

/// The rows of the 47-stereo-screen recipe, built as the generator builds
/// them.
fn pair(row: &str, t: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let f0 = 220.0;
    let saw = bandlimited_saw(f0, RATE, t, |_| 0.0);
    let delayed = |delay: f64| bandlimited_saw(f0, RATE, t, |h| 2.0 * PI * h * f0 * delay);
    match row {
        "mono" => (saw.clone(), saw),
        "delay_0_4_ms" => (saw, delayed(0.0004)),
        "haas_10_ms" => (saw, delayed(0.010)),
        "velvet_20_ms" => (convolve(&saw, &velvet(4710)), convolve(&saw, &velvet(4711))),
        "chorus" => {
            let channel = |phi: f64| -> Vec<f64> {
                let mut x = saw.clone();
                let mut h = 1.0;
                while h * f0 < RATE / 2.0 {
                    for (sample, &time) in x.iter_mut().zip(t) {
                        let delay = 0.007 + 0.002 * (2.0 * PI * 0.8 * time + phi).sin();
                        *sample += (2.0 * PI * h * f0 * (time - delay)).sin() / h;
                    }
                    h += 1.0;
                }
                x
            };
            (channel(0.0), channel(PI))
        }
        _ if row.starts_with("reverb") => {
            let wet: f64 = row.split('_').nth(1).unwrap().parse().unwrap();
            let channel = |seed: u64| -> Vec<f64> {
                let n = 66_150;
                let ir: Vec<f64> = times(RATE, n)
                    .iter()
                    .zip(gaussian(seed, n))
                    .map(|(t, g)| g * (-LN_1000 * t).exp())
                    .collect();
                let norm = ir.iter().map(|v| v * v).sum::<f64>().sqrt();
                let ir: Vec<f64> = ir.iter().map(|v| v / norm).collect();
                let tail = convolve(&saw, &ir);
                saw.iter()
                    .zip(tail)
                    .map(|(d, w)| (1.0 - wet) * d + wet * w)
                    .collect()
            };
            (channel(4730), channel(4731))
        }
        _ => unison(row, t),
    }
}

fn velvet(seed: u64) -> Vec<f64> {
    let cell = 44;
    let draws = uniform(seed, 40);
    let mut v = vec![0.0; 20 * cell];
    for c in 0..20 {
        let offset = round_to_usize((draws[2 * c] * 44.0).floor());
        v[c * cell + offset] = if draws[2 * c + 1] < 0.5 { 1.0 } else { -1.0 };
    }
    v
}

fn unison(row: &str, t: &[f64]) -> (Vec<f64>, Vec<f64>) {
    let mut parts = row.split('_').skip(1);
    let voices: usize = parts.next().unwrap().parse().unwrap();
    let width: f64 = parts.next().unwrap().parse().unwrap();
    let count = f64::from(u32::try_from(voices).unwrap());
    let step = 50.0 / (count - 1.0);
    let detunes: Vec<f64> = (0..voices)
        .map(|v| -25.0 + f64::from(u32::try_from(v).unwrap()) * step)
        .collect();
    let mut order: Vec<usize> = (0..voices).collect();
    order.sort_by(|&a, &b| detunes[a].abs().total_cmp(&detunes[b].abs()));
    let max_rank = (count - 1.0) / 2.0;
    let (mut left, mut right) = (vec![0.0; t.len()], vec![0.0; t.len()]);
    for (rank_index, &v) in order.iter().enumerate() {
        let rank = f64::from(u32::try_from(rank_index.div_ceil(2)).unwrap());
        let side = if rank_index % 2 == 1 { 1.0 } else { -1.0 };
        let pan = side * width * rank / max_rank;
        let phase = 2.0 * PI * uniform(4720 + u64::try_from(v).unwrap(), 1)[0];
        let f = 220.0 * 2f64.powf(detunes[v] / 1200.0);
        let voice = bandlimited_saw(f, RATE, t, |h| -phase * h);
        let (gain_l, gain_r) = (
            (PI / 4.0 * (1.0 + pan)).cos(),
            (PI / 4.0 * (1.0 + pan)).sin(),
        );
        for (i, sample) in voice.iter().enumerate() {
            left[i] += gain_l * sample;
            right[i] += gain_r * sample;
        }
    }
    (left, right)
}

#[test]
fn the_screen_reproduces_its_prototype_table() {
    // Tolerance of #47: 10 % or 2 degrees, whichever is larger.
    let golden = Golden::load("prototype/47-stereo-screen");
    let t = times(RATE, 66_150);
    for (row, expected) in golden.values().as_object().unwrap() {
        let (left, right) = pair(row, &t);
        let stats = full(&left, &right, Some(220.0));
        let close = |actual: Option<f64>, key: &str| match (actual, expected[key].as_f64()) {
            (Some(a), Some(e)) => {
                let bound = if key == "ipd_deg" {
                    (0.1 * e.abs()).max(2.0)
                } else {
                    0.1 * e.abs()
                };
                assert!((a - e).abs() <= bound, "{row} {key}: {a} vs {e}");
            }
            (None, None) => {}
            (a, e) => panic!("{row} {key}: {a:?} vs {e:?}"),
        };
        close(stats.rho.map(Correlation::get), "rho");
        close(stats.sm_db.map(Decibels::get), "sm_db");
        close(stats.ipd_deg.map(Degrees::get), "ipd_deg");
    }
}
