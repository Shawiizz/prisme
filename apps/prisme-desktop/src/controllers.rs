//! MIDI controllers. A mapped control's messages become `controller-input` events for
//! the UI, which performs the actions and says which lights are on, except for the jog
//! wheels and tempo faders, which drive the engine from here (`controller_direct`).
//! Mappings are learnt from the controller itself and kept in `controllers.json`, with the
//! controllers in use.

use crate::controller_direct::{Direct, Performer, Tempo};
use crate::dto::{ControllerInputDto, DeckPitchDto, LightDto};
use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState};
use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use prisme_control::{ControlKind, Encoding, Event, Mapping, Mappings, Reader, SHIFT};
use prisme_engine::Command;
use prisme_session::JsonFile;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tracing::{error, warn};

/// The low half of a 14-bit control follows its high half within this.
const FINE_HALF_WITHIN: Duration = Duration::from_millis(100);

const CLIENT: &str = "Prisme";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ControllerSetup {
    /// MIDI ports in use: Windows lets a single program open a port, so none is taken
    /// unless the DJ asks.
    pub enabled: Vec<String>,
    pub mappings: Mappings,
    /// Where the MIDI clock goes (see `midi_clock`).
    pub clock_output: Option<String>,
}

/// The next control moved will drive `target`; an encoder's steps are read as it sends
/// its first one.
struct Learning {
    target: String,
    encoder: bool,
}

#[derive(Debug, PartialEq)]
enum Routed {
    Learned(Mapping),
    Input(ControllerInputDto),
    Direct(Direct, f32),
}

struct Connection {
    device: String,
    _input: MidiInputConnection<()>,
    /// Lights; not every controller has them.
    output: Option<MidiOutputConnection>,
}

pub struct Controllers {
    setup: JsonFile<ControllerSetup>,
    learning: Mutex<Option<Learning>>,
    /// The mapping just learnt: it becomes a 14-bit one if its low half follows.
    just_learnt: Mutex<Option<(Mapping, Instant)>>,
    reader: Mutex<Reader>,
    performer: Mutex<Performer>,
    connections: Mutex<Vec<Connection>>,
}

impl Controllers {
    pub fn new(setup: JsonFile<ControllerSetup>) -> Self {
        Self {
            setup,
            learning: Mutex::new(None),
            just_learnt: Mutex::new(None),
            reader: Mutex::new(Reader::default()),
            performer: Mutex::new(Performer::default()),
            connections: Mutex::new(Vec::new()),
        }
    }

    /// The MIDI ports plugged in.
    pub fn plugged() -> AppResult<Vec<String>> {
        let input = MidiInput::new(CLIENT).map_err(midi_error)?;
        Ok(input.ports().iter().filter_map(|p| input.port_name(p).ok()).collect())
    }

    pub fn setup(&self) -> ControllerSetup {
        self.setup.get()
    }

    /// Opens the controllers in use that are plugged in.
    pub fn open_enabled(&self, app: &AppHandle) {
        for device in self.setup.read(|s| s.enabled.clone()) {
            if let Err(e) = self.open(app, &device) {
                warn!("controller {device} not opened: {e}");
            }
        }
    }

    pub fn set_enabled(&self, app: &AppHandle, device: &str, enabled: bool) -> AppResult<()> {
        if enabled {
            self.open(app, device)?;
        } else {
            lock(&self.connections).retain(|c| c.device != device);
        }
        self.save(|s| {
            s.enabled.retain(|d| d != device);
            if enabled {
                s.enabled.push(device.to_owned());
            }
        });
        Ok(())
    }

    pub fn learn(&self, target: String, encoder: bool) {
        *lock(&self.learning) = Some(Learning { target, encoder });
    }

    /// Changes how the control of `target` on `device` is read (an encoder's steps).
    pub fn set_encoding(&self, device: &str, target: &str, encoding: Encoding) {
        self.save(|s| s.mappings.set_encoding(device, target, encoding));
    }

    pub fn cancel_learning(&self) {
        *lock(&self.learning) = None;
    }

    pub fn remember_clock_output(&self, name: Option<String>) {
        self.save(|s| s.clock_output = name);
    }

    pub fn forget(&self, target: &str) {
        self.save(|s| s.mappings.forget(target));
    }

    /// Switches the lights of the controls mapped to the `lights` targets.
    pub fn set_lights(&self, lights: &[LightDto]) {
        let mut connections = lock(&self.connections);
        self.setup.read(|s| {
            for mapping in s.mappings.all() {
                let Some(light) = lights.iter().find(|l| l.target == mapping.target) else { continue };
                let Some(message) = prisme_control::light(mapping.control, light.on) else { continue };
                let output = connections
                    .iter_mut()
                    .find(|c| c.device == mapping.device)
                    .and_then(|c| c.output.as_mut());
                if let Some(output) = output {
                    let _ = output.send(&message);
                }
            }
        });
    }

