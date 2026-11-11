//! Separated stems stored next to their track, in a `.prisme` folder: copying a music folder
//! to another computer brings its separations along.
//!
//! Each stem is a 16-bit FLAC at the track's own rate and length, at half its level (a track
//! mastered over full scale leaves stems above it too). What no stem holds is never stored:
//! it is the track minus its stems.

use std::ops::Range;
use std::path::{Path, PathBuf};

use prisme_media::{DecodeStream, TrackAudio};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

use crate::network::MaskNetwork;
use crate::separate::{Separation, Separator};
use crate::StemsError;

/// The folder holding the separations of the tracks beside it.
pub const FOLDER: &str = ".prisme";
const STORED_GAIN: f32 = 0.5;

/// Where `stem` of `track` is stored.
pub fn stem_path(track: &Path, stem: &str) -> Option<PathBuf> {
    let name = track.file_name()?.to_string_lossy();
    Some(track.parent()?.join(FOLDER).join(format!("{name}.{stem}.flac")))
}

/// Whether `track` has `stem` stored (not checking that it still matches the track).
pub fn has_stem(track: &Path, stem: &str) -> bool {
    stem_path(track, stem).is_some_and(|p| p.is_file())
}

/// Separates `track` with each separator in turn and stores every stem next to it.
///
/// `progress(done, total)` counts the chunks of all the separators: returning `false` cancels,
/// keeping the stems of the separators already through.
pub fn separate_file<N: MaskNetwork>(
    separators: &mut [&mut Separator<N>],
    track: &Path,
    mut progress: impl FnMut(usize, usize) -> bool,
) -> Result<Vec<PathBuf>, StemsError> {
    let mut separation = TrackSeparation::open(separators, track, None, None)?;
    let mut stored = Vec::new();
    while !separation.is_done() {
        separation.step(separators)?;
        stored.extend(separation.store_through()?);
        if !progress(separation.done(), separation.chunks()) {
            return Err(StemsError::Cancelled);
        }
    }
    Ok(stored)
}

/// A track being separated by several separators, whose stems can be read as they come.
///
/// Played, the track is separated around a frame: every separator works on the chunks over
/// it first ([`Separator::begin`]), in turns that keep them abreast, so that all the stems
/// come there at once. Otherwise the separators work one after the other from the start,
/// and the stems of each are stored as soon as it is through.
pub struct TrackSeparation {
    track: PathBuf,
    sample_rate: u32,
    frames: usize,
    /// The track at each separator's rate, left and right.
    inputs: Vec<(u32, [Vec<f32>; 2])>,
    runs: Vec<Run>,
    abreast: bool,
}

/// What one separator does of a [`TrackSeparation`].
struct Run {
    /// Its rate, an index into `inputs`.
    input: usize,
    sample_rate: u32,
    stems: Vec<String>,
    targets: Vec<PathBuf>,
    /// `None` once its stems are stored.
    separation: Option<Separation>,
    chunks: usize,
}

