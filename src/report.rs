//! The outputs of a stem: `layers.json`, `spectrogram.png` and the final
//! `notes.mid` from `analyze`, and the first `notes.mid` from `notes`.
//!
//! [`analyze`] decodes the stem, refines its tempo, reads the note list if
//! one is given or else runs the notes step, reads the pitch layer, runs
//! the stem estimators and the descriptors, maps the pitch, voices and
//! envelope groups to the synth's suggested edits, rewrites the note list
//! with the layers into the final notes, and writes the three files into
//! the output folder.
//! [`notes`] refines the tempo, transcribes the stem and writes its notes
//! into the same folder. The schema of `layers.json` is committed as
//! `schemas/layers.schema.json`.

use std::error::Error;
use std::fmt;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use crate::audio::{self, Audio, BeatGrid, DecodeError, SampleRate, SkippedPacket, Stem, Tempo};
use crate::confidence::ConfidenceError;
use crate::confidence::Level;
use crate::dsp::{StftParams, Summary, describe};
use crate::estimate::envelope::{
    EnvelopeError, EnvelopeInput, EnvelopeLayer, KNEE_END_GAP, NoteEnvelope, SUSTAIN_SHARE,
    amp_proxy, full_band_level, full_band_track,
};
use crate::estimate::pitch::{PitchError, PitchInput, PitchLayer};
use crate::estimate::voices::{VoicesError, VoicesInput, VoicesLayer};
use crate::notes::{
    DEFAULT_VELOCITY, DropSpan, FinalNotes, FinishInput, GlideSpan, MidiError, MidiWriteError,
    Note, NoteFacts, NoteList, NotesError, PAD_FALL, PitchTrack, Profile, QUIET_GATE, Transcriber,
    Transcription, Velocities, VibratoSpan, Voicing, finish, midi_track_name, read_midi_near,
    transcribe_stem, voicing_of_notes, write_named_midi,
};
use crate::preset::mapper::pitch_envelope::velocity_reaches_level;
use crate::preset::mapper::{MapContext, Mapped, PitchEnvelopeGroups, SynthMapper, mapper_for};
use crate::preset::registry::registry_for;
use crate::synth::{Synth, capabilities};
use crate::units::{Bpm, Hz, Seconds};

mod layers;
pub mod spectrogram;

pub use layers::{SCHEMA, layers_json};
pub use spectrogram::Spectrogram;

/// The report file name.
pub const LAYERS_FILE: &str = "layers.json";
/// The spectrogram file name.
pub const SPECTROGRAM_FILE: &str = "spectrogram.png";
/// The notes file name.
pub const NOTES_FILE: &str = "notes.mid";

/// The output folder of a stem: `out`, or `<stem folder>/<stem file stem>/`.
/// A file stem of `.` or `..` would name the stem's folder or its parent,
/// and a name without an extension, such as `.wav` or `bass`, would name
/// the stem file itself, so such a stem needs `out`.
fn out_dir(stem: &Path, out: Option<&Path>) -> Result<PathBuf, AnalyzeError> {
    if let Some(out) = out {
        return Ok(out.to_path_buf());
    }
    match (stem.file_stem(), stem.extension()) {
        (Some(name), Some(_)) if name != "." && name != ".." => Ok(stem.with_file_name(name)),
        (name, extension) => Err(AnalyzeError::NoFolderName {
            source: source_of(stem),
            stem: extension
                .and(name)
                .map(|name| name.to_string_lossy().into_owned()),
        }),
    }
}

/// The file name of a stem, as a report records it.
fn source_of(stem: &Path) -> String {
    stem.file_name()
        .unwrap_or(stem.as_os_str())
        .to_string_lossy()
        .into_owned()
}

/// What [`analyze`] reads and where it writes.
#[derive(Clone, Copy, Debug)]
pub struct AnalyzeRequest<'a> {
    /// The audio stem.
    pub stem: &'a Path,
    /// The tempo of the stem, refined from the stem unless `bpm_exact`.
    pub bpm: Bpm,
    /// Use `bpm` as it is.
    pub bpm_exact: bool,
    /// The target synth, written into the report.
    pub synth: Synth,
    /// A MIDI file with the stem's notes. Without it the notes step runs.
    pub notes: Option<&'a Path>,
    /// The profile that overrides detection in the notes step, if any. The
    /// notes step does not run when `notes` gives the notes, so the profile
    /// then has no effect; the command line rejects the two together.
    pub profile: Option<Profile>,
    /// The output folder; by default `<stem folder>/<stem file stem>/`.
    pub out: Option<&'a Path>,
    /// Replace files in the output folder that no report of this stem
    /// owns: a report or notes of another stem, a report of the same name
    /// with other content, a file that is not this tool's, or a
    /// `spectrogram.png` without a report.
    pub force: bool,
}

impl AnalyzeRequest<'_> {
    /// Returns the output folder.
    ///
    /// # Errors
    ///
    /// [`AnalyzeError::NoFolderName`] without `out` for a stem whose name
    /// has no extension or is `.` or `..` without it.
    pub fn out_dir(&self) -> Result<PathBuf, AnalyzeError> {
        out_dir(self.stem, self.out)
    }

    /// Returns the file name of the stem, as the report records it.
    #[must_use]
    pub fn source(&self) -> String {
        source_of(self.stem)
    }
}

/// What [`analyze`] found and wrote.
#[derive(Clone, Debug)]
pub struct Analysis {
    /// The path of `layers.json`.
    pub layers: PathBuf,
    /// The path of `spectrogram.png`.
    pub spectrogram: PathBuf,
    /// The path of the final `notes.mid`; `None` without notes.
    pub notes: Option<PathBuf>,
    /// The final note list that `notes.mid` holds; `None` without notes.
    pub final_notes: Option<FinalNotes>,
    /// The given, refined and used tempo.
    pub tempo: Tempo,
    /// The notes step, `None` when `--notes` gave the notes.
    pub transcription: Option<Transcription>,
    /// The pitch layer: glides, pitch drops and vibrato.
    pub pitch: PitchLayer,
    /// The voices layer: the count of voices and the joins of a mono line.
    pub voices: VoicesLayer,
    /// The envelope layer: the amplitude envelope and the pump.
    pub envelope: EnvelopeLayer,
    /// The packets that did not decode and became silence.
    pub skipped: Vec<SkippedPacket>,
    /// What the analysis leaves out of the file; see [`audio::Decoded::warnings`].
    pub warnings: Vec<String>,
}

impl Analysis {
    /// Returns the lines that tell what each step found, in order: the
    /// tempo, the notes step when it ran, the pitch layer, the
    /// envelope layer, then the voices layer.
    #[must_use]
    pub fn summary(&self) -> Vec<String> {
        std::iter::once(self.tempo.summary())
            .chain(self.transcription.iter().flat_map(transcription_summary))
            .chain(self.pitch.summary())
            .chain(self.envelope.summary())
            .chain(self.voices.summary())
            .collect()
    }
}

