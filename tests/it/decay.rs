//! Backward integration on tails after a stop (#53).

use sound_to_patch::audio::SampleRate;
use sound_to_patch::dsp::{Decay, DecayFailure};
use sound_to_patch::units::Seconds;

use crate::support::cast::round_to_usize;
use crate::support::golden::{Golden, float};
use crate::support::rng::gaussian;
use crate::support::signals::times;

const RATE: f64 = 16_000.0;
const LN_1000: f64 = 6.907_755_278_982_137;

fn rate() -> SampleRate {
    SampleRate::new(16_000).unwrap()
}

fn samples(seconds: f64) -> usize {
    round_to_usize(seconds * RATE)
}

/// Exponential-decay noise as an impulse response: power 1 at the start and
/// 60 dB lower after `rt60`.
fn decay_noise(rt60: f64, length: usize, seed: u64) -> Vec<f64> {
    times(RATE, length)
        .into_iter()
        .zip(gaussian(seed, length))
        .map(|(t, n)| n * (-LN_1000 * t / rt60).exp())
        .collect()
}

/// The response plus a noise floor `floor_db` below its start, 2 RT60 long.
fn with_floor(rt60: f64, floor_db: Option<f64>, seed: u64) -> Vec<f64> {
    let length = samples(2.0 * rt60);
    let mut x = decay_noise(rt60, length, seed);
    if let Some(floor) = floor_db {
        let gain = 10f64.powf(floor / 20.0);
        for (v, n) in x.iter_mut().zip(gaussian(seed + 50_000, length)) {
            *v += gain * n;
        }
    }
    x
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let mid = values.len() / 2;
    if values.len() % 2 == 1 {
        values[mid]
    } else {
        f64::midpoint(values[mid - 1], values[mid])
    }
}

/// Median T20 and T30 over 15 seeded trials, or the failure of any trial.
fn fits(
    rt60: f64,
    floor_db: Option<f64>,
) -> (Result<f64, DecayFailure>, Result<f64, DecayFailure>) {
    let decays: Vec<Decay> = (0..15)
        .map(|trial| Decay::of(&with_floor(rt60, floor_db, 9300 + trial), rate()))
        .collect();
    let gather = |pick: fn(&Decay) -> Result<f64, DecayFailure>| {
        decays
            .iter()
            .map(pick)
            .collect::<Result<Vec<_>, _>>()
            .map(median)
    };
    (
        gather(|d| d.t20().map(Seconds::get)),
        gather(|d| d.t30().map(Seconds::get)),
    )
}

#[test]
fn closed_forms_hold() {
    // decay.json stores -60 / RT60 and ln(1000) tau, with no outside tool.
    let golden = Golden::load("decay");
    for entry in golden.values()["slope_db_per_s"].as_array().unwrap() {
        let rt60 = float(&entry["rt60_s"]);
        assert!((float(&entry["slope"]) * rt60 + 60.0).abs() < 1e-12);
    }
    for entry in golden.values()["release_rt60_s"].as_array().unwrap() {
        assert!((float(&entry["rt60"]) - LN_1000 * float(&entry["tau_s"])).abs() < 1e-12);
    }
}

#[test]
fn a_noise_free_decay_reads_within_three_percent() {
    for rt60 in [0.4, 1.2, 2.5] {
        let (t20, t30) = fits(rt60, None);
        for time in [t20.unwrap(), t30.unwrap()] {
            assert!((time / rt60 - 1.0).abs() < 0.03, "RT60 {rt60}: {time}");
        }
    }
}

#[test]
fn a_floor_at_minus_60_db_reads_within_three_percent() {
    for rt60 in [0.4, 1.2, 2.5] {
        let (t20, t30) = fits(rt60, Some(-60.0));
        for time in [t20.unwrap(), t30.unwrap()] {
            assert!((time / rt60 - 1.0).abs() < 0.03, "RT60 {rt60}: {time}");
        }
    }
}

