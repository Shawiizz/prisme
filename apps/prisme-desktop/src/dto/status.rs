//! What the status stream sends: the state at a frame, and the sections that moved.

use super::{
    AudioHealthDto, AudioStateDto, AutoDjStatusDto, ControllersStateDto, DeckPadsDto, DeckSettingsDto,
    DeckStatusDto, FoldersStateDto, LibraryListsDto, LibraryViewDto, MixerStateDto, PreferencesDto,
    RecordingStatusDto, RoutingStateDto, StemsStateDto, TrackInfoDto,
};
use crate::state::{lock, AppState, Section};
use prisme_actions::meter::Levels;
use prisme_actions::screen;
use prisme_actions::view::{plays, View};
use prisme_audio_io::StreamStats;
use prisme_engine::MAX_DECKS;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "EngineStatus"))]
pub struct StatusDto {
    pub audio_running: bool,
    pub decks: Vec<DeckStatusDto>,
    /// A deck plays (see `view::plays`): no update is installed meanwhile.
    pub plays: bool,
    /// What the meters show: each deck's, the master's, the live sources'.
    pub meters: Levels,
    pub health: Option<AudioHealthDto>,
    pub recording: Option<RecordingStatusDto>,
    pub preview: Option<PreviewStatusDto>,
    /// The strip comparing two decks' beats.
    pub phase: screen::PhaseStrip,
    /// Seconds before the Auto DJ's next mix starts, while one is planned.
    pub mix_in_s: Option<f64>,
    /// What the interface mirrors, when it changed since the status before.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(skip))]
    pub controls: Option<ControlStateDto>,
}

/// What the interface mirrors of the decks, their tracks, the mixer and the DJ's preferences,
/// by section (`state::Section`): those that did not change since last sent are left out.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "ControlChanges", optional_fields))]
pub struct ControlStateDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mixer: Option<MixerStateDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub routing: Option<RoutingStateDto>,
    /// The decks' settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decks: Option<Vec<DeckSettingsDto>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantize: Option<bool>,
    /// The library over the whole window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library_only: Option<bool>,
    /// The range of the tempo faders without one of their own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pitch_range: Option<f64>,
    /// Each deck's track, as it is now.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracks: Option<Vec<Option<TrackInfoDto>>>,
    /// Whether each deck loads a track.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loading: Option<[bool; MAX_DECKS]>,
    /// What each deck's pads hold.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub perform: Option<[DeckPadsDto; MAX_DECKS]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<PreviewStateDto>,
    /// The list browsed, and the rows of its window.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub library: Option<LibraryViewDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lists: Option<LibraryListsDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferences: Option<PreferencesDto>,
    /// The separation into stems, with the DJ's preferences of it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stems: Option<StemsStateDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub controllers: Option<ControllersStateDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<AudioStateDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folders: Option<FoldersStateDto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_dj: Option<AutoDjStatusDto>,
}

impl ControlStateDto {
    /// The `sections` of `state`, what is shown of them read in `view`.
    pub fn of(state: &AppState, view: &View, sections: impl IntoIterator<Item = Section>) -> Self {
        let mut controls = Self::default();
        for section in sections {
            match section {
                Section::Mixer => {
                    let (quantize, pitch_range) =
                        state.preferences.read(|p| (p.dj.quantize, p.dj.pitch_range));
                    controls.mixer = Some(MixerStateDto::of(state, view));
                    (controls.quantize, controls.pitch_range) = (Some(quantize), Some(pitch_range));
                }
                Section::Routing => controls.routing = Some(RoutingStateDto::of(state, view.decks_in_use)),
                Section::Decks => controls.decks = Some(DeckSettingsDto::all(state, view)),
                Section::Tracks => {
                    let tracks = lock(&state.tracks)
                        .iter()
                        .enumerate()
                        .map(|(deck, t)| t.as_ref().map(|t| t.info(deck)))
                        .collect();
                    controls.tracks = Some(tracks);
                    controls.loading = Some(state.loading());
                }
                Section::Perform => {
                    let perform = lock(&state.perform);
                    controls.perform = Some(perform.decks.each_ref().map(DeckPadsDto::from));
                    controls.library_only = Some(perform.library_only);
                }
                Section::Preview => controls.preview = Some(lock(&state.preview).clone()),
                Section::Library => controls.library = Some(lock(&state.library).view()),
                Section::Lists => controls.lists = Some(lock(&state.library).lists()),
                Section::Settings => controls.preferences = Some(PreferencesDto::of(state)),
                Section::Stems | Section::Controllers => {}
                Section::Audio => controls.audio = Some(AudioStateDto::of(state)),
                Section::Folders => controls.folders = Some(lock(&state.folders).clone()),
                Section::AutoDj => controls.auto_dj = Some(lock(&state.auto_dj).clone()),
            }
            if matches!(section, Section::Stems | Section::Settings) {
                let stems = lock(&state.stems).clone();
                controls.stems = Some(state.preferences.read(|p| stems.dto(p)));
            }
            // the actions of the decks not in use leave the mappings shown
            if matches!(section, Section::Controllers | Section::Settings) {
                let shown = lock(&state.controllers).clone();
                controls.controllers = Some(ControllersStateDto::of(shown, state.decks_in_use()));
            }
        }
        controls
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "PreviewStatus"))]
pub struct PreviewStatusDto {
    pub id: u64,
    pub position_s: f64,
    /// Estimated until the track is all read.
    pub duration_s: f64,
    /// Its audio is not read that far yet: it waits, silent.
    pub waiting: bool,
}

/// What the headphones preview: the track the engine was given, and the file being opened
/// for the preview asked last, if any.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "PreviewState"))]
pub struct PreviewStateDto {
    pub track: Option<PreviewDto>,
    pub opening: Option<String>,
}

/// A track previewed in the headphones.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "PreviewTrack"))]
pub struct PreviewDto {
    /// What the status tells it by.
    pub id: u64,
    pub path: String,
    pub title: String,
    pub artist: Option<String>,
    /// Estimated until the track is all read.
    pub duration_s: f64,
    /// The waveform can be fetched (`preview_waveform`); otherwise a `preview-update` says
    /// when it can.
    pub waveform_ready: bool,
}

impl StatusDto {
    /// What `view` shows, its engine running a stream of `stats` at `sample_rate` (none: the
    /// audio is off): the decks in use, and the `meters`.
    pub fn of(view: &View, stream: Option<(&StreamStats, u32)>, meters: Levels) -> Self {
        let s = &view.status;
        let health = stream.map(|(stats, rate)| AudioHealthDto::of(stats, rate));
        let sample_rate = stream.map_or(1.0, |(_, rate)| f64::from(rate));
        Self {
            audio_running: health.as_ref().is_some_and(|h| !h.device_lost),
            decks: (0..view.decks_in_use).map(|d| DeckStatusDto::of(view, d)).collect(),
            plays: s.decks.iter().any(plays),
            meters,
            health,
            recording: s.recording.map(|r| RecordingStatusDto {
                seconds: r.frames as f64 / sample_rate,
                dropped_seconds: r.dropped as f64 / sample_rate,
            }),
            preview: s.preview.map(|p| PreviewStatusDto {
                id: p.id,
                position_s: p.position_s,
                duration_s: p.duration_s,
                waiting: p.waiting,
            }),
            phase: screen::phase_strip(view),
            mix_in_s: None,
            controls: None,
        }
    }
}
