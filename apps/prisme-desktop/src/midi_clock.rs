//! MIDI clock out: 24 ticks per beat of the sync leader, so that a drum machine or a
//! sequencer plays along. The ticks follow the leader's beat position rather than a
//! timer: they stay in phase through tempo changes and nudges. Start goes out on a
//! beat, Stop when the leader stops. The output chosen is let go when it is unplugged and
//! opened again once it is back.

use crate::controllers::port_of;
use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState};
use midir::{MidiOutput, MidiOutputConnection};
use prisme_engine::EngineStatus;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tracing::{info, warn};

const CLIENT: &str = "Prisme";
const TICKS_PER_BEAT: u32 = 24;
const POLL: Duration = Duration::from_millis(1);
const IDLE_POLL: Duration = Duration::from_millis(100);
/// How often the ports are looked at for the output chosen.
const WATCH_EVERY: Duration = Duration::from_millis(500);
/// Slower than this, the leader counts as stopped (record held, deck paused).
const MIN_SPEED: f64 = 0.1;

const TICK: u8 = 0xf8;
const START: u8 = 0xfa;
const STOP: u8 = 0xfc;

pub struct MidiClock {
    /// The output the DJ chose, plugged in or not.
    chosen: Mutex<Option<String>>,
    output: Mutex<Option<Output>>,
}

/// An output open, and the clock it is sent.
struct Output {
    /// Its port as plugged in now: the name chosen, or with a number Windows added.
    port: String,
    connection: MidiOutputConnection,
    clock: Clock,
}

impl Output {
    fn new((port, connection): (String, MidiOutputConnection)) -> Self {
        Self { port, connection, clock: Clock::default() }
    }

    /// Closed, not only dropped: on Android a port dropped keeps its device open.
    fn close(self) {
        self.connection.close();
    }
}

impl MidiClock {
    /// The clock, sent to `chosen` once it is plugged in.
    pub fn new(chosen: Option<String>) -> Self {
        Self { chosen: Mutex::new(chosen), output: Mutex::new(None) }
    }

    pub fn outputs() -> AppResult<Vec<String>> {
        Ok(port_names(&MidiOutput::new(CLIENT).map_err(midi_error)?))
    }

    /// The port the clock goes to, else the output chosen, unplugged.
    pub fn output_name(&self) -> Option<String> {
        let chosen = lock(&self.chosen);
        lock(&self.output).as_ref().map(|o| o.port.clone()).or_else(|| chosen.clone())
    }

    /// Sends the clock to `name`, or nowhere.
    pub fn set_output(&self, name: Option<&str>) -> AppResult<()> {
        let mut chosen = lock(&self.chosen);
        let output = name.map(|name| open(name).map(Output::new)).transpose()?;
        if let Some(mut previous) = std::mem::replace(&mut *lock(&self.output), output) {
            let _ = previous.connection.send(&[STOP]);
            previous.close();
        }
        *chosen = name.map(str::to_owned);
        Ok(())
    }

    pub fn start(app: AppHandle) {
        let watched = app.clone();
        std::thread::Builder::new()
            .name("prisme-midi-clock".into())
            .spawn(move || {
                let (clock, state) = (app.state::<MidiClock>(), app.state::<AppState>());
                let mut messages = Vec::new();
                loop {
                    if lock(&clock.output).is_none() {
                        std::thread::sleep(IDLE_POLL);
                        continue;
                    }
                    let phase =
                        lock(&state.session).as_mut().and_then(|s| leader_phase(&s.controller.status()));
                    clock.tick(phase, &mut messages);
                    std::thread::sleep(POLL);
                }
            })
            .expect("cannot start the MIDI clock thread");
        std::thread::Builder::new()
            .name("prisme-midi-clock-watch".into())
            .spawn(move || {
                let clock = watched.state::<MidiClock>();
                // one client for looking, kept, as the controllers' (see `Controllers::watch`)
                let mut looking: Option<MidiOutput> = None;
                let mut told = false;
                loop {
                    std::thread::sleep(WATCH_EVERY);
                    if lock(&clock.chosen).is_none() {
                        continue;
                    }
                    if looking.is_none() {
                        looking = MidiOutput::new(CLIENT).ok();
                    }
                    let Some(plugged) = looking.as_ref().map(port_names) else { continue };
                    match clock.follow(&plugged) {
                        Ok(()) => told = false,
                        Err(e) if !told => {
                            warn!("MIDI clock not sent to its output: {e}");
                            told = true;
                        }
                        Err(_) => {}
                    }
                }
            })
            .expect("cannot start the MIDI clock watch thread");
    }

    /// Sends the output what the leader at `phase` gives; an output that cannot be sent to is
    /// let go, to be opened again (see [`Self::follow`]).
    fn tick(&self, phase: Option<f64>, messages: &mut Vec<u8>) {
        let mut output = lock(&self.output);
        let Some(sent) = output.as_mut() else { return };
        sent.clock.advance(phase, messages);
        if let Err(e) = messages.drain(..).try_for_each(|m| sent.connection.send(&[m])) {
            warn!("MIDI clock output {} lost: {e}", sent.port);
            if let Some(lost) = output.take() {
                lost.close();
            }
        }
    }

    /// Of the ports `plugged`, keeps the output chosen open: let go once its port is gone,
    /// opened again once it is back.
    fn follow(&self, plugged: &[String]) -> AppResult<()> {
        let chosen = lock(&self.chosen);
        let mut output = lock(&self.output);
        if let Some(gone) = output.take_if(|o| !plugged.contains(&o.port)) {
            info!("MIDI clock output {} unplugged", gone.port);
            gone.close();
        }
        let Some(name) = chosen.as_deref() else { return Ok(()) };
        if output.is_none() && port_of(plugged, name, &[]).is_some() {
            let opened = Output::new(open(name)?);
            info!("MIDI clock sent to {} again", opened.port);
            *output = Some(opened);
        }
        Ok(())
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

/// Opens the output `name`, or the same plugged in elsewhere: its port and the connection.
fn open(name: &str) -> AppResult<(String, MidiOutputConnection)> {
    let output = MidiOutput::new(CLIENT).map_err(midi_error)?;
    let ports = output.ports();
    let names: Vec<String> = ports.iter().map(|p| output.port_name(p).unwrap_or_default()).collect();
    let at = port_of(&names, name, &[]).ok_or_else(|| AppError::ControllerUnplugged(name.to_owned()))?;
    let connection = output.connect(&ports[at], "Prisme clock").map_err(midi_error)?;
    Ok((names[at].clone(), connection))
}

fn port_names(output: &MidiOutput) -> Vec<String> {
    output.ports().iter().filter_map(|p| output.port_name(p).ok()).collect()
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
