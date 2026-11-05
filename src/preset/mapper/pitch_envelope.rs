//! The synth-neutral flow from the pitch, voices and envelope groups to
//! suggested edits.
//!
//! [`plan`] reads the groups and decides what a patch should hold: the
//! voicing, the glide, the pitch drop, the velocity path and the amplitude
//! envelope, each in physical units with its confidence. It leaves out a
//! low-confidence or flagged field and lists it in `unwritten`. An adapter
//! implements [`PitchEnvelopeControls`], which turns one [`Wanted`] into
//! the edits of its synth through its own keys and laws. [`map`] runs the
//! plan through an adapter, drops each value equal to its default and
//! sorts the edits.
//!
//! The laws are interim until the calibration set (epic #11) measures
//! them. No law maps the fitted glide curve or the envelope contour, so the
//! curves stay at their defaults and `unwritten` says "curve not set".

use crate::confidence::{Estimate, Level, Spread};
use crate::dsp::{Stats, Summary};
use crate::estimate::envelope::{EnvelopeLayer, HOLD_MIN};
use crate::estimate::pitch::PitchLayer;
use crate::estimate::voices::VoicesLayer;
use crate::preset::edits::{Record, Resolved, resolve_record};
use crate::preset::mapper::{MapContext, Mapped, Unwritten, UnwrittenReason, sort_edits};
use crate::preset::registry::ParamRegistry;
use crate::units::{Decibels, Seconds, Semitones};

/// The spread of the pooled peak levels above which velocity gets a path to
/// the level.
pub const VELOCITY_SPREAD: Decibels = Decibels::from_const(3.0);

/// Whether velocity gets a path to the level: the level spread has a value,
/// read over at least three pooled notes, and exceeds [`VELOCITY_SPREAD`].
///
/// The writer of the final `notes.mid` is to read this rule too (#72): a
/// velocity per note from its peak level when it holds, one constant
/// velocity otherwise. The spread itself is always low confidence, so the
/// rule does not gate on its level.
///
/// ```
/// use sound_to_patch::preset::mapper::pitch_envelope::velocity_reaches_level;
/// use sound_to_patch::units::Decibels;
///
/// assert!(velocity_reaches_level(Some(Decibels::new(4.0)?)));
/// assert!(!velocity_reaches_level(Some(Decibels::new(3.0)?)));
/// assert!(!velocity_reaches_level(None));
/// # Ok::<(), sound_to_patch::units::RangeError>(())
/// ```
#[must_use]
pub fn velocity_reaches_level(level_spread: Option<Decibels>) -> bool {
    level_spread.is_some_and(|spread| spread.get() > VELOCITY_SPREAD.get())
}

/// A measured value and how far to trust it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading<T> {
    /// The value.
    pub value: T,
    /// Its confidence.
    pub level: Level,
}

/// The pitch, voices and envelope groups of the layer report, as the
/// mapper reads them. Each group is `None` when the report holds no
/// values for it.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct PitchEnvelopeGroups {
    /// The `pitch` group.
    pub pitch: Option<PitchValues>,
    /// The `voices` group.
    pub voices: Option<VoicesValues>,
    /// The `envelope` group.
    pub envelope: Option<EnvelopeValues>,
}

/// The values of the `pitch` group that the mapper reads.
#[derive(Clone, Debug, PartialEq)]
pub struct PitchValues {
    /// The level of the group.
    pub level: Level,
    /// `glides`.
    pub glides: Vec<GlideValue>,
    /// `drops`.
    pub drops: Vec<DropValue>,
}

/// A glide record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlideValue {
    /// `time_s`: the full 0 to 100 % time.
    pub time: Seconds,
    /// `from_silence`.
    pub from_silence: bool,
    /// The record's level.
    pub level: Level,
}

/// A pitch-drop record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DropValue {
    /// `df_st`: the start pitch above the settled pitch.
    pub df: Semitones,
    /// `tau_s`: the time constant.
    pub tau: Seconds,
    /// The record's level.
    pub level: Level,
}

/// The values of the `voices` group that the mapper reads.
#[derive(Clone, Debug, PartialEq)]
pub struct VoicesValues {
    /// The level of the group.
    pub level: Level,
    /// `mono`.
    pub mono: bool,
    /// `max_polyphony`.
    pub max_polyphony: usize,
    /// `joins`.
    pub joins: Vec<JoinValue>,
}

/// A join record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JoinValue {
    /// `legato`: the join shows no new attack; `None` without a verdict.
    pub legato: Option<bool>,
    /// The record's level.
    pub level: Level,
}

/// The values of the `envelope` group that the mapper reads. A statistic
/// is its median, `None` when it has no value.
#[derive(Clone, Debug, PartialEq)]
pub struct EnvelopeValues {
    /// The level of the group.
    pub level: Level,
    /// `attack_s`: the full 0 to 100 % time.
    pub attack: Option<Reading<Seconds>>,
    /// `attack_is_upper_bound`.
    pub attack_is_upper_bound: bool,
    /// `hold_s`.
    pub hold: Option<Reading<Seconds>>,
    /// `decay_s`: from the peak to the sustain level.
    pub decay: Option<Reading<Seconds>>,
    /// `sustain_db`: the held level from the peak.
    pub sustain: Option<Reading<Decibels>>,
    /// `release_s`: the linear release after the note-off knee.
    pub release: Option<Reading<Seconds>>,
    /// `release_confidence`.
    pub release_confidence: Option<Level>,
    /// `level_spread_db`.
    pub level_spread: Option<Decibels>,
}

