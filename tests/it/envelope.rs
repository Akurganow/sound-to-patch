//! The envelope layer (#71, #69): the sidechain pump and the amplitude
//! envelope on generated signals.
//!
//! `cargo xtask eval --layer envelope` scores the same methods on a larger
//! generated set in a release build; these cases pin the acceptance of the
//! two issues.

use std::f64::consts::TAU;

use sound_to_patch::audio::{BeatGrid, SampleRate};
use sound_to_patch::confidence::Level;
use sound_to_patch::estimate::envelope::{
    EnvelopeInput, EnvelopeLayer, LevelTrack, PUMP_RMS_PERIODS, Pump, PumpLock, amp_proxy,
};
use sound_to_patch::notes::NoteList;
use sound_to_patch::units::{Bpm, Hz, Seconds};

use crate::support::envelope::{
    Adsr, CHORD, Duck, adsr_notes, chord, note_list, plucks, samples, saw, swept_saw, tremolo,
};
use crate::support::golden::{Golden, float};
use crate::support::rng::uniform;

const RATE: u32 = 22_050;
const BPM: f64 = 128.0;

fn fs() -> f64 {
    f64::from(RATE)
}

fn rate() -> SampleRate {
    SampleRate::new(RATE).unwrap()
}

fn grid(bpm: f64) -> BeatGrid {
    BeatGrid::new(Bpm::new(bpm).unwrap(), Seconds::clamped(0.0))
}

fn beat() -> f64 {
    60.0 / BPM
}

fn layer(x: &[f64], notes: &NoteList, bpm: f64) -> EnvelopeLayer {
    EnvelopeLayer::of(&EnvelopeInput {
        mono: x,
        rate: rate(),
        notes,
        grid: grid(bpm),
    })
    .unwrap()
}

/// The pump of a stem whose notes hold from their onsets to their ends.
fn pump(x: &[f64], notes: &NoteList, f0: f64) -> Option<Pump> {
    pump_at(x, notes, f0, BPM)
}

/// [`pump`] on a grid at `bpm`.
fn pump_at(x: &[f64], notes: &NoteList, f0: f64, bpm: f64) -> Option<Pump> {
    let end = f64::from(u32::try_from(x.len()).unwrap()) / fs();
    let track = LevelTrack::rms(
        x,
        0,
        rate(),
        (Seconds::clamped(0.0), Seconds::clamped(end)),
        Hz::new(f0).unwrap(),
        PUMP_RMS_PERIODS,
    );
    let held: Vec<(f64, f64)> = notes
        .notes()
        .iter()
        .map(|n| (n.onset().get(), n.offset().get()))
        .collect();
    Pump::find(&track, grid(bpm), notes, &held)
}

/// The held three-note chord of the pump table, 16 beats long.
fn held_chord() -> (Vec<f64>, NoteList) {
    let seconds = 16.0 * beat();
    (
        chord(&CHORD, fs(), seconds, 1),
        note_list(&CHORD.map(|f| (0.0, seconds, f))),
    )
}

#[test]
fn a_held_chord_without_a_duck_reads_no_pump() {
    let (x, notes) = held_chord();
    let p = pump(&x, &notes, CHORD[0]).unwrap();
    assert!(p.depth.get() <= 2.0, "{p:?}");
    assert_eq!(p.lock, PumpLock::None);
}

#[test]
fn each_row_of_the_pump_table_reads_its_depth_and_recovery() {
    // #71: true depth and release, and the time back within 1.5 dB that
    // the issue measured.
    let (x, notes) = held_chord();
    for (depth_db, release_s, back_s) in [
        (6.0, 0.15, 0.212),
        (12.0, 0.08, 0.176),
        (12.0, 0.15, 0.234),
        (12.0, 0.3, 0.264),
        (18.0, 0.15, 0.242),
    ] {
        let duck = Duck {
            depth_db,
            release_s,
            bpm: BPM,
        };
        let p = pump(&duck.apply(&x, fs()), &notes, CHORD[0]).unwrap();
        let depth = p.depth.get();
        assert!(
            depth <= depth_db && depth >= depth_db - 5.0,
            "{depth_db}/{release_s}: {depth}"
        );
        let back = p.back.unwrap().get();
        assert!(
            (back / back_s - 1.0).abs() <= 0.3,
            "{depth_db}/{release_s}: {back}"
        );
        assert_eq!(p.lock, PumpLock::Beat, "{depth_db}/{release_s}");
        assert!(p.strips());
        let tau = p.tau.unwrap().get();
        assert!((tau - back / (depth / 1.5).ln()).abs() < 1e-9, "{tau}");
    }
}

#[test]
fn a_note_without_a_knee_holds_to_25_ms_before_its_end() {
    // The held chord fills the stem to its last sample, so no note has a
    // knee. Its held part ends 25 ms before the end, and the last beat,
    // which ends with the stem, does not count: beats 1 to 14.
    let (x, notes) = held_chord();
    let read = layer(&x, &notes, BPM);
    assert_eq!(read.counts.knees, 0, "{:?}", read.notes);
    assert_eq!(read.pump.as_ref().unwrap().beats, 14);
}

#[test]
fn each_note_holds_to_its_own_knee() {
    // A2 held 1 s with a 50 ms release, then A2 held to the stem's end:
    // the first note's held part ends at its own knee, so the gap between
    // the notes counts no beat.
    let adsr = Adsr {
        attack_s: 0.01,
        hold_s: 0.0,
        decay_s: 0.1,
        sustain_db: -6.0,
        release_s: 0.05,
    };
    let seconds = 16.0 * beat();
    let x = adsr_notes(
        &[
            (0.0, 4.0 * beat(), 110.0, 0.0),
            (6.0 * beat(), seconds + 1.0, 110.0, 0.0),
        ],
        adsr,
        fs(),
        seconds,
    );
    let notes = note_list(&[
        (0.0, 4.0 * beat() + 0.09, 110.0),
        (6.0 * beat(), seconds, 110.0),
    ]);
    let read = layer(&x, &notes, BPM);
    assert_eq!(read.counts.knees, 1, "{:?}", read.notes);
    // Beats 1 and 2 of the first note, 7 to 14 of the second.
    assert_eq!(read.pump.as_ref().unwrap().beats, 10);
}

#[test]
fn only_a_beat_lock_is_counted_again_over_the_notes() {
    // Five A2 notes, each over two beats and a rest beat: struck 60 ms
    // before its first beat, released over 50 ms to end 10 ms before the
    // rest beat, its span in the list running 30 ms into it. The held parts
    // end at the knees and count 5 flat beats, no lock, and nothing is
    // stripped.
    let adsr = Adsr {
        attack_s: 0.01,
        hold_s: 0.0,
        decay_s: 0.1,
        sustain_db: 0.0,
        release_s: 0.05,
    };
    let b = beat();
    let (mut audio, mut list) = (Vec::new(), Vec::new());
    for k in [1, 4, 7, 10, 13] {
        let on = f64::from(k) * b - 0.06;
        audio.push((on, f64::from(k + 2) * b - 0.06, 110.0, 0.0));
        list.push((on, f64::from(k + 2) * b + 0.03, 110.0));
    }
    let x = adsr_notes(&audio, adsr, fs(), 16.0 * b + 0.1);
    let read = layer(&x, &note_list(&list), BPM);
    assert_eq!(read.counts.knees, 5, "{:?}", read.notes);
    let p = read.pump.as_ref().unwrap();
    assert_eq!(p.lock, PumpLock::None, "{p:?}");
    assert!(!read.flags.pump_stripped, "{p:?}");
}

#[test]
fn a_duck_late_in_the_beat_recovers_over_the_wrap() {
    // The grid starts at 0 s, so a stem that starts half a beat early
    // shifts the duck to mid-beat: a 300 ms release then recovers after
    // the beat's end.
    let (x, notes) = held_chord();
    let duck = Duck {
        depth_db: 12.0,
        release_s: 0.3,
        bpm: BPM,
    };
    let shift = 0.5 * beat();
    let late: Vec<f64> = x
        .iter()
        .enumerate()
        .map(|(n, v)| v * duck.gain(f64::from(u32::try_from(n).unwrap()) / fs() - shift))
        .collect();
    let aligned = pump(&duck.apply(&x, fs()), &notes, CHORD[0]).unwrap();
    let p = pump(&late, &notes, CHORD[0]).unwrap();
    assert_eq!(p.lock, PumpLock::Beat, "{p:?}");
    let (back, want) = (p.back.unwrap().get(), aligned.back.unwrap().get());
    assert!(back > 0.5 * beat(), "{back}");
    // The pump table's tolerance: the slow tail crosses 1.5 dB where the
    // chord's ripple moves it.
    assert!((back / want - 1.0).abs() <= 0.3, "{back} against {want}");
}

#[test]
fn a_duck_over_fewer_than_8_counted_beats_is_not_stripped() {
    // Six beats of the chord count five: the duck locks to the beat and
    // reads its depth, but five beats are too few to strip.
    let seconds = 6.0 * beat();
    let x = chord(&CHORD, fs(), seconds, 1);
    let notes = note_list(&CHORD.map(|f| (0.0, seconds, f)));
    let duck = Duck {
        depth_db: 12.0,
        release_s: 0.15,
        bpm: BPM,
    };
    let p = pump(&duck.apply(&x, fs()), &notes, CHORD[0]).unwrap();
    assert_eq!(p.lock, PumpLock::Beat, "{p:?}");
    assert!(p.beats < 8, "{p:?}");
    assert!(p.depth.get() >= 7.0, "{p:?}");
    assert!(!p.strips());
}