/// The f0 track for given notes: the pitch layer reads it, and skips a
/// list that is empty or plays chords without it.
fn given_track(
    transcriber: &Transcriber,
    mono: &[f64],
    rate: audio::SampleRate,
    notes: &NoteList,
) -> Result<PitchTrack, AnalyzeError> {
    if notes.is_empty() || voicing_of_notes(notes) == Voicing::Polyphonic {
        return Ok(PitchTrack::default());
    }
    transcriber
        .track(mono, rate)
        .map_err(|error| AnalyzeError::Transcription(NotesError::Track(error)))
}

/// Analyses a stem and writes `layers.json`, `spectrogram.png` and, when
/// the stem has notes, the final `notes.mid` into the output folder,
/// creating it.
///
/// The tempo is refined from the stem unless the request says `bpm_exact`.
/// Without a note list the stem runs through [`transcribe_stem`] once,
/// with `transcriber`; with one, the model never runs, but the f0 track
/// of `transcriber` still reads the stem for the pitch layer.
///
/// The files are replaced; nothing else in the folder changes. The stem
/// is read once, for its checksum and for the decoder. Each file goes to
/// a hidden temporary sibling and is renamed into place, `notes.mid`
/// first and `layers.json` last. The old report moves aside before any
/// file changes, so a `layers.json` that exists always belongs to the
/// files next to it, and a failure puts the old files back. The final
/// `notes.mid` replaces the first one of `notes`.
///
/// # Errors
///
/// Returns an [`AnalyzeError`] when the stem gives no folder name and no
/// `out` is given, when the output folder is a file, or, without `force`,
/// when it holds a report or notes of another stem, a report of the same
/// name with other content, a `layers.json` or `notes.mid` that is not
/// this tool's, a `spectrogram.png` without a report, or the backup that a
/// killed run left; when the note list or the stem cannot be read, when the
/// notes step fails, or when an output file cannot be written.
pub fn analyze(
    request: &AnalyzeRequest<'_>,
    transcriber: &Transcriber,
) -> Result<Analysis, AnalyzeError> {
    analyze_with(request, transcriber, mapper_for(request.synth).ok())
}

/// [`analyze`] with a given mapper for the suggested edits, such as a test
/// mapper. Without one, or for a synth without a registry, every
/// `suggested_edits` and `unrepresentable` list is empty.
///
/// # Errors
///
/// As [`analyze`].
pub fn analyze_with(
    request: &AnalyzeRequest<'_>,
    transcriber: &Transcriber,
    mapper: Option<&dyn SynthMapper>,
) -> Result<Analysis, AnalyzeError> {
    let dir = request.out_dir()?;
    let source = request.source();
    let (bytes, checksum, backups) =
        guard(&dir, request.stem, &source, request.force, Guarded::Report)?;
    let decoded = audio::decode_bytes(request.stem, bytes).map_err(AnalyzeError::Decode)?;
    let skipped = decoded.skipped().to_vec();
    let warnings = decoded.warnings();
    let stem_audio = decoded.into_audio();
    let mono = stem_audio.mono();
    let tempo = tempo_of(
        &mono,
        stem_audio.sample_rate(),
        request.bpm,
        request.bpm_exact,
    )?;
    let (notes, transcription, track) = if let Some(path) = request.notes {
        let notes =
            read_midi_near(path, tempo.given(), tempo.trusted()).map_err(AnalyzeError::Notes)?;
        let track = given_track(transcriber, &mono, stem_audio.sample_rate(), &notes)?;
        (Some(notes), None, Some(track))
    } else {
        let transcription = transcribe_stem(
            transcriber,
            &mono,
            stem_audio.sample_rate(),
            tempo.used(),
            request.profile,
        )
        .map_err(AnalyzeError::Transcription)?;
        // A stem without notes keeps the frame rule of a stem without a list.
        let notes = (!transcription.notes.is_empty()).then(|| transcription.notes.clone());
        (notes, Some(transcription), None)
    };
    let empty = NoteList::default();
    let note_list = notes.as_ref().unwrap_or(&empty);
    let grid = BeatGrid::new(tempo.used(), Seconds::clamped(0.0));
    let voicing = transcription
        .as_ref()
        .map_or_else(|| voicing_of_notes(note_list), |t| t.voicing);
    let (pitch, envelope, voices, mono_line) = read_layers(
        &mono,
        &stem_audio,
        note_list,
        track
            .as_ref()
            .or(transcription.as_ref().map(|t| &t.track))
            .unwrap_or(&PitchTrack::default()),
        // The profile counts only where the notes step ran: given notes
        // take no profile.
        (
            voicing,
            transcription.as_ref().and_then(|t| t.profile) == Some(Profile::Pad),
        ),
        grid,
    )?;
    let final_notes = final_notes(
        &mono,
        stem_audio.sample_rate(),
        note_list,
        (&pitch, &envelope),
        mono_line,
        tempo.used(),
    )?;
    // The stem and descriptor steps read the channels, not the mono mix.
    drop(mono);
    let stem = Stem::new(stem_audio, grid, notes.as_ref())?;
    let params = StftParams::fixed(stem.frame_len());
    let descriptors = describe(
        stem.mid(),
        stem.sample_rate(),
        params,
        stem.median_f0(),
        stem.hard_limited(),
    )?;
    let read = (&pitch, &envelope, &voices);
    let json = layers_json(
        &stem,
        &descriptors,
        request.synth,
        (&source, &checksum),
        &tempo,
        (
            transcription.as_ref().map(|t| &t.record),
            final_notes.as_ref(),
        ),
        (read, &map_groups(mapper, request.synth, read)),
    )?;
    let png = Spectrogram::of(stem.mid(), stem.sample_rate()).to_png()?;

    let midi = final_notes.as_ref().map(|list| (list, source.as_str()));
    let (layers, spectrogram, notes_file) = write_outputs(&dir, &json, &png, midi)?;
    remove_backups(&backups);
    Ok(Analysis {
        layers,
        spectrogram,
        notes: notes_file,
        final_notes,
        tempo,
        transcription,
        pitch,
        voices,
        envelope,
        skipped,
        warnings,
    })
}

/// Writes the files of `analyze` into `dir`, creating it, through
/// [`replace_files`]: `notes.mid`, or its removal when there are no notes,
/// then `spectrogram.png`, then `layers.json`. Returns the paths of
/// `layers.json`, `spectrogram.png` and `notes.mid` when written. The
/// notes, when given, are encoded with the stem's name as the track name.
fn write_outputs(
    dir: &Path,
    json: &str,
    png: &[u8],
    notes: Option<(&FinalNotes, &str)>,
) -> Result<(PathBuf, PathBuf, Option<PathBuf>), AnalyzeError> {
    let midi = notes
        .map(|(list, name)| list.to_midi(Some(name)))
        .transpose()
        .map_err(AnalyzeError::Midi)?;
    // The guard has passed, so a `notes.mid` here is this stem's or one that
    // `--force` replaces. A run without notes leaves none that the report
    // does not describe.
    replace_files(
        dir,
        &[
            (NOTES_FILE, midi.as_deref()),
            (SPECTROGRAM_FILE, Some(png)),
            (LAYERS_FILE, Some(json.as_bytes())),
        ],
    )?;
    Ok((
        dir.join(LAYERS_FILE),
        dir.join(SPECTROGRAM_FILE),
        midi.map(|_| dir.join(NOTES_FILE)),
    ))
}

