//! The pitch layer of a monophonic line: glides between notes, pitch drops
//! at the strike of a low note, and vibrato.
//!
//! The glides and the vibrato read the f0 track; the drops read the
//! sub-band line of the stem, because the track smears a drop that settles
//! in tens of milliseconds. Every record carries its own confidence. A
//! line that plays chords reports nothing, since no method here separates
//! the voices.

mod drop;
mod fit;
mod glide;
mod line;
mod vibrato;

pub use drop::{
    DROP_CONVENTION, DROP_FIT_START, DROP_MAX, DROP_MAX_RESIDUAL, DROP_MIN, DROP_ONSET_SHARE,
    DROP_TAU_MIN, DropFit, DropMiss, ONSET_SMOOTHING, band_onset, fit_drop,
};
pub use glide::{CURVE_RANGE, GLIDE_CONVENTION, GLIDE_SLOPE_MIN, GlideFit};
pub use line::{LINE_CONVENTION, SubBandLine};
pub use vibrato::{EXTENT_SHARE, VIBRATO_CONVENTION, VIBRATO_SPAN_MIN, VibratoFit, fit_vibrato};

use drop::{DROP_FIT_END, band_attack};
use glide::{GlideRead, find_glides, steady_spans, track_lag};
use line::{LINE_HI, LINE_LO, LINE_LO_FAST};

use crate::audio::SampleRate;
use crate::cast;
use crate::confidence::{Confidence, ConfidenceError, Level, NoSpread, Reason, ReasonCode, Signal};
use crate::dsp::{
    ACTIVE_GATE_DB, FilterError, StftParams, band_magnitude, band_pass_zero_phase, frame_rms,
};
use crate::notes::{NoteList, PitchTrack, QUIET_FRAME, Voicing};
use crate::units::{Cents, Decibels, Hz, MidiPitch, Seconds, Semitones};

/// The onset search of a strike starts at most this long before the note
/// onset, because the transcriber places the onset of a falling note late.
pub const STRIKE_LOOKBACK: Seconds = Seconds::from_const(0.5);
/// The onset search ends this long after the note onset.
pub const STRIKE_LOOKAHEAD: Seconds = Seconds::from_const(0.1);
/// The vibrato search skips this much at each end of a steady span.
pub const RUN_TRIM: Seconds = Seconds::from_const(0.05);
/// The vibrato search skips at least this much after a drop's onset, and
/// five time constants when that is longer.
const SETTLE_MIN: f64 = 0.1;
/// A glide from silence is a drop when its run starts this close to the
/// onset of an accepted drop.
pub const DROP_GLIDE_DISTANCE: Seconds = Seconds::from_const(0.2);
/// A drop settles on a note whose pitch lies within this many cents of
/// `f_end`.
pub const DROP_NOTE_MATCH: Cents = Cents::from_const(100.0);
/// The note a drop settles on starts at most this long before the onset.
///
/// A note that started earlier belongs to a sound before the strike. On
/// real stems a fit on the lead-in to a step settled on the note that
/// had sounded for 790 ms, and a fit inside a chord on a note an octave
/// below that had started 210 ms before. The latest note a generated drop
/// settles on starts 95 ms before its onset.
pub const DROP_NOTE_LEAD: Seconds = Seconds::from_const(0.15);
/// The harmonic check of a strike reads the bands from this long after the
/// onset.
pub const WEAK_FROM: Seconds = Seconds::from_const(0.05);
/// A fundamental is weak when the band around twice the pitch exceeds the
/// band around the pitch by this factor, 6 dB.
///
/// White noise reads a factor of 1.41, since the upper band is twice as
/// wide. Below this factor a fit that reads the harmonic settles 1200
/// cents off the note and fails.
pub const WEAK_RATIO: f64 = 2.0;
/// A strike raises the full-band RMS by this factor, 6 dB. The glide
/// search reads it as a new note, and a drop that is fast or starts at the
/// top of the band needs it.
pub const STRIKE_RISE: f64 = 2.0;
/// A drop with `tau` under this needs a strike of [`STRIKE_RISE`]. The swing
/// of the line where a ringing tone restarts its phase fits a fast fall:
/// a plain retriggered 808 read 32 semitones with a `tau` of 4 ms.
pub const FAST_TAU: Seconds = Seconds::from_const(0.01);
/// The low-pass of the note line, as a multiple of the note's pitch.
///
/// A fit after a strike of [`STRIKE_RISE`] that misses on its residual is
/// fitted again on a line that keeps the band below this edge, when the
/// edge lies under 200 Hz, the top of the band. Below 66 Hz the second and
/// third harmonics of a note lie inside the band, and at -5 to -14 dB they
/// swing the line by more than the note's pitch: a real sweep into C1
/// fitted a residual of 2.5. At 1.5 times the note the
/// second harmonic falls 10 dB more, and a fall that starts higher stays
/// the strongest component on its way down. Without a strike, the note
/// line smooths the swing where a retriggered tone restarts its phase into
/// a fall: three generated retriggers that rose 0.6 to 1.6 dB read one.
pub const NOTE_LINE_EDGE: f64 = 1.5;
/// The spans, in seconds from a band onset, of the RMS before and after
/// it that a strike compares. A pitch falling into the band from above
/// raises the band envelope with no strike. The span before ends 25 ms
/// early, since the band onset of a broadband sound such as noise lags
/// its start by about 15 ms.
const STRIKE_SPANS: [(f64, f64); 2] = [(-0.045, -0.025), (0.0, 0.02)];
/// The spans, in seconds from a time, of the RMS before and after it that
/// tell whether a sound starts there, for the glide search.
const RUN_START_SPANS: [(f64, f64); 2] = [(-0.06, -0.03), (0.0, 0.03)];
/// A sound starts where the full-band RMS rises by this factor, 20 dB:
/// from silence, not from a dip of a beating unison.
pub const RUN_START_RISE: f64 = 10.0;
/// The step at which the glide search looks for the start of a sound.
const ONSET_STEP: f64 = 0.005;
/// A gap of the track whose full-band RMS, over 10 ms, falls this far
/// below the 20 ms before or after it is not bridged.
///
/// The dip breaks the sound, and the glide search reads the runs apart. So
/// does a gap that falls more than [`ACTIVE_GATE_DB`] below the loudest
/// frame of the stem.
pub const GAP_DIP_DB: Decibels = Decibels::from_const(20.0);
/// The RMS window of the dip check.
const DIP_WINDOW: f64 = 0.01;
/// The span before and after a gap that the dip check compares with.
const DIP_SIDE: f64 = 0.02;
/// The line keeps this much signal after the fit span, so the cut end of
/// the analytic signal stays far from it.
const LINE_TAIL: f64 = 0.5;
/// The line starts this long before the onset search, for the same reason.
const LINE_LEAD: f64 = 0.1;