impl PitchEnvelopeGroups {
    /// The groups of the three layers, with the values `layers.json` holds.
    #[must_use]
    pub fn from_layers(pitch: &PitchLayer, voices: &VoicesLayer, envelope: &EnvelopeLayer) -> Self {
        Self {
            pitch: pitch.confidence.level().map(|level| PitchValues {
                level,
                glides: pitch
                    .glides
                    .iter()
                    .filter_map(|glide| {
                        Some(GlideValue {
                            time: glide.value.time,
                            from_silence: glide.value.from_silence,
                            level: glide.confidence.level()?,
                        })
                    })
                    .collect(),
                drops: pitch
                    .drops
                    .iter()
                    .filter_map(|drop| {
                        Some(DropValue {
                            df: drop.value.df()?,
                            tau: drop.value.tau,
                            level: drop.confidence.level()?,
                        })
                    })
                    .collect(),
            }),
            voices: voices.confidence.level().map(|level| VoicesValues {
                level,
                mono: voices.mono,
                max_polyphony: voices.max_polyphony,
                joins: voices
                    .joins
                    .iter()
                    .filter_map(|join| {
                        Some(JoinValue {
                            legato: join.legato,
                            level: join.confidence.level()?,
                        })
                    })
                    .collect(),
            }),
            envelope: envelope.confidence.level().map(|level| EnvelopeValues {
                level,
                attack: pooled(&envelope.attack),
                attack_is_upper_bound: envelope.flags.attack_is_upper_bound,
                hold: pooled(&envelope.hold),
                decay: pooled(&envelope.decay),
                sustain: pooled(&envelope.sustain),
                release: pooled(&envelope.release),
                release_confidence: envelope.release_confidence,
                level_spread: envelope.level_spread.value().copied(),
            }),
        }
    }
}

/// The median of a pooled statistic and its level.
fn pooled<T: Copy, S: Spread>(estimate: &Estimate<Stats<T>, S>) -> Option<Reading<T>> {
    Some(Reading {
        value: estimate.value()?.median(),
        level: estimate.confidence().level()?,
    })
}

/// One setting the groups ask of a synth, in physical units.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Want {
    /// One voice at a time, or more.
    Mono(bool),
    /// A held note slides into the next without a new attack.
    Legato(bool),
    /// The count of voices: the most notes at once, times 1 unison voice
    /// and 1 oscillator. The unison mapper sets the final value (#121,
    /// #186).
    Voices(usize),
    /// The full 0 to 100 % glide time. The adapter clamps it to the synth's
    /// maximum and lists the rest.
    GlideTime(Seconds),
    /// A note glides even after silence.
    GlideFromSilence,
    /// A unipolar envelope into oscillator pitch: the start pitch `depth`
    /// above the settled pitch, falling with time constant `tau`. The depth
    /// law and the envelope are the adapter's interim proposals.
    PitchDrop {
        /// The drop in semitones.
        depth: Semitones,
        /// The time constant.
        tau: Seconds,
    },
    /// A path from velocity to the level, with the spread it should cover.
    VelocityToLevel {
        /// The 90th minus the 10th percentile of the pooled peak levels.
        spread: Decibels,
    },
    /// The amplitude attack, 0 to 100 %.
    Attack(Seconds),
    /// The amplitude hold.
    Hold(Seconds),
    /// The amplitude decay, from the peak to the sustain level.
    Decay(Seconds),
    /// The sustain level from the peak. The adapter's law turns it into its
    /// stored value.
    Sustain(Decibels),
    /// The amplitude release after the note-off.
    Release(Seconds),
}

impl Want {
    /// The path of the field it comes from, in its group's `values`.
    #[must_use]
    pub fn field(&self) -> &'static str {
        match self {
            Self::Mono(_) => "mono",
            Self::Legato(_) => "joins.legato",
            Self::Voices(_) => "max_polyphony",
            Self::GlideTime(_) => "glides.time_s",
            Self::GlideFromSilence => "glides.from_silence",
            Self::PitchDrop { .. } => "drops.df_st",
            Self::VelocityToLevel { .. } => "level_spread_db",
            Self::Attack(_) => "attack_s",
            Self::Hold(_) => "hold_s",
            Self::Decay(_) => "decay_s",
            Self::Sustain(_) => "sustain_db",
            Self::Release(_) => "release_s",
        }
    }

    /// The layer group it comes from, which is the reason of its edits.
    #[must_use]
    pub fn group(&self) -> &'static str {
        match self {
            Self::Mono(_) | Self::Legato(_) | Self::Voices(_) => "voices",
            Self::GlideTime(_) | Self::GlideFromSilence | Self::PitchDrop { .. } => "pitch",
            Self::VelocityToLevel { .. }
            | Self::Attack(_)
            | Self::Hold(_)
            | Self::Decay(_)
            | Self::Sustain(_)
            | Self::Release(_) => "envelope",
        }
    }
}