/// Creates the output folder and its parents, and returns the folders it
/// created, the deepest first.
fn create_dir(dir: &Path) -> Result<Vec<PathBuf>, AnalyzeError> {
    let created: Vec<PathBuf> = dir
        .ancestors()
        .take_while(|folder| !folder.as_os_str().is_empty() && !folder.exists())
        .map(Path::to_path_buf)
        .collect();
    if let Err(source) = std::fs::create_dir_all(dir) {
        remove_created(&created);
        return Err(AnalyzeError::Write {
            path: dir.to_path_buf(),
            source,
        });
    }
    Ok(created)
}

/// Removes the folders that [`create_dir`] created, the deepest first.
/// Only an empty folder goes: `remove_dir` keeps one with files.
fn remove_created(created: &[PathBuf]) {
    for folder in created {
        let _ = std::fs::remove_dir(folder);
    }
}

/// A file of a write: its name in the folder, and its new bytes or `None`
/// to remove it.
type Change<'a> = (&'static str, Option<&'a [u8]>);

/// A step of [`replace_files`] that a failure undoes.
enum Undo {
    /// An old file moved to a hidden backup; undo renames it back, and
    /// success drops the backup, which removes it.
    MovedAside {
        /// The backup.
        backup: tempfile::TempPath,
        /// Where the file was.
        to: PathBuf,
    },
    /// A new file landed here; undo removes it.
    Placed(PathBuf),
}

