//! The music library.
//!
//! * [`db`]: SQLite database of the tracks and their analysis results, keyed by
//!   path and invalidated when the file or the analysis algorithms change;
//! * [`scan`]: finding the audio files under a folder;
//! * [`analyzer`]: analysing, in the background and at low priority, every file
//!   whose stored results are missing or stale.

pub mod analyzer;
pub mod db;
pub mod scan;

pub use analyzer::{default_workers, AnalysisListener, AnalysisProgress, BackgroundAnalyzer};
pub use db::{path_key, DbError, FileStamp, LibraryDb, TrackRecord};
pub use scan::{scan_folder, ScannedFile};
