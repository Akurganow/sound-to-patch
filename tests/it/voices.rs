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
    assert!(legato.joins[0].legato, "{:?}", legato.joins[0]);
    let retrigger = joined(12.0, 0.005);
    assert_eq!(retrigger.joins.len(), 1, "{retrigger:?}");
    assert!(!retrigger.joins[0].legato, "{:?}", retrigger.joins[0]);
}

#[test]
fn a_long_attack_lowers_a_join() {
    let layer = joined(12.0, 0.3);
    assert_eq!(layer.joins.len(), 1, "{layer:?}");
    assert_eq!(layer.joins[0].confidence.level(), Some(Level::Low));
}

#[test]
fn an_overlap_counts_from_50_ms_of_held_notes() {
    let line = |overlap: f64| {
        let list = [(0.1, 0.6, 110.0), (0.6 - overlap, 1.1, 165.0)];
        let mut x = vec![0.0; samples(1.2, FS)];
        for &(a, b, f) in &list {
            tone(&mut x, (a, b), f, 0.005, 0.0);
        }
        read(&x, &note_list(&list))
    };
    assert_eq!(line(0.09).max_polyphony, 2);
    assert_eq!(line(0.03).max_polyphony, 1);
}

#[test]
fn a_gap_of_30_ms_or_more_is_no_join() {
    let line = |gap: f64| {
        let list = [(0.1, 0.6, 110.0), (0.6 + gap, 1.1, 165.0)];
        let mut x = vec![0.0; samples(1.2, FS)];
        for &(a, b, f) in &list {
            tone(&mut x, (a, b), f, 0.005, 0.0);
        }
        read(&x, &note_list(&list)).joins.len()
    };
    assert_eq!(line(0.02), 1);
    assert_eq!(line(0.04), 0);
}

#[test]
fn a_stem_without_notes_has_no_count() {
    let layer = read(&vec![0.0; 2205], &NoteList::default());
    assert!(!layer.confidence.is_estimate());
    assert_eq!(layer.max_polyphony, 0);
}
