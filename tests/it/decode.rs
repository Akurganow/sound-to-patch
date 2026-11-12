//! Stem intake (#45): decoding generated files and resampling.

use std::fs;
use std::path::Path;

use sound_to_patch::audio::{self, SampleRate};
use sound_to_patch::dsp::{Stft, StftParams, strongest_peak};

use crate::support::containers::{
    write_adts_silence, write_aiff16, write_flac16, write_mkv_pcm16, write_mkv_video_only,
    write_mp4_pcm16, write_ogg_flac16,
};
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

/// Two channels of different tones and levels, `frames` long.
fn stereo_tones(rate: u32, frames: usize) -> Vec<Vec<f64>> {
    let left = sine(440.0, 0.0, f64::from(rate), frames);
    let right = sine(660.0, 1.0, f64::from(rate), frames)
        .iter()
        .map(|v| 0.5 * v)
        .collect();
    vec![left, right]
}

/// Decodes `path` and checks the rate, the channel count, the frame count
/// and that every sample lies within one 16-bit step of `expected`.
fn assert_decodes_to(path: &Path, rate: u32, expected: &[Vec<f64>]) {
    let decoded = audio::decode(path).unwrap();
    assert_eq!(decoded.skipped(), []);
    let decoded = decoded.into_audio();
    assert_eq!(decoded.sample_rate().get(), rate);
    assert_eq!(decoded.channels().len(), expected.len());
    assert_eq!(decoded.frames(), expected[0].len());
    // One 16-bit step is 2^-15, 3.1e-5.
    for (channel, original) in decoded.channels().iter().zip(expected) {
        for (a, b) in channel.iter().zip(original) {
            assert!((a - b).abs() <= 3.1e-5, "{a} vs {b}");
        }
    }
}

#[test]
fn a_stereo_aiff_decodes_at_its_rate_with_both_channels() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stereo.aiff");
    let signal = stereo_tones(44_100, 4410);
    write_aiff16(&path, 44_100, &signal);
    assert_decodes_to(&path, 44_100, &signal);
}

#[test]
fn a_stereo_flac_decodes_across_frames_with_a_short_last_one() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stereo.flac");
    // Three frames of 1024, 1024 and 952 samples.
    let signal = stereo_tones(48_000, 3000);
    write_flac16(&path, 48_000, &signal);
    assert_decodes_to(&path, 48_000, &signal);
}

#[test]
fn a_mono_flac_in_ogg_decodes_at_its_rate() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mono.ogg");
    let signal = vec![sine(440.0, 0.0, 32_000.0, 2500)];
    write_ogg_flac16(&path, 32_000, &signal);
    assert_decodes_to(&path, 32_000, &signal);
}

#[test]
fn a_stereo_pcm_track_in_matroska_decodes_across_blocks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stereo.mkv");
    // Five blocks of 1000 frames, the last one with 410.
    let signal = stereo_tones(44_100, 4410);
    write_mkv_pcm16(&path, 44_100, &signal);
    assert_decodes_to(&path, 44_100, &signal);
}

#[test]
fn a_stereo_pcm_track_in_mp4_decodes_across_samples() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stereo.m4a");
    // Five samples of 1024 frames, the last one with 314.
    let signal = stereo_tones(44_100, 4410);
    write_mp4_pcm16(&path, 44_100, &signal);
    assert_decodes_to(&path, 44_100, &signal);
}

#[test]
fn an_adts_stream_decodes_in_whole_aac_frames_for_one_and_two_channels() {
    let dir = tempfile::tempdir().unwrap();
    for channels in [1u8, 2] {
        let path = dir.path().join(format!("silence{channels}.aac"));
        write_adts_silence(&path, 44_100, channels, 10);
        let decoded = audio::decode(&path).unwrap();
        assert_eq!(decoded.skipped(), []);
        let decoded = decoded.into_audio();
        assert_eq!(decoded.sample_rate().get(), 44_100);
        assert_eq!(decoded.channels().len(), usize::from(channels));
        // Ten ADTS frames of 1024 samples per channel. The decoder trims no
        // priming frames, so the count is exact.
        assert_eq!(decoded.frames(), 10 * 1024);
        assert!(decoded.channels().iter().flatten().all(|&v| v == 0.0));
    }
}

