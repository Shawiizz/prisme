//! Errors returned to the frontend: each with a code the interface words in its language and
//! what it needs to word it (`params`), and its English message for the codes the interface
//! does not know (`errors.ts`).

use prisme_audio_io::latency::LatencyError;
use prisme_audio_io::AudioError;
use prisme_media::MediaError;
use serde::ser::{Serialize, SerializeStruct, Serializer};
use serde_json::{json, Value};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("audio is not running")]
    AudioNotRunning,
    #[error("the separation model is not installed")]
    NoStemsModel,
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
    #[error("a sampler sound lasts {0} s at most: load longer files on a deck")]
    SampleTooLong(f64),
    #[error("already recording")]
    AlreadyRecording,
    /// The files whose genre could not be written.
    #[error("{}", genre_not_written(.0))]
    GenreNotWritten(Vec<String>),
    #[error(transparent)]
    Library(#[from] prisme_library::DbError),
    #[error(transparent)]
    Rekordbox(#[from] prisme_import::rekordbox::RekordboxError),
    #[error("MIDI: {0}")]
    Midi(String),
    #[error(transparent)]
    Latency(#[from] LatencyError),
    #[error("controller {0} is not plugged in")]
    ControllerUnplugged(String),
    #[error("unknown timecode format {0}")]
    UnknownFormat(String),
    #[error("the engine is busy, try again")]
    EngineBusy,
    #[error("allow Prisme to listen to the inputs of the sound card, then apply again")]
    InputsNotAllowed,
    #[error(transparent)]
    Media(#[from] MediaError),
    #[error(transparent)]
    Audio(#[from] AudioError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Other(String),
}

fn genre_not_written(files: &[String]) -> String {
    match files {
        [one] => format!("genre not written into {one}"),
        many => format!("genre not written into {} files", many.len()),
    }
}

impl AppError {
    /// The code the interface knows this error by, and what it needs to word it (decks and
    /// hot cues counted from 0).
    fn code(&self) -> (&'static str, Value) {
        let none = || json!({});
        match self {
            AppError::AudioNotRunning => ("audioNotRunning", none()),
            AppError::NoStemsModel => ("noStemsModel", none()),
            AppError::InvalidDeck(deck) => ("invalidDeck", json!({ "deck": deck })),
            AppError::NoTrack(deck) => ("noTrack", json!({ "deck": deck })),
            AppError::InvalidCue(cue) => ("invalidCue", json!({ "cue": cue })),
            AppError::EmptyCrateName => ("emptyCrateName", none()),
            AppError::SmartCrate => ("smartCrate", none()),
            AppError::NoBeatGrid(deck) => ("noBeatGrid", json!({ "deck": deck })),
            AppError::InvalidBpm(bpm) => ("invalidBpm", json!({ "bpm": bpm })),
            AppError::InvalidKey(key) => ("invalidKey", json!({ "key": key })),
            AppError::SampleTooLong(seconds) => ("sampleTooLong", json!({ "seconds": seconds })),
            AppError::AlreadyRecording => ("alreadyRecording", none()),
            AppError::GenreNotWritten(files) => {
                ("genreNotWritten", json!({ "count": files.len(), "file": files.first() }))
            }
            AppError::Library(_) => ("library", none()),
            AppError::Rekordbox(_) => ("rekordbox", none()),
            AppError::Midi(_) => ("midi", none()),
            AppError::Latency(LatencyError::Audio(e)) | AppError::Audio(e) => audio_code(e),
            AppError::Latency(LatencyError::NoClick { output, input }) => {
                ("noClick", json!({ "output": output, "input": input }))
            }
            AppError::ControllerUnplugged(name) => ("controllerUnplugged", json!({ "controller": name })),
            AppError::UnknownFormat(format) => ("unknownFormat", json!({ "format": format })),
            AppError::EngineBusy => ("engineBusy", none()),
            AppError::InputsNotAllowed => ("inputsNotAllowed", none()),
            AppError::Media(MediaError::Open { path, .. }) => ("cannotOpen", json!({ "path": path })),
            AppError::Media(MediaError::Format(_) | MediaError::NoAudio | MediaError::NoSampleRate) => {
                ("unreadableFile", none())
            }
            AppError::Media(_) => ("media", none()),
            AppError::Io(_) => ("io", none()),
            AppError::Other(_) => ("other", none()),
        }
    }
}

fn audio_code(e: &AudioError) -> (&'static str, Value) {
    match e {
        AudioError::NoOutputDevice => ("noOutputDevice", json!({})),
        AudioError::DeviceNotFound(device) => ("deviceNotFound", json!({ "device": device })),
        AudioError::BackendUnavailable(backend) => {
            ("backendUnavailable", json!({ "backend": backend.to_string() }))
        }
        AudioError::SampleRateMismatch { input, output } => {
            ("sampleRateMismatch", json!({ "input": input, "output": output }))
        }
        _ => ("audio", json!({})),
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let (code, params) = self.code();
        let mut error = s.serialize_struct("AppError", 3)?;
        error.serialize_field("code", code)?;
        error.serialize_field("params", &params)?;
        error.serialize_field("message", &self.to_string())?;
        error.end()
    }
}

pub type AppResult<T> = Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_error_reaches_the_interface_with_its_code_and_its_message() {
        let json = |e: AppError| serde_json::to_value(e).unwrap();
        assert_eq!(
            json(AppError::NoTrack(1)),
            json!({ "code": "noTrack", "params": { "deck": 1 }, "message": "no track on deck 2" })
        );
        assert_eq!(
            json(AppError::Audio(AudioError::DeviceNotFound("UMC404HD".into()))),
            json!({
                "code": "deviceNotFound",
                "params": { "device": "UMC404HD" },
                "message": "audio device not found: UMC404HD",
            })
        );
        assert_eq!(json(AppError::Other("lost".into()))["message"], "lost");
    }

    #[test]
    fn a_genre_not_written_says_into_what() {
        let one = AppError::GenreNotWritten(vec!["a.mp3".into()]);
        assert_eq!(one.to_string(), "genre not written into a.mp3");
        let two = AppError::GenreNotWritten(vec!["a.mp3".into(), "b.flac".into()]);
        assert_eq!(serde_json::to_value(two).unwrap()["params"], json!({ "count": 2, "file": "a.mp3" }));
    }
}
