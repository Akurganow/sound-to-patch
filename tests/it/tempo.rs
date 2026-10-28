//! Tempo refinement on generated drum, bass, lead and pad parts.

use sound_to_patch::audio::{SampleRate, Tempo};
use sound_to_patch::confidence::Level;
use sound_to_patch::units::Bpm;

use crate::support::cast::round_to_usize;
use crate::support::rng::uniform;
use crate::support::signals::polyblep_saw;

const RATE: u32 = 22_050;
/// Beats in each part: the 32 lags plus the overlap, plus one.
const BEATS: usize = 37;
const TEMPOS: [f64; 5] = [70.3, 97.55, 123.4, 147.81, 178.2];
const OFFSETS: [f64; 4] = [-0.03, -0.01, 0.01, 0.03];

#[derive(Clone, Copy, Debug)]
enum Part {
    Drums,
    Bass,
    Lead,
    Pad,
    /// One kick on the first beat of each bar.
    BarKick,
    /// One kick on every other beat.
    HalfKick,
    /// Kicks on the beat and hats on every sixteenth.
    Hats16,
}

/// Where the beats of a part fall.
#[derive(Clone, Copy, Debug)]
struct Clock {
    /// The tempo at the first beat.
    bpm: f64,
    /// The tempo at the last beat; the tempo moves linearly in beats.
    end_bpm: f64,
    /// Where the off-beat eighth falls, as a share of the beat.
    swing: f64,
    /// The largest random shift of each onset, in seconds.
    jitter: f64,
    /// Beats in the part.
    beats: usize,
}

impl Clock {
    fn steady(bpm: f64) -> Self {
        Self {
            bpm,
            end_bpm: bpm,
            swing: 0.5,
            jitter: 0.0,
            beats: BEATS,
        }
    }

    /// The time of beat position `beat`, in seconds, before the jitter.
    fn at(&self, beat: f64) -> f64 {
        let whole = beat.floor();
        let share = beat - whole;
        let beat = whole
            + if (share - 0.5).abs() < 1e-9 {
                self.swing
            } else {
                share
            };
        if (self.end_bpm - self.bpm).abs() < 1e-12 {
            beat * 60.0 / self.bpm
        } else {
            let slope = (self.end_bpm - self.bpm) / f(self.beats);
            60.0 / slope * ((self.bpm + slope * beat) / self.bpm).ln()
        }
    }

    /// The time of onset `n` at beat position `beat`, with its jitter.
    fn onset(&self, beat: f64, n: usize) -> f64 {
        let shift = uniform(1000 + u64::try_from(n).unwrap(), 1)[0];
        (self.at(beat) + self.jitter * (2.0 * shift - 1.0)).max(0.0)
    }

    fn samples(&self) -> usize {
        round_to_usize((self.at(f(self.beats)) * f64::from(RATE)).ceil())
    }
}

/// One note of a part: onset and length in beats, frequency, attack and
/// release in seconds.
struct Note {
    beat: f64,
    beats: f64,
    hz: f64,
    attack: f64,
    release: f64,
}

fn f(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap())
}

fn midi(key: f64) -> f64 {
    440.0 * 2_f64.powf((key - 69.0) / 12.0)
}

fn render_notes(notes: &[Note], clock: Clock, gain: f64) -> Vec<f64> {
    let rate = f64::from(RATE);
    let mut out = vec![0.0; clock.samples()];
    for (n, note) in notes.iter().enumerate() {
        let onset = clock.onset(note.beat, n);
        let start = round_to_usize(onset * rate);
        let hold = clock.at(note.beat + note.beats) - clock.at(note.beat);
        let total = round_to_usize(((hold + note.release) * rate).ceil());
        let saw = polyblep_saw(note.hz, rate, total);
        for (i, s) in saw.iter().enumerate() {
            let t = f(i) / rate;
            let env = if t < note.attack {
                t / note.attack
            } else if t < hold {
                1.0
            } else {
                (1.0 - (t - hold) / note.release).max(0.0)
            };
            if let Some(slot) = out.get_mut(start + i) {
                *slot += gain * env * s;
            }
        }
    }
    out
}

