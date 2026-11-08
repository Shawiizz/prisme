//! Separated vocals stored next to their track, in a `.prisme` folder: copying a music folder
//! to another computer brings its separations along.
//!
//! The vocals are a 16-bit FLAC at the track's own rate and length, at half their level (a
//! track mastered over full scale leaves vocals above it too). The instrumental is never
//! stored: it is the track minus its vocals.

use std::path::{Path, PathBuf};

use prisme_media::{DecodeStream, TrackAudio};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

use crate::network::MaskNetwork;
use crate::separate::Separator;
use crate::StemsError;

/// The folder holding the separations of the tracks beside it.
pub const FOLDER: &str = ".prisme";
const STORED_GAIN: f32 = 0.5;

/// Where the vocals of `track` are stored.
pub fn vocals_path(track: &Path) -> Option<PathBuf> {
    let name = track.file_name()?.to_string_lossy();
    Some(track.parent()?.join(FOLDER).join(format!("{name}.vocals.flac")))
}

/// Whether `track` has stored vocals (not checking that they still match it).
pub fn has_vocals(track: &Path) -> bool {
    vocals_path(track).is_some_and(|p| p.is_file())
}

/// Separates the vocals of `track` and stores them next to it.
pub fn separate_file<N: MaskNetwork>(
    separator: &mut Separator<N>,
    track: &Path,
    progress: impl FnMut(usize, usize) -> bool,
) -> Result<PathBuf, StemsError> {
    let target =
        vocals_path(track).ok_or_else(|| StemsError::Track(format!("{} is not a file", track.display())))?;
    let (rate, samples) = decode(track)?;
    let frames = samples.len() / 2;
    let model_rate = separator.sample_rate();
    let input = resample(&samples, rate, model_rate)?;
    let (left, right) = deinterleave(&input);
    let [l, r] = separator.vocals(&left, &right, progress)?;
    let vocals: Vec<f32> = l.iter().zip(&r).flat_map(|(a, b)| [*a, *b]).collect();
    let mut vocals = resample(&vocals, model_rate, rate)?;
    vocals.resize(frames * 2, 0.0);

    let flac = encode(&vocals, rate)?;
    let folder = target.parent().expect("the vocals path has a folder");
    std::fs::create_dir_all(folder)?;
    let partial = target.with_extension("flac.part");
    std::fs::write(&partial, flac)?;
    std::fs::rename(&partial, &target)?;
    Ok(target)
}

/// The stored vocals of a track whose decoded audio has `frames` frames at `sample_rate`:
/// `None` when there are none, or when they were made from a different version of the file.
pub fn load_vocals(track: &Path, frames: usize, sample_rate: u32) -> Result<Option<TrackAudio>, StemsError> {
    let Some(path) = vocals_path(track).filter(|p| p.is_file()) else { return Ok(None) };
    let (rate, mut samples) = decode(&path)?;
    if rate != sample_rate || samples.len() != frames * 2 {
        return Ok(None);
    }
    samples.iter_mut().for_each(|s| *s /= STORED_GAIN);
    Ok(Some(TrackAudio::from_interleaved(rate, samples)))
}

fn decode(path: &Path) -> Result<(u32, Vec<f32>), StemsError> {
    let media = |e: prisme_media::MediaError| StemsError::Track(e.to_string());
    let mut stream = DecodeStream::open(path).map_err(media)?;
    let mut samples = Vec::with_capacity(stream.estimated_frames() * 2);
    while stream.next_chunk(&mut samples).map_err(media)? {}
    Ok((stream.sample_rate(), samples))
}

/// Interleaved stereo from one rate to another, aligned (the resampler's delay removed).
fn resample(samples: &[f32], from: u32, to: u32) -> Result<Vec<f32>, StemsError> {
    if from == to {
        return Ok(samples.to_vec());
    }
    let error = |e: &dyn std::fmt::Display| StemsError::Track(format!("resampling: {e}"));
    let frames = samples.len() / 2;
    let mut resampler =
        Fft::<f32>::new(from as usize, to as usize, 1024, 2, FixedSync::Both).map_err(|e| error(&e))?;
    let input = InterleavedSlice::new(samples, 2, frames).map_err(|e| error(&e))?;
    let output = resampler.process_all(&input, frames, None).map_err(|e| error(&e))?;
    Ok(output.take_data())
}

fn deinterleave(samples: &[f32]) -> (Vec<f32>, Vec<f32>) {
    samples.chunks_exact(2).map(|f| (f[0], f[1])).unzip()
}

fn encode(samples: &[f32], sample_rate: u32) -> Result<Vec<u8>, StemsError> {
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;

    let error = |e: &dyn std::fmt::Debug| StemsError::Track(format!("FLAC encoding: {e:?}"));
    let ints: Vec<i32> =
        samples.iter().map(|s| (s * STORED_GAIN * 32768.0).round().clamp(-32768.0, 32767.0) as i32).collect();
    let config = flacenc::config::Encoder::default().into_verified().map_err(|e| error(&e))?;
    let source = flacenc::source::MemSource::from_samples(&ints, 2, 16, sample_rate as usize);
    let stream =
        flacenc::encode_with_fixed_block_size(&config, source, config.block_size).map_err(|e| error(&e))?;
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream.write(&mut sink).map_err(|e| error(&e))?;
    let mut bytes = sink.as_slice().to_vec();
    // The encoder counts the shorter last block in STREAMINFO's minimum block size, which the
    // specification excludes; decoders (symphonia) then take the stream for a variable-block
    // one and reject its fixed-block frames. Minimum = maximum, as the blocks are fixed.
    if bytes.len() >= 12 && &bytes[..4] == b"fLaC" {
        bytes.copy_within(10..12, 8);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vocals_live_in_a_folder_beside_the_track() {
        let path = vocals_path(Path::new("C:/Music/House/a b.mp3")).unwrap();
        assert_eq!(path, Path::new("C:/Music/House/.prisme/a b.mp3.vocals.flac"));
    }

    #[test]
    fn resampling_there_and_back_keeps_the_signal_in_place() {
        let frames = 48_000;
        let tone: Vec<f32> = (0..frames)
            .flat_map(|i| {
                let v = (i as f32 * 2.0 * std::f32::consts::PI * 440.0 / 48_000.0).sin() * 0.5;
                [v, -v]
            })
            .collect();
        let down = resample(&tone, 48_000, 44_100).unwrap();
        let mut back = resample(&down, 44_100, 48_000).unwrap();
        back.resize(tone.len(), 0.0);
        let middle = 2000..tone.len() - 2000;
        let err =
            tone[middle.clone()].iter().zip(&back[middle]).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err < 1e-3, "max error {err}");
    }
}
