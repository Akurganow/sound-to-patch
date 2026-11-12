//! Decoding with symphonia at the file's native rate.

use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use symphonia::core::codecs::audio::AudioCodecId;
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_AC3, CODEC_ID_ALAC, CODEC_ID_EAC3, CODEC_ID_MONKEYS_AUDIO, CODEC_ID_MP1, CODEC_ID_MP2,
    CODEC_ID_OPUS, CODEC_ID_WAVPACK, CODEC_ID_WMA,
};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Duration, TimeBase};

use super::{Audio, AudioError, SampleRate};
use crate::units::Seconds;

/// The containers this build reads, for the unsupported-container message.
const CONTAINERS: &str = "WAV, AIFF, FLAC, MP3, AAC (ADTS), MP4 or M4A, Ogg, MKV or WebM";

/// The lowest sample rate the analysis reads: the band-edge rule looks for
/// its fall above 2 kHz, so it needs a Nyquist frequency well above that.
pub const MIN_SAMPLE_RATE: SampleRate = SampleRate::from_const(8_000);
/// The highest sample rate the analysis reads, the top of the standard
/// export rates. The cost of the analysis grows faster than the rate.
pub const MAX_SAMPLE_RATE: SampleRate = SampleRate::from_const(192_000);

/// The most packets a file may lose to decode errors.
///
/// symphonia reports a corrupt packet as recoverable: the next packet
/// decodes. A lost packet becomes silence of its duration, so every later
/// sample keeps its time. More lost packets than this mean a damaged file.
pub const MAX_SKIPPED_PACKETS: usize = 4;

/// A decoded file: its audio and the packets that became silence.
#[derive(Clone, Debug, PartialEq)]
pub struct Decoded {
    audio: Audio,
    skipped: Vec<SkippedPacket>,
}

impl Decoded {
    /// Returns the audio.
    #[must_use]
    pub fn audio(&self) -> &Audio {
        &self.audio
    }

    /// Returns the audio and drops the report.
    #[must_use]
    pub fn into_audio(self) -> Audio {
        self.audio
    }

    /// Returns what the analysis leaves out of the file, one line each,
    /// without a `warning:` prefix: the channels past the first two.
    #[must_use]
    pub fn warnings(&self) -> Vec<String> {
        match self.audio.channels().len() {
            count @ 3.. => vec![format!(
                "the file has {count} channels; the analysis reads the first two and ignores the other {}",
                count - 2
            )],
            _ => Vec::new(),
        }
    }

    /// Returns the packets that failed to decode, at most
    /// [`MAX_SKIPPED_PACKETS`], in file order.
    #[must_use]
    pub fn skipped(&self) -> &[SkippedPacket] {
        &self.skipped
    }
}

/// A packet that failed to decode and became silence.
#[derive(Clone, Debug, PartialEq)]
pub struct SkippedPacket {
    index: usize,
    start: Seconds,
    frames: usize,
    message: &'static str,
}

impl SkippedPacket {
    /// Returns the packet's position among the track's packets, from 0.
    #[must_use]
    pub fn index(&self) -> usize {
        self.index
    }

    /// Returns where the silence starts in the decoded audio.
    #[must_use]
    pub fn start(&self) -> Seconds {
        self.start
    }

    /// Returns the length of the silence in frames.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// Returns symphonia's message.
    #[must_use]
    pub fn message(&self) -> &'static str {
        self.message
    }
}

impl fmt::Display for SkippedPacket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "packet {} at {} ({})",
            self.index, self.start, self.message
        )
    }
}

/// Decodes the default audio track of a file at its native rate, with every
/// channel kept: the audio track the container flags as default, else the
/// first one with a known codec.
///
/// A packet that fails to decode becomes silence and is listed in
/// [`Decoded::skipped`]; see [`MAX_SKIPPED_PACKETS`].
///
/// # Errors
///
/// Returns a [`DecodeError`] that names the file, what failed and how to fix
/// it: an unreadable file, an unsupported container or codec, a malformed
/// stream or too many corrupt packets, a sample rate outside
/// [`MIN_SAMPLE_RATE`] to [`MAX_SAMPLE_RATE`], or a NaN or infinite sample
/// with its offset.
pub fn decode(path: &Path) -> Result<Decoded, DecodeError> {
    let file = File::open(path).map_err(|source| DecodeError {
        path: path.to_path_buf(),
        kind: Kind::Open(source),
    })?;
    decode_source(path, Box::new(file))
}

