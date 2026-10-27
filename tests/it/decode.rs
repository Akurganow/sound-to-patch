//! Stem intake (#45): decoding generated files and resampling.

use std::fs;
use std::path::Path;

use sound_to_patch::audio::{self, SampleRate};
use sound_to_patch::dsp::{Stft, StftParams, strongest_peak};

use crate::support::signals::{sine, write_wav16};

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
    assert_eq!(decoded.skipped(), []);
    let decoded = decoded.into_audio();
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

/// MPEG-1 layer III frames, 128 kbit/s at 44.1 kHz, of silence. A frame in
/// `bad` declares 511 big values in its side information, more than the 288
/// a granule holds, so symphonia rejects that packet and decodes the next.
pub(crate) fn mp3_with_bad_frames(path: &Path, frames: usize, bad: &[usize]) {
    let mut bytes = Vec::new();
    for index in 0..frames {
        let mut frame = vec![0u8; 417];
        frame[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x04]);
        if bad.contains(&index) {
            frame[8] = 0xFF;
            frame[9] = 0x80;
        }
        bytes.extend(frame);
    }
    fs::write(path, bytes).unwrap();
}

#[test]
fn a_corrupt_packet_becomes_silence_of_its_length() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("glitch.mp3");
    mp3_with_bad_frames(&path, 20, &[0, 5]);
    let decoded = audio::decode(&path).unwrap();
    // 20 frames of 1152 samples: the lost packets keep their place.
    assert_eq!(decoded.audio().frames(), 20 * 1152);
    let skipped = decoded.skipped();
    assert_eq!(skipped.len(), 2);
    assert_eq!((skipped[0].index(), skipped[0].frames()), (0, 1152));
    assert_eq!(skipped[1].index(), 5);
    assert_eq!(skipped[1].start().get(), 5.0 * 1152.0 / 44_100.0);
    assert!(
        skipped[1].message().contains("big_values"),
        "{}",
        skipped[1].message()
    );
}

#[test]
fn too_many_corrupt_packets_fail_and_name_the_first() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("damaged.mp3");
    let bad: Vec<usize> = (3..=3 + audio::MAX_SKIPPED_PACKETS).collect();
    mp3_with_bad_frames(&path, 20, &bad);
    let message = audio::decode(&path).unwrap_err().to_string();
    assert!(message.contains("damaged.mp3"), "{message}");
    assert!(
        message.contains("more than 4 packets do not decode, the first packet 3 at"),
        "{message}"
    );
}
