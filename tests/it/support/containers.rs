//! Writers of tiny audio files, one per container, so the intake tests read
//! every container the build enables without a committed audio file. Each
//! writer lays the bytes out by hand with 16-bit samples and no
//! compression, except the ADTS writer, which frames silence.

use std::fs;
use std::path::Path;

use super::cast::pcm16;

/// The samples of all channels as 16-bit values in frame order, in the
/// byte order that the container stores.
fn interleaved16(channels: &[Vec<f64>], big_endian: bool) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(channels.len() * channels[0].len() * 2);
    for frame in 0..channels[0].len() {
        for channel in channels {
            let value = pcm16(channel[frame]);
            bytes.extend(if big_endian {
                value.to_be_bytes()
            } else {
                value.to_le_bytes()
            });
        }
    }
    bytes
}

/// The length of a collection as the `u32` that every box and chunk header
/// stores.
fn len32(bytes: &[u8]) -> u32 {
    u32::try_from(bytes.len()).unwrap()
}

/// CRC-8 with polynomial 0x07, initial value 0, as a FLAC frame header ends.
fn crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &byte in bytes {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ 0x07
            };
        }
    }
    crc
}

/// CRC-16 with polynomial 0x8005, initial value 0, as a FLAC frame ends.
fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0u16;
    for &byte in bytes {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ 0x8005
            };
        }
    }
    crc
}

/// CRC-32 with polynomial 0x04C11DB7, initial value 0, most significant bit
/// first, as an Ogg page carries it.
fn crc32_ogg(bytes: &[u8]) -> u32 {
    let mut crc = 0u32;
    for &byte in bytes {
        crc ^= u32::from(byte) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ 0x04C1_1DB7
            };
        }
    }
    crc
}

/// An AIFF file: a `FORM` chunk of type `AIFF` that holds a `COMM` chunk and
/// an `SSND` chunk, with big-endian 16-bit PCM and the rate as an 80-bit
/// extended float.
pub(crate) fn write_aiff16(path: &Path, rate: u32, channels: &[Vec<f64>]) {
    let frames = channels[0].len();
    // The extended float is a 15-bit exponent biased by 16383 and a 64-bit
    // mantissa with its top bit set, so an integer rate shifts up to bit 63.
    let exponent = rate.ilog2();
    let mantissa = u64::from(rate) << (63 - exponent);
    let mut comm = Vec::new();
    comm.extend(u16::try_from(channels.len()).unwrap().to_be_bytes());
    comm.extend(u32::try_from(frames).unwrap().to_be_bytes());
    comm.extend(16u16.to_be_bytes());
    comm.extend(u16::try_from(16383 + exponent).unwrap().to_be_bytes());
    comm.extend(mantissa.to_be_bytes());
    // The sound data starts with a zero offset and a zero block size.
    let mut ssnd = vec![0u8; 8];
    ssnd.extend(interleaved16(channels, true));
    let mut body = b"AIFF".to_vec();
    for (tag, chunk) in [(b"COMM", comm), (b"SSND", ssnd)] {
        body.extend(tag);
        body.extend(len32(&chunk).to_be_bytes());
        body.extend(&chunk);
    }
    let mut file = b"FORM".to_vec();
    file.extend(len32(&body).to_be_bytes());
    file.extend(body);
    fs::write(path, file).unwrap();
}

/// The 34-byte FLAC `STREAMINFO` payload for fixed blocks of `block`
/// samples, 16-bit samples and no MD5 signature.
fn flac_stream_info(rate: u32, channels: usize, frames: usize, block: usize) -> Vec<u8> {
    let block = u16::try_from(block).unwrap().to_be_bytes();
    let mut info = Vec::new();
    // Minimum and maximum block size, then the minimum and maximum frame
    // size, where 0 means unknown.
    info.extend(block);
    info.extend(block);
    info.extend([0u8; 6]);
    // Rate in 20 bits, channels less one in 3, bits per sample less one in 5
    // and the total sample count in 36.
    let packed = (u64::from(rate) << 44)
        | (u64::try_from(channels - 1).unwrap() << 41)
        | (15 << 36)
        | u64::try_from(frames).unwrap();
    info.extend(packed.to_be_bytes());
    info.extend([0u8; 16]);
    info
}

