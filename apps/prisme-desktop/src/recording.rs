//! Recording the mix to a WAV file (24-bit), with its tracklist from the history of the
//! set, as text and as a CUE sheet next to it.
//!
//! The engine queues what it records; a writer thread empties the queue to the file and
//! stops when asked or when the engine is gone (audio restarted). The file's header is kept
//! up to date, so that a crash loses a second at most; a set longer than a WAV file holds
//! (4 GB: 4 hours at 48 kHz) goes on in `… 2.wav`, `… 3.wav`.

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use hound::{SampleFormat, WavSpec, WavWriter};
use prisme_engine::{Command, RecordSource};
use prisme_library::{HistoryPlay, LibraryDb};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tracing::warn;

/// What the queue holds if the writer stalls (seconds).
const QUEUE_S: usize = 4;
const IDLE: Duration = Duration::from_millis(20);
const I24_MAX: f32 = 8_388_607.0;
/// `2026-10-11 21h30m05`: sorts by date, valid in a file name.
const FILE_TIME: &str = "%Y-%m-%d %Hh%Mm%S";
/// The audio a WAV file takes before its 32-bit sizes overflow, less room for the header.
const MAX_FILE_BYTES: u64 = 4_000_000_000;
/// How often the header is brought up to date.
const FLUSH_EVERY: Duration = Duration::from_secs(1);

pub struct Recorder {
    /// The first file: the others are named after it.
    path: PathBuf,
    source: RecordSource,
    started_at_ms: i64,
    stop: Arc<AtomicBool>,
    /// The writer of the current audio session.
    writer: Option<JoinHandle<Result<f64, hound::Error>>>,
    /// Seconds the writers of earlier audio sessions wrote.
    earlier_s: f64,
    /// The last file written in, by its number (1: the first).
    part: Arc<AtomicUsize>,
    /// Where the plays of the tracklist are read.
    db: Arc<LibraryDb>,
}

/// What a finished recording holds.
pub struct RecordingSummary {
    pub path: PathBuf,
    pub seconds: f64,
    pub tracks: usize,
}

impl Recorder {
    /// Starts recording `source` into a new file in `folder`, named after the local time.
    pub fn start(
        state: &AppState,
        source: RecordSource,
        folder: &Path,
        db: Arc<LibraryDb>,
    ) -> AppResult<Self> {
        std::fs::create_dir_all(folder)?;
        let path = folder.join(format!("Prisme {}.wav", chrono::Local::now().format(FILE_TIME)));
        let (stop, part) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicUsize::new(1)));
        let writer = begin(state, source, &path, &part, &stop)?;
        let started_at_ms = prisme_library::now_ms();
        Ok(Self { path, source, started_at_ms, stop, writer: Some(writer), earlier_s: 0.0, part, db })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Goes on recording from a new audio session, the old one gone with its engine: in the
    /// next file, at the new session's rate.
    pub fn resume(&mut self, state: &AppState) -> AppResult<()> {
        if let Some(ended) = self.writer.take() {
            // the old engine is gone: its writer ends once it has written what it queued
            match ended.join() {
                Ok(Ok(seconds)) => self.earlier_s += seconds,
                Ok(Err(e)) => warn!("recording cut short: {e}"),
                Err(_) => warn!("the recording thread failed"),
            }
        }
        self.part.fetch_add(1, Ordering::AcqRel);
        match begin(state, self.source, &self.path, &self.part, &self.stop) {
            Ok(writer) => {
                self.writer = Some(writer);
                Ok(())
            }
            Err(e) => {
                self.part.fetch_sub(1, Ordering::AcqRel);
                Err(e)
            }
        }
    }

    /// Ends the file and writes the tracklist of what was played since the start.
    pub fn stop(self, state: &AppState) -> AppResult<RecordingSummary> {
        match state.send(Command::StopRecording) {
            Ok(()) | Err(AppError::AudioNotRunning) => {}
            Err(e) => return Err(e),
        }
        self.stop.store(true, Ordering::Release);
        let last_s = match self.writer {
            Some(writer) => writer
                .join()
                .map_err(|_| AppError::Other("the recording thread failed".into()))?
                .map_err(|e| AppError::Other(e.to_string()))?,
            None => 0.0,
        };
        let plays = self.db.plays_since(self.started_at_ms)?;
        let file_name = self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        std::fs::write(self.path.with_extension("txt"), tracklist_text(&plays, self.started_at_ms))?;
        // a sheet holds the tracks of one file
        if self.part.load(Ordering::Acquire) == 1 {
            std::fs::write(
                self.path.with_extension("cue"),
                cue_sheet(&plays, self.started_at_ms, &file_name),
            )?;
        }
        Ok(RecordingSummary { seconds: self.earlier_s + last_s, tracks: plays.len(), path: self.path })
    }
}

