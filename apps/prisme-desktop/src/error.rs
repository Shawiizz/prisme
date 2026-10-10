//! Errors returned to the frontend (serialized as plain messages).

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("audio is not running")]
    AudioNotRunning,
    #[error("invalid deck {0}")]
    InvalidDeck(usize),
    #[error("no track on deck {}", .0 + 1)]
    NoTrack(usize),
    #[error("invalid hot cue {0}")]
    InvalidCue(u8),
    #[error("a crate needs a name")]
    EmptyCrateName,
    #[error("a smart crate picks its own tracks")]
    SmartCrate,
    #[error("deck {} has no beat grid", .0 + 1)]
    NoBeatGrid(usize),
    #[error("{0} BPM is out of range")]
    InvalidBpm(f64),
    #[error("unknown key {0}")]
    InvalidKey(String),
    #[error(transparent)]
    Library(#[from] prisme_library::DbError),
    #[error("unknown timecode format {0}")]
    UnknownFormat(String),
    #[error("the engine is busy, try again")]
    EngineBusy,
    #[error(transparent)]
    Media(#[from] prisme_media::MediaError),
    #[error(transparent)]
    Audio(#[from] prisme_audio_io::AudioError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

impl serde::Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