/// A setting and the confidence its edits carry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wanted {
    /// The setting.
    pub want: Want,
    /// The lowest level of the records behind it, capped by its group.
    pub confidence: Level,
}

/// What the groups ask of a synth, and what they leave unwritten.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    /// The settings, in the order of the groups.
    pub wants: Vec<Wanted>,
    /// The measured fields left out, with the reason.
    pub unwritten: Vec<Unwritten>,
}

/// The controls and laws of one synth for the pitch and envelope groups.
pub trait PitchEnvelopeControls {
    /// The edits of one setting: `fit_param` and its siblings with the
    /// synth's keys, `wanted.want.field()` as the field and
    /// `wanted.want.group()` as the reason. A setting the synth has no
    /// control for gives a `not_in_synth` entry; one whose effect is
    /// unprobed gives an `unprobed` entry in `unwritten`.
    fn edits(&self, wanted: &Wanted, ctx: &MapContext<'_>) -> Mapped;
}

/// The edits of the groups through one adapter: the plan, then each
/// setting through `controls`. A parameter whose value equals its default
/// gives no edit. The edits are sorted by key.
#[must_use]
pub fn map(
    groups: &PitchEnvelopeGroups,
    ctx: &MapContext<'_>,
    controls: &dyn PitchEnvelopeControls,
) -> Mapped {
    let plan = plan(groups);
    let mut mapped = Mapped {
        unwritten: plan.unwritten,
        ..Mapped::default()
    };
    for wanted in &plan.wants {
        mapped.extend(controls.edits(wanted, ctx));
    }
    mapped
        .edits
        .retain(|edit| !equals_default(&edit.edit.record, ctx.registry));
    sort_edits(&mut mapped);
    mapped
}

/// Whether a parameter record resolves to its default stored value.
fn equals_default(record: &Record, registry: &dyn ParamRegistry) -> bool {
    if !matches!(record, Record::Param { .. } | Record::Menu { .. }) {
        return false;
    }
    let Ok(Resolved::Param { key, stored, .. }) = resolve_record(record, registry) else {
        return false;
    };
    registry.get(&key).is_some_and(|spec| {
        // A unit string prints a rounded number, so a value that round
        // trips to the default counts as the default.
        (stored - spec.default).abs() <= 1e-9 * (spec.stored_max - spec.stored_min).abs()
    })
}

/// The settings the groups ask for and the fields they leave unwritten.
#[must_use]
pub fn plan(groups: &PitchEnvelopeGroups) -> Plan {
    let mut plan = Plan::default();
    if let Some(voices) = &groups.voices {
        plan_voices(voices, &mut plan);
    }
    if let Some(pitch) = &groups.pitch {
        plan_pitch(pitch, &mut plan);
    }
    if let Some(envelope) = &groups.envelope {
        plan_envelope(envelope, &mut plan);
    }
    plan
}

impl Plan {
    fn want(&mut self, want: Want, confidence: Level) {
        self.wants.push(Wanted { want, confidence });
    }