/// Reads a stem file into memory. [`decode_bytes`] then decodes the same
/// bytes that a checksum reads, so a file that allows one read only, such
/// as a pipe, still decodes.
///
/// # Errors
///
/// The [`DecodeError`] that [`decode`] gives when the file cannot be opened
/// or read.
pub(crate) fn read_stem(path: &Path) -> Result<Vec<u8>, DecodeError> {
    use std::io::Read as _;
    let fail = |kind| DecodeError {
        path: path.to_path_buf(),
        kind,
    };
    let mut file = File::open(path).map_err(|source| fail(Kind::Open(source)))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|source| fail(Kind::Read(source)))?;
    Ok(bytes)
}

/// [`decode`] of a file that [`read_stem`] read; errors name `path`.
///
/// # Errors
///
/// As [`decode`].
pub(crate) fn decode_bytes(path: &Path, bytes: Vec<u8>) -> Result<Decoded, DecodeError> {
    decode_source(path, Box::new(io::Cursor::new(bytes)))
}

/// [`decode`] of an open source; errors name `path`.
fn decode_source(
    path: &Path,
    source: Box<dyn symphonia::core::io::MediaSource>,
) -> Result<Decoded, DecodeError> {
    let fail = |kind| DecodeError {
        path: path.to_path_buf(),
        kind,
    };
    let stream = MediaSourceStream::new(source, MediaSourceStreamOptions::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
        hint.with_extension(extension);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|error| match error {
            SymphoniaError::Unsupported(_) => fail(Kind::UnsupportedContainer),
            other => fail(Kind::from_symphonia(other)),
        })?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| fail(Kind::NoAudioTrack))?;
    let track_id = track.id;
    let time_base = track.time_base;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|params| params.audio())
        .ok_or_else(|| fail(Kind::NoAudioTrack))?
        .clone();
    let sample_rate = params
        .sample_rate
        .and_then(|rate| SampleRate::new(rate).ok())
        .ok_or_else(|| fail(Kind::NoSampleRate))?;
    // A header rate far outside the range costs gigabytes later on.
    if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
        return Err(fail(Kind::RateOutOfRange(sample_rate)));
    }
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|error| match error {
            SymphoniaError::Unsupported(_) => fail(Kind::UnsupportedCodec(params.codec)),
            other => fail(Kind::from_symphonia(other)),
        })?;

    let mut collected = Collected::default();
    let mut planes: Vec<Vec<f64>> = Vec::new();
    let mut index = 0;
    while let Some(packet) = format
        .next_packet()
        .map_err(|error| fail(Kind::from_symphonia(error)))?
    {
        if packet.track_id != track_id {
            continue;
        }
        index += 1;
        match decoder.decode(&packet) {
            Ok(buffer) => {
                buffer.copy_to_vecs_planar::<f64>(&mut planes);
                collected.append(&planes).map_err(fail)?;
            }
            Err(SymphoniaError::DecodeError(message)) => {
                let frames = packet_frames(packet.dur, time_base, sample_rate);
                collected
                    .skip(index - 1, frames, message, sample_rate)
                    .map_err(fail)?;
            }
            Err(error) => return Err(fail(Kind::from_symphonia(error))),
        }
    }
    let Collected {
        channels, skipped, ..
    } = collected;
    if channels.is_empty() {
        return Err(fail(Kind::Empty));
    }
    let audio = Audio::new(sample_rate, channels).map_err(|error| match error {
        AudioError::NonFinite { channel, frame } => fail(Kind::NonFinite { channel, frame }),
        other => fail(Kind::Audio(other)),
    })?;
    Ok(Decoded { audio, skipped })
}

/// The channels decoded so far and the packets that became silence.
#[derive(Default)]
struct Collected {
    channels: Vec<Vec<f64>>,
    skipped: Vec<SkippedPacket>,
    /// Silence owed before the first decoded packet fixes the channel count.
    leading_silence: usize,
    /// The length of the last decoded packet, for a lost packet of unknown
    /// duration.
    last_frames: usize,
}

impl Collected {
    fn append(&mut self, planes: &[Vec<f64>]) -> Result<(), Kind> {
        if self.channels.is_empty() {
            self.channels
                .resize(planes.len(), vec![0.0; self.leading_silence]);
        } else if planes.len() != self.channels.len() {
            return Err(Kind::ChannelsChanged {
                from: self.channels.len(),
                to: planes.len(),
            });
        }
        for (channel, plane) in self.channels.iter_mut().zip(planes) {
            channel.extend_from_slice(plane);
        }
        self.last_frames = planes.first().map_or(0, Vec::len);
        Ok(())
    }