/// A glide whose fit residual stays at or below this many cents, whose
/// stable pitches jitter at most [`JITTER_HIGH`] and whose motion lasts
/// [`SPAN_HIGH`] or more reads at high confidence.
pub const RESIDUAL_HIGH: f64 = 15.0;
/// The largest jitter of a high-confidence glide, in cents. A unison
/// beats, and the track then wanders on the stable pitches.
pub const JITTER_HIGH: f64 = 3.0;
/// The largest curve power of a high-confidence glide. A faster start
/// reads its rise less well: up to 11 % off at `p` 4.4 on the generated
/// sets, within 7.1 % at 3 or less.
pub const CURVE_HIGH: f64 = 3.0;
/// The shortest motion of a high-confidence glide.
pub const SPAN_HIGH: Seconds = Seconds::from_const(0.15);
/// The jitter above which a glide reads at low confidence.
///
/// The unison glides of the generated sets jitter 3.77 to 7.07 cents at
/// 22.05 kHz and 3.45 to 6.93 at 44.1 kHz, the mono ones 0.66 to 8.03 at
/// both rates, so the jitter alone cannot tell the two apart: a mono glide
/// above it reads low too. From [`JITTER_HIGH`] to here only mono glides
/// fall, and they read medium.
pub const JITTER_LOW: f64 = 3.4;
/// A drop without a full-band rise needs an attack this short or shorter.
/// Struck 808 notes read 9 to 13 ms; a unison falling into the band from
/// above reads 190 ms.
pub const STRIKE_ATTACK: Seconds = Seconds::from_const(0.03);
/// A strike whose RMS in the band of the line, over the 20 ms after the
/// onset, lies more than this below the loudest [`QUIET_FRAME`] of that
/// band in the stem is no candidate.
///
/// Checked against an independent IF on real stems, the drops of a line's
/// own low notes read 3 to 12 dB below the loudest passage of the band.
/// Quiet falls under a louder line in the band, and a fall in a noise
/// floor, read 22 to 33 dB below it.
pub const DROP_BAND_GATE: Decibels = Decibels::from_const(18.0);
/// The longest attack of a high-confidence drop.
///
/// The pitch falls during a slower attack, before the fit span starts, and
/// `df` reads low. The 10 ms average of the envelope spreads a sharp strike
/// to 9 to 11 ms; a 20 ms ramp reads about 17.5 ms.
pub const ATTACK_HIGH: Seconds = Seconds::from_const(0.012);
/// A drop whose residual share stays below this reads at high confidence.
pub const DROP_RESIDUAL_HIGH: f64 = 0.05;
/// A vibrato over this many cycles or more reads at high confidence.
///
/// Checked against an independent f0 on real stems, every vibrato above
/// 100 Hz of 4 cycles or more matched it, at a share of 0.51 to 0.64: the
/// share did not separate the real ones from the rest.
pub const CYCLES_HIGH: f64 = 4.0;
/// A vibrato on a pitch below this reads at low confidence.
///
/// Checked against an independent f0 on real stems, few vibratos below it
/// matched, and their extent read up to 5 times too wide. A generated
/// low tone with amplitude modulation or two beating voices reads no
/// vibrato, so the cause lies elsewhere.
pub const VIBRATO_LOW_PITCH: Hz = Hz::from_const(100.0);
/// A vibrato whose contour has a kurtosis below this reads at low
/// confidence: it jumps between two levels, as a trill does. The sines of
/// the generated set read 1.36 to 1.66, a semitone trill 1.06.
pub const KURTOSIS_LOW: f64 = 1.25;
/// The layer reads at low confidence below this share of note frames.
///
/// The share counts the track frames inside the notes that are voiced
/// within [`NOTE_FRAME_MATCH`] of a sounding note. The tones of the
/// generated set read 0.80 or more.
pub const NOTE_FRAMES_LOW: f64 = 0.5;
/// A voiced frame belongs to a sounding note within this many cents.
pub const NOTE_FRAME_MATCH: Cents = Cents::from_const(100.0);