/// Kicks at `kicks` and hats at `hats`, in beats.
fn render_drums(kicks: &[f64], hats: &[f64], clock: Clock) -> Vec<f64> {
    let rate = f64::from(RATE);
    let mut out = vec![0.0; clock.samples()];
    let noise = uniform(7, out.len());
    let onsets = kicks
        .iter()
        .map(|&b| (b, true))
        .chain(hats.iter().map(|&b| (b, false)));
    for (n, (beat, kick)) in onsets.enumerate() {
        let start = round_to_usize(clock.onset(beat, n) * rate);
        for i in 0..round_to_usize(0.3 * rate) {
            let Some(slot) = out.get_mut(start + i) else {
                break;
            };
            let t = f(i) / rate;
            *slot += if kick {
                // A falling sine.
                let phase =
                    2.0 * std::f64::consts::PI * (50.0 * t + 2.0 * (1.0 - (-t / 0.02).exp()));
                0.8 * phase.sin() * (-t / 0.12).exp()
            } else {
                // A noise burst.
                0.2 * (2.0 * noise[start + i] - 1.0) * (-t / 0.02).exp()
            };
        }
    }
    out
}

fn part(kind: Part, clock: Clock) -> Vec<f64> {
    let beats = clock.beats;
    let every = |step: usize, offset: f64| -> Vec<f64> {
        (0..beats).step_by(step).map(|b| f(b) + offset).collect()
    };
    match kind {
        Part::Drums => render_drums(&every(1, 0.0), &every(1, 0.5), clock),
        Part::BarKick => render_drums(&every(4, 0.0), &[], clock),
        Part::HalfKick => render_drums(&every(2, 0.0), &[], clock),
        Part::Hats16 => {
            let hats: Vec<f64> = (0..beats * 4).map(|n| f(n) / 4.0).collect();
            render_drums(&every(1, 0.0), &hats, clock)
        }
        Part::Bass => {
            let keys = [33.0, 33.0, 36.0, 33.0, 40.0, 33.0, 31.0, 33.0];
            let notes: Vec<Note> = (0..beats * 2)
                .map(|n| Note {
                    beat: f(n) / 2.0,
                    beats: 0.4,
                    hz: midi(keys[n % keys.len()]),
                    attack: 0.003,
                    release: 0.01,
                })
                .collect();
            render_notes(&notes, clock, 0.3)
        }
        Part::Lead => {
            // Quarter, quarter, eighth, eighth, half: four beats per bar.
            let bar = [(0.0, 1.0), (1.0, 1.0), (2.0, 0.5), (2.5, 0.5), (3.0, 1.0)];
            let keys = [69.0, 72.0, 76.0, 74.0, 72.0, 71.0, 69.0, 67.0];
            let mut notes = Vec::new();
            for b in 0..=beats / 4 {
                for (i, (at, length)) in bar.iter().enumerate() {
                    notes.push(Note {
                        beat: f(b) * 4.0 + at,
                        beats: length * 0.9,
                        hz: midi(keys[(b * bar.len() + i) % keys.len()]),
                        attack: 0.01,
                        release: 0.05,
                    });
                }
            }
            render_notes(&notes, clock, 0.25)
        }
        Part::Pad => {
            let chords = [
                [57.0, 60.0, 64.0],
                [53.0, 57.0, 60.0],
                [55.0, 59.0, 62.0],
                [52.0, 55.0, 59.0],
            ];
            let mut notes = Vec::new();
            for b in 0..=beats / 4 {
                for &key in &chords[b % chords.len()] {
                    notes.push(Note {
                        beat: f(b) * 4.0,
                        beats: 4.0,
                        hz: midi(key),
                        attack: 0.2,
                        release: 0.3,
                    });
                }
            }
            render_notes(&notes, clock, 0.15)
        }
    }
}

fn refine(samples: &[f64], given: f64) -> Tempo {
    Tempo::refine(
        samples,
        SampleRate::new(RATE).unwrap(),
        Bpm::new(given).unwrap(),
    )
    .unwrap()
}