    fn skip(&mut self, field: &str, reason: UnwrittenReason) {
        self.unwritten.push(Unwritten::new(field, reason));
    }
}

/// Mono or poly, legato and the voice count. A poly stem writes legato off.
fn plan_voices(voices: &VoicesValues, plan: &mut Plan) {
    let group = voices.level;
    if group == Level::Low {
        plan.skip(
            if voices.mono { "mono" } else { "max_polyphony" },
            UnwrittenReason::LowConfidence,
        );
        return;
    }
    if !voices.mono {
        plan.want(Want::Mono(false), group);
        plan.want(Want::Legato(false), group);
        plan.want(Want::Voices(voices.max_polyphony), group);
        return;
    }
    plan.want(Want::Mono(true), group);
    let verdicts: Vec<(bool, Level)> = voices
        .joins
        .iter()
        .filter_map(|join| Some((join.legato?, join.level.min(group))))
        .filter(|(_, level)| *level > Level::Low)
        .collect();
    // Legato on when a confident join reads legato, off when every
    // confident join retriggers: a synth's legato acts only where notes
    // overlap, so one legato join needs it on.
    let on: Vec<Level> = verdicts
        .iter()
        .filter(|(legato, _)| *legato)
        .map(|(_, level)| *level)
        .collect();
    if let Some(level) = on.iter().min() {
        plan.want(Want::Legato(true), *level);
    } else if let Some(level) = verdicts.iter().map(|(_, level)| *level).min() {
        plan.want(Want::Legato(false), level);
    } else if !voices.joins.is_empty() {
        plan.skip("joins.legato", UnwrittenReason::LowConfidence);
    }
}

/// The glide time and always-glide from the confident glides, and the pitch
/// drop from the confident drops.
fn plan_pitch(pitch: &PitchValues, plan: &mut Plan) {
    let group = pitch.level;
    let glides: Vec<&GlideValue> = pitch
        .glides
        .iter()
        .filter(|glide| glide.level.min(group) > Level::Low)
        .collect();
    if let Some(level) = lowest(glides.iter().map(|glide| glide.level), group) {
        let time = median(glides.iter().map(|glide| glide.time.get()));
        plan.want(Want::GlideTime(Seconds::clamped(time)), level);
        plan.skip("glides.curve", UnwrittenReason::CurveNotSet);
        if glides.iter().any(|glide| glide.from_silence) {
            plan.want(Want::GlideFromSilence, level);
        }
    } else if !pitch.glides.is_empty() {
        plan.skip("glides.time_s", UnwrittenReason::LowConfidence);
    }
    let drops: Vec<&DropValue> = pitch
        .drops
        .iter()
        .filter(|drop| drop.level.min(group) > Level::Low)
        .collect();
    if let Some(level) = lowest(drops.iter().map(|drop| drop.level), group) {
        let depth = median(drops.iter().map(|drop| drop.df.get()));
        let tau = median(drops.iter().map(|drop| drop.tau.get()));
        plan.want(
            Want::PitchDrop {
                depth: Semitones::clamped(depth),
                tau: Seconds::clamped(tau),
            },
            level,
        );
    } else if !pitch.drops.is_empty() {
        plan.skip("drops.df_st", UnwrittenReason::LowConfidence);
    }
}

/// The amplitude stages and the velocity path. Tail and pump have no
/// envelope key: they belong to the space and modulation layers.
fn plan_envelope(envelope: &EnvelopeValues, plan: &mut Plan) {
    let group = envelope.level;
    if envelope.attack_is_upper_bound {
        plan.skip("attack_s", UnwrittenReason::UpperBound);
    } else {
        stage(plan, envelope.attack, group, Want::Attack, true);
    }
    // The estimator reads a hold under HOLD_MIN as 0: no plateau, no edit.
    match envelope.hold {
        Some(hold) if hold.value < HOLD_MIN => {}
        hold => stage(plan, hold, group, Want::Hold, false),
    }
    stage(plan, envelope.decay, group, Want::Decay, true);
    match envelope
        .sustain
        .filter(|sustain| sustain.level.min(group) > Level::Low)
    {
        Some(sustain) => plan.want(Want::Sustain(sustain.value), sustain.level.min(group)),
        None => plan.skip("sustain_db", UnwrittenReason::LowConfidence),
    }
    // A release under a heavy tail or with a large residual has no value or
    // a low release confidence: the report cannot tell it.
    let identifiable = envelope
        .release_confidence
        .is_some_and(|level| level > Level::Low);
    match (envelope.release, identifiable) {
        (release @ Some(_), true) => stage(plan, release, group, Want::Release, true),
        _ => plan.skip("release_s", UnwrittenReason::NotIdentifiable),
    }
    match envelope.level_spread {
        Some(spread) if velocity_reaches_level(Some(spread)) => {
            plan.want(Want::VelocityToLevel { spread }, Level::Low);
        }
        Some(_) => {}
        None => plan.skip("level_spread_db", UnwrittenReason::LowConfidence),
    }
}

/// A timed stage: its setting when confident, else a low-confidence entry;
/// a stage with a curve also lists the curve as not set.
fn stage(
    plan: &mut Plan,
    reading: Option<Reading<Seconds>>,
    group: Level,
    want: fn(Seconds) -> Want,
    curve: bool,
) {
    let field = want(Seconds::clamped(0.0)).field();
    match reading.filter(|reading| reading.level.min(group) > Level::Low) {
        Some(reading) => {
            plan.want(want(reading.value), reading.level.min(group));
            if curve {
                plan.skip(field, UnwrittenReason::CurveNotSet);
            }
        }
        None => plan.skip(field, UnwrittenReason::LowConfidence),
    }
}

/// The lowest of some levels capped by the group, or `None` when there are
/// none.
fn lowest(levels: impl Iterator<Item = Level>, group: Level) -> Option<Level> {
    levels.min().map(|level| level.min(group))
}

/// The median of a non-empty list.
fn median(values: impl Iterator<Item = f64>) -> f64 {
    let values: Vec<f64> = values.collect();
    Summary::of(&values).map_or(0.0, Summary::median)
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use super::*;
    use crate::cast::usize_to_f64;
    use crate::preset::edits::{EditList, ParamValue, Path, PolarityText, parse, resolve};
    use crate::preset::mapper::{
        UnrepresentableReason, fit_flag, fit_param, fit_route, fit_stored,
    };
    use crate::preset::registry::tests::spec;
    use crate::preset::registry::{
        Law, ParamKind, ParamSpec, Physical, Provenance, Registry, UnitKind,
    };
    use crate::synth::Synth;

    const LINEAR: Law = Law::Affine {
        scale: 1.0,
        offset: 0.0,
    };

    fn seconds(key: &str, max: f64, default: f64) -> ParamSpec {
        ParamSpec {
            default,
            ..spec(key, LINEAR, 0.0, max, UnitKind::Seconds)
        }
    }

    fn flag(key: &str) -> ParamSpec {
        ParamSpec {
            kind: ParamKind::Flag,
            law: None,
            ..spec(key, LINEAR, 0.0, 1.0, UnitKind::Seconds)
        }
    }

    fn interim(spec: ParamSpec) -> ParamSpec {
        ParamSpec {
            provenance: Provenance::Interim,
            tuning_note: Some(format!("tune {} by ear", spec.key)),
            ..spec
        }
    }

    /// A synth with one control per setting. Glide time, sustain, velocity
    /// and the drop envelope are interim, as on a synth with no measured
    /// law for them.
    static TEST_REGISTRY: LazyLock<Registry> = LazyLock::new(|| {
        Registry::new(
            Synth::Vital,
            vec![
                flag("mono"),
                flag("legato"),
                flag("glide_always"),
                ParamSpec {
                    kind: ParamKind::Count,
                    law: None,
                    default: 8.0,
                    ..spec("voices", LINEAR, 1.0, 16.0, UnitKind::Seconds)
                },
                interim(seconds("glide_time", 10.0, 0.0)),
                interim(ParamSpec {
                    law: None,
                    ..spec("glide_curve", LINEAR, -1.0, 1.0, UnitKind::Seconds)
                }),
                seconds("attack", 20.0, 0.001),
                seconds("hold", 20.0, 0.0),
                seconds("decay", 20.0, 1.0),
                interim(ParamSpec {
                    law: None,
                    default: 1.0,
                    ..spec("sustain", LINEAR, 0.0, 1.0, UnitKind::Seconds)
                }),
                seconds("release", 20.0, 0.05),
                interim(spec(
                    "velocity_amount",
                    Law::Affine {
                        scale: 100.0,
                        offset: 0.0,
                    },
                    0.0,
                    1.0,
                    UnitKind::PercentUnipolar,
                )),
                ParamSpec {
                    kind: ParamKind::Count,
                    default: 0.0,
                    ..spec("pitch_transpose", LINEAR, -24.0, 24.0, UnitKind::Semitones)
                },
                interim(seconds("drop_decay", 20.0, 1.0)),
            ],
        )
        .unwrap()
    });

    /// The test synth's laws: the interim laws this issue names. Sustain is
    /// linear amplitude, velocity gets 50 %, the drop envelope decays over
    /// three time constants.
    struct TestControls;

    impl PitchEnvelopeControls for TestControls {
        fn edits(&self, wanted: &Wanted, ctx: &MapContext<'_>) -> Mapped {
            let (registry, field) = (ctx.registry, wanted.want.field());
            let (reason, level) = (wanted.want.group(), wanted.confidence);
            let time = |key: &str, value: Seconds| {
                fit_param(
                    registry,
                    field,
                    key,
                    Physical::Seconds(value),
                    reason,
                    level,
                )
            };
            match wanted.want {
                Want::Mono(on) => fit_flag(registry, field, "mono", on, reason, level),
                Want::Legato(on) => fit_flag(registry, field, "legato", on, reason, level),
                Want::Voices(count) => fit_stored(
                    registry,
                    field,
                    "voices",
                    usize_to_f64(count),
                    reason,
                    level,
                ),
                Want::GlideTime(value) => time("glide_time", value),
                Want::GlideFromSilence => {
                    fit_flag(registry, field, "glide_always", true, reason, level)
                }
                Want::PitchDrop { depth, tau } => {
                    let mut mapped = fit_route(
                        registry,
                        field,
                        ("env_2", "pitch_transpose"),
                        depth,
                        Some(PolarityText::Unipolar),
                        reason,
                        level,
                    );
                    mapped.extend(time("drop_decay", Seconds::clamped(3.0 * tau.get())));
                    mapped
                }
                Want::VelocityToLevel { .. } => fit_param(
                    registry,
                    field,
                    "velocity_amount",
                    Physical::new(UnitKind::PercentUnipolar, 50.0).unwrap(),
                    reason,
                    level,
                ),
                Want::Attack(value) => time("attack", value),
                Want::Hold(value) => time("hold", value),
                Want::Decay(value) => time("decay", value),
                Want::Release(value) => time("release", value),
                Want::Sustain(db) => fit_stored(
                    registry,
                    field,
                    "sustain",
                    10f64.powf(db.get() / 20.0),
                    reason,
                    level,
                ),
            }
        }
    }

    /// The keys an adapter writes each setting to, for the cases below.
    struct Keys {
        mono: &'static str,
        voices: &'static str,
        glide_time: &'static str,
        glide_curve: &'static str,
        glide_always: &'static str,
        drop_dest: &'static str,
        drop_decay: &'static str,
        velocity: &'static str,
        attack: &'static str,
        sustain: &'static str,
        release: &'static str,
    }

    struct Adapter {
        controls: &'static (dyn PitchEnvelopeControls + Sync),
        registry: &'static Registry,
        keys: Keys,
    }

    /// Every adapter the cases run on. An adapter task adds its synth here.
    fn adapters() -> Vec<Adapter> {
        vec![Adapter {
            controls: &TestControls,
            registry: &TEST_REGISTRY,
            keys: Keys {
                mono: "mono",
                voices: "voices",
                glide_time: "glide_time",
                glide_curve: "glide_curve",
                glide_always: "glide_always",
                drop_dest: "pitch_transpose",
                drop_decay: "drop_decay",
                velocity: "velocity_amount",
                attack: "attack",
                sustain: "sustain",
                release: "release",
            },
        }]
    }

    impl Adapter {
        fn run(&self, groups: &PitchEnvelopeGroups) -> Mapped {
            let ctx = MapContext {
                registry: self.registry,
                capabilities: &crate::preset::vital::CAPABILITIES,
            };
            map(groups, &ctx, self.controls)
        }

        /// The edits resolved through the edit-list path, as `preset
        /// apply` reads them.
        fn resolved(&self, mapped: &Mapped) -> Vec<Resolved> {
            let list = EditList {
                synth: self.registry.synth(),
                edits: mapped.edits.iter().map(|edit| edit.edit.clone()).collect(),
                ambiguities: Vec::new(),
            };
            let text = serde_json::to_string(&list).unwrap();
            resolve(&parse(&text, Path::Agent).unwrap(), self.registry).unwrap()
        }

        fn stored(&self, mapped: &Mapped, key: &str) -> Option<f64> {
            self.resolved(mapped)
                .into_iter()
                .find_map(|item| match item {
                    Resolved::Param {
                        key: name, stored, ..
                    } if name == key => Some(stored),
                    _ => None,
                })
        }
    }

    fn written(mapped: &Mapped, key: &str) -> bool {
        mapped
            .edits
            .iter()
            .any(|edit| edit.edit.record.name() == key)
    }

    fn skipped(mapped: &Mapped, field: &str, reason: UnwrittenReason) -> bool {
        mapped.unwritten.contains(&Unwritten::new(field, reason))
    }

    fn s(value: f64) -> Seconds {
        Seconds::clamped(value)
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "a reading of the groups is optional"
    )]
    fn high<T>(value: T) -> Option<Reading<T>> {
        Some(Reading {
            value,
            level: Level::High,
        })
    }

