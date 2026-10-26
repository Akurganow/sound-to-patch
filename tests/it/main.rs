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
mod fixtures;
#[cfg(test)]
mod golden;
mod mapper;
#[cfg(test)]
mod masks;
mod preset;
#[cfg(test)]
mod stft;
#[cfg(test)]
mod support;
mod synth;
