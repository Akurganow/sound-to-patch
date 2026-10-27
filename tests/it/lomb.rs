//! The Lomb-Scargle periodogram (#52).

use proptest::prelude::*;
use sound_to_patch::dsp::{grid, lomb_scargle, periodogram_rate, scan_range};
use sound_to_patch::units::{Bpm, Hz, Seconds};

use crate::support::golden::{Golden, assert_close, floats};

/// The gated track of the lombscargle recipe: `1.5 + sin(2 pi 4 t + 0.4)` at
/// 200 Hz for 8 s, kept while `t mod 1 < 0.5`.
fn gated() -> (Vec<Seconds>, Vec<f64>) {
    (0..1600)
        .map(|i| f64::from(i) / 200.0)
        .filter(|t| t.rem_euclid(1.0) < 0.5)
        .map(|t| {
            (
                Seconds::new(t).unwrap(),
                1.5 + (2.0 * std::f64::consts::PI * 4.0 * t + 0.4).sin(),
            )
        })
        .unzip()
}

#[test]
fn the_periodogram_matches_the_reference() {
    // Reference: scipy.signal.lombscargle on centred values, see lombscargle.json.
    let golden = Golden::load("lombscargle");
    let (times, values) = gated();
    let frequencies: Vec<Hz> = floats(&golden.values()["frequencies_hz"])
        .into_iter()
        .map(|f| Hz::new(f).unwrap())
        .collect();
    let power = lomb_scargle(&times, &values, &frequencies);
    assert_close(
        &power,
        &floats(&golden.values()["power"]),
        golden.tolerance(),
    );
}

#[test]
fn gated_samples_read_4_hz_where_a_plain_fft_reads_the_gate() {
    let (times, values) = gated();
    let frequencies = grid(Hz::new(0.2).unwrap(), Hz::new(20.0).unwrap());
    let power = lomb_scargle(&times, &values, &frequencies);
    let (peak, _) = frequencies
        .iter()
        .zip(&power)
        .max_by(|a, b| a.1.total_cmp(b.1))
        .unwrap();
    assert!((peak.get() - 4.0).abs() <= 0.125, "{peak:?}");
    let rate = periodogram_rate(
        &times,
        &values,
        Hz::new(0.2).unwrap(),
        Hz::new(20.0).unwrap(),
    )
    .unwrap();
    let measured = rate.value().unwrap().get();
    let span = times.last().unwrap().get() - times[0].get();
    assert!((measured - 4.0).abs() <= 1.0 / span, "{measured}");

    // The same track zero-filled, mean kept, through a plain FFT.
    let mut track = vec![0.0; 1600];
    for (t, v) in times.iter().zip(&values) {
        track[crate::support::cast::round_to_usize(t.get() * 200.0)] = *v;
    }
    let mut planner = realfft::RealFftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(1600);
    let mut spectrum = fft.make_output_vec();
    fft.process(&mut track, &mut spectrum).unwrap();
    // Bins are 0.125 Hz apart; search 0.2 Hz to 20 Hz.
    let (bin, _) = spectrum
        .iter()
        .enumerate()
        .skip(2)
        .take(159)
        .max_by(|a, b| a.1.norm().total_cmp(&b.1.norm()))
        .unwrap();
    let line = f64::from(u32::try_from(bin).unwrap()) * 0.125;
    assert!((line - 1.0).abs() <= 0.125, "{line}");
}

#[test]
fn too_short_a_span_gives_no_rate() {
    let (times, values): (Vec<_>, Vec<_>) = gated().0.into_iter().zip(gated().1).take(100).unzip();
    // 0.5 s of samples cannot hold two cycles of 1 Hz.
    let rate = periodogram_rate(
        &times,
        &values,
        Hz::new(0.2).unwrap(),
        Hz::new(1.0).unwrap(),
    )
    .unwrap();
    assert!(rate.value().is_none());
}

proptest! {
    // Each case runs three periodograms of 500 frequencies; 32 cases keep
    // the debug run short.
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn a_constant_changes_nothing_and_a_gain_scales_the_power_by_its_square(offset in -10.0..10.0f64, gain in 0.1..10.0f64) {
        let (times, values) = gated();
        let frequencies = grid(Hz::new(1.0).unwrap(), Hz::new(6.0).unwrap());
        let base = lomb_scargle(&times, &values, &frequencies);
        let shifted: Vec<f64> = values.iter().map(|v| v + offset).collect();
        let scaled: Vec<f64> = values.iter().map(|v| v * gain).collect();
        for ((b, s), g) in base.iter().zip(lomb_scargle(&times, &shifted, &frequencies)).zip(lomb_scargle(&times, &scaled, &frequencies)) {
            prop_assert!((s - b).abs() <= 1e-9 * b.abs().max(1.0));
            prop_assert!((g - gain * gain * b).abs() <= 1e-9 * (gain * gain * b).abs().max(1.0));
        }
    }
}

/// A level track `1 + 0.3 sin(2 pi r t)` at 200 Hz for 8 s, kept while a note
/// of period `p` and duty `d` sounds.
fn synced(p: f64, d: f64, r: f64) -> Option<f64> {
    let (times, values): (Vec<Seconds>, Vec<f64>) = (0..1600)
        .map(|i| f64::from(i) / 200.0)
        .filter(|t| t.rem_euclid(p) < d * p)
        .map(|t| {
            (
                Seconds::new(t).unwrap(),
                1.0 + 0.3 * (std::f64::consts::TAU * r * t).sin(),
            )
        })
        .unzip();
    let (lo, hi) = scan_range(Bpm::new(120.0).unwrap());
    periodogram_rate(&times, &values, lo, hi)
        .unwrap()
        .value()
        .map(|rate| rate.get())
}

#[test]
fn an_lfo_synced_to_the_notes_reads_its_own_rate() {
    // The note rhythm puts alias lines at r +- k / p; none may win. The grid
    // covers rates of 1 to 8 times the note rate and duties of 0.5 to 0.8.
    for p in [0.25, 0.5, 1.0, 2.0] {
        for d in [0.5, 0.6, 0.7, 0.8] {
            for k in [1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0] {
                let r = k / p;
                if r > 20.0 || r * 8.0 < 2.0 {
                    continue;
                }
                let measured = synced(p, d, r);
                assert!(
                    measured.is_some_and(|m| (m - r).abs() <= 1.0 / 8.0),
                    "p {p}, d {d}, r {r}: {measured:?}"
                );
            }
        }
    }
}