impl TrackSeparation {
    /// The separation of `track` by `separators`: `audio` is the track decoded already
    /// (its rate and interleaved stereo samples), else it is decoded here; `around` is the
    /// frame it is played from.
    pub fn open<N: MaskNetwork>(
        separators: &[&mut Separator<N>],
        track: &Path,
        audio: Option<(u32, Vec<f32>)>,
        around: Option<usize>,
    ) -> Result<Self, StemsError> {
        let (sample_rate, samples) = match audio {
            Some(audio) => audio,
            None => decode(track)?,
        };
        let frames = samples.len() / 2;
        let mut inputs: Vec<(u32, [Vec<f32>; 2])> = Vec::new();
        let mut runs = Vec::new();
        for separator in separators {
            let rate = separator.sample_rate();
            let input = match inputs.iter().position(|(r, _)| *r == rate) {
                Some(input) => input,
                None => {
                    inputs.push((rate, deinterleave(&resample(&samples, sample_rate, rate)?)));
                    inputs.len() - 1
                }
            };
            let len = inputs[input].1[0].len();
            // from far enough before the frame played for it to be resampled
            let anchor = around.map_or(0, |frame| {
                let at = (frame as u64 * u64::from(rate) / u64::from(sample_rate)) as usize;
                let margin =
                    if rate == sample_rate { 0 } else { RESAMPLING_MARGIN + aligned_step(rate, sample_rate) };
                at.saturating_sub(margin)
            });
            let separation = separator.begin(len, anchor);
            let stems = separator.stems().to_vec();
            let targets = stems
                .iter()
                .map(|stem| stem_path(track, stem))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| StemsError::Track(format!("{} is not a file", track.display())))?;
            runs.push(Run {
                input,
                sample_rate: rate,
                stems,
                targets,
                chunks: separation.chunks(),
                separation: Some(separation),
            });
        }
        Ok(Self { track: track.to_owned(), sample_rate, frames, inputs, runs, abreast: around.is_some() })
    }

    pub fn track(&self) -> &Path {
        &self.track
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn frames(&self) -> usize {
        self.frames
    }

    /// The chunks of every separator.
    pub fn chunks(&self) -> usize {
        self.runs.iter().map(|r| r.chunks).sum()
    }

    /// The chunks through.
    pub fn done(&self) -> usize {
        self.runs.iter().map(Run::done).sum()
    }

    pub fn is_done(&self) -> bool {
        self.done() == self.chunks()
    }

    /// Separates one more chunk, with the separators given to [`Self::open`].
    pub fn step<N: MaskNetwork>(&mut self, separators: &mut [&mut Separator<N>]) -> Result<(), StemsError> {
        let unfinished = |(_, run): &(usize, &Run)| run.done() < run.chunks;
        let behind = if self.abreast {
            let share = |run: &Run| run.done() as f64 / run.chunks as f64;
            self.runs.iter().enumerate().filter(unfinished).min_by(|a, b| share(a.1).total_cmp(&share(b.1)))
        } else {
            self.runs.iter().enumerate().find(unfinished)
        };
        let Some((index, _)) = behind else { return Ok(()) };
        let run = &mut self.runs[index];
        let [left, right] = &self.inputs[run.input].1;
        let separation = run.separation.as_mut().expect("an unfinished run has its separation");
        separators[index].step(separation, left, right)?;
        Ok(())
    }

    /// Each stem, in the order of the separators then of their stems, and the frames of the
    /// track where it is final.
    pub fn stems(&self) -> Vec<(&str, Range<usize>)> {
        self.runs
            .iter()
            .flat_map(|run| {
                let finished = match &run.separation {
                    Some(separation) => self.track_frames(run, separation.finished()),
                    None => 0..self.frames,
                };
                run.stems.iter().map(move |stem| (stem.as_str(), finished.clone()))
            })
            .collect()
    }

    /// Whether stem `stem` (an index into [`Self::stems`]) is stored, and to be read from
    /// its file rather than with [`Self::read`].
    pub fn is_stored(&self, stem: usize) -> bool {
        self.stem(stem).0.separation.is_none()
    }

    /// Stem `stem` (an index into [`Self::stems`]) over `frames`, final frames of the
    /// track: interleaved, at the track's rate. Not once stored.
    pub fn read(&self, stem: usize, frames: Range<usize>) -> Result<Vec<f32>, StemsError> {
        let (run, local) = self.stem(stem);
        let separation = run.separation.as_ref().expect("a stored stem is not read back");
        if run.sample_rate == self.sample_rate {
            let [l, r] = separation.read(local, frames);
            return Ok(interleave(&l, &r));
        }
        let span = input_span(&frames, run.sample_rate, self.sample_rate, self.input_len(run));
        let [l, r] = separation.read(local, span.clone());
        let resampled = resample(&interleave(&l, &r), run.sample_rate, self.sample_rate)?;
        let offset = (span.start as u64 * u64::from(self.sample_rate) / u64::from(run.sample_rate)) as usize;
        let mut out = vec![0.0; frames.len() * 2];
        let from = (frames.start - offset) * 2;
        let available = resampled.len().saturating_sub(from).min(out.len());
        out[..available].copy_from_slice(&resampled[from..from + available]);
        Ok(out)
    }

    /// Stores the stems of the separators through and not stored yet, and gives their paths.
    pub fn store_through(&mut self) -> Result<Vec<PathBuf>, StemsError> {
        let mut stored = Vec::new();
        for run in &mut self.runs {
            if !run.separation.as_ref().is_some_and(Separation::is_done) {
                continue;
            }
            let stems = run.separation.take().expect("checked above").into_stems();
            for ([l, r], target) in stems.into_iter().zip(&run.targets) {
                let mut stem = resample(&interleave(&l, &r), run.sample_rate, self.sample_rate)?;
                stem.resize(self.frames * 2, 0.0);
                store(target, &encode(&stem, self.sample_rate)?)?;
                stored.push(target.clone());
            }
        }
        Ok(stored)
    }

    /// The run of stem `stem`, and the stem's index in it.
    fn stem(&self, stem: usize) -> (&Run, usize) {
        let mut index = stem;
        for run in &self.runs {
            if index < run.stems.len() {
                return (run, index);
            }
            index -= run.stems.len();
        }
        panic!("no stem {stem}");
    }

    fn input_len(&self, run: &Run) -> usize {
        self.inputs[run.input].1[0].len()
    }

    /// The frames of the track that `finished` samples of a run make final: all of them at
    /// the track's rate, else those the resampler finds enough samples around.
    fn track_frames(&self, run: &Run, finished: Range<usize>) -> Range<usize> {
        if run.sample_rate == self.sample_rate {
            return finished;
        }
        let len = self.input_len(run);
        let (from, to) = (u64::from(run.sample_rate), u64::from(self.sample_rate));
        let margin = RESAMPLING_MARGIN + aligned_step(run.sample_rate, self.sample_rate);
        let start = if finished.start == 0 {
            0
        } else {
            ((finished.start + margin) as u64 * to).div_ceil(from) as usize
        };
        let end = if finished.end == len {
            self.frames
        } else {
            (finished.end.saturating_sub(RESAMPLING_MARGIN + 1) as u64 * to / from) as usize
        };
        start.min(end).min(self.frames)..end.min(self.frames)
    }
}

