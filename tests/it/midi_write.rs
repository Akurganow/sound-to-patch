//! The format 0 MIDI writer (#62).

use midly::{Format, MetaMessage, MidiMessage, Smf, Timing, TrackEventKind};
use sound_to_patch::notes::{
    DEFAULT_VELOCITY, Note, NoteList, TICKS_PER_BEAT, Velocities, Velocity, read_midi, write_midi,
};
use sound_to_patch::units::{Bpm, MidiPitch, Seconds};

use crate::support::cast::round_to_usize;
use crate::support::golden::{Golden, floats};

fn bpm(value: f64) -> Bpm {
    Bpm::new(value).unwrap()
}

fn note(onset: f64, offset: f64, pitch: f64) -> Note {
    Note::new(
        Seconds::new(onset).unwrap(),
        Seconds::new(offset).unwrap(),
        MidiPitch::new(pitch).unwrap(),
    )
    .unwrap()
}

/// One decoded event: absolute tick, note-on or note-off, key, velocity.
type Event = (u64, bool, u8, u8);

/// The tempos, the note events and every other event kind of a file, after
/// checking that it is format 0 with one track at 480 ticks per beat.
fn read(bytes: &[u8]) -> (Vec<u32>, Vec<Event>, Vec<String>) {
    let smf = Smf::parse(bytes).unwrap();
    assert_eq!(smf.header.format, Format::SingleTrack);
    assert_eq!(smf.header.timing, Timing::Metrical(TICKS_PER_BEAT.into()));
    assert_eq!(smf.tracks.len(), 1);
    let (mut tempos, mut notes, mut other) = (Vec::new(), Vec::new(), Vec::new());
    let mut tick = 0_u64;
    for event in &smf.tracks[0] {
        tick += u64::from(event.delta.as_int());
        match event.kind {
            TrackEventKind::Meta(MetaMessage::Tempo(us)) => {
                assert_eq!(tick, 0);
                tempos.push(us.as_int());
            }
            TrackEventKind::Meta(MetaMessage::TimeSignature(4, 2, _, _)) => assert_eq!(tick, 0),
            TrackEventKind::Meta(MetaMessage::EndOfTrack) => {}
            TrackEventKind::Midi { channel, message } => {
                assert_eq!(channel.as_int(), 0);
                match message {
                    MidiMessage::NoteOn { key, vel } => {
                        notes.push((tick, true, key.as_int(), vel.as_int()));
                    }
                    MidiMessage::NoteOff { key, .. } => notes.push((tick, false, key.as_int(), 0)),
                    other_message => other.push(format!("{other_message:?}")),
                }
            }
            other_kind => other.push(format!("{other_kind:?}")),
        }
    }
    (tempos, notes, other)
}

fn constant() -> Velocities {
    Velocities::Constant(DEFAULT_VELOCITY)
}

#[test]
fn a_file_has_one_track_one_tempo_and_nothing_but_notes() {
    let list = NoteList::new(vec![note(0.0, 0.5, 45.0), note(0.5, 1.0, 47.0)]);
    for tempo in [4.0, 120.0, 140.0, 173.5, 1000.0] {
        let (tempos, notes, other) = read(&write_midi(&list, bpm(tempo), constant()).unwrap());
        assert_eq!(
            tempos,
            vec![u32::try_from(round_to_usize(60e6 / tempo)).unwrap()]
        );
        assert_eq!(notes.len(), 4);
        assert!(other.is_empty(), "{other:?}");
    }
}