#[test]
fn a_duck_over_a_level_step_is_counted_over_the_notes() {
    // A held saw under a 12 dB duck falls 20 dB over 100 ms after 6.2
    // beats and holds there. The first read takes the fall for a note-off
    // knee, which leaves 5 held beats; counted again over the note to its
    // end, 14 beats strip the pump.
    let seconds = 16.0 * beat();
    let duck = Duck {
        depth_db: 12.0,
        release_s: 0.15,
        bpm: BPM,
    };
    let (step, low) = (6.2 * beat(), 10f64.powf(-20.0 / 20.0));
    let x: Vec<f64> = duck
        .apply(&chord(&[110.0], fs(), seconds, 1), fs())
        .iter()
        .enumerate()
        .map(|(n, v)| {
            let t = f64::from(u32::try_from(n).unwrap()) / fs();
            v * (1.0 - (1.0 - low) * ((t - step) / 0.1).clamp(0.0, 1.0))
        })
        .collect();
    let read = layer(&x, &note_list(&[(0.0, seconds, 110.0)]), BPM);
    let p = read.pump.as_ref().unwrap();
    assert!(read.flags.pump_stripped, "{p:?}");
    // Beats 1 to 14: the span ends 25 ms before the note's end.
    assert_eq!(p.beats, 14, "{p:?}");
}

#[test]
fn a_sub_under_a_kick_on_every_beat_is_not_stripped() {
    // A sine sub at 43.65 Hz, decaying with a 2 s time constant, under a
    // kick on each of the first five beats at 120 BPM that drops from 205
    // to 55 Hz. The kick's tail beats against the sub, and the curve over
    // the 5 counted beats locks to the beat at 3.7 dB.
    let kick = |t: f64| -> f64 {
        if t >= 0.35 {
            return 0.0;
        }
        let phase = TAU * (55.0 * t + 150.0 * 0.02 * (1.0 - (-t / 0.02).exp()));
        phase.sin() * (-t / 0.09).exp()
    };
    let x: Vec<f64> = (0..samples(3.5, fs()))
        .map(|n| {
            let t = f64::from(u32::try_from(n).unwrap()) / fs() - 0.2;
            if !(0.0..3.0).contains(&t) {
                return 0.0;
            }
            let sub = 0.9 * (TAU * 43.65 * t).sin() * (-t / 2.0).exp() * (t / 0.005).min(1.0);
            let struck = if t < 2.35 { 0.7 * kick(t % 0.5) } else { 0.0 };
            sub + struck
        })
        .collect();
    let read = layer(&x, &note_list(&[(0.2, 3.2, 43.65)]), 120.0);
    assert!(!read.flags.pump_stripped, "{:?}", read.pump);
}

#[test]
fn the_pump_reads_the_numpy_detector_on_the_issue_rows() {
    // tests/fixtures/golden/prototype/71-pump.json: the #71 method in
    // NumPy on the same chord and ducks.
    let golden = Golden::load("prototype/71-pump");
    let tolerance = golden.tolerance().abs;
    let (x, notes) = held_chord();
    for row in golden.values()["rows"].as_array().unwrap() {
        let duck = Duck {
            depth_db: float(&row["depth_db"]),
            release_s: float(&row["release_s"]),
            bpm: BPM,
        };
        let p = pump(&duck.apply(&x, fs()), &notes, CHORD[0]).unwrap();
        let want = float(&row["read_depth_db"]);
        assert!((p.depth.get() - want).abs() <= tolerance, "{row}: {p:?}");
        assert_eq!(
            p.beats,
            usize::try_from(row["beats"].as_u64().unwrap()).unwrap()
        );
        if !row["back_s"].is_null() {
            let back = p.back.unwrap().get();
            assert!(
                (back - float(&row["back_s"])).abs() <= tolerance,
                "{row}: {back}"
            );
        }
    }
}

#[test]
fn the_pump_needs_4_beats_struck_50_ms_before_them() {
    // A chord struck 60 ms before beat 1 and held to the end of beat 4
    // counts beats 1 to 4; held to the end of beat 3, too few.
    let chord_of = |beats: f64, lead: f64| {
        let seconds = beats * beat();
        let x = chord(&CHORD, fs(), seconds, 1);
        (x, note_list(&CHORD.map(|f| (beat() - lead, seconds, f))))
    };
    let (x, notes) = chord_of(5.0, 0.06);
    assert_eq!(pump(&x, &notes, CHORD[0]).unwrap().beats, 4);
    let (x, notes) = chord_of(4.0, 0.06);
    assert!(pump(&x, &notes, CHORD[0]).is_none());
    // Struck 40 ms before beat 1, the chord counts from beat 2.
    let (x, notes) = chord_of(5.0, 0.04);
    assert!(pump(&x, &notes, CHORD[0]).is_none());
    let (x, notes) = chord_of(6.0, 0.04);
    assert_eq!(pump(&x, &notes, CHORD[0]).unwrap().beats, 4);
}

#[test]
fn a_free_tremolo_is_read_on_the_longest_held_run() {
    // Two held chords with a beat of rest between them, the longer under
    // a 5.3 Hz tremolo, first or second: the free test reads the longer.
    let b = beat();
    let run = |(from, to): (f64, f64), shaken: bool| {
        let x = chord(&CHORD, fs(), (to - from) * b, 1);
        let x = if shaken {
            tremolo(&x, fs(), 5.3, 3.0)
        } else {
            x
        };
        let notes: Vec<(f64, f64, f64)> = CHORD
            .iter()
            .map(|&f| (from * b - 0.06, to * b, f))
            .collect();
        (x, notes)
    };
    for (first, second) in [
        (((0.94, 4.0), false), ((5.0, 23.0), true)),
        (((0.94, 18.0), true), ((19.0, 23.0), false)),
    ] {
        let (one, mut list) = run(first.0, first.1);
        let (two, more) = run(second.0, second.1);
        let mut x = vec![0.0; samples(first.0.0 * b - 0.06, fs())];
        x.extend(one);
        x.resize(samples(second.0.0 * b - 0.06, fs()), 0.0);
        x.extend(two);
        list.extend(more);
        let p = pump(&x, &note_list(&list), CHORD[0]).unwrap();
        assert_eq!(p.lock, PumpLock::Free, "{first:?} {second:?}: {p:?}");
    }
}

#[test]
fn plucks_on_every_beat_or_eighth_read_no_pump() {
    let seconds = 16.0 * beat();
    for every in [beat(), beat() / 2.0] {
        let (x, notes) = plucks(110.0, fs(), seconds, every, 0.25);
        assert!(pump(&x, &notes, 110.0).is_none(), "{every}");
    }
}

#[test]
fn a_pad_that_changes_note_each_bar_reads_under_1_db() {
    let bar = 4.0 * beat();
    let pitches = [110.0, 98.0, 87.31, 98.0];
    let seconds = 4.0 * bar;
    let mut x = vec![0.0; samples(seconds, fs())];
    for (b, &f0) in pitches.iter().enumerate() {
        let start = f64::from(u32::try_from(b).unwrap()) * bar;
        let tone = saw(f0, fs(), x.len(), 0.0);
        for (i, v) in x.iter_mut().enumerate() {
            let t = f64::from(u32::try_from(i).unwrap()) / fs() - start;
            // Held for its bar, then a 15 ms linear release into the next.
            let g = if t < 0.0 {
                0.0
            } else if t < bar {
                1.0
            } else {
                (1.0 - (t - bar) / 0.015).max(0.0)
            };
            *v += 0.3 * g * tone[i];
        }
    }
    let notes = note_list(
        &pitches
            .iter()
            .enumerate()
            .map(|(b, &f)| {
                let start = f64::from(u32::try_from(b).unwrap()) * bar;
                (start, start + bar, f)
            })
            .collect::<Vec<_>>(),
    );
    let p = pump(&x, &notes, 87.31).unwrap();
    assert!(p.depth.get() < 1.0, "{p:?}");
    assert_eq!(p.lock, PumpLock::None);
}

#[test]
fn a_tremolo_off_the_tempo_runs_free_and_one_on_it_is_no_pump() {
    let (x, notes) = held_chord();
    let free = pump(&tremolo(&x, fs(), 5.3, 3.0), &notes, CHORD[0]).unwrap();
    assert_eq!(free.lock, PumpLock::Free, "{free:?}");
    assert!(free.back.is_some() && free.tau.is_none(), "{free:?}");
    // A swing of 0.4 dB either way stays under the 1 dB floor.
    let faint = pump(&tremolo(&x, fs(), 5.3, 0.4), &notes, CHORD[0]).unwrap();
    assert_eq!(faint.lock, PumpLock::None, "{faint:?}");
    // Eighths and sixteenths at the tempo: two and four dips a beat.
    for division in [2.0, 4.0] {
        let synced = pump(
            &tremolo(&x, fs(), division * BPM / 60.0, 3.0),
            &notes,
            CHORD[0],
        )
        .unwrap();
        assert_eq!(synced.lock, PumpLock::None, "{division}: {synced:?}");
    }
}