/// What the pitch layer reads.
#[derive(Clone, Copy, Debug)]
pub struct PitchInput<'a> {
    /// The mono mix at the native rate.
    pub mono: &'a [f64],
    /// The native rate.
    pub rate: SampleRate,
    /// The f0 track of the stem.
    pub track: &'a PitchTrack,
    /// The note list, transcribed or given.
    pub notes: &'a NoteList,
    /// Whether the line plays one note at a time.
    pub voicing: Voicing,
}

/// A record with its confidence.
#[derive(Clone, Debug, PartialEq)]
pub struct Rated<T> {
    /// The record.
    pub value: T,
    /// How far the layer trusts it.
    pub confidence: Confidence<NoSpread>,
}

/// Flags of the pitch layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[expect(clippy::struct_excessive_bools, reason = "independent report flags")]
pub struct PitchFlags {
    /// The line plays chords, so no record was searched for.
    pub chord_skipped: bool,
    /// A motion between two notes lasted 35 to 90 ms, over its detected
    /// span or its fitted time: a glide too short to read.
    pub glide_too_short: bool,
    /// A low note whose second harmonic exceeds its fundamental by 6 dB or
    /// more had its drop fit dropped, since the band-pass then reads the
    /// wrong partial.
    pub weak_fundamental: bool,
    /// The stem gave no note list, so no record was searched for.
    pub no_notes: bool,
}

/// The pitch layer of a stem.
#[derive(Clone, Debug, PartialEq)]
pub struct PitchLayer {
    /// The glides, in time order.
    pub glides: Vec<Rated<GlideFit>>,
    /// The accepted pitch drops, in time order.
    pub drops: Vec<Rated<DropFit>>,
    /// The accepted vibratos, in time order.
    pub vibrato: Vec<Rated<VibratoFit>>,
    /// The flags.
    pub flags: PitchFlags,
    /// The confidence of the layer as a whole; no estimate when the line
    /// plays chords or the stem has no notes.
    pub confidence: Confidence<NoSpread>,
    /// The counts behind the summary.
    pub counts: PitchCounts,
}

/// How many candidates each search read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PitchCounts {
    /// Motions between two notes shorter than a glide.
    pub too_short: usize,
    /// Low notes whose strike the drop fit read.
    pub drop_candidates: usize,
    /// Why the drop search read no drop at a low note, per gate.
    pub drop_skips: DropSkips,
    /// Steady spans long enough for the vibrato search.
    pub vibrato_candidates: usize,
    /// Voiced frames of the track.
    pub voiced_frames: usize,
}

/// The low notes the drop search read no drop at, by the first gate that
/// stopped each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DropSkips {
    /// No attack inside the onset search.
    pub no_onset: usize,
    /// The strike lies more than [`ACTIVE_GATE_DB`] below the loudest
    /// frame of the stem, or more than [`DROP_BAND_GATE`] below the
    /// loudest frame of the band.
    pub quiet: usize,
    /// The full-band level does not rise as the fall needs.
    pub no_strike: usize,
    /// The second harmonic exceeds the fundamental.
    pub weak_fundamental: usize,
    /// The fit failed, or its drop lies outside [`DROP_MIN`] to
    /// [`DROP_MAX`] in full or inside its span.
    pub no_fall: usize,
    /// The fit's `tau` sits on the bound of the search.
    pub tau_bound: usize,
    /// The fit's residual reaches [`DROP_MAX_RESIDUAL`].
    pub residual: usize,
    /// No note sounds at `f_end` while the drop is settled.
    pub off_note: usize,
}

impl DropSkips {
    /// The gates that skipped a note, as `count gate` items.
    #[must_use]
    pub fn items(&self) -> Vec<String> {
        [
            (self.no_onset, "no onset"),
            (self.quiet, "quiet"),
            (self.no_strike, "no strike"),
            (self.weak_fundamental, "weak fundamental"),
            (self.no_fall, "no fall"),
            (self.tau_bound, "tau on bound"),
            (self.residual, "residual"),
            (self.off_note, "off note"),
        ]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, gate)| format!("{n} {gate}"))
        .collect()
    }
}

