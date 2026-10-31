//! The voices layer (#68): the count of voices and the joins of a mono line,
//! on generated signals.

use sound_to_patch::audio::{BeatGrid, SampleRate};
use sound_to_patch::confidence::Level;
use sound_to_patch::estimate::envelope::{EnvelopeInput, EnvelopeLayer};
use sound_to_patch::estimate::voices::{VoicesInput, VoicesLayer};
use sound_to_patch::notes::NoteList;
use sound_to_patch::units::{Bpm, Seconds};

use crate::support::envelope::{note_list, samples, saw};

const FS: f64 = 22_050.0;

fn read(x: &[f64], notes: &NoteList) -> VoicesLayer {
    let rate = SampleRate::new(22_050).unwrap();
    let envelope = EnvelopeLayer::of(&EnvelopeInput {
        mono: x,
        rate,
        notes,
        grid: BeatGrid::new(Bpm::new(120.0).unwrap(), Seconds::clamped(0.0)),
    })
    .unwrap();
    VoicesLayer::of(&VoicesInput {
        mono: x,
        rate,
        notes,
        envelope: &envelope,
    })
    .unwrap()
}

/// `f` from `from` to `to`, with a linear attack and release in seconds.
fn tone(x: &mut [f64], (from, to): (f64, f64), f: f64, attack: f64, release: f64) {
    let first = samples(from, FS);
    let source = saw(f, FS, samples(to + release, FS) - first, 0.0);
    for (i, v) in source.iter().enumerate() {
        let t = f64::from(u32::try_from(i).unwrap()) / FS;
        let gain = if attack > 0.0 {
            (t / attack).min(1.0)
        } else {
            1.0
        } * (1.0 - (t + from - to).max(0.0) / release.max(1e-9)).clamp(0.0, 1.0);
        if let Some(slot) = x.get_mut(first + i) {
            *slot += 0.2 * gain * v;
        }
    }
}

#[test]
fn one_saw_note_is_one_voice() {
    let mut x = vec![0.0; samples(1.2, FS)];
    tone(&mut x, (0.1, 1.0), 110.0, 0.005, 0.05);
    let layer = read(&x, &note_list(&[(0.1, 1.05, 110.0)]));
    assert_eq!(layer.max_polyphony, 1);
    assert!(layer.mono);
    assert_eq!(layer.confidence.level(), Some(Level::High));
}

#[test]
fn a_unison_of_seven_is_one_voice() {
    let cents = [-25.0, -16.0, -8.0, 0.0, 8.0, 16.0, 25.0];
    let list: Vec<(f64, f64, f64)> = cents
        .iter()
        .map(|c: &f64| (0.1, 1.05, 110.0 * 2f64.powf(c / 1200.0)))
        .collect();
    let mut x = vec![0.0; samples(1.2, FS)];
    for &(_, _, f) in &list {
        tone(&mut x, (0.1, 1.0), f, 0.005, 0.05);
    }
    let layer = read(&x, &note_list(&list));
    assert_eq!(layer.max_polyphony, 1);
    assert!(layer.mono);
    assert_eq!(layer.joins, []);
}

#[test]
fn three_notes_at_once_for_100_ms_are_three_voices() {
    let list = [(0.1, 1.0, 110.0), (0.5, 0.6, 165.0), (0.5, 0.6, 220.0)];
    let mut x = vec![0.0; samples(1.2, FS)];
    for &(a, b, f) in &list {
        tone(&mut x, (a, b), f, 0.005, 0.0);
    }
    let layer = read(&x, &note_list(&list));
    assert_eq!(layer.max_polyphony, 3);
    assert!(!layer.mono);
    assert_eq!(layer.joins, []);
}

