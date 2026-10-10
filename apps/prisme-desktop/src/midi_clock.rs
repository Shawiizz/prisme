//! MIDI clock out: 24 ticks per beat of the sync leader, so that a drum machine or a
//! sequencer plays along. The ticks follow the leader's beat position rather than a
//! timer: they stay in phase through tempo changes and nudges. Start goes out on a
//! beat, Stop when the leader stops.

use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState};
use midir::{MidiOutput, MidiOutputConnection};
use prisme_engine::EngineStatus;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager};

const TICKS_PER_BEAT: u32 = 24;
const POLL: Duration = Duration::from_millis(1);
const IDLE_POLL: Duration = Duration::from_millis(100);
/// Slower than this, the leader counts as stopped (record held, deck paused).
const MIN_SPEED: f64 = 0.1;

const TICK: u8 = 0xf8;
const START: u8 = 0xfa;
const STOP: u8 = 0xfc;

pub struct MidiClock {
    output: Mutex<Option<(String, MidiOutputConnection)>>,
}

impl MidiClock {
    pub fn new() -> Self {
        Self { output: Mutex::new(None) }
    }

    pub fn outputs() -> AppResult<Vec<String>> {
        let output = MidiOutput::new("Prisme").map_err(midi_error)?;
        Ok(output.ports().iter().filter_map(|p| output.port_name(p).ok()).collect())
    }

    pub fn output_name(&self) -> Option<String> {
        lock(&self.output).as_ref().map(|(name, _)| name.clone())
    }

    /// Sends the clock to `name`, or nowhere.
    pub fn set_output(&self, name: Option<&str>) -> AppResult<()> {
        let connection = name.map(|name| open(name).map(|c| (name.to_owned(), c))).transpose()?;
        if let Some((_, mut previous)) = std::mem::replace(&mut *lock(&self.output), connection) {
            let _ = previous.send(&[STOP]);
        }
        Ok(())
    }

    pub fn start(app: AppHandle) {
        std::thread::Builder::new()
            .name("prisme-midi-clock".into())
            .spawn(move || {
                let (clock, state) = (app.state::<MidiClock>(), app.state::<AppState>());
                let mut beats = Clock::default();
                let mut messages = Vec::new();
                loop {
                    if lock(&clock.output).is_none() {
                        beats = Clock::default();
                        std::thread::sleep(IDLE_POLL);
                        continue;
                    }
                    let phase =
                        lock(&state.session).as_mut().and_then(|s| leader_phase(&s.controller.status()));
                    beats.advance(phase, &mut messages);
                    if let Some((_, output)) = lock(&clock.output).as_mut() {
                        for message in messages.drain(..) {
                            let _ = output.send(&[message]);
                        }
                    }
                    messages.clear();
                    std::thread::sleep(POLL);
                }
            })
            .expect("cannot start the MIDI clock thread");
    }
}

/// Where the sync leader is in its beat, while it plays forwards.
fn leader_phase(status: &EngineStatus) -> Option<f64> {
    let leader = status.decks.iter().find(|d| d.sync.leader && d.loaded && d.speed >= MIN_SPEED)?;
    leader.beat_phase
}

/// Turns the leader's beat phase into clock messages.
#[derive(Debug, Default, PartialEq)]
enum Clock {
    #[default]
    Stopped,
    /// Waiting for the next beat to send Start.
    Starting {
        tick: u32,
    },
    Running {
        tick: u32,
    },
}

impl Clock {
    fn advance(&mut self, phase: Option<f64>, out: &mut Vec<u8>) {
        let Some(phase) = phase else {
            if matches!(self, Clock::Running { .. }) {
                out.push(STOP);
            }
            *self = Clock::Stopped;
            return;
        };
        let tick = ((phase * f64::from(TICKS_PER_BEAT)) as u32).min(TICKS_PER_BEAT - 1);
        *self = match *self {
            Clock::Stopped => Clock::Starting { tick },
            Clock::Starting { tick: last } if tick < last => {
                out.extend([START, TICK]);
                Clock::Running { tick }
            }
            Clock::Starting { .. } => Clock::Starting { tick },
            Clock::Running { tick: last } => {
                let ahead = (tick + TICKS_PER_BEAT - last) % TICKS_PER_BEAT;
                // more than half a beat ahead is a step back: wait until the phase is past
                if ahead > TICKS_PER_BEAT / 2 {
                    return;
                }
                out.extend(std::iter::repeat_n(TICK, ahead as usize));
                Clock::Running { tick }
            }
        };
    }
}

fn open(name: &str) -> AppResult<MidiOutputConnection> {
    let output = MidiOutput::new("Prisme").map_err(midi_error)?;
    let port = output
        .ports()
        .into_iter()
        .find(|p| output.port_name(p).is_ok_and(|n| n == name))
        .ok_or_else(|| AppError::ControllerUnplugged(name.to_owned()))?;
    output.connect(&port, "Prisme clock").map_err(midi_error)
}

fn midi_error(e: impl std::fmt::Display) -> AppError {
    AppError::Midi(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Messages sent while the phase goes through `phases`.
    fn run(clock: &mut Clock, phases: &[Option<f64>]) -> Vec<u8> {
        let mut out = Vec::new();
        for &phase in phases {
            clock.advance(phase, &mut out);
        }
        out
    }

    fn sweep(from: f64, to: f64, steps: usize) -> Vec<Option<f64>> {
        (0..=steps).map(|i| Some((from + (to - from) * i as f64 / steps as f64).rem_euclid(1.0))).collect()
    }

    #[test]
    fn starts_on_the_next_beat_then_ticks_24_times_a_beat() {
        let mut clock = Clock::default();
        assert!(run(&mut clock, &sweep(0.3, 0.99, 50)).is_empty());
        let out = run(&mut clock, &sweep(1.0, 3.0, 500));
        assert_eq!(&out[..2], [START, TICK]);
        assert_eq!(out[2..].len(), 2 * 24);
        assert!(out[2..].iter().all(|&m| m == TICK));
    }

    #[test]
    fn skipped_ticks_are_caught_up() {
        let mut clock = Clock::Running { tick: 0 };
        assert_eq!(run(&mut clock, &[Some(5.5 / 24.0)]), [TICK; 5]);
    }

    #[test]
    fn a_step_back_sends_nothing_until_the_phase_is_past_again() {
        let mut clock = Clock::Running { tick: 10 };
        assert!(run(&mut clock, &[Some(8.2 / 24.0), Some(9.9 / 24.0)]).is_empty());
        assert_eq!(run(&mut clock, &[Some(11.1 / 24.0)]), [TICK]);
    }

    #[test]
    fn stops_when_the_leader_stops() {
        let mut clock = Clock::Running { tick: 3 };
        assert_eq!(run(&mut clock, &[None, None]), [STOP]);
        assert_eq!(clock, Clock::Stopped);
    }
}