/// The FLAC frames of `channels`, one per `block` samples. Each frame has a
/// fixed-blocksize header with a 16-bit block size, the rate and sample size
/// taken from `STREAMINFO`, independent channels, one verbatim subframe per
/// channel and the CRC-16 footer.
fn flac_frames(channels: &[Vec<f64>], block: usize) -> Vec<Vec<u8>> {
    let frames = channels[0].len();
    (0..frames.div_ceil(block))
        .map(|index| {
            let start = index * block;
            let end = (start + block).min(frames);
            let mut frame = vec![
                0xFF,
                0xF8,
                // Block size code 7: the size less one follows in 16 bits.
                0x70,
                // Independent channels, then 16 bits per sample.
                (u8::try_from(channels.len() - 1).unwrap() << 4) | 0x08,
            ];
            let number = char::from_u32(u32::try_from(index).unwrap()).unwrap();
            frame.extend(number.to_string().as_bytes());
            frame.extend(u16::try_from(end - start - 1).unwrap().to_be_bytes());
            frame.push(crc8(&frame));
            for channel in channels {
                // Subframe header: padding bit 0, type 000001 for verbatim,
                // no wasted bits.
                frame.push(0x02);
                for &sample in &channel[start..end] {
                    frame.extend(pcm16(sample).to_be_bytes());
                }
            }
            frame.extend(crc16(&frame).to_be_bytes());
            frame
        })
        .collect()
}

/// A native FLAC file: the `fLaC` marker, one last `STREAMINFO` block and
/// verbatim 16-bit frames of 1024 samples.
pub(crate) fn write_flac16(path: &Path, rate: u32, channels: &[Vec<f64>]) {
    let info = flac_stream_info(rate, channels.len(), channels[0].len(), 1024);
    let mut file = b"fLaC".to_vec();
    file.extend([0x80, 0, 0, 34]);
    file.extend(info);
    for frame in flac_frames(channels, 1024) {
        file.extend(frame);
    }
    fs::write(path, file).unwrap();
}

/// One Ogg page that holds a single packet, with its CRC-32. The segment
/// table splits the packet into 255-byte lacing values.
fn ogg_page(kind: u8, granule: u64, sequence: u32, packet: &[u8]) -> Vec<u8> {
    let mut segments = vec![255u8; packet.len() / 255];
    segments.push(u8::try_from(packet.len() % 255).unwrap());
    let mut page = b"OggS".to_vec();
    page.extend([0, kind]);
    page.extend(granule.to_le_bytes());
    page.extend(1u32.to_le_bytes());
    page.extend(sequence.to_le_bytes());
    page.extend([0u8; 4]);
    page.push(u8::try_from(segments.len()).unwrap());
    page.extend(segments);
    page.extend(packet);
    let crc = crc32_ogg(&page).to_le_bytes();
    page[22..26].copy_from_slice(&crc);
    page
}

/// An Ogg file of one FLAC logical stream in the Ogg mapping: a first page
/// with the 51-byte identification packet (`0x7F`, `FLAC`, version 1.0, no
/// header count, `fLaC` and the last `STREAMINFO` block), then one page per
/// verbatim 16-bit frame of 1024 samples, the last page flagged as the end.
pub(crate) fn write_ogg_flac16(path: &Path, rate: u32, channels: &[Vec<f64>]) {
    let info = flac_stream_info(rate, channels.len(), channels[0].len(), 1024);
    let mut ident = vec![0x7F];
    ident.extend(b"FLAC");
    ident.extend([1, 0, 0, 0]);
    ident.extend(b"fLaC");
    ident.extend([0x80, 0, 0, 34]);
    ident.extend(info);
    let mut file = ogg_page(0x02, 0, 0, &ident);
    let frames = flac_frames(channels, 1024);
    let last = frames.len() - 1;
    for (index, frame) in frames.iter().enumerate() {
        let granule = u64::try_from(((index + 1) * 1024).min(channels[0].len())).unwrap();
        let kind = if index == last { 0x04 } else { 0x00 };
        file.extend(ogg_page(
            kind,
            granule,
            u32::try_from(index + 1).unwrap(),
            frame,
        ));
    }
    fs::write(path, file).unwrap();
}

/// One EBML element. The size is always the 8-byte form, a `0x01` marker
/// and 7 length bytes, which every reader accepts.
fn ebml(id: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut element = id.to_vec();
    element.push(0x01);
    element.extend(&u64::try_from(payload.len()).unwrap().to_be_bytes()[1..]);
    element.extend(payload);
    element
}

/// An EBML unsigned integer element in its shortest form.
fn ebml_uint(id: &[u8], value: u64) -> Vec<u8> {
    let bytes = value.to_be_bytes();
    let skip = bytes.iter().take(7).take_while(|&&byte| byte == 0).count();
    ebml(id, &bytes[skip..])
}

