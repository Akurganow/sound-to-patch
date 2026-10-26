//! Stem intake (#45): decoding generated files and resampling.

use std::fs;
use std::path::Path;

use sound_to_patch::audio::{self, SampleRate};
use sound_to_patch::dsp::{Stft, StftParams, strongest_peak};

use crate::support::cast::pcm16;
use crate::support::signals::sine;

fn write_wav16(path: &Path, rate: u32, channels: &[Vec<f64>]) {
    let spec = hound::WavSpec {
        channels: u16::try_from(channels.len()).unwrap(),
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for frame in 0..channels[0].len() {
        for channel in channels {
            writer.write_sample(pcm16(channel[frame])).unwrap();
        }
    }
    writer.finalize().unwrap();
}

#[test]
fn a_stereo_wav_decodes_at_its_rate_with_both_channels() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stereo.wav");
    let left = sine(440.0, 0.0, 48_000.0, 48_000);
    let right: Vec<f64> = sine(660.0, 1.0, 48_000.0, 48_000)
        .iter()
        .map(|v| 0.5 * v)
        .collect();
    write_wav16(&path, 48_000, &[left.clone(), right.clone()]);
    let decoded = audio::decode(&path).unwrap();
    assert_eq!(decoded.sample_rate().get(), 48_000);
    assert_eq!(decoded.channels().len(), 2);
    assert_eq!(decoded.frames(), 48_000);
    // One 16-bit step is 2^-15, 3.1e-5.
    for (channel, original) in decoded.channels().iter().zip([&left, &right]) {
        for (a, b) in channel.iter().zip(original) {
            assert!((a - b).abs() <= 3.1e-5, "{a} vs {b}");
        }
    }
}

#[test]
fn a_resampled_sine_keeps_its_frequency() {
    let x = sine(1000.0, 0.0, 48_000.0, 96_000);
    let from = SampleRate::new(48_000).unwrap();
    let to = SampleRate::new(22_050).unwrap();
    let y = audio::resample(&x, from, to).unwrap();
    assert_eq!(y.len(), 44_100);
    let params = StftParams::new(8192).unwrap().with_oversample(4).unwrap();
    let mut stft = Stft::new(params);
    let magnitudes: Vec<f64> = stft.frame(&y, 20).iter().map(|c| c.norm()).collect();
    let peak = strongest_peak(&magnitudes, 1, magnitudes.len(), 8).unwrap();
    // Positions in bins of the 8192-point frame.
    let measured = peak.position() / 4.0;
    let expected = 1000.0 * 8192.0 / 22_050.0;
    assert!(
        (measured - expected).abs() < 0.02,
        "{measured} bins, expected {expected}"
    );
}

#[test]
fn a_missing_codec_names_the_codec_and_the_feature() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("layer2.mp2");
    // MPEG-1 layer II frames: 128 kbit/s at 44.1 kHz, 417 bytes each.
    let mut bytes = Vec::new();
    for _ in 0..20 {
        let mut frame = vec![0u8; 417];
        frame[..4].copy_from_slice(&[0xFF, 0xFD, 0x80, 0x04]);
        bytes.extend(frame);
    }
    fs::write(&path, bytes).unwrap();
    let message = audio::decode(&path).unwrap_err().to_string();
    assert!(message.contains("MPEG audio layer II"), "{message}");
    assert!(message.contains("`mp2`"), "{message}");
    assert!(message.contains("layer2.mp2"), "{message}");
}

#[test]
fn a_non_finite_sample_names_the_file_and_the_offset() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("float.wav");
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 44_100,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    for n in 0..1000 {
        writer
            .write_sample(if n == 617 { f32::NAN } else { 0.25 })
            .unwrap();
    }
    writer.finalize().unwrap();
    let message = audio::decode(&path).unwrap_err().to_string();
    assert!(message.contains("float.wav"), "{message}");
    assert!(message.contains("frame 617 of channel 0"), "{message}");
}

#[test]
fn an_unknown_container_and_a_missing_file_fail_with_a_fix() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("noise.bin");
    fs::write(&path, [0x42u8; 4096]).unwrap();
    let message = audio::decode(&path).unwrap_err().to_string();
    assert!(
        message.contains("the container is not supported"),
        "{message}"
    );
    assert!(message.contains("WAV"), "{message}");
    let missing = audio::decode(&dir.path().join("absent.wav")).unwrap_err();
    assert!(
        missing.to_string().contains("cannot open the file"),
        "{missing}"
    );
}
