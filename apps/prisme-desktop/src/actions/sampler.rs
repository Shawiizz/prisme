//! A deck's sampler: its sounds played and stopped (see `crate::sampler`), and its keys.

use super::Performing;
use crate::error::{AppError, AppResult};
use crate::sampler;
use crate::state::{lock, AppState};
use prisme_actions::input::Given;
use prisme_control::SampleAction;
use prisme_engine::{Command, SAMPLE_SLOTS};
use std::path::PathBuf;

impl Performing<'_> {
    /// A key of `slot` of `deck`'s sampler pressed, or let go (only `Held` acts then).
    pub(super) fn sample_key(
        &self,
        deck: usize,
        slot: usize,
        action: SampleAction,
        given: Given,
    ) -> AppResult<()> {
        let state = self.state;
        match action {
            SampleAction::Held if given.pressed() => trigger_sample(state, deck, slot, None),
            SampleAction::Held => stop_sample(state, deck, slot),
            _ if !given.pressed() => Ok(()),
            SampleAction::Trigger => trigger_sample(state, deck, slot, None),
            // as hard as the pad was hit
            SampleAction::Velocity => match given {
                Given::Value(velocity) => trigger_sample(state, deck, slot, Some(velocity)),
                Given::Steps(_) => Ok(()),
            },
            SampleAction::Stop => stop_sample(state, deck, slot),
            SampleAction::Load => self.load_sample(deck, slot),
            SampleAction::Eject => clear_sample(state, deck, slot),
            SampleAction::Smart => self.smart_sample(deck, slot),
        }
    }

    /// The sound of the selected track put in `slot` of `deck`'s sampler.
    fn load_sample(&self, deck: usize, slot: usize) -> AppResult<()> {
        match self.track() {
            Some(path) => load_sample(self.state, deck, slot, path.to_owned()),
            None => Ok(()),
        }
    }

    /// A pad of the sampler: its sound stopped while it plays, played while it waits. On an
    /// empty slot it is sent as a load (`Actions::sent_as`): one emptied since does nothing,
    /// never opening a file on the thread that sent it.
    fn smart_sample(&self, deck: usize, slot: usize) -> AppResult<()> {
        let state = self.state;
        if plays(state, deck, slot) {
            stop_sample(state, deck, slot)
        } else if lock(&state.samplers)[deck][slot].is_some() {
            trigger_sample(state, deck, slot, None)
        } else {
            Ok(())
        }
    }
}

/// Whether the sound of `slot` of `deck`'s sampler plays.
fn plays(state: &AppState, deck: usize, slot: usize) -> bool {
    state.deck_status(deck).is_ok_and(|s| s.samples_playing & 1 << slot != 0)
}

fn check_slot(deck: usize, slot: usize) -> AppResult<()> {
    AppState::deck_exists(deck)?;
    if slot < SAMPLE_SLOTS {
        Ok(())
    } else {
        Err(AppError::Other(format!("no sampler slot {}", slot + 1)))
    }
}

/// Plays a slot from its start (on the next beat with quantize on), as loud as its pad was
/// hit (`velocity` 0-1) times the sampler's level, or at that level.
pub fn trigger_sample(state: &AppState, deck: usize, slot: usize, velocity: Option<f32>) -> AppResult<()> {
    check_slot(deck, slot)?;
    let gain = velocity.unwrap_or(1.0).clamp(0.0, 1.0);
    state.send(Command::TriggerSample { deck, slot, gain })
}

/// Puts the sound of `path` in a slot, read first; kept for the next start.
pub fn load_sample(state: &AppState, deck: usize, slot: usize, path: PathBuf) -> AppResult<()> {
    check_slot(deck, slot)?;
    let audio = sampler::decode(&path)?;
    let saved = path.display().to_string();
    sampler::put(state, deck, slot, path, audio)?;
    state.update_preferences(|p| {
        let samples = &mut p.decks[deck].samples;
        samples.resize(SAMPLE_SLOTS, None);
        samples[slot] = Some(saved);
    });
    Ok(())
}

/// Empties a slot; kept for the next start.
pub fn clear_sample(state: &AppState, deck: usize, slot: usize) -> AppResult<()> {
    check_slot(deck, slot)?;
    lock(&state.samplers)[deck][slot] = None;
    state.update_preferences(|p| {
        let samples = &mut p.decks[deck].samples;
        if let Some(saved) = samples.get_mut(slot) {
            *saved = None;
        }
        while samples.last() == Some(&None) {
            samples.pop();
        }
    });
    state.send_if_running(Command::SetSample { deck, slot, sample: None })
}

pub fn stop_sample(state: &AppState, deck: usize, slot: usize) -> AppResult<()> {
    check_slot(deck, slot)?;
    state.send(Command::StopSample { deck, slot })
}

/// The level of the deck's sampler (a gain of 0 or more); kept for the next start.
pub fn set_sampler_gain(state: &AppState, deck: usize, gain: f32) -> AppResult<()> {
    AppState::deck_exists(deck)?;
    if !(gain.is_finite() && gain >= 0.0) {
        return Err(AppError::Other("a sampler level is a gain of 0 or more".into()));
    }
    lock(&state.decks)[deck].sampler_gain = gain;
    state.update_preferences(|p| p.decks[deck].sampler_gain = gain);
    state.send_if_running(Command::SetSamplerGain { deck, gain })
}

/// The sound of `slot` stopped if it plays, else taken off its pad.
pub fn take_off(state: &AppState, deck: usize, slot: usize) -> AppResult<()> {
    if plays(state, deck, slot) {
        stop_sample(state, deck, slot)
    } else {
        clear_sample(state, deck, slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::bench::Bench;
    use prisme_media::TrackAudio;
    use std::sync::Arc;

    #[test]
    fn a_sound_taken_off_is_stopped_while_it_plays_then_taken_off_its_pad() {
        let bench = Bench::new();
        let audio = Arc::new(TrackAudio::from_interleaved(48_000, vec![0.0; 48_000]));
        sampler::put(&bench.state, 0, 2, "kick.wav".into(), audio).unwrap();
        trigger_sample(&bench.state, 0, 2, None).unwrap();
        bench.run(0.01);
        assert_eq!(bench.status(0).samples_playing, 1 << 2);
        take_off(&bench.state, 0, 2).unwrap();
        bench.run(0.01);
        assert_eq!(bench.status(0).samples_playing, 0, "stopped");
        assert!(lock(&bench.state.samplers)[0][2].is_some(), "still on its pad");
        take_off(&bench.state, 0, 2).unwrap();
        assert!(matches!(bench.sent()[..], [Command::SetSample { deck: 0, slot: 2, sample: None }]));
        assert!(lock(&bench.state.samplers)[0][2].is_none());
    }
}
