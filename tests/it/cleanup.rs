//! The cleanup of decoded notes (#60) on the octave ghost that the
//! decoder adds above a low saw.

use sound_to_patch::audio::SampleRate;
use sound_to_patch::notes::{
    CleanupSteps, MODEL_SHA256, NoteList, PitchTrack, Profile, TRANSCRIBER_VERSION, Transcriber,
    Voicing, clean_up, join_splits, merge_overlaps, transcribe_stem,
};
use sound_to_patch::units::Bpm;

use crate::support::signals::{bandlimited_saw, times};

/// One A2 (110 Hz) band-limited saw at amplitude 0.3: 0.5 s from 0.5 s
/// with a 5 ms attack, then a 0.3 s linear release, dry, at 22,050 Hz.
fn a2_saw() -> Vec<f64> {
    let rate = 22_050.0;
    let t = times(rate, crate::support::cast::round_to_usize(1.5 * rate));
    bandlimited_saw(110.0, rate, &t, |_| 0.0)
        .into_iter()
        .zip(&t)
        .map(|(value, &time)| {
            let since = time - 0.5;
            let envelope = if since < 0.0 {
                0.0
            } else {
                (since / 0.005).min(1.0) * (1.0 - (since - 0.5).max(0.0) / 0.3).max(0.0)
            };
            0.3 * envelope * value
        })
        .collect()
}

/// The decoder gives the octave ghost at MIDI 57 with the bass profile; the
/// cleanup leaves the one A2.
#[test]
fn the_octave_ghost_of_a_110_hz_saw_merges_into_its_note() {
    let rate = SampleRate::new(22_050).unwrap();
    let bpm = Bpm::new(120.0).unwrap();
    let found = transcribe_stem(
        &Transcriber::new().unwrap(),
        &a2_saw(),
        rate,
        bpm,
        Some(Profile::Bass),
    )
    .unwrap();
    let pitches = |list: &NoteList| {
        list.notes()
            .iter()
            .map(|note| note.pitch().get())
            .collect::<Vec<_>>()
    };
    assert!(
        pitches(&found.decoded).contains(&57.0),
        "no ghost to remove: {:?}",
        pitches(&found.decoded)
    );
    assert_eq!(pitches(&found.notes), vec![45.0]);
    assert_eq!(found.steps.octaves.moved, 1);
    assert_eq!(found.notes.notes()[0].octave_uncertain(), Some(false));
    assert!(found.notes.notes()[0].contour().is_some());
    // The record names the model, the crate, the profile and the decoder
    // parameters of this decode.
    assert_eq!(found.record.model_sha256, MODEL_SHA256);
    assert_eq!(found.record.crate_version, TRANSCRIBER_VERSION);
    assert_eq!(found.record.profile, "bass");
    assert_eq!(
        found.record.note_params,
        Profile::Bass.settings(bpm).note_params()
    );
}

const RATE: f64 = 22_050.0;

/// The times of a 4 s stem at 22,050 Hz.
fn four_seconds() -> Vec<f64> {
    times(RATE, crate::support::cast::round_to_usize(4.0 * RATE))
}

/// Band-limited saws at `f0s`, each at amplitude 0.3 shared between them,
/// for 4 s at 22,050 Hz. They sound together for `on` seconds in every
/// 0.25 s with a linear attack of `attack` seconds, then fall with time
/// constant `tail` (silence at 0), over white noise `floor` decibels below
/// 0.3.
pub(crate) fn gated(f0s: &[f64], attack: f64, on: f64, tail: f64, floor: f64) -> Vec<f64> {
    let t = four_seconds();
    let saws: Vec<Vec<f64>> = f0s
        .iter()
        .map(|&f0| bandlimited_saw(f0, RATE, &t, |_| 0.0))
        .collect();
    let noise = crate::support::rng::uniform(278, t.len());
    let share = 0.3 / f64::from(u32::try_from(f0s.len()).unwrap());
    t.iter()
        .zip(&noise)
        .enumerate()
        .map(|(index, (&time, &uniform))| {
            let since = time % 0.25;
            let envelope = if since < on {
                (since / attack).min(1.0)
            } else if tail > 0.0 {
                (-(since - on) / tail).exp()
            } else {
                0.0
            };
            let tone: f64 = saws.iter().map(|saw| saw[index]).sum();
            share * envelope * tone + 0.3 * 10_f64.powf(floor / 20.0) * (2.0 * uniform - 1.0)
        })
        .collect()
}