impl Undo {
    fn revert(self) {
        match self {
            Self::MovedAside { backup, to } => {
                // A backup that cannot go back stays on disk, so the old
                // file is hidden but not lost.
                if let Err(error) = backup.persist(&to) {
                    let _ = error.path.keep();
                }
            }
            Self::Placed(path) => {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// Applies `changes` to `dir`, creating it, such that a failure leaves the
/// folder as it was and success leaves exactly the new files. Each new file
/// is first written to a hidden temporary sibling with a name of its own,
/// so two runs never share one. Then each old file moves aside to a hidden
/// backup and the new one is renamed into place, in order, except that an
/// old `layers.json` moves aside before any other file changes and the new
/// one lands last: a `layers.json` that exists always belongs to the files
/// next to it, and none is ever cut short. A failure renames every backup
/// back, removes every placed file and every folder this call created; the
/// temporary files remove themselves. A process killed during the renames
/// can leave a backup, `.<name>.<random>.old`, that holds the old file.
fn replace_files(dir: &Path, changes: &[Change<'_>]) -> Result<(), AnalyzeError> {
    let created = create_dir(dir)?;
    let result = swap_files(dir, changes);
    if result.is_err() {
        remove_created(&created);
    }
    result
}

/// [`replace_files`] in a folder that exists.
fn swap_files(dir: &Path, changes: &[Change<'_>]) -> Result<(), AnalyzeError> {
    let mut staged = changes
        .iter()
        .map(|&(name, bytes)| Ok((name, bytes.map(|b| stage(dir, name, b)).transpose()?)))
        .collect::<Result<Vec<_>, AnalyzeError>>()?;
    // The report goes last, after every file it describes.
    staged.sort_by_key(|&(name, _)| name == LAYERS_FILE);
    let mut undo = Vec::new();
    let report = staged.iter().any(|&(name, _)| name == LAYERS_FILE);
    let result = (|| {
        if report {
            move_aside(dir, LAYERS_FILE, &mut undo)?;
        }
        for (name, new) in staged {
            if name != LAYERS_FILE {
                move_aside(dir, name, &mut undo)?;
            }
            if let Some(new) = new {
                let to = dir.join(name);
                new.persist(&to).map_err(|error| AnalyzeError::Write {
                    path: to.clone(),
                    source: error.error,
                })?;
                undo.push(Undo::Placed(to));
            }
        }
        Ok(())
    })();
    if result.is_err() {
        for step in undo.into_iter().rev() {
            step.revert();
        }
    }
    result
}

/// Moves the file `name` in `dir` to a hidden backup, if it exists. A
/// folder in its place stays, so the rename of the new file onto it fails.
fn move_aside(dir: &Path, name: &str, undo: &mut Vec<Undo>) -> Result<(), AnalyzeError> {
    let path = dir.join(name);
    let fail = |source| AnalyzeError::Write {
        path: path.clone(),
        source,
    };
    match std::fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(fail(error)),
        Ok(meta) if meta.is_dir() => return Ok(()),
        Ok(_) => {}
    }
    let backup = tempfile::Builder::new()
        .prefix(&format!(".{name}."))
        .suffix(".old")
        .make_in(dir, |backup| std::fs::rename(&path, backup))
        .map_err(fail)?;
    undo.push(Undo::MovedAside {
        backup: backup.into_temp_path(),
        to: path,
    });
    Ok(())
}

/// The mapper's edits for the pitch, voices and envelope groups.
fn map_groups(
    mapper: Option<&dyn SynthMapper>,
    synth: Synth,
    (pitch, envelope, voices): (&PitchLayer, &EnvelopeLayer, &VoicesLayer),
) -> Mapped {
    let (Some(mapper), Ok(registry), Ok(capabilities)) =
        (mapper, registry_for(synth), capabilities(synth))
    else {
        return Mapped::default();
    };
    let ctx = MapContext {
        registry,
        capabilities,
    };
    let groups = PitchEnvelopeGroups::from_layers(pitch, voices, envelope);
    mapper.pitch_envelope(&groups, &ctx)
}

/// The final note list: the list rewritten with the layers; `None` for an
/// empty list. Only the records the mapper writes into the preset, those
/// above low confidence in a group above low confidence, leave the notes
/// as routes.
fn final_notes(
    mono: &[f64],
    rate: SampleRate,
    notes: &NoteList,
    (pitch, envelope): (&PitchLayer, &EnvelopeLayer),
    mono_line: bool,
    bpm: Bpm,
) -> Result<Option<FinalNotes>, AnalyzeError> {
    if notes.is_empty() {
        return Ok(None);
    }
    let facts = note_facts(mono, rate, notes, envelope, !mono_line)?;
    let group = pitch.confidence.level();
    let confident = |confidence: &crate::confidence::Confidence<_>| {
        group
            .min(confidence.level())
            .is_some_and(|level| level > Level::Low)
    };
    let glides: Vec<GlideSpan> = pitch
        .glides
        .iter()
        .filter(|g| confident(&g.confidence))
        .map(|g| GlideSpan {
            start: g.value.start,
            interval: g.value.interval,
            time: g.value.time,
        })
        .collect();
    let drops: Vec<DropSpan> = pitch
        .drops
        .iter()
        .filter(|d| confident(&d.confidence))
        .map(|d| DropSpan {
            onset: d.value.onset,
            f_end: d.value.f_end,
            df_hz: d.value.df_hz,
            settled: d.value.settled(),
        })
        .collect();
    let vibrato: Vec<VibratoSpan> = pitch
        .vibrato
        .iter()
        .filter(|v| confident(&v.confidence))
        .map(|v| VibratoSpan {
            start: v.value.start,
            end: v.value.end,
            pitch: v.value.pitch,
        })
        .collect();
    let level = |from, to, f0| full_band_level(mono, rate, (from, to), f0);
    Ok(Some(finish(&FinishInput {
        notes,
        facts: &facts,
        bpm,
        mono: mono_line,
        level_velocity: velocity_reaches_level(envelope.level_spread.value().copied()),
        glides: &glides,
        drops: &drops,
        vibrato: &vibrato,
        level: &level,
    })))
}

/// The knee, the pad end and the full-band level of each note of the
/// list. The envelope layer owns the full-band level.
fn note_facts(
    mono: &[f64],
    rate: SampleRate,
    notes: &NoteList,
    envelope: &EnvelopeLayer,
    pad: bool,
) -> Result<Vec<NoteFacts>, AnalyzeError> {
    let list = notes.notes();
    list.iter()
        .enumerate()
        .map(|(i, note)| {
            let read = envelope
                .notes
                .iter()
                .find(|n| n.onset == note.onset() && n.pitch == note.pitch());
            let pad_end = match read {
                Some(read) if pad => pad_end(mono, rate, note, list, read)?,
                None if pad => full_band_pad_end(mono, rate, note),
                _ => None,
            };
            Ok(NoteFacts {
                knee: read.and_then(|n| n.knee),
                pad_end,
                level: envelope.levels.get(i).copied().flatten(),
                read: read.is_some(),
            })
        })
        .collect()
}

/// Where a pad note's amp proxy falls [`PAD_FALL`] below its plateau, the
/// peak plus the sustain level, first after its knee or else its peak;
/// `None` without a sustain, a proxy or such a fall.
fn pad_end(
    mono: &[f64],
    rate: SampleRate,
    note: &Note,
    list: &[Note],
    read: &NoteEnvelope,
) -> Result<Option<Seconds>, AnalyzeError> {
    let Some(sustain) = read.sustain else {
        return Ok(None);
    };
    let others: Vec<&Note> = list
        .iter()
        .filter(|o| *o != note && o.onset() < note.offset() && note.onset() < o.offset())
        .collect();
    let Some(proxy) = amp_proxy(mono, rate, note, &others)
        .map_err(|error| AnalyzeError::Envelope(error.into()))?
    else {
        return Ok(None);
    };
    let db = proxy.db();
    let Some((peak_at, peak)) = db
        .iter()
        .copied()
        .enumerate()
        .reduce(|a, b| if b.1 > a.1 { b } else { a })
    else {
        return Ok(None);
    };
    let floor = peak + sustain.get() - PAD_FALL.get();
    let from = read.knee.map_or(peak_at, |knee| proxy.index(knee.get()));
    Ok(db
        .iter()
        .enumerate()
        .skip(from)
        .find(|&(_, &level)| level <= floor)
        .map(|(i, _)| Seconds::clamped(proxy.time(i))))
}

/// Where the full-band level of a pad note that the envelope layer could
/// not read falls [`PAD_FALL`] below its plateau, first after its peak.
/// The plateau is the median of the last [`SUSTAIN_SHARE`] of the held
/// part, from the first frame within 1 dB of the peak to
/// [`KNEE_END_GAP`] before the note's end, as the envelope layer reads a
/// sustain; `None` without a held part or such a fall.
fn full_band_pad_end(mono: &[f64], rate: SampleRate, note: &Note) -> Option<Seconds> {
    let track = full_band_track(
        mono,
        rate,
        (note.onset(), note.offset()),
        Hz::from(note.pitch()),
    );
    let db = track.db();
    let (peak_at, peak) = db
        .iter()
        .copied()
        .enumerate()
        .reduce(|a, b| if b.1 > a.1 { b } else { a })?;
    let held_from = db.iter().position(|&level| level >= peak - 1.0)?;
    let held_to = track.index(note.offset().get() - KNEE_END_GAP.get());
    let held = db.get(held_from..held_to).filter(|part| !part.is_empty())?;
    let window = crate::cast::floor_to_usize(SUSTAIN_SHARE * crate::cast::usize_to_f64(held.len()))
        .unwrap_or(0)
        .clamp(1, held.len());
    let plateau = Summary::of(&held[held.len() - window..])?.median();
    let floor = plateau - PAD_FALL.get();
    db.iter()
        .enumerate()
        .skip(peak_at)
        .find(|&(_, &level)| level <= floor)
        .map(|(i, _)| Seconds::clamped(track.time(i)))
}

/// The first two channels of a stem that has them.
fn two_channels(channels: &[Vec<f64>]) -> Option<(&[f64], &[f64])> {
    match channels {
        [left, right, ..] => Some((left, right)),
        _ => None,
    }
}

/// Reads the pitch, envelope and voices layers of the mono mix, and the
/// one voicing of the final notes: a mono line holds at most one voice at
/// a time by the voices layer, or the layer counts none, and `--profile
/// pad` makes any line polyphonic when the notes step ran. The envelope's full-band levels read that voicing, so
/// they wait for the voices layer. `voicing` is the notes step's, which
/// the pitch layer reads.
fn read_layers(
    mono: &[f64],
    audio: &Audio,
    notes: &NoteList,
    track: &PitchTrack,
    (voicing, pad_profile): (Voicing, bool),
    grid: BeatGrid,
) -> Result<(PitchLayer, EnvelopeLayer, VoicesLayer, bool), AnalyzeError> {
    let rate = audio.sample_rate();
    let pitch = PitchLayer::of(&PitchInput {
        mono,
        rate,
        track,
        notes,
        voicing,
    })
    .map_err(AnalyzeError::Pitch)?;
    let input = EnvelopeInput {
        mono,
        rate,
        notes,
        grid,
        stereo: two_channels(audio.channels()),
    };
    let mut envelope = EnvelopeLayer::of(&input).map_err(AnalyzeError::Envelope)?;
    let voices = VoicesLayer::of(&VoicesInput {
        mono,
        rate,
        notes,
        envelope: &envelope,
    })
    .map_err(AnalyzeError::Voices)?;
    // A layer that counted no voices leaves the line mono: the mono rules
    // only join and cut notes, while the pad rule reads what the count
    // could not.
    let mono_line = (voices.mono || !voices.confidence.is_estimate()) && !pad_profile;
    envelope
        .read_levels(&input, mono_line)
        .map_err(AnalyzeError::Envelope)?;
    Ok((pitch, envelope, voices, mono_line))
}

/// The tempo of a stem: `bpm` as given when `exact`, else refined from
/// the mono mix.
fn tempo_of(mono: &[f64], rate: SampleRate, bpm: Bpm, exact: bool) -> Result<Tempo, AnalyzeError> {
    Ok(if exact {
        Tempo::exact(bpm)?
    } else {
        Tempo::refine(mono, rate, bpm)?
    })
}

/// What [`notes`] reads and where it writes.
#[derive(Clone, Copy, Debug)]
pub struct NotesRequest<'a> {
    /// The audio stem.
    pub stem: &'a Path,
    /// The tempo of the stem, refined from the stem unless `bpm_exact`.
    pub bpm: Bpm,
    /// Use `bpm` as it is.
    pub bpm_exact: bool,
    /// The profile that overrides detection, if any.
    pub profile: Option<Profile>,
    /// The output folder; by default `<stem folder>/<stem file stem>/`.
    pub out: Option<&'a Path>,
    /// Write next to a report that is not this stem's, replace a
    /// `notes.mid` of another stem or with no track name, or replace the
    /// final notes and remove the report that describes them.
    pub force: bool,
}

/// What [`notes`] found and wrote.
#[derive(Clone, Debug)]
pub struct NotesReport {
    /// The path of `notes.mid`.
    pub path: PathBuf,
    /// The given, refined and used tempo.
    pub tempo: Tempo,
    /// The notes step: the model run, the track, the settings and the
    /// counts of each cleanup step.
    pub transcription: Transcription,
    /// The packets that did not decode and became silence.
    pub skipped: Vec<SkippedPacket>,
    /// What the analysis leaves out of the file; see [`audio::Decoded::warnings`].
    pub warnings: Vec<String>,
}

impl NotesReport {
    /// Returns the lines that tell what each step found and kept, in
    /// order: the tempo, then the notes step.
    #[must_use]
    pub fn summary(&self) -> Vec<String> {
        std::iter::once(self.tempo.summary())
            .chain(transcription_summary(&self.transcription))
            .collect()
    }
}

/// The lines of the notes step: what detection found, the settings and the
/// count after each cleanup step.
fn transcription_summary(t: &Transcription) -> Vec<String> {
    let detection = &t.detection;
    let range = |range: crate::notes::PitchRange| {
        format!("MIDI {} to {}", range.lowest().get(), range.highest().get())
    };
    let settings = &t.settings;
    let steps = &t.steps;
    vec![
        format!(
            "detected range: {}, from the {}",
            range(detection.settings.range),
            detection.source.name()
        ),
        format!(
            "detected voicing: {}, chord share {:.2}",
            detection.voicing.name(),
            detection.chord_share
        ),
        format!("register window: {} dB", detection.window.get()),
        format!(
            "cleanup voicing: {}, {}",
            t.voicing.name(),
            t.profile.map_or_else(
                || "as detected".to_owned(),
                |p| format!("from --profile {p}")
            )
        ),
        format!(
            "settings: {}, {}, minimum note {} frames ({:.1} ms), melodia {}",
            t.profile
                .map_or_else(|| "detected".to_owned(), |p| format!("profile {p}")),
            range(settings.range),
            settings.min_note_frames,
            settings.note_params().minimum_note_length_ms,
            if settings.melodia { "on" } else { "off" }
        ),
        format!("decoded: {} notes", steps.decoded),
        format!(
            "octave check: {} notes, {} moved down an octave, {} flagged, {} unchecked",
            steps.decoded, steps.octaves.moved, steps.octaves.flagged, steps.octaves.unchecked
        ),
        format!("ghost merge: {} notes", steps.merged),
        format!("attack split: {} notes", steps.split),
        format!("split join: {} notes", steps.joined),
        format!(
            "quiet-note guard: {} notes, {} dropped below {} dB",
            steps.kept,
            steps.quiet,
            QUIET_GATE.get()
        ),
    ]
}

/// Transcribes a stem and writes `notes.mid` into the output folder,
/// creating it.
///
/// The tempo is refined from the stem unless the request says
/// `bpm_exact`. The stem runs through [`transcribe_stem`] at that tempo
/// with the detected settings, or those of the profile, and the file
/// carries that tempo. The file is written to a hidden temporary sibling
/// and renamed into place, so a failure leaves the old file. Nothing else
/// in the folder changes, except that `force` removes a `layers.json`
/// that describes the final notes once the new file is in place, so no
/// report describes a file that is gone.
///
/// # Errors
///
/// Returns an [`AnalyzeError`] when the stem gives no folder name and no
/// `out` is given, when the output folder is a file, or, without `force`,
/// when it holds a `layers.json` that describes the final notes, a report
/// or notes of another stem, a report of the same name with other content,
/// a `layers.json` or `notes.mid` that is not this tool's, or the backup
/// that a killed run left; when the stem cannot be decoded or transcribed,
/// when it gives no note, or when the file cannot be written.
pub fn notes(
    request: &NotesRequest<'_>,
    transcriber: &Transcriber,
) -> Result<NotesReport, AnalyzeError> {
    let dir = out_dir(request.stem, request.out)?;
    let source = source_of(request.stem);
    // A report that describes the final notes would describe a file this
    // command replaces, so it stops the run, whatever its stem; `--force`
    // removes it with the old file.
    let report = dir.join(LAYERS_FILE);
    let describes = describes_notes(&report);
    if describes && !request.force {
        return Err(AnalyzeError::NotesDescribed { path: report });
    }
    let (stem_bytes, _, backups) = guard(
        &dir,
        request.stem,
        &source,
        request.force,
        Guarded::ReportBeside,
    )?;
    let decoded = audio::decode_bytes(request.stem, stem_bytes).map_err(AnalyzeError::Decode)?;
    let skipped = decoded.skipped().to_vec();
    let warnings = decoded.warnings();
    let stem_audio = decoded.into_audio();
    let mono = stem_audio.mono();
    let tempo = tempo_of(
        &mono,
        stem_audio.sample_rate(),
        request.bpm,
        request.bpm_exact,
    )?;
    let transcription = transcribe_stem(
        transcriber,
        &mono,
        stem_audio.sample_rate(),
        tempo.used(),
        request.profile,
    )
    .map_err(AnalyzeError::Transcription)?;
    if transcription.notes.is_empty() {
        return Err(AnalyzeError::NoNotes { source });
    }
    let bytes = write_named_midi(
        &transcription.notes,
        tempo.used(),
        Velocities::Constant(DEFAULT_VELOCITY),
        &source,
    )
    .map_err(AnalyzeError::Midi)?;
    // The report that describes the final notes goes with them: removed
    // only once the new file lands, and kept when the write fails.
    let mut changes: Vec<Change<'_>> = Vec::with_capacity(2);
    if describes {
        changes.push((LAYERS_FILE, None));
    }
    changes.push((NOTES_FILE, Some(&bytes)));
    replace_files(&dir, &changes)?;
    remove_backups(&backups);
    let path = dir.join(NOTES_FILE);
    Ok(NotesReport {
        path,
        tempo,
        transcription,
        skipped,
        warnings,
    })
}

/// Whether a `layers.json` describes the final notes, the `notes.mid` that
/// `analyze` writes next to it.
fn describes_notes(report: &Path) -> bool {
    std::fs::read_to_string(report)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .is_some_and(|report| report.get("notes").is_some_and(|notes| !notes.is_null()))
}

/// Runs the guards of the output folder for the stem file `stem` named
/// `source` and returns the bytes of the file, their SHA-256 and the
/// backups that a forced run removes once its files are in place. The
/// guards that read only names run before the stem is read, so a refusal
/// answers first. The stem is read once, so the decoder gets the bytes the
/// checksum read, as a pipe allows. `guarded` is [`Guarded::Report`] for
/// `analyze`, which also guards `spectrogram.png`, and
/// [`Guarded::ReportBeside`] for `notes`.
fn guard(
    dir: &Path,
    stem: &Path,
    source: &str,
    force: bool,
    guarded: Guarded,
) -> Result<(Vec<u8>, String, Vec<PathBuf>), AnalyzeError> {
    let recorded = check_out_dir(dir, source, force, guarded)?;
    let backups = check_backups(dir, force)?;
    if guarded == Guarded::Report {
        check_spectrogram(dir, force)?;
    }
    check_notes_file(dir, source, force)?;
    let bytes = audio::read_stem(stem).map_err(AnalyzeError::Decode)?;
    let checksum = crate::render::sha256_hex(&bytes);
    if recorded.is_some_and(|recorded| recorded != checksum) {
        return Err(AnalyzeError::OtherAudio {
            path: dir.join(LAYERS_FILE),
            source: source.to_owned(),
            guarded,
        });
    }
    Ok((bytes, checksum, backups))
}

/// The backups of report files in the folder, `.<file>.<random>.old`,
/// with the file each backs up. A run that was killed during its renames
/// left them. With `force` off, fails with every backup when one of them
/// stands for a file that the folder lacks, and otherwise returns none, so
/// a plain run removes no backup. With `force` on, returns every backup,
/// so the forced run removes them once its new files are in place.
fn check_backups(dir: &Path, force: bool) -> Result<Vec<PathBuf>, AnalyzeError> {
    // A folder that does not exist yet holds no backup.
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut names: Vec<String> = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let backups: Vec<(PathBuf, &'static str)> = [LAYERS_FILE, NOTES_FILE, SPECTROGRAM_FILE]
        .into_iter()
        .flat_map(|file| {
            let prefix = format!(".{file}.");
            names
                .iter()
                .filter(move |name| {
                    // The suffix is the one `move_aside` writes, so case matters.
                    name.starts_with(&prefix)
                        && Path::new(name).extension().is_some_and(|ext| ext == "old")
                })
                .map(move |name| (dir.join(name), file))
        })
        .collect();
    if force {
        return Ok(backups.into_iter().map(|(path, _)| path).collect());
    }
    if backups.iter().any(|(_, file)| !dir.join(file).exists()) {
        return Err(AnalyzeError::Backup { backups });
    }
    Ok(Vec::new())
}

/// Removes the backups that a forced run replaced, after its new files
/// are in place. A backup that cannot go stays, as before the run.
fn remove_backups(backups: &[PathBuf]) {
    for backup in backups {
        let _ = std::fs::remove_file(backup);
    }
}

/// The `source` and `source_sha256` of a `layers.json`; `None` when it
/// cannot be read, is not JSON or has no string `source`. A report from
/// before the checksum has no `source_sha256`.
fn recorded_stem(report: &Path) -> Option<(String, Option<String>)> {
    let report: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(report).ok()?).ok()?;
    let name = report.get("source")?.as_str()?.to_owned();
    let sha256 = report
        .get("source_sha256")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    Some((name, sha256))
}

/// Fails when the output folder is a file, or, with `force` off, holds a
/// `layers.json` that is not a report or whose source is not `source`.
/// Returns the checksum that a report of `source` records, which
/// [`guard`] compares once it has read the stem; a report from before the
/// checksum has none and is matched by the name alone. `guarded` says what
/// `--force` does to that report.
fn check_out_dir(
    dir: &Path,
    source: &str,
    force: bool,
    guarded: Guarded,
) -> Result<Option<String>, AnalyzeError> {
    if dir.exists() && !dir.is_dir() {
        return Err(AnalyzeError::OutIsFile {
            path: dir.to_path_buf(),
        });
    }
    let existing = dir.join(LAYERS_FILE);
    if force || !existing.exists() {
        return Ok(None);
    }
    match recorded_stem(&existing) {
        None => Err(AnalyzeError::NotOurs {
            path: existing,
            guarded,
        }),
        Some((name, _)) if name != source => Err(AnalyzeError::OtherSource {
            path: existing,
            recorded: name,
            source: source.to_owned(),
            guarded,
        }),
        Some((_, sha256)) => Ok(sha256),
    }
}

/// Fails when the output folder holds a `spectrogram.png` but no
/// `layers.json` and `force` is off: no report owns that file, so it is
/// the user's. [`check_out_dir`] has passed a `layers.json` that exists.
fn check_spectrogram(dir: &Path, force: bool) -> Result<(), AnalyzeError> {
    let existing = dir.join(SPECTROGRAM_FILE);
    if force || dir.join(LAYERS_FILE).exists() || !existing.exists() {
        return Ok(());
    }
    Err(AnalyzeError::Unowned { path: existing })
}

/// Fails when the output folder holds a `notes.mid` whose track name is
/// not `source` and `force` is off.
fn check_notes_file(dir: &Path, source: &str, force: bool) -> Result<(), AnalyzeError> {
    let existing = dir.join(NOTES_FILE);
    if force || !existing.exists() {
        return Ok(());
    }
    match std::fs::read(&existing)
        .ok()
        .and_then(|bytes| midi_track_name(&bytes))
    {
        Some(recorded) if recorded == source => Ok(()),
        Some(recorded) => Err(AnalyzeError::OtherSource {
            path: existing,
            recorded,
            source: source.to_owned(),
            guarded: Guarded::Notes,
        }),
        None => Err(AnalyzeError::NotOurs {
            path: existing,
            guarded: Guarded::Notes,
        }),
    }
}

/// The file of another stem that stopped a run, and what `--force` does
/// to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Guarded {
    /// A `layers.json` that `analyze --force` replaces.
    Report,
    /// A `layers.json` that `notes --force` leaves, writing `notes.mid`
    /// next to it.
    ReportBeside,
    /// A `notes.mid` that `notes --force` replaces.
    Notes,
}

impl Guarded {
    /// The fix that a refusal names.
    fn fix(self) -> &'static str {
        match self {
            Self::Report | Self::Notes => {
                "Pass --out <dir> for this stem, or --force to replace it"
            }
            Self::ReportBeside => {
                "Pass --out <dir> for this stem, or --force to write notes.mid next to it"
            }
        }
    }
}

/// Writes `bytes` to a new hidden temporary sibling of `name` in `dir` and
/// flushes it to disk. The file removes itself when dropped, so a failed
/// write leaves none.
fn stage(dir: &Path, name: &str, bytes: &[u8]) -> Result<tempfile::NamedTempFile, AnalyzeError> {
    let prefix = format!(".{name}.");
    let mut builder = tempfile::Builder::new();
    builder.prefix(&prefix).suffix(".tmp");
    // The umask applies, as it does to a file that File::create makes.
    #[cfg(unix)]
    builder.permissions(std::os::unix::fs::PermissionsExt::from_mode(0o666));
    builder
        .tempfile_in(dir)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.as_file().sync_all()?;
            Ok(file)
        })
        .map_err(|source| AnalyzeError::Write {
            path: dir.join(name),
            source,
        })
}

