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
fn out_dir(stem: &Path, out: Option<&Path>) -> PathBuf {
    out.map_or_else(
        || {
            let name = stem.file_stem().unwrap_or(stem.as_os_str());
            stem.with_file_name(name)
        },
        Path::to_path_buf,
    )
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
    /// Replace a report of another stem in the output folder.
    pub force: bool,
}

impl AnalyzeRequest<'_> {
    /// Returns the output folder.
    #[must_use]
    pub fn out_dir(&self) -> PathBuf {
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
/// The files are replaced; nothing else in the folder changes. Each file
/// is written to a temporary sibling and renamed into place, `notes.mid`
/// first and `layers.json` last: an old report is removed before the new
/// files land, so a `layers.json` that exists always belongs to the files
/// next to it, and none is ever cut short. The final `notes.mid` replaces
/// the first one of `notes`.
///
/// # Errors
///
/// Returns an [`AnalyzeError`] when the output folder is a file or holds
/// the report or the notes of another stem (without `force`), when the
/// note list or the stem cannot be read, when the notes step fails, or
/// when an output file cannot be written.
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
    let dir = request.out_dir();
    let source = request.source();
    check_out_dir(&dir, &source, request.force, Guarded::Report)?;
    check_notes_file(&dir, &source, request.force)?;
    let decoded = audio::decode(request.stem).map_err(AnalyzeError::Decode)?;
    let skipped = decoded.skipped().to_vec();
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
        (voicing, request.profile == Some(Profile::Pad)),
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
        &source,
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
    })
}

