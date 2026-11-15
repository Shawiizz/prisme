//! MIDI controllers. A mapped control's messages become `controller-input` events for
//! the UI, which performs the actions and tells the state of each (its lights show it),
//! except for the jog wheels and tempo faders, which drive the engine from here
//! (`controller_direct`). The lights that blink, the levels a controller shows and the
//! messages some models want again and again go out from a thread of their own.
//! Mappings are learnt from the controller itself and kept in `controllers.json`, with the
//! controllers in use; a model Prisme knows comes mapped by its profile
//! (`controller_profiles`), what is learnt on it taking the profile's place.

use crate::controller_direct::{Direct, Performer, Tempo};
use crate::controller_profiles;
use crate::dto::{ControllerInputDto, DeckPitchDto, LightDto};
use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState};
use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use prisme_audio_io::base_device_name;
use prisme_control::{
    selected_mode, Control, ControlKind, Encoding, Event, LightValue, Mapping, Mappings, Modes, Reader, OFF,
    ON, SHIFT,
};
use prisme_engine::Command;
use prisme_session::JsonFile;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tracing::{error, info, warn};

/// The low half of a 14-bit control follows its high half within this.
const FINE_HALF_WITHIN: Duration = Duration::from_millis(100);

const CLIENT: &str = "Prisme";
/// How often the controllers in use are looked for, to open again one plugged back: less
/// than a controller takes to come back on the USB bus, so that none goes unseen.
const WATCH_EVERY: Duration = Duration::from_millis(500);
/// On a system whose MIDI does not answer (no sequencer loaded, on Linux).
const WATCH_WITHOUT_MIDI_EVERY: Duration = Duration::from_secs(10);
/// How often blinking lights and keep-alive messages are seen to.
const OUTPUT_EVERY: Duration = Duration::from_millis(20);
/// How often the levels shown on the controllers are sent.
const METERS_EVERY: Duration = Duration::from_millis(40);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ControllerSetup {
    /// MIDI ports in use: Windows lets a single program open a port, so none is taken
    /// unless the DJ asks.
    pub enabled: Vec<String>,
    pub mappings: Mappings,
    /// Where the MIDI clock goes (see `midi_clock`).
    pub clock_output: Option<String>,
    /// Actions the DJ took off every controller: the profiles' controls no longer drive
    /// them either.
    pub unmapped: Vec<String>,
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
    /// The controller as saved.
    device: String,
    /// The port it was opened on: the same name, or with a number Windows added.
    port: String,
    _input: MidiInputConnection<()>,
    /// Lights; not every controller has them.
    output: Option<MidiOutputConnection>,
    /// When its keep-alive message last went.
    kept_alive: Instant,
    /// The value each light and meter shows, by its message's first two bytes.
    shown: HashMap<[u8; 2], u8>,
    /// The lights blinking, by their message's first two bytes.
    blinking: HashMap<[u8; 2], Blink>,
}

/// A light showing its two values in turn, since `since`.
struct Blink {
    values: [u8; 2],
    ms: u32,
    since: Instant,
}

impl Connection {
    /// Sends `message` unless it shows what is shown already.
    fn show(&mut self, [status, number, value]: [u8; 3]) {
        let Some(output) = self.output.as_mut() else { return };
        if self.shown.insert([status, number], value) != Some(value) {
            let _ = output.send(&[status, number, value]);
        }
    }

    /// Sets the light of `control` to `value`, blinking or not.
    fn light(&mut self, control: Control, value: LightValue) {
        let Some([status, number, _]) = prisme_control::light(control, 0) else { return };
        match value {
            LightValue::Steady(v) => {
                self.blinking.remove(&[status, number]);
                self.show([status, number, v]);
            }
            LightValue::Blink { blink, ms } => {
                let same =
                    self.blinking.get(&[status, number]).is_some_and(|b| b.values == blink && b.ms == ms);
                if !same {
                    self.blinking
                        .insert([status, number], Blink { values: blink, ms, since: Instant::now() });
                    self.show([status, number, blink[0]]);
                }
            }
        }
    }

    /// Each blinking light at the value of the moment.
    fn blink(&mut self) {
        let now: Vec<[u8; 3]> = self
            .blinking
            .iter()
            .map(|(&[status, number], b)| {
                let half = (b.since.elapsed().as_millis() / u128::from(b.ms.max(1))) % 2;
                [status, number, b.values[half as usize]]
            })
            .collect();
        for message in now {
            self.show(message);
        }
    }