/// An analysis that failed.
#[derive(Debug)]
pub enum AnalyzeError {
    /// The note list cannot be read.
    Notes(MidiError),
    /// The stem cannot be decoded.
    Decode(DecodeError),
    /// The output folder is a file.
    OutIsFile {
        /// The path of the folder.
        path: PathBuf,
    },
    /// The default output folder of a stem has no name of its own: the
    /// stem's name has no extension, or is `.` or `..` without it.
    NoFolderName {
        /// The file name of the stem.
        source: String,
        /// Its name without the extension; `None` without an extension.
        stem: Option<String>,
    },
    /// The output folder holds the report or the notes of another stem.
    OtherSource {
        /// The existing `layers.json` or `notes.mid`.
        path: PathBuf,
        /// The stem it names.
        recorded: String,
        /// The stem of this run.
        source: String,
        /// What the file is, and what `--force` does to it.
        guarded: Guarded,
    },
    /// The output folder holds a report of the stem's name with other
    /// content: the recorded checksum differs, so it is another stem of
    /// that name or this stem changed since that run.
    OtherAudio {
        /// The existing `layers.json`.
        path: PathBuf,
        /// The file name of the stem.
        source: String,
        /// What `--force` does to the report.
        guarded: Guarded,
    },
    /// The output folder holds a `layers.json` or a `notes.mid` that this
    /// tool did not write: it cannot be read, or names no stem.
    NotOurs {
        /// The existing file.
        path: PathBuf,
        /// What the file would be, and what `--force` does to it.
        guarded: Guarded,
    },
    /// The output folder holds the backup of a report file, but not the
    /// file: a run was killed during its renames.
    Backup {
        /// Every backup in the folder, `.<file>.<random>.old`, with the
        /// file it backs up.
        backups: Vec<(PathBuf, &'static str)>,
    },
    /// The output folder holds a `spectrogram.png` that no report owns.
    Unowned {
        /// The existing file.
        path: PathBuf,
    },
    /// The output folder holds a `layers.json` that describes the final
    /// notes, which `notes` would replace with the first list.
    NotesDescribed {
        /// The existing `layers.json`.
        path: PathBuf,
    },
    /// The model run, a decode or the f0 track failed.
    Transcription(NotesError),
    /// The pitch layer failed.
    Pitch(PitchError),
    /// The envelope layer failed.
    Envelope(EnvelopeError),
    /// The voices layer failed.
    Voices(VoicesError),
    /// The stem gave no note.
    NoNotes {
        /// The file name of the stem.
        source: String,
    },
    /// The notes cannot be stored as MIDI.
    Midi(MidiWriteError),
    /// An output file or folder cannot be written.
    Write {
        /// The path that failed.
        path: PathBuf,
        /// The cause.
        source: io::Error,
    },
    /// An internal step failed: a confidence that could not be built, or a
    /// JSON or PNG encoder error.
    Internal(String),
}

impl fmt::Display for AnalyzeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Notes(error) => write!(f, "{error}"),
            Self::Decode(error) => write!(f, "{error}"),
            Self::OutIsFile { path } => write!(
                f,
                "the output folder {} is a file. Pass --out <dir>",
                path.display()
            ),
            Self::NoFolderName { source, stem } => match stem {
                Some(stem) => write!(
                    f,
                    "`{source}` gives no folder name: without its extension it is `{stem}`. Pass --out <dir>"
                ),
                None => write!(
                    f,
                    "`{source}` gives no folder name: it has no extension to strip, so the folder would be the stem file itself. Pass --out <dir>"
                ),
            },
            Self::OtherSource {
                path,
                recorded,
                source,
                guarded,
            } => {
                let what = match guarded {
                    Guarded::Report | Guarded::ReportBeside => "is the report",
                    Guarded::Notes => "holds the notes",
                };
                write!(
                    f,
                    "{} {what} of `{recorded}`, not of `{source}`. {}",
                    path.display(),
                    guarded.fix()
                )
            }
            Self::OtherAudio {
                path,
                source,
                guarded,
            } => write!(
                f,
                "{} is the report of `{source}` with other content: another stem of that name, or this stem changed since that run. {}",
                path.display(),
                guarded.fix()
            ),
            Self::NotOurs { path, guarded } => {
                let what = match guarded {
                    Guarded::Report | Guarded::ReportBeside => {
                        "is not a report of this tool: it cannot be read, is not JSON or names no `source`"
                    }
                    Guarded::Notes => {
                        "holds no notes of this tool: it cannot be read as MIDI or has no track name"
                    }
                };
                write!(f, "{} exists but {what}. {}", path.display(), guarded.fix())
            }
            Self::Backup { backups } => {
                let list = backups
                    .iter()
                    .map(|(path, file)| format!("{} of {file}", path.display()))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(
                    f,
                    "the folder holds backups that a run left when it stopped before it ended: {list}. Rename each backup to its file name to restore the earlier files, or pass --force to write new files and remove the backups"
                )
            }
            Self::Unowned { path } => write!(
                f,
                "{} exists, and no report in the folder owns it. {}",
                path.display(),
                Guarded::Report.fix()
            ),
            Self::NotesDescribed { path } => write!(
                f,
                "{} describes the final notes.mid that `analyze` wrote, and `notes` would replace that file with the first note list. Pass --out <dir> for the first list, or --force to replace notes.mid and remove the report",
                path.display()
            ),
            Self::Transcription(error) => write!(f, "{error}"),
            Self::Pitch(error) => write!(f, "{error}"),
            Self::Envelope(error) => write!(f, "{error}"),
            Self::Voices(error) => write!(f, "{error}"),
            Self::NoNotes { source } => write!(
                f,
                "`{source}` gave no note, so nothing was written: it is silent, or no note sounds within {} dB of its loudest passage. Check the stem, or pass --profile bass|lead|pad",
                QUIET_GATE.get()
            ),
            Self::Midi(error) => write!(f, "{error}"),
            Self::Write { path, source } => write!(
                f,
                "cannot write {}: {source}. Check the folder and its permissions, or pass another --out",
                path.display()
            ),
            Self::Internal(message) => write!(
                f,
                "the analysis failed internally ({message}). Report this as a bug"
            ),
        }
    }
}

