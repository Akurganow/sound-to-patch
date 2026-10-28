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
    DROP_CONVENTION, DROP_FIT_END, DROP_FIT_START, DROP_MAX_RESIDUAL, DROP_MIN, DROP_ONSET_SHARE,
    DropFit, ONSET_SMOOTHING, band_onset, fit_drop,
};
pub use glide::{
    CURVE_RANGE, GLIDE_CONVENTION, GLIDE_INTERVAL_MIN, GLIDE_SLOPE_MIN, GLIDE_STEP_MAX,
    GLIDE_TIME_MIN, GlideFit, GlideRead, NOTE_MATCH, PLATEAU_MIN, PLATEAU_RANGE, SMOOTH_FRAMES,
    STEADY_SPLIT, find_glides, steady_spans, track_lag,
};
pub use line::{
    LINE_CONVENTION, LINE_HI, LINE_LO, LINE_MEDIAN, SubBandLine, median_filter, median_len,
};
pub use vibrato::{
    EXTENT_SHARE, RATE_SLACK, VIBRATO_CONVENTION, VIBRATO_EXTENT_MIN, VIBRATO_RATE_MAX,
    VIBRATO_RATE_MIN, VIBRATO_SHARE_MIN, VIBRATO_SPAN_MIN, VibratoFit, extent_factor, fit_vibrato,
};

use crate::audio::SampleRate;
use crate::cast;
use crate::confidence::{Confidence, ConfidenceError, Level, NoSpread, Reason, ReasonCode, Signal};
use crate::dsp::{FilterError, band_magnitude};
use crate::notes::{NoteList, PitchTrack, Voicing};
use crate::units::{Cents, Hz, MidiPitch, Seconds, Semitones};

