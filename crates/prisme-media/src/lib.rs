//! Audio file decoding.
//!
//! A track is decoded entirely into memory as interleaved stereo `f32` ([`TrackAudio`]).
//! For DVS this is what we want: scratching and needle drops need instant random access
//! anywhere in the track, which streaming decoders cannot give. Decoding happens on a
//! worker thread, never on the audio thread, and the track can be played while it is
//! being decoded ([`LoadingTrack`]). [`DecodeStream`] decodes chunk by chunk for work
//! that only scans the audio once (library analysis).

mod tags;
mod track;

pub use tags::{read_tags, CoverArt, TrackTags};

use std::fs::File;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
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
}

pub use track::{LoadingTrack, TrackAudio, TrackView};

/// Extensions of the files [`decode_file`] can read.
pub const SUPPORTED_EXTENSIONS: &[&str] = &["mp3", "flac", "wav", "m4a", "aac", "mp4", "ogg", "oga"];

/// Whether `path` has the extension of a file this crate can decode.
pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| SUPPORTED_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Decodes an audio file (MP3, AAC/M4A, FLAC, ALAC, OGG/Vorbis, WAV) into memory.
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

impl DecodeStream {
    pub fn open(path: &Path) -> Result<Self, MediaError> {
        let file = File::open(path)
            .map_err(|source| MediaError::Open { path: path.display().to_string(), source })?;
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
        let format = probed.format;
        let track = format.default_track().ok_or(MediaError::NoAudio)?;
        let decoder =
            symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;
        Ok(Self {
            track_id: track.id,
            sample_rate: track.codec_params.sample_rate.ok_or(MediaError::NoSampleRate)?,
            estimated_frames: track.codec_params.n_frames.unwrap_or(0) as usize,
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

    #[test]
    fn supported_files_are_recognised_by_extension() {
        assert!(is_supported(Path::new("a/b/Track.MP3")));
        assert!(is_supported(Path::new("x.flac")));
        assert!(!is_supported(Path::new("cover.jpg")));
        assert!(!is_supported(Path::new("noext")));
    }
}