#[test]
fn a_dip_only_where_a_note_starts_locks_to_the_note() {
    // A held A2; on every other beat an E3 starts 0.3 beats in, and the
    // whole mix dips 9 dB there, back in 40 ms: the new note's envelope.
    let seconds = 16.0 * beat();
    let length = samples(seconds, fs());
    let held = saw(110.0, fs(), length, 0.0);
    let upper = saw(164.81, fs(), length, 0.3);
    let mut notes = vec![(0.0, seconds, 110.0)];
    let onsets: Vec<f64> = (0..8)
        .map(|k| (2.0 * f64::from(k) + 0.3) * beat())
        .collect();
    for &on in &onsets {
        notes.push((on, on + 0.6 * beat(), 164.81));
    }
    let x: Vec<f64> = (0..length)
        .map(|i| {
            let t = f64::from(u32::try_from(i).unwrap()) / fs();
            let since = onsets
                .iter()
                .filter(|&&on| on <= t)
                .map(|&on| t - on)
                .fold(f64::INFINITY, f64::min);
            let sounding = since < 0.6 * beat();
            let dip = 1.0 - (1.0 - 10f64.powf(-9.0 / 20.0)) * (-since / 0.04).exp();
            0.3 * dip * (held[i] + if sounding { upper[i] } else { 0.0 })
        })
        .collect();
    let p = pump(&x, &note_list(&notes), 110.0).unwrap();
    assert_eq!(p.lock, PumpLock::Note, "{p:?}");
    assert!(!p.strips());
}

#[test]
fn a_pump_over_3_db_is_stripped_and_the_notes_read_no_dip() {
    // Eight held notes of 1.6 s under a 12 dB duck: stripped, each note
    // reads a sustain near 0 dB instead of the duck's dip.
    let adsr = Adsr {
        attack_s: 0.01,
        hold_s: 0.0,
        decay_s: 0.05,
        sustain_db: 0.0,
        release_s: 0.05,
    };
    let notes: Vec<(f64, f64, f64, f64)> = (0..4)
        .map(|i| {
            let on = 4.0 * beat() * f64::from(i) + 0.05;
            (on, on + 3.5 * beat(), 110.0, 0.0)
        })
        .collect();
    let seconds = 16.0 * beat() + 0.5;
    let duck = Duck {
        depth_db: 12.0,
        release_s: 0.15,
        bpm: BPM,
    };
    let x = duck.apply(&adsr_notes(&notes, adsr, fs(), seconds), fs());
    let list = note_list(
        &notes
            .iter()
            .map(|n| (n.0, n.1 + 0.09, n.2))
            .collect::<Vec<_>>(),
    );
    let read = layer(&x, &list, BPM);
    assert!(read.flags.pump_stripped, "{:?}", read.pump);
    let sustain = read.sustain.value().unwrap().median().get();
    assert!(sustain > -3.0, "{sustain}");
    // Unstripped, the duck swings the sustain window by 6.5 dB.
    let ripple = read.ripple.unwrap().get();
    assert!(ripple < 1.0, "{ripple}");
    // A 2 dB duck stays: under the 3 dB floor, nothing is stripped.
    let light = Duck {
        depth_db: 2.0,
        release_s: 0.15,
        bpm: BPM,
    };
    let x = light.apply(&adsr_notes(&notes, adsr, fs(), seconds), fs());
    assert!(!layer(&x, &list, BPM).flags.pump_stripped);
}

/// One saw note under an ADSR, 0.2 s in, held 0.8 s past its attack.
fn one_note(f0: f64, adsr: Adsr) -> EnvelopeLayer {
    let off = 0.2 + adsr.attack_s + 0.8;
    let x = adsr_notes(&[(0.2, off, f0, 0.0)], adsr, fs(), off + 0.5);
    layer(
        &x,
        &note_list(&[(0.2, off + adsr.release_s + 0.04, f0)]),
        120.0,
    )
}

#[test]
fn the_attack_ladder_reads_long_attacks_and_bounds_short_ones() {
    // #69: above 4 periods the 10-90 % rise within 15 %; under 1.5
    // periods an upper bound.
    for f0 in [55.0, 220.0] {
        for attack_s in [0.001, 0.005, 0.02, 0.1] {
            let read = one_note(
                f0,
                Adsr {
                    attack_s,
                    hold_s: 0.0,
                    decay_s: 0.2,
                    sustain_db: -6.0,
                    release_s: 0.05,
                },
            );
            let note = &read.notes[0];
            let periods = attack_s * f0;
            if periods > 4.0 {
                let rise = note.attack.unwrap().get() / 1.25;
                assert!(
                    (rise / (0.8 * attack_s) - 1.0).abs() <= 0.15,
                    "{f0} Hz {attack_s}: {rise}"
                );
                assert!(!note.attack_upper_bound);
            } else if periods < 1.5 {
                assert!(note.attack_upper_bound, "{f0} Hz {attack_s}: {note:?}");
            }
        }
    }
}

#[test]
fn the_attack_reads_the_numpy_rise_of_the_ladder() {
    // tests/fixtures/golden/prototype/69-attack.json: the 10-90 % rise of
    // the centred RMS over one period in NumPy, on the same notes.
    let golden = Golden::load("prototype/69-attack");
    let tolerance = golden.tolerance().abs;
    for row in golden.values()["rows"].as_array().unwrap() {
        let read = one_note(
            float(&row["f0_hz"]),
            Adsr {
                attack_s: float(&row["attack_s"]),
                hold_s: 0.0,
                decay_s: 0.2,
                sustain_db: -6.0,
                release_s: 0.05,
            },
        );
        let rise = read.notes[0].attack.unwrap().get() / 1.25;
        let want = float(&row["rise_s"]);
        assert!((rise - want).abs() <= tolerance, "{row}: {rise}");
    }
}

#[test]
fn a_hold_reads_from_10_ms_and_a_short_note_hides_its_decay() {
    let adsr = |hold_s: f64, decay_s: f64| Adsr {
        attack_s: 0.02,
        hold_s,
        decay_s,
        sustain_db: -6.0,
        release_s: 0.05,
    };
    let read = one_note(220.0, adsr(0.03, 0.1)).notes[0].hold.get();
    assert!((read - 0.03).abs() <= 0.003, "{read}");
    assert_eq!(one_note(220.0, adsr(0.005, 0.1)).notes[0].hold.get(), 0.0);
    // Held 0.8 s past the attack, under three decays of 0.4 s: the knee
    // comes before the sustain, so neither decay nor sustain is read.
    let short = &one_note(220.0, adsr(0.0, 0.4)).notes[0];
    assert!(
        short.knee.is_some() && short.decay.is_none() && short.sustain.is_none(),
        "{short:?}"
    );
}

#[test]
fn a_note_past_the_stem_end_is_read_to_the_end() {
    let adsr = Adsr {
        attack_s: 0.01,
        hold_s: 0.0,
        decay_s: 0.1,
        sustain_db: -6.0,
        release_s: 0.05,
    };
    let x = adsr_notes(&[(0.2, 1.0, 110.0, 0.0)], adsr, fs(), 1.0);
    let notes = note_list(&[(0.2, 3.0, 110.0)]);
    let proxy = amp_proxy(&x, rate(), &notes.notes()[0], &[])
        .unwrap()
        .unwrap();
    assert!((proxy.time(0) - 0.17).abs() < 1e-9, "{}", proxy.time(0));
    assert!(proxy.time(proxy.db().len() - 1) <= 1.0 + 1e-9);
    let read = layer(&x, &notes, 120.0);
    assert!(
        (read.notes[0].end.get() - 1.0).abs() < 1e-6,
        "{:?}",
        read.notes[0]
    );
}

#[test]
fn an_overlapping_note_bounds_a_fast_attack_by_its_band() {
    // A fifth: the band of each note rises in 0.98 s over its cutoff, so a
    // 5 ms attack is a bound and a 100 ms one is read.
    let notes = |attack_s: f64| {
        let adsr = Adsr {
            attack_s,
            hold_s: 0.0,
            decay_s: 0.1,
            sustain_db: -6.0,
            release_s: 0.05,
        };
        let x = adsr_notes(
            &[(0.2, 1.6, 110.0, 0.0), (0.2, 1.6, 164.81, 0.0)],
            adsr,
            fs(),
            2.0,
        );
        layer(
            &x,
            &note_list(&[(0.2, 1.69, 110.0), (0.2, 1.69, 164.81)]),
            120.0,
        )
    };
    assert!(
        notes(0.005)
            .notes
            .iter()
            .all(|n| n.overlap && n.attack_upper_bound)
    );
    let slow = notes(0.1);
    assert!(
        slow.notes.iter().all(|n| !n.attack_upper_bound),
        "{:?}",
        slow.notes
    );
}

