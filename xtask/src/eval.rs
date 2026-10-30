//! `cargo xtask eval`: the notes step on generated parts with known notes.
//!
//! Every part runs the model once, then decodes with each manual profile
//! and with the detected settings. The tables score each decode by the
//! `mir_eval` rules, raw and after the cleanup, compare detection with
//! the right profile per part class, count the notes after each cleanup
//! step, and show what the quiet-note guard does to a part with bleed.
//! The set and the results stay local; nothing here is committed.

pub(crate) mod envelope;
mod matcher;
mod signals;

use std::collections::BTreeMap;

use sound_to_patch::audio::SampleRate;
use sound_to_patch::notes::{
    CleanupSteps, DecodeSettings, Detection, NoteList, PitchTrack, Profile, QUIET_GATE,
    REGISTER_WINDOW, Transcriber, clean_up, detect, detect_with_window, drop_quiet,
};
use sound_to_patch::units::{Bpm, Decibels};

use matcher::{EvalNote, Score, octave_errors, offset_score, onset_score, pitch_class_score};
use signals::{Class, Part, RATE, evaluation_set};

/// The layer `cargo xtask eval` scores.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub(crate) enum Layer {
    /// The notes step on parts with known notes.
    Notes,
    /// The envelope layer on parts with known envelopes and pumps.
    Envelope,
}

/// The gates the bleed table tries, in decibels below the loudest frame.
const GATES_DB: [f64; 4] = [30.0, 40.0, 50.0, 60.0];

/// The scores of one decode against the reference.
#[derive(Clone, Copy, Default)]
struct Scores {
    onset: Score,
    offset: Score,
    pitch_class: Score,
    octave_up: usize,
    octave_down: usize,
}

impl Scores {
    fn of(reference: &[EvalNote], estimated: &[EvalNote]) -> Self {
        let (octave_up, octave_down) = octave_errors(reference, estimated);
        Self {
            onset: onset_score(reference, estimated),
            offset: offset_score(reference, estimated),
            pitch_class: pitch_class_score(reference, estimated),
            octave_up,
            octave_down,
        }
    }

    fn add(self, other: Self) -> Self {
        Self {
            onset: self.onset.add(other.onset),
            offset: self.offset.add(other.offset),
            pitch_class: self.pitch_class.add(other.pitch_class),
            octave_up: self.octave_up + other.octave_up,
            octave_down: self.octave_down + other.octave_down,
        }
    }

    fn row(self) -> String {
        format!(
            "{:>4} {:>5.3} {:>5.3} {:>5.3} {:>5.3} {:>5.3} {:>3}",
            self.onset.estimated,
            self.onset.precision(),
            self.onset.recall(),
            self.onset.f1(),
            self.offset.f1(),
            self.pitch_class.f1(),
            self.octave_up + self.octave_down,
        )
    }
}

fn eval_notes(list: &NoteList) -> Vec<EvalNote> {
    list.notes()
        .iter()
        .map(|note| EvalNote {
            onset: note.onset().get(),
            offset: note.offset().get(),
            pitch: note.pitch().get(),
        })
        .collect()
}

/// The settings each part decodes with, by column name.
fn settings(bpm: Bpm, detection: &Detection) -> Vec<(&'static str, DecodeSettings)> {
    let mut columns: Vec<_> = Profile::ALL
        .iter()
        .map(|profile| (profile.name(), profile.settings(bpm)))
        .collect();
    columns.push(("detected", detection.settings));
    columns
}

fn right_profile(class: Class) -> Option<&'static str> {
    match class {
        Class::Bass => Some("bass"),
        Class::Lead => Some("lead"),
        Class::Pad => Some("pad"),
        Class::Silence => None,
    }
}

/// The settings columns besides the profiles: the detected settings with
/// the library's window, and the detection with other register windows.
const WINDOWS: [(&str, Option<f64>); 3] = [
    ("win 15dB", Some(15.0)),
    ("win 25dB", Some(25.0)),
    ("no window", None),
];

/// Every column of the group table, in order.
const COLUMNS: [&str; 7] = [
    "bass",
    "lead",
    "pad",
    "detected",
    "win 15dB",
    "win 25dB",
    "no window",
];

