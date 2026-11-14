//! What the user chooses once and finds again at the next start.

use crate::dto::{AudioSettings, CrossfaderCurveDto, HeadphonesOutputDto, MixModeDto, RpmDto, VinylModeDto};
use prisme_engine::MAX_DECKS;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Preferences {
    pub audio: AudioSettings,
    #[serde(deserialize_with = "decks_of")]
    pub decks: [DeckPreferences; MAX_DECKS],
    pub dj: DjPreferences,
    pub mixer: MixerPreferences,
    pub library: LibraryPreferences,
}

/// The decks a file holds, fewer than there are (two, before there could be four): the
/// others take the defaults.
fn decks_of<'de, D: Deserializer<'de>>(file: D) -> Result<[DeckPreferences; MAX_DECKS], D::Error> {
    let mut saved = Vec::<DeckPreferences>::deserialize(file)?.into_iter();
    Ok(std::array::from_fn(|_| saved.next().unwrap_or_default()))
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DeckPreferences {
    pub vinyl_enabled: bool,
    pub vinyl_mode: VinylModeDto,
    pub rpm: RpmDto,
    pub lead_in_s: f64,
    /// The turntable's left and right channels are crossed.
    pub swap_channels: bool,
    /// The pitch stays when the speed changes.
    pub key_lock: bool,
    /// Sound file in each sampler slot.
    pub samples: Vec<Option<String>>,
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
    /// Every track played at the same loudness (see `LoadedTrack::level`).
    pub auto_gain: bool,
    /// Two decks, or four.
    pub deck_count: usize,
    /// The knobs of the mixer hold at their middle when turned through it.
    pub knob_magnet: bool,
}

impl DjPreferences {
    /// The decks of the engine: four if asked for, else two.
    pub fn decks(&self) -> usize {
        if self.deck_count == MAX_DECKS {
            MAX_DECKS
        } else {
            2
        }
    }
}

impl Default for DjPreferences {
    fn default() -> Self {
        Self {
            vinyl_sync: VinylSync::Once,
            quantize: true,
            pitch_range: 0.08,
            auto_gain: true,
            deck_count: 2,
            knob_magnet: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MixerPreferences {
    pub mode: MixModeDto,
    pub headphones: HeadphonesOutputDto,
    /// Headphones: 0 = cued decks only, 1 = master only.
    pub cue_mix: f32,
    pub headphone_gain: f32,
    pub crossfader_curve: CrossfaderCurveDto,
    /// Master limiter (adds 1 ms of latency to the software mixer).
    pub limiter: bool,
}

impl Default for MixerPreferences {
    fn default() -> Self {
        Self {
            // a phone mixes itself: the controller plugged into it only sends what its
            // faders and knobs do
            mode: if cfg!(mobile) { MixModeDto::Internal } else { MixModeDto::External },
            headphones: HeadphonesOutputDto::Auto,
            cue_mix: 0.0,
            headphone_gain: 1.0,
            crossfader_curve: CrossfaderCurveDto::Smooth,
            limiter: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LibraryPreferences {
    pub last_folder: Option<String>,
    /// Columns of the track list the DJ hid (frontend ids).
    pub hidden_columns: Vec<String>,
    /// Tempo and key written into the files' tags (see `tag_writer`).
    pub write_tags: bool,
    /// Those another program wrote (Mixed In Key, say) replaced as well.
    pub replace_tags: bool,
    /// The background analysis of the music folder waits while a deck plays.
    pub pause_while_playing: bool,
    /// A track loaded on a deck is separated right away, before the library's; one put in
    /// the prepare list in the background. Not chosen: with a graphics card only (see
    /// `stems_service::separates_on_load`).
    #[serde(rename = "stemsOnLoad")]
    pub separate_on_load: Option<bool>,
    /// The same as saved before it could be left open, when on was what nobody chose (see
    /// [`LibraryPreferences::settle`]).
    #[serde(rename = "separateOnLoad", skip_serializing)]
    separate_on_load_before: Option<bool>,
}

impl LibraryPreferences {
    /// Takes over the choice of a file saved before it could be left open: off was chosen
    /// and stays, on was the default and is left open. `true` if something changed.
    pub fn settle(&mut self) -> bool {
        match self.separate_on_load_before.take() {
            Some(false) if self.separate_on_load.is_none() => {
                self.separate_on_load = Some(false);
                true
            }
            before => before.is_some(),
        }
    }
}

impl Default for LibraryPreferences {
    fn default() -> Self {
        Self {
            last_folder: None,
            hidden_columns: Vec::new(),
            // a phone lets an application read the music of the others, not rewrite it
            write_tags: cfg!(desktop),
            replace_tags: false,
            pause_while_playing: true,
            separate_on_load: None,
            separate_on_load_before: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_partial_file_keeps_the_other_defaults() {
        let p: Preferences = serde_json::from_str(r#"{ "dj": { "vinylSync": "locked" } }"#).unwrap();
        assert_eq!(p.dj.vinyl_sync, VinylSync::Locked);
        assert!(p.dj.quantize);
        assert!(p.dj.auto_gain);
        assert_eq!(p.dj.pitch_range, 0.08);
        assert_eq!(p.audio, AudioSettings::default());
        assert!(p.library.write_tags);
        assert!(!p.library.replace_tags);
        assert!(p.library.pause_while_playing);
        assert_eq!(p.library.separate_on_load, None);
    }

    #[test]
    fn separating_on_load_stays_off_where_it_was_chosen_and_open_where_it_was_the_default() {
        let saved = |json: &str| {
            let mut p: Preferences = serde_json::from_str(json).unwrap();
            p.library.settle();
            p.library.separate_on_load
        };
        assert_eq!(saved(r#"{ "library": { "separateOnLoad": true } }"#), None);
        assert_eq!(saved(r#"{ "library": { "separateOnLoad": false } }"#), Some(false));
        assert_eq!(saved(r#"{ "library": { "separateOnLoad": false, "stemsOnLoad": true } }"#), Some(true));
        assert_eq!(saved(r#"{ "library": { "stemsOnLoad": true } }"#), Some(true));
        // what is saved from now on holds the choice alone
        let mut p: Preferences =
            serde_json::from_str(r#"{ "library": { "separateOnLoad": false } }"#).unwrap();
        assert!(p.library.settle());
        assert!(!p.library.settle());
        let json = serde_json::to_string(&p).unwrap();
        assert!(json.contains(r#""stemsOnLoad":false"#) && !json.contains("separateOnLoad"));
    }

    #[test]
    fn a_file_from_before_four_decks_keeps_its_two() {
        let p: Preferences =
            serde_json::from_str(r#"{ "decks": [{ "keyLock": true }, { "rpm": "45" }] }"#).unwrap();
        assert!(p.decks[0].key_lock);
        assert_eq!(p.decks[1].rpm, RpmDto::Rpm45);
        assert_eq!(p.decks[2], DeckPreferences::default());
        assert_eq!(p.dj.decks(), 2);
    }

    #[test]
    fn preferences_survive_a_round_trip() {
        let mut p = Preferences::default();
        p.decks[1] = DeckPreferences {
            vinyl_enabled: true,
            vinyl_mode: VinylModeDto::Relative,
            rpm: RpmDto::Rpm45,
            lead_in_s: 2.5,
            swap_channels: true,
            key_lock: true,
            samples: vec![Some("C:/Samples/horn.wav".into()), None],
        };
        p.library.last_folder = Some("C:/Music/House".into());
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<Preferences>(&json).unwrap(), p);
    }
}