    fn send(&mut self, message: &[u8]) {
        if let Some(output) = self.output.as_mut() {
            let _ = output.send(message);
        }
    }
}

pub struct Controllers {
    setup: JsonFile<ControllerSetup>,
    /// The profiles of the controllers in use, with what was learnt over them.
    mappings: Mutex<Mappings>,
    learning: Mutex<Option<Learning>>,
    /// The mapping just learnt: it becomes a 14-bit one if its low half follows.
    just_learnt: Mutex<Option<(Mapping, Instant)>>,
    reader: Mutex<Reader>,
    performer: Mutex<Performer>,
    connections: Mutex<Vec<Connection>>,
    /// The state of each action the UI last told (`on`, `off`, `playing`…).
    states: Mutex<HashMap<String, String>>,
}

impl Controllers {
    pub fn new(setup: JsonFile<ControllerSetup>) -> Self {
        let controllers = Self {
            setup,
            mappings: Mutex::new(Mappings::default()),
            learning: Mutex::new(None),
            just_learnt: Mutex::new(None),
            reader: Mutex::new(Reader::default()),
            performer: Mutex::new(Performer::default()),
            connections: Mutex::new(Vec::new()),
            states: Mutex::new(HashMap::new()),
        };
        controllers.layer();
        controllers
    }

    /// The MIDI ports plugged in.
    pub fn plugged() -> AppResult<Vec<String>> {
        Ok(port_names(&MidiInput::new(CLIENT).map_err(midi_error)?))
    }

    pub fn setup(&self) -> ControllerSetup {
        self.setup.get()
    }

    /// What drives what: the profiles of the controllers in use, and what was learnt.
    pub fn mappings(&self) -> Mappings {
        lock(&self.mappings).clone()
    }

    /// Opens the controllers in use that are plugged in.
    pub fn open_enabled(&self, app: &AppHandle) {
        for device in self.setup.read(|s| s.enabled.clone()) {
            if let Err(e) = self.open(app, &device) {
                warn!("controller {device} not opened: {e}");
            }
        }
    }

    /// Keeps the controllers in use open: one unplugged (a USB cable pulled, a hub that
    /// resets) is closed, and opened again as soon as it is back.
    pub fn watch(app: AppHandle) {
        let spawned = std::thread::Builder::new().name("prisme-midi-watch".into()).spawn(move || {
            let mut missing: Vec<String> = Vec::new();
            // one client for looking, kept: a new one each time would come and go in every
            // MIDI patchbay of the system
            let mut looking: Option<MidiInput> = None;
            loop {
                std::thread::sleep(if looking.is_some() { WATCH_EVERY } else { WATCH_WITHOUT_MIDI_EVERY });
                if looking.is_none() {
                    looking = MidiInput::new(CLIENT).ok();
                }
                let Some(plugged) = looking.as_ref().map(port_names) else { continue };
                let controllers = app.state::<Controllers>();
                let unplugged: Vec<String> = {
                    let mut connections = lock(&controllers.connections);
                    let gone = |c: &Connection| !plugged.contains(&c.port);
                    let devices = connections.iter().filter(|c| gone(c)).map(|c| c.device.clone()).collect();
                    connections.retain(|c| !gone(c));
                    devices
                };
                for device in &unplugged {
                    controllers.let_go(&app, device);
                }
                for device in controllers.setup.read(|s| s.enabled.clone()) {
                    let open = lock(&controllers.connections).iter().any(|c| c.device == device);
                    if open || port_of(&plugged, &device).is_none() {
                        continue;
                    }
                    match controllers.open(&app, &device) {
                        // turned off meanwhile
                        Ok(()) if !controllers.setup.read(|s| s.enabled.contains(&device)) => {
                            lock(&controllers.connections).retain(|c| c.device != device);
                        }
                        Ok(()) => {
                            missing.retain(|d| *d != device);
                            info!("controller {device} opened again");
                            // back with its lights off
                            let _ = app.emit("controller-opened", &device);
                        }
                        // told once, tried again
                        Err(e) if !missing.contains(&device) => {
                            warn!("controller {device} not opened again: {e}");
                            missing.push(device);
                        }
                        Err(_) => {}
                    }
                }
            }
        });
        if let Err(e) = spawned {
            error!("controllers not watched: {e}");
        }
    }