    fn glide(time: f64, from_silence: bool, level: Level) -> GlideValue {
        GlideValue {
            time: s(time),
            from_silence,
            level,
        }
    }

    fn mono_line(glides: Vec<GlideValue>, drops: Vec<DropValue>) -> PitchEnvelopeGroups {
        PitchEnvelopeGroups {
            pitch: Some(PitchValues {
                level: Level::High,
                glides,
                drops,
            }),
            voices: Some(VoicesValues {
                level: Level::High,
                mono: true,
                max_polyphony: 1,
                joins: Vec::new(),
            }),
            envelope: None,
        }
    }

    fn envelope() -> EnvelopeValues {
        EnvelopeValues {
            level: Level::High,
            attack: high(s(0.02)),
            attack_is_upper_bound: false,
            hold: high(s(0.0)),
            decay: high(s(0.3)),
            sustain: high(Decibels::clamped(-6.0)),
            release: high(s(0.2)),
            release_confidence: Some(Level::Medium),
            level_spread: Some(Decibels::clamped(1.0)),
        }
    }

    fn with_envelope(envelope: EnvelopeValues) -> PitchEnvelopeGroups {
        PitchEnvelopeGroups {
            envelope: Some(envelope),
            ..PitchEnvelopeGroups::default()
        }
    }

