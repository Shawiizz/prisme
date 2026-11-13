//! The music library.
//!
//! * [`db`]: SQLite database of the tracks, their analysis results (invalidated when the
//!   file or the algorithms change), their [`cues`], the crates and the history of the sets;
//! * [`metadata`] and [`covers`]: tags and cover thumbnails;
//! * [`duplicates`]: tracks that are the same song;
//! * [`scan`]: finding the audio files under a folder;
//! * [`smart`]: the rules of smart crates;
//! * [`analyzer`]: analysing, in the background and at low priority, every file
//!   whose stored results are missing or stale.

pub mod analyzer;
pub mod covers;
pub mod cues;
pub mod db;
pub mod duplicates;
pub mod identity;
pub mod metadata;
pub mod scan;
pub mod smart;

pub use analyzer::{
    background_priority, default_workers, remember_audio, AnalysisListener, AnalysisProgress,
    BackgroundAnalyzer,
};
pub use cues::{Cue, CueSlot, HOT_CUES};
pub use db::{
    now_ms, path_key, Crate, CrateId, DbError, FileStamp, GridEdit, HistoryPlay, HistorySession, LibraryDb,
    RecordVersion, SessionId, TrackEdits, TrackEntry, TrackMarks, TrackRecord, WrittenTags, MAX_RATING,
    TAGS_VERSION,
};
pub use duplicates::duplicate_groups;
pub use identity::{adopt, adopt_unless, Search};
pub use metadata::{read_metadata, Metadata};
pub use scan::{scan_folder, Scan, ScannedFile};
pub use smart::{Rule, SmartRules, TextField};