impl Error for AnalyzeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Notes(error) => Some(error),
            Self::Decode(error) => Some(error),
            Self::Transcription(error) => Some(error),
            Self::Pitch(error) => Some(error),
            Self::Envelope(error) => Some(error),
            Self::Voices(error) => Some(error),
            Self::Midi(error) => Some(error),
            Self::Write { source, .. } => Some(source),
            Self::OutIsFile { .. }
            | Self::NoFolderName { .. }
            | Self::OtherSource { .. }
            | Self::OtherAudio { .. }
            | Self::NotOurs { .. }
            | Self::Backup { .. }
            | Self::Unowned { .. }
            | Self::NotesDescribed { .. }
            | Self::NoNotes { .. }
            | Self::Internal(_) => None,
        }
    }
}

impl From<ConfidenceError> for AnalyzeError {
    fn from(error: ConfidenceError) -> Self {
        Self::Internal(error.to_string())
    }
}

impl From<serde_json::Error> for AnalyzeError {
    fn from(error: serde_json::Error) -> Self {
        Self::Internal(error.to_string())
    }
}

impl From<png::EncodingError> for AnalyzeError {
    fn from(error: png::EncodingError) -> Self {
        Self::Internal(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{Decibels, MidiPitch};

    /// The names in a folder, sorted, hidden ones included.
    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// A name with a missing folder in it cannot be staged: here the second
    /// stage fails after the first file is staged. The staged file goes,
    /// and the old files stay.
    #[test]
    fn a_failed_stage_leaves_the_old_files_and_no_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(NOTES_FILE), b"old notes").unwrap();
        std::fs::write(dir.path().join(LAYERS_FILE), b"old report").unwrap();
        let error = replace_files(
            dir.path(),
            &[
                (NOTES_FILE, Some(b"new notes".as_slice())),
                ("missing/spectrogram.png", Some(b"new png".as_slice())),
                (LAYERS_FILE, Some(b"new report".as_slice())),
            ],
        )
        .unwrap_err();
        assert!(matches!(error, AnalyzeError::Write { .. }), "{error}");
        assert_eq!(entries(dir.path()), [LAYERS_FILE, NOTES_FILE]);
        assert_eq!(
            std::fs::read(dir.path().join(NOTES_FILE)).unwrap(),
            b"old notes"
        );
        assert_eq!(
            std::fs::read(dir.path().join(LAYERS_FILE)).unwrap(),
            b"old report"
        );
    }