#[test]
fn a_note_whose_band_would_reach_the_nyquist_frequency_is_skipped() {
    // 10.99 kHz at 22.05 kHz: its band of 40 Hz either side would pass
    // 11.025 kHz, so no harmonic is usable.
    let x: Vec<f64> = (0..samples(1.0, fs()))
        .map(|n| 0.3 * (TAU * 10_990.0 * f64::from(u32::try_from(n).unwrap()) / fs()).sin())
        .collect();
    let read = layer(&x, &note_list(&[(0.2, 0.8, 10_990.0)]), 120.0);
    assert_eq!(read.counts.read, 0);
    assert_eq!(read.counts.no_free_harmonic, 1);
    // 440 Hz keeps its band below the Nyquist frequency.
    let x: Vec<f64> = (0..samples(1.0, fs()))
        .map(|n| 0.3 * (TAU * 440.0 * f64::from(u32::try_from(n).unwrap()) / fs()).sin())
        .collect();
    assert_eq!(
        layer(&x, &note_list(&[(0.2, 0.8, 440.0)]), 120.0)
            .counts
            .read,
        1
    );
}

#[test]
fn notes_that_only_touch_do_not_overlap() {
    // A2 ends where E3 starts: each reads alone, on its sliding DFT.
    let adsr = Adsr {
        attack_s: 0.01,
        hold_s: 0.0,
        decay_s: 0.1,
        sustain_db: -6.0,
        release_s: 0.05,
    };
    let x = adsr_notes(
        &[(0.2, 1.0, 110.0, 0.0), (1.0, 1.8, 164.81, 0.0)],
        adsr,
        fs(),
        2.2,
    );
    let read = layer(
        &x,
        &note_list(&[(0.2, 1.0, 110.0), (1.0, 1.8, 164.81)]),
        120.0,
    );
    assert!(read.notes.iter().all(|n| !n.overlap), "{:?}", read.notes);
}

#[test]
fn the_fundamental_reads_the_fall_under_a_closing_filter() {
    // #69: a 110 Hz saw falls 20 dB, linear in dB, over 1.15 s from 0.2 s,
    // under a two-pole low-pass that sweeps from 4 kHz to the fundamental.
    let gain = |t: f64| {
        if t < 0.19 {
            t / 0.19
        } else {
            10f64.powf(-((t - 0.2) / 1.15).clamp(0.0, 1.5))
        }
    };
    let cutoff = |t: f64| 4000.0 * (110.0f64 / 4000.0).powf(((t - 0.2) / 1.15).clamp(0.0, 1.0));
    let x = swept_saw(110.0, fs(), 2.0, gain, cutoff);
    let notes = note_list(&[(0.19, 1.9, 110.0)]);
    let proxy = amp_proxy(&x, rate(), &notes.notes()[0], &[])
        .unwrap()
        .unwrap();
    let db = proxy.db();
    let peak = db.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let top = db.iter().position(|&d| d >= peak - 0.5).unwrap();
    let fall = db.iter().position(|&d| d <= peak - 20.0).unwrap();
    let time = proxy.time(fall) - proxy.time(top);
    assert!((time / 1.15 - 1.0).abs() <= 0.1, "{time}");
}

#[test]
fn each_note_of_a_fifth_reads_its_own_envelope() {
    // #69: A2 and E3 overlap, each with its own envelope; each amp proxy
    // follows its note within 0.5 dB RMS and a correlation of 0.99.
    let a = Adsr {
        attack_s: 0.15,
        hold_s: 0.0,
        decay_s: 0.4,
        sustain_db: -8.0,
        release_s: 0.1,
    };
    let b = Adsr {
        attack_s: 0.01,
        hold_s: 0.05,
        decay_s: 0.2,
        sustain_db: -12.0,
        release_s: 0.1,
    };
    let xa = adsr_notes(&[(0.2, 1.4, 110.0, 0.0)], a, fs(), 2.0);
    let xb = adsr_notes(&[(0.5, 1.6, 164.81, -3.0)], b, fs(), 2.0);
    let mix: Vec<f64> = xa.iter().zip(&xb).map(|(p, q)| p + q).collect();
    let list = note_list(&[(0.2, 1.4, 110.0), (0.5, 1.6, 164.81)]);
    let [lower, upper] = [&list.notes()[0], &list.notes()[1]];
    for (note, other, adsr, on, off) in [(lower, upper, a, 0.2, 1.4), (upper, lower, b, 0.5, 1.6)] {
        let proxy = amp_proxy(&mix, rate(), note, &[other]).unwrap().unwrap();
        // Compare where the note is above -30 dB of its own peak, after its
        // attack, so the band's smear of the onset stays out.
        let pairs: Vec<(f64, f64)> = (0..proxy.db().len())
            .filter_map(|i| {
                let t = proxy.time(i);
                let g = adsr.gain(t - on, off - on);
                (t > on + adsr.attack_s + 0.03 && t < off && g > 0.03)
                    .then(|| (proxy.db()[i], 20.0 * g.log10()))
            })
            .collect();
        let n = f64::from(u32::try_from(pairs.len()).unwrap());
        let (mx, my) = (
            pairs.iter().map(|p| p.0).sum::<f64>() / n,
            pairs.iter().map(|p| p.1).sum::<f64>() / n,
        );
        let cov: f64 = pairs.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
        let (vx, vy): (f64, f64) = (
            pairs.iter().map(|p| (p.0 - mx).powi(2)).sum(),
            pairs.iter().map(|p| (p.1 - my).powi(2)).sum(),
        );
        let corr = cov / (vx * vy).sqrt();
        let rms = (pairs
            .iter()
            .map(|p| (p.0 - mx - (p.1 - my)).powi(2))
            .sum::<f64>()
            / n)
            .sqrt();
        assert!(corr >= 0.99 && rms <= 0.5, "{on}: corr {corr}, rms {rms}");
    }
}

/// Eight saw notes of one line under one ADSR, 1.2 s each.
fn eight_notes(adsr: Adsr, decays: Option<[f64; 8]>) -> EnvelopeLayer {
    let pitches = [110.0, 123.47, 130.81, 146.83];
    let mut x = vec![0.0; samples(14.0, fs())];
    let mut list = Vec::new();
    for i in 0..8 {
        let on = 0.2 + 1.6 * f64::from(u32::try_from(i).unwrap());
        let adsr = Adsr {
            decay_s: decays.map_or(adsr.decay_s, |d| d[i]),
            ..adsr
        };
        let note = adsr_notes(&[(on, on + 1.2, pitches[i % 4], 0.0)], adsr, fs(), 14.0);
        for (v, n) in x.iter_mut().zip(note) {
            *v += n;
        }
        list.push((on, on + 1.2 + adsr.release_s + 0.04, pitches[i % 4]));
    }
    layer(&x, &note_list(&list), 120.0)
}

#[test]
fn eight_notes_pool_their_decay_and_sustain() {
    // #69: a true DEC of 300 ms and SUS of -6 dB.
    let read = eight_notes(
        Adsr {
            attack_s: 0.01,
            hold_s: 0.0,
            decay_s: 0.3,
            sustain_db: -6.0,
            release_s: 0.05,
        },
        None,
    );
    let decay = read.decay.value().unwrap();
    assert!((decay.median().get() / 0.3 - 1.0).abs() <= 0.2, "{decay:?}");
    let sustain = read.sustain.value().unwrap();
    assert!((sustain.median().get() + 6.0).abs() <= 2.0, "{sustain:?}");
    assert!(decay.iqr().get() >= 0.0);
    assert_eq!(read.decay.confidence().level(), Some(Level::Medium));
    assert_eq!(read.pooled.iter().filter(|&&p| p).count(), 8);
    assert!(
        !read.summary()[0].contains("skipped"),
        "{:?}",
        read.summary()
    );
    // Each note ends in the list 50 ms of release and 40 ms after its
    // note-off.
    let offset = read.note_off_offset.value().unwrap().median().get();
    assert!((offset - 0.09).abs() <= 0.015, "{offset}");
    // The knee of each note within 15 ms of its note-off.
    for (i, note) in read.notes.iter().enumerate() {
        let off = 0.2 + 1.6 * f64::from(u32::try_from(i).unwrap()) + 1.2;
        let knee = note.knee.unwrap().get();
        assert!((knee - off).abs() <= 0.015, "{i}: {knee}");
    }
}

#[test]
fn a_sustain_just_under_the_peak_prints_as_0_db() {
    // The sustain is read from the peak, so a flat note reads a few
    // hundredths of a dB under 0, which rounds to 0.0 without a sign.
    let read = eight_notes(
        Adsr {
            attack_s: 0.01,
            hold_s: 0.0,
            decay_s: 0.05,
            sustain_db: 0.0,
            release_s: 0.05,
        },
        None,
    );
    let sustain = read.sustain.value().unwrap().median().get();
    assert!((-0.05..0.0).contains(&sustain), "{sustain}");
    let summary = read.summary();
    assert!(summary[1].ends_with("sustain 0.0 dB"), "{summary:?}");
    // Within 1 dB of the peak there is no decay to read.
    assert!(
        read.notes.iter().all(|n| n.decay.is_none()),
        "{:?}",
        read.notes
    );
}

#[test]
fn the_sustain_is_read_up_to_the_knee_not_the_note_end() {
    // #69 and #70: a 300 ms linear release lies inside each note's span in
    // the list. Read up to the knee, the sustain stays at -6 dB; read to
    // the note's end, the release would pull it low.
    let read = eight_notes(
        Adsr {
            attack_s: 0.01,
            hold_s: 0.0,
            decay_s: 0.1,
            sustain_db: -6.0,
            release_s: 0.3,
        },
        None,
    );
    assert_eq!(read.counts.knees, 8, "{:?}", read.notes);
    let sustain = read.sustain.value().unwrap();
    assert!((sustain.median().get() + 6.0).abs() <= 1.0, "{sustain:?}");
    assert_eq!(read.sustain.confidence().level(), Some(Level::Medium));
}