#[test]
fn a_mono_line_with_release_tails_stays_mono() {
    let mut x = vec![0.0; samples(2.0, FS)];
    let mut list = Vec::new();
    for (k, f) in [110.0, 165.0, 131.0, 196.0].into_iter().enumerate() {
        let a = 0.1 + 0.4 * f64::from(u32::try_from(k).unwrap());
        tone(&mut x, (a, a + 0.4), f, 0.005, 0.08);
        // The transcriber's end lies past the note-off by the release and more.
        list.push((a, a + 0.4 + 0.08 + 0.03, f));
    }
    let layer = read(&x, &note_list(&list));
    assert_eq!(layer.max_polyphony, 1, "{layer:?}");
    assert!(layer.mono);
}

#[test]
fn two_notes_inside_60_cents_are_one_voice() {
    let list = [(0.1, 1.0, 110.0), (0.1, 1.0, 110.5)];
    let mut x = vec![0.0; samples(1.2, FS)];
    for &(a, b, f) in &list {
        tone(&mut x, (a, b), f, 0.005, 0.0);
    }
    let layer = read(&x, &note_list(&list));
    assert_eq!(layer.max_polyphony, 1);
}

/// 110 then 165 Hz saws that meet at 1 s; the second dips by `dip_db` and
/// rises over 5 ms when `dip_db` is above 0.
fn joined(dip_db: f64, attack: f64) -> VoicesLayer {
    let at = 1.0;
    let length = samples(2.0, FS);
    let (a, b) = (saw(110.0, FS, length, 0.0), saw(165.0, FS, length, 0.0));
    let floor = 10f64.powf(-dip_db / 20.0);
    let x: Vec<f64> = (0..length)
        .map(|i| {
            let t = f64::from(u32::try_from(i).unwrap()) / FS;
            if t < at {
                0.2 * (t / attack.max(0.005)).min(1.0) * a[i]
            } else {
                let rise = if dip_db > 0.0 {
                    floor + (1.0 - floor) * ((t - at) / attack.max(0.005)).min(1.0)
                } else {
                    1.0
                };
                0.2 * rise * b[i]
            }
        })
        .collect();
    read(&x, &note_list(&[(0.0, at, 110.0), (at, 2.0, 165.0)]))
}

#[test]
fn a_join_that_keeps_its_level_is_legato_and_a_new_attack_is_not() {
    let legato = joined(0.0, 0.005);
    assert_eq!(legato.joins.len(), 1, "{legato:?}");
    assert_eq!(legato.joins[0].legato, Some(true), "{:?}", legato.joins[0]);
    let retrigger = joined(12.0, 0.005);
    assert_eq!(retrigger.joins.len(), 1, "{retrigger:?}");
    assert_eq!(
        retrigger.joins[0].legato,
        Some(false),
        "{:?}",
        retrigger.joins[0]
    );
}

#[test]
fn a_long_attack_lowers_a_join() {
    let layer = joined(12.0, 0.3);
    assert_eq!(layer.joins.len(), 1, "{layer:?}");
    assert_eq!(layer.joins[0].confidence.level(), Some(Level::Low));
}

#[test]
fn an_overlap_counts_from_50_ms_of_held_notes() {
    // Both tones run on past the first note's end, so its held part ends 25 ms
    // before its listed end, at 0.575 s.
    let line = |overlap: f64| {
        let list = [(0.1, 0.6, 110.0), (0.575 - overlap, 1.1, 165.0)];
        let mut x = vec![0.0; samples(1.2, FS)];
        tone(&mut x, (0.1, 1.2), 110.0, 0.005, 0.0);
        tone(&mut x, (0.575 - overlap, 1.2), 165.0, 0.005, 0.0);
        read(&x, &note_list(&list)).max_polyphony
    };
    assert_eq!(line(0.055), 2);
    assert_eq!(line(0.045), 1);
}

#[test]
fn a_gap_under_30_ms_joins() {
    let line = |gap: f64| {
        let list = [(0.1, 0.6, 110.0), (0.6 + gap, 1.1, 165.0)];
        let mut x = vec![0.0; samples(1.2, FS)];
        for &(a, b, f) in &list {
            tone(&mut x, (a, b), f, 0.005, 0.0);
        }
        read(&x, &note_list(&list)).joins.len()
    };
    assert_eq!(line(0.027), 1);
    assert_eq!(line(0.033), 0);
}