    /// A path through a file cannot be read, so its move aside fails after
    /// the old report moved aside and the new notes landed. Every step
    /// before it is undone.
    #[test]
    fn a_failed_move_aside_restores_every_earlier_step() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(NOTES_FILE), b"old notes").unwrap();
        std::fs::write(dir.path().join(LAYERS_FILE), b"old report").unwrap();
        std::fs::write(dir.path().join("plain"), b"a file").unwrap();
        let error = replace_files(
            dir.path(),
            &[
                (NOTES_FILE, Some(b"new notes".as_slice())),
                ("plain/spectrogram.png", None),
                (LAYERS_FILE, Some(b"new report".as_slice())),
            ],
        )
        .unwrap_err();
        assert!(matches!(error, AnalyzeError::Write { .. }), "{error}");
        assert_eq!(entries(dir.path()), [LAYERS_FILE, NOTES_FILE, "plain"]);
        assert_eq!(
            std::fs::read(dir.path().join(NOTES_FILE)).unwrap(),
            b"old notes"
        );
        assert_eq!(
            std::fs::read(dir.path().join(LAYERS_FILE)).unwrap(),
            b"old report"
        );
    }

    /// A failed write into a folder the run created removes that folder and
    /// the parents it created, and keeps the parent that existed.
    #[test]
    fn a_failed_write_removes_the_folders_it_created() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("new").join("out");
        replace_files(&out, &[("missing/notes.mid", Some(b"notes".as_slice()))]).unwrap_err();
        assert_eq!(entries(dir.path()), Vec::<String>::new());
        // A folder that existed stays, empty or not.
        std::fs::create_dir(dir.path().join("kept")).unwrap();
        replace_files(
            &dir.path().join("kept"),
            &[("missing/notes.mid", Some(b"notes".as_slice()))],
        )
        .unwrap_err();
        assert_eq!(entries(dir.path()), ["kept"]);
    }

    /// A 220 Hz tone held at full level to 0.5 s that then falls 60 dB/s
    /// reaches 20 dB under its plateau at 0.833 s, within its list span.
    #[test]
    fn a_pad_note_ends_where_its_level_falls_20_db_under_the_plateau() {
        let rate = SampleRate::new(22_050).unwrap();
        let fs = 22_050.0;
        let mono: Vec<f64> = (0..33_075)
            .map(|n| {
                let t = f64::from(n) / fs;
                let gain = if t < 0.5 {
                    1.0
                } else {
                    10f64.powf(-3.0 * (t - 0.5))
                };
                0.5 * gain * (std::f64::consts::TAU * 220.0 * t).sin()
            })
            .collect();
        let pitch = MidiPitch::new(57.0).unwrap();
        let note = Note::new(Seconds::clamped(0.0), Seconds::clamped(1.5), pitch).unwrap();
        let read = NoteEnvelope {
            onset: note.onset(),
            end: note.offset(),
            pitch,
            peak_db: Decibels::clamped(0.0),
            attack: None,
            attack_upper_bound: false,
            hold: Seconds::clamped(0.5),
            decay: None,
            sustain: Some(Decibels::clamped(0.0)),
            knee: Some(Seconds::clamped(0.5)),
            ripple: None,
            residual: None,
            overlap: false,
            tail: None,
        };
        let end = pad_end(&mono, rate, &note, std::slice::from_ref(&note), &read)
            .unwrap()
            .unwrap();
        assert!((end.get() - (0.5 + 20.0 / 60.0)).abs() < 0.01, "{end}");
        let held = NoteEnvelope {
            sustain: None,
            ..read
        };
        let unread = pad_end(&mono, rate, &note, std::slice::from_ref(&note), &held).unwrap();
        assert_eq!(unread, None);
        // Listed only to 0.6 s, the tone falls 20 dB after its list end, so
        // it has no pad end: the search stops there, and the release belongs
        // to the preset's amp envelope.
        let short = Note::new(note.onset(), Seconds::clamped(0.6), pitch).unwrap();
        let read = NoteEnvelope {
            end: short.offset(),
            ..read
        };
        let end = pad_end(&mono, rate, &short, std::slice::from_ref(&short), &read).unwrap();
        assert_eq!(end, None);
    }
}