    fn open(&self, app: &AppHandle, device: &str) -> AppResult<()> {
        if lock(&self.connections).iter().any(|c| c.device == device) {
            return Ok(());
        }
        let mut input = MidiInput::new(CLIENT).map_err(midi_error)?;
        input.ignore(Ignore::All);
        let port = input
            .ports()
            .into_iter()
            .find(|p| input.port_name(p).is_ok_and(|name| name == device))
            .ok_or_else(|| AppError::ControllerUnplugged(device.to_owned()))?;
        let (app, name) = (app.clone(), device.to_owned());
        let connection = input
            .connect(
                &port,
                CLIENT,
                move |stamp_us, bytes, _| {
                    if let Some(event) = prisme_control::parse(bytes) {
                        let at_s = stamp_us as f64 / 1e6;
                        app.state::<Controllers>().receive(&app, &name, event, at_s);
                    }
                },
                (),
            )
            .map_err(midi_error)?;
        let output = open_output(device);
        lock(&self.connections).push(Connection { device: device.to_owned(), _input: connection, output });
        Ok(())
    }

    /// An event of `device`, at `at_s` on its clock.
    fn receive(&self, app: &AppHandle, device: &str, event: Event, at_s: f64) {
        let _ = match self.route(device, event) {
            Some(Routed::Learned(mapping)) => app.emit("controller-learned", mapping),
            Some(Routed::Input(input)) => app.emit("controller-input", input),
            Some(Routed::Direct(action, value)) => {
                self.perform(app, action, value, at_s);
                Ok(())
            }
            None => Ok(()),
        };
    }

    /// Learns the control if a target waits for one, else finds what it drives.
    fn route(&self, device: &str, event: Event) -> Option<Routed> {
        if let Some(learnt) = self.complete_fine(device, event) {
            return Some(Routed::Learned(learnt));
        }
        let mut reader = lock(&self.reader);
        let shift = self
            .setup
            .read(|s| s.mappings.find(device, event.control, false).is_some_and(|m| m.target == SHIFT));
        // a release is the end of the previous press, not the control to learn; nor is shift
        let released = event.control.kind == ControlKind::Note && event.raw == 0;
        let learning = lock(&self.learning).take_if(|_| !released && !shift);
        if let Some(Learning { target, encoder }) = learning {
            let mapping = Mapping {
                device: device.to_owned(),
                control: event.control,
                target,
                encoding: if encoder { Encoding::guess(event.raw) } else { Encoding::Absolute },
                fine: None,
                shift: reader.shifted(device),
            };
            self.save(|s| s.mappings.learn(mapping.clone()));
            *lock(&self.just_learnt) = Some((mapping.clone(), Instant::now()));
            return Some(Routed::Learned(mapping));
        }
        self.setup.read(|s| {
            let (mapping, value) = reader.read(&s.mappings, device, event)?;
            Some(match Direct::of(&mapping.target) {
                Some(action) => Routed::Direct(action, value),
                None => Routed::Input(ControllerInputDto { target: mapping.target.clone(), value }),
            })
        })
    }

    /// A fader just learnt whose low half follows on the controller numbered 32 higher is
    /// a 14-bit one: it is learnt as such.
    fn complete_fine(&self, device: &str, event: Event) -> Option<Mapping> {
        let mut just_learnt = lock(&self.just_learnt);
        let (mapping, at) =
            just_learnt.take_if(|(m, at)| m.device == device && at.elapsed() < FINE_HALF_WITHIN)?;
        let coarse = mapping.control;
        let fine = coarse.number + 32;
        let follows = coarse.kind == ControlKind::ControlChange
            && event.control.kind == ControlKind::ControlChange
            && coarse.number < 32
            && event.control.channel == coarse.channel
            && event.control.number == fine
            && mapping.encoding == Encoding::Absolute;
        if !follows {
            *just_learnt = Some((mapping, at));
            return None;
        }
        self.save(|s| s.mappings.make_fine(&mapping, fine));
        Some(Mapping { fine: Some(fine), ..mapping })
    }

    /// Drives the engine for a direct action; the UI hears of a new tempo.
    fn perform(&self, app: &AppHandle, action: Direct, value: f32, at_s: f64) {
        let state = app.state::<AppState>();
        let deck = match action {
            Direct::Jog(d) | Direct::JogTouch(d) | Direct::Pitch(d) => d,
        };
        if AppState::check_deck(deck).is_err() {
            return;
        }
        let tempo = Tempo {
            ratio: lock(&state.decks)[deck].pitch,
            range: state.preferences.read(|p| p.dj.pitch_range),
        };
        let Some(command) = lock(&self.performer).command(action, value, at_s, tempo) else { return };
        if let Command::SetPitch { ratio, .. } = command {
            lock(&state.decks)[deck].pitch = ratio;
            let _ = app.emit("deck-pitch", DeckPitchDto { deck, ratio });
        }
        if let Err(e) = state.send_if_running(command) {
            warn!("controller action on deck {} lost: {e}", deck + 1);
        }
    }