    fn skip(
        &mut self,
        index: usize,
        frames: Option<usize>,
        message: &'static str,
        rate: SampleRate,
    ) -> Result<(), Kind> {
        let frames = frames.unwrap_or(self.last_frames);
        let done = self.channels.first().map_or(self.leading_silence, Vec::len);
        self.skipped.push(SkippedPacket {
            index,
            start: rate.seconds(done),
            frames,
            message,
        });
        if self.skipped.len() > MAX_SKIPPED_PACKETS {
            return Err(Kind::TooManyBadPackets(self.skipped.swap_remove(0)));
        }
        if self.channels.is_empty() {
            self.leading_silence += frames;
        }
        for channel in &mut self.channels {
            channel.resize(channel.len() + frames, 0.0);
        }
        Ok(())
    }
}

/// The frames a packet of duration `dur` covers, from the track's time base.
fn packet_frames(dur: Duration, time_base: Option<TimeBase>, rate: SampleRate) -> Option<usize> {
    let time_base = time_base?;
    let ticks = u128::from(dur.get()) * u128::from(time_base.numer.get()) * u128::from(rate.get());
    let denom = u128::from(time_base.denom.get());
    usize::try_from((ticks + denom / 2) / denom).ok()
}

/// A file that does not decode: the path and the cause.
#[derive(Debug)]
pub struct DecodeError {
    path: PathBuf,
    kind: Kind,
}