impl Run {
    fn done(&self) -> usize {
        self.separation.as_ref().map_or(self.chunks, Separation::done)
    }
}

/// Samples a resampled stretch needs on either side to come out as in the whole signal.
const RESAMPLING_MARGIN: usize = 4096;

/// The step at which the samples at `from` and the frames at `to` fall at the same time.
fn aligned_step(from: u32, to: u32) -> usize {
    let (mut a, mut b) = (from, to);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    (from / a) as usize
}

/// The samples at `from` that give `frames` at `to` once resampled: the stretch over them
/// with [`RESAMPLING_MARGIN`] either side, starting at a whole [`aligned_step`].
fn input_span(frames: &Range<usize>, from: u32, to: u32, len: usize) -> Range<usize> {
    let (rate_in, rate_out) = (u64::from(from), u64::from(to));
    let step = aligned_step(from, to);
    let start = (frames.start as u64 * rate_in / rate_out) as usize;
    let start = start.saturating_sub(RESAMPLING_MARGIN) / step * step;
    let end = (frames.end as u64 * rate_in).div_ceil(rate_out) as usize + RESAMPLING_MARGIN;
    start..end.min(len)
}

fn interleave(left: &[f32], right: &[f32]) -> Vec<f32> {
    left.iter().zip(right).flat_map(|(a, b)| [*a, *b]).collect()
}

fn store(target: &Path, bytes: &[u8]) -> Result<(), StemsError> {
    std::fs::create_dir_all(target.parent().expect("a stem path has a folder"))?;
    let partial = target.with_extension("flac.part");
    std::fs::write(&partial, bytes)?;
    std::fs::rename(&partial, target)?;
    Ok(())
}

/// The stored `stem` of a track whose decoded audio has `frames` frames at `sample_rate`:
/// `None` when there is none, or when it was made from a different version of the file.
pub fn load_stem(
    track: &Path,
    stem: &str,
    frames: usize,
    sample_rate: u32,
) -> Result<Option<TrackAudio>, StemsError> {
    let Some(path) = stem_path(track, stem).filter(|p| p.is_file()) else { return Ok(None) };
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

fn deinterleave(samples: &[f32]) -> [Vec<f32>; 2] {
    let (left, right) = samples.chunks_exact(2).map(|f| (f[0], f[1])).unzip();
    [left, right]
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
    fn the_stems_live_in_a_folder_beside_the_track() {
        let path = stem_path(Path::new("C:/Music/House/a b.mp3"), "vocals").unwrap();
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
