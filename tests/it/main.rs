//! The one integration-test binary. Each module covers one feature.

// `cfg(test)` lets the helpers of each module unwrap, as test code may.
#[cfg(test)]
mod analytic;
mod apply;
mod cli;
#[cfg(test)]
mod decay;
#[cfg(test)]
mod decode;
#[cfg(test)]
mod descriptors;
mod fixtures;
#[cfg(test)]
mod golden;
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
mod preset;
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
mod transcribe;
mod vita;