    fn save(&self, change: impl FnOnce(&mut ControllerSetup)) {
        if let (_, Err(e)) = self.setup.update(change) {
            error!("controllers not saved: {e}");
        }
    }
}

/// The output of the same name, for the lights.
fn open_output(device: &str) -> Option<MidiOutputConnection> {
    let output = MidiOutput::new(CLIENT).ok()?;
    let port = output.ports().into_iter().find(|p| output.port_name(p).is_ok_and(|name| name == device))?;
    output.connect(&port, CLIENT).ok()
}

fn midi_error(e: impl std::fmt::Display) -> AppError {
    AppError::Midi(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use prisme_control::{parse, Control};

    const PAD: Control = Control { kind: ControlKind::Note, channel: 0, number: 36 };

    fn controllers() -> Controllers {
        Controllers::new(JsonFile::in_memory(ControllerSetup::default()))
    }

    fn input(target: &str, value: f32) -> Option<Routed> {
        Some(Routed::Input(ControllerInputDto { target: target.into(), value }))
    }

    fn mapping(control: Control, target: &str) -> Mapping {
        Mapping {
            device: "DDJ".into(),
            control,
            target: target.into(),
            encoding: Encoding::Absolute,
            fine: None,
            shift: false,
        }
    }

    fn route(c: &Controllers, bytes: &[u8]) -> Option<Routed> {
        c.route("DDJ", parse(bytes).unwrap())
    }

    #[test]
    fn a_learnt_control_then_drives_its_target() {
        let c = controllers();
        c.learn("deck1.play".into(), false);
        // the release of a press made before Learn is not learnt
        assert_eq!(c.route("DDJ", parse(&[0x80, 40, 0]).unwrap()), None);
        let learnt = c.route("DDJ", parse(&[0x90, 36, 127]).unwrap());
        assert_eq!(learnt, Some(Routed::Learned(mapping(PAD, "deck1.play"))));
        assert_eq!(c.route("DDJ", parse(&[0x80, 36, 0]).unwrap()), input("deck1.play", 0.0));
        assert_eq!(c.route("DDJ", parse(&[0x90, 36, 127]).unwrap()), input("deck1.play", 1.0));
        assert_eq!(c.route("Other", parse(&[0x90, 36, 127]).unwrap()), None);
    }

    #[test]
    fn encoders_are_learnt_with_their_kind_of_steps() {
        let c = controllers();
        c.learn("library.browse".into(), true);
        route(&c, &[0xb0, 64, 1]);
        assert_eq!(route(&c, &[0xb0, 64, 127]), input("library.browse", -1.0));
        // a jog sending steps around 64
        c.learn("deck1.jog".into(), true);
        route(&c, &[0xb0, 33, 65]);
        assert_eq!(route(&c, &[0xb0, 33, 61]), Some(Routed::Direct(Direct::Jog(0), -3.0)));
    }

    #[test]
    fn a_fader_sending_its_low_half_is_learnt_14_bit() {
        let c = controllers();
        c.learn("deck1.pitch".into(), false);
        route(&c, &[0xb0, 0, 64]);
        let fine = route(&c, &[0xb0, 32, 0]);
        let tempo = Control { kind: ControlKind::ControlChange, channel: 0, number: 0 };
        assert_eq!(fine, Some(Routed::Learned(Mapping { fine: Some(32), ..mapping(tempo, "deck1.pitch") })));
        assert_eq!(route(&c, &[0xb0, 0, 64]), None);
        assert_eq!(route(&c, &[0xb0, 32, 64]), Some(Routed::Direct(Direct::Pitch(0), 8256.0 / 16383.0)));
    }

    #[test]
    fn shift_held_while_learning_learns_the_second_layer() {
        let c = controllers();
        c.learn(SHIFT.into(), false);
        route(&c, &[0x90, 63, 127]);
        route(&c, &[0x90, 63, 0]);
        c.learn("deck1.cue".into(), false);
        route(&c, &[0x90, 36, 127]);
        c.learn("deck1.hotcue1.delete".into(), false);
        // shift is pressed first: it is not what is learnt
        assert_eq!(route(&c, &[0x90, 63, 127]), None);
        let learnt = route(&c, &[0x90, 36, 127]);
        assert_eq!(
            learnt,
            Some(Routed::Learned(Mapping { shift: true, ..mapping(PAD, "deck1.hotcue1.delete") }))
        );
        route(&c, &[0x90, 63, 0]);
        assert_eq!(route(&c, &[0x90, 36, 127]), input("deck1.cue", 1.0));
    }

    #[test]
    fn learning_can_be_cancelled() {
        let c = controllers();
        c.learn("deck1.play".into(), false);
        c.cancel_learning();
        assert_eq!(c.route("DDJ", parse(&[0x90, 36, 127]).unwrap()), None);
        assert!(c.setup().mappings.all().is_empty());
    }
}