/// The scores and rows the tables print, summed over the parts.
#[derive(Default)]
struct Tally {
    /// Raw and clean scores per class and settings.
    by_class: BTreeMap<(Class, &'static str, bool), Scores>,
    by_group: BTreeMap<(&'static str, &'static str, bool), Score>,
    detections: Vec<String>,
    steps: Vec<String>,
    bleed_rows: Vec<String>,
}

impl Tally {
    /// Runs one part through every column and prints its rows.
    fn part(
        &mut self,
        transcriber: &Transcriber,
        part: &Part,
        rate: SampleRate,
        verbose: bool,
    ) -> crate::Result<()> {
        let bpm = Bpm::new(part.bpm)?;
        let model = transcriber.run(&part.audio, rate)?;
        let track = PitchTrack::of(&part.audio, rate)?;
        let detection = detect(&model, &track, bpm)?;
        let mut columns = settings(bpm, &detection);
        for (name, window) in WINDOWS {
            let window = Decibels::new(window.unwrap_or(1000.0))?;
            columns.push((
                name,
                detect_with_window(&model, &track, bpm, window)?.settings,
            ));
        }
        for (name, decode) in columns {
            let notes = model.decode(&decode)?;
            let scores = Scores::of(&part.notes, &eval_notes(&notes));
            let voicing = Profile::ALL
                .into_iter()
                .find(|profile| profile.name() == name)
                .map_or(detection.voicing, Profile::voicing);
            let (kept, steps) = clean_up(&notes, &model, &track, voicing, &part.audio, rate);
            let clean = Scores::of(&part.notes, &eval_notes(&kept));
            println!(
                "{:<28} {:<9} {} {:>4} {:>5.3} {:>5.3} {:>3}",
                part.name,
                name,
                scores.row(),
                kept.notes().len(),
                clean.onset.recall(),
                clean.onset.f1(),
                clean.octave_up + clean.octave_down
            );
            if name == "detected" {
                self.steps.push(steps_row(part, &steps));
            }
            if name == "bass" {
                check_60(part, &notes, &kept)?;
            }
            if verbose {
                println!("  reference {}", listing(&part.notes));
                println!("  estimated {}", listing(&eval_notes(&notes)));
                println!("  clean     {}", listing(&eval_notes(&kept)));
            }
            if part.class == Class::Silence && !notes.is_empty() {
                return Err(
                    format!("{name} found {} notes in silence", notes.notes().len()).into(),
                );
            }
            for (is_clean, score) in [(false, scores), (true, clean)] {
                let entry = self
                    .by_class
                    .entry((part.class, name, is_clean))
                    .or_default();
                *entry = entry.add(score);
                let entry = self
                    .by_group
                    .entry((group(part), name, is_clean))
                    .or_default();
                *entry = entry.add(score.onset);
            }
            if name == "detected" && !part.bleed.is_empty() {
                self.bleed_rows.extend(bleed_row(part, &notes, rate)?);
            }
        }
        self.detections.push(detection_row(part, &detection));
        Ok(())
    }

    /// Prints the verdict table and returns the classes where detection
    /// scores worse than the right profile.
    fn verdict(&self) -> Vec<&'static str> {
        println!(
            "\nDetection against the right manual profile, onset F1 over each class, raw and clean\n"
        );
        println!(
            "{:<8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8}  verdict",
            "class", "profile", "detected", "pc prof", "pc det", "clean pr", "clean de"
        );
        let mut worse = Vec::new();
        for class in [Class::Bass, Class::Lead, Class::Pad] {
            let Some(profile) = right_profile(class) else {
                continue;
            };
            // A class with no part in the selection has nothing to compare.
            if !self.by_class.contains_key(&(class, profile, false)) {
                continue;
            }
            let get = |name, clean| {
                self.by_class
                    .get(&(class, name, clean))
                    .copied()
                    .unwrap_or_default()
            };
            let (manual, detected) = (get(profile, false), get("detected", false));
            let (manual_clean, detected_clean) = (get(profile, true), get("detected", true));
            let verdict = if detected.onset.f1() + 1e-12 >= manual.onset.f1()
                && detected_clean.onset.f1() + 1e-12 >= manual_clean.onset.f1()
            {
                "no worse"
            } else {
                worse.push(class.name());
                "WORSE"
            };
            println!(
                "{:<8} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.3} {:>8.3}  {verdict}",
                class.name(),
                manual.onset.f1(),
                detected.onset.f1(),
                manual.pitch_class.f1(),
                detected.pitch_class.f1(),
                manual_clean.onset.f1(),
                detected_clean.onset.f1(),
            );
            println!(
                "{:<8} {:>8.3} {:>8.3} {:>8} {:>8} {:>8.3} {:>8.3}  recall",
                "",
                manual.onset.recall(),
                detected.onset.recall(),
                "",
                "",
                manual_clean.onset.recall(),
                detected_clean.onset.recall(),
            );
        }
        worse
    }

