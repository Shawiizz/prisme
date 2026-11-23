//! Audio file decoding.
//!
//! A track is decoded entirely into memory as interleaved stereo `f32` ([`TrackAudio`]).
//! For DVS this is what we want: scratching and needle drops need instant random access
//! anywhere in the track, which streaming decoders cannot give. Decoding happens on a
//! worker thread, never on the audio thread, and the track can be played while it is
//! being decoded ([`LoadingTrack`]). [`DecodeStream`] decodes chunk by chunk for work
//! that only scans the audio once (library analysis).

mod audio_id;
mod resident;
mod tag_writing;
mod tags;
mod track;

pub use audio_id::audio_id;
pub use tag_writing::{is_tag_copy, write_genre, write_tempo_and_key, Replacing, TagFields, TempoAndKey};
pub use tags::{read_tags, CoverArt, TrackTags};

use std::fs::File;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CodecParameters, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("cannot open {path}: {source}")]
    Open { path: String, source: std::io::Error },
    #[error("unsupported or corrupt file: {0}")]
    Format(#[from] SymphoniaError),
    #[error("the file has no audio track")]
    NoAudio,
    #[error("the audio track has no sample rate")]
    NoSampleRate,
    #[error("cannot read the tags of {path}: {message}")]
    Tags { path: String, message: String },
    #[error("cannot write the tags of {path}: {message}")]
    WriteTags { path: String, message: String },
}

impl MediaError {
    /// Whether the file could not be read at that moment (its drive gone, the file locked),
    /// rather than read and found to be no audio this crate can decode.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Open { .. } => true,
            // the system's, not one symphonia makes of a stream cut short or damaged
            Self::Format(SymphoniaError::IoError(e)) => e.raw_os_error().is_some(),
            _ => false,
        }
    }
}

pub use track::{LoadingTrack, TrackAudio, TrackView, PENDING_BLOCK};

/// Extensions of the files [`decode_file`] can read.
pub const SUPPORTED_EXTENSIONS: &[&str] =
    &["mp3", "flac", "wav", "aif", "aiff", "m4a", "aac", "mp4", "ogg", "oga"];

/// Whether `path` has the extension of a file this crate can decode.
pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| SUPPORTED_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Decodes an audio file (MP3, AAC/M4A, FLAC, ALAC, OGG/Vorbis, WAV, AIFF) into memory.
/// Mono files are duplicated on both channels; extra channels are dropped.
pub fn decode_file(path: &Path) -> Result<TrackAudio, MediaError> {
    let mut stream = DecodeStream::open(path)?;
    let mut samples = Vec::with_capacity(stream.estimated_frames() * 2);
    while stream.next_chunk(&mut samples)? {}
    if samples.is_empty() {
        return Err(MediaError::NoAudio);
    }
    Ok(TrackAudio::from_interleaved(stream.sample_rate(), samples))
}

/// A file being decoded chunk by chunk, for work that does not need the whole track
/// in memory (analysing a library).
pub struct DecodeStream {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    sample_rate: u32,
    estimated_frames: usize,
    buffer: Option<SampleBuffer<f32>>,
}

/// More frames than any codec gets out of a byte (16 kb/s Opus gives about 22).
const MAX_FRAMES_PER_BYTE: u64 = 64;

/// The most frames a file of `file_bytes` can hold: uncompressed audio takes a byte or more
/// per sample.
fn max_frames(params: &CodecParameters, file_bytes: u64) -> u64 {
    let uncompressed = symphonia::default::get_codecs()
        .get_codec(params.codec)
        .is_some_and(|codec| codec.short_name.starts_with("pcm_"));
    if uncompressed {
        return file_bytes / params.channels.map_or(1, |c| c.count().max(1)) as u64;
    }
    file_bytes.saturating_mul(MAX_FRAMES_PER_BYTE)
}

