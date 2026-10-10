//! The app's log: one file per day in the log folder, the last week kept, also on the
//! console while developing. Panics are logged too, so that a crash leaves a trace.

use std::path::{Path, PathBuf};
use tracing::error;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::fmt::writer::MakeWriterExt;

const PREFIX: &str = "prisme";
const SUFFIX: &str = "log";
const DAYS_KEPT: usize = 7;

/// Keeps the log writer running: logs written after it is dropped are lost.
pub struct LogGuard {
    _writer: Option<WorkerGuard>,
}

/// Starts logging to `dir` (to the console only without it).
pub fn init(dir: Option<&Path>) -> LogGuard {
    let file = dir.and_then(|dir| {
        RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_prefix(PREFIX)
            .filename_suffix(SUFFIX)
            .max_log_files(DAYS_KEPT)
            .build(dir)
            .inspect_err(|e| eprintln!("no log file: {e}"))
            .ok()
    });
    let console = std::io::stderr.with_max_level(if cfg!(debug_assertions) {
        tracing::Level::DEBUG
    } else {
        tracing::Level::WARN
    });
    let guard = match file {
        Some(file) => {
            let (writer, guard) = tracing_appender::non_blocking(file);
            let writer = writer.with_max_level(tracing::Level::INFO).and(console);
            tracing_subscriber::fmt().with_timer(LocalTime).with_writer(writer).with_ansi(false).init();
            Some(guard)
        }
        None => {
            tracing_subscriber::fmt().with_timer(LocalTime).with_writer(console).init();
            None
        }
    };
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        error!("{info}");
        default_hook(info);
    }));
    LogGuard { _writer: guard }
}

/// The computer's time: the log is read against what happened that evening.
struct LocalTime;

impl FormatTime for LocalTime {
    fn format_time(&self, w: &mut Writer<'_>) -> std::fmt::Result {
        write!(w, "{}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"))
    }
}

/// The most recent log file in `dir`.
pub fn latest_file(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(PREFIX)))
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

/// The last `count` lines of `text`.
pub fn tail(text: &str, count: usize) -> &str {
    if count == 0 {
        return "";
    }
    let body = text.strip_suffix('\n').unwrap_or(text);
    let start = body.rmatch_indices('\n').nth(count - 1).map_or(0, |(i, _)| i + 1);
    &text[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_keeps_the_last_lines() {
        assert_eq!(tail("a\nb\nc\n", 2), "b\nc\n");
        assert_eq!(tail("a\nb\nc", 2), "b\nc");
        assert_eq!(tail("a\nb", 5), "a\nb");
        assert_eq!(tail("", 3), "");
        assert_eq!(tail("a", 0), "");
    }
}