    #[test]
    fn a_mono_bass_with_one_glide_sets_mono_and_the_glide_time() {
        for adapter in adapters() {
            let keys = &adapter.keys;
            let mapped = adapter.run(&mono_line(vec![glide(0.12, false, Level::High)], vec![]));
            assert_eq!(adapter.stored(&mapped, keys.mono), Some(1.0));
            let time = adapter.stored(&mapped, keys.glide_time).unwrap();
            let spec = adapter.registry.get(keys.glide_time).unwrap();
            assert!((spec.physical(time).unwrap().get() - 0.12).abs() < 1e-9);
            assert!(!written(&mapped, keys.voices));
            assert!(!written(&mapped, keys.glide_curve));
            assert!(skipped(
                &mapped,
                "glides.curve",
                UnwrittenReason::CurveNotSet
            ));
            assert!(
                mapped.unrepresentable.is_empty(),
                "{:?}",
                mapped.unrepresentable
            );
        }
    }

    #[test]
    fn a_glide_beyond_the_maximum_clamps_and_is_listed() {
        for adapter in adapters() {
            let keys = &adapter.keys;
            let spec = adapter.registry.get(keys.glide_time).unwrap();
            let top = spec.physical(spec.stored_max).unwrap().get();
            let mapped = adapter.run(&mono_line(
                vec![glide(top + 10.0, false, Level::High)],
                vec![],
            ));
            assert_eq!(
                adapter.stored(&mapped, keys.glide_time),
                Some(spec.stored_max)
            );
            let miss = &mapped.unrepresentable[0];
            assert_eq!(miss.field, "glides.time_s");
            assert_eq!(miss.reason, UnrepresentableReason::AboveRange);
            assert!((miss.residual().unwrap() + 10.0).abs() < 1e-9);
        }
    }

    #[test]
    fn a_glide_from_silence_sets_always_glide() {
        for adapter in adapters() {
            let keys = &adapter.keys;
            let mapped = adapter.run(&mono_line(
                vec![
                    glide(0.1, false, Level::High),
                    glide(0.2, true, Level::Medium),
                ],
                vec![],
            ));
            assert_eq!(adapter.stored(&mapped, keys.glide_always), Some(1.0));
            let without = adapter.run(&mono_line(vec![glide(0.1, false, Level::High)], vec![]));
            assert!(!written(&without, keys.glide_always));
        }
    }

