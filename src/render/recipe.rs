//! The render recipe: the one fixed performance that every render plays.
//!
//! The recipe carries an integer version. Any change to a field raises it,
//! and every render record carries it, so two renders compare only when
//! their versions match. A snapshot of the recipe at each version guards
//! the rule: a field that changes without a new version fails that test.

use serde::{Deserialize, Serialize};

use super::RenderNote;
use crate::audio::SampleRate;
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

/// The render recipe. [`Recipe::current`] is the only constructor, so a
/// recipe always matches its version.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Recipe {
    version: u32,
    sample_rate: u32,
    channels: u32,
    block_size: u32,
    process_mode: &'static str,
    velocity: u8,
    bass: &'static [u8],
    lead: &'static [u8],
    pad: &'static [u8],
    hold: Seconds,
    clip: Seconds,
    tail_floor: Decibels,
    tail_hold: Seconds,
    tail_cap: Seconds,
    settle: Seconds,
    warm_up: Seconds,
    vital_oversampling: Oversampling,
}

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
    /// - Before the clip: 0.5 s of silence after the state load, a warm-up
    ///   note of 1 s that is thrown away, and 0.5 s of silence again.
    /// - Vital renders at 2x oversampling, its default, whatever the preset
    ///   stores.
    #[must_use]
    pub fn current() -> Self {
        Self {
            version: RECIPE_VERSION,
            sample_rate: 44_100,
            channels: 2,
            block_size: 512,
            process_mode: "offline",
            velocity: 100,
            bass: &[36],
            lead: &[60],
            pad: &[60, 64, 67],
            hold: Seconds::from_const(3.0),
            clip: Seconds::from_const(4.0),
            tail_floor: Decibels::from_const(-96.0),
            tail_hold: Seconds::from_const(1.0),
            tail_cap: Seconds::from_const(30.0),
            settle: Seconds::from_const(0.5),
            warm_up: Seconds::from_const(1.0),
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

    /// Returns the silence rendered after the state load and after the
    /// warm-up note.
    #[must_use]
    pub fn settle(&self) -> Seconds {
        self.settle
    }

    /// Returns the length of the warm-up note, which is thrown away.
    #[must_use]
    pub fn warm_up(&self) -> Seconds {
        self.warm_up
    }

    /// Returns the oversampling of a Vital render.
    #[must_use]
    pub fn vital_oversampling(&self) -> Oversampling {
        self.vital_oversampling
    }

    /// Starts a watch for the end of a clip whose last note ends at
    /// `last_note_off`.
    #[must_use]
    pub fn tail_watch(&self, last_note_off: Seconds) -> TailWatch {
        let rate = f64::from(self.sample_rate);
        let frames =
            |time: Seconds| crate::cast::ceil_to_usize(time.get() * rate).unwrap_or(usize::MAX);
        let last_off = frames(last_note_off);
        TailWatch {
            floor: self.tail_floor.to_amplitude_gain().unwrap_or(0.0),
            min_frames: frames(self.clip),
            last_off,
            hold: frames(self.tail_hold),
            cap: last_off.saturating_add(frames(self.tail_cap)),
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
/// the last note-off. Silence before the last note-off never ends it. If
/// the output is still above the floor at the cap after the last note-off,
/// the clip ends there and is marked capped.
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
    /// Takes the peak absolute sample of the next frame over all channels.
    /// Returns the end once the clip is complete; the frame just pushed is
    /// its last one.
    pub fn push(&mut self, peak: f64) -> Option<ClipEnd> {
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
        if self.frame >= self.min_frames && self.quiet >= self.hold {
            end(false)
        } else if self.frame >= self.cap.max(self.min_frames) {
            end(true)
        } else {
            None
        }
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
        for value in [
            recipe.hold,
            recipe.clip,
            recipe.tail_hold,
            recipe.tail_cap,
            recipe.settle,
            recipe.warm_up,
        ] {
            assert_eq!(Seconds::new(value.get()).ok(), Some(value));
        }
        assert!(Decibels::new(recipe.tail_floor.get()).is_ok());
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

    /// Pushes `loud` frames at full scale, then silence, and returns the end.
    fn run(watch: &mut TailWatch, loud: usize) -> ClipEnd {
        let mut frame = 0;
        loop {
            let peak = if frame < loud { 1.0 } else { 0.0 };
            frame += 1;
            if let Some(end) = watch.push(peak) {
                return end;
            }
        }
    }

    #[test]
    fn a_short_tail_gives_the_minimum_clip() {
        let recipe = Recipe::current();
        let end = run(
            &mut recipe.tail_watch(Seconds::new(3.0).unwrap()),
            3 * 44_100,
        );
        assert_eq!(
            end,
            ClipEnd {
                frames: 4 * 44_100,
                capped: false
            }
        );
    }

    #[test]
    fn a_long_tail_grows_the_clip_past_its_quiet_hold() {
        let recipe = Recipe::current();
        // Sound until 6 s, then the 1 s hold: the clip ends at 7 s.
        let end = run(
            &mut recipe.tail_watch(Seconds::new(3.0).unwrap()),
            6 * 44_100,
        );
        assert_eq!(
            end,
            ClipEnd {
                frames: 7 * 44_100,
                capped: false
            }
        );
    }

    #[test]
    fn silence_before_the_last_note_off_does_not_end_the_clip() {
        let recipe = Recipe::current();
        let mut watch = recipe.tail_watch(Seconds::new(10.0).unwrap());
        let end = run(&mut watch, 0);
        assert_eq!(
            end,
            ClipEnd {
                frames: 11 * 44_100,
                capped: false
            }
        );
    }

    #[test]
    fn a_tail_that_never_ends_is_capped_after_the_last_note_off() {
        let recipe = Recipe::current();
        let end = run(
            &mut recipe.tail_watch(Seconds::new(3.0).unwrap()),
            usize::MAX,
        );
        assert_eq!(
            end,
            ClipEnd {
                frames: 33 * 44_100,
                capped: true
            }
        );
    }
}