#[test]
fn a_cluster_ends_at_60_cents() {
    let line = |cents: f64| {
        let f = 110.0 * 2f64.powf(cents / 1200.0);
        let list = [(0.1, 1.0, 110.0), (0.1, 1.0, f)];
        let mut x = vec![0.0; samples(1.2, FS)];
        tone(&mut x, (0.1, 1.0), 110.0, 0.005, 0.0);
        tone(&mut x, (0.1, 1.0), f, 0.005, 0.0);
        read(&x, &note_list(&list)).max_polyphony
    };
    assert_eq!(line(55.0), 1);
    assert_eq!(line(65.0), 2);
}

#[test]
fn a_short_note_inside_a_long_one_joins_nothing() {
    let list = [(0.1, 1.0, 110.0), (0.1, 0.13, 165.0), (0.5, 0.53, 220.0)];
    let mut x = vec![0.0; samples(1.2, FS)];
    for &(a, b, f) in &list {
        tone(&mut x, (a, b), f, 0.005, 0.0);
    }
    let layer = read(&x, &note_list(&list));
    assert!(layer.mono, "{layer:?}");
    assert_eq!(layer.joins, []);
}

#[test]
fn a_note_stacked_inside_a_long_one_joins_nothing() {
    let list = [(0.1, 1.0, 110.0), (0.5, 0.53, 165.0)];
    let mut x = vec![0.0; samples(1.2, FS)];
    for &(a, b, f) in &list {
        tone(&mut x, (a, b), f, 0.005, 0.0);
    }
    let layer = read(&x, &note_list(&list));
    assert!(layer.mono, "{layer:?}");
    assert_eq!(layer.joins, []);
}

#[test]
fn a_later_note_at_the_same_onset_joins_nothing() {
    let list = [(0.1, 0.13, 110.0), (0.1, 1.0, 165.0)];
    let mut x = vec![0.0; samples(1.2, FS)];
    for &(a, b, f) in &list {
        tone(&mut x, (a, b), f, 0.005, 0.0);
    }
    let layer = read(&x, &note_list(&list));
    assert!(layer.mono, "{layer:?}");
    assert_eq!(layer.joins, []);
}

#[test]
fn a_note_starting_inside_the_50_ms_does_not_count_early() {
    // The first holds to 0.16 s and the second starts at 0.12 s: 40 ms held
    // together, though both are held 50 ms after the first onset would reach
    // the second's start.
    let list = [(0.1, 0.185, 110.0), (0.12, 1.1, 165.0)];
    let mut x = vec![0.0; samples(1.3, FS)];
    tone(&mut x, (0.1, 1.3), 110.0, 0.005, 0.0);
    tone(&mut x, (0.12, 1.3), 165.0, 0.005, 0.0);
    assert_eq!(read(&x, &note_list(&list)).max_polyphony, 1);
}

#[test]
fn silence_before_a_late_onset_is_no_legato() {
    // The note list puts the second onset 40 ms after the silence ends, past the
    // 25 ms before it, as a transcribed onset can.
    let list = [(0.1, 0.57, 110.0), (0.59, 1.1, 165.0)];
    let mut x = vec![0.0; samples(1.2, FS)];
    tone(&mut x, (0.1, 0.5), 110.0, 0.005, 0.0);
    tone(&mut x, (0.55, 1.1), 165.0, 0.005, 0.0);
    let layer = read(&x, &note_list(&list));
    assert_eq!(layer.joins[0].legato, Some(false), "{:?}", layer.joins[0]);
}

#[test]
fn a_join_reads_medium_only_on_a_dip_with_a_short_attack() {
    let level = |layer: &VoicesLayer| layer.joins[0].confidence.level();
    assert_eq!(level(&joined(12.0, 0.08)), Some(Level::Medium));
    assert_eq!(level(&joined(12.0, 0.15)), Some(Level::Low));
    // A join that keeps its level cannot rule out an instant retrigger.
    assert_eq!(level(&joined(0.0, 0.08)), Some(Level::Low));
}