#[test]
fn the_upper_bound_flag_needs_more_than_half_the_pooled_attacks() {
    // Eight A1 notes, 1.6 s apart: a 1 ms attack is a bound, 100 ms is not.
    let read = |attacks: &[f64]| {
        let mut x = vec![0.0; samples(14.0, fs())];
        let mut list = Vec::new();
        for (i, &attack_s) in attacks.iter().enumerate() {
            let on = 0.2 + 1.6 * f64::from(u32::try_from(i).unwrap());
            let adsr = Adsr {
                attack_s,
                hold_s: 0.0,
                decay_s: 0.2,
                sustain_db: -6.0,
                release_s: 0.05,
            };
            let note = adsr_notes(&[(on, on + 1.2, 55.0, 0.0)], adsr, fs(), 14.0);
            for (v, n) in x.iter_mut().zip(note) {
                *v += n;
            }
            list.push((on, on + 1.29, 55.0));
        }
        layer(&x, &note_list(&list), 120.0)
    };
    assert!(read(&[0.001; 8]).flags.attack_is_upper_bound);
    let half = read(&[0.001, 0.1, 0.001, 0.1, 0.001, 0.1, 0.001, 0.1]);
    assert_eq!(
        half.notes.iter().filter(|n| n.attack_upper_bound).count(),
        4
    );
    assert!(!half.flags.attack_is_upper_bound);
}

#[test]
fn the_overlap_fraction_counts_the_notes_that_overlap_another() {
    // A fifth held together, then a note alone: two of three overlap.
    let adsr = Adsr {
        attack_s: 0.01,
        hold_s: 0.0,
        decay_s: 0.1,
        sustain_db: -6.0,
        release_s: 0.05,
    };
    let x = adsr_notes(
        &[
            (0.2, 1.2, 110.0, 0.0),
            (0.2, 1.2, 164.81, 0.0),
            (1.6, 2.6, 110.0, 0.0),
        ],
        adsr,
        fs(),
        3.0,
    );
    let notes = note_list(&[(0.2, 1.29, 110.0), (0.2, 1.29, 164.81), (1.6, 2.69, 110.0)]);
    let read = layer(&x, &notes, 120.0);
    assert!(
        (read.overlap_fraction - 2.0 / 3.0).abs() < 1e-12,
        "{}",
        read.overlap_fraction
    );
}

#[test]
fn the_summary_counts_the_skipped_notes_in_its_total() {
    // A2 under A3, which has no free harmonic, and a 10 ms note.
    let adsr = Adsr {
        attack_s: 0.01,
        hold_s: 0.0,
        decay_s: 0.1,
        sustain_db: -6.0,
        release_s: 0.05,
    };
    let x = adsr_notes(
        &[
            (0.2, 1.2, 110.0, 0.0),
            (0.2, 1.2, 220.0, 0.0),
            (1.6, 1.61, 110.0, 0.0),
        ],
        adsr,
        fs(),
        2.0,
    );
    let notes = note_list(&[(0.2, 1.29, 110.0), (0.2, 1.29, 220.0), (1.6, 1.61, 110.0)]);
    let read = layer(&x, &notes, 120.0);
    assert!(
        read.summary()[0].starts_with("envelope: 1 of 3 notes read"),
        "{:?}",
        read.summary()
    );
    assert!(read.summary()[0].ends_with("; skipped 1 no free harmonic, 1 too short"));
}

#[test]
fn notes_whose_decays_disagree_read_low() {
    // Decays of 50 ms to 1 s: an interquartile range above half the median.
    let read = eight_notes(
        Adsr {
            attack_s: 0.01,
            hold_s: 0.0,
            decay_s: 0.3,
            sustain_db: -12.0,
            release_s: 0.05,
        },
        Some([0.05, 1.0, 0.08, 0.6, 0.05, 1.0, 0.1, 0.5]),
    );
    assert_eq!(
        read.decay.confidence().level(),
        Some(Level::Low),
        "{:?}",
        read.decay
    );
}

#[test]
fn two_usable_notes_read_low() {
    let adsr = Adsr {
        attack_s: 0.01,
        hold_s: 0.0,
        decay_s: 0.2,
        sustain_db: -6.0,
        release_s: 0.05,
    };
    let x = adsr_notes(
        &[(0.2, 1.2, 110.0, 0.0), (1.6, 2.6, 110.0, 0.0)],
        adsr,
        fs(),
        3.0,
    );
    let read = layer(
        &x,
        &note_list(&[(0.2, 1.29, 110.0), (1.6, 2.69, 110.0)]),
        120.0,
    );
    assert_eq!(read.sustain.confidence().level(), Some(Level::Low));
    assert!(read.level_spread.value().is_none());
    assert_eq!(read.confidence.level(), Some(Level::Low));
    // A third note brings the group to medium.
    let x = adsr_notes(
        &[
            (0.2, 1.2, 110.0, 0.0),
            (1.6, 2.6, 110.0, 0.0),
            (3.0, 4.0, 110.0, 0.0),
        ],
        adsr,
        fs(),
        4.4,
    );
    let notes = note_list(&[(0.2, 1.29, 110.0), (1.6, 2.69, 110.0), (3.0, 4.09, 110.0)]);
    assert_eq!(
        layer(&x, &notes, 120.0).confidence.level(),
        Some(Level::Medium)
    );
}

#[test]
fn the_level_spread_reads_the_gap_between_peaks() {
    let adsr = Adsr {
        attack_s: 0.01,
        hold_s: 0.0,
        decay_s: 0.2,
        sustain_db: -6.0,
        release_s: 0.05,
    };
    for spread in [0.0, 6.0] {
        let notes: Vec<(f64, f64, f64, f64)> = (0..8)
            .map(|i| {
                let on = 0.2 + 1.2 * f64::from(i);
                (on, on + 0.9, 110.0, -spread * f64::from(i % 2))
            })
            .collect();
        let x = adsr_notes(&notes, adsr, fs(), 10.0);
        let list = note_list(
            &notes
                .iter()
                .map(|n| (n.0, n.1 + 0.09, n.2))
                .collect::<Vec<_>>(),
        );
        let read = layer(&x, &list, 120.0);
        let value = read.level_spread.value().unwrap().get();
        assert!((value - spread).abs() <= 0.5, "{spread}: {value}");
        assert_eq!(read.level_spread.confidence().level(), Some(Level::Low));
    }
}

#[test]
fn a_decaying_pluck_has_no_knee_and_sustains_at_its_end_level() {
    let (x, notes) = plucks(110.0, fs(), 6.0, 1.2, 0.2);
    let read = layer(&x, &notes, 120.0);
    assert!(read.notes.iter().all(|n| n.knee.is_none()));
    let sustain = read.sustain.value().unwrap().median().get();
    assert!(sustain < -30.0, "{sustain}");
    assert!(read.note_off_offset.value().is_none());
}

#[test]
fn a_stem_without_notes_has_no_envelope() {
    let read = layer(&vec![0.0; 22_050], &NoteList::default(), 120.0);
    assert!(read.flags.no_notes);
    assert!(!read.confidence.is_estimate());
    assert!(read.pump.is_none());
}

/// The chord held over `beats` beats, as in the pump table.
fn held_chord_over(beats: f64) -> (Vec<f64>, NoteList) {
    let seconds = beats * beat();
    (
        chord(&CHORD, fs(), seconds, 1),
        note_list(&CHORD.map(|f| (0.0, seconds, f))),
    )
}

/// `x` under a duck that starts `phase` of its beat into the stem.
fn ducked(x: &[f64], duck: Duck, phase: f64) -> Vec<f64> {
    let shift = phase * 60.0 / duck.bpm;
    x.iter()
        .enumerate()
        .map(|(n, v)| v * duck.gain(f64::from(u32::try_from(n).unwrap()) / fs() + shift))
        .collect()
}

/// `x` under a gain in dB.
fn under(x: &[f64], gain_db: impl Fn(f64) -> f64) -> Vec<f64> {
    x.iter()
        .enumerate()
        .map(|(n, v)| v * 10f64.powf(gain_db(f64::from(u32::try_from(n).unwrap()) / fs()) / 20.0))
        .collect()
}

#[test]
fn a_duck_off_the_given_tempo_is_found_and_stripped() {
    // The sidechain follows the song's tempo, and a stem's tempo may be off
    // by 1 or 2 %. Over 32 beats a 1 % error moves the duck a third of a
    // beat: read on the grid alone, none of these locked to the beat.
    let (x, notes) = held_chord_over(32.0);
    for off in [-0.02, -0.01, 0.01, 0.02] {
        for (depth_db, release_s) in [(6.0, 0.15), (12.0, 0.08)] {
            let duck = Duck {
                depth_db,
                release_s,
                bpm: BPM * (1.0 + off),
            };
            let p = pump(&duck.apply(&x, fs()), &notes, CHORD[0]).unwrap();
            assert_eq!(p.lock, PumpLock::Beat, "{off} {depth_db}: {p:?}");
            assert!(p.strips());
            let depth = p.depth.get();
            assert!(
                depth <= depth_db && depth >= depth_db - 5.0,
                "{off} {depth_db}: {depth}"
            );
            let bpm = p.bpm().get() / (BPM * (1.0 + off));
            assert!((bpm - 1.0).abs() <= 0.001, "{off} {depth_db}: {p:?}");
        }
    }
}

