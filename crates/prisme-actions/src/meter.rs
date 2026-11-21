//! The level meters of the screen, reading the engine's peaks as a mixer's meter does: it rises
//! at once and falls back slowly, its highest level held a moment above it, a clip shown a
//! moment.

use crate::perform::ClipHold;
use prisme_engine::{gain_to_db, EngineStatus, MAX_DECKS};
use serde::Serialize;

/// The lowest level a meter shows (dBFS): its scale runs from there to full scale.
const FLOOR_DB: f32 = -48.0;
/// It falls back this fast (dB per second).
const RELEASE_DB_S: f32 = 20.0;
/// Its highest level stays this long (s) before it falls back too.
const HOLD_S: f64 = 1.0;

/// What a meter shows, as positions on its scale: 0 at its floor or below, 1 at full scale.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "MeterLevel"))]
pub struct Level {
    pub level: f32,
    /// The highest level of late.
    pub held: f32,
    /// A peak reached full scale a moment ago.
    pub clip: bool,
}

/// A meter between two reads of the peaks: what it shows, and since when.
#[derive(Debug, Clone, Copy)]
pub struct Meter {
    db: f32,
    held: f32,
    held_at_s: f64,
    read_at_s: Option<f64>,
    clip: ClipHold,
}

impl Default for Meter {
    fn default() -> Self {
        Self {
            db: f32::NEG_INFINITY,
            held: f32::NEG_INFINITY,
            held_at_s: 0.0,
            read_at_s: None,
            clip: ClipHold::default(),
        }
    }
}

impl Meter {
    /// The meter once it reads `peak` (1 = full scale) at `now_s`.
    pub fn read(&mut self, peak: f32, now_s: f64) -> Level {
        let db = gain_to_db(peak);
        let fall = self.read_at_s.map_or(f32::INFINITY, |at| RELEASE_DB_S * (now_s - at) as f32);
        self.db = db.max(self.db - fall);
        if self.db >= self.held {
            (self.held, self.held_at_s) = (self.db, now_s);
        } else if now_s - self.held_at_s > HOLD_S {
            self.held = self.db.max(self.held - fall);
        }
        self.read_at_s = Some(now_s);
        Level { level: position(self.db), held: position(self.held), clip: self.clip.heard(peak, now_s) }
    }
}

fn position(db: f32) -> f32 {
    ((db - FLOOR_DB) / -FLOOR_DB).clamp(0.0, 1.0)
}

/// The meters of the screen: each deck's, the master's, the microphone's and the aux line's.
#[derive(Debug, Clone, Copy, Default)]
pub struct Meters {
    decks: [Meter; MAX_DECKS],
    main: Meter,
    mic: Meter,
    aux: Meter,
}

/// What each of the [`Meters`] shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "MeterLevels"))]
pub struct Levels {
    pub decks: [Level; MAX_DECKS],
    pub main: Level,
    pub mic: Level,
    pub aux: Level,
}

impl Meters {
    /// The meters once they read the peaks of `status` at `now_s`.
    pub fn read(&mut self, status: &EngineStatus, now_s: f64) -> Levels {
        Levels {
            decks: std::array::from_fn(|d| self.decks[d].read(status.decks[d].peak, now_s)),
            main: self.main.read(status.main_peak, now_s),
            mic: self.mic.read(status.mic_peak, now_s),
            aux: self.aux.read(status.aux_peak, now_s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HALF: f32 = 0.501_187; // -6 dB

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn a_meter_rises_at_once_and_falls_back_at_twenty_db_a_second() {
        let mut meter = Meter::default();
        assert_eq!(meter.read(0.0, 0.0).level, 0.0, "silence lights nothing");
        assert!(close(meter.read(1.0, 0.0).level, 1.0));
        // half a second later, silent: 10 dB down, on a scale of 48
        assert!(close(meter.read(0.0, 0.5).level, 1.0 - 10.0 / 48.0));
        assert!(close(meter.read(0.1, 0.5).level, 1.0 - 10.0 / 48.0), "a lower peak is under it");
        assert!(close(meter.read(0.1, 1.0).level, 1.0 - 20.0 / 48.0), "until it falls to it");
        assert!(close(meter.read(HALF, 1.0).level, 1.0 - 6.0 / 48.0), "a higher one at once");
    }

    #[test]
    fn its_highest_level_stays_a_second_then_falls_back_too() {
        let mut meter = Meter::default();
        meter.read(1.0, 0.0);
        assert_eq!(meter.read(0.0, 0.9).held, 1.0);
        let after = meter.read(0.0, 1.1).held;
        assert!(after < 1.0 && close(after, 1.0 - 4.0 / 48.0), "{after}");
    }

    #[test]
    fn a_clip_shows_a_second_whatever_the_level_does() {
        let mut meter = Meter::default();
        assert!(!meter.read(0.99, 0.0).clip);
        assert!(meter.read(1.0, 0.1).clip);
        assert!(meter.read(0.0, 1.0).clip);
        assert!(!meter.read(0.0, 1.2).clip);
    }

    #[test]
    fn the_meters_read_every_deck_and_the_masters_and_live_peaks() {
        let mut status = EngineStatus::default();
        status.decks[1].peak = 1.0;
        status.mic_peak = HALF;
        let levels = Meters::default().read(&status, 0.0);
        assert_eq!(levels.decks[1].level, 1.0);
        assert_eq!(levels.decks[0].level, 0.0);
        assert!(close(levels.mic.level, 1.0 - 6.0 / 48.0));
        assert_eq!(levels.main.level, 0.0);
    }
}