/// The onset search of a strike starts at most this long before the note
/// onset, because the transcriber places the onset of a falling note late.
pub const STRIKE_LOOKBACK: Seconds = Seconds::from_const(0.5);
/// The onset search ends this long after the note onset.
pub const STRIKE_LOOKAHEAD: Seconds = Seconds::from_const(0.1);
/// The vibrato search skips this much at each end of a steady span.
pub const RUN_TRIM: Seconds = Seconds::from_const(0.05);
/// A glide from silence is a drop when its run starts this close to the
/// onset of an accepted drop.
pub const DROP_GLIDE_DISTANCE: Seconds = Seconds::from_const(0.2);
/// A drop settles on a note whose pitch lies within this many cents of
/// `f_end`.
pub const DROP_NOTE_MATCH: Cents = Cents::from_const(100.0);
/// The harmonic check of a strike reads the bands from this long after the
/// onset.
pub const WEAK_FROM: Seconds = Seconds::from_const(0.05);
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
pub const JITTER_HIGH: f64 = 3.5;
/// The shortest motion of a high-confidence glide.
pub const SPAN_HIGH: Seconds = Seconds::from_const(0.15);
/// A glide whose residual or jitter exceeds these reads at low confidence.
pub const RESIDUAL_LOW: f64 = 40.0;
/// The jitter above which a glide reads at low confidence.
pub const JITTER_LOW: f64 = 4.0;
/// A drop whose residual share stays below this reads at high confidence.
pub const DROP_RESIDUAL_HIGH: f64 = 0.05;
/// A vibrato whose sine explains at least this share of the power reads
/// at high confidence.
pub const SHARE_HIGH: f64 = 0.8;

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
    /// A motion between two notes lasted 60 to 90 ms: a glide too short to
    /// read.
    pub glide_too_short: bool,
    /// A low note whose fundamental is weaker than its second harmonic had
    /// its drop fit dropped, since the band-pass then reads the wrong
    /// partial.
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
    /// Steady spans long enough for the vibrato search.
    pub vibrato_candidates: usize,
    /// Voiced frames of the track.
    pub voiced_frames: usize,
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
        let mut flags = PitchFlags::default();
        let mut counts = counts;
        let drops = strikes(input, &mut flags, &mut counts)?;
        let near_drop = |time: Seconds| {
            drops
                .iter()
                .any(|d| (d.onset.get() - time.get()).abs() <= DROP_GLIDE_DISTANCE.get())
        };
        let read = find_glides(input.track, input.notes, near_drop);
        counts.too_short = read.too_short;
        flags.glide_too_short = read.too_short > 0;
        let vibrato = vibratos(input.track, &read.glides, &drops, &mut counts);
        Ok(Self {
            glides: read
                .glides
                .into_iter()
                .map(rate_glide)
                .collect::<Result<_, _>>()?,
            drops: drops.into_iter().map(rate_drop).collect::<Result<_, _>>()?,
            vibrato: vibrato
                .into_iter()
                .map(rate_vibrato)
                .collect::<Result<_, _>>()?,
            flags,
            confidence: Confidence::estimate(
                Level::Medium,
                None,
                vec![Reason::new(
                    ReasonCode::VoicedFrames,
                    Signal::Count(count(voiced_frames)),
                )?],
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
                "pitch drops: {} of {} low strikes{}{}",
                self.drops.len(),
                self.counts.drop_candidates,
                if self.flags.weak_fundamental {
                    ", a weak fundamental skipped"
                } else {
                    ""
                },
                list(&drops)
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

fn count(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

/// Fits a drop at the strike of each low note. A note that continues the
/// one before it, with no fall of the band envelope between them, has no
/// onset and so no fit.
fn strikes(
    input: &PitchInput<'_>,
    flags: &mut PitchFlags,
    counts: &mut PitchCounts,
) -> Result<Vec<DropFit>, PitchError> {
    let rate = input.rate.hz().get();
    let mut drops: Vec<DropFit> = Vec::new();
    // The search of each note starts after the previous note's search
    // ended, so it never finds the previous strike again.
    let mut previous_to = 0.0_f64;
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
            continue;
        };
        if drops
            .iter()
            .any(|d| (d.onset.get() - onset.get()).abs() < 1e-9)
        {
            continue;
        }
        counts.drop_candidates += 1;
        if weak_fundamental(segment, input.rate, start, onset, settled)? {
            flags.weak_fundamental = true;
            continue;
        }
        let Some(fit) = fit_drop(&line, onset, Seconds::clamped(note_off.max(onset.get()))) else {
            continue;
        };
        if fit.accepted() && settles_on_a_note(input.notes, &fit) {
            drops.push(fit);
        }
    }
    Ok(drops)
}

/// Whether a drop settles on a note: a note sounds while the drop is
/// settled, from five time constants after the onset to the end of the fit
/// span, with its pitch within [`DROP_NOTE_MATCH`] of `f_end`. Several
/// partials inside the band make the line swing at a saw's onset into a
/// fall that settles off the note.
fn settles_on_a_note(notes: &NoteList, fit: &DropFit) -> bool {
    let from = fit.onset.get() + 5.0 * fit.tau.get();
    let to = fit.onset.get() + DROP_FIT_END.get();
    notes.notes().iter().any(|note| {
        note.onset().get() < to
            && note.offset().get() > from
            && (100.0 * note.pitch().get()
                - 100.0 * MidiPitch::try_from(fit.f_end).map_or(f64::NAN, MidiPitch::get))
            .abs()
                <= DROP_NOTE_MATCH.get()
    })
}

/// Whether the fundamental of a struck note is weaker than its second
/// harmonic: the median of the half-octave band around the note's pitch
/// against the band around twice it, from 50 ms after the onset to the end
/// of the fit span. The line follows the strongest partial of its band, so
/// it would read the second harmonic as the note.
fn weak_fundamental(
    segment: &[f64],
    rate: SampleRate,
    start: Seconds,
    onset: Seconds,
    pitch: Hz,
) -> Result<bool, PitchError> {
    let r = rate.hz().get();
    let index = |t: f64| cast::round_to_usize(((t - start.get()) * r).max(0.0)).unwrap_or(0);
    let from = index(onset.get() + WEAK_FROM.get());
    let to = index(onset.get() + DROP_FIT_END.get()).min(segment.len());
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
    Ok(band(pitch.get())? < band(2.0 * pitch.get())?)
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
            let settled = drop.onset.get() + (5.0 * drop.tau.get()).max(0.1) + lag;
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
    let level = if residual > RESIDUAL_LOW || jitter > JITTER_LOW {
        Level::Low
    } else if residual <= RESIDUAL_HIGH && jitter <= JITTER_HIGH && glide.detected >= SPAN_HIGH {
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
            ],
        )?,
        value: glide,
    })
}

fn rate_drop(drop: DropFit) -> Result<Rated<DropFit>, ConfidenceError> {
    let level = if drop.residual < DROP_RESIDUAL_HIGH {
        Level::High
    } else {
        Level::Medium
    };
    Ok(Rated {
        confidence: Confidence::estimate(
            level,
            None,
            vec![Reason::new(
                ReasonCode::FitResidual,
                Signal::Number(drop.residual),
            )?],
        )?,
        value: drop,
    })
}

fn rate_vibrato(vibrato: VibratoFit) -> Result<Rated<VibratoFit>, ConfidenceError> {
    let cycles = vibrato.rate.get() * (vibrato.end.get() - vibrato.start.get());
    let level = if vibrato.share >= SHARE_HIGH && cycles >= 4.0 {
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