/// The container of `path`, ready to give its packets, and the size of the file.
fn open_format(path: &Path) -> Result<(Box<dyn FormatReader>, u64), MediaError> {
    let file =
        File::open(path).map_err(|source| MediaError::Open { path: path.display().to_string(), source })?;
    let file_bytes = file.metadata().map_or(u64::MAX, |m| m.len());
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe().format(
        &hint,
        stream,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;
    Ok((probed.format, file_bytes))
}

impl DecodeStream {
    pub fn open(path: &Path) -> Result<Self, MediaError> {
        let (format, file_bytes) = open_format(path)?;
        let track = format.default_track().ok_or(MediaError::NoAudio)?;
        let decoder =
            symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
        Ok(Self {
            track_id: track.id,
            sample_rate: track.codec_params.sample_rate.ok_or(MediaError::NoSampleRate)?,
            // a damaged header, or the placeholder of a file written as a stream, announcing
            // more than its file can hold is not believed
            estimated_frames: track
                .codec_params
                .n_frames
                .filter(|&n| n <= max_frames(&track.codec_params, file_bytes))
                .unwrap_or(0) as usize,
            format,
            decoder,
            buffer: None,
        })
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Frame count announced by the file header (0 if unknown).
    pub fn estimated_frames(&self) -> usize {
        self.estimated_frames
    }

    /// Appends the next packet as interleaved stereo to `out`. `false` at the end.
    pub fn next_chunk(&mut self, out: &mut Vec<f32>) -> Result<bool, MediaError> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                // end of stream
                Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return Ok(false)
                }
                Err(SymphoniaError::ResetRequired) => return Ok(false),
                Err(e) => return Err(e.into()),
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            let decoded = match self.decoder.decode(&packet) {
                Ok(decoded) => decoded,
                // a damaged frame: skip it, like every player does
                Err(SymphoniaError::DecodeError(_)) => continue,
                Err(e) => return Err(e.into()),
            };
            let spec = *decoded.spec();
            let channels = spec.channels.count();
            let buffer =
                self.buffer.get_or_insert_with(|| SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
            if buffer.capacity() < decoded.capacity() * channels {
                *buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
            }
            buffer.copy_interleaved_ref(decoded);
            append_stereo(out, buffer.samples(), channels);
            return Ok(true);
        }
    }
}