/// One band-limited saw at `f0` held for 4 s at amplitude 0.3, with 5 ms
/// ramps, times `gain(t)`.
fn held(f0: f64, gain: impl Fn(f64) -> f64) -> Vec<f64> {
    let t = four_seconds();
    bandlimited_saw(f0, RATE, &t, |_| 0.0)
        .into_iter()
        .zip(&t)
        .map(|(value, &time)| {
            let ramps = (time / 0.005).min(1.0).min((4.0 - time) / 0.005);
            0.3 * ramps * gain(time) * value
        })
        .collect()
}

/// A tremolo of `depth` at `rate` Hz.
fn tremolo(depth: f64, rate: f64) -> impl Fn(f64) -> f64 {
    move |time| 1.0 - depth * (0.5 + 0.5 * (std::f64::consts::TAU * rate * time).sin())
}

/// A full-depth tremolo at `rate` Hz that starts `phase` radians into its
/// cycle.
fn phased(rate: f64, phase: f64) -> impl Fn(f64) -> f64 {
    move |time| 1.0 - (0.5 + 0.5 * (std::f64::consts::TAU * rate * time + phase).sin())
}

/// What the cleanup gives on a stem: the decoded notes with the bass
/// profile at 120 BPM, the count after the ghost merge, the notes after
/// the split join with no attack test (the stem read as silence), the
/// kept notes and the count after each cleanup step. It runs `clean_up`
/// with an empty f0 track, which takes minutes on these stems in a debug
/// build: every note stays unchecked.
struct Cleaned {
    decoded: NoteList,
    merged: usize,
    onset_only: NoteList,
    kept: NoteList,
    steps: CleanupSteps,
}

fn cleaned(stem: &[f64]) -> Cleaned {
    let rate = SampleRate::new(22_050).unwrap();
    let run = Transcriber::new().unwrap().run(stem, rate).unwrap();
    let decoded = run
        .decode(&Profile::Bass.settings(Bpm::new(120.0).unwrap()))
        .unwrap();
    let merged = merge_overlaps(&decoded);
    let onset_only = join_splits(&merged, &run);
    let (kept, steps) = clean_up(
        &decoded,
        &run,
        &PitchTrack::from_frames(Vec::new()),
        Voicing::Monophonic,
        stem,
        rate,
    );
    assert_eq!(steps.merged, merged.notes().len());
    Cleaned {
        decoded,
        merged: merged.notes().len(),
        onset_only,
        kept,
        steps,
    }
}

/// The decoder fills the 50 ms gaps of a gated A1 and reads most repeats
/// on a weak onset (#278): the onset rule alone joins the 16 notes into 6.
/// The attack test keeps them apart, and no onset moves.
#[test]
fn a_gated_55_hz_saw_keeps_its_16_repeats() {
    let found = cleaned(&gated(&[55.0], 0.005, 0.2, 0.0, -400.0));
    assert_eq!(found.decoded.notes().len(), 16);
    assert_eq!(found.onset_only.notes().len(), 6);
    // The eval's onset tolerance. The decoder starts these notes from 6 ms
    // before to 37 ms after their gate, wider than a model frame.
    let tolerance = 0.05;
    let notes = found.kept.notes();
    assert_eq!(notes.len(), 16);
    for (index, note) in notes.iter().enumerate() {
        let gate = 0.25 * f64::from(u8::try_from(index).unwrap());
        assert_eq!(note.pitch().get(), 33.0);
        let late = note.onset().get() - gate;
        assert!(
            late.abs() <= tolerance,
            "note {index} starts {late} s after its gate"
        );
        assert!(
            found
                .decoded
                .notes()
                .iter()
                .any(|decoded| decoded.onset() == note.onset()),
            "note {index} starts off every decoded onset"
        );
    }
}

/// The decoder reads a gated A1 with 30 ms gaps as 9 notes for 16, and no
/// later step could find the rest (#280). The split at attacks gives each
/// gate its note, within the eval's 50 ms, before the split join, which
/// keeps them.
#[test]
fn a_gated_55_hz_saw_with_30_ms_gaps_gets_its_16_notes() {
    let found = cleaned(&gated(&[55.0], 0.005, 0.22, 0.0, -400.0));
    assert_eq!(
        (
            found.decoded.notes().len(),
            found.kept.notes().len(),
            gates_hit(&found.kept, 33.0)
        ),
        (9, 16, 16)
    );
    let steps = found.steps;
    assert_eq!(
        (steps.merged, steps.split, steps.joined, steps.kept),
        (9, 16, 16, 16)
    );
}