#[test]
fn a_level_that_follows_the_bar_is_not_stripped() {
    // The level rises over each bar, or each two beats, and drops back. A
    // beat's own curve then starts low and wraps into a dip at its start,
    // and these locked to the beat and stripped. The beat before ends
    // where the beat starts on all but the first beat of each bar or pair.
    let (x, notes) = held_chord_over(32.0);
    for (every, swing_db) in [(4.0, 12.0), (2.0, 6.0), (2.0, 12.0)] {
        let span = every * beat();
        let y = under(&x, |t| -swing_db * (1.0 - (t / span).fract()));
        let p = pump(&y, &notes, CHORD[0]).unwrap();
        assert_ne!(p.lock, PumpLock::Beat, "{every} {swing_db}: {p:?}");
    }
}

#[test]
fn dips_near_most_beats_at_random_depths_are_not_stripped() {
    // On about 70 % of 64 beats a dip of 2 to 8 dB, 40 to 120 ms wide,
    // from 30 ms before to 60 ms after the beat: a player's articulation.
    // At one seed the period search lined these up into a 3.2 dB
    // curve that falls into its dip; its odd beats alone do not.
    let (x, notes) = held_chord_over(64.0);
    for seed in 1..=6 {
        let u = uniform(seed + 50, 4 * 64);
        let dips: Vec<(f64, f64, f64)> = (0..64)
            .filter(|&b| u[4 * b] < 0.7)
            .map(|b| {
                let at = f64::from(u32::try_from(b).unwrap()) * beat() - 0.03 + 0.09 * u[4 * b + 1];
                (at, 2.0 + 6.0 * u[4 * b + 2], 0.04 + 0.08 * u[4 * b + 3])
            })
            .collect();
        let y = under(&x, |t| {
            -dips
                .iter()
                .map(|&(at, depth, width)| depth * (-((t - at) / (0.5 * width)).powi(2)).exp())
                .sum::<f64>()
        });
        let p = pump(&y, &notes, CHORD[0]).unwrap();
        assert!(!p.strips(), "{seed}: {p:?}");
    }
}

/// A gain in dB over the time since a note's onset.
type GainDb = fn(f64) -> f64;

/// A saw note of 1.5 s from 0.2 s under a linear attack, a 50 ms release
/// and a gain over the note's time.
fn swelling_note(rate: u32, f0: f64, attack_s: f64, gain_db: GainDb) -> EnvelopeLayer {
    let fs = f64::from(rate);
    let (on, length) = (0.2, 1.5);
    let n = samples(on + length + 0.3, fs);
    let tone = saw(f0, fs, n, 0.0);
    let x: Vec<f64> = (0..n)
        .map(|i| {
            let t = f64::from(u32::try_from(i).unwrap()) / fs - on;
            if !(0.0..=length).contains(&t) {
                return 0.0;
            }
            let ramp = (t / attack_s).min(1.0) * ((length - t) / 0.05).min(1.0);
            0.3 * tone[i] * ramp * 10f64.powf(gain_db(t) / 20.0)
        })
        .collect();
    EnvelopeLayer::of(&EnvelopeInput {
        mono: &x,
        rate: SampleRate::new(rate).unwrap(),
        notes: &note_list(&[(on, on + length, f0)]),
        grid: grid(120.0),
    })
    .unwrap()
}

#[test]
fn an_attack_that_swells_or_ripples_on_reads_its_fast_rise() {
    // A 50 ms attack, then a swell of 2 or 3 dB over 600 ms or a ripple of
    // 4 or 6 dB at 5 Hz. The note's peak lies late in the swell, and the
    // rise to 90 % of it read 400 to 514 ms; the attack's top is where the
    // fast rise ends.
    for rate in [22_050, 44_100] {
        for f0 in [110.0, 261.63] {
            let swells: [(&str, GainDb); 4] = [
                ("swell 2", |t: f64| -2.0 + 2.0 * (t / 0.6).min(1.0)),
                ("swell 3", |t: f64| -3.0 + 3.0 * (t / 0.6).min(1.0)),
                ("ripple 4", |t: f64| 2.0 * (TAU * 5.0 * t).sin()),
                ("ripple 6", |t: f64| 3.0 * (TAU * 5.0 * t).sin()),
            ];
            for (name, gain_db) in swells {
                let read = swelling_note(rate, f0, 0.05, gain_db);
                let rise = read.notes[0].attack.unwrap().get() / 1.25;
                assert!(
                    (rise / 0.04 - 1.0).abs() <= 0.15,
                    "{rate} {f0} {name}: {rise}"
                );
            }
        }
    }
}

#[test]
fn a_steep_swell_hides_the_top_of_a_slow_attack() {
    // A 300 ms attack under a swell over 0.8 s: up to 2.9 dB the attack has
    // a top, and from 3.1 dB the level still rises 1 dB past every frame.
    let attack = |gain: GainDb| {
        swelling_note(48_000, 110.0, 0.3, gain).notes[0]
            .attack
            .unwrap()
            .get()
    };
    let below = attack(|t: f64| -2.9 + 2.9 * (t / 0.8).min(1.0));
    let above = attack(|t: f64| -3.1 + 3.1 * (t / 0.8).min(1.0));
    assert!((0.32..=0.37).contains(&below), "{below}");
    assert!((0.64..=0.71).contains(&above), "{above}");
}

/// A held A2 and an E3 on every other beat whose onset the list puts
/// `early` seconds before the beat; the mix dips 9 dB at each E3 onset and
/// comes back in 40 ms.
fn upper_note_on_the_beat(early: f64) -> Option<Pump> {
    let seconds = 16.0 * beat();
    let length = samples(seconds, fs());
    let held = saw(110.0, fs(), length, 0.0);
    let upper = saw(164.81, fs(), length, 0.3);
    let onsets: Vec<f64> = (1..8).map(|k| 2.0 * f64::from(k) * beat()).collect();
    let mut notes = vec![(0.0, seconds, 110.0)];
    for &on in &onsets {
        notes.push((on - early, on + 0.6 * beat(), 164.81));
    }
    let x: Vec<f64> = (0..length)
        .map(|i| {
            let t = f64::from(u32::try_from(i).unwrap()) / fs();
            let since = onsets
                .iter()
                .filter(|&&on| on <= t)
                .map(|&on| t - on)
                .fold(f64::INFINITY, f64::min);
            let sounding = since < 0.6 * beat();
            let dip = 1.0 - (1.0 - 10f64.powf(-9.0 / 20.0)) * (-since / 0.04).exp();
            0.3 * dip * (held[i] + if sounding { upper[i] } else { 0.0 })
        })
        .collect();
    pump(&x, &note_list(&notes), 110.0)
}

#[test]
fn a_note_struck_on_the_beat_or_listed_early_locks_to_the_note() {
    // At 128 BPM an onset on the beat starts that beat, and one the list
    // puts 20 ms early still does: the lead before a beat belongs to it.
    for early in [0.0, 0.02] {
        let p = upper_note_on_the_beat(early).unwrap();
        assert_eq!(p.lock, PumpLock::Note, "{early}: {p:?}");
        assert!(!p.strips());
    }
}

#[test]
fn a_duck_whose_first_read_counts_too_few_beats_is_counted_over_the_notes() {
    // A held saw under a 12 dB duck falls 20 dB over 100 ms after 2.2
    // beats. The first read ends the held part at the fall, too few beats
    // for a pump; counted again over the note to its end, 14 beats strip.
    let seconds = 16.0 * beat();
    let duck = Duck {
        depth_db: 12.0,
        release_s: 0.15,
        bpm: BPM,
    };
    let (step, low) = (2.2 * beat(), 10f64.powf(-20.0 / 20.0));
    let x: Vec<f64> = duck
        .apply(&chord(&[110.0], fs(), seconds, 1), fs())
        .iter()
        .enumerate()
        .map(|(n, v)| {
            let t = f64::from(u32::try_from(n).unwrap()) / fs();
            v * (1.0 - (1.0 - low) * ((t - step) / 0.1).clamp(0.0, 1.0))
        })
        .collect();
    let read = layer(&x, &note_list(&[(0.0, seconds, 110.0)]), BPM);
    let p = read.pump.as_ref().unwrap();
    assert!(read.flags.pump_stripped, "{p:?}");
    assert_eq!(p.beats, 14, "{p:?}");
}

#[test]
fn a_20_ms_release_ending_30_ms_before_the_note_end_reads_its_knee() {
    // The search stops 25 ms before the note's end, 5 ms after the release
    // ends; at 50 ms it would stop 20 ms before, at the release's start.
    let adsr = Adsr {
        attack_s: 0.01,
        hold_s: 0.0,
        decay_s: 0.1,
        sustain_db: -6.0,
        release_s: 0.02,
    };
    let off = 1.2;
    let x = adsr_notes(&[(0.2, off, 110.0, 0.0)], adsr, fs(), off + 0.5);
    let read = layer(&x, &note_list(&[(0.2, off + 0.02 + 0.03, 110.0)]), 120.0);
    let knee = read.notes[0].knee.unwrap().get();
    assert!((knee - off).abs() <= 0.005, "{knee}");
}

