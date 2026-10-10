//! Recording the mix to a WAV file (24-bit), with its tracklist from the history of the
//! set, as text and as a CUE sheet next to it.
//!
//! The engine queues what it records; a writer thread empties the queue to the file and
//! stops when asked or when the engine is gone (audio restarted).

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use hound::{SampleFormat, WavSpec, WavWriter};
use prisme_engine::{Command, RecordSource};
use prisme_library::{HistoryPlay, LibraryDb};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// What the queue holds if the writer stalls (seconds).
const QUEUE_S: usize = 4;
const IDLE: Duration = Duration::from_millis(20);
const I24_MAX: f32 = 8_388_607.0;
/// `2026-10-11 21h30m05`: sorts by date, valid in a file name.
const FILE_TIME: &str = "%Y-%m-%d %Hh%Mm%S";

pub struct Recorder {
    path: PathBuf,
    sample_rate: u32,
    started_at_ms: i64,
    stop: Arc<AtomicBool>,
    writer: JoinHandle<Result<u64, hound::Error>>,
}

/// What a finished recording holds.
pub struct RecordingSummary {
    pub path: PathBuf,
    pub seconds: f64,
    pub tracks: usize,
}

impl Recorder {
    /// Starts recording `source` into a new file in `folder`, named after the local time.
    pub fn start(state: &AppState, source: RecordSource, folder: &Path) -> AppResult<Self> {
        let sample_rate = state.sample_rate().ok_or(AppError::AudioNotRunning)?;
        std::fs::create_dir_all(folder)?;
        let path = folder.join(format!("Prisme {}.wav", chrono::Local::now().format(FILE_TIME)));
        let spec =
            WavSpec { channels: 2, sample_rate, bits_per_sample: 24, sample_format: SampleFormat::Int };
        let mut wav = WavWriter::create(&path, spec).map_err(|e| AppError::Other(e.to_string()))?;
        let (sink, mut queue) = rtrb::RingBuffer::new(sample_rate as usize * 2 * QUEUE_S);
        state.send(Command::StartRecording { source, sink })?;
        let stop = Arc::new(AtomicBool::new(false));
        let writer = {
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("prisme-recording".into())
                .spawn(move || {
                    let mut written = 0u64;
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
                            wav.write_sample((sample.clamp(-1.0, 1.0) * I24_MAX).round() as i32)?;
                        }
                        written += available as u64 / 2;
                    }
                    wav.finalize()?;
                    Ok(written)
                })
                .expect("cannot start the recording thread")
        };
        Ok(Self { path, sample_rate, started_at_ms: prisme_library::now_ms(), stop, writer })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Ends the file and writes the tracklist of what was played since the start.
    pub fn stop(self, state: &AppState, db: &LibraryDb) -> AppResult<RecordingSummary> {
        match state.send(Command::StopRecording) {
            Ok(()) | Err(AppError::AudioNotRunning) => {}
            Err(e) => return Err(e),
        }
        self.stop.store(true, Ordering::Release);
        let frames = self
            .writer
            .join()
            .map_err(|_| AppError::Other("the recording thread failed".into()))?
            .map_err(|e| AppError::Other(e.to_string()))?;
        let plays = db.plays_since(self.started_at_ms)?;
        let file_name = self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        std::fs::write(self.path.with_extension("txt"), tracklist_text(&plays, self.started_at_ms))?;
        std::fs::write(self.path.with_extension("cue"), cue_sheet(&plays, self.started_at_ms, &file_name))?;
        Ok(RecordingSummary {
            seconds: frames as f64 / self.sample_rate as f64,
            tracks: plays.len(),
            path: self.path,
        })
    }
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
