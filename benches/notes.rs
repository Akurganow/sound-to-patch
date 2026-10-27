//! The notes budget (#64): each step of `notes` on one generated 60 s
//! stem, so a regression names its step. Run with `cargo bench --bench
//! notes`; the budget in the testing guide comes from these medians.

use std::error::Error;
use std::f64::consts::TAU;
use std::hint::black_box;
use std::num::NonZeroUsize;
use std::time::Duration;

use criterion::Criterion;
use sound_to_patch::audio::SampleRate;
use sound_to_patch::notes::{
    DEFAULT_VELOCITY, PitchTrack, Transcriber, Velocities, transcribe_stem, write_midi,
};
use sound_to_patch::units::Bpm;

/// The model rate, so neither the model nor the track resamples.
const RATE: u32 = 22_050;
/// The stem length in seconds.
const SECONDS: u32 = 60;

/// A 60 s line at 120 BPM: an eighth note every quarter, saws and sines in
/// turn over two octaves from A2.
fn stem() -> Vec<f64> {
    let rate = f64::from(RATE);
    let pitches = [45.0, 48.0, 52.0, 55.0, 57.0, 60.0, 64.0, 67.0];
    // A beat at 120 BPM lasts half a second.
    let beat = RATE / 2;
    (0..RATE * SECONDS)
        .map(|n| {
            if n % beat >= beat / 2 {
                return 0.0;
            }
            let t = f64::from(n) / rate;
            let index = usize::try_from(n / beat).unwrap_or(0) % pitches.len();
            let f0 = 440.0 * 2_f64.powf((pitches[index] - 69.0) / 12.0);
            if index.is_multiple_of(2) {
                (1_u32..100)
                    .map(f64::from)
                    .take_while(|h| h * f0 < rate / 2.0)
                    .map(|h| 0.3 * (TAU * h * f0 * t).sin() / h)
                    .sum()
            } else {
                0.3 * (TAU * f0 * t).sin()
            }
        })
        .collect()
}

fn benches(criterion: &mut Criterion) -> Result<(), Box<dyn Error>> {
    let mono = stem();
    let rate = SampleRate::new(RATE)?;
    let bpm = Bpm::new(120.0)?;
    let all = std::thread::available_parallelism()?;
    let threads = [("1 thread", NonZeroUsize::MIN), ("all threads", all)];

    criterion.bench_function("model load", |b| b.iter(Transcriber::new));

    let mut group = criterion.benchmark_group("model run");
    group.sample_size(10);
    for (name, count) in threads {
        let transcriber = Transcriber::new()?.with_threads(count);
        group.bench_function(name, |b| {
            b.iter(|| transcriber.run(black_box(&mono), rate));
        });
    }
    group.finish();

    let mut group = criterion.benchmark_group("f0 track");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(400));
    group.bench_function("whole stem", |b| {
        b.iter(|| PitchTrack::of(black_box(&mono), rate));
    });
    group.finish();

    let found = transcribe_stem(&Transcriber::new()?, &mono, rate, bpm, None)?;
    criterion.bench_function("midi write", |b| {
        b.iter(|| {
            write_midi(
                black_box(&found.notes),
                bpm,
                Velocities::Constant(DEFAULT_VELOCITY),
            )
        });
    });

    let mut group = criterion.benchmark_group("end to end");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(400));
    for (name, count) in threads {
        let transcriber = Transcriber::new()?.with_threads(count);
        group.bench_function(name, |b| {
            b.iter(|| transcribe_stem(&transcriber, black_box(&mono), rate, bpm, None));
        });
    }
    group.finish();
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut criterion = Criterion::default().configure_from_args();
    benches(&mut criterion)?;
    criterion.final_summary();
    Ok(())
}