#[test]
fn a_join_with_no_attack_read_reads_low() {
    // The tones sound before their listed onsets, so no attack shows.
    let list = [(0.5, 1.0, 110.0), (1.0, 1.5, 165.0)];
    let mut x = vec![0.0; samples(1.6, FS)];
    tone(&mut x, (0.0, 1.0), 110.0, 0.0, 0.0);
    tone(&mut x, (1.0, 1.6), 165.0, 0.0, 0.0);
    let layer = read(&x, &note_list(&list));
    assert_eq!(
        layer.joins[0].confidence.level(),
        Some(Level::Low),
        "{layer:?}"
    );
    assert_eq!(layer.joins[0].legato, Some(true));
}

#[test]
fn the_count_reads_low_medium_and_high_by_the_share_that_agrees() {
    // The tones run on past the list, so each note holds to 25 ms before its
    // end, and the two overlap over 1 s less the second's delay.
    let level = |delay: f64| {
        let list = [(0.1, 1.125, 110.0), (0.1 + delay, 1.125, 165.0)];
        let mut x = vec![0.0; samples(1.3, FS)];
        tone(&mut x, (0.1, 1.3), 110.0, 0.005, 0.0);
        tone(&mut x, (0.1 + delay, 1.3), 165.0, 0.005, 0.0);
        read(&x, &note_list(&list)).confidence.level()
    };
    assert_eq!(level(0.05), Some(Level::High));
    assert_eq!(level(0.15), Some(Level::Medium));
    assert_eq!(level(0.25), Some(Level::Medium));
    assert_eq!(level(0.5), Some(Level::Low));
}

#[test]
fn a_flat_line_with_no_dip_reads_legato_under_a_wobble() {
    // A 6 dB wobble at 8 Hz is no dip at the join.
    let at = 1.0;
    let length = samples(2.0, FS);
    let (a, b) = (saw(110.0, FS, length, 0.0), saw(165.0, FS, length, 0.0));
    let x: Vec<f64> = (0..length)
        .map(|i| {
            let t = f64::from(u32::try_from(i).unwrap()) / FS;
            let wobble = 10f64.powf((-3.0 + 3.0 * (std::f64::consts::TAU * 8.0 * t).sin()) / 20.0);
            0.2 * wobble * if t < at { a[i] } else { b[i] }
        })
        .collect();
    let layer = read(&x, &note_list(&[(0.0, at, 110.0), (at, 2.0, 165.0)]));
    assert_eq!(layer.joins[0].legato, Some(true), "{:?}", layer.joins[0]);
}

#[test]
fn a_note_joins_up_to_50_ms_inside_the_earlier_held_part() {
    // The first note holds to 0.575 s, with both tones running on past it.
    let joins_of = |list: &[(f64, f64, f64)]| {
        let mut x = vec![0.0; samples(1.3, FS)];
        tone(&mut x, (0.1, 1.3), 110.0, 0.005, 0.0);
        tone(&mut x, (list[1].0, 1.3), 165.0, 0.005, 0.0);
        let layer = read(&x, &note_list(list));
        assert!(layer.mono, "{layer:?}");
        layer.joins
    };
    let inside = joins_of(&[(0.1, 0.6, 110.0), (0.545, 1.1, 165.0)]);
    assert_eq!(inside.len(), 1, "{inside:?}");
    assert!(inside[0].gap < 0.0, "{:?}", inside[0]);
    // A short note, so it stays mono, 70 ms inside.
    assert_eq!(joins_of(&[(0.1, 0.6, 110.0), (0.505, 0.53, 165.0)]), []);
}