/// A held saw under an amplitude LFO of 6 dB, one cycle per quarter or
/// per eighth note at 120 BPM, keeps the onset rule's notes: one, but two
/// for A1 at 1/8, where the decoder reads a strong onset at 0.19 s.
#[test]
fn a_6_db_lfo_never_splits_a_held_note() {
    for (name, f0, rate, count) in [
        ("A1 at 1/4", 55.0, 2.0, 1),
        ("A1 at 1/8", 55.0, 4.0, 2),
        ("E1 at 1/4", 41.203, 2.0, 1),
        ("E1 at 1/8", 41.203, 4.0, 1),
    ] {
        let found = cleaned(&held(f0, |time| {
            10_f64.powf(-6.0 / 20.0 * (0.5 - 0.5 * (std::f64::consts::TAU * rate * time).cos()))
        }));
        assert_eq!(found.kept.notes().len(), count, "{name}");
        assert_eq!(starts(&found.kept), starts(&found.onset_only), "{name}");
    }
}

/// A held saw through a low-pass filter swept from 200 Hz to 4 kHz and back
/// once a second stays one note.
#[test]
fn a_filter_sweep_never_splits_a_held_note() {
    for (name, f0) in [("A1", 55.0), ("E1", 41.203)] {
        let mut state = 0.0;
        let stem: Vec<f64> = held(f0, |_| 1.0)
            .into_iter()
            .zip(four_seconds())
            .map(|(value, time)| {
                let sweep = 0.5 - 0.5 * (std::f64::consts::TAU * time).cos();
                let cutoff = 200.0 * 20_f64.powf(sweep);
                let share = 1.0 - (-std::f64::consts::TAU * cutoff / RATE).exp();
                state += share * (value - state);
                state
            })
            .collect();
        let found = cleaned(&stem);
        assert_eq!(found.kept.notes().len(), 1, "{name}");
        assert_eq!(starts(&found.kept), starts(&found.onset_only), "{name}");
    }
}

/// The onset and the pitch of each note in `notes`.
fn starts(notes: &NoteList) -> Vec<(f64, f64)> {
    notes
        .notes()
        .iter()
        .map(|note| (note.onset().get(), note.pitch().get()))
        .collect()
}

/// The gates of 0.25 s that a note of `key` in `notes` starts on, within
/// the eval's 50 ms.
fn gates_hit(notes: &NoteList, key: f64) -> usize {
    (0..16_u8)
        .filter(|&gate| {
            notes.notes().iter().any(|note| {
                (note.pitch().get() - key).abs() < 0.5
                    && (note.onset().get() - 0.25 * f64::from(gate)).abs() <= 0.05
            })
        })
        .count()
}

