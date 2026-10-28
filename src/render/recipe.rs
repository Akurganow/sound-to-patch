//! The render recipe: the one fixed performance that every render plays.
//!
//! The recipe carries an integer version. Any change to a field raises it,
//! and every render record carries it, so two renders compare only when
//! their versions match. A snapshot of the recipe at each version, and a
//! hash of that snapshot, guard the rule: a field that changes without a
//! new version fails those tests.

use serde::{Deserialize, Serialize};

use super::{EventKind, RenderNote, frame_of, note_events};
use crate::audio::{AudioError, SampleRate};
use crate::notes::Velocity;
use crate::units::{Decibels, Seconds};

/// The version of [`Recipe::current`].
pub const RECIPE_VERSION: u32 = 1;

/// What kind of sound a recipe clip plays. A pluck plays as a lead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Material {
    /// A low single note.
    Bass,
    /// A single note in the middle of the keyboard.
    Lead,
    /// A chord.
    Pad,
}

/// The oversampling of Vital, its only quality setting. The preset stores
/// it as the index of the factor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Oversampling {
    /// No oversampling.
    #[serde(rename = "1x")]
    X1,
    /// Twice the rate, Vital's default.
    #[serde(rename = "2x")]
    X2,
    /// Four times the rate.
    #[serde(rename = "4x")]
    X4,
    /// Eight times the rate.
    #[serde(rename = "8x")]
    X8,
}

impl Oversampling {
    /// Returns the value Vital stores in the `oversampling` control.
    #[must_use]
    pub fn stored(self) -> f64 {
        match self {
            Self::X1 => 0.0,
            Self::X2 => 1.0,
            Self::X4 => 2.0,
            Self::X8 => 3.0,
        }
    }
}

/// How the host tells the plug-in it processes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProcessMode {
    /// Faster or slower than real time, as the host renders a file.
    Offline,
}

/// One step of the pre-roll, which plays after the state load and before
/// the clip and is thrown away.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "step", rename_all = "snake_case")]
pub enum PreRoll {
    /// Silence of the given length, so smoothed parameters settle.
    Settle {
        /// The length of the silence.
        length: Seconds,
    },
    /// A note that clears state left from earlier renders, such as LFO
    /// phase and envelope residue.
    WarmUp {
        /// The MIDI key.
        key: u8,
        /// The velocity.
        velocity: u8,
        /// The note-on, from the start of the step.
        start: Seconds,
        /// The note-off, from the start of the step.
        end: Seconds,
        /// The length of the step.
        length: Seconds,
    },
}

/// The render recipe. [`Recipe::current`] is the only constructor, so a
/// recipe always matches its version.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Recipe {
    version: u32,
    sample_rate: u32,
    channels: u32,
    block_size: u32,
    process_mode: ProcessMode,
    velocity: u8,
    bass: &'static [u8],
    lead: &'static [u8],
    pad: &'static [u8],
    hold: Seconds,
    clip: Seconds,
    tail_floor: Decibels,
    tail_hold: Seconds,
    tail_cap: Seconds,
    pre_roll: &'static [PreRoll],
    vital_oversampling: Oversampling,
}

/// The pre-roll of [`RECIPE_VERSION`]: settle, warm-up note, settle.
const PRE_ROLL: [PreRoll; 3] = [
    PreRoll::Settle {
        length: Seconds::from_const(0.5),
    },
    PreRoll::WarmUp {
        key: 60,
        velocity: 100,
        start: Seconds::from_const(0.0),
        end: Seconds::from_const(0.5),
        length: Seconds::from_const(1.0),
    },
    PreRoll::Settle {
        length: Seconds::from_const(0.5),
    },
];