impl PitchLayer {
    /// Reads the pitch layer of a stem.
    ///
    /// # Errors
    ///
    /// Returns a [`PitchError`] when the stem's rate is too low for the
    /// 200 Hz band of the sub-band line, or a confidence cannot be built.
    pub fn of(input: &PitchInput<'_>) -> Result<Self, PitchError> {
        let voiced_frames = input
            .track
            .frames()
            .iter()
            .filter(|f| f.f0.is_some())
            .count();
        let counts = PitchCounts {
            voiced_frames,
            ..PitchCounts::default()
        };
        let skip = |flags: PitchFlags, reason: Reason| -> Result<Self, PitchError> {
            Ok(Self {
                glides: Vec::new(),
                drops: Vec::new(),
                vibrato: Vec::new(),
                flags,
                confidence: Confidence::no_estimate(vec![reason])?,
                counts,
            })
        };
        if input.notes.is_empty() {
            let flags = PitchFlags {
                no_notes: true,
                ..PitchFlags::default()
            };
            return skip(
                flags,
                Reason::new(ReasonCode::NoNoteList, Signal::Flag(true))?,
            );
        }
        if input.voicing == Voicing::Polyphonic {
            let flags = PitchFlags {
                chord_skipped: true,
                ..PitchFlags::default()
            };
            return skip(
                flags,
                Reason::new(ReasonCode::PolyphonicLine, Signal::Flag(true))?,
            );
        }
        let note_frames = note_frames(input.track, input.notes);
        let mut flags = PitchFlags::default();
        let mut counts = counts;
        let loudest = loudest_frame(input.mono, input.rate);
        let struck = strikes(input, loudest, &mut flags, &mut counts)?;
        let drops: Vec<DropFit> = struck.iter().map(|s| s.fit).collect();
        let read = glides(input, loudest, &drops);
        counts.too_short = read.too_short;
        flags.glide_too_short = read.too_short > 0;
        let vibrato = vibratos(input.track, &read.glides, &drops, &mut counts);
        Ok(Self {
            glides: read
                .glides
                .into_iter()
                .map(rate_glide)
                .collect::<Result<_, _>>()?,
            drops: struck
                .into_iter()
                .map(rate_drop)
                .collect::<Result<_, _>>()?,
            vibrato: vibrato
                .into_iter()
                .map(rate_vibrato)
                .collect::<Result<_, _>>()?,
            flags,
            confidence: Confidence::estimate(
                if note_frames < NOTE_FRAMES_LOW {
                    Level::Low
                } else {
                    Level::Medium
                },
                None,
                vec![
                    Reason::new(
                        ReasonCode::VoicedFrames,
                        Signal::Count(count(voiced_frames)),
                    )?,
                    Reason::new(ReasonCode::NoteFrames, Signal::Number(note_frames))?,
                ],
            )?,
            counts,
        })
    }

    /// Returns the lines that tell what the layer found.
    #[must_use]
    pub fn summary(&self) -> Vec<String> {
        if self.flags.no_notes {
            return vec!["pitch: no notes, so no glides, drops or vibrato".to_owned()];
        }
        if self.flags.chord_skipped {
            return vec!["pitch: the line plays chords, so no glides, drops or vibrato".to_owned()];
        }
        let glides = self
            .glides
            .iter()
            .map(|g| {
                format!(
                    "{:+.1} st in {:.0} ms at {:.2} s, curve {:.2}{}",
                    g.value.interval.get(),
                    1000.0 * g.value.time.get(),
                    g.value.start.get(),
                    g.value.curve,
                    if g.value.from_silence {
                        ", from silence"
                    } else {
                        ""
                    }
                )
            })
            .collect::<Vec<_>>();
        let drops = self
            .drops
            .iter()
            .map(|d| {
                format!(
                    "{:.1} st, tau {:.1} ms, to {:.1} Hz at {:.2} s",
                    d.value.df().map_or(0.0, Semitones::get),
                    1000.0 * d.value.tau.get(),
                    d.value.f_end.get(),
                    d.value.onset.get()
                )
            })
            .collect::<Vec<_>>();
        let vibrato = self
            .vibrato
            .iter()
            .map(|v| {
                format!(
                    "{:.2} Hz, {:.0} ct at {:.2} s",
                    v.value.rate.get(),
                    v.value.extent().get(),
                    v.value.start.get()
                )
            })
            .collect::<Vec<_>>();
        let list = |items: &[String]| {
            if items.is_empty() {
                String::new()
            } else {
                format!(": {}", items.join("; "))
            }
        };
        vec![
            format!(
                "glides: {}, {} too short{}",
                self.glides.len(),
                self.counts.too_short,
                list(&glides)
            ),
            format!(
                "pitch drops: {} of {} low strikes{}",
                self.drops.len(),
                self.counts.drop_candidates,
                list(&drops)
            ),
            format!(
                "drop skips: {}",
                match self.counts.drop_skips.items() {
                    items if items.is_empty() => "none".to_owned(),
                    items => items.join(", "),
                }
            ),
            format!(
                "vibrato: {} of {} steady spans{}",
                self.vibrato.len(),
                self.counts.vibrato_candidates,
                list(&vibrato)
            ),
        ]
    }
}

/// Finds the glides, with the audio telling where a sound starts, breaks
/// or jumps in level, and the drops where a glide from silence is a drop.
/// `loudest` is the RMS of the loudest frame of the stem.
fn glides(input: &PitchInput<'_>, loudest: f64, drops: &[DropFit]) -> GlideRead {
    let near_drop = |time: Seconds| {
        drops
            .iter()
            .any(|d| (d.onset.get() - time.get()).abs() <= DROP_GLIDE_DISTANCE.get())
    };
    let starts = |from: Seconds, to: Seconds, spans: [(f64, f64); 2], rise: f64| {
        scan(from.get(), to.get(), ONSET_STEP).any(|at| {
            rise_ratio(
                input.mono,
                input.rate,
                Seconds::clamped(0.0),
                Seconds::clamped(at),
                spans,
            ) >= rise
        })
    };
    let rms = |at: f64, span: (f64, f64)| {
        span_rms(
            input.mono,
            input.rate,
            Seconds::clamped(0.0),
            Seconds::clamped(at),
            span,
        )
    };
    let floor = loudest * gain_below(ACTIVE_GATE_DB);
    let dips = |from: Seconds, to: Seconds| {
        let window = (-0.5 * DIP_WINDOW, 0.5 * DIP_WINDOW);
        let side = rms(from.get(), (-DIP_SIDE, 0.0)).max(rms(to.get(), (0.0, DIP_SIDE)));
        let low = scan(from.get(), to.get(), ONSET_STEP)
            .map(|at| rms(at, window))
            .fold(rms(f64::midpoint(from.get(), to.get()), window), f64::min);
        low < side * gain_below(GAP_DIP_DB) || low < floor
    };
    find_glides(
        input.track,
        input.notes,
        |from, to| starts(from, to, STRIKE_SPANS, STRIKE_RISE) || dips(from, to),
        |from, to| starts(from, to, RUN_START_SPANS, RUN_START_RISE) && !near_drop(to),
    )
}

