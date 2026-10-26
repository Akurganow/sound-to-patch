//! The stem (#46): band edge, flat-top flag and the frame rule.

use sound_to_patch::audio::{Audio, BAND_EDGE_STEEP_DROP_DB, BeatGrid, SampleRate, Stem};
use sound_to_patch::confidence::{Level, ReasonCode, Signal};
use sound_to_patch::notes::{Note, NoteList};
use sound_to_patch::units::{Bpm, MidiPitch, Seconds};

use crate::support::rng::gaussian;

const RATE: u32 = 44_100;

fn grid() -> BeatGrid {
    BeatGrid::new(Bpm::new(120.0).unwrap(), Seconds::new(0.0).unwrap())
}

fn stem(channels: Vec<Vec<f64>>, notes: Option<&NoteList>) -> Stem {
    let audio = Audio::new(SampleRate::new(RATE).unwrap(), channels).unwrap();
    Stem::new(audio, grid(), notes).unwrap()
}

/// White noise shaped by the amplitude gain `gain(f)` in the frequency
/// domain.
fn shaped_noise(seed: u64, gain: impl Fn(f64) -> f64) -> Vec<f64> {
    use realfft::RealFftPlanner;
    let len = 1 << 17;
    let mut planner = RealFftPlanner::<f64>::new();
    let forward = planner.plan_fft_forward(len);
    let inverse = planner.plan_fft_inverse(len);
    let mut input = gaussian(seed, len);
    let mut spectrum = forward.make_output_vec();
    forward.process(&mut input, &mut spectrum).unwrap();
    let bin = f64::from(RATE) / f64::from(u32::try_from(len).unwrap());
    for (k, value) in spectrum.iter_mut().enumerate() {
        *value *= gain(f64::from(u32::try_from(k).unwrap()) * bin);
    }
    let mut out = vec![0.0; len];
    inverse.process(&mut spectrum, &mut out).unwrap();
    let scale = f64::from(u32::try_from(len).unwrap());
    out.iter().map(|v| v / scale).collect()
}

fn db(level: f64) -> f64 {
    10f64.powf(level / 20.0)
}

/// White noise with everything above `edge` 90 dB down, as a lossy codec's
/// low-pass leaves a stem.
fn low_passed_noise(edge: f64, seed: u64) -> Vec<f64> {
    shaped_noise(seed, |f| if f <= edge { 1.0 } else { db(-90.0) })
}

fn edge_of(samples: Vec<f64>) -> (f64, Level, f64) {
    let stem = stem(vec![samples], None);
    let estimate = stem.band_edge();
    let drop = estimate
        .confidence()
        .reasons()
        .iter()
        .find(|reason| reason.code() == ReasonCode::BandEdgeDrop)
        .map(|reason| match reason.value() {
            Signal::Number(depth) => depth,
            other => panic!("{other:?}"),
        })
        .unwrap();
    (
        estimate.value().unwrap().get(),
        estimate.confidence().level().unwrap(),
        drop,
    )
}

/// The distance from `truth` in semitones.
fn semitones(measured: f64, truth: f64) -> f64 {
    12.0 * (measured / truth).log2()
}

#[test]
fn a_low_passed_noise_stem_reads_its_band_edge() {
    // The steep-drop rule puts the edge at the half-depth point of the cut,
    // within 0.1 semitone of it; the smoothing is one semitone wide.
    for edge in [8_000.0, 16_000.0] {
        let (measured, level, drop) = edge_of(low_passed_noise(edge, 4600));
        assert!(semitones(measured, edge).abs() < 0.1, "{edge}: {measured}");
        assert_eq!(level, Level::High);
        assert!(drop > 60.0, "{drop}");
    }
}

#[test]
fn a_noise_shelf_above_the_cut_does_not_move_the_edge() {
    // Real stems carry noise some 10 to 20 dB above the floor beyond their
    // cut. The 30 dB margin alone reads this shelf 3 semitones high.
    let shelf = shaped_noise(4630, |f| match f {
        f if f <= 16_000.0 => 1.0,
        f if f <= 20_000.0 => db(-60.0),
        _ => db(-90.0),
    });
    let (measured, _, _) = edge_of(shelf);
    assert!(semitones(measured, 16_000.0).abs() < 0.1, "{measured}");
}

#[test]
fn a_gentle_roll_off_falls_back_to_the_margin_rule() {
    // A two-pole roll-off at 3 kHz falls about 1 dB per semitone: no cut.
    let dark = shaped_noise(4640, |f| 1.0 / (1.0 + (f / 3000.0).powi(2)));
    let (_, level, drop) = edge_of(dark);
    assert_eq!(level, Level::Medium);
    assert!(drop < BAND_EDGE_STEEP_DROP_DB.get(), "{drop}");
}

#[test]
fn a_full_band_stem_has_its_edge_near_nyquist() {
    let noise = gaussian(4610, 1 << 17);
    let measured = *stem(vec![noise], None).band_edge().value().unwrap();
    assert!(measured.get() > 21_000.0, "{measured:?}");
}

#[test]
fn a_hard_clipped_sine_sets_the_flat_top_flag() {
    let sine: Vec<f64> = (0..44_100)
        .map(|n| (std::f64::consts::TAU * 110.0 * f64::from(n) / 44_100.0).sin())
        .collect();
    let clipped: Vec<f64> = sine.iter().map(|v| v.clamp(-0.5, 0.5)).collect();
    assert!(stem(vec![clipped.clone(), clipped], None).hard_limited());
    assert!(!stem(vec![sine.clone(), sine], None).hard_limited());
}

#[test]
fn a_low_note_list_picks_the_long_frame() {
    let note = |pitch| {
        Note::new(
            Seconds::new(0.0).unwrap(),
            Seconds::new(1.0).unwrap(),
            MidiPitch::new(pitch).unwrap(),
        )
        .unwrap()
    };
    let x = gaussian(4620, 44_100);
    // E1 is 41.2 Hz, A3 220 Hz.
    let low = NoteList::new(vec![note(28.0)]);
    let high = NoteList::new(vec![note(57.0)]);
    assert_eq!(stem(vec![x.clone()], Some(&low)).frame_len(), 8192);
    assert_eq!(stem(vec![x.clone()], Some(&high)).frame_len(), 4096);
    let f0 = stem(vec![x], Some(&low)).median_f0().unwrap().get();
    assert!((f0 - 41.203_444_614_108_74).abs() < 1e-9);
}
