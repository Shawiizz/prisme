//! Errors returned to the frontend (serialized as plain messages).

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("audio is not running")]
    AudioNotRunning,
    #[error("invalid deck {0}")]
    InvalidDeck(usize),
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
