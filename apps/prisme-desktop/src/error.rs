//! Errors returned to the frontend: each with a code the interface words in its language and
//! what it needs to word it (`params`), and its English message for the codes the interface
//! does not know (`errors.ts`).

use prisme_actions::switch::LiveSource;
use prisme_audio_io::latency::LatencyError;
use prisme_audio_io::AudioError;
use prisme_media::MediaError;
use serde::{Serialize, Serializer};
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
    #[error("a crate needs a name")]
    EmptyCrateName,
    #[error("a smart crate picks its own tracks")]
    SmartCrate,
    #[error("a smart crate needs a rule")]
    NoRule,
    /// A rule of a smart crate (counted from 0) bounding the years by other than whole years.
    #[error("rule {}: a year is a whole number", .0 + 1)]
    RuleYear(usize),
    /// A rule of a smart crate (counted from 0) naming a key the Camelot wheel does not have.
    #[error("rule {}: {key} is not a key of the Camelot wheel", .rule + 1)]
    RuleKey { rule: usize, key: String },
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
    #[error("your own mixer mixes: the master of Prisme plays nothing to record")]
    SilentMaster,
    #[error("no headphones of their own to preview in: choose their outputs in the settings")]
    NoPreviewOutput,
    #[error("nothing to record without asking: choose the inputs of your mixer in the Rec menu")]
    NoRecordSource,
    /// An input of a live source chosen for the other.
    #[error("the {0:?} source has one of these inputs")]
    InputTaken(LiveSource),
    /// The track of a deck that plays stays on it, the DJ having chosen so (`eject`: taken off,
    /// else replaced).
    #[error("deck {} plays: its track stays on it", .deck + 1)]
    DeckPlays { deck: usize, eject: bool },
    /// The track of a deck that plays leaves it once the DJ says so: `target` performed again
    /// then, taking `track` of the library if it took one.
    #[error("deck {} plays: its track leaves it once the DJ says so", .deck + 1)]
    ConfirmReplace { target: String, deck: usize, eject: bool, track: Option<String> },
    #[error("{0} is not a folder")]
    FolderNotFound(String),
    /// The music folder it is, or is in.
    #[error("already in the music folder {0}")]
    FolderAlreadyIn(String),
    /// The music folder it holds.
    #[error("holds the music folder {0}")]
    FolderHoldsAnother(String),
    #[error("no folder is added on iOS")]
    FoldersFixed,
    #[error("the main music folder stays: choose another one instead")]
    MainFolderStays,
    /// The files whose genre could not be written.
    #[error("{}", genre_not_written(.0))]
    GenreNotWritten(Vec<String>),
    #[error(transparent)]
    Library(#[from] prisme_library::DbError),
    #[error(transparent)]
    Rekordbox(#[from] prisme_import::rekordbox::RekordboxError),
    #[error("MIDI: {0}")]
    Midi(String),
    #[error("HID: {0}")]
    Hid(String),
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
    #[cfg(target_os = "android")]
    #[error("the file downloaded is not the new version of Prisme")]
    NotTheUpdate,
    #[error("a deck plays: the new version installs once they are all stopped")]
    DecksPlay,
    #[error(transparent)]
    Media(#[from] MediaError),
    #[error(transparent)]
    Audio(#[from] AudioError),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// An action asked of the one that does not perform it (the interface's, or one only shown).
    #[error("{0} is not performed here")]
    NotPerformed(String),
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
            AppError::EmptyCrateName => ("emptyCrateName", none()),
            AppError::SmartCrate => ("smartCrate", none()),
            AppError::NoRule => ("noRule", none()),
            AppError::RuleYear(rule) => ("ruleYear", json!({ "rule": rule })),
            AppError::RuleKey { rule, key } => ("ruleKey", json!({ "rule": rule, "key": key })),
            AppError::NoBeatGrid(deck) => ("noBeatGrid", json!({ "deck": deck })),
            AppError::InvalidBpm(bpm) => ("invalidBpm", json!({ "bpm": bpm })),
            AppError::InvalidKey(key) => ("invalidKey", json!({ "key": key })),
            AppError::SampleTooLong(seconds) => ("sampleTooLong", json!({ "seconds": seconds })),
            AppError::AlreadyRecording => ("alreadyRecording", none()),
            AppError::SilentMaster => ("silentMaster", none()),
            AppError::NoPreviewOutput => ("noPreviewOutput", none()),
            AppError::NoRecordSource => ("noRecordSource", none()),
            AppError::InputTaken(source) => ("inputTaken", json!({ "source": source })),
            AppError::DeckPlays { deck, eject } => ("deckPlays", json!({ "deck": deck, "eject": eject })),
            AppError::ConfirmReplace { target, deck, eject, track } => {
                ("confirmReplace", json!({ "target": target, "deck": deck, "eject": eject, "track": track }))
            }
            AppError::FolderNotFound(_) => ("folderNotFound", none()),
            AppError::FolderAlreadyIn(folder) => ("folderAlreadyIn", json!({ "folder": folder })),
            AppError::FolderHoldsAnother(folder) => ("folderHoldsAnother", json!({ "folder": folder })),
            AppError::FoldersFixed => ("foldersFixed", none()),
            AppError::MainFolderStays => ("mainFolderStays", none()),
            AppError::GenreNotWritten(files) => {
                ("genreNotWritten", json!({ "count": files.len(), "file": files.first() }))
            }
            AppError::Library(_) => ("library", none()),
            AppError::Rekordbox(_) => ("rekordbox", none()),
            AppError::Midi(_) => ("midi", none()),
            AppError::Hid(_) => ("hid", none()),
            AppError::Latency(LatencyError::Audio(e)) | AppError::Audio(e) => audio_code(e),
            AppError::Latency(LatencyError::NoClick { output, input }) => {
                ("noClick", json!({ "output": output, "input": input }))
            }
            AppError::ControllerUnplugged(name) => ("controllerUnplugged", json!({ "controller": name })),
            AppError::UnknownFormat(format) => ("unknownFormat", json!({ "format": format })),
            AppError::EngineBusy => ("engineBusy", none()),
            AppError::InputsNotAllowed => ("inputsNotAllowed", none()),
            #[cfg(target_os = "android")]
            AppError::NotTheUpdate => ("notTheUpdate", none()),
            AppError::DecksPlay => ("decksPlay", none()),
            AppError::Media(MediaError::Open { path, .. }) => ("cannotOpen", json!({ "path": path })),
            AppError::Media(MediaError::Format(_) | MediaError::NoAudio | MediaError::NoSampleRate) => {
                ("unreadableFile", none())
            }
            AppError::Media(_) => ("media", none()),
            AppError::Io(_) => ("io", none()),
            AppError::NotPerformed(_) => ("notPerformed", none()),
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

/// An error as the interface is sent it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "BackendError"))]
pub struct ErrorDto {
    pub code: &'static str,
    /// What the interface needs to word it.
    #[cfg_attr(test, ts(type = "Record<string, string | number | boolean | null>"))]
    pub params: Value,
    /// Its English message.
    pub message: String,
}

impl From<&AppError> for ErrorDto {
    fn from(e: &AppError) -> Self {
        let (code, params) = e.code();
        Self { code, params, message: e.to_string() }
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        ErrorDto::from(self).serialize(s)
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
    fn a_music_folder_refused_names_the_one_in_the_way() {
        assert_eq!(
            serde_json::to_value(AppError::FolderHoldsAnother("E:\\Sets".into())).unwrap(),
            json!({
                "code": "folderHoldsAnother",
                "params": { "folder": "E:\\Sets" },
                "message": "holds the music folder E:\\Sets",
            })
        );
        assert_eq!(AppError::FolderAlreadyIn("D:\\Music".into()).code().1, json!({ "folder": "D:\\Music" }));
    }

    #[test]
    fn a_genre_not_written_says_into_what() {
        let one = AppError::GenreNotWritten(vec!["a.mp3".into()]);
        assert_eq!(one.to_string(), "genre not written into a.mp3");
        let two = AppError::GenreNotWritten(vec!["a.mp3".into(), "b.flac".into()]);
        assert_eq!(serde_json::to_value(two).unwrap()["params"], json!({ "count": 2, "file": "a.mp3" }));
    }
}
