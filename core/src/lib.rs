//! The render core of sound-to-patch: the part of the library that the
//! plug-in host process shares with it.
//!
//! The host links this crate and not the library, so it carries none of
//! the transcriber or the analysis. The library re-exports every module
//! here at the same path, such as `sound_to_patch::units`.

pub mod audio;
pub mod cast;
pub mod notes;
pub mod render;
pub mod synth;
pub mod units;
