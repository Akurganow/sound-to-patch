//! The MIDI reader behind `analyze --notes` (#56).

use std::path::{Path, PathBuf};

use midly::num::{u4, u7, u15, u24, u28};
use midly::{
    Format, Fps, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind,
};
use sound_to_patch::notes::{NoteList, read_midi};
use sound_to_patch::units::Bpm;

/// An event `delta` ticks after the previous one.
enum Ev {
    On(u32, u8, u8),
    Off(u32, u8),
    Tempo(u32, f64),
}

fn track(events: &[Ev]) -> Vec<TrackEvent<'static>> {
    let midi = |delta: u32, message| TrackEvent {
        delta: u28::new(delta),
        kind: TrackEventKind::Midi {
            channel: u4::new(0),
            message,
        },
    };
    let mut track: Vec<TrackEvent<'static>> = events
        .iter()
        .map(|event| match *event {
            Ev::On(delta, key, vel) => midi(
                delta,
                MidiMessage::NoteOn {
                    key: u7::new(key),
                    vel: u7::new(vel),
                },
            ),
            Ev::Off(delta, key) => midi(
                delta,
                MidiMessage::NoteOff {
                    key: u7::new(key),
                    vel: u7::new(64),
                },
            ),
            Ev::Tempo(delta, bpm) => TrackEvent {
                delta: u28::new(delta),
                kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::new(
                    u32::try_from(crate::support::cast::round_to_usize(60e6 / bpm)).unwrap(),
                ))),
            },
        })
        .collect();
    track.push(TrackEvent {
        delta: u28::new(0),
        kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
    });
    track
}

fn save(dir: &Path, format: Format, timing: Timing, tracks: &[&[Ev]]) -> PathBuf {
    let mut smf = Smf::new(Header::new(format, timing));
    smf.tracks = tracks.iter().map(|events| track(events)).collect();
    let path = dir.join("notes.mid");
    smf.save(&path).unwrap();
    path
}

const PPQ: Timing = Timing::Metrical(u15::new(480));

fn bpm(value: f64) -> Bpm {
    Bpm::new(value).unwrap()
}

/// `(onset, offset, pitch, velocity)` of each note.
fn summary(notes: &NoteList) -> Vec<(f64, f64, f64, u8)> {
    notes
        .notes()
        .iter()
        .map(|note| {
            (
                note.onset().get(),
                note.offset().get(),
                note.pitch().get(),
                note.velocity().unwrap().get(),
            )
        })
        .collect()
}

#[test]
fn notes_follow_the_tempo_track_of_a_format_1_file() {
    let dir = tempfile::tempdir().unwrap();
    // 480 ticks per beat at 120 BPM: one tick is 1/960 s.
    let path = save(
        dir.path(),
        Format::Parallel,
        PPQ,
        &[
            &[Ev::Tempo(0, 120.0)],
            &[
                Ev::On(480, 40, 90),
                Ev::Off(240, 40),
                Ev::On(0, 52, 100),
                // A note-on of velocity 0 is a note-off.
                Ev::On(960, 52, 0),
            ],
        ],
    );
    let notes = read_midi(&path, bpm(120.0)).unwrap();
    assert_eq!(
        summary(&notes),
        [(0.5, 0.75, 40.0, 90), (0.75, 1.75, 52.0, 100)]
    );
}

#[test]
fn a_repeated_key_closes_first_in_first_out_and_a_hanging_note_ends_with_its_track() {
    let dir = tempfile::tempdir().unwrap();
    let path = save(
        dir.path(),
        Format::SingleTrack,
        PPQ,
        &[&[
            Ev::Tempo(0, 120.0),
            Ev::On(0, 45, 80),
            Ev::On(480, 45, 81),
            Ev::Off(480, 45),
            Ev::Off(480, 45),
            Ev::On(0, 47, 82),
            // A zero-length note carries no sound and is left out.
            Ev::On(480, 48, 83),
            Ev::Off(0, 48),
        ]],
    );
    let notes = read_midi(&path, bpm(120.0)).unwrap();
    assert_eq!(
        summary(&notes),
        [
            (0.0, 1.0, 45.0, 80),
            (0.5, 1.5, 45.0, 81),
            (1.5, 2.0, 47.0, 82)
        ]
    );
}

#[test]
fn a_tempo_other_than_the_bpm_fails_with_both_numbers() {
    let dir = tempfile::tempdir().unwrap();
    let path = save(
        dir.path(),
        Format::SingleTrack,
        PPQ,
        &[&[
            Ev::Tempo(0, 120.0),
            Ev::On(0, 45, 80),
            Ev::Tempo(480, 140.0),
            Ev::Off(480, 45),
        ]],
    );
    let message = read_midi(&path, bpm(120.0)).unwrap_err().to_string();
    assert!(message.contains("notes.mid"), "{message}");
    assert!(message.contains("140.000 BPM"), "{message}");
    assert!(message.contains("--bpm is 120"), "{message}");
}

#[test]
fn a_file_without_tempo_plays_at_the_midi_default() {
    let dir = tempfile::tempdir().unwrap();
    let path = save(
        dir.path(),
        Format::SingleTrack,
        PPQ,
        &[&[Ev::On(0, 45, 80), Ev::Off(960, 45)]],
    );
    let notes = read_midi(&path, bpm(120.0)).unwrap();
    assert_eq!(summary(&notes), [(0.0, 1.0, 45.0, 80)]);
    let message = read_midi(&path, bpm(90.0)).unwrap_err().to_string();
    assert!(
        message.contains("120.000 BPM (the MIDI default"),
        "{message}"
    );
}

#[test]
fn timecode_ticks_are_seconds_and_ignore_the_tempo() {
    let dir = tempfile::tempdir().unwrap();
    // 25 frames of 40 ticks: 1000 ticks per second.
    let path = save(
        dir.path(),
        Format::SingleTrack,
        Timing::Timecode(Fps::Fps25, 40),
        &[&[Ev::Tempo(0, 90.0), Ev::On(500, 45, 80), Ev::Off(250, 45)]],
    );
    let notes = read_midi(&path, bpm(120.0)).unwrap();
    assert_eq!(summary(&notes), [(0.5, 0.75, 45.0, 80)]);
}

#[test]
fn a_sequential_or_empty_file_fails_with_the_cause() {
    let dir = tempfile::tempdir().unwrap();
    let path = save(
        dir.path(),
        Format::Sequential,
        PPQ,
        &[&[Ev::On(0, 45, 80), Ev::Off(960, 45)]],
    );
    let message = read_midi(&path, bpm(120.0)).unwrap_err().to_string();
    assert!(message.contains("format 2"), "{message}");
    let path = save(
        dir.path(),
        Format::SingleTrack,
        PPQ,
        &[&[Ev::Tempo(0, 120.0)]],
    );
    let message = read_midi(&path, bpm(120.0)).unwrap_err().to_string();
    assert!(message.contains("holds no notes"), "{message}");
}