#[test]
fn a_quiet_second_voice_stays_out_of_the_pool() {
    // Eight A2 notes decay over 300 ms to -6 dB; under each an E3 18 dB
    // quieter decays over 50 ms to -20 dB. Pooled, the E3 notes would put
    // the median decay between the two.
    let loud = Adsr {
        attack_s: 0.01,
        hold_s: 0.0,
        decay_s: 0.3,
        sustain_db: -6.0,
        release_s: 0.05,
    };
    let quiet = Adsr {
        decay_s: 0.05,
        sustain_db: -20.0,
        ..loud
    };
    let mut x = vec![0.0; samples(14.0, fs())];
    let mut list = Vec::new();
    for i in 0..8 {
        let on = 0.2 + 1.6 * f64::from(u32::try_from(i).unwrap());
        for (f0, adsr, level) in [(110.0, loud, 0.0), (164.81, quiet, -18.0)] {
            let note = adsr_notes(&[(on, on + 1.2, f0, level)], adsr, fs(), 14.0);
            for (v, n) in x.iter_mut().zip(note) {
                *v += n;
            }
            list.push((on, on + 1.2 + adsr.release_s + 0.04, f0));
        }
    }
    let read = layer(&x, &note_list(&list), 120.0);
    let pooled: Vec<bool> = (0..16).map(|i| i % 2 == 0).collect();
    assert_eq!(read.pooled, pooled, "{:?}", read.notes);
    let decay = read.decay.value().unwrap().median().get();
    assert!((decay / 0.3 - 1.0).abs() <= 0.2, "{decay}");
}

#[test]
fn a_note_outside_the_ducked_section_keeps_its_own_level() {
    // A chord under a 12 dB duck for 20 beats, then 12 beats without, one
    // note each per pitch. The pump locks over both, and the second part
    // takes no strip: the curve would write its ripple into the held level.
    let split = 20.0 * beat();
    let seconds = 32.0 * beat();
    let duck = Duck {
        depth_db: 12.0,
        release_s: 0.15,
        bpm: BPM,
    };
    let x: Vec<f64> = chord(&CHORD, fs(), seconds, 1)
        .iter()
        .enumerate()
        .map(|(n, v)| {
            let t = f64::from(u32::try_from(n).unwrap()) / fs();
            if t < split { v * duck.gain(t) } else { *v }
        })
        .collect();
    let notes: Vec<(f64, f64, f64)> = CHORD
        .iter()
        .flat_map(|&f| [(0.0, split, f), (split, seconds, f)])
        .collect();
    let read = layer(&x, &note_list(&notes), BPM);
    let p = read.pump.as_ref().unwrap();
    assert!(read.flags.pump_stripped, "{p:?}");
    assert!(p.carries(0.0, split));
    assert!(!p.carries(split, seconds));
    // The chord's own beating, about 2 dB; the stripped curve would add
    // its whole depth.
    let clean = read.notes[read.notes.len() - 3..]
        .iter()
        .find_map(|n| n.ripple)
        .unwrap()
        .get();
    assert!(clean < 0.5 * p.depth.get(), "{clean}: {p:?}");
}

#[test]
fn notes_over_a_silent_stem_are_skipped() {
    let x = vec![0.0; samples(4.0, fs())];
    let list: Vec<(f64, f64, f64)> = (0..4)
        .map(|i| (0.5 + 0.8 * f64::from(i), 1.0 + 0.8 * f64::from(i), 110.0))
        .collect();
    let read = layer(&x, &note_list(&list), BPM);
    assert_eq!(read.counts.silent, 4);
    assert_eq!(read.counts.read, 0);
    assert!(
        read.summary()[0].contains("0 of 4 notes read"),
        "{:?}",
        read.summary()
    );
    assert!(!read.confidence.is_estimate());
}

#[test]
fn a_duck_off_the_given_tempo_folds_over_two_runs() {
    // The same duck, 1.5 % off the grid, under two notes with a gap: only
    // a period right over the whole span folds both runs onto one phase,
    // which the search over the longest run alone does not reach.
    let (x, _) = held_chord_over(48.0);
    let (split, resume) = (20.0 * beat(), 28.0 * beat());
    let notes: Vec<(f64, f64, f64)> = CHORD
        .iter()
        .flat_map(|&f| [(0.0, split, f), (resume, 48.0 * beat(), f)])
        .collect();
    let duck = Duck {
        depth_db: 12.0,
        release_s: 0.08,
        bpm: BPM * 1.015,
    };
    let p = pump(&duck.apply(&x, fs()), &note_list(&notes), CHORD[0]).unwrap();
    assert_eq!(p.lock, PumpLock::Beat, "{p:?}");
    assert!(p.depth.get() >= 7.0, "{p:?}");
}

#[test]
fn a_clean_start_before_a_level_step_is_not_counted_again() {
    // A saw held clean for 6.2 beats falls 20 dB over 100 ms, then ducks. The
    // first read ends at the fall and counts 5 flat beats, a read with no
    // lock. Only no pump or a short beat lock is counted again, so nothing
    // is stripped, though the count over the note would lock to the beat.
    let seconds = 26.0 * beat();
    let duck = Duck {
        depth_db: 12.0,
        release_s: 0.15,
        bpm: BPM,
    };
    let (step, low) = (6.2 * beat(), 10f64.powf(-20.0 / 20.0));
    let x: Vec<f64> = chord(&[110.0], fs(), seconds, 1)
        .iter()
        .enumerate()
        .map(|(n, v)| {
            let t = f64::from(u32::try_from(n).unwrap()) / fs();
            let g = if t < step { 1.0 } else { duck.gain(t) };
            v * g * (1.0 - (1.0 - low) * ((t - step) / 0.1).clamp(0.0, 1.0))
        })
        .collect();
    let read = layer(&x, &note_list(&[(0.0, seconds, 110.0)]), BPM);
    let p = read.pump.as_ref().unwrap();
    assert_ne!(p.lock, PumpLock::Beat, "{p:?}");
    assert!(p.beats <= 6, "{p:?}");
    assert!(!read.flags.pump_stripped, "{p:?}");
}

#[test]
fn an_overlapping_note_whose_band_reaches_0_hz_reads_a_higher_harmonic() {
    // MIDI 6 sits at 11.6 Hz, under the band's lowest cutoff of 12 Hz. Its
    // fundamental has no band, so it reads its second harmonic. The A2's
    // harmonics all lie near one of its dense harmonics, so that note skips.
    let f0 = 440.0 * 2f64.powf((6.0 - 69.0) / 12.0);
    let seconds = 2.0;
    let x = chord(&[f0, 110.0], fs(), seconds, 1);
    let read = layer(
        &x,
        &note_list(&[(0.1, seconds, f0), (0.1, seconds, 110.0)]),
        BPM,
    );
    assert_eq!(read.counts.read, 1, "{:?}", read.counts);
    assert_eq!(read.counts.no_free_harmonic, 1, "{:?}", read.counts);
}

#[test]
fn a_duck_off_the_given_tempo_is_found_past_the_search_cap() {
    // A 1 % error moves the duck by 1.6 beats over 160 beats. The search's
    // first stage reads 64 of them, which fits the period closely enough.
    let (x, notes) = held_chord_over(160.0);
    let duck = Duck {
        depth_db: 12.0,
        release_s: 0.08,
        bpm: BPM * 1.01,
    };
    let p = pump(&duck.apply(&x, fs()), &notes, CHORD[0]).unwrap();
    assert_eq!(p.lock, PumpLock::Beat, "{p:?}");
    assert!(p.depth.get() >= 7.0, "{p:?}");
}

#[test]
fn a_short_duck_off_the_tempo_needs_the_second_stage_of_the_search() {
    // A 10 ms release leaves few bins to move: over 480 beats the period of
    // the first 64 still misses the duck by more than its width, and only
    // the fit over all of them locks. The first stage alone reads 1.56 dB.
    let (x, notes) = held_chord_over(480.0);
    let duck = Duck {
        depth_db: 12.0,
        release_s: 0.01,
        bpm: BPM * 1.02,
    };
    let p = pump(&duck.apply(&x, fs()), &notes, CHORD[0]).unwrap();
    assert!(p.strips(), "{p:?}");
}

/// `CHORD` from `f0`, over `beats` of `bpm`.
fn chord_at(f0: f64, beats: f64, bpm: f64) -> (Vec<f64>, NoteList) {
    let seconds = beats * 60.0 / bpm;
    let pitches = CHORD.map(|f| f * f0 / CHORD[0]);
    (
        chord(&pitches, fs(), seconds, 1),
        note_list(&pitches.map(|f| (0.0, seconds, f))),
    )
}