#[test]
fn a_stacked_note_does_not_end_a_line() {
    let read_line = |ghosts: &[(f64, f64, f64)]| {
        let mut list = vec![(0.1, 0.6, 110.0), (0.6, 1.1, 165.0)];
        list.extend_from_slice(ghosts);
        list.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut x = vec![0.0; samples(1.2, FS)];
        tone(&mut x, (0.1, 0.6), 110.0, 0.005, 0.0);
        tone(&mut x, (0.6, 1.1), 165.0, 0.005, 0.0);
        for &(a, b, f) in ghosts {
            tone(&mut x, (a, b), f, 0.005, 0.0);
        }
        read(&x, &note_list(&list))
    };
    let times =
        |layer: &VoicesLayer| -> Vec<f64> { layer.joins.iter().map(|j| j.time.get()).collect() };
    assert_eq!(times(&read_line(&[])), [0.6]);
    assert_eq!(times(&read_line(&[(0.3, 0.34, 220.0)])), [0.6]);
    assert_eq!(
        times(&read_line(&[(0.3, 0.33, 220.0), (0.34, 0.37, 196.0)])),
        [0.6]
    );
}

#[test]
fn a_silent_note_joins_nothing_and_does_not_end_a_line() {
    // Two real notes around a silent listed one in the gap of digital silence.
    let list = [(0.1, 0.5, 110.0), (0.7, 0.75, 220.0), (0.75, 1.1, 165.0)];
    let mut x = vec![0.0; samples(1.2, FS)];
    tone(&mut x, (0.1, 0.5), 110.0, 0.005, 0.0);
    tone(&mut x, (0.76, 1.1), 165.0, 0.005, 0.0);
    let layer = read(&x, &note_list(&list));
    assert_eq!(layer.joins, []);

    // A silent note between two that nearly touch leaves them a line.
    let list = [(0.1, 2.0, 110.0), (1.0, 1.5, 247.0), (2.0, 2.4, 165.0)];
    let mut x = vec![0.0; samples(2.5, FS)];
    tone(&mut x, (0.1, 0.3), 110.0, 0.005, 0.0);
    tone(&mut x, (2.0, 2.4), 165.0, 0.005, 0.0);
    let layer = read(&x, &note_list(&list));
    let times: Vec<f64> = layer.joins.iter().map(|j| j.time.get()).collect();
    assert_eq!(times, [2.0], "{layer:?}");

    // Touching notes over zeros, past one that sounds.
    let list = [(0.1, 0.5, 110.0), (0.7, 0.9, 165.0), (0.9, 1.1, 220.0)];
    let mut x = vec![0.0; samples(1.2, FS)];
    tone(&mut x, (0.1, 0.5), 110.0, 0.005, 0.0);
    assert_eq!(read(&x, &note_list(&list)).joins, []);
}

#[test]
fn notes_that_all_sound_silent_have_no_count() {
    let list = [(0.1, 0.5, 110.0), (0.5, 0.9, 165.0)];
    let layer = read(&vec![0.0; samples(1.0, FS)], &note_list(&list));
    assert!(!layer.confidence.is_estimate(), "{layer:?}");
    assert_eq!(layer.max_polyphony, 0);
    assert!(!layer.mono);
    assert_eq!(layer.joins, []);
    assert_eq!(
        layer.summary(),
        ["voices: no note holds a sounding part, so no count"]
    );
}

#[test]
fn a_join_past_the_stem_has_no_verdict() {
    let list = [
        (0.1, 0.5, 110.0),
        (0.5, 0.9, 165.0),
        (2.0, 2.4, 110.0),
        (2.4, 2.8, 165.0),
    ];
    let mut x = vec![0.0; samples(1.0, FS)];
    tone(&mut x, (0.1, 0.5), 110.0, 0.005, 0.0);
    tone(&mut x, (0.5, 0.9), 165.0, 0.005, 0.0);
    let layer = read(&x, &note_list(&list));
    assert_eq!(layer.joins.len(), 2, "{layer:?}");
    assert!(layer.joins[0].dip.is_some(), "{layer:?}");
    for join in &layer.joins[1..] {
        assert_eq!(join.dip, None, "{join:?}");
        assert_eq!(join.legato, None, "{join:?}");
        assert!(!join.confidence.is_estimate(), "{join:?}");
    }
}