#[test]
fn a_container_without_an_audio_track_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("video.mkv");
    write_mkv_video_only(&path);
    let message = audio::decode(&path).unwrap_err().to_string();
    assert!(message.contains("video.mkv"), "{message}");
    assert!(message.contains("the file has no audio track"), "{message}");
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

/// A 16-bit PCM WAV whose header declares `rate` and `claimed` data bytes,
/// followed by `frames` frames of silence in `channels` channels.
pub(crate) fn raw_wav(path: &Path, rate: u32, channels: u16, frames: usize, claimed: Option<u32>) {
    let block = channels * 2;
    let data = vec![0u8; frames * usize::from(block)];
    let size = claimed.unwrap_or_else(|| u32::try_from(data.len()).unwrap());
    let mut bytes = Vec::new();
    bytes.extend(b"RIFF");
    bytes.extend((36 + size).to_le_bytes());
    bytes.extend(b"WAVEfmt ");
    bytes.extend(16_u32.to_le_bytes());
    bytes.extend(1_u16.to_le_bytes());
    bytes.extend(channels.to_le_bytes());
    bytes.extend(rate.to_le_bytes());
    bytes.extend(rate.wrapping_mul(u32::from(block)).to_le_bytes());
    bytes.extend(block.to_le_bytes());
    bytes.extend(16_u16.to_le_bytes());
    bytes.extend(b"data");
    bytes.extend(size.to_le_bytes());
    bytes.extend(data);
    fs::write(path, bytes).unwrap();
}

#[test]
fn a_header_rate_outside_the_range_is_refused_before_decoding() {
    let dir = tempfile::tempdir().unwrap();
    for rate in [1, 8, 7_999, 192_001, 2_000_000_000] {
        let path = dir.path().join(format!("rate{rate}.wav"));
        raw_wav(&path, rate, 1, 4000, None);
        let message = audio::decode(&path).unwrap_err().to_string();
        assert_eq!(
            message,
            format!(
                "cannot decode {}: the header declares a sample rate of {rate} Hz, outside the \
8000 Hz to 192000 Hz that the analysis reads. Export the stem again at 44.1 or 48 kHz",
                path.display()
            )
        );
    }
    for rate in [8_000, 192_000] {
        let path = dir.path().join(format!("edge{rate}.wav"));
        raw_wav(&path, rate, 1, 4000, None);
        let decoded = audio::decode(&path).unwrap().into_audio();
        assert_eq!(decoded.sample_rate().get(), rate);
        assert_eq!(decoded.frames(), 4000);
    }
}

#[test]
fn a_header_without_samples_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("empty.wav");
    raw_wav(&path, 44_100, 1, 0, None);
    let message = audio::decode(&path).unwrap_err().to_string();
    assert!(
        message.ends_with("empty.wav: the audio track holds no samples"),
        "{message}"
    );
}

#[test]
fn a_header_without_a_sample_rate_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("norate.wav");
    raw_wav(&path, 0, 1, 100, None);
    let message = audio::decode(&path).unwrap_err().to_string();
    assert!(
        message.ends_with(
            "norate.wav: the audio track declares no sample rate. Export the stem again as WAV"
        ),
        "{message}"
    );
}

#[test]
fn a_header_that_claims_more_data_than_the_file_holds_fails_as_a_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("short.wav");
    // 1000 frames of 4 bytes hold 4000 bytes; the header claims 100 times as many.
    raw_wav(&path, 44_100, 2, 1000, Some(400_000));
    let message = audio::decode(&path).unwrap_err().to_string();
    assert!(
        message.contains("short.wav: reading failed (") && message.ends_with("). Check the file"),
        "{message}"
    );
}

#[test]
fn a_file_of_three_channels_warns_that_two_are_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("three.wav");
    raw_wav(&path, 48_000, 3, 100, None);
    let decoded = audio::decode(&path).unwrap();
    assert_eq!(decoded.audio().channels().len(), 3);
    assert_eq!(
        decoded.warnings(),
        ["the file has 3 channels; the analysis reads the first two and ignores the other 1"]
    );
    let pair = dir.path().join("two.wav");
    raw_wav(&pair, 48_000, 2, 100, None);
    assert_eq!(
        audio::decode(&pair).unwrap().warnings(),
        Vec::<String>::new()
    );
}