    fn groups(&self) {
        println!(
            "\nOnset F1 (matched/estimated/reference) per group, raw and clean: after the octave check,\nthe ghost merge, the attack split, the split join and the {} dB quiet-note guard.\nThe detected settings read the f0 register within {} dB of the loudest frame.\n",
            QUIET_GATE.get(),
            REGISTER_WINDOW.get()
        );
        println!(
            "{:<11} {:<9} {:>22} {:>22}",
            "group", "settings", "raw", "clean"
        );
        for group in ["bass clean", "bass bleed", "bass transient", "lead", "pad"] {
            for name in COLUMNS {
                let cell = |clean| {
                    let score = self
                        .by_group
                        .get(&(group, name, clean))
                        .copied()
                        .unwrap_or_default();
                    format!(
                        "{:.3} ({}/{}/{})",
                        score.f1(),
                        score.matched,
                        score.estimated,
                        score.reference
                    )
                };
                println!(
                    "{group:<11} {name:<9} {:>22} {:>22}",
                    cell(false),
                    cell(true)
                );
            }
        }
    }
}

/// Runs the evaluation and prints its tables. With `only`, it runs the
/// parts whose name holds that text and prints their notes too.
pub(crate) fn run(only: Option<&str>) -> crate::Result<()> {
    let transcriber = Transcriber::new()?;
    let rate = SampleRate::new(RATE)?;
    let parts: Vec<Part> = evaluation_set()
        .into_iter()
        .filter(|part| only.is_none_or(|text| part.name.contains(text)))
        .collect();
    if parts.is_empty() {
        return Err(format!(
            "no part name holds {:?}. Run `cargo xtask eval` without --part to list every part",
            only.unwrap_or_default()
        )
        .into());
    }
    println!("Scores per part (onset tolerance 50 ms, pitch 50 cents, offsets 20 % or 50 ms)\n");
    println!(
        "{:<28} {:<9} {:>4} {:>5} {:>5} {:>5} {:>5} {:>5} {:>3} {:>4} {:>5} {:>5} {:>3}",
        "part",
        "settings",
        "est",
        "P",
        "R",
        "F1",
        "F1off",
        "F1pc",
        "oct",
        "kept",
        "Rc",
        "F1c",
        "oct"
    );
    let mut tally = Tally::default();
    for part in &parts {
        tally.part(&transcriber, part, rate, only.is_some())?;
    }
    let worse = tally.verdict();
    tally.groups();
    println!("\nDetection per part\n");
    println!(
        "{:<28} {:<11} {:>6} {:>13} {:>13} {:>9} {:>6} {:>5}",
        "part", "voicing", "chords", "f0 register", "probe", "range", "frames", "holds"
    );
    for row in &tally.detections {
        println!("{row}");
    }
    println!("\nCleanup steps per part (detected settings): notes after each step\n");
    println!(
        "{:<28} {:>7} {:>5} {:>7} {:>9} {:>6} {:>5} {:>6} {:>5} {:>5}",
        "part",
        "decoded",
        "moved",
        "flagged",
        "unchecked",
        "merged",
        "split",
        "joined",
        "quiet",
        "kept"
    );
    for row in &tally.steps {
        println!("{row}");
    }
    println!("\nQuiet-note guard on the bleed parts (detected settings)\n");
    println!(
        "{:<28} {:>5} {:>6} {:>6} {:>6} {:>6} {:>6}",
        "part", "gate", "kept", "main", "bleed", "drop", "lost"
    );
    for row in &tally.bleed_rows {
        println!("{row}");
    }
    if worse.is_empty() {
        println!("\nDetection scores no worse than the right profile on every class.");
    } else {
        println!(
            "\nDetection scores worse than the right profile on: {}.",
            worse.join(", ")
        );
    }
    Ok(())
}

/// The group of the summary table: bass splits into parts with and
/// without bleed.
fn group(part: &Part) -> &'static str {
    match part.class {
        Class::Bass if part.name.starts_with("bass transient") => "bass transient",
        Class::Bass if part.bleed.is_empty() => "bass clean",
        Class::Bass => "bass bleed",
        Class::Lead => "lead",
        Class::Pad => "pad",
        Class::Silence => "silence",
    }
}