/// The EBML header and the `Segment` start for a Matroska file that holds
/// `children`, with `Info` at the default millisecond scale.
fn mkv_file(children: &[Vec<u8>]) -> Vec<u8> {
    let mut header = Vec::new();
    header.extend(ebml_uint(&[0x42, 0x86], 1));
    header.extend(ebml_uint(&[0x42, 0xF7], 1));
    header.extend(ebml_uint(&[0x42, 0xF2], 4));
    header.extend(ebml_uint(&[0x42, 0xF3], 8));
    header.extend(ebml(&[0x42, 0x82], b"matroska"));
    header.extend(ebml_uint(&[0x42, 0x87], 4));
    header.extend(ebml_uint(&[0x42, 0x85], 2));
    let info = [
        ebml_uint(&[0x2A, 0xD7, 0xB1], 1_000_000),
        ebml(&[0x4D, 0x80], b"test"),
        ebml(&[0x57, 0x41], b"test"),
    ]
    .concat();
    let mut segment = ebml(&[0x15, 0x49, 0xA9, 0x66], &info);
    for child in children {
        segment.extend(child);
    }
    let mut file = ebml(&[0x1A, 0x45, 0xDF, 0xA3], &header);
    file.extend(ebml(&[0x18, 0x53, 0x80, 0x67], &segment));
    file
}

/// A Matroska file with one audio track, codec `A_PCM/INT/LIT` at 16 bits,
/// and one cluster of simple blocks that hold 1000 frames each, stamped in
/// milliseconds.
pub(crate) fn write_mkv_pcm16(path: &Path, rate: u32, channels: &[Vec<f64>]) {
    let audio = [
        ebml(&[0xB5], &f64::from(rate).to_be_bytes()),
        ebml_uint(&[0x9F], u64::try_from(channels.len()).unwrap()),
        ebml_uint(&[0x62, 0x64], 16),
    ]
    .concat();
    let track = [
        ebml_uint(&[0xD7], 1),
        ebml_uint(&[0x73, 0xC5], 1),
        ebml_uint(&[0x83], 2),
        ebml(&[0x86], b"A_PCM/INT/LIT"),
        ebml(&[0xE1], &audio),
    ]
    .concat();
    let tracks = ebml(&[0x16, 0x54, 0xAE, 0x6B], &ebml(&[0xAE], &track));
    let frames = channels[0].len();
    let mut cluster = ebml_uint(&[0xE7], 0);
    for start in (0..frames).step_by(1000) {
        let end = (start + 1000).min(frames);
        let slice: Vec<Vec<f64>> = channels.iter().map(|c| c[start..end].to_vec()).collect();
        // Track number 1 as a vint, the time in milliseconds from the
        // cluster start, and the keyframe flag.
        let millis = u16::try_from(start * 1000 / usize::try_from(rate).unwrap()).unwrap();
        let mut block = vec![0x81];
        block.extend(millis.to_be_bytes());
        block.push(0x80);
        block.extend(interleaved16(&slice, false));
        cluster.extend(ebml(&[0xA3], &block));
    }
    let cluster = ebml(&[0x1F, 0x43, 0xB6, 0x75], &cluster);
    fs::write(path, mkv_file(&[tracks, cluster])).unwrap();
}

/// A Matroska file with a single video track, codec `V_VP8`, and an empty
/// cluster: a container that parses and has no audio track.
pub(crate) fn write_mkv_video_only(path: &Path) {
    let video = [ebml_uint(&[0xB0], 64), ebml_uint(&[0xBA], 48)].concat();
    let track = [
        ebml_uint(&[0xD7], 1),
        ebml_uint(&[0x73, 0xC5], 1),
        ebml_uint(&[0x83], 1),
        ebml(&[0x86], b"V_VP8"),
        ebml(&[0xE0], &video),
    ]
    .concat();
    let tracks = ebml(&[0x16, 0x54, 0xAE, 0x6B], &ebml(&[0xAE], &track));
    let cluster = ebml(&[0x1F, 0x43, 0xB6, 0x75], &ebml_uint(&[0xE7], 0));
    fs::write(path, mkv_file(&[tracks, cluster])).unwrap();
}