/// The times from `from` to `to`, both included, `step` apart, each the
/// previous plus `step`.
fn scan(from: f64, to: f64, step: f64) -> impl Iterator<Item = f64> {
    std::iter::successors(Some(from), move |at| Some(at + step)).take_while(move |&at| at <= to)
}

/// The amplitude gain of `gate` below a level.
fn gain_below(gate: Decibels) -> f64 {
    Decibels::clamped(-gate.get())
        .to_amplitude_gain()
        .unwrap_or(0.0)
}

/// The share of the track frames inside the notes that are voiced within
/// [`NOTE_FRAME_MATCH`] of a note sounding at the frame, in track time.
fn note_frames(track: &PitchTrack, notes: &NoteList) -> f64 {
    let (mut inside, mut matched) = (0_usize, 0_usize);
    for frame in track.frames() {
        let sounding: Vec<f64> = notes
            .notes()
            .iter()
            .filter(|note| note.sounds_at(frame.time))
            .map(|note| note.pitch().get())
            .collect();
        if sounding.is_empty() {
            continue;
        }
        inside += 1;
        let Some(cents) = frame
            .f0
            .and_then(|f0| MidiPitch::try_from(f0).ok())
            .map(|pitch| 100.0 * pitch.get())
        else {
            continue;
        };
        if sounding
            .iter()
            .any(|pitch| (100.0 * pitch - cents).abs() <= NOTE_FRAME_MATCH.get())
        {
            matched += 1;
        }
    }
    if inside == 0 {
        0.0
    } else {
        cast::usize_to_f64(matched) / cast::usize_to_f64(inside)
    }
}