#[test]
fn note_times_match_the_golden_ticks() {
    let golden = Golden::load("midi-timing");
    let tolerance = golden.tolerance().abs;
    let recipe = &golden.recipe()["notes"];
    let list = NoteList::new(
        recipe
            .as_array()
            .unwrap()
            .iter()
            .map(|n| {
                note(
                    n["onset_s"].as_f64().unwrap(),
                    n["offset_s"].as_f64().unwrap(),
                    n["pitch"].as_f64().unwrap(),
                )
            })
            .collect(),
    );
    let mut checked = 0;
    for (tempo_text, values) in golden.values().as_object().unwrap() {
        let tempo: f64 = tempo_text.parse().unwrap();
        let (tempos, events, _) = read(&write_midi(&list, bpm(tempo), constant()).unwrap());
        assert!((f64::from(tempos[0]) - values["tempo_us"].as_f64().unwrap()).abs() <= 0.5);
        let onsets = floats(&values["onset_ticks"]);
        let offsets = floats(&values["offset_ticks"]);
        for (i, n) in recipe.as_array().unwrap().iter().enumerate() {
            let key = u8::try_from(n["pitch"].as_u64().unwrap()).unwrap();
            let tick = |on: bool| {
                let found = events.iter().find(|e| e.2 == key && e.1 == on).unwrap();
                f64::from(u32::try_from(found.0).unwrap())
            };
            assert!(
                (tick(true) - onsets[i]).abs() <= tolerance,
                "{tempo} BPM note {i} onset"
            );
            assert!(
                (tick(false) - offsets[i]).abs() <= tolerance,
                "{tempo} BPM note {i} offset"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 36);
}

#[test]
fn touching_notes_of_one_pitch_stay_two() {
    let list = NoteList::new(vec![note(0.0, 0.5, 60.0), note(0.5, 1.0, 60.0)]);
    let (_, events, _) = read(&write_midi(&list, bpm(120.0), constant()).unwrap());
    assert_eq!(
        events,
        vec![
            (0, true, 60, 100),
            (480, false, 60, 0),
            (480, true, 60, 100),
            (960, false, 60, 0),
        ]
    );
}

#[test]
fn overlaps_are_cut_and_short_notes_lengthened() {
    let list = NoteList::new(vec![
        // Cut at the next onset of its pitch.
        note(0.0, 1.0, 60.0),
        note(0.5, 1.5, 60.0),
        // Of two notes at one onset and pitch the longer stays.
        note(2.0, 2.2, 62.0),
        note(2.0, 2.5, 62.0),
        // 3 ms becomes 10 ms: 9.6 ticks at 120 BPM, 10 after rounding.
        note(3.0, 3.003, 64.0),
    ]);
    let (_, events, _) = read(&write_midi(&list, bpm(120.0), constant()).unwrap());
    let span =
        |key: u8| -> Vec<u64> { events.iter().filter(|e| e.2 == key).map(|e| e.0).collect() };
    assert_eq!(span(60), vec![0, 480, 480, 1440]);
    assert_eq!(span(62), vec![1920, 2400]);
    assert_eq!(span(64), vec![2880, 2890]);
    // At 4 BPM one tick lasts 31 ms, so 10 ms rounds to 0 ticks and the
    // note keeps one tick.
    let (_, events, _) = read(&write_midi(&list, bpm(4.0), constant()).unwrap());
    let ticks: Vec<_> = events.iter().filter(|e| e.2 == 64).map(|e| e.0).collect();
    assert_eq!(ticks[1] - ticks[0], 1);
}

#[test]
fn velocities_come_from_the_notes_or_one_constant() {
    let velocity = |v| Velocity::new(v).unwrap();
    let list = NoteList::new(vec![
        note(0.0, 0.5, 60.0).with_velocity(velocity(40)),
        note(0.5, 1.0, 62.0).with_velocity(velocity(100)),
        note(1.0, 1.5, 64.0),
    ]);
    let ons = |velocities| -> Vec<u8> {
        let (_, events, _) = read(&write_midi(&list, bpm(120.0), velocities).unwrap());
        events.iter().filter(|e| e.1).map(|e| e.3).collect()
    };
    assert_eq!(ons(Velocities::FromNotes(velocity(90))), vec![40, 100, 90]);
    assert_eq!(ons(Velocities::Constant(velocity(77))), vec![77, 77, 77]);
}

#[test]
fn a_173_5_bpm_file_reads_back_with_its_notes() {
    let list = NoteList::new(vec![
        note(0.0, 0.25, 33.0),
        note(0.3, 0.9, 45.0),
        note(0.3, 0.9, 52.0),
        note(1.7, 2.6, 57.5),
    ]);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.mid");
    std::fs::write(
        &path,
        write_midi(&list, bpm(173.5), Velocities::FromNotes(DEFAULT_VELOCITY)).unwrap(),
    )
    .unwrap();
    let back = read_midi(&path, bpm(173.5)).unwrap();
    assert_eq!(back.notes().len(), 4);
    let half_tick = 60.0 / (173.5 * 480.0) / 2.0;
    for (a, b) in list.notes().iter().zip(back.notes()) {
        assert_eq!(b.pitch().get(), a.pitch().get().round());
        assert!((a.onset().get() - b.onset().get()).abs() <= half_tick + 1e-12);
        assert!((a.offset().get() - b.offset().get()).abs() <= half_tick + 1e-12);
        assert_eq!(b.velocity(), Some(DEFAULT_VELOCITY));
    }
}

#[test]
fn the_longer_of_two_notes_at_one_key_and_start_stays_in_either_order() {
    for list in [
        vec![note(0.0, 1.0, 60.0), note(0.0, 0.3, 60.0)],
        vec![note(0.0, 0.3, 60.0), note(0.0, 1.0, 60.0)],
    ] {
        let (_, events, _) =
            read(&write_midi(&NoteList::new(list), bpm(120.0), constant()).unwrap());
        assert_eq!(events, vec![(0, true, 60, 100), (960, false, 60, 0)]);
    }
}

#[test]
fn a_fractional_pitch_rounds_to_the_nearest_key() {
    let list = NoteList::new(vec![note(0.0, 0.5, 59.4), note(0.5, 1.0, 59.6)]);
    let (_, events, _) = read(&write_midi(&list, bpm(120.0), constant()).unwrap());
    let keys: Vec<u8> = events.iter().filter(|e| e.1).map(|e| e.2).collect();
    assert_eq!(keys, vec![59, 60]);
}

#[test]
fn an_empty_list_is_an_error() {
    let error = write_midi(&NoteList::default(), bpm(120.0), constant()).unwrap_err();
    assert_eq!(error.to_string(), "cannot write a MIDI file without notes");
}