    #[test]
    fn the_glide_time_is_the_median_of_the_confident_glides() {
        let groups = mono_line(
            vec![
                glide(0.1, false, Level::High),
                glide(0.3, false, Level::Medium),
                glide(0.2, false, Level::High),
                glide(5.0, true, Level::Low),
            ],
            vec![],
        );
        let wants = plan(&groups).wants;
        assert!(wants.contains(&Wanted {
            want: Want::GlideTime(s(0.2)),
            confidence: Level::Medium,
        }));
        // The low glide from silence counts for nothing.
        assert!(!wants.iter().any(|w| w.want == Want::GlideFromSilence));
        let low = plan(&mono_line(vec![glide(0.1, false, Level::Low)], vec![]));
        assert!(
            !low.wants
                .iter()
                .any(|w| matches!(w.want, Want::GlideTime(_)))
        );
        assert!(low.unwritten.contains(&Unwritten::new(
            "glides.time_s",
            UnwrittenReason::LowConfidence
        )));
    }

    #[test]
    fn a_poly_report_counts_its_voices() {
        for adapter in adapters() {
            let groups = PitchEnvelopeGroups {
                voices: Some(VoicesValues {
                    level: Level::High,
                    mono: false,
                    max_polyphony: 3,
                    joins: Vec::new(),
                }),
                ..PitchEnvelopeGroups::default()
            };
            let mapped = adapter.run(&groups);
            assert_eq!(adapter.stored(&mapped, adapter.keys.voices), Some(3.0));
            assert!(
                plan(&groups)
                    .wants
                    .iter()
                    .any(|w| w.want == Want::Legato(false))
            );
        }
    }

    #[test]
    fn legato_follows_the_confident_joins() {
        let joins = |verdicts: &[(Option<bool>, Level)]| {
            plan(&PitchEnvelopeGroups {
                voices: Some(VoicesValues {
                    level: Level::High,
                    mono: true,
                    max_polyphony: 1,
                    joins: verdicts
                        .iter()
                        .map(|&(legato, level)| JoinValue { legato, level })
                        .collect(),
                }),
                ..PitchEnvelopeGroups::default()
            })
        };
        let legato = |plan: &Plan| {
            plan.wants.iter().find_map(|w| match w.want {
                Want::Legato(on) => Some(on),
                _ => None,
            })
        };
        let mixed = joins(&[(Some(false), Level::High), (Some(true), Level::Medium)]);
        assert_eq!(legato(&mixed), Some(true));
        let retrigger = joins(&[(Some(false), Level::High), (Some(true), Level::Low)]);
        assert_eq!(legato(&retrigger), Some(false));
        let unknown = joins(&[(None, Level::High), (Some(true), Level::Low)]);
        assert_eq!(legato(&unknown), None);
        assert!(unknown.unwritten.contains(&Unwritten::new(
            "joins.legato",
            UnwrittenReason::LowConfidence
        )));
        assert_eq!(legato(&joins(&[])), None);
    }

    #[test]
    fn a_pitch_drop_routes_an_envelope_to_pitch() {
        for adapter in adapters() {
            let keys = &adapter.keys;
            let drop = |df: f64| DropValue {
                df: Semitones::clamped(df),
                tau: s(0.03),
                level: Level::High,
            };
            let mapped = adapter.run(&mono_line(vec![], vec![drop(7.0)]));
            let route = mapped
                .edits
                .iter()
                .find(|edit| matches!(&edit.edit.record, Record::Route(r) if r.dest == keys.drop_dest))
                .unwrap();
            assert!(route.uncalibrated);
            let Record::Route(record) = &route.edit.record else {
                unreachable!()
            };
            assert_eq!(record.polarity, Some(PolarityText::Unipolar));
            let dest = adapter.registry.get(keys.drop_dest).unwrap();
            let span = dest.stored_max - dest.stored_min;
            let amount = adapter
                .resolved(&mapped)
                .into_iter()
                .find_map(|item| match item {
                    Resolved::Route { amount, .. } => Some(amount),
                    _ => None,
                })
                .unwrap();
            assert!((amount - 7.0 / span).abs() < 1e-9);
            let decay = mapped
                .edits
                .iter()
                .find(|edit| edit.edit.record.name() == keys.drop_decay)
                .unwrap();
            assert!(decay.uncalibrated);
            assert!(
                adapter
                    .registry
                    .get(keys.drop_decay)
                    .unwrap()
                    .tuning_note
                    .is_some()
            );
            assert_eq!(mapped.unrepresentable, Vec::new());

            let deep = adapter.run(&mono_line(vec![], vec![drop(span + 4.0)]));
            let miss = &deep.unrepresentable[0];
            assert_eq!(miss.field, "drops.df_st");
            assert_eq!(miss.reason, UnrepresentableReason::AboveRange);
            assert!((miss.residual().unwrap() + 4.0).abs() < 1e-9);
        }
    }