fn append_stereo(out: &mut Vec<f32>, interleaved: &[f32], channels: usize) {
    match channels {
        0 => {}
        1 => out.extend(interleaved.iter().flat_map(|&v| [v, v])),
        2 => out.extend_from_slice(interleaved),
        n => out.extend(interleaved.chunks_exact(n).flat_map(|f| [f[0], f[1]])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mono_is_duplicated_and_extra_channels_dropped() {
        let mut out = Vec::new();
        append_stereo(&mut out, &[0.1, 0.2], 1);
        assert_eq!(out, vec![0.1, 0.1, 0.2, 0.2]);
        out.clear();
        append_stereo(&mut out, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 3);
        assert_eq!(out, vec![1.0, 2.0, 4.0, 5.0]);
    }

    /// A 16-bit stereo WAV of `frames` frames whose header announces `announced` bytes of data.
    fn wav(name: &str, frames: u32, announced: u32) -> std::path::PathBuf {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&announced.saturating_add(36).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        for v in [1u16, 2] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        bytes.extend_from_slice(&44_100u32.to_le_bytes());
        bytes.extend_from_slice(&(44_100u32 * 4).to_le_bytes());
        for v in [4u16, 16] {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&announced.to_le_bytes());
        bytes.extend((0..frames * 2).flat_map(|i| ((i % 100) as i16 * 100).to_le_bytes()));
        let path = std::env::temp_dir().join(format!("prisme-media-{}-{name}.wav", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn a_header_announcing_more_than_its_file_holds_is_not_believed() {
        let honest = wav("honest", 1000, 4000);
        assert_eq!(DecodeStream::open(&honest).unwrap().estimated_frames(), 1000);
        let lying = wav("lying", 1000, 0x7fff_0000);
        let stream = DecodeStream::open(&lying);
        let decoded = decode_file(&lying);
        let _ = (std::fs::remove_file(&honest), std::fs::remove_file(&lying));
        assert_eq!(stream.unwrap().estimated_frames(), 0);
        assert_eq!(decoded.unwrap().frames(), 1000);
    }

    #[test]
    fn a_read_failed_by_the_system_is_transient_and_a_damaged_stream_not() {
        let failed = std::io::Error::from_raw_os_error(21);
        let damaged = std::io::Error::other("unexpected end of bitstream");
        assert!(MediaError::Format(SymphoniaError::IoError(failed)).is_transient());
        assert!(!MediaError::Format(SymphoniaError::IoError(damaged)).is_transient());
    }

    #[test]
    fn an_uncompressed_file_is_not_believed_beyond_a_byte_a_sample() {
        let lying = wav("overstated", 1000, 1000 * 4 * 16);
        let stream = DecodeStream::open(&lying);
        let loaded = LoadingTrack::open(&lying);
        let _ = std::fs::remove_file(&lying);
        assert_eq!(stream.unwrap().estimated_frames(), 0);
        assert_eq!(loaded.unwrap().audio().frames(), 1000);
    }

    #[test]
    fn a_track_no_longer_wanted_ends_where_its_decoding_was_left() {
        let path = wav("given-up", 88_200, 88_200 * 4);
        let loading = LoadingTrack::open(&path).unwrap();
        let (audio, result) = loading.decode_rest_while(|| false);
        let _ = std::fs::remove_file(&path);
        assert!(result.is_ok());
        assert!(audio.is_complete());
        assert!(audio.frames() < 88_200, "{} frames", audio.frames());
    }

    #[test]
    fn mp3s_joined_are_decoded_whole_though_the_first_header_counts_its_own_frames() {
        // the frames after the first's ID3 tag and Info frame, as a file joined to it brings
        let one = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/silence.mp3")).unwrap();
        let mut joined = one.clone();
        for _ in 0..12 {
            joined.extend_from_slice(&one[45 + 182..]);
        }
        let path = std::env::temp_dir().join(format!("prisme-media-{}-joined.mp3", std::process::id()));
        std::fs::write(&path, joined).unwrap();
        let loading = LoadingTrack::open(&path).unwrap();
        let announced = DecodeStream::open(&path).unwrap().estimated_frames();
        let whole = decode_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        let (audio, result) = loading.decode_rest_while(|| true);
        assert!(result.is_ok());
        assert!(whole.frames() > announced + 2 * 44_100, "{} frames, {announced} announced", whole.frames());
        assert_eq!(audio.frames(), whole.frames());
    }

    #[test]
    fn a_header_with_no_audio_after_it_is_no_track() {
        let path = wav("hollow", 0, 80);
        let loaded = LoadingTrack::open(&path);
        let _ = std::fs::remove_file(&path);
        assert!(matches!(loaded, Err(MediaError::NoAudio)), "{:?}", loaded.map(|t| t.audio().frames()));
    }

    #[test]
    fn an_aiff_file_decodes() {
        let frames = 1000u32;
        let mut comm = Vec::new();
        comm.extend_from_slice(&2u16.to_be_bytes());
        comm.extend_from_slice(&frames.to_be_bytes());
        comm.extend_from_slice(&16u16.to_be_bytes());
        // 44 100 as an 80-bit extended float
        comm.extend_from_slice(&[0x40, 0x0e, 0xac, 0x44, 0, 0, 0, 0, 0, 0]);
        let mut ssnd = vec![0u8; 8];
        ssnd.extend((0..frames * 2).flat_map(|i| ((i % 100) as i16 * 100).to_be_bytes()));
        let mut body = b"AIFF".to_vec();
        for (id, chunk) in [(b"COMM", &comm), (b"SSND", &ssnd)] {
            body.extend_from_slice(id);
            body.extend_from_slice(&(chunk.len() as u32).to_be_bytes());
            body.extend_from_slice(chunk);
        }
        let mut bytes = b"FORM".to_vec();
        bytes.extend_from_slice(&(body.len() as u32).to_be_bytes());
        bytes.extend(body);
        let path = std::env::temp_dir().join(format!("prisme-media-{}.aiff", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let decoded = decode_file(&path);
        let _ = std::fs::remove_file(&path);
        let track = decoded.unwrap();
        assert!(is_supported(&path));
        assert_eq!((track.sample_rate(), track.frames()), (44_100, 1000));
        // frame 3 is samples 6 and 7
        assert!((track.frame(3).0 - 600.0 / 32768.0).abs() < 1e-6);
    }

    #[test]
    fn supported_files_are_recognised_by_extension() {
        assert!(is_supported(Path::new("a/b/Track.MP3")));
        assert!(is_supported(Path::new("x.flac")));
        assert!(!is_supported(Path::new("cover.jpg")));
        assert!(!is_supported(Path::new("noext")));
    }
}