fn count(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// Fits a drop at the strike of each low note. A note that continues the
/// one before it, with no fall of the band envelope between them, has no
/// onset and so no fit; nor has a band onset where the full-band level
/// does not rise as [`struck_enough`] asks. `loudest` is the RMS of the
/// loudest frame of the stem.
fn strikes(
    input: &PitchInput<'_>,
    loudest: f64,
    flags: &mut PitchFlags,
    counts: &mut PitchCounts,
) -> Result<Vec<Strike>, PitchError> {
    let rate = input.rate.hz().get();
    let mut drops: Vec<Strike> = Vec::new();
    // The search of each note starts after the previous note's search
    // ended, so it never finds the previous strike again.
    let mut previous_to = 0.0_f64;
    let floor = loudest * gain_below(ACTIVE_GATE_DB);
    let band = band_pass_zero_phase(input.mono, input.rate, LINE_LO, LINE_HI)?;
    let band_floor = loudest_frame(&band, input.rate) * gain_below(DROP_BAND_GATE);
    let origin = Seconds::clamped(0.0);
    let skips = &mut counts.drop_skips;
    for note in input.notes.notes() {
        let (note_on, note_off) = (note.onset().get(), note.offset().get());
        let settled = Hz::from(note.pitch());
        let search_from = previous_to.max(note_on - STRIKE_LOOKBACK.get()).max(0.0);
        let search_to = note_on + STRIKE_LOOKAHEAD.get();
        previous_to = previous_to.max(search_to);
        if !(LINE_LO..=LINE_HI).contains(&settled) || search_from >= search_to {
            continue;
        }
        let first = cast::floor_to_usize((search_from - LINE_LEAD).max(0.0) * rate).unwrap_or(0);
        let last = cast::ceil_to_usize((search_to + DROP_FIT_END.get() + LINE_TAIL) * rate)
            .unwrap_or(usize::MAX)
            .min(input.mono.len());
        if first >= last {
            continue;
        }
        let segment = &input.mono[first..last];
        let start = Seconds::clamped(cast::usize_to_f64(first) / rate);
        let line = SubBandLine::of(segment, input.rate, start)?;
        let Some(onset) = band_onset(
            &line,
            Seconds::clamped(search_from),
            Seconds::clamped(search_to),
        ) else {
            skips.no_onset += 1;
            continue;
        };
        let rise = rise_ratio(segment, input.rate, start, onset, STRIKE_SPANS);
        if span_rms(segment, input.rate, start, onset, STRIKE_SPANS[1]) < floor
            || span_rms(&band, input.rate, origin, onset, STRIKE_SPANS[1]) < band_floor
        {
            skips.quiet += 1;
            continue;
        }
        let attack = band_attack(
            &line,
            Seconds::clamped(search_from),
            Seconds::clamped(search_to),
        )
        .unwrap_or(Seconds::clamped(0.0));
        counts.drop_candidates += 1;
        let span_end = Seconds::clamped(note_off.max(onset.get()));
        let mut fit = fit_drop(&line, onset, span_end);
        let mut lo = LINE_LO;
        if fit.is_some_and(|fit| fit.tau < FAST_TAU) || rise >= RUN_START_RISE {
            lo = LINE_LO_FAST;
            let low = SubBandLine::with_low_edge(segment, input.rate, start, lo)?;
            fit = fit_drop(&low, onset, span_end);
        }
        let note_hi = Hz::clamped(NOTE_LINE_EDGE * settled.get());
        if fit.is_some_and(|fit| fit.miss() == Some(DropMiss::Residual))
            && note_hi < LINE_HI
            && rise >= STRIKE_RISE
        {
            let note_line = SubBandLine::with_edges(segment, input.rate, start, lo, note_hi)?;
            if let Some(note_fit) =
                fit_drop(&note_line, onset, span_end).filter(|note_fit| note_fit.miss().is_none())
            {
                fit = Some(note_fit);
            }
        }
        let Some(fit) = fit else {
            skips.no_fall += 1;
            continue;
        };
        if !struck_enough(&fit, rise, attack) {
            skips.no_strike += 1;
            continue;
        }
        let window = (
            Seconds::clamped(
                (onset.get().max(note_on) + WEAK_FROM.get()).max(onset.get() + in_band(&fit)),
            ),
            Seconds::clamped((onset.get() + DROP_FIT_END.get()).min(note_off)),
        );
        if weak_fundamental(segment, input.rate, start, window, settled)? {
            flags.weak_fundamental = true;
            skips.weak_fundamental += 1;
            continue;
        }
        match fit.miss() {
            Some(DropMiss::Size) => skips.no_fall += 1,
            Some(DropMiss::TauBound) => skips.tau_bound += 1,
            Some(DropMiss::Residual) => skips.residual += 1,
            None if !settles_on_a_note(input.notes, &fit, span_end) => skips.off_note += 1,
            None => drops.push(Strike { fit, attack }),
        }
    }
    Ok(drops)
}

/// Whether the strike under a drop rises enough for its fit.
///
/// A fall with `tau` under [`FAST_TAU`] needs [`STRIKE_RISE`], since the
/// line swings where a ringing tone restarts its phase, and an attack of
/// [`STRIKE_ATTACK`] or less, since it ends inside a slower attack, where
/// the line wanders on a rising level. So does a fall
/// that starts at [`LINE_HI`] or above, since a pitch falling into the band
/// from above raises the band envelope with no strike. A slower fall that
/// starts inside the band, with an attack of [`STRIKE_ATTACK`] or less,
/// needs no rise: an 808 note struck while the one before still rings
/// rises by that note's decay alone, under 3 dB with 0.4 s notes and a 1 s
/// decay.
fn struck_enough(fit: &DropFit, rise: f64, attack: Seconds) -> bool {
    if fit.tau < FAST_TAU {
        return rise >= STRIKE_RISE && attack <= STRIKE_ATTACK;
    }
    rise >= STRIKE_RISE || (fit.f_end.get() + fit.df_hz < LINE_HI.get() && attack <= STRIKE_ATTACK)
}

/// Whether a drop settles on a note: a note sounds while the drop is
/// settled, from five time constants after the onset to the end of the fit
/// span at `span_end` or [`DROP_FIT_END`], with its pitch within
/// [`DROP_NOTE_MATCH`] of `f_end`, that starts [`DROP_NOTE_LEAD`] or less
/// before the onset. A slow fall that is not settled by then needs the
/// note at the end of the span. Several partials inside the band make the
/// line swing at a saw's onset into a fall that settles off the note.
fn settles_on_a_note(notes: &NoteList, fit: &DropFit, span_end: Seconds) -> bool {
    let to = (fit.onset.get() + DROP_FIT_END.get()).min(span_end.get());
    let from = (fit.onset.get() + 5.0 * fit.tau.get()).min(to);
    notes.notes().iter().any(|note| {
        note.onset().get() <= to
            && note.onset().get() >= fit.onset.get() - DROP_NOTE_LEAD.get()
            && note.offset().get() >= from
            && (100.0 * note.pitch().get()
                - 100.0 * MidiPitch::try_from(fit.f_end).map_or(f64::NAN, MidiPitch::get))
            .abs()
                <= DROP_NOTE_MATCH.get()
    })
}

/// The full-band RMS over `after` over the RMS over `before`, both spans in
/// seconds from `at`; infinite when the span before is silent. `samples`
/// start at `start`.
fn rise_ratio(
    samples: &[f64],
    rate: SampleRate,
    start: Seconds,
    at: Seconds,
    [before, after]: [(f64, f64); 2],
) -> f64 {
    let (a, b) = (
        span_rms(samples, rate, start, at, after),
        span_rms(samples, rate, start, at, before),
    );
    if b > 0.0 {
        a / b
    } else if a > 0.0 {
        f64::INFINITY
    } else {
        0.0
    }
}

/// The full-band RMS over `(from, to)`, in seconds from `at`. `samples`
/// start at `start`.
fn span_rms(
    samples: &[f64],
    rate: SampleRate,
    start: Seconds,
    at: Seconds,
    (from, to): (f64, f64),
) -> f64 {
    let r = rate.hz().get();
    let index = |t: f64| {
        cast::round_to_usize(((at.get() + t - start.get()) * r).max(0.0))
            .unwrap_or(0)
            .min(samples.len())
    };
    let (a, b) = (index(from), index(to));
    if a >= b {
        return 0.0;
    }
    (samples[a..b].iter().map(|x| x * x).sum::<f64>() / cast::usize_to_f64(b - a)).sqrt()
}

/// The RMS of the loudest [`QUIET_FRAME`] of the stem, with a quarter
/// frame hop.
fn loudest_frame(mono: &[f64], rate: SampleRate) -> f64 {
    let frame = cast::round_to_usize(QUIET_FRAME.get() * rate.hz().get())
        .unwrap_or(4)
        .max(4);
    frame_rms(mono, StftParams::fixed(frame))
        .into_iter()
        .fold(0.0, f64::max)
}

/// The time after the onset at which a fit's pitch falls inside the
/// half-octave band around `f_end`, so a slow fall no longer reads as the
/// band around twice the note.
fn in_band(fit: &DropFit) -> f64 {
    let edge = fit.f_end.get() * (2f64.powf(0.25) - 1.0);
    if fit.df_hz > edge {
        fit.tau.get() * (fit.df_hz / edge).ln()
    } else {
        0.0
    }
}

/// Whether the fundamental of a struck note is weaker than its second
/// harmonic: the median of the half-octave band around twice the note's
/// pitch exceeds [`WEAK_RATIO`] times the band around the pitch, over
/// `window`: from 50 ms after the onset or the note's start, whichever is
/// later, and not before the fit's pitch lies inside the band around the
/// note, to the end of the fit span or of the note. The line follows the
/// strongest partial of its band, so it would read the second harmonic as
/// the note.
fn weak_fundamental(
    segment: &[f64],
    rate: SampleRate,
    start: Seconds,
    window: (Seconds, Seconds),
    pitch: Hz,
) -> Result<bool, PitchError> {
    let r = rate.hz().get();
    let index = |t: f64| cast::round_to_usize(((t - start.get()) * r).max(0.0)).unwrap_or(0);
    let from = index(window.0.get());
    let to = index(window.1.get()).min(segment.len());
    if from >= to {
        return Ok(false);
    }
    let band = |centre: f64| -> Result<f64, PitchError> {
        let lo = Hz::clamped(centre * 2f64.powf(-0.25));
        let hi = Hz::clamped((centre * 2f64.powf(0.25)).min(0.99 * rate.nyquist().get()));
        let mut values = band_magnitude(segment, rate, lo, hi)?[from..to].to_vec();
        values.sort_by(f64::total_cmp);
        Ok(values[values.len() / 2])
    };
    Ok(band(2.0 * pitch.get())? > WEAK_RATIO * band(pitch.get())?)
}

/// Fits a vibrato to each steady span of the track, without 50 ms at each
/// end and without the settling of a drop.
fn vibratos(
    track: &PitchTrack,
    glides: &[GlideFit],
    drops: &[DropFit],
    counts: &mut PitchCounts,
) -> Vec<VibratoFit> {
    let mut found = Vec::new();
    for span in steady_spans(track, glides) {
        let (Some(&(first, _)), Some(&(last, _))) = (span.first(), span.last()) else {
            continue;
        };
        let mut pitches: Vec<f64> = span.iter().map(|(_, f0)| f0.get()).collect();
        pitches.sort_by(f64::total_cmp);
        let lag = track_lag(Hz::clamped(pitches[pitches.len() / 2])).get();
        let mut from = first.get() + RUN_TRIM.get();
        let to = last.get() - RUN_TRIM.get();
        for drop in drops {
            let settled = drop.onset.get() + (5.0 * drop.tau.get()).max(SETTLE_MIN) + lag;
            if drop.onset.get() + lag <= to && settled > from {
                from = settled;
            }
        }
        let frames: Vec<(Seconds, Hz)> = span
            .into_iter()
            .filter(|(time, _)| (from..=to).contains(&time.get()))
            .collect();
        let Some(fit) = fit_vibrato(&frames) else {
            continue;
        };
        counts.vibrato_candidates += 1;
        if fit.accepted() {
            found.push(VibratoFit {
                start: Seconds::clamped(fit.start.get() - lag),
                end: Seconds::clamped(fit.end.get() - lag),
                ..fit
            });
        }
    }
    found
}

fn rate_glide(glide: GlideFit) -> Result<Rated<GlideFit>, ConfidenceError> {
    let (residual, jitter) = (glide.residual.get(), glide.jitter.get());
    let level = if jitter > JITTER_LOW || glide.on_bound {
        Level::Low
    } else if residual <= RESIDUAL_HIGH
        && jitter <= JITTER_HIGH
        && glide.detected >= SPAN_HIGH
        && glide.curve <= CURVE_HIGH
    {
        Level::High
    } else {
        Level::Medium
    };
    Ok(Rated {
        confidence: Confidence::estimate(
            level,
            None,
            vec![
                Reason::new(ReasonCode::MotionSpan, Signal::Number(glide.detected.get()))?,
                Reason::new(ReasonCode::FitResidual, Signal::Number(residual))?,
                Reason::new(ReasonCode::PitchJitter, Signal::Number(jitter))?,
                Reason::new(ReasonCode::CurveBound, Signal::Flag(glide.on_bound))?,
            ],
        )?,
        value: glide,
    })
}

/// An accepted drop with the attack of its strike.
#[derive(Clone, Copy, Debug)]
struct Strike {
    fit: DropFit,
    attack: Seconds,
}

fn rate_drop(strike: Strike) -> Result<Rated<DropFit>, ConfidenceError> {
    let Strike { fit, attack } = strike;
    let level = if attack > STRIKE_ATTACK {
        Level::Low
    } else if fit.residual < DROP_RESIDUAL_HIGH && fit.tau >= FAST_TAU && attack <= ATTACK_HIGH {
        Level::High
    } else {
        Level::Medium
    };
    Ok(Rated {
        confidence: Confidence::estimate(
            level,
            None,
            vec![
                Reason::new(ReasonCode::FitResidual, Signal::Number(fit.residual))?,
                Reason::new(ReasonCode::AttackTime, Signal::Number(attack.get()))?,
            ],
        )?,
        value: fit,
    })
}

fn rate_vibrato(vibrato: VibratoFit) -> Result<Rated<VibratoFit>, ConfidenceError> {
    let cycles = vibrato.cycles();
    let level = if vibrato.kurtosis < KURTOSIS_LOW || vibrato.pitch < VIBRATO_LOW_PITCH {
        Level::Low
    } else if cycles >= CYCLES_HIGH {
        Level::High
    } else {
        Level::Medium
    };
    Ok(Rated {
        confidence: Confidence::estimate(
            level,
            None,
            vec![
                Reason::new(ReasonCode::SineShare, Signal::Number(vibrato.share))?,
                Reason::new(ReasonCode::CyclesInSpan, Signal::Number(cycles))?,
                Reason::new(
                    ReasonCode::ContourKurtosis,
                    Signal::Number(vibrato.kurtosis),
                )?,
            ],
        )?,
        value: vibrato,
    })
}

/// The pitch layer failed.
#[derive(Debug)]
pub enum PitchError {
    /// The rate puts the band of the sub-band line at or above the Nyquist
    /// frequency.
    Filter(FilterError),
    /// A confidence could not be built.
    Confidence(ConfidenceError),
}

impl std::fmt::Display for PitchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Filter(error) => write!(f, "cannot read the pitch layer: {error}"),
            Self::Confidence(error) => write!(f, "cannot rate the pitch layer: {error}"),
        }
    }
}

