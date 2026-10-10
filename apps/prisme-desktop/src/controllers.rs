//! MIDI controllers. A mapped control's messages become `controller-input` events for
//! the UI, which performs the actions and says which lights are on. Mappings are learnt
//! from the controller itself and kept in `controllers.json`, with the controllers in use.

use crate::dto::{ControllerInputDto, LightDto};
use crate::error::{AppError, AppResult};
use crate::state::lock;
use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use prisme_control::{ControlKind, Encoding, Event, Mapping, Mappings};
use prisme_session::JsonFile;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

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

/// The next control moved will drive `target`.
struct Learning {
    target: String,
    encoding: Encoding,
}

#[derive(Debug, PartialEq)]
enum Routed {
    Learned(Mapping),
    Input(ControllerInputDto),
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
    connections: Mutex<Vec<Connection>>,
}

impl Controllers {
    pub fn new(setup: JsonFile<ControllerSetup>) -> Self {
        Self { setup, learning: Mutex::new(None), connections: Mutex::new(Vec::new()) }
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
                eprintln!("controller {device} not opened: {e}");
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

    pub fn learn(&self, target: String, encoding: Encoding) {
        *lock(&self.learning) = Some(Learning { target, encoding });
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
                move |_, bytes, _| {
                    if let Some(event) = prisme_control::parse(bytes) {
                        app.state::<Controllers>().receive(&app, &name, event);
                    }
                },
                (),
            )
            .map_err(midi_error)?;
        let output = open_output(device);
        lock(&self.connections).push(Connection { device: device.to_owned(), _input: connection, output });
        Ok(())
    }

    fn receive(&self, app: &AppHandle, device: &str, event: Event) {
        let _ = match self.route(device, event) {
            Some(Routed::Learned(mapping)) => app.emit("controller-learned", mapping),
            Some(Routed::Input(input)) => app.emit("controller-input", input),
            None => Ok(()),
        };
    }

    /// Learns the control if a target waits for one, else finds what it drives.
    fn route(&self, device: &str, event: Event) -> Option<Routed> {
        // a release is the end of the previous press, not the control to learn
        let released = event.control.kind == ControlKind::Note && event.raw == 0;
        let learning = lock(&self.learning).take_if(|_| !released);
        if let Some(Learning { target, encoding }) = learning {
            let mapping = Mapping { device: device.to_owned(), control: event.control, target, encoding };
            self.save(|s| s.mappings.learn(mapping.clone()));
            return Some(Routed::Learned(mapping));
        }
        self.setup.read(|s| {
            let (target, value) = s.mappings.resolve(device, event)?;
            Some(Routed::Input(ControllerInputDto { target: target.to_owned(), value }))
        })
    }

    fn save(&self, change: impl FnOnce(&mut ControllerSetup)) {
        if let (_, Err(e)) = self.setup.update(change) {
            eprintln!("controllers not saved: {e}");
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

    #[test]
    fn a_learnt_control_then_drives_its_target() {
        let c = controllers();
        c.learn("deck1.play".into(), Encoding::Absolute);
        // the release of a press made before Learn is not learnt
        assert_eq!(c.route("DDJ", parse(&[0x80, 40, 0]).unwrap()), None);
        let learnt = c.route("DDJ", parse(&[0x90, 36, 127]).unwrap());
        let mapping = Mapping {
            device: "DDJ".into(),
            control: PAD,
            target: "deck1.play".into(),
            encoding: Encoding::Absolute,
        };
        assert_eq!(learnt, Some(Routed::Learned(mapping)));
        assert_eq!(c.route("DDJ", parse(&[0x80, 36, 0]).unwrap()), input("deck1.play", 0.0));
        assert_eq!(c.route("DDJ", parse(&[0x90, 36, 127]).unwrap()), input("deck1.play", 1.0));
        assert_eq!(c.route("Other", parse(&[0x90, 36, 127]).unwrap()), None);
    }

    #[test]
    fn encoders_are_learnt_relative() {
        let c = controllers();
        c.learn("library.browse".into(), Encoding::Relative);
        c.route("DDJ", parse(&[0xb0, 64, 1]).unwrap());
        assert_eq!(c.route("DDJ", parse(&[0xb0, 64, 127]).unwrap()), input("library.browse", -1.0));
    }

    #[test]
    fn learning_can_be_cancelled() {
        let c = controllers();
        c.learn("deck1.play".into(), Encoding::Absolute);
        c.cancel_learning();
        assert_eq!(c.route("DDJ", parse(&[0x90, 36, 127]).unwrap()), None);
        assert!(c.setup().mappings.all().is_empty());
    }
}
