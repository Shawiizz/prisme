//! The Auto DJ.

use crate::auto_dj::machine::{Reason, State, Status, Takeover};
use crate::auto_dj::plan::FadeLength;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "AutoDjState"))]
pub enum AutoDjStateDto {
    #[default]
    Off,
    On,
    Mixing,
    Paused,
}

/// Why the Auto DJ stopped, paused or left a track out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "AutoDjReason"))]
pub enum AutoDjReasonDto {
    ExternalMixer,
    Records,
    ListEmpty,
    ListEnded,
    /// The DJ played, stopped or moved a deck (`reasonDeck`).
    Transport,
    Crossfader,
    /// The DJ loaded a deck (`reasonDeck`).
    Load,
    Unloadable,
}

/// The Auto DJ as the interface shows it.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(test, derive(ts_rs::TS), ts(rename = "AutoDjStatus"))]
pub struct AutoDjStatusDto {
    pub state: AutoDjStateDto,
    pub reason: Option<AutoDjReasonDto>,
    pub reason_deck: Option<usize>,
    /// The deck being mixed in.
    pub incoming: Option<usize>,
    pub fade_beats: u32,
    pub fade_s: f64,
}

impl AutoDjStatusDto {
    pub fn of(status: Status, fade: FadeLength) -> Self {
        let (reason, reason_deck) = match status.reason {
            None => (None, None),
            Some(Reason::ExternalMixer) => (Some(AutoDjReasonDto::ExternalMixer), None),
            Some(Reason::Records) => (Some(AutoDjReasonDto::Records), None),
            Some(Reason::ListEmpty) => (Some(AutoDjReasonDto::ListEmpty), None),
            Some(Reason::ListEnded) => (Some(AutoDjReasonDto::ListEnded), None),
            Some(Reason::Unloadable) => (Some(AutoDjReasonDto::Unloadable), None),
            Some(Reason::TakenOver(Takeover::Transport(deck))) => {
                (Some(AutoDjReasonDto::Transport), Some(deck))
            }
            Some(Reason::TakenOver(Takeover::Load(deck))) => (Some(AutoDjReasonDto::Load), Some(deck)),
            Some(Reason::TakenOver(Takeover::Crossfader)) => (Some(AutoDjReasonDto::Crossfader), None),
        };
        Self {
            state: match status.state {
                State::Off => AutoDjStateDto::Off,
                State::On => AutoDjStateDto::On,
                State::Mixing => AutoDjStateDto::Mixing,
                State::Paused => AutoDjStateDto::Paused,
            },
            reason,
            reason_deck,
            incoming: status.incoming,
            fade_beats: fade.beats,
            fade_s: fade.seconds,
        }
    }
}