#[test]
fn a_join_onto_zeros_is_no_legato() {
    // The later note sounds only from 0.9 s, so the level after the join is the floor.
    let list = [(0.1, 0.5, 110.0), (0.5, 1.1, 165.0)];
    let mut x = vec![0.0; samples(1.2, FS)];
    tone(&mut x, (0.1, 0.5), 110.0, 0.005, 0.0);
    tone(&mut x, (0.9, 1.1), 165.0, 0.005, 0.0);
    let layer = read(&x, &note_list(&list));
    assert_ne!(layer.joins[0].legato, Some(true), "{layer:?}");
}

#[test]
fn notes_held_for_no_time_have_no_count() {
    let list = [(0.5, 0.51, 110.0), (0.51, 0.52, 165.0)];
    let mut x = vec![0.0; samples(1.0, FS)];
    for &(a, b, f) in &list {
        tone(&mut x, (a, b), f, 0.0, 0.0);
    }
    let layer = read(&x, &note_list(&list));
    assert!(!layer.confidence.is_estimate(), "{layer:?}");
    assert_eq!(layer.max_polyphony, 0);
}

#[test]
fn a_join_just_past_the_stem_end_has_no_dip() {
    let list = [(0.1, 0.95, 110.0), (0.95, 1.3, 165.0)];
    let mut x = vec![0.0; samples(0.9, FS)];
    tone(&mut x, (0.1, 0.9), 110.0, 0.005, 0.0);
    let layer = read(&x, &note_list(&list));
    assert_eq!(layer.joins[0].dip, None, "{layer:?}");
    assert_eq!(layer.joins[0].legato, None, "{layer:?}");
}

/// A 0 dB saw, then a saw of `after_db` from a whole-cycle join to the stem end,
/// with the later note listed 0.3 s on.
fn cut_after_join(after_db: f64, stem_after_join: f64) -> VoicesLayer {
    let at = 30.0 / 55.0;
    let length = samples(at + stem_after_join, FS);
    let (a, b) = (saw(110.0, FS, length, 0.0), saw(165.0, FS, length, 0.0));
    let gain = 10f64.powf(after_db / 20.0);
    let x: Vec<f64> = (0..length)
        .map(|i| {
            if f64::from(u32::try_from(i).unwrap()) / FS < at {
                0.2 * a[i]
            } else {
                0.2 * gain * b[i]
            }
        })
        .collect();
    read(&x, &note_list(&[(0.0, at, 110.0), (at, at + 0.3, 165.0)]))
}

#[test]
fn the_window_after_a_join_stops_at_the_stem_end() {
    // The quieter later note sets the level, though 0.1 s of it is all there is.
    let layer = cut_after_join(-12.0, 0.1);
    assert_eq!(layer.joins[0].legato, Some(true), "{:?}", layer.joins[0]);
}

#[test]
fn the_dip_window_stops_at_the_stem_end() {
    // Past the stem the level is the floor, 200 dB down.
    let layer = cut_after_join(0.0, 0.015);
    let dip = layer.joins[0].dip.expect("a level before the join");
    assert!(dip.get() < 3.0, "{:?}", layer.joins[0]);
}

#[test]
fn a_note_past_the_stem_holds_no_voice() {
    // Two overlapping notes past the end and one that crosses it.
    let list = [(0.1, 1.5, 110.0), (1.2, 1.6, 165.0), (1.3, 1.7, 247.0)];
    let mut x = vec![0.0; samples(1.0, FS)];
    tone(&mut x, (0.1, 1.0), 110.0, 0.005, 0.0);
    let layer = read(&x, &note_list(&list));
    assert!(layer.mono, "{layer:?}");
    assert_eq!(layer.max_polyphony, 1, "{layer:?}");
    assert!((layer.mean_polyphony - 1.0).abs() < 1e-9, "{layer:?}");
}