fn level(tempo: &Tempo) -> Option<Level> {
    tempo.refined().confidence().level()
}

/// Asserts that `tempo` refined `truth` to within `tolerance` BPM at high
/// confidence.
fn assert_refined(tempo: &Tempo, truth: f64, tolerance: f64, case: &str) {
    assert!(
        (tempo.used().get() - truth).abs() <= tolerance && level(tempo) == Some(Level::High),
        "{case}: {}",
        tempo.summary()
    );
}

/// Asserts that `tempo` kept the given tempo and trusts no refined one.
fn assert_kept(tempo: &Tempo, case: &str) {
    assert!(
        tempo.used() == tempo.given() && level(tempo) != Some(Level::High),
        "{case}: {}",
        tempo.summary()
    );
}

fn check(kind: Part, truth: f64) {
    let samples = part(kind, Clock::steady(truth));
    for offset in OFFSETS {
        let tempo = refine(&samples, truth * (1.0 + offset));
        let case = format!("{kind:?} at {truth} BPM, --bpm offset {offset}");
        match kind {
            Part::Pad => assert_kept(&tempo, &case),
            _ => assert_refined(&tempo, truth, 0.05, &case),
        }
    }
}

/// One test per part and tempo, so the grid runs in parallel.
macro_rules! grid {
    ($($name:ident: $kind:ident, $tempo:literal;)+) => {
        $(
            #[test]
            fn $name() {
                check(Part::$kind, TEMPOS[$tempo]);
            }
        )+
    };
}

grid! {
    drums_70: Drums, 0; drums_98: Drums, 1; drums_123: Drums, 2; drums_148: Drums, 3; drums_178: Drums, 4;
    bass_70: Bass, 0; bass_98: Bass, 1; bass_123: Bass, 2; bass_148: Bass, 3; bass_178: Bass, 4;
    lead_70: Lead, 0; lead_98: Lead, 1; lead_123: Lead, 2; lead_148: Lead, 3; lead_178: Lead, 4;
    pad_70: Pad, 0; pad_98: Pad, 1; pad_123: Pad, 2; pad_148: Pad, 3; pad_178: Pad, 4;
    bar_kick_70: BarKick, 0; bar_kick_98: BarKick, 1; bar_kick_123: BarKick, 2;
    bar_kick_148: BarKick, 3; bar_kick_178: BarKick, 4;
}

/// A steady tone keeps `--bpm`. Most read no onset at all and report no
/// refined tempo.
#[test]
fn a_steady_tone_keeps_the_given_tempo() {
    // A steady tone of ten harmonics whose period divides the beat, so any
    // ripple at the period would repeat on every beat.
    let rate = f64::from(RATE);
    for (hz, bpm) in [
        (110.0, 120.0),
        (55.0, 132.0),
        (41.25, 165.0),
        (220.0, 120.0),
    ] {
        let len = round_to_usize(40.0 * 60.0 / bpm * rate);
        let samples: Vec<f64> = (0..len)
            .map(|n| {
                let t = f(n) / rate;
                let onset = (t / 0.005).min(1.0);
                let tone: f64 = (1..=10)
                    .map(|h| {
                        let h = f64::from(h);
                        (2.0 * std::f64::consts::PI * h * hz * t).sin() / h
                    })
                    .sum();
                0.2 * onset * tone
            })
            .collect();
        let tempo = refine(&samples, bpm * 1.01);
        assert_kept(&tempo, &format!("{hz} Hz at {bpm} BPM"));
    }
}

/// Onsets on every other beat of `--bpm` refine the beat, not half of it:
/// the search never reaches half or double the given tempo.
#[test]
fn onsets_on_every_other_beat_keep_the_tempo_near_the_given_one() {
    let truth = TEMPOS[2];
    let given = truth * 1.01;
    let tempo = refine(&part(Part::HalfKick, Clock::steady(truth)), given);
    assert_refined(&tempo, truth, 0.05, "half-time kick");
    let share = tempo.used().get() / given - 1.0;
    assert!((-0.0292..=0.031).contains(&share), "{share}");
}

