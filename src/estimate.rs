//! The layer estimators that read the note list and the f0 track.
//!
//! Each layer turns the stem, its notes and its track into synth-neutral
//! records with a confidence each. The report copies the records into
//! `layers.json`, and a synth mapper turns them into edits.
//!
//! - [`pitch`]: glides, pitch drops and vibrato of a monophonic line.
//! - [`envelope`]: the amplitude envelope of the notes and the sidechain
//!   pump.

pub mod envelope;
pub mod pitch;