    #[test]
    fn velocity_reaches_the_level_above_3_db_only() {
        assert!(!velocity_reaches_level(None));
        assert!(!velocity_reaches_level(Some(Decibels::clamped(2.0))));
        assert!(!velocity_reaches_level(Some(Decibels::clamped(3.0))));
        assert!(velocity_reaches_level(Some(Decibels::clamped(3.01))));
        for adapter in adapters() {
            let spread = |db: f64| {
                adapter.run(&with_envelope(EnvelopeValues {
                    level_spread: Some(Decibels::clamped(db)),
                    ..envelope()
                }))
            };
            let wide = spread(4.0);
            let edit = wide
                .edits
                .iter()
                .find(|edit| edit.edit.record.name() == adapter.keys.velocity)
                .unwrap();
            assert_eq!(edit.confidence, Level::Low);
            assert!(!written(&spread(2.0), adapter.keys.velocity));
        }
    }

    #[test]
    fn a_value_equal_to_its_default_gives_no_edit() {
        for adapter in adapters() {
            let keys = &adapter.keys;
            let default_of = |key: &str| {
                let spec = adapter.registry.get(key).unwrap();
                spec.physical(spec.default).unwrap().get()
            };
            let mapped = adapter.run(&with_envelope(EnvelopeValues {
                attack: high(s(default_of(keys.attack))),
                sustain: high(Decibels::clamped(0.0)),
                ..envelope()
            }));
            assert!(!written(&mapped, keys.attack));
            assert!(!written(&mapped, keys.sustain));
            assert!(written(&mapped, keys.release));
        }
    }

    #[test]
    fn low_and_flagged_fields_give_no_edit() {
        for adapter in adapters() {
            let keys = &adapter.keys;
            let low = Some(Reading {
                value: s(0.05),
                level: Level::Low,
            });
            let mapped = adapter.run(&with_envelope(EnvelopeValues {
                attack: low,
                decay: None,
                ..envelope()
            }));
            assert!(!written(&mapped, keys.attack));
            assert!(skipped(&mapped, "attack_s", UnwrittenReason::LowConfidence));
            assert!(skipped(&mapped, "decay_s", UnwrittenReason::LowConfidence));

            let bound = adapter.run(&with_envelope(EnvelopeValues {
                attack_is_upper_bound: true,
                ..envelope()
            }));
            assert!(!written(&bound, keys.attack));
            assert!(skipped(&bound, "attack_s", UnwrittenReason::UpperBound));

            // A tail heavier than 0.5 leaves no release and a low release
            // confidence.
            let tail = adapter.run(&with_envelope(EnvelopeValues {
                release: None,
                release_confidence: Some(Level::Low),
                ..envelope()
            }));
            assert!(!written(&tail, keys.release));
            assert!(skipped(
                &tail,
                "release_s",
                UnwrittenReason::NotIdentifiable
            ));
            let unsure = adapter.run(&with_envelope(EnvelopeValues {
                release_confidence: Some(Level::Low),
                ..envelope()
            }));
            assert!(!written(&unsure, keys.release));
        }
    }

    #[test]
    fn a_hold_needs_a_plateau() {
        let hold = |value: f64| {
            plan(&with_envelope(EnvelopeValues {
                hold: high(s(value)),
                ..envelope()
            }))
            .wants
            .iter()
            .any(|w| matches!(w.want, Want::Hold(_)))
        };
        assert!(!hold(0.0));
        assert!(!hold(0.009));
        assert!(hold(0.01));
        assert!(hold(0.05));
    }

    #[test]
    fn every_edit_has_a_unit_a_range_and_a_reason() {
        for adapter in adapters() {
            let groups = PitchEnvelopeGroups {
                envelope: Some(EnvelopeValues {
                    hold: high(s(0.02)),
                    level_spread: Some(Decibels::clamped(6.0)),
                    ..envelope()
                }),
                ..mono_line(
                    vec![glide(0.12, true, Level::High)],
                    vec![DropValue {
                        df: Semitones::clamped(12.0),
                        tau: s(0.02),
                        level: Level::Medium,
                    }],
                )
            };
            let mapped = adapter.run(&groups);
            assert!(mapped.edits.len() >= 10, "{:?}", mapped.edits);
            let names: Vec<&str> = mapped.edits.iter().map(|e| e.edit.record.name()).collect();
            let mut sorted = names.clone();
            sorted.sort_unstable();
            assert_eq!(names, sorted);
            for edit in &mapped.edits {
                let reason = edit.edit.reason.as_deref().unwrap();
                assert!(["pitch", "voices", "envelope"].contains(&reason));
                let spec = adapter.registry.get(edit.edit.record.name()).unwrap();
                if let Record::Param { value, .. } = &edit.edit.record
                    && spec.unit().is_some()
                {
                    assert!(matches!(value, ParamValue::Text(_)), "{value:?}");
                }
                if spec.provenance == Provenance::Interim {
                    assert!(edit.uncalibrated, "{}", spec.key);
                    assert!(spec.tuning_note.is_some());
                }
            }
            for item in adapter.resolved(&mapped) {
                if let Resolved::Param { key, stored, .. } = item {
                    assert!(adapter.registry.get(&key).unwrap().accepts(stored));
                }
            }
        }
    }

    #[test]
    fn empty_groups_map_to_nothing() {
        for adapter in adapters() {
            assert_eq!(
                adapter.run(&PitchEnvelopeGroups::default()),
                Mapped::default()
            );
        }
    }
}