impl DecodeError {
    /// Returns the file that failed.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug)]
enum Kind {
    Open(io::Error),
    Read(io::Error),
    UnsupportedContainer,
    UnsupportedCodec(AudioCodecId),
    NoAudioTrack,
    NoSampleRate,
    RateOutOfRange(SampleRate),
    Malformed(&'static str),
    Limit(&'static str),
    ResetRequired,
    Unexpected,
    ChannelsChanged { from: usize, to: usize },
    TooManyBadPackets(SkippedPacket),
    Empty,
    NonFinite { channel: usize, frame: usize },
    Audio(AudioError),
}

impl Kind {
    fn from_symphonia(error: SymphoniaError) -> Self {
        match error {
            SymphoniaError::IoError(source) => Self::Read(source),
            SymphoniaError::DecodeError(message) | SymphoniaError::Unsupported(message) => {
                Self::Malformed(message)
            }
            SymphoniaError::LimitError(message) => Self::Limit(message),
            // A reset means the stream changed shape. A seek never happens
            // here, and the enum is non-exhaustive.
            SymphoniaError::ResetRequired => Self::ResetRequired,
            _ => Self::Unexpected,
        }
    }
}

/// The name of a codec that this build lacks, and the symphonia Cargo
/// feature that adds it, if one exists.
fn codec_name(codec: AudioCodecId) -> (&'static str, Option<&'static str>) {
    match codec {
        CODEC_ID_MP1 => ("MPEG audio layer I", Some("mp1")),
        CODEC_ID_MP2 => ("MPEG audio layer II", Some("mp2")),
        CODEC_ID_ALAC => ("ALAC", Some("alac")),
        CODEC_ID_OPUS => ("Opus", None),
        CODEC_ID_AC3 => ("AC-3", None),
        CODEC_ID_EAC3 => ("E-AC-3", None),
        CODEC_ID_WMA => ("WMA", None),
        CODEC_ID_WAVPACK => ("WavPack", None),
        CODEC_ID_MONKEYS_AUDIO => ("Monkey's Audio", None),
        _ => ("this", None),
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let path = self.path.display();
        write!(f, "cannot decode {path}: ")?;
        match &self.kind {
            Kind::Open(source) => write!(f, "cannot open the file ({source}). Check the path"),
            Kind::Read(source) => write!(f, "reading failed ({source}). Check the file"),
            Kind::UnsupportedContainer => write!(
                f,
                "the container is not supported. Export the stem as one of: {CONTAINERS}"
            ),
            Kind::UnsupportedCodec(codec) => match codec_name(*codec) {
                (name, Some(feature)) => write!(
                    f,
                    "the {name} codec is not built in. Build with the symphonia Cargo feature `{feature}`, or export the stem as WAV"
                ),
                (name, None) => write!(
                    f,
                    "the {name} codec is not supported. Export the stem as WAV or FLAC"
                ),
            },
            Kind::NoAudioTrack => write!(f, "the file has no audio track"),
            Kind::NoSampleRate => write!(
                f,
                "the audio track declares no sample rate. Export the stem again as WAV"
            ),
            Kind::RateOutOfRange(rate) => write!(
                f,
                "the header declares a sample rate of {rate}, outside the {MIN_SAMPLE_RATE} to {MAX_SAMPLE_RATE} that the analysis reads. Export the stem again at 44.1 or 48 kHz"
            ),
            Kind::Malformed(message) => write!(
                f,
                "the stream is malformed ({message}). Export the stem again"
            ),
            Kind::Limit(message) => write!(f, "a decoder limit was reached ({message})"),
            Kind::ResetRequired => write!(
                f,
                "the stream changes its format midway. Export the stem again as one track"
            ),
            Kind::Unexpected => {
                write!(f, "the decoder failed unexpectedly. Export the stem as WAV")
            }
            Kind::ChannelsChanged { from, to } => write!(
                f,
                "the channel count changes from {from} to {to} midway. Export the stem again"
            ),
            Kind::TooManyBadPackets(first) => write!(
                f,
                "more than {MAX_SKIPPED_PACKETS} packets do not decode, the first {first}. Export the stem again"
            ),
            Kind::Empty => write!(f, "the audio track holds no samples"),
            Kind::NonFinite { channel, frame } => write!(
                f,
                "the sample at frame {frame} of channel {channel} is NaN or infinite. Export the stem again"
            ),
            Kind::Audio(error) => write!(f, "{error}"),
        }
    }
}

impl Error for DecodeError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            Kind::Open(source) | Kind::Read(source) => Some(source),
            Kind::Audio(source) => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rate() -> SampleRate {
        SampleRate::new(44_100).unwrap()
    }

    fn error(kind: Kind) -> DecodeError {
        DecodeError {
            path: PathBuf::from("bass.wav"),
            kind,
        }
    }

    #[test]
    fn every_symphonia_error_maps_to_its_kind() {
        let io = || io::Error::new(io::ErrorKind::UnexpectedEof, "short");
        assert!(matches!(
            Kind::from_symphonia(SymphoniaError::IoError(io())),
            Kind::Read(_)
        ));
        assert!(matches!(
            Kind::from_symphonia(SymphoniaError::DecodeError("bad")),
            Kind::Malformed("bad")
        ));
        assert!(matches!(
            Kind::from_symphonia(SymphoniaError::Unsupported("odd")),
            Kind::Malformed("odd")
        ));
        assert!(matches!(
            Kind::from_symphonia(SymphoniaError::LimitError("big")),
            Kind::Limit("big")
        ));
        assert!(matches!(
            Kind::from_symphonia(SymphoniaError::ResetRequired),
            Kind::ResetRequired
        ));
        assert!(matches!(
            Kind::from_symphonia(SymphoniaError::SeekError(
                symphonia::core::errors::SeekErrorKind::Unseekable
            )),
            Kind::Unexpected
        ));
    }

    #[test]
    fn every_failure_names_the_file_and_what_failed() {
        let skipped = SkippedPacket {
            index: 3,
            start: Seconds::new(0.5).unwrap(),
            frames: 1152,
            message: "bad frame",
        };
        let io = || io::Error::other("denied");
        let cases = [
            (
                Kind::Open(io()),
                "cannot open the file (denied). Check the path",
            ),
            (Kind::Read(io()), "reading failed (denied). Check the file"),
            (Kind::NoAudioTrack, "the file has no audio track"),
            (
                Kind::RateOutOfRange(SampleRate::new(1).unwrap()),
                "the header declares a sample rate of 1 Hz, outside the 8000 Hz to 192000 Hz that the analysis reads. Export the stem again at 44.1 or 48 kHz",
            ),
            (
                Kind::NoSampleRate,
                "the audio track declares no sample rate. Export the stem again as WAV",
            ),
            (
                Kind::Malformed("bad"),
                "the stream is malformed (bad). Export the stem again",
            ),
            (Kind::Limit("big"), "a decoder limit was reached (big)"),
            (
                Kind::ResetRequired,
                "the stream changes its format midway. Export the stem again as one track",
            ),
            (
                Kind::Unexpected,
                "the decoder failed unexpectedly. Export the stem as WAV",
            ),
            (
                Kind::ChannelsChanged { from: 2, to: 1 },
                "the channel count changes from 2 to 1 midway. Export the stem again",
            ),
            (
                Kind::TooManyBadPackets(skipped),
                "more than 4 packets do not decode, the first packet 3 at 0.5 s (bad frame). Export the stem again",
            ),
            (Kind::Empty, "the audio track holds no samples"),
            (
                Kind::NonFinite {
                    channel: 1,
                    frame: 7,
                },
                "the sample at frame 7 of channel 1 is NaN or infinite. Export the stem again",
            ),
            (
                Kind::Audio(AudioError::NoChannels),
                "the audio has no channel; it needs at least one",
            ),
            (
                Kind::UnsupportedCodec(CODEC_ID_OPUS),
                "the Opus codec is not supported. Export the stem as WAV or FLAC",
            ),
            (
                Kind::UnsupportedCodec(CODEC_ID_ALAC),
                "the ALAC codec is not built in. Build with the symphonia Cargo feature `alac`, or export the stem as WAV",
            ),
        ];
        for (kind, cause) in cases {
            assert_eq!(
                error(kind).to_string(),
                format!("cannot decode bass.wav: {cause}")
            );
        }
        assert!(error(Kind::Open(io())).source().is_some());
        assert!(error(Kind::Read(io())).source().is_some());
        assert!(
            error(Kind::Audio(AudioError::NoChannels))
                .source()
                .is_some()
        );
        assert!(error(Kind::Empty).source().is_none());
        assert_eq!(error(Kind::Empty).path(), Path::new("bass.wav"));
    }

    #[test]
    fn a_channel_count_that_changes_midway_is_an_error() {
        let mut collected = Collected::default();
        collected.append(&[vec![0.1, 0.2], vec![0.3, 0.4]]).unwrap();
        let error = collected.append(&[vec![0.5]]).unwrap_err();
        assert!(matches!(error, Kind::ChannelsChanged { from: 2, to: 1 }));
    }

    #[test]
    fn a_lost_first_packet_becomes_leading_silence_in_every_channel() {
        let mut collected = Collected::default();
        collected.skip(0, Some(3), "bad", rate()).unwrap();
        collected.append(&[vec![1.0, 2.0], vec![3.0, 4.0]]).unwrap();
        assert_eq!(
            collected.channels,
            [vec![0.0, 0.0, 0.0, 1.0, 2.0], vec![0.0, 0.0, 0.0, 3.0, 4.0]]
        );
        assert_eq!(collected.skipped[0].start().get(), 0.0);
        assert_eq!(collected.skipped[0].frames(), 3);
    }

    #[test]
    fn a_lost_packet_of_unknown_length_lasts_as_long_as_the_last_one() {
        let mut collected = Collected::default();
        collected.skip(0, None, "bad", rate()).unwrap();
        assert_eq!(collected.skipped[0].frames(), 0);
        collected.append(&[vec![1.0, 2.0, 3.0]]).unwrap();
        collected.skip(2, None, "bad", rate()).unwrap();
        assert_eq!(collected.channels, [vec![1.0, 2.0, 3.0, 0.0, 0.0, 0.0]]);
        assert_eq!(collected.skipped[1].frames(), 3);
        assert_eq!(collected.skipped[1].start().get(), 3.0 / 44_100.0);
    }

    #[test]
    fn the_packet_after_the_limit_fails_and_names_the_first() {
        let mut collected = Collected::default();
        for index in 0..MAX_SKIPPED_PACKETS {
            collected.skip(index, Some(10), "bad", rate()).unwrap();
        }
        let error = collected
            .skip(MAX_SKIPPED_PACKETS, Some(10), "bad", rate())
            .unwrap_err();
        assert!(matches!(error, Kind::TooManyBadPackets(first) if first.index() == 0));
    }

    #[test]
    fn a_packet_length_needs_the_time_base() {
        let dur = Duration::new(1152);
        assert_eq!(packet_frames(dur, None, rate()), None);
        let native = TimeBase::try_from_recip(44_100);
        assert_eq!(packet_frames(dur, native, rate()), Some(1152));
        // 1152 ticks of 1/48000 s at 44.1 kHz is 1058.4 frames.
        let other = TimeBase::try_from_recip(48_000);
        assert_eq!(packet_frames(dur, other, rate()), Some(1058));
    }
}