fn listing(notes: &[EvalNote]) -> String {
    notes
        .iter()
        .map(|n| format!("{:.3}-{:.3}:{}", n.onset, n.offset, n.pitch))
        .collect::<Vec<_>>()
        .join(" ")
}

fn pitch_pair(
    pair: Option<(
        sound_to_patch::units::MidiPitch,
        sound_to_patch::units::MidiPitch,
    )>,
) -> String {
    pair.map_or_else(
        || "-".to_owned(),
        |(low, high)| format!("{:.1}-{:.1}", low.get(), high.get()),
    )
}

fn detection_row(part: &Part, detection: &Detection) -> String {
    let range = detection.settings.range;
    let (low, high) = (range.lowest().get(), range.highest().get());
    let holds = part.notes.iter().all(|n| (low..=high).contains(&n.pitch));
    format!(
        "{:<28} {:<11} {:>6.3} {:>13} {:>13} {:>4}-{:<4} {:>6} {:>5}",
        part.name,
        detection.voicing.name(),
        detection.chord_share,
        pitch_pair(detection.f0_register),
        pitch_pair(detection.probe_register),
        low,
        high,
        detection.settings.min_note_frames,
        if holds { "yes" } else { "NO" },
    )
}

/// The goldens of #60 on the bass profile. The dry 110 Hz saw shows the
/// octave ghost at MIDI 57 before the cleanup; after it every note is a
/// true A2, and all 6 match: the model misses the onset of the fourth, and
/// the attack split restores it. Its six notes through reverb give the six
/// true notes.
fn check_60(part: &Part, decoded: &NoteList, clean: &NoteList) -> crate::Result<()> {
    let at_57 = |list: &NoteList| {
        list.notes()
            .iter()
            .filter(|note| (note.pitch().get() - 57.0).abs() < 0.5)
            .count()
    };
    let found = eval_notes(clean);
    let matched = onset_score(&part.notes, &found).matched;
    let fail = |what: String| Err(format!("#60 golden, {}: {what}", part.name).into());
    let expected = match part.name.as_str() {
        "bass 110 Hz release 0.3 s" if at_57(decoded) == 0 => {
            return fail("the decoder gave no ghost at MIDI 57 to remove".to_owned());
        }
        "bass 110 Hz release 0.3 s" | "bass 110 Hz release 0.3 s+reverb" => 6,
        _ => return Ok(()),
    };
    if found.len() == expected && matched == expected {
        Ok(())
    } else {
        fail(format!(
            "{} notes after the cleanup, {matched} of them true, not {expected} true notes alone",
            found.len()
        ))
    }
}

fn steps_row(part: &Part, steps: &CleanupSteps) -> String {
    format!(
        "{:<28} {:>7} {:>5} {:>7} {:>9} {:>6} {:>5} {:>6} {:>5} {:>5}",
        part.name,
        steps.decoded,
        steps.octaves.moved,
        steps.octaves.flagged,
        steps.octaves.unchecked,
        steps.merged,
        steps.split,
        steps.joined,
        steps.quiet,
        steps.kept
    )
}

/// The guard at each gate: notes kept, main notes found, bleed notes left,
/// notes dropped, and main notes the guard lost.
fn bleed_row(part: &Part, notes: &NoteList, rate: SampleRate) -> crate::Result<Vec<String>> {
    let before = eval_notes(notes);
    let main_before = onset_score(&part.notes, &before).matched;
    let mut rows = vec![format!(
        "{:<28} {:>5} {:>6} {:>6} {:>6} {:>6} {:>6}",
        part.name,
        "none",
        before.len(),
        main_before,
        onset_score(&part.bleed, &before).matched,
        0,
        0
    )];
    for gate in GATES_DB {
        let (kept, dropped) = drop_quiet(notes, &part.audio, rate, Decibels::new(gate)?);
        let after = eval_notes(&kept);
        let main = onset_score(&part.notes, &after).matched;
        rows.push(format!(
            "{:<28} {:>5} {:>6} {:>6} {:>6} {:>6} {:>6}",
            "",
            gate,
            after.len(),
            main,
            onset_score(&part.bleed, &after).matched,
            dropped,
            main_before - main
        ));
    }
    Ok(rows)
}
