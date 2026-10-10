//! The music library.
//!
//! * [`db`]: SQLite database of the tracks, their analysis results (invalidated when the
//!   file or the algorithms change) and their [`cues`];
//! * [`scan`]: finding the audio files under a folder;
//! * [`analyzer`]: analysing, in the background and at low priority, every file
//!   whose stored results are missing or stale.

pub mod analyzer;
pub mod cues;
pub mod db;
pub mod scan;

pub use analyzer::{default_workers, AnalysisListener, AnalysisProgress, BackgroundAnalyzer};
pub use cues::{Cue, CueSlot, HOT_CUES};
pub use db::{path_key, DbError, FileStamp, LibraryDb, TrackRecord};
pub use scan::{scan_folder, ScannedFile};