#[test]
fn a_duck_on_the_grid_keeps_the_grids_tempo() {
    // Over 60 to 174 BPM and 55 to 220 Hz, up to 34 of 4847 grid locks on the
    // tempo gain `FIT_GAIN`, nearly all 18 dB at 220 Hz, and move by up to
    // 0.22 %. At these depths and this tempo none does.
    for f0 in [110.0, 220.0] {
        for beats in [10.0, 12.0, 16.0] {
            let (x, notes) = chord_at(f0, beats, BPM);
            for (depth_db, release_s) in [(6.0, 0.15), (12.0, 0.08)] {
                let duck = Duck {
                    depth_db,
                    release_s,
                    bpm: BPM,
                };
                let p = pump_at(&ducked(&x, duck, 0.0), &notes, f0, BPM).unwrap();
                assert_eq!(p.lock, PumpLock::Beat, "{f0} {beats} {depth_db}: {p:?}");
                assert_eq!(p.bpm().get(), BPM, "{f0} {beats} {depth_db}: {p:?}");
            }
        }
    }
}

#[test]
fn a_fitted_read_on_the_tempo_needs_its_gain() {
    // The search gains 1.49 dB on the first and 1.52 dB on the second.
    let read = |beats: f64, bpm: f64, phase: f64| {
        let (x, notes) = chord_at(220.0, beats, bpm);
        let duck = Duck {
            depth_db: 12.0,
            release_s: 0.15,
            bpm,
        };
        pump_at(&ducked(&x, duck, phase), &notes, 220.0, bpm).unwrap()
    };
    assert_eq!(read(8.9, 80.0, 0.75).bpm().get(), 80.0);
    let moved = read(12.0, BPM, 0.3).bpm().get();
    assert!(
        moved != BPM && (moved / BPM - 1.0).abs() <= 0.002,
        "{moved}"
    );
}

#[test]
fn a_fitted_read_does_not_give_up_the_strip() {
    // A slower period counts fewer beats: each grid lock counts 8 beats and
    // strips, and the fitted read counts 7 and reads deeper.
    for (beats, bpm, off, release_s) in [(9.0, 96.0, -0.01, 0.08), (9.2, BPM, -0.02, 0.15)] {
        let (x, notes) = chord_at(CHORD[0], beats, bpm);
        let duck = Duck {
            depth_db: 12.0,
            release_s,
            bpm: bpm * (1.0 + off),
        };
        let p = pump_at(&ducked(&x, duck, 0.0), &notes, CHORD[0], bpm).unwrap();
        assert!(p.strips(), "{beats} {bpm}: {p:?}");
        assert_eq!(p.bpm().get(), bpm, "{beats} {bpm}: {p:?}");
    }
}

#[test]
fn a_fitted_read_replaces_a_grid_lock_that_does_not_strip() {
    // 8.9 beats, 0.5 % slow: both reads count 7 beats and neither strips.
    let (x, notes) = chord_at(CHORD[0], 8.9, BPM);
    let duck = Duck {
        depth_db: 12.0,
        release_s: 0.08,
        bpm: BPM * 0.995,
    };
    let p = pump_at(&ducked(&x, duck, 0.75), &notes, CHORD[0], BPM).unwrap();
    assert_eq!(p.lock, PumpLock::Beat, "{p:?}");
    assert!(!p.strips(), "{p:?}");
    assert_ne!(p.bpm().get(), BPM, "{p:?}");
}

#[test]
fn a_fitted_read_can_gain_the_strip() {
    // 8.9 beats count 7 on the grid, and a faster period counts 8.
    let (x, notes) = chord_at(CHORD[0], 8.9, BPM);
    let duck = Duck {
        depth_db: 12.0,
        release_s: 0.08,
        bpm: BPM * 1.02,
    };
    let p = pump_at(&ducked(&x, duck, 0.0), &notes, CHORD[0], BPM).unwrap();
    assert!(p.strips(), "{p:?}");
}

#[test]
fn a_smeared_grid_lock_gives_way_to_the_ducks_own_tempo() {
    // Phase 0.25 at 2.5 % and 0 at -1 % lock on the grid, at 4.4 and 7.2 dB.
    let (x, notes) = held_chord_over(16.0);
    for (off, phase) in [(0.025, 0.25), (-0.01, 0.0)] {
        let duck = Duck {
            depth_db: 12.0,
            release_s: 0.08,
            bpm: BPM * (1.0 + off),
        };
        let p = pump(&ducked(&x, duck, phase), &notes, CHORD[0]).unwrap();
        assert_eq!(p.lock, PumpLock::Beat, "{off}: {p:?}");
        assert!(p.depth.get() >= 9.0, "{off}: {p:?}");
        let bpm = p.bpm().get() / (BPM * (1.0 + off));
        assert!((bpm - 1.0).abs() <= 0.002, "{off}: {p:?}");
    }
}

#[test]
fn a_duck_that_reads_free_near_the_search_end_is_found_by_its_shape() {
    // 10 beats at -2.5 % and phase 0.25: the short span reads the beat rate
    // over 50 cents off, so the grid's read runs free. At phase 0.75 the 6 dB
    // duck reads no lock.
    let (x, notes) = held_chord_over(10.0);
    for (depth_db, release_s, least) in [(6.0, 0.15, 6.0), (12.0, 0.08, 8.0), (12.0, 0.3, 8.0)] {
        let off = -0.025;
        let duck = Duck {
            depth_db,
            release_s,
            bpm: BPM * (1.0 + off),
        };
        let p = pump(&ducked(&x, duck, 0.25), &notes, CHORD[0]).unwrap();
        assert_eq!(p.lock, PumpLock::Beat, "{depth_db}: {p:?}");
        assert!(p.strips(), "{depth_db}: {p:?}");
        assert!(p.depth.get() >= least, "{depth_db}: {p:?}");
    }
}

#[test]
fn a_duck_past_the_search_reach_stays_free() {
    // 4 % off over 12 beats: the search folds it at its end, and the free
    // line lies 71 cents from the beat rate.
    let (x, notes) = held_chord_over(12.0);
    let duck = Duck {
        depth_db: 12.0,
        release_s: 0.15,
        bpm: BPM * 0.96,
    };
    let p = pump(&duck.apply(&x, fs()), &notes, CHORD[0]).unwrap();
    assert_eq!(p.lock, PumpLock::Free, "{p:?}");
    assert_eq!(p.bpm().get(), BPM, "{p:?}");
    // 3.5 % off over 10 beats: the line lies 67 cents off.
    let (x, notes) = held_chord_over(10.0);
    let duck = Duck {
        depth_db: 12.0,
        release_s: 0.08,
        bpm: BPM * 0.965,
    };
    let p = pump(&duck.apply(&x, fs()), &notes, CHORD[0]).unwrap();
    assert_eq!(p.lock, PumpLock::Free, "{p:?}");
}

#[test]
fn a_duck_3_percent_off_strips_inside_the_free_reach() {
    let (x, notes) = held_chord_over(10.0);
    let duck = Duck {
        depth_db: 12.0,
        release_s: 0.08,
        bpm: BPM * 0.97,
    };
    let p = pump(&ducked(&x, duck, 0.0), &notes, CHORD[0]).unwrap();
    assert!(p.strips(), "{p:?}");
}

#[test]
fn a_gate_smeared_by_a_low_notes_window_stays_free() {
    // At 55 Hz the 36 ms window smears the edge over 7 bins at 174 BPM.
    let bpm = 174.0;
    for beats in [12.0, 16.0] {
        let (x, notes) = chord_at(55.0, beats, bpm);
        let period = 60.0 / (bpm * 0.97);
        let gated = under(&x, |t| {
            if (t / period).fract() < 0.1 {
                -12.0
            } else {
                0.0
            }
        });
        let p = pump_at(&gated, &notes, 55.0, bpm).unwrap();
        assert_ne!(p.lock, PumpLock::Beat, "{beats}: {p:?}");
    }
}

#[test]
fn a_fold_that_never_comes_back_is_no_duck() {
    // A wide gate 3.5 % off over 64 beats smears into a shallow fold, whose
    // curve stays under a quarter of its depth either way from the dip.
    let bpm = 96.0;
    let seconds = 64.0 * 60.0 / bpm;
    let notes = note_list(&CHORD.map(|f| (0.0, seconds, f)));
    let period = 60.0 / (bpm * 1.035);
    let x = chord(&CHORD, fs(), seconds, 1);
    let gated = under(&x, |t| {
        if (t / period).fract() < 0.3 {
            -9.0
        } else {
            0.0
        }
    });
    let p = pump_at(&gated, &notes, CHORD[0], bpm).unwrap();
    assert_eq!(p.lock, PumpLock::Free, "{p:?}");
}

#[test]
fn a_square_gate_near_a_division_stays_free_after_the_period_search() {
    // A gate 51 to 82 cents off the tempo: the search moves the tempo by up
    // to 43 cents, and a fitted read folds the gate onto one curve, but the
    // curve comes back at once.
    for (beats, off, low) in [(12.0, 0.035, 0.3), (16.0, 0.03, 0.1), (16.0, 0.0485, 0.1)] {
        let (x, notes) = held_chord_over(beats);
        let period = 60.0 / (BPM * (1.0 + off));
        let gated = under(&x, |t| {
            if (t / period).fract() < low {
                -9.0
            } else {
                0.0
            }
        });
        let p = pump(&gated, &notes, CHORD[0]).unwrap();
        assert_eq!(p.lock, PumpLock::Free, "{beats} {off}: {p:?}");
    }
}