impl Recipe {
    /// Returns the recipe of [`RECIPE_VERSION`].
    ///
    /// - 44.1 kHz stereo, blocks of 512 samples, offline processing.
    /// - Velocity 100. Bass plays MIDI 36, a lead or pluck MIDI 60 and a
    ///   pad the triad 60, 64 and 67. Notes are MIDI numbers, because
    ///   octave names differ per synth.
    /// - The notes start at 0 s and are held 3 s. The clip is at least 4 s
    ///   and grows until the tail is over (see [`TailWatch`]): the output
    ///   stays below -96 dBFS for 1 s after the last note-off, or 30 s pass.
    /// - The pre-roll after the state load, thrown away: 0.5 s of silence,
    ///   a warm-up step of 1 s with MIDI 60 at velocity 100 held for its
    ///   first 0.5 s, and 0.5 s of silence again.
    /// - Vital renders at 2x oversampling, its default, whatever the preset
    ///   stores.
    #[must_use]
    pub fn current() -> Self {
        Self {
            version: RECIPE_VERSION,
            sample_rate: 44_100,
            channels: 2,
            block_size: 512,
            process_mode: ProcessMode::Offline,
            velocity: 100,
            bass: &[36],
            lead: &[60],
            pad: &[60, 64, 67],
            hold: Seconds::from_const(3.0),
            clip: Seconds::from_const(4.0),
            tail_floor: Decibels::from_const(-96.0),
            tail_hold: Seconds::from_const(1.0),
            tail_cap: Seconds::from_const(30.0),
            pre_roll: &PRE_ROLL,
            vital_oversampling: Oversampling::X2,
        }
    }

    /// Returns the version.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// Returns the sample rate.
    #[must_use]
    pub fn sample_rate(&self) -> SampleRate {
        SampleRate::from_const(self.sample_rate)
    }

    /// Returns the number of output channels.
    #[must_use]
    pub fn channels(&self) -> u32 {
        self.channels
    }

    /// Returns the block size in samples.
    #[must_use]
    pub fn block_size(&self) -> u32 {
        self.block_size
    }

    /// Returns the process mode.
    #[must_use]
    pub fn process_mode(&self) -> ProcessMode {
        self.process_mode
    }

    /// Returns the velocity of the recipe notes.
    #[must_use]
    pub fn velocity(&self) -> Velocity {
        Velocity::from_const(self.velocity)
    }

    /// Returns the notes for `material`, each held from 0 s to the hold time.
    #[must_use]
    pub fn notes(&self, material: Material) -> Vec<RenderNote> {
        let keys = match material {
            Material::Bass => self.bass,
            Material::Lead => self.lead,
            Material::Pad => self.pad,
        };
        keys.iter()
            .map(|&key| RenderNote {
                key,
                onset: Seconds::from_const(0.0),
                offset: self.hold,
                velocity: self.velocity(),
            })
            .collect()
    }

    /// Returns the shortest clip.
    #[must_use]
    pub fn clip(&self) -> Seconds {
        self.clip
    }

    /// Returns the pre-roll steps in the order they play.
    #[must_use]
    pub fn pre_roll(&self) -> &[PreRoll] {
        self.pre_roll
    }

    /// Returns the silence after the state load: the first settle step.
    #[must_use]
    pub fn settle(&self) -> Seconds {
        self.pre_roll
            .iter()
            .find_map(|step| match step {
                PreRoll::Settle { length } => Some(*length),
                PreRoll::WarmUp { .. } => None,
            })
            .unwrap_or(Seconds::from_const(0.0))
    }

    /// Returns the oversampling of a Vital render.
    #[must_use]
    pub fn vital_oversampling(&self) -> Oversampling {
        self.vital_oversampling
    }

    /// Starts a watch for the end of a clip that plays `notes`. The last
    /// note-off is the latest note-off frame of [`super::note_events`], so a
    /// note shorter than a frame ends where its event does.
    #[must_use]
    pub fn tail_watch(&self, notes: &[RenderNote]) -> TailWatch {
        let rate = self.sample_rate();
        let last_off = note_events(notes, rate)
            .iter()
            .filter(|event| event.kind == EventKind::Off)
            .map(|event| event.frame)
            .max()
            .unwrap_or(0);
        TailWatch {
            // The floor is a finite dB value, so the gain is a finite
            // number above zero.
            floor: 10_f64.powf(self.tail_floor.get() / 20.0),
            min_frames: frame_of(self.clip, rate),
            last_off,
            hold: frame_of(self.tail_hold, rate),
            cap: last_off.saturating_add(frame_of(self.tail_cap, rate)),
            frame: 0,
            quiet: 0,
        }
    }
}

