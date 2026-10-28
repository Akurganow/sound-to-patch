//! The WAV writer: 32-bit float samples, interleaved, the form a render
//! writes.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

use super::Audio;
use crate::cast;

/// The WAVE format tag of IEEE float samples.
const IEEE_FLOAT: u16 = 3;
/// The bytes of one sample.
const SAMPLE_BYTES: u16 = 4;

/// Writes `audio` to `path` as a WAV file of 32-bit float samples. Each
/// sample rounds to the nearest `f32`.
///
/// # Errors
///
/// Returns the I/O error of the write, or [`io::ErrorKind::InvalidInput`]
/// for audio too long for the 4 GB size field of a WAV file.
pub fn write_wav(path: &Path, audio: &Audio) -> io::Result<()> {
    let mut out = BufWriter::new(File::create(path)?);
    write_to(&mut out, audio)?;
    out.flush()
}

fn write_to(out: &mut impl Write, audio: &Audio) -> io::Result<()> {
    let too_long = || io::Error::new(io::ErrorKind::InvalidInput, "the audio is too long for WAV");
    let channels = u16::try_from(audio.channels().len()).map_err(|_| too_long())?;
    let block = channels.checked_mul(SAMPLE_BYTES).ok_or_else(too_long)?;
    let data = u32::try_from(audio.frames())
        .ok()
        .and_then(|frames| frames.checked_mul(u32::from(block)))
        .ok_or_else(too_long)?;
    let riff = data.checked_add(36).ok_or_else(too_long)?;
    let rate = audio.sample_rate().get();
    out.write_all(b"RIFF")?;
    out.write_all(&riff.to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&16_u32.to_le_bytes())?;
    out.write_all(&IEEE_FLOAT.to_le_bytes())?;
    out.write_all(&channels.to_le_bytes())?;
    out.write_all(&rate.to_le_bytes())?;
    out.write_all(&(rate * u32::from(block)).to_le_bytes())?;
    out.write_all(&block.to_le_bytes())?;
    out.write_all(&(SAMPLE_BYTES * 8).to_le_bytes())?;
    out.write_all(b"data")?;
    out.write_all(&data.to_le_bytes())?;
    for frame in 0..audio.frames() {
        for channel in audio.channels() {
            out.write_all(&cast::f64_to_f32(channel[frame]).to_le_bytes())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{SampleRate, decode};

    #[test]
    fn the_decoder_reads_a_written_file_back() {
        let dir = std::env::temp_dir().join(format!("stp-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("two.wav");
        let left = vec![0.0, 0.5, -0.25, 1.0];
        let right = vec![-1.0, 0.125, 0.0, 0.75];
        let audio = Audio::new(SampleRate::new(44_100).unwrap(), vec![left, right]).unwrap();
        write_wav(&path, &audio).unwrap();
        let read = decode(&path).unwrap().into_audio();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(read, audio);
    }

    #[test]
    fn the_header_names_float_samples() {
        let audio = Audio::new(SampleRate::new(48_000).unwrap(), vec![vec![0.5; 3]]).unwrap();
        let mut bytes = Vec::new();
        write_to(&mut bytes, &audio).unwrap();
        assert_eq!(bytes.len(), 44 + 12);
        assert_eq!(&bytes[20..22], &3_u16.to_le_bytes());
        assert_eq!(&bytes[24..28], &48_000_u32.to_le_bytes());
        assert_eq!(&bytes[40..44], &12_u32.to_le_bytes());
        assert_eq!(&bytes[44..48], &0.5_f32.to_le_bytes());
    }
}