/// Repeats that fall into the gap with a tail, over a noise floor, in two
/// voices or with a DC offset keep more gates than the onset rule alone:
/// the gates hit by each key, decoded, after the onset rule alone and
/// kept, and the count of each list, so an extra note fails.
#[test]
fn repeats_with_a_tail_a_floor_or_a_second_voice_keep_more_gates() {
    for (name, stem, keys, hits, counts) in [
        (
            "10 ms tail over -60 dB",
            gated(&[55.0], 0.005, 0.2, 0.01, -60.0),
            vec![33.0],
            vec![(13, 6, 16)],
            (13, 6, 16),
        ),
        (
            "30 ms gaps, 5 ms tail",
            gated(&[55.0], 0.005, 0.22, 0.005, -400.0),
            vec![33.0],
            vec![(8, 2, 16)],
            (8, 2, 16),
        ),
        (
            "A1 and E2",
            gated(&[55.0, 82.407], 0.005, 0.2, 0.0, -400.0),
            vec![33.0, 40.0],
            vec![(14, 5, 11), (16, 16, 16)],
            (39, 30, 36),
        ),
        (
            "A1 and A2",
            gated(&[55.0, 110.0], 0.005, 0.2, 0.0, -400.0),
            vec![33.0, 45.0],
            vec![(16, 12, 16), (16, 16, 16)],
            (42, 28, 32),
        ),
        (
            "8ths, 10 ms attack, 100 ms gaps, 20 ms tail over -40 dB",
            gated(&[55.0], 0.01, 0.15, 0.02, -40.0),
            vec![33.0],
            vec![(16, 12, 13)],
            (16, 12, 13),
        ),
        (
            "a DC offset 20 dB below the saw",
            gated(&[55.0], 0.005, 0.2, 0.0, -400.0)
                .into_iter()
                .map(|x| x + 0.03)
                .collect(),
            vec![33.0],
            vec![(16, 6, 16)],
            (16, 6, 16),
        ),
    ] {
        let found = cleaned(&stem);
        let found_hits: Vec<_> = keys
            .iter()
            .map(|&key| {
                (
                    gates_hit(&found.decoded, key),
                    gates_hit(&found.onset_only, key),
                    gates_hit(&found.kept, key),
                )
            })
            .collect();
        assert_eq!(found_hits, hits, "{name}");
        assert_eq!(
            (
                found.decoded.notes().len(),
                found.onset_only.notes().len(),
                found.kept.notes().len(),
            ),
            counts,
            "{name}"
        );
    }
}

/// A triangle tremolo to silence at `rate` Hz.
fn triangle(rate: f64) -> impl Fn(f64) -> f64 {
    move |time| (2.0 * (rate * time - (rate * time + 0.5).floor())).abs()
}

/// One band-limited saw at `f0` like [`held`], with a vibrato of 50 cents
/// at 5.5 Hz.
fn held_with_vibrato(f0: f64, gain: impl Fn(f64) -> f64) -> Vec<f64> {
    let mut phase = 0.0;
    four_seconds()
        .into_iter()
        .map(|time| {
            phase += f0 * 2_f64.powf((std::f64::consts::TAU * 5.5 * time).sin() / 24.0) / RATE;
            let mut value = 0.0;
            let mut h = 1.0;
            while h * f0 * 1.03 < RATE / 2.0 {
                value += (std::f64::consts::TAU * h * phase).sin() / h;
                h += 1.0;
            }
            let ramps = (time / 0.005).min(1.0).min((4.0 - time) / 0.005);
            0.3 * ramps * gain(time) * value
        })
        .collect()
}

/// A sidechain duck every 0.5 s to `depth` decibels, back to 0 dB
/// linearly in decibels over 50 ms.
fn duck(depth: f64) -> impl Fn(f64) -> f64 {
    move |time| {
        let since = time % 0.5;
        let db = if since < 0.05 {
            depth * (1.0 - since / 0.05)
        } else {
            0.0
        };
        10_f64.powf(db / 20.0)
    }
}

/// The gain over time of a held case.
#[derive(Clone, Copy)]
enum Gain {
    /// [`tremolo`]: a depth and a rate.
    Tremolo(f64, f64),
    /// [`phased`]: a rate and a phase.
    Phased(f64, f64),
    /// [`triangle`]: a rate.
    Triangle(f64),
    /// [`tremolo`] over [`held_with_vibrato`]: a depth and a rate.
    Vibrato(f64, f64),
    /// [`duck`]: a depth.
    Duck(f64),
}