/// An ISO box: a 32-bit size that counts the header, the four-character
/// type and the payload.
fn mp4_box(kind: [u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut atom = (len32(payload) + 8).to_be_bytes().to_vec();
    atom.extend(kind);
    atom.extend(payload);
    atom
}

/// An ISO box with the version and flags word, both zero, before the
/// payload.
fn mp4_full_box(kind: [u8; 4], payload: &[u8]) -> Vec<u8> {
    mp4_box(kind, &[&[0u8; 4], payload].concat())
}

/// A big-endian `u32` list as bytes.
fn be32s(values: &[u32]) -> Vec<u8> {
    values
        .iter()
        .flat_map(|value| value.to_be_bytes())
        .collect()
}

/// The `stbl` box of the PCM track: one `sowt` sample description, the
/// durations of its samples in frame ticks, one chunk that holds every
/// sample, the sample sizes and the chunk offset.
fn mp4_sample_table(
    rate: u32,
    channels: usize,
    sizes: &[u32],
    frames: u32,
    offset: u32,
) -> Vec<u8> {
    let count = u32::try_from(sizes.len()).unwrap();
    // Sample entry: reserved bytes, data reference 1, sound version 0 with
    // revision and vendor, channels, 16 bits, compression id and packet
    // size, then the rate as 16.16 fixed point.
    let mut entry = vec![0u8; 6];
    entry.extend(1u16.to_be_bytes());
    entry.extend([0u8; 8]);
    entry.extend(u16::try_from(channels).unwrap().to_be_bytes());
    entry.extend(16u16.to_be_bytes());
    entry.extend([0u8; 4]);
    entry.extend((rate << 16).to_be_bytes());
    let description = mp4_full_box(*b"stsd", &[be32s(&[1]), mp4_box(*b"sowt", &entry)].concat());
    // Full samples last 1024 ticks, then the shorter rest, if any.
    let rest = frames - (count - 1) * 1024;
    let mut runs = vec![(count - 1, 1024), (1, rest)];
    runs.retain(|&(run, _)| run > 0);
    let timing = mp4_full_box(
        *b"stts",
        &[
            be32s(&[u32::try_from(runs.len()).unwrap()]),
            runs.iter()
                .flat_map(|&(run, delta)| be32s(&[run, delta]))
                .collect(),
        ]
        .concat(),
    );
    let chunking = mp4_full_box(*b"stsc", &be32s(&[1, 1, count, 1]));
    let sizing = mp4_full_box(*b"stsz", &[be32s(&[0, count]), be32s(sizes)].concat());
    let offsets = mp4_full_box(*b"stco", &be32s(&[1, offset]));
    mp4_box(
        *b"stbl",
        &[description, timing, chunking, sizing, offsets].concat(),
    )
}

/// The `moov` box of the PCM track. The media timescale is the rate, so one
/// tick is one frame.
fn mp4_movie(rate: u32, channels: usize, sizes: &[u32], frames: u32, offset: u32) -> Vec<u8> {
    let movie_header = [
        &[0u8; 8][..],
        &rate.to_be_bytes(),
        &frames.to_be_bytes(),
        // Playback rate 1.0, volume 1.0, reserved bytes, the unit matrix
        // as zeros, predefined bytes and the next track id.
        &0x0001_0000u32.to_be_bytes(),
        &0x0100u16.to_be_bytes(),
        &[0u8; 70],
        &2u32.to_be_bytes(),
    ]
    .concat();
    let track_header = [
        &[0u8; 8][..],
        &1u32.to_be_bytes(),
        &[0u8; 4],
        &frames.to_be_bytes(),
        &[0u8; 12],
        &0x0100u16.to_be_bytes(),
        &[0u8; 46],
    ]
    .concat();
    // The language is `und`, then the quality word.
    let media_header = [
        &[0u8; 8][..],
        &rate.to_be_bytes(),
        &frames.to_be_bytes(),
        &0x55C4u16.to_be_bytes(),
        &[0u8; 2],
    ]
    .concat();
    let handler = [&[0u8; 4][..], b"soun", &[0u8; 12], b"\0"].concat();
    let data_ref = mp4_full_box(
        *b"dref",
        &[be32s(&[1]), mp4_full_box(*b"url ", &[])].concat(),
    );
    let media_info = mp4_box(
        *b"minf",
        &[
            mp4_full_box(*b"smhd", &[0u8; 4]),
            mp4_box(*b"dinf", &data_ref),
            mp4_sample_table(rate, channels, sizes, frames, offset),
        ]
        .concat(),
    );
    let media = mp4_box(
        *b"mdia",
        &[
            mp4_full_box(*b"mdhd", &media_header),
            mp4_full_box(*b"hdlr", &handler),
            media_info,
        ]
        .concat(),
    );
    let track = mp4_box(
        *b"trak",
        &[mp4_full_box(*b"tkhd", &track_header), media].concat(),
    );
    mp4_box(
        *b"moov",
        &[mp4_full_box(*b"mvhd", &movie_header), track].concat(),
    )
}

/// An MP4 file, `ftyp` then `moov` then `mdat`, with one sound track of
/// little-endian 16-bit PCM in a `sowt` sample entry. The track has one
/// chunk of samples that hold 1024 frames each, the last one shorter. The
/// rate must fit the 16.16 field of the sample entry, below 65536.
pub(crate) fn write_mp4_pcm16(path: &Path, rate: u32, channels: &[Vec<f64>]) {
    assert!(rate < 65_536, "the sample entry stores the rate in 16 bits");
    let frames = u32::try_from(channels[0].len()).unwrap();
    let data = interleaved16(channels, false);
    let sizes: Vec<u32> = data.chunks(1024 * channels.len() * 2).map(len32).collect();
    let ftyp = mp4_box(
        *b"ftyp",
        &[&b"M4A "[..], &[0u8; 4][..], &b"M4A "[..], &b"isom"[..]].concat(),
    );
    // The chunk offset counts from the file start, and the `moov` length
    // does not depend on it.
    let movie = |offset| mp4_movie(rate, channels.len(), &sizes, frames, offset);
    let offset = len32(&ftyp) + len32(&movie(0)) + 8;
    let mut file = ftyp;
    file.extend(movie(offset));
    file.extend(mp4_box(*b"mdat", &data));
    fs::write(path, file).unwrap();
}

/// A bit string that fills bytes from the most significant bit.
#[derive(Default)]
struct Bits {
    bytes: Vec<u8>,
    used: usize,
}

impl Bits {
    /// Appends the low `width` bits of `value`, most significant first.
    fn put(&mut self, value: u32, width: u32) {
        for shift in (0..width).rev() {
            if self.used.is_multiple_of(8) {
                self.bytes.push(0);
            }
            let bit = u8::from((value >> shift) & 1 == 1);
            let last = self.bytes.len() - 1;
            self.bytes[last] |= bit << (7 - self.used % 8);
            self.used += 1;
        }
    }
}

/// An AAC-LC raw data block that decodes to silence: for one channel a
/// single channel element, for two a channel pair without a common window.
/// Each channel stream has global gain 100, a long window, zero scale factor
/// bands, and no pulse, TNS or gain control, so it carries no spectrum. The
/// block ends with the end element and pads to a byte.
fn aac_silent_block(channels: u8) -> Vec<u8> {
    let mut bits = Bits::default();
    // Element type 0 is a single channel, 1 a channel pair, then tag 0.
    bits.put(u32::from(channels - 1), 3);
    bits.put(0, 4);
    if channels == 2 {
        // No common window, so each stream has its own window info.
        bits.put(0, 1);
    }
    for _ in 0..channels {
        bits.put(100, 8);
        // Reserved bit, window sequence, window shape, band count and
        // prediction flag.
        bits.put(0, 11);
        // Pulse, TNS and gain control flags.
        bits.put(0, 3);
    }
    bits.put(7, 3);
    bits.bytes
}

/// An AAC file in ADTS framing: `frames` frames of MPEG-4 AAC-LC with no
/// CRC, each a 7-byte header and a silent raw data block of 1024 samples per
/// channel. `channels` is 1 or 2.
pub(crate) fn write_adts_silence(path: &Path, rate: u32, channels: u8, frames: usize) {
    let index = [
        96_000, 88_200, 64_000, 48_000, 44_100, 32_000, 24_000, 22_050, 16_000, 12_000, 11_025,
        8_000, 7_350,
    ]
    .iter()
    .position(|&candidate| candidate == rate)
    .unwrap();
    let block = aac_silent_block(channels);
    let mut file = Vec::new();
    for _ in 0..frames {
        let mut header = Bits::default();
        // Sync word, MPEG-4, layer 0, no CRC, profile LC less one, rate
        // index, private bit, channel configuration, original, home and the
        // two copyright bits.
        header.put(0xFFF, 12);
        header.put(0b0001, 4);
        header.put(1, 2);
        header.put(u32::try_from(index).unwrap(), 4);
        header.put(0, 1);
        header.put(u32::from(channels), 3);
        header.put(0, 4);
        // Frame length with the header, buffer fullness for a variable bit
        // rate and one raw data block.
        header.put(u32::try_from(block.len() + 7).unwrap(), 13);
        header.put(0x7FF, 11);
        header.put(0, 2);
        file.extend(header.bytes);
        file.extend(&block);
    }
    fs::write(path, file).unwrap();
}
