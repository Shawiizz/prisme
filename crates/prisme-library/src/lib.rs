//! The music library.
//!
//! * [`db`]: SQLite database of the tracks, their analysis results (invalidated when the
//!   file or the algorithms change) and their [`cues`];
//! * [`metadata`] and [`covers`]: tags and cover thumbnails;
//! * [`scan`]: finding the audio files under a folder;
//! * [`analyzer`]: analysing, in the background and at low priority, every file
//!   whose stored results are missing or stale.

pub mod analyzer;
pub mod covers;
pub mod cues;
pub mod db;
pub mod metadata;
pub mod scan;

pub use analyzer::{default_workers, AnalysisListener, AnalysisProgress, BackgroundAnalyzer};
pub use cues::{Cue, CueSlot, HOT_CUES};
pub use db::{path_key, DbError, FileStamp, LibraryDb, RecordVersion, TrackRecord, TAGS_VERSION};
pub use metadata::{read_metadata, Metadata};
pub use scan::{scan_folder, ScannedFile};
