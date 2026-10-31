//! The one integration-test binary. Each module covers one feature.

// `cfg(test)` lets the helpers of each module unwrap, as test code may.
#[cfg(test)]
mod analytic;
mod apply;
#[cfg(test)]
mod cleanup;
mod cli;
#[cfg(test)]
mod decay;
#[cfg(test)]
mod decode;
#[cfg(test)]
mod descriptors;
#[cfg(test)]
mod envelope;
mod fixtures;
#[cfg(test)]
mod golden;
#[cfg(test)]
mod load_check;
#[cfg(test)]
mod lomb;
mod mapper;
#[cfg(test)]
mod masks;
#[cfg(test)]
mod midi;
#[cfg(test)]
mod midi_write;
#[cfg(test)]
mod modulation;
#[cfg(test)]
mod music;
#[cfg(test)]
mod pitch;
mod preset;
#[cfg(test)]
mod profile;
#[cfg(test)]
mod reference;
#[cfg(test)]
mod render;
#[cfg(test)]
mod render_file;
#[cfg(test)]
mod render_session;
#[cfg(test)]
mod render_spread;
#[cfg(test)]
mod render_timing;
#[cfg(test)]
mod render_vital;
#[cfg(test)]
mod report;
#[cfg(test)]
mod stem;
#[cfg(test)]
mod stereo;
#[cfg(test)]
mod stft;
#[cfg(test)]
mod support;
mod synth;
#[cfg(test)]
mod tempo;
#[cfg(test)]
mod track;
#[cfg(test)]
mod transcribe;
#[cfg(test)]
mod vita;
#[cfg(test)]
mod words;