/// The off-beat eighth at two thirds of the beat leaves the beat in place.
#[test]
fn swing_keeps_the_refined_tempo() {
    let truth = TEMPOS[2];
    for kind in [Part::Drums, Part::Bass, Part::Lead, Part::BarKick] {
        let swing = Clock {
            swing: 0.667,
            ..Clock::steady(truth)
        };
        let tempo = refine(&part(kind, swing), truth * 1.01);
        assert_refined(&tempo, truth, 0.05, &format!("{kind:?} with swing"));
    }
}

/// Every onset moved by up to 8 ms at random still refines the tempo.
#[test]
fn humanised_onsets_keep_the_refined_tempo() {
    let truth = TEMPOS[2];
    for kind in [Part::Drums, Part::Bass, Part::Lead, Part::BarKick] {
        let human = Clock {
            jitter: 0.008,
            ..Clock::steady(truth)
        };
        let tempo = refine(&part(kind, human), truth * 1.01);
        assert_refined(&tempo, truth, 0.1, &format!("{kind:?} humanised"));
    }
}

/// A `--bpm` 3.5 % or 4 % off puts the true tempo outside the search, and
/// the edge of the search is not trusted. At 70.3 BPM, drum and bass parts
/// line up every 8 beats at 16/15 of the truth, and sixteenth hats every 4,
/// so these cases pin the sparse thresholds.
fn check_outside(kind: Part, truth: f64) {
    let samples = part(kind, Clock::steady(truth));
    for offset in [-0.04, -0.035, 0.035, 0.04] {
        let tempo = refine(&samples, truth * (1.0 + offset));
        assert_kept(
            &tempo,
            &format!("{kind:?} at {truth} BPM, --bpm offset {offset}"),
        );
    }
}

/// One test per part and tempo, so the cases run in parallel.
macro_rules! outside {
    ($($name:ident: $kind:ident, $tempo:literal;)+) => {
        $(
            #[test]
            fn $name() {
                check_outside(Part::$kind, TEMPOS[$tempo]);
            }
        )+
    };
}

outside! {
    outside_drums_70: Drums, 0; outside_drums_98: Drums, 1; outside_drums_123: Drums, 2;
    outside_drums_148: Drums, 3; outside_drums_178: Drums, 4;
    outside_bass_70: Bass, 0; outside_bass_98: Bass, 1; outside_bass_123: Bass, 2;
    outside_bass_148: Bass, 3; outside_bass_178: Bass, 4;
    outside_hats_70: Hats16, 0; outside_hats_98: Hats16, 1; outside_hats_123: Hats16, 2;
    outside_hats_148: Hats16, 3; outside_hats_178: Hats16, 4;
}

/// The lead and the kick a bar keep `--bpm` 3.5 % off too.
#[test]
fn a_given_tempo_outside_the_search_is_kept() {
    let truth = TEMPOS[2];
    let samples: Vec<(Part, Vec<f64>)> = [Part::Lead, Part::BarKick]
        .into_iter()
        .map(|kind| (kind, part(kind, Clock::steady(truth))))
        .collect();
    for (kind, samples) in &samples {
        for offset in [-0.035, 0.035] {
            let tempo = refine(samples, truth * (1.0 + offset));
            assert_kept(&tempo, &format!("{kind:?}, --bpm offset {offset}"));
        }
    }
}

/// A tempo that moves by 2 or 3 % either way over the part has no single
/// tempo to refine.
#[test]
fn an_accelerando_keeps_the_given_tempo() {
    let truth = TEMPOS[2];
    for kind in [Part::Drums, Part::BarKick] {
        for spread in [0.02, 0.03] {
            let ramp = Clock {
                bpm: truth * (1.0 - spread),
                end_bpm: truth * (1.0 + spread),
                ..Clock::steady(truth)
            };
            let tempo = refine(&part(kind, ramp), truth);
            assert_kept(&tempo, &format!("{kind:?} ramp of {spread}"));
        }
    }
}