#[test]
fn a_stem_with_only_notes_past_it_has_no_count() {
    let layer = read(
        &vec![0.1; samples(1.0, FS)],
        &note_list(&[(2.0, 2.5, 110.0)]),
    );
    assert!(!layer.confidence.is_estimate(), "{layer:?}");
}

#[test]
fn a_join_near_the_stem_start_reads_no_centroid() {
    // 2 samples into the stem.
    let at = 2.0 / FS;
    let list = [(0.0, at, 110.0), (at, 0.5, 165.0)];
    let mut x = vec![0.0; samples(0.6, FS)];
    tone(&mut x, (0.0, 0.5), 165.0, 0.0, 0.0);
    let layer = read(&x, &note_list(&list));
    for join in &layer.joins {
        assert_eq!(join.centroid_change, None, "{join:?}");
    }
}

#[test]
fn a_note_listed_past_the_stem_still_reads_its_dip() {
    // 12 dB, 20 ms retrigger, the later note listed 0.3 s on and the stem ending
    // 0.1 s after the join.
    let at = 0.5;
    let length = samples(at + 0.1, FS);
    let (a, b) = (saw(110.0, FS, length, 0.0), saw(165.0, FS, length, 0.0));
    let floor = 10f64.powf(-12.0 / 20.0);
    let x: Vec<f64> = (0..length)
        .map(|i| {
            let t = f64::from(u32::try_from(i).unwrap()) / FS;
            if t < at {
                0.2 * a[i]
            } else {
                0.2 * (floor + (1.0 - floor) * ((t - at) / 0.02).min(1.0)) * b[i]
            }
        })
        .collect();
    let layer = read(&x, &note_list(&[(0.0, at, 110.0), (at, at + 0.3, 165.0)]));
    assert_eq!(layer.joins[0].legato, Some(false), "{:?}", layer.joins[0]);
}

#[test]
fn a_stem_without_notes_has_no_count() {
    let layer = read(&vec![0.0; 2205], &NoteList::default());
    assert!(!layer.confidence.is_estimate());
    assert_eq!(layer.max_polyphony, 0);
}

#[test]
fn a_silent_note_holds_no_voice() {
    // Two listed notes overlap over digital silence, after the one that sounds.
    let mut x = vec![0.0; samples(1.2, FS)];
    tone(&mut x, (0.1, 0.5), 110.0, 0.005, 0.0);
    let list = [(0.1, 0.5, 110.0), (0.7, 1.1, 165.0), (0.7, 1.1, 220.0)];
    let layer = read(&x, &note_list(&list));
    assert_eq!(layer.max_polyphony, 1, "{layer:?}");
}

#[test]
fn a_join_with_no_level_beside_it_has_no_verdict() {
    // A later note holds, so the stem has a count while the short pair has no level.
    let list = [(0.5, 0.51, 110.0), (0.51, 0.52, 165.0), (0.8, 1.2, 110.0)];
    let mut x = vec![0.0; samples(1.3, FS)];
    for &(a, b, f) in &list {
        tone(&mut x, (a, b), f, 0.0, 0.0);
    }
    let layer = read(&x, &note_list(&list));
    assert_eq!(layer.joins.len(), 1, "{layer:?}");
    assert_eq!(layer.joins[0].legato, None);
    assert_eq!(layer.joins[0].dip, None);
    assert!(!layer.joins[0].confidence.is_estimate());
}

#[test]
fn a_centroid_past_the_stem_is_not_read() {
    let list = [(0.1, 0.5, 110.0), (0.5, 0.505, 110.0 * 1.5)];
    let mut x = vec![0.0; samples(0.51, FS)];
    for &(a, b, f) in &list {
        tone(&mut x, (a, b), f, 0.0, 0.0);
    }
    let layer = read(&x, &note_list(&list));
    assert_eq!(layer.joins[0].centroid_change, None, "{layer:?}");
}