/// Where a clip ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClipEnd {
    /// The clip length in frames.
    pub frames: usize,
    /// Whether the tail reached its cap before it went quiet.
    pub capped: bool,
}

/// Finds the end of a clip while it renders, one frame at a time.
///
/// The clip ends at the first frame where it is at least the recipe's clip
/// long and the output has stayed below the floor for the hold time after
/// the last note-off. Silence before the last note-off never ends it, and
/// sound after a quiet stretch starts the hold again. If the output is
/// still above the floor at the cap after the last note-off, the clip ends
/// there and is marked capped.
#[derive(Clone, Debug, PartialEq)]
pub struct TailWatch {
    floor: f64,
    min_frames: usize,
    last_off: usize,
    hold: usize,
    cap: usize,
    frame: usize,
    quiet: usize,
}

impl TailWatch {
    /// Takes the samples of the next frame, one per channel. Returns the end
    /// once the clip is complete; the frame just pushed is its last one.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::NoChannels`] for a frame with no sample and
    /// [`AudioError::NonFinite`] for a NaN or infinite sample.
    pub fn push(&mut self, samples: &[f64]) -> Result<Option<ClipEnd>, AudioError> {
        if samples.is_empty() {
            return Err(AudioError::NoChannels);
        }
        if let Some(channel) = samples.iter().position(|sample| !sample.is_finite()) {
            return Err(AudioError::NonFinite {
                channel,
                frame: self.frame,
            });
        }
        let peak = samples.iter().fold(0.0_f64, |a, s| a.max(s.abs()));
        self.frame += 1;
        if self.frame > self.last_off && peak < self.floor {
            self.quiet += 1;
        } else {
            self.quiet = 0;
        }
        let end = |capped| {
            Some(ClipEnd {
                frames: self.frame,
                capped,
            })
        };
        Ok(
            if self.frame >= self.min_frames && self.quiet >= self.hold {
                end(false)
            } else if self.frame >= self.cap.max(self.min_frames) {
                end(true)
            } else {
                None
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_constants_pass_their_checks() {
        let recipe = Recipe::current();
        assert!(SampleRate::new(recipe.sample_rate).is_ok());
        assert!(Velocity::new(recipe.velocity).is_ok());
        let mut times = vec![recipe.hold, recipe.clip, recipe.tail_hold, recipe.tail_cap];
        for step in recipe.pre_roll() {
            match *step {
                PreRoll::Settle { length } => times.push(length),
                PreRoll::WarmUp {
                    velocity,
                    start,
                    end,
                    length,
                    ..
                } => {
                    assert!(Velocity::new(velocity).is_ok());
                    assert!(start < end && end <= length);
                    times.extend([start, end, length]);
                }
            }
        }
        for value in times {
            assert_eq!(Seconds::new(value.get()).ok(), Some(value));
        }
        assert!(Decibels::new(recipe.tail_floor.get()).is_ok());
    }

    #[test]
    fn the_pre_roll_settles_warms_up_and_settles() {
        let recipe = Recipe::current();
        let kinds: Vec<&str> = recipe
            .pre_roll()
            .iter()
            .map(|step| match step {
                PreRoll::Settle { .. } => "settle",
                PreRoll::WarmUp { .. } => "warm-up",
            })
            .collect();
        assert_eq!(kinds, ["settle", "warm-up", "settle"]);
        assert_eq!(recipe.settle().get(), 0.5);
    }

    #[test]
    fn oversampling_stores_the_index_of_the_factor() {
        let stored: Vec<f64> = [
            Oversampling::X1,
            Oversampling::X2,
            Oversampling::X4,
            Oversampling::X8,
        ]
        .map(Oversampling::stored)
        .to_vec();
        assert_eq!(stored, [0.0, 1.0, 2.0, 3.0]);
        assert_eq!(Recipe::current().vital_oversampling(), Oversampling::X2);
    }

    #[test]
    fn each_material_plays_its_keys_for_the_hold_time() {
        let recipe = Recipe::current();
        let keys = |m| recipe.notes(m).iter().map(|n| n.key()).collect::<Vec<_>>();
        assert_eq!(keys(Material::Bass), [36]);
        assert_eq!(keys(Material::Lead), [60]);
        assert_eq!(keys(Material::Pad), [60, 64, 67]);
        for note in recipe.notes(Material::Pad) {
            assert_eq!((note.onset().get(), note.offset().get()), (0.0, 3.0));
            assert_eq!(note.velocity().get(), 100);
        }
    }

    const RATE: usize = 44_100;

    /// Pushes one mono frame per entry of `level` until the clip ends.
    fn run(watch: &mut TailWatch, level: impl Fn(usize) -> f64) -> ClipEnd {
        let mut frame = 0;
        loop {
            let sample = level(frame);
            frame += 1;
            if let Some(end) = watch.push(&[sample, -sample]).unwrap() {
                return end;
            }
        }
    }

    /// Starts a watch for one note from 0 s to `last_note_off`.
    fn watch(last_note_off: f64) -> TailWatch {
        let note = RenderNote::new(
            60,
            Seconds::new(0.0).unwrap(),
            Seconds::new(last_note_off).unwrap(),
            Velocity::new(100).unwrap(),
        )
        .unwrap();
        Recipe::current().tail_watch(&[note])
    }

    #[test]
    fn a_short_tail_gives_the_minimum_clip() {
        let end = run(&mut watch(3.0), |f| if f < 3 * RATE { 1.0 } else { 0.0 });
        assert_eq!(
            end,
            ClipEnd {
                frames: 4 * RATE,
                capped: false
            }
        );
    }

    #[test]
    fn a_long_tail_grows_the_clip_past_its_quiet_hold() {
        // Sound until 6 s, then the 1 s hold: the clip ends at 7 s.
        let end = run(&mut watch(3.0), |f| if f < 6 * RATE { 1.0 } else { 0.0 });
        assert_eq!(
            end,
            ClipEnd {
                frames: 7 * RATE,
                capped: false
            }
        );
    }

    #[test]
    fn sound_after_a_quiet_stretch_starts_the_hold_again() {
        // Quiet from 3 s to 3.5 s, sound again until 5 s: the clip ends at 6 s.
        let end = run(&mut watch(3.0), |f| {
            if f < 3 * RATE || (7 * RATE / 2..5 * RATE).contains(&f) {
                1.0
            } else {
                0.0
            }
        });
        assert_eq!(
            end,
            ClipEnd {
                frames: 6 * RATE,
                capped: false
            }
        );
    }

    #[test]
    fn silence_before_the_last_note_off_does_not_end_the_clip() {
        let end = run(&mut watch(10.0), |_| 0.0);
        assert_eq!(
            end,
            ClipEnd {
                frames: 11 * RATE,
                capped: false
            }
        );
    }

    #[test]
    fn the_floor_lies_between_1_5e_5_and_1_6e_5() {
        // -96 dBFS is an amplitude of 1.585e-5.
        let quiet = run(&mut watch(3.0), |_| 1.5e-5);
        assert_eq!(
            quiet,
            ClipEnd {
                frames: 4 * RATE,
                capped: false
            }
        );
        let loud = run(&mut watch(3.0), |_| 1.6e-5);
        assert_eq!(
            loud,
            ClipEnd {
                frames: 33 * RATE,
                capped: true
            }
        );
    }

    #[test]
    fn a_non_finite_sample_is_rejected() {
        let mut watch = watch(3.0);
        assert!(watch.push(&[0.0, 0.0]).unwrap().is_none());
        assert!(matches!(
            watch.push(&[0.0, f64::NAN]),
            Err(AudioError::NonFinite {
                channel: 1,
                frame: 1
            })
        ));
    }

    #[test]
    fn a_frame_with_no_sample_is_rejected() {
        assert!(matches!(watch(3.0).push(&[]), Err(AudioError::NoChannels)));
    }

    #[test]
    fn the_watch_takes_the_last_note_off_from_the_events() {
        // 3.00001 s is 132 300.441 frames: round gives 132 300.
        assert_eq!(watch(3.000_01).last_off, 3 * RATE);
        // A note shorter than half a frame ends one frame after it starts.
        assert_eq!(watch(1e-6).last_off, 1);
        assert_eq!(Recipe::current().tail_watch(&[]).last_off, 0);
    }
}