/// Held notes under a tremolo or a sidechain duck from B0 to A3, each
/// named after its register, with the note's fundamental and its gain.
const HELD_CASES: [(&str, f64, Gain); 28] = [
    ("A2, depth 0.8 at 5 Hz", 110.0, Gain::Tremolo(0.8, 5.0)),
    ("A2, depth 1 at 8 Hz", 110.0, Gain::Tremolo(1.0, 8.0)),
    ("A1, depth 1 at 4 Hz", 55.0, Gain::Tremolo(1.0, 4.0)),
    ("A1, depth 1 at 8 Hz", 55.0, Gain::Tremolo(1.0, 8.0)),
    ("A1, depth 1 at 12 Hz", 55.0, Gain::Tremolo(1.0, 12.0)),
    ("A1, depth 1 at 14 Hz", 55.0, Gain::Tremolo(1.0, 14.0)),
    (
        "A1, depth 1 at 12 Hz, phase 0.68",
        55.0,
        Gain::Phased(12.0, 0.68),
    ),
    (
        "A1, depth 1 at 12 Hz, phase 1.39",
        55.0,
        Gain::Phased(12.0, 1.39),
    ),
    (
        "A1, depth 1 at 16 Hz, phase 0.77",
        55.0,
        Gain::Phased(16.0, 0.77),
    ),
    (
        "E1, depth 1 at 12 Hz, phase 1.78",
        41.203,
        Gain::Phased(12.0, 1.78),
    ),
    (
        "E1, depth 1 at 18 Hz, phase 5.61",
        41.203,
        Gain::Phased(18.0, 5.61),
    ),
    (
        "E1, depth 1 at 14 Hz, phase 0.85",
        41.203,
        Gain::Phased(14.0, 0.85),
    ),
    (
        "E1, depth 1 at 14 Hz, phase 4.65",
        41.203,
        Gain::Phased(14.0, 4.65),
    ),
    (
        "E1, depth 1 at 14.41 Hz, phase 5.58",
        41.203,
        Gain::Phased(14.41, 5.58),
    ),
    (
        "E1, depth 1 at 16 Hz, phase 4.21",
        41.203,
        Gain::Phased(16.0, 4.21),
    ),
    (
        "E1, depth 1 at 17.22 Hz, phase 1.70",
        41.203,
        Gain::Phased(17.22, 1.7),
    ),
    (
        "E1, depth 1 at 18 Hz, phase 5.31",
        41.203,
        Gain::Phased(18.0, 5.31),
    ),
    (
        "E1, depth 1 at 20 Hz, phase 1.32",
        41.203,
        Gain::Phased(20.0, 1.32),
    ),
    (
        "B0, depth 1 at 14 Hz, phase 2.62",
        30.868,
        Gain::Phased(14.0, 2.62),
    ),
    ("A1, triangle at 8 Hz", 55.0, Gain::Triangle(8.0)),
    (
        "A1, depth 1 at 8 Hz with vibrato",
        55.0,
        Gain::Vibrato(1.0, 8.0),
    ),
    ("A1, -16 dB duck", 55.0, Gain::Duck(-16.0)),
    ("A1, -30 dB duck", 55.0, Gain::Duck(-30.0)),
    ("E1, depth 1 at 3 Hz", 41.203, Gain::Tremolo(1.0, 3.0)),
    ("E1, depth 1 at 10 Hz", 41.203, Gain::Tremolo(1.0, 10.0)),
    ("B0, depth 1 at 2 Hz", 30.868, Gain::Tremolo(1.0, 2.0)),
    ("A3, depth 1 at 12 Hz", 220.0, Gain::Tremolo(1.0, 12.0)),
    ("A3, depth 1 at 14 Hz", 220.0, Gain::Tremolo(1.0, 14.0)),
];

/// The stem of a case of [`HELD_CASES`].
fn held_case(f0: f64, gain: Gain) -> Vec<f64> {
    match gain {
        Gain::Tremolo(depth, rate) => held(f0, tremolo(depth, rate)),
        Gain::Phased(rate, phase) => held(f0, phased(rate, phase)),
        Gain::Triangle(rate) => held(f0, triangle(rate)),
        Gain::Vibrato(depth, rate) => held_with_vibrato(f0, tremolo(depth, rate)),
        Gain::Duck(depth) => held(f0, duck(depth)),
    }
}

/// The cases of [`HELD_CASES`] in `register` join as the onset rule alone
/// would join them.
fn assert_held_cases_join(register: &str) {
    let mut cases = 0;
    for (name, f0, gain) in HELD_CASES {
        if name.split(',').next() != Some(register) {
            continue;
        }
        cases += 1;
        let found = cleaned(&held_case(f0, gain));
        assert!(
            found.onset_only.notes().len() < found.merged,
            "{name}: nothing to join"
        );
        assert_eq!(starts(&found.kept), starts(&found.onset_only), "{name}");
    }
    assert!(cases > 0, "no case at {register}");
}

#[test]
fn every_held_case_has_a_register_with_a_test() {
    for (name, ..) in HELD_CASES {
        let register = name.split(',').next().unwrap_or_default();
        assert!(["B0", "E1", "A1", "A2", "A3"].contains(&register), "{name}");
    }
}

#[test]
fn a_tremolo_or_a_duck_never_splits_a_held_note_at_b0() {
    assert_held_cases_join("B0");
}

