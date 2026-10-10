//! Audio file decoding.
//!
//! A track is decoded entirely into memory as interleaved stereo `f32` ([`TrackAudio`]).
//! For DVS this is what we want: scratching and needle drops need instant random access
//! anywhere in the track, which streaming decoders cannot give. Decoding happens on a
//! worker thread, never on the audio thread.

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
}

/// A fully decoded track: interleaved stereo samples at the file's sample rate.
#[derive(Debug, Clone)]
pub struct TrackAudio {
    sample_rate: u32,
    samples: Vec<f32>,
}

impl TrackAudio {
    /// Builds a track from interleaved stereo samples.
    pub fn from_interleaved(sample_rate: u32, samples: Vec<f32>) -> Self {
        assert!(samples.len() % 2 == 0, "stereo data must have an even number of samples");
        Self { sample_rate, samples }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Number of stereo frames.
    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }

    pub fn duration_s(&self) -> f64 {
        self.frames() as f64 / self.sample_rate as f64
    }

    /// Interleaved stereo samples (left, right, left, right, ...).
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    /// Frame `i` as (left, right), or silence outside the track.
    #[inline]
    pub fn frame(&self, i: isize) -> (f32, f32) {
        if i < 0 || i as usize >= self.frames() {
            (0.0, 0.0)
        } else {
            let k = i as usize * 2;
            (self.samples[k], self.samples[k + 1])
        }
    }
}

/// Decodes an audio file (MP3, AAC/M4A, FLAC, ALAC, OGG/Vorbis, WAV) into memory.
/// Mono files are duplicated on both channels; extra channels are dropped.
pub fn decode_file(path: &Path) -> Result<TrackAudio, MediaError> {
    let file = File::open(path).map_err(|source| MediaError::Open { path: path.display().to_string(), source })?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed =
        symphonia::default::get_probe().format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default())?;
    let mut format = probed.format;
    let track = format.default_track().ok_or(MediaError::NoAudio)?;
    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.ok_or(MediaError::NoSampleRate)?;
    let estimated = track.codec_params.n_frames.unwrap_or(0) as usize * 2;
    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

    let mut samples = Vec::with_capacity(estimated);
    let mut buffer: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            // end of stream
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // a damaged frame: skip it, like every player does
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count();
        let buffer = buffer.get_or_insert_with(|| SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
        if buffer.capacity() < decoded.capacity() * channels {
            *buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        }
        buffer.copy_interleaved_ref(decoded);
        append_stereo(&mut samples, buffer.samples(), channels);
    }
    if samples.is_empty() {
        return Err(MediaError::NoAudio);
    }
    Ok(TrackAudio { sample_rate, samples })
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
    fn frames_outside_the_track_are_silent() {
        let t = TrackAudio::from_interleaved(44_100, vec![0.5, -0.5]);
        assert_eq!(t.frame(0), (0.5, -0.5));
        assert_eq!(t.frame(-1), (0.0, 0.0));
        assert_eq!(t.frame(1), (0.0, 0.0));
    }
}