    pub fn set_enabled(&self, app: &AppHandle, device: &str, enabled: bool) -> AppResult<()> {
        if enabled {
            self.open(app, device)?;
        } else {
            self.close(device);
            self.let_go(app, device);
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

    /// Changes how the control of `target` on `device` is read (an encoder's steps). A
    /// control of a profile becomes the DJ's own, read that way.
    pub fn set_encoding(&self, device: &str, target: &str, encoding: Encoding) {
        let of_target = |m: &&Mapping| m.device == device && m.target == target;
        let profiled: Vec<Mapping> = lock(&self.mappings).all().iter().filter(of_target).cloned().collect();
        self.save(|s| {
            if !s.mappings.all().iter().any(|m| of_target(&m)) {
                for mapping in profiled {
                    s.mappings.learn(mapping, Direct::takes_several(target));
                }
            }
            s.mappings.set_encoding(device, target, encoding);
        });
    }

    pub fn cancel_learning(&self) {
        *lock(&self.learning) = None;
    }

    pub fn remember_clock_output(&self, name: Option<String>) {
        self.save(|s| s.clock_output = name);
    }

    pub fn forget(&self, target: &str) {
        self.save(|s| {
            s.mappings.forget(target);
            if !s.unmapped.iter().any(|t| t == target) {
                s.unmapped.push(target.to_owned());
            }
        });
    }

    /// The actions whose state the profiles of the controllers in use show on lights of
    /// their own.
    pub fn light_targets(&self) -> Vec<String> {
        let mut targets: Vec<String> = self
            .setup
            .read(|s| s.enabled.clone())
            .iter()
            .filter_map(|device| controller_profiles::of(device))
            .flat_map(|p| p.lights.iter().map(|l| l.target.clone()))
            .filter(|t| selected_mode(t).is_none())
            .collect();
        targets.sort();
        targets.dedup();
        targets
    }

    /// Keeps the state of the `lights` targets, and shows it on the controllers.
    pub fn set_lights(&self, lights: &[LightDto]) {
        let mut states = lock(&self.states);
        for light in lights {
            states.insert(light.target.clone(), light.state.clone());
        }
        drop(states);
        let devices: Vec<String> = lock(&self.connections).iter().map(|c| c.device.clone()).collect();
        for device in devices {
            self.show_lights(&device);
        }
    }

    /// Shows on `device`'s lights the state of their actions, in the modes it is in: its
    /// mapped controls', its mode keys', and the other lights its profile gives.
    fn show_lights(&self, device: &str) {
        let modes = lock(&self.reader).modes(device);
        let states = lock(&self.states);
        let state_of = |target: &str| match selected_mode(target) {
            Some((group, mode)) => Some(if modes.get(group).is_some_and(|m| m == mode) { ON } else { OFF }),
            None => states.get(target).map(String::as_str),
        };
        let mut lights: Vec<(Control, LightValue)> = Vec::new();
        for mapping in lock(&self.mappings).all().iter().filter(|m| m.device == device && m.applies(&modes)) {
            if let Some(state) = state_of(&mapping.target) {
                lights.push((mapping.control, mapping.light.value(state)));
            }
        }
        if let Some(profile) = controller_profiles::of(device) {
            for extra in &profile.lights {
                if let Some(value) = state_of(&extra.target).and_then(|s| extra.value(s, &modes)) {
                    lights.push((extra.control, value));
                }
            }
        }
        drop(states);
        if let Some(connection) = lock(&self.connections).iter_mut().find(|c| c.device == device) {
            for (control, value) in lights {
                connection.light(control, value);
            }
        }
    }

    /// Closes `device`: its lights go off, and its profile's last messages go out.
    fn close(&self, device: &str) {
        let connection = {
            let mut connections = lock(&self.connections);
            let at = connections.iter().position(|c| c.device == device);
            at.map(|at| connections.remove(at))
        };
        let Some(mut connection) = connection else { return };
        let mappings = lock(&self.mappings);
        for mapping in mappings.all().iter().filter(|m| m.device == device) {
            connection.light(mapping.control, mapping.light.value(OFF));
        }
        if let Some(profile) = controller_profiles::of(device) {
            for extra in &profile.lights {
                connection.light(extra.control, extra.values.value(OFF));
            }
            for message in &profile.close {
                connection.send(message);
            }
        }
    }

    /// Closes every controller, as Prisme quits.
    pub fn close_all(&self) {
        let devices: Vec<String> = lock(&self.connections).iter().map(|c| c.device.clone()).collect();
        for device in devices {
            self.close(&device);
        }
    }

    /// Sends what goes out by itself: the lights that blink, the keep-alive messages, and
    /// the levels the controllers show.
    pub fn drive_outputs(app: AppHandle) {
        let spawned = std::thread::Builder::new().name("prisme-midi-out".into()).spawn(move || {
            let mut metered = Instant::now();
            loop {
                std::thread::sleep(OUTPUT_EVERY);
                let controllers = app.state::<Controllers>();
                let levels = (metered.elapsed() >= METERS_EVERY).then(|| {
                    metered = Instant::now();
                    levels(&app.state::<AppState>())
                });
                for connection in lock(&controllers.connections).iter_mut() {
                    connection.blink();
                    let Some(profile) = controller_profiles::of(&connection.device) else { continue };
                    if let Some(keep) = &profile.keep_alive {
                        if connection.kept_alive.elapsed() >= Duration::from_millis(u64::from(keep.every_ms))
                        {
                            connection.kept_alive = Instant::now();
                            connection.send(&keep.message);
                        }
                    }
                    for meter in profile.meters.iter() {
                        let level = levels.as_ref().and_then(|l| l.get(&meter.source)).copied();
                        let message = prisme_control::light(meter.control, 0);
                        if let (Some(level), Some([status, number, _])) = (level, message) {
                            connection.show([status, number, meter.value(level)]);
                        }
                    }
                }
            }
        });
        if let Err(e) = spawned {
            error!("controller lights and levels not sent: {e}");
        }
    }

    fn open(&self, app: &AppHandle, device: &str) -> AppResult<()> {
        if lock(&self.connections).iter().any(|c| c.device == device) {
            return Ok(());
        }
        let mut input = MidiInput::new(CLIENT).map_err(midi_error)?;
        input.ignore(Ignore::All);
        let ports = input.ports();
        let names: Vec<String> = ports.iter().map(|p| input.port_name(p).unwrap_or_default()).collect();
        let at = port_of(&names, device).ok_or_else(|| AppError::ControllerUnplugged(device.to_owned()))?;
        let (port, port_name) = (&ports[at], names[at].clone());
        let (app, name) = (app.clone(), device.to_owned());
        let jog_steps_per_turn = controller_profiles::jog_steps_per_turn(device);
        let connection = input
            .connect(
                port,
                CLIENT,
                move |stamp_us, bytes, _| {
                    if let Some(event) = prisme_control::parse(bytes) {
                        let at_s = stamp_us as f64 / 1e6;
                        app.state::<Controllers>().receive(&app, &name, jog_steps_per_turn, event, at_s);
                    }
                },
                (),
            )
            .map_err(midi_error)?;
        let mut output = open_output(&port_name);
        if let (Some(output), Some(profile)) = (output.as_mut(), controller_profiles::of(device)) {
            for message in &profile.init {
                let _ = output.send(message);
            }
        }
        let connection = Connection {
            device: device.to_owned(),
            port: port_name,
            _input: connection,
            output,
            kept_alive: Instant::now(),
            shown: HashMap::new(),
            blinking: HashMap::new(),
        };
        let modes = controller_profiles::of(device).map(|p| p.first_modes()).unwrap_or_default();
        lock(&self.reader).set_modes(device, modes);
        lock(&self.connections).push(connection);
        self.show_lights(device);
        Ok(())
    }

    /// An event of `device`, whose jog wheels turn by `jog_steps_per_turn`, at `at_s` on its
    /// clock.
    fn receive(&self, app: &AppHandle, device: &str, jog_steps_per_turn: f64, event: Event, at_s: f64) {
        let _ = match self.route(device, event) {
            Some(Routed::Learned(mapping)) => app.emit("controller-learned", mapping),
            Some(Routed::Input(input)) => app.emit("controller-input", input),
            Some(Routed::Direct(action, value)) => {
                self.perform(app, action, value, at_s, jog_steps_per_turn);
                Ok(())
            }
            None => Ok(()),
        };
    }

    /// A controller gone lets go of what it held: its keys (shift among them) are released,
    /// and so are the jog wheels it touched, however they tell it.
    fn let_go(&self, app: &AppHandle, device: &str) {
        let jog_steps_per_turn = controller_profiles::jog_steps_per_turn(device);
        let held = lock(&self.reader).held(device);
        for control in held {
            self.receive(app, device, jog_steps_per_turn, Event { control, raw: 0 }, 0.0);
        }
        let touches: Vec<Direct> = lock(&self.mappings)
            .all()
            .iter()
            .filter(|m| m.device == device)
            .filter_map(|m| Direct::of(&m.target))
            .filter(|d| matches!(d, Direct::JogTouch(_)))
            .collect();
        for touch in touches {
            self.perform(app, touch, 0.0, 0.0, jog_steps_per_turn);
        }
    }

    /// Learns the control if a target waits for one, else finds what it drives.
    fn route(&self, device: &str, event: Event) -> Option<Routed> {
        if let Some(learnt) = self.complete_fine(device, event) {
            return Some(Routed::Learned(learnt));
        }
        let mut reader = lock(&self.reader);
        let shift = lock(&self.mappings)
            .find(device, event.control, false, &Modes::new())
            .is_some_and(|m| m.target == SHIFT);
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
                when: None,
                invert: false,
                light: Default::default(),
            };
            self.save(|s| {
                s.unmapped.retain(|t| *t != mapping.target);
                s.mappings.learn(mapping.clone(), Direct::takes_several(&mapping.target));
            });
            *lock(&self.just_learnt) = Some((mapping.clone(), Instant::now()));
            return Some(Routed::Learned(mapping));
        }
        let mappings = lock(&self.mappings);
        let changes = reader.mode_changes(device);
        let Some((mapping, value)) = reader.read(&mappings, device, event) else {
            if reader.mode_changes(device) != changes {
                drop((mappings, reader));
                self.show_lights(device);
            }
            return None;
        };
        Some(match Direct::of(&mapping.target) {
            Some(action) => Routed::Direct(action, value),
            None => Routed::Input(ControllerInputDto { target: mapping.target.clone(), value }),
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
    fn perform(&self, app: &AppHandle, action: Direct, value: f32, at_s: f64, jog_steps_per_turn: f64) {
        let state = app.state::<AppState>();
        let deck = match action {
            Direct::Jog(d) | Direct::JogTouch(d) | Direct::Pitch(d) => d,
        };
        if state.has_deck(deck).is_err() {
            return;
        }
        let tempo = Tempo {
            ratio: lock(&state.decks)[deck].pitch,
            range: state.preferences.read(|p| p.dj.pitch_range),
        };
        let Some(command) = lock(&self.performer).command(action, value, at_s, jog_steps_per_turn, tempo)
        else {
            return;
        };
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
        self.layer();
    }

    /// Lays what was learnt over the profiles of the controllers in use.
    fn layer(&self) {
        let layered = self.setup.read(|s| {
            let profiled: Vec<Mapping> = s
                .enabled
                .iter()
                .filter_map(|device| {
                    Some(controller_profiles::of(device)?.mappings(device).collect::<Vec<_>>())
                })
                .flatten()
                .collect();
            Mappings::layered(profiled, &s.mappings, &s.unmapped, Direct::takes_several)
        });
        *lock(&self.mappings) = layered;
    }
}

/// The level of each source of a meter (`deck1`…, `main`) at the moment; none when the audio
/// is not running.
fn levels(state: &AppState) -> HashMap<String, f32> {
    let mut session = lock(&state.session);
    let Some(session) = session.as_mut() else { return HashMap::new() };
    let status = session.controller.status();
    let decks = status.decks[..status.deck_count].iter().enumerate();
    decks
        .map(|(d, deck)| (format!("deck{}", d + 1), deck.peak))
        .chain([("main".to_owned(), status.main_peak)])
        .collect()
}

/// The output of the same name, for the lights.
fn open_output(device: &str) -> Option<MidiOutputConnection> {
    let output = MidiOutput::new(CLIENT).ok()?;
    let ports = output.ports();
    let names: Vec<String> = ports.iter().map(|p| output.port_name(p).unwrap_or_default()).collect();
    let port = &ports[port_of(&names, device)?];
    output.connect(port, CLIENT).ok()
}

fn port_names(input: &MidiInput) -> Vec<String> {
    input.ports().iter().filter_map(|p| input.port_name(p).ok()).collect()
}

/// Of the ports `names`, the one of the controller saved as `device`: its name, else the
/// same wherever it is plugged (see [`base_port_name`]).
fn port_of(names: &[String], device: &str) -> Option<usize> {
    names.iter().position(|n| n == device).or_else(|| {
        let base = base_port_name(device);
        names.iter().position(|n| base_port_name(n) == base)
    })
}

/// A port's name wherever its controller is plugged: without the numbers Windows puts in,
/// nor the address ALSA ends it with (`DDJ-400 MIDI 1 24:0`).
fn base_port_name(name: &str) -> String {
    let number = |n: &str| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit());
    let address =
        |end: &str| end.split_once(':').is_some_and(|(client, port)| number(client) && number(port));
    let name = match name.rsplit_once(' ') {
        Some((name, end)) if address(end) => name,
        _ => name,
    };
    base_device_name(name)
}

fn midi_error(e: impl std::fmt::Display) -> AppError {
    AppError::Midi(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_controller_is_found_under_another_alsa_address() {
        let plugged =
            ["Midi Through:Midi Through Port-0 14:0".to_owned(), "DDJ-400:DDJ-400 MIDI 1 28:0".to_owned()];
        assert_eq!(port_of(&plugged, "DDJ-400:DDJ-400 MIDI 1 24:0"), Some(1));
        assert_eq!(port_of(&plugged, "DDJ-400:DDJ-400 MIDI 2 24:0"), None);
    }

    #[test]
    fn a_controller_is_found_on_another_usb_port() {
        let names = ["2- DJControl Inpulse 200".to_owned(), "Other".to_owned()];
        assert_eq!(port_of(&names, "DJControl Inpulse 200"), Some(0));
        let both = ["DJControl Inpulse 200".to_owned(), "2- DJControl Inpulse 200".to_owned()];
        assert_eq!(port_of(&both, "2- DJControl Inpulse 200"), Some(1), "the same name first");
        assert_eq!(port_of(&names, "FLX4"), None);
    }
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
            when: None,
            invert: false,
            light: Default::default(),
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

    fn sb2() -> Controllers {
        let c = controllers();
        c.save(|s| s.enabled.push("DDJ-SB2".into()));
        c
    }

    #[test]
    fn a_known_model_comes_mapped() {
        let c = sb2();
        let play = |c: &Controllers| c.route("DDJ-SB2", parse(&[0x90, 0x0b, 127]).unwrap());
        assert_eq!(play(&c), input("deck1.play", 1.0));
        // learnt over the profile, the control drives what was learnt
        c.learn("deck1.cue".into(), false);
        play(&c);
        assert_eq!(play(&c), input("deck1.cue", 1.0));
        // forgotten, an action leaves the profile's controls too
        c.forget("deck1.cue");
        c.forget("deck1.play");
        assert_eq!(play(&c), None);
        assert!(c.setup().unmapped.contains(&"deck1.play".to_owned()));
        // learnt again, it is mapped again
        c.learn("deck1.play".into(), false);
        play(&c);
        assert_eq!(play(&c), input("deck1.play", 1.0));
    }

    #[test]
    fn a_profiled_encoder_read_another_way_becomes_the_djs() {
        let c = sb2();
        let jog = |m: &Mapping| m.device == "DDJ-SB2" && m.target == "deck1.jog";
        assert!(c.setup().mappings.all().is_empty());
        c.set_encoding("DDJ-SB2", "deck1.jog", Encoding::Relative);
        let learnt = c.setup().mappings;
        assert!(!learnt.all().is_empty());
        assert!(learnt.all().iter().filter(|m| jog(m)).all(|m| m.encoding == Encoding::Relative));
        assert!(c.mappings().all().iter().filter(|m| jog(m)).all(|m| m.encoding == Encoding::Relative));
    }

    #[test]
    fn a_profile_waits_for_its_controller_to_be_in_use() {
        let c = controllers();
        assert_eq!(c.route("DDJ-SB2", parse(&[0x90, 0x0b, 127]).unwrap()), None);
        assert!(c.mappings().all().is_empty());
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
