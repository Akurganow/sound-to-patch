//! The modulation spectrum and the snap (#51).

use std::f64::consts::PI;

use sound_to_patch::audio::{Division, Feel, NoteValue, SampleRate};
use sound_to_patch::dsp::{ModulationSpectrum, band_envelope, scan_range, snap};
use sound_to_patch::units::{Bpm, Hz};

use crate::support::golden::{Golden, float};
use crate::support::signals::{naive_saw, times};

const RATE: u32 = 48_000;

fn hz(value: f64) -> Hz {
    Hz::new(value).unwrap()
}

/// A 1 kHz carrier with a sine LFO on its amplitude, half depth.
fn tremolo(rate_hz: f64, seconds: f64) -> Vec<f64> {
    let t = times(
        f64::from(RATE),
        crate::support::cast::round_to_usize(seconds * f64::from(RATE)),
    );
    t.iter()
        .map(|&t| (1.0 + 0.5 * (2.0 * PI * rate_hz * t).sin()) * (2.0 * PI * 1000.0 * t).sin())
        .collect()
}

fn lfo_rate(x: &[f64], bpm: f64) -> Option<f64> {
    let rate = SampleRate::new(RATE).unwrap();
    let track = band_envelope(x, rate, hz(900.0), hz(1100.0)).unwrap();
    let (lo, hi) = scan_range(Bpm::new(bpm).unwrap());
    ModulationSpectrum::of(&track)
        .rate(lo, hi)
        .unwrap()
        .value()
        .map(|r| r.get())
}

#[test]
fn a_4_hz_tremolo_reads_4_hz() {
    let measured = lfo_rate(&tremolo(4.0, 4.0), 120.0).unwrap();
    assert!((measured - 4.0).abs() <= 1.0 / 4.0, "{measured}");
}

#[test]
fn two_detuned_saws_beat_at_the_prototype_rates() {
    let golden = Golden::load("prototype/51-two-saw-peaks");
    let t = times(f64::from(RATE), 8 * 48_000);
    let f2 = 55.0 * 2f64.powf(10.0 / 1200.0);
    let x: Vec<f64> = naive_saw(55.0, &t)
        .iter()
        .zip(naive_saw(f2, &t))
        .map(|(a, b)| a + b)
        .collect();
    let rate = SampleRate::new(RATE).unwrap();
    for (h, expected) in golden.values().as_object().unwrap() {
        let h: f64 = h.parse().unwrap();
        let track = band_envelope(&x, rate, hz(h * 55.0 - 15.0), hz(h * 55.0 + 15.0)).unwrap();
        let peak = ModulationSpectrum::of(&track)
            .peak(hz(0.2), hz(20.0))
            .unwrap()
            .get();
        // The rate rule of #51: within 1 / T, 0.125 Hz for 8 s.
        assert!(
            (peak - float(&expected["peak_hz"])).abs() <= 0.125,
            "h {h}: {peak}"
        );
        assert!(
            (peak - float(&expected["predicted_hz"])).abs() <= 0.125,
            "h {h}: {peak}"
        );
    }
}

#[test]
fn lfos_locked_to_six_divisions_read_their_rate_and_snap_to_it() {
    let bpm = Bpm::new(140.0).unwrap();
    let d = |value, feel| Division { value, feel };
    for division in [
        d(NoteValue::Whole, Feel::Straight),
        d(NoteValue::Half, Feel::Straight),
        d(NoteValue::Quarter, Feel::Straight),
        d(NoteValue::Quarter, Feel::Triplet),
        d(NoteValue::Eighth, Feel::Straight),
        d(NoteValue::Sixteenth, Feel::Straight),
    ] {
        let rate = division.rate(bpm).get();
        let measured = lfo_rate(&tremolo(rate, 8.0), 140.0).unwrap();
        assert!(
            (measured - rate).abs() <= 1.0 / 8.0,
            "{division}: {measured} vs {rate}"
        );
        assert_eq!(
            snap(hz(measured), bpm).division,
            division,
            "{division}: {measured}"
        );
    }
}

#[test]
fn a_4_bar_lfo_needs_two_cycles_in_the_stem() {
    let rate = Division::ALL[0].rate(Bpm::new(140.0).unwrap()).get();
    let long = lfo_rate(&tremolo(rate, 14.0), 140.0).unwrap();
    assert!((long - rate).abs() <= 1.0 / 14.0, "{long} vs {rate}");
    assert_eq!(lfo_rate(&tremolo(rate, 10.0), 140.0), None);
}