impl std::error::Error for PitchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Filter(error) => Some(error),
            Self::Confidence(error) => Some(error),
        }
    }
}

impl From<FilterError> for PitchError {
    fn from(error: FilterError) -> Self {
        Self::Filter(error)
    }
}

impl From<ConfidenceError> for PitchError {
    fn from(error: ConfidenceError) -> Self {
        Self::Confidence(error)
    }
}

/// `x` with at most two decimals and no trailing zeros, as the convention
/// strings print their numbers.
#[cfg(test)]
fn num(x: f64) -> String {
    let text = format!("{x:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// The convention `text` holds each phrase of `phrases`.
#[cfg(test)]
fn says(text: &str, phrases: &[String]) {
    for phrase in phrases {
        assert!(
            text.contains(phrase.as_str()),
            "missing {phrase:?} in {text:?}"
        );
    }
}

/// A full-band RMS ratio in whole decibels.
#[cfg(test)]
fn ratio_db(ratio: f64) -> String {
    format!("{:.0} dB", 20.0 * ratio.log10())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::Note;

    #[test]
    fn a_drop_settles_on_a_note_of_its_own_strike() {
        let fit = DropFit {
            onset: Seconds::clamped(1.0),
            f_end: Hz::clamped(69.3),
            df_hz: 100.0,
            tau: Seconds::clamped(0.03),
            residual: 0.05,
            points: 1000,
        };
        let settles = |on: f64| {
            let note = Note::new(
                Seconds::clamped(on),
                Seconds::clamped(1.6),
                MidiPitch::new(37.0).unwrap(),
            )
            .unwrap();
            settles_on_a_note(&NoteList::new(vec![note]), &fit, Seconds::clamped(1.5))
        };
        assert!(settles(1.05));
        assert!(settles(0.86));
        assert!(!settles(0.84));
        assert!(!settles(0.2));
    }

    #[test]
    fn the_least_jitter_of_a_unison_reads_low() {
        let glide = GlideFit {
            start: Seconds::clamped(0.5),
            interval: Semitones::clamped(7.0),
            time: Seconds::clamped(0.2),
            curve: 1.0,
            from_silence: false,
            detected: Seconds::clamped(0.2),
            residual: Cents::clamped(5.0),
            jitter: Cents::clamped(3.45),
            on_bound: false,
        };
        let level = |jitter: f64| {
            rate_glide(GlideFit {
                jitter: Cents::clamped(jitter),
                ..glide
            })
            .unwrap()
            .confidence
            .level()
        };
        // A unison of the generated sets jittered 3.45 cents at 44.1 kHz;
        // mono glides from 3.06 to 3.40 read medium.
        assert_eq!(level(3.45), Some(Level::Low));
        assert_eq!(level(3.4), Some(Level::Medium));
        assert_eq!(level(JITTER_HIGH), Some(Level::High));
    }
}