/// One kick a bar at exactly `--bpm` refines to `--bpm` and says so.
#[test]
fn a_kick_a_bar_at_the_given_tempo_reads_as_refined() {
    let long = Clock {
        beats: 64,
        ..Clock::steady(120.0)
    };
    let tempo = refine(&part(Part::BarKick, long), 120.0);
    assert_refined(&tempo, 120.0, 0.0, "kick a bar");
    assert!(
        tempo
            .summary()
            .starts_with("tempo: 120 BPM, refined from --bpm 120 (peak ratio "),
        "{}",
        tempo.summary()
    );
}

/// The notes of a bass line in eighths at `bpm`, `beats` long, at the
/// rate of these tests.
pub(crate) fn bass_line(bpm: f64, beats: usize) -> (u32, Vec<f64>) {
    let clock = Clock {
        beats,
        ..Clock::steady(bpm)
    };
    (RATE, part(Part::Bass, clock))
}

/// A chord of four sines from 220 to 440 Hz under a tremolo
/// `(0.5 (1 - cos))^power` with a period of `period_beats` beats at
/// `lfo_bpm`, `beats` long.
pub(crate) fn lfo_pad(lfo_bpm: f64, period_beats: f64, power: i32, beats: usize) -> Vec<f64> {
    let rate = f64::from(RATE);
    let len = round_to_usize(f(beats) * 60.0 / lfo_bpm * rate);
    let period = period_beats * 60.0 / lfo_bpm;
    (0..len)
        .map(|n| {
            let t = f(n) / rate;
            let env = (0.5 * (1.0 - (2.0 * std::f64::consts::PI * t / period).cos())).powi(power);
            let tone: f64 = [220.0, 277.18, 329.63, 440.0]
                .iter()
                .map(|hz| (2.0 * std::f64::consts::PI * hz * t).sin())
                .sum();
            0.1 * env * tone
        })
        .collect()
}

/// A sparse part whose tempo moves by 1 % either way keeps `--bpm`: its
/// sparse periodicity reads up to 0.63, under the 0.75 it needs.
#[test]
fn a_sparse_part_under_a_ramp_of_1_percent_keeps_the_given_tempo() {
    for truth in [TEMPOS[3], TEMPOS[4]] {
        for kind in [Part::BarKick, Part::HalfKick] {
            let ramp = Clock {
                bpm: truth * 0.99,
                end_bpm: truth * 1.01,
                ..Clock::steady(truth)
            };
            let tempo = refine(&part(kind, ramp), truth);
            assert_kept(&tempo, &format!("{kind:?} at {truth} BPM, ramp of 1 %"));
        }
    }
}

/// A pad under a smooth tremolo repeats like a drum part at the tremolo's
/// tempo, but its onsets are swells, so it keeps `--bpm`.
fn check_tremolo(power: i32) {
    for period in [4.0, 2.0, 1.0] {
        let lfo = 120.0 * 1.025;
        let tempo = refine(&lfo_pad(lfo, period, power, 64), 120.0);
        assert_kept(&tempo, &format!("tremolo of {period} beats, power {power}"));
    }
}

#[test]
fn a_pad_under_a_smooth_tremolo_keeps_the_given_tempo() {
    check_tremolo(1);
}

#[test]
fn a_pad_under_a_peaked_tremolo_keeps_the_given_tempo() {
    check_tremolo(4);
}

/// A limit: a loop whose bar lies within 3 % of 4 beats, such as a kick
/// every 3.9 or 4.1 beats, refines to the tempo at which it is 4 beats.
#[test]
fn a_loop_off_its_bar_refines_to_the_loop_tempo() {
    let clock = Clock {
        beats: 64,
        ..Clock::steady(120.0)
    };
    for bar in [3.9, 4.1] {
        let kicks: Vec<f64> = (0..64)
            .map(|n| f(n) * bar)
            .take_while(|&beat| beat < 64.0)
            .collect();
        let tempo = refine(&render_drums(&kicks, &[], clock), 120.0);
        assert_refined(
            &tempo,
            120.0 * 4.0 / bar,
            0.05,
            &format!("a kick every {bar} beats"),
        );
    }
}