/// Has the engine queue `source` to a writer of the recording at `first`, from file `part`.
fn begin(
    state: &AppState,
    source: RecordSource,
    first: &Path,
    part: &Arc<AtomicUsize>,
    stop: &Arc<AtomicBool>,
) -> AppResult<JoinHandle<Result<f64, hound::Error>>> {
    let sample_rate = state.sample_rate().ok_or(AppError::AudioNotRunning)?;
    let spec = WavSpec { channels: 2, sample_rate, bits_per_sample: 24, sample_format: SampleFormat::Int };
    let path = part_path(first, part.load(Ordering::Acquire));
    let wav = WavWriter::create(&path, spec).map_err(|e| AppError::Other(e.to_string()))?;
    let (sink, queue) = rtrb::RingBuffer::new(sample_rate as usize * 2 * QUEUE_S);
    if let Err(e) = state.send(Command::StartRecording { source, sink }) {
        drop(wav);
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    let (stop, part, first) = (stop.clone(), part.clone(), first.to_owned());
    Ok(std::thread::Builder::new()
        .name("prisme-recording".into())
        .spawn(move || write(queue, wav, &stop, &first, &part, spec, MAX_FILE_BYTES))
        .expect("cannot start the recording thread"))
}

/// Writes what the engine queues to `wav` (file `part` of the recording at `first`) until
/// stopped or the engine is gone: the header up to date every second, the next part in a
/// file of its own past `max_bytes` of audio. The seconds written.
fn write(
    mut queue: rtrb::Consumer<f32>,
    mut wav: WavWriter<std::io::BufWriter<std::fs::File>>,
    stop: &AtomicBool,
    first: &Path,
    part: &AtomicUsize,
    spec: WavSpec,
    max_bytes: u64,
) -> Result<f64, hound::Error> {
    let channels = u64::from(spec.channels);
    let samples_per_file = max_bytes / u64::from(spec.bits_per_sample / 8) / channels * channels;
    let (mut samples, mut in_file) = (0u64, 0u64);
    let mut flushed = Instant::now();
    loop {
        let available = queue.slots();
        if available == 0 {
            if stop.load(Ordering::Acquire) || queue.is_abandoned() {
                break;
            }
            std::thread::sleep(IDLE);
            continue;
        }
        let chunk = queue.read_chunk(available).expect("slots were counted");
        // the chunk's iterator frees the slots as it reads them
        for sample in chunk {
            // a file ends on a whole frame
            if in_file == samples_per_file {
                let next = part.fetch_add(1, Ordering::AcqRel) + 1;
                let next = WavWriter::create(part_path(first, next), spec)?;
                std::mem::replace(&mut wav, next).finalize()?;
                in_file = 0;
            }
            wav.write_sample((sample.clamp(-1.0, 1.0) * I24_MAX).round() as i32)?;
            in_file += 1;
        }
        samples += available as u64;
        if flushed.elapsed() >= FLUSH_EVERY {
            wav.flush()?;
            flushed = Instant::now();
        }
    }
    wav.finalize()?;
    Ok((samples / channels) as f64 / f64::from(spec.sample_rate))
}

/// The file of part `part` of the recording at `first`: `first` itself, then `… 2.wav`.
fn part_path(first: &Path, part: usize) -> PathBuf {
    if part == 1 {
        return first.to_owned();
    }
    let stem = first.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    first.with_file_name(format!("{stem} {part}.wav"))
}

/// "Artist - Title", or the file name without its extension.
fn track_name(play: &HistoryPlay) -> (Option<String>, String) {
    let tags = play.record.as_ref().map(|r| &r.tags);
    let title = tags.and_then(|t| t.title.clone()).unwrap_or_else(|| {
        Path::new(&play.path)
            .file_stem()
            .map_or_else(|| play.path.clone(), |s| s.to_string_lossy().into_owned())
    });
    (tags.and_then(|t| t.artist.clone()), title)
}

fn offset_ms(play: &HistoryPlay, started_at_ms: i64) -> i64 {
    (play.played_at_ms - started_at_ms).max(0)
}

/// One line per track: `h:mm:ss Artist - Title`.
fn tracklist_text(plays: &[HistoryPlay], started_at_ms: i64) -> String {
    let mut text = String::new();
    for play in plays {
        let s = offset_ms(play, started_at_ms) / 1000;
        let (artist, title) = track_name(play);
        let name = match artist {
            Some(artist) => format!("{artist} - {title}"),
            None => title,
        };
        let _ = writeln!(text, "{}:{:02}:{:02} {name}", s / 3600, s / 60 % 60, s % 60);
    }
    text
}

/// A CUE sheet: players split the recording into its tracks with it.
fn cue_sheet(plays: &[HistoryPlay], started_at_ms: i64, file_name: &str) -> String {
    let quoted = |s: &str| s.replace('"', "'");
    let mut cue = format!(
        "TITLE \"{}\"\nFILE \"{}\" WAVE\n",
        quoted(file_name.trim_end_matches(".wav")),
        quoted(file_name)
    );
    for (i, play) in plays.iter().enumerate() {
        let (artist, title) = track_name(play);
        // CUE positions are minutes:seconds:frames, 75 frames a second
        let frames = offset_ms(play, started_at_ms) * 75 / 1000;
        let _ = write!(cue, "  TRACK {:02} AUDIO\n    TITLE \"{}\"\n", i + 1, quoted(&title));
        if let Some(artist) = artist {
            let _ = writeln!(cue, "    PERFORMER \"{}\"", quoted(&artist));
        }
        let _ =
            writeln!(cue, "    INDEX 01 {:02}:{:02}:{:02}", frames / 75 / 60, frames / 75 % 60, frames % 75);
    }
    cue
}

#[cfg(test)]
mod tests {
    use super::*;
    use prisme_library::{FileStamp, TrackRecord};
    use prisme_media::TrackTags;

    fn plays() -> Vec<HistoryPlay> {
        let tags = TrackTags {
            title: Some("Strobe".into()),
            artist: Some("deadmau5".into()),
            ..TrackTags::default()
        };
        let analysis = prisme_analysis::TrackAnalysis::default();
        let record = TrackRecord::analyzed(
            Path::new("C:/Music/s.mp3"),
            FileStamp { size: 1, modified_ms: 1 },
            1.0,
            analysis,
            tags,
        );
        vec![
            HistoryPlay { path: record.path.clone(), deck: 0, played_at_ms: 1_000, record: Some(record) },
            HistoryPlay {
                path: "C:/Music/Untitled \"live\".mp3".into(),
                deck: 1,
                played_at_ms: 3_726_500,
                record: None,
            },
        ]
    }

    #[test]
    fn a_recording_goes_on_in_a_new_file_when_one_is_full() {
        let dir = std::env::temp_dir().join(format!("prisme-recording-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = dir.join("set.wav");
        let spec = WavSpec {
            channels: 2,
            sample_rate: 48_000,
            bits_per_sample: 24,
            sample_format: SampleFormat::Int,
        };
        let (mut sink, queue) = rtrb::RingBuffer::new(64);
        for i in 0..20 {
            sink.push(i as f32 / 100.0).unwrap();
        }
        drop(sink);
        let wav = WavWriter::create(&first, spec).unwrap();
        // 4 frames of 6 bytes a file
        let part = AtomicUsize::new(1);
        let seconds = write(queue, wav, &AtomicBool::new(true), &first, &part, spec, 24).unwrap();
        assert_eq!(part.load(Ordering::Acquire), 3);
        assert!((seconds - 10.0 / 48_000.0).abs() < 1e-12);
        let frames: Vec<u32> = [first.clone(), dir.join("set 2.wav"), dir.join("set 3.wav")]
            .iter()
            .map(|p| hound::WavReader::open(p).unwrap().duration())
            .collect();
        assert_eq!(frames, [4, 4, 2]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn tracklist_from_the_start_of_the_recording() {
        assert_eq!(tracklist_text(&plays(), 0), "0:00:01 deadmau5 - Strobe\n1:02:06 Untitled \"live\"\n");
    }

    #[test]
    fn cue_sheet() {
        let cue = super::cue_sheet(&plays(), 0, "Prisme set.wav");
        assert_eq!(
            cue,
            "TITLE \"Prisme set\"\nFILE \"Prisme set.wav\" WAVE\n  TRACK 01 AUDIO\n    TITLE \"Strobe\"\n    PERFORMER \"deadmau5\"\n    INDEX 01 00:01:00\n  TRACK 02 AUDIO\n    TITLE \"Untitled 'live'\"\n    INDEX 01 62:06:37\n"
        );
    }
}
