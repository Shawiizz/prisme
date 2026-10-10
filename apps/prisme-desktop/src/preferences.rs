//! What the user chooses once and finds again at the next start.

use crate::audio_host::DECKS;
use crate::dto::{AudioSettings, MixModeDto, VinylModeDto};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Preferences {
    pub audio: AudioSettings,
    pub decks: [DeckPreferences; DECKS],
    pub dj: DjPreferences,
    pub mixer: MixerPreferences,
    pub library: LibraryPreferences,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DeckPreferences {
    pub vinyl_enabled: bool,
    pub vinyl_mode: VinylModeDto,
    pub lead_in_s: f64,
    /// The turntable's left and right channels are crossed.
    pub swap_channels: bool,
}

/// What SYNC does on a record in relative mode.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VinylSync {
    #[default]
    Once,
    Locked,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DjPreferences {
    pub vinyl_sync: VinylSync,
    pub quantize: bool,
    /// Range of the tempo faders (0.08 = ±8 %).
    pub pitch_range: f64,
}

impl Default for DjPreferences {
    fn default() -> Self {
        Self { vinyl_sync: VinylSync::Once, quantize: true, pitch_range: 0.08 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MixerPreferences {
    pub mode: MixModeDto,
    /// Headphones: 0 = cued decks only, 1 = master only.
    pub cue_mix: f32,
    pub headphone_gain: f32,
}

impl Default for MixerPreferences {
    fn default() -> Self {
        Self { mode: MixModeDto::External, cue_mix: 0.0, headphone_gain: 1.0 }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LibraryPreferences {
    pub last_folder: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_partial_file_keeps_the_other_defaults() {
        let p: Preferences = serde_json::from_str(r#"{ "dj": { "vinylSync": "locked" } }"#).unwrap();
        assert_eq!(p.dj.vinyl_sync, VinylSync::Locked);
        assert!(p.dj.quantize);
        assert_eq!(p.dj.pitch_range, 0.08);
        assert_eq!(p.audio, AudioSettings::default());
    }

    #[test]
    fn preferences_survive_a_round_trip() {
        let mut p = Preferences::default();
        p.decks[1] = DeckPreferences {
            vinyl_enabled: true,
            vinyl_mode: VinylModeDto::Relative,
            lead_in_s: 2.5,
            swap_channels: true,
        };
        p.library.last_folder = Some("C:/Music/House".into());
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<Preferences>(&json).unwrap(), p);
    }
}