/// Writes the files of `analyze` into `dir`, creating it: each to a
/// temporary sibling, then renamed into place, `notes.mid` first and
/// `layers.json` last, after the old report is removed, and the old
/// `notes.mid` with it when there are no notes. Returns the paths
/// of `layers.json`, `spectrogram.png` and `notes.mid` when written. The
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
    let midi = midi.as_deref();
    std::fs::create_dir_all(dir).map_err(|source| AnalyzeError::Write {
        path: dir.to_path_buf(),
        source,
    })?;
    let layers = dir.join(LAYERS_FILE);
    let spectrogram = dir.join(SPECTROGRAM_FILE);
    let notes = midi.map(|_| dir.join(NOTES_FILE));
    let layers_tmp = stage(dir, LAYERS_FILE, json.as_bytes())?;
    let spectrogram_tmp = stage(dir, SPECTROGRAM_FILE, png)?;
    let notes_tmp = midi
        .map(|bytes| stage(dir, NOTES_FILE, bytes))
        .transpose()?;
    // The guard has passed, so a `notes.mid` here is this stem's or one that
    // `--force` replaces. A run without notes leaves none that the report
    // does not describe.
    let stale = dir.join(NOTES_FILE);
    for old in std::iter::once(&layers).chain(notes.is_none().then_some(&stale)) {
        match std::fs::remove_file(old) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                return Err(AnalyzeError::Write {
                    path: old.clone(),
                    source: error,
                });
            }
            _ => {}
        }
    }
    let renames = notes_tmp
        .as_ref()
        .zip(notes.as_ref())
        .into_iter()
        .chain([(&spectrogram_tmp, &spectrogram), (&layers_tmp, &layers)]);
    for (from, to) in renames {
        std::fs::rename(from, to).map_err(|source| AnalyzeError::Write {
            path: to.clone(),
            source,
        })?;
    }
    Ok((layers, spectrogram, notes))
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
            let horizon = || pad_horizon(mono, rate, note, list);
            let pad_end = match read {
                Some(read) if pad => pad_end(mono, rate, (note, horizon()), list, read)?,
                None if pad => full_band_pad_end(mono, rate, note, horizon()),
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

/// How far a pad note's end may lie: the next onset on the note's key,
/// or the stem's end, and never before the note's end in the list. A pad
/// tail rings past the list end, and another note on the key restarts it.
fn pad_horizon(mono: &[f64], rate: SampleRate, note: &Note, list: &[Note]) -> Seconds {
    let key = |n: &Note| crate::cast::round_to_usize(n.pitch().get());
    let stem_end = crate::cast::usize_to_f64(mono.len()) / rate.hz().get();
    let next = list
        .iter()
        .filter(|o| key(o) == key(note) && o.onset() > note.onset())
        .map(|o| o.onset().get())
        .fold(stem_end, f64::min);
    Seconds::clamped(next.max(note.offset().get()))
}

/// The note with its end moved to `to`, for a read past its list end.
fn reach(note: &Note, to: Seconds) -> Option<Note> {
    Note::new(note.onset(), to, note.pitch()).ok()
}

/// Where a pad note's amp proxy falls [`PAD_FALL`] below its plateau, the
/// peak plus the sustain level, first after its knee or else its peak, and
/// before `horizon`. The peak lies within the list span, and the proxy keeps
/// the harmonic that the notes overlapping that span leave free. `None`
/// without a sustain, a proxy or such a fall.
fn pad_end(
    mono: &[f64],
    rate: SampleRate,
    (note, horizon): (&Note, Seconds),
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
    let Some(long) = reach(note, horizon) else {
        return Ok(None);
    };
    let Some(proxy) = amp_proxy(mono, rate, &long, &others)
        .map_err(|error| AnalyzeError::Envelope(error.into()))?
    else {
        return Ok(None);
    };
    let db = proxy.db();
    let listed = proxy.frames_to(note.offset().get());
    let Some((peak_at, peak)) = db
        .iter()
        .take(listed)
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
/// not read falls [`PAD_FALL`] below its plateau, first after its peak and
/// before `horizon`. The peak lies within the list span, and the plateau
/// is the median of the last [`SUSTAIN_SHARE`] of the held part, from the
/// first frame within 1 dB of the peak to [`KNEE_END_GAP`] before the
/// note's end, as the envelope layer reads a sustain; `None` without a
/// held part or such a fall.
fn full_band_pad_end(
    mono: &[f64],
    rate: SampleRate,
    note: &Note,
    horizon: Seconds,
) -> Option<Seconds> {
    let track = full_band_track(mono, rate, (note.onset(), horizon), Hz::from(note.pitch()));
    let db = track.db();
    let listed = track.frames_to(note.offset().get());
    let (peak_at, peak) = db
        .iter()
        .take(listed)
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
/// a time by the voices layer, and `--profile pad` makes any line
/// polyphonic. The envelope's full-band levels read that voicing, so
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
    let mono_line = voices.mono && !pad_profile;
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
    /// Write next to a report of another stem.
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
/// carries that tempo. The file is written to a temporary sibling
/// and renamed into place, and nothing else in the folder changes.
///
/// # Errors
///
/// Returns an [`AnalyzeError`] when the output folder is a file or holds
/// the report of another stem (without `force`), when the stem cannot be
/// decoded or transcribed, when it gives no note, or when the file cannot
/// be written.
pub fn notes(
    request: &NotesRequest<'_>,
    transcriber: &Transcriber,
) -> Result<NotesReport, AnalyzeError> {
    let dir = out_dir(request.stem, request.out);
    let source = source_of(request.stem);
    check_out_dir(&dir, &source, request.force, Guarded::ReportBeside)?;
    check_notes_file(&dir, &source, request.force)?;
    let decoded = audio::decode(request.stem).map_err(AnalyzeError::Decode)?;
    let skipped = decoded.skipped().to_vec();
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
    std::fs::create_dir_all(&dir).map_err(|source| AnalyzeError::Write {
        path: dir.clone(),
        source,
    })?;
    let path = dir.join(NOTES_FILE);
    let staged = stage(&dir, NOTES_FILE, &bytes)?;
    if let Err(source) = std::fs::rename(&staged, &path) {
        // The staged copy is ours alone; a failed removal changes nothing
        // the error does not already report.
        let _ = std::fs::remove_file(&staged);
        return Err(AnalyzeError::Write { path, source });
    }
    Ok(NotesReport {
        path,
        tempo,
        transcription,
        skipped,
    })
}

/// Fails when the output folder is a file, or holds a `layers.json` whose
/// source is not `source` and `force` is off. `guarded` says what `--force`
/// does to that report.
fn check_out_dir(
    dir: &Path,
    source: &str,
    force: bool,
    guarded: Guarded,
) -> Result<(), AnalyzeError> {
    if dir.exists() && !dir.is_dir() {
        return Err(AnalyzeError::OutIsFile {
            path: dir.to_path_buf(),
        });
    }
    let existing = dir.join(LAYERS_FILE);
    if force || !existing.exists() {
        return Ok(());
    }
    let recorded = std::fs::read_to_string(&existing)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|report| report.get("source")?.as_str().map(str::to_owned));
    if recorded.as_deref() == Some(source) {
        Ok(())
    } else {
        Err(AnalyzeError::OtherSource {
            path: existing,
            recorded,
            source: source.to_owned(),
            guarded,
        })
    }
}

/// Fails when the output folder holds a `notes.mid` whose track name is
/// not `source` and `force` is off.
fn check_notes_file(dir: &Path, source: &str, force: bool) -> Result<(), AnalyzeError> {
    let existing = dir.join(NOTES_FILE);
    if force || !existing.exists() {
        return Ok(());
    }
    let recorded = std::fs::read(&existing)
        .ok()
        .and_then(|bytes| midi_track_name(&bytes));
    if recorded.as_deref() == Some(source) {
        Ok(())
    } else {
        Err(AnalyzeError::OtherSource {
            path: existing,
            recorded,
            source: source.to_owned(),
            guarded: Guarded::Notes,
        })
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

/// Writes `bytes` to a hidden temporary sibling of `name` in `dir` and
/// flushes it to disk; returns its path.
fn stage(dir: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, AnalyzeError> {
    let path = dir.join(format!(".{name}.tmp"));
    let fail = |source| AnalyzeError::Write {
        path: path.clone(),
        source,
    };
    let mut file = std::fs::File::create(&path).map_err(fail)?;
    file.write_all(bytes).map_err(fail)?;
    file.sync_all().map_err(fail)?;
    Ok(path)
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
    /// The output folder holds the report or the notes of another stem.
    OtherSource {
        /// The existing `layers.json` or `notes.mid`.
        path: PathBuf,
        /// The stem it names, if it names one.
        recorded: Option<String>,
        /// The stem of this run.
        source: String,
        /// What the file is, and what `--force` does to it.
        guarded: Guarded,
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
            Self::OtherSource {
                path,
                recorded,
                source,
                guarded,
            } => {
                let other = recorded
                    .as_deref()
                    .map_or_else(|| "no named stem".to_owned(), |name| format!("`{name}`"));
                let (what, force) = match guarded {
                    Guarded::Report => ("is the report", "replace it"),
                    Guarded::ReportBeside => ("is the report", "write notes.mid next to it"),
                    Guarded::Notes => ("holds the notes", "replace it"),
                };
                write!(
                    f,
                    "{} {what} of {other}, not of `{source}`. Pass --out <dir> for this stem, or --force to {force}",
                    path.display()
                )
            }
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
            | Self::OtherSource { .. }
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

    /// A 220 Hz tone held at full level to 0.5 s that then falls 60 dB/s
    /// reaches 20 dB under its plateau at 0.833 s.
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
        let list = std::slice::from_ref(&note);
        let horizon = pad_horizon(&mono, rate, &note, list);
        assert_eq!(horizon, Seconds::clamped(1.5));
        let end = pad_end(&mono, rate, (&note, horizon), list, &read)
            .unwrap()
            .unwrap();
        assert!((end.get() - (0.5 + 20.0 / 60.0)).abs() < 0.01, "{end}");
        let held = NoteEnvelope {
            sustain: None,
            ..read
        };
        let unread = pad_end(&mono, rate, (&note, horizon), list, &held).unwrap();
        assert_eq!(unread, None);
    }

    /// The same tone listed only to 0.6 s rings on past its list end: its
    /// pad end lies at 0.833 s, up to the stem's end, but not past the next
    /// note on its key.
    #[test]
    fn a_pad_tail_is_found_past_the_list_end_up_to_the_next_note_on_its_key() {
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
        let s = Seconds::clamped;
        let note = Note::new(s(0.0), s(0.6), pitch).unwrap();
        let list = [note.clone()];
        let horizon = pad_horizon(&mono, rate, &note, &list);
        assert_eq!(horizon, s(1.5));
        let end = full_band_pad_end(&mono, rate, &note, horizon).unwrap();
        assert!((end.get() - (0.5 + 20.0 / 60.0)).abs() < 0.01, "{end}");
        // A note on the key at 0.7 s, or one a semitone up that leaves it.
        let next = Note::new(s(0.7), s(1.0), pitch).unwrap();
        let other = Note::new(s(0.65), s(1.0), MidiPitch::new(58.0).unwrap()).unwrap();
        let list = [note.clone(), other, next];
        let horizon = pad_horizon(&mono, rate, &note, &list);
        assert_eq!(horizon, s(0.7));
        assert_eq!(full_band_pad_end(&mono, rate, &note, horizon), None);
    }
}