#[test]
fn a_floor_at_minus_40_db_keeps_t20_and_fails_t30() {
    for rt60 in [0.4, 1.2, 2.5] {
        let (t20, t30) = fits(rt60, Some(-40.0));
        let t20 = t20.unwrap();
        assert!(
            t20 / rt60 - 1.0 < 0.25 && t20 / rt60 > 0.9,
            "RT60 {rt60}: {t20}"
        );
        assert!(
            matches!(t30, Err(DecayFailure::FloorTooHigh { .. })),
            "{t30:?}"
        );
    }
}

#[test]
fn a_floor_at_minus_30_db_fails_both() {
    for rt60 in [0.4, 1.2, 2.5] {
        let (t20, t30) = fits(rt60, Some(-30.0));
        assert!(t20.is_err() && t30.is_err(), "RT60 {rt60}: {t20:?} {t30:?}");
    }
}

/// Linear convolution by FFT, cut to the length of `x`.
fn convolve(x: &[f64], h: &[f64]) -> Vec<f64> {
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

/// The 53-release-reverb recipe: a noise note gated for 0.5 s, an
/// exponential release, a fully wet noise reverb of RT60 `reverb`. Returns
/// the tail from note-off.
fn note_with_reverb(tau: f64, reverb: f64, seed: u64) -> Vec<f64> {
    let tail = (6.0 * tau).max(3.0 * reverb.max(LN_1000 * tau));
    let length = samples(0.5 + tail);
    let note: Vec<f64> = times(RATE, length)
        .into_iter()
        .zip(gaussian(seed, length))
        .map(|(t, n)| {
            n * if t < 0.5 {
                1.0
            } else {
                (-(t - 0.5) / tau).exp()
            }
        })
        .collect();
    let wet = if reverb > 0.0 {
        let mut ir = decay_noise(reverb, samples(1.5 * reverb), seed + 70_000);
        let norm = ir.iter().map(|v| v * v).sum::<f64>().sqrt();
        for v in &mut ir {
            *v /= norm;
        }
        convolve(&note, &ir)
    } else {
        note
    };
    wet[samples(0.5)..].to_vec()
}

#[test]
fn the_tail_follows_the_slower_of_release_and_reverb() {
    let golden = Golden::load("prototype/53-release-reverb");
    let rows = golden.values()["rows"].as_array().unwrap();
    let t20 = |a: u64, b: u64, tau: f64, reverb: f64, trials: u64| {
        median(
            (0..trials)
                .map(|trial| {
                    let tail = note_with_reverb(tau, reverb, 5310 + 1000 * a + 100 * b + trial);
                    Decay::of(&tail, rate()).t20().unwrap().get()
                })
                .collect(),
        )
    };
    // A 50 ms release under a 1 s reverb reads the reverb.
    let short = t20(0, 2, 0.05, 1.0, 15);
    assert!((short - 1.0).abs() < 0.05, "{short}");
    // A 1 s release reads its own 6.9 s whatever the reverb. Its 21 s tails
    // spread by under 0.1 s, so 5 trials keep the debug run short.
    for (b, reverb) in [0.0, 0.5, 1.0, 2.0].into_iter().enumerate() {
        let long = t20(2, u64::try_from(b).unwrap(), 1.0, reverb, 5);
        assert!(
            (long / (LN_1000 * 1.0) - 1.0).abs() < 0.05,
            "reverb {reverb}: {long}"
        );
        // The prototype read the same tails with the plain integral.
        let stored = float(&rows[2]["cells"][b]["median_s"]);
        assert!(
            (long / stored - 1.0).abs() < 0.05,
            "reverb {reverb}: {long} vs {stored}"
        );
    }
}

#[test]
fn leading_silence_shifts_the_truncation_and_keeps_the_decay_time() {
    let tail = with_floor(0.4, Some(-70.0), 9400);
    let lead = samples(0.2);
    let late: Vec<f64> = std::iter::repeat_n(0.0, lead)
        .chain(tail.iter().copied())
        .collect();
    let (direct, delayed) = (Decay::of(&tail, rate()), Decay::of(&late, rate()));
    assert_eq!(delayed.t20(), direct.t20());
    assert_eq!(delayed.t30(), direct.t30());
    let shift = delayed.truncation().get() - direct.truncation().get();
    assert!((shift - 0.2).abs() < 1e-9, "{shift}");
}