#[test]
fn a_tremolo_or_a_duck_never_splits_a_held_note_at_e1() {
    assert_held_cases_join("E1");
}

#[test]
fn a_tremolo_or_a_duck_never_splits_a_held_note_at_a1() {
    assert_held_cases_join("A1");
}

#[test]
fn a_tremolo_or_a_duck_never_splits_a_held_note_at_a2_and_a3() {
    assert_held_cases_join("A2");
    assert_held_cases_join("A3");
}

/// A square tremolo to silence at `rate` Hz, loud for the share `on` of
/// each cycle, with linear edges of 1 ms.
fn square(rate: f64, on: f64) -> impl Fn(f64) -> f64 {
    move |time| {
        let cycle = (rate * time).fract();
        let into = if cycle < on {
            cycle.min(on - cycle)
        } else {
            -(cycle - on).min(1.0 - cycle)
        };
        (0.5 + into / rate / 0.001).clamp(0.0, 1.0)
    }
}

/// A square tremolo off half of each cycle joins as the onset rule alone
/// would join it.
#[test]
fn a_square_tremolo_at_half_duty_never_splits_a_held_note() {
    for (name, f0, rate) in [
        ("E1 at 3 Hz", 41.203, 3.0),
        ("A1 at 6 Hz", 55.0, 6.0),
        ("A1 at 9 Hz", 55.0, 9.0),
    ] {
        let found = cleaned(&held(f0, square(rate, 0.5)));
        assert!(
            found.onset_only.notes().len() < found.merged,
            "{name}: nothing to join"
        );
        assert_eq!(starts(&found.kept), starts(&found.onset_only), "{name}");
    }
}

/// A square tremolo off for 30 % of each cycle reads by level as a gated
/// repeat with short gaps, and splits a held note at A1 into a note per
/// pulse, two more than the decoder gives.
#[test]
fn a_square_tremolo_off_for_30_percent_splits_like_a_gate() {
    let found = cleaned(&held(55.0, square(6.0, 0.7)));
    assert_eq!(
        (
            found.decoded.notes().len(),
            found.onset_only.notes().len(),
            found.kept.notes().len()
        ),
        (22, 6, 24)
    );
}

/// A held A1 20 dB under a louder A2 gated for 0.2 s in every 0.25 s.
/// The stem dips at each gate, and below A2 so do the A1's harmonics, so
/// the split join keeps 12 of the decoder's 14 pieces apart. The A1's
/// fundamental never dips, so the attack split cuts nothing more.
#[test]
fn a_held_note_under_a_louder_gated_octave_gets_no_more_cuts() {
    let low = held(55.0, |_| 0.1);
    let stem: Vec<f64> = gated(&[110.0], 0.005, 0.2, 0.0, -400.0)
        .into_iter()
        .zip(low)
        .map(|(high, low)| high + low)
        .collect();
    let found = cleaned(&stem);
    let at_a1 = |list: &NoteList| {
        list.notes()
            .iter()
            .filter(|note| (note.pitch().get() - 33.0).abs() < 0.5)
            .count()
    };
    assert_eq!(
        (
            at_a1(&found.decoded),
            at_a1(&found.onset_only),
            at_a1(&found.kept)
        ),
        (14, 4, 12)
    );
}

/// A held A1 9.5 dB under a louder B1, two semitones up, gated for 0.2 s
/// in every 0.25 s. The A1's fundamental bin reads the B1 within 1 dB, so
/// it dips at each gate, but the split join joins the decoder's 16 A1
/// pieces into 4, and the attack split cuts nothing more.
#[test]
fn a_held_note_under_a_louder_gated_second_gets_no_more_cuts() {
    let low = held(55.0, |_| 1.0 / 3.0);
    let stem: Vec<f64> = gated(&[61.735], 0.005, 0.2, 0.0, -400.0)
        .into_iter()
        .zip(low)
        .map(|(high, low)| high + low)
        .collect();
    let found = cleaned(&stem);
    let at_a1 = |list: &NoteList| {
        list.notes()
            .iter()
            .filter(|note| (note.pitch().get() - 33.0).abs() < 0.5)
            .count()
    };
    assert_eq!(
        (
            at_a1(&found.decoded),
            at_a1(&found.onset_only),
            at_a1(&found.kept)
        ),
        (16, 4, 4)
    );
}
