//! MIDI controllers. A mapped control's messages become `controller-input` events for
//! the UI, which performs the actions and tells the state of each (its lights show it),
//! except for the jog wheels and tempo faders, which drive the engine from here
//! (`controller_direct`). The lights that blink, the levels a controller shows and the
//! messages some models want again and again go out from a thread of their own.
//! Mappings are learnt from the controller itself and kept in `controllers.json`, with the
//! controllers in use; a model Prisme knows comes mapped by its profile
//! (`controller_profiles`), what is learnt on it taking the profile's place.

use crate::auto_dj::{AutoDj, Takeover};
use crate::controller_direct::{Direct, Performer, Tempo};
use crate::controller_profiles;
use crate::dto::{ControllerInputDto, DeckPitchDto, LightDto, OutputDto};
use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState};
use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use prisme_audio_io::base_device_name;
use prisme_control::{
    nearest, selected_mode, Control, ControlKind, Encoding, Event, FaderSet, Hold, LightValue, Mapping,
    Mappings, Modes, Profile, ProfileControl, Reader, Shifts, JOG_STEPS_PER_TURN, OFF, ON, SHIFT,
};
use prisme_engine::Command;
use prisme_session::JsonFile;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
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
/// How long a meter shows its clip value after its level reached full scale.
const CLIP_SHOWN: Duration = Duration::from_secs(1);
/// The most steps of an encoder driving keys pressed for one of its messages.
const MOST_KEY_STEPS: i32 = 16;
/// The state a light shows while its key is held, if its profile gives it one.
const PRESSED: &str = "pressed";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ControllerSetup {
    /// MIDI ports in use: Windows lets a single program open a port, so none is taken
    /// unless the DJ asks.
    pub enabled: Vec<String>,
    pub mappings: Mappings,
    /// Where the MIDI clock goes (see `midi_clock`).
    pub clock_output: Option<String>,
    /// Actions the DJ took off a controller: its profile's controls no longer drive them.
    pub unmapped: Vec<Unmapped>,
}

/// An action taken off a controller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Unmapped {
    On {
        device: String,
        target: String,
    },
    /// Off every controller: what an action taken off was before it could be taken off one alone.
    Everywhere(String),
}

impl Unmapped {
    /// Whether `mapping` is taken off by this.
    fn covers(&self, mapping: &Mapping) -> bool {
        match self {
            Unmapped::On { device, target } => mapping.device == *device && mapping.target == *target,
            Unmapped::Everywhere(target) => mapping.target == *target,
        }
    }
}

/// The next control moved will drive `target`; an encoder's steps are read as it sends
/// its first one.
struct Learning {
    target: String,
    encoder: bool,
}

#[derive(Debug, PartialEq)]
enum Routed {
    Learned(Box<Mapping>),
    /// What the controller drives, in order.
    Actions(Vec<Action>),
}

/// An action a controller drives: one the UI performs, or one driving the engine from here.
#[derive(Debug, PartialEq)]
enum Action {
    Ui(ControllerInputDto),
    Direct(Direct, f32),
}

impl Action {
    /// `input` performed where its action is: a direct one here, whatever drove it.
    fn of(input: ControllerInputDto) -> Self {
        match Direct::of(&input.target) {
            Some(direct) => Action::Direct(direct, input.value),
            None => Action::Ui(input),
        }
    }
}

/// A key pressed whose action waits to know whether it is held: held for `hold.ms`, it drives
/// `hold.target` (`fired`), let go sooner its own.
struct Holding {
    device: String,
    control: Control,
    hold: Hold,
    since: Instant,
    /// When it was pressed, on its controller's clock.
    at_s: f64,
    fired: bool,
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
    lit: Lit,
    /// The messages of its profile to send later, and when.
    later: Vec<(Instant, Vec<u8>)>,
    /// When each of its meters last reached full scale.
    clipped: HashMap<usize, Instant>,
    /// What each of its screens' numbers was last sent, by its place among the displays.
    displayed: HashMap<usize, Vec<u8>>,
}

/// What a controller's lights and meters show, so that a message goes only for a change.
#[derive(Default)]
struct Lit {
    /// The value each one shows, by its message's first two bytes.
    shown: HashMap<[u8; 2], u8>,
    /// Those blinking, by their message's first two bytes.
    blinking: HashMap<[u8; 2], Blink>,
    /// Those shown by a message of their own, by their control's first two bytes.
    messages: HashMap<[u8; 2], Vec<u8>>,
}

/// A light showing its two values in turn, since `since`.
struct Blink {
    values: [u8; 2],
    ms: u32,
    since: Instant,
}

impl Lit {
    /// `message`, unless it shows what is shown already.
    fn show(&mut self, [status, number, value]: [u8; 3]) -> Option<[u8; 3]> {
        (self.shown.insert([status, number], value) != Some(value)).then_some([status, number, value])
    }

    /// The message setting the light of `control` to `value`, blinking or not, unless it
    /// shows it already.
    fn light(&mut self, control: Control, value: LightValue) -> Option<Vec<u8>> {
        let [status, number, _] = prisme_control::light(control, 0)?;
        if !matches!(value, LightValue::Message(_)) {
            self.messages.remove(&[status, number]);
        }
        match value {
            LightValue::Steady(v) => {
                self.blinking.remove(&[status, number]);
                self.show([status, number, v]).map(Vec::from)
            }
            LightValue::Message(message) => {
                self.blinking.remove(&[status, number]);
                self.shown.remove(&[status, number]);
                let before = self.messages.insert([status, number], message.clone());
                (before.as_ref() != Some(&message)).then_some(message)
            }
            LightValue::Blink { blink, ms } => {
                let same =
                    self.blinking.get(&[status, number]).is_some_and(|b| b.values == blink && b.ms == ms);
                if same {
                    return None;
                }
                self.blinking.insert([status, number], Blink { values: blink, ms, since: Instant::now() });
                self.show([status, number, blink[0]]).map(Vec::from)
            }
        }
    }

    /// The messages of the blinking lights whose value of the moment is not shown yet.
    fn blink(&mut self) -> Vec<[u8; 3]> {
        let now: Vec<[u8; 3]> = self
            .blinking
            .iter()
            .map(|(&[status, number], b)| {
                let half = (b.since.elapsed().as_millis() / u128::from(b.ms.max(1))) % 2;
                [status, number, b.values[half as usize]]
            })
            .collect();
        now.into_iter().filter_map(|message| self.show(message)).collect()
    }
}

impl Connection {
    /// Sets the light of `control` to `value`, blinking or not.
    fn light(&mut self, control: Control, value: LightValue) {
        if let Some(message) = self.lit.light(control, value) {
            self.send(&message);
        }
    }

    /// Each blinking light at the value of the moment.
    fn blink(&mut self) {
        for message in self.lit.blink() {
            self.send(&message);
        }
    }

    fn send(&mut self, message: &[u8]) {
        if let Some(output) = self.output.as_mut() {
            let _ = output.send(message);
        }
    }

    /// Sends the messages of its profile whose time came.
    fn send_due(&mut self) {
        let now = Instant::now();
        let (due, later): (Vec<_>, Vec<_>) =
            std::mem::take(&mut self.later).into_iter().partition(|(at, _)| *at <= now);
        self.later = later;
        for (_, message) in due {
            self.send(&message);
        }
    }
}

/// The state of an action the UI last told, and the colour it shows.
struct Shown {
    state: String,
    color: Option<String>,
}

/// What the keys of the controllers do beyond driving their action, by controller and key.
#[derive(Default)]
struct Keys {
    /// Keys latched down.
    latched: HashSet<(String, Control)>,
    /// Keys putting a fader at two positions in turn, at their second.
    second: HashSet<(String, Control)>,
    /// When each key was last pressed.
    pressed_at: HashMap<(String, Control), Instant>,
    /// Keys whose press drove another action than their own (pressed twice, or with another
    /// key held): their release lets that one go.
    instead: HashMap<(String, Control), String>,
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
    states: Mutex<HashMap<String, Shown>>,
    /// The value of each source of the profiles' outputs the UI last told.
    values: Mutex<HashMap<String, f32>>,
    holdings: Mutex<Vec<Holding>>,
    keys: Mutex<Keys>,
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
            values: Mutex::new(HashMap::new()),
            holdings: Mutex::new(Vec::new()),
            keys: Mutex::new(Keys::default()),
        };
        controllers.layer();
        controllers
    }

    /// The profiles were read again: what they map, and the lights they give.
    pub fn profiles_changed(&self) {
        self.layer();
        let devices: Vec<String> = lock(&self.connections).iter().map(|c| c.device.clone()).collect();
        for device in devices {
            self.show_lights(&device);
        }
    }

    /// What `device` drives now, as the profile of its model: its own profile's, what was
    /// learnt over it in place of what it maps.
    pub fn as_profile(&self, device: &str) -> Profile {
        let controls: Vec<ProfileControl> = lock(&self.mappings)
            .all()
            .iter()
            .filter(|m| m.device == device && selected_mode(&m.target).is_none())
            .map(|m| m.binding.clone())
            .collect();
        let name = base_port_name(device);
        let profile = controller_profiles::of(device).map(|p| (*p).clone()).unwrap_or_else(|| Profile {
            schema: None,
            name: name.clone(),
            ports: vec![name],
            jog_steps_per_turn: JOG_STEPS_PER_TURN,
            credits: String::new(),
            verified: false,
            init: Vec::new(),
            close: Vec::new(),
            keep_alive: None,
            modes: Vec::new(),
            controls: Vec::new(),
            lights: Vec::new(),
            meters: Vec::new(),
            outputs: Vec::new(),
            displays: Vec::new(),
        });
        Profile { controls, verified: false, ..profile }
    }

    /// Looks a while after Prisme starts, once a day at most, for the controller profiles
    /// published since: those that came drive their controllers at once.
    pub fn keep_profiles_up_to_date(app: AppHandle) {
        const AFTER_START: Duration = Duration::from_secs(20);
        let spawned = std::thread::Builder::new().name("prisme-profiles".into()).spawn(move || {
            std::thread::sleep(AFTER_START);
            if !controller_profiles::update_due() {
                return;
            }
            match tauri::async_runtime::block_on(controller_profiles::update()) {
                Ok(0) => {}
                Ok(came) => {
                    app.state::<Controllers>().profiles_changed();
                    let _ = app.emit("controller-profiles-changed", came);
                }
                Err(e) => warn!("controller profiles not looked for: {e}"),
            }
        });
        if let Err(e) = spawned {
            warn!("controller profiles not looked for: {e}");
        }
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

    /// Takes `target` off `device`: neither what was learnt nor its profile drives it there.
    pub fn forget(&self, device: &str, target: &str) {
        self.save(|s| {
            s.mappings.forget(device, target);
            let unmapped = Unmapped::On { device: device.to_owned(), target: target.to_owned() };
            if !s.unmapped.contains(&unmapped) {
                s.unmapped.push(unmapped);
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
            .flat_map(|p| p.lights.iter().map(|l| l.target.clone()).collect::<Vec<_>>())
            .filter(|t| selected_mode(t).is_none())
            .collect();
        targets.sort();
        targets.dedup();
        targets
    }

    /// The sources of the outputs and the displays of the profiles of the controllers in use,
    /// whose values the UI tells (`set_outputs`).
    pub fn output_targets(&self) -> Vec<String> {
        let mut targets: Vec<String> = self
            .setup
            .read(|s| s.enabled.clone())
            .iter()
            .filter_map(|device| controller_profiles::of(device))
            .flat_map(|p| {
                let outputs = p.outputs.iter().map(|o| o.source.clone());
                outputs.chain(p.displays.iter().map(|d| d.source.clone())).collect::<Vec<_>>()
            })
            .collect();
        targets.sort();
        targets.dedup();
        targets
    }

    /// Keeps the values of the outputs' sources, shown on the controllers as they go.
    pub fn set_outputs(&self, values: &[OutputDto]) {
        let mut kept = lock(&self.values);
        for value in values {
            kept.insert(value.target.clone(), value.value);
        }
    }

    /// Keeps the state of the `lights` targets, and shows it on the controllers.
    pub fn set_lights(&self, lights: &[LightDto]) {
        let mut states = lock(&self.states);
        for light in lights {
            states.insert(
                light.target.clone(),
                Shown { state: light.state.clone(), color: light.color.clone() },
            );
        }
        drop(states);
        let devices: Vec<String> = lock(&self.connections).iter().map(|c| c.device.clone()).collect();
        for device in devices {
            self.show_lights(&device);
        }
    }

    /// Shows on `device`'s lights the state of their actions.
    fn show_lights(&self, device: &str) {
        let lights = self.lights(device);
        if let Some(connection) = lock(&self.connections).iter_mut().find(|c| c.device == device) {
            for (control, value) in lights {
                connection.light(control, value);
            }
        }
    }

    /// What `device`'s lights show, in the modes it is in and the shifts held: the state of
    /// the actions its mapped controls and its mode keys drive (of a control mapped in several
    /// layers, the one in use's), and of the other lights its profile gives. A key lit while
    /// held, whose action has no state of its own, is off once let go.
    fn lights(&self, device: &str) -> Vec<(Control, LightValue)> {
        let (modes, shifts, held) = {
            let reader = lock(&self.reader);
            (reader.modes(device), reader.shifts(device), reader.held(device))
        };
        // in the order a key read takes them
        let mappings = lock(&self.mappings);
        let states = lock(&self.states);
        let keys = lock(&self.keys);
        let state_of = |target: &str| match selected_mode(target) {
            Some((group, mode)) => Some(if modes.get(group).is_some_and(|m| m == mode) { ON } else { OFF }),
            None => states.get(target).map(|s| s.state.as_str()),
        };
        let coloured = |target: &str, palette: &BTreeMap<String, u8>, value: LightValue| {
            let colour = states.get(target).and_then(|s| s.color.as_deref());
            match colour.and_then(|c| nearest(palette, c)) {
                Some(v) if state_of(target).is_some_and(|s| s != OFF) => LightValue::Steady(v),
                _ => value,
            }
        };
        let mut lights: Vec<(Control, LightValue)> = Vec::new();
        for mapping in mappings.shown(device, &shifts, &modes) {
            let key = (device.to_owned(), mapping.control);
            let lit_held = mapping.light.has(PRESSED);
            let state = if lit_held && held.contains(&mapping.control) {
                Some(PRESSED)
            } else if matches!(mapping.set, Some(FaderSet::InTurn(_))) {
                Some(if keys.second.contains(&key) { ON } else { OFF })
            } else {
                state_of(&mapping.target).or(lit_held.then_some(OFF))
            };
            if let Some(state) = state {
                let value = coloured(&mapping.target, &mapping.palette, mapping.light.value(state));
                lights.push((mapping.control, value));
            }
        }
        if let Some(profile) = controller_profiles::of(device) {
            for extra in &profile.lights {
                if let Some(value) = state_of(&extra.target).and_then(|s| extra.value(s, &modes)) {
                    lights.push((extra.control, coloured(&extra.target, &extra.palette, value)));
                }
            }
        }
        lights
    }

    /// Closes `device`: its lights go off, its meters show silence, and its profile's last
    /// messages go out.
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
            for (control, value) in profile.meters.iter().flat_map(|m| m.show(0.0, false)) {
                connection.light(control, value);
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
                controllers.fire_holds(&app);
                let levels = (metered.elapsed() >= METERS_EVERY).then(|| {
                    metered = Instant::now();
                    levels(&app.state::<AppState>())
                });
                let values = lock(&controllers.values).clone();
                for connection in lock(&controllers.connections).iter_mut() {
                    connection.blink();
                    connection.send_due();
                    let Some(profile) = controller_profiles::of(&connection.device) else { continue };
                    let modes = lock(&controllers.reader).modes(&connection.device);
                    let shown = |when: &Option<prisme_control::When>| {
                        when.as_ref().is_none_or(|w| modes.get(&w.group) == Some(&w.mode))
                    };
                    if let Some(keep) = &profile.keep_alive {
                        if connection.kept_alive.elapsed() >= Duration::from_millis(u64::from(keep.every_ms))
                        {
                            connection.kept_alive = Instant::now();
                            connection.send(&keep.message);
                        }
                    }
                    for (at, meter) in profile.meters.iter().enumerate().filter(|(_, m)| shown(&m.when)) {
                        let Some(&level) = levels.as_ref().and_then(|l| l.get(&meter.source)) else {
                            continue;
                        };
                        if level >= 1.0 {
                            connection.clipped.insert(at, Instant::now());
                        }
                        let clipping = connection.clipped.get(&at).is_some_and(|t| t.elapsed() < CLIP_SHOWN);
                        for (control, value) in meter.show(level, clipping) {
                            connection.light(control, value);
                        }
                    }
                    for output in profile.outputs.iter().filter(|o| shown(&o.when)) {
                        if let Some(&value) = values.get(&output.source) {
                            connection.light(output.control, LightValue::Steady(output.value(value)));
                        }
                    }
                    for (at, display) in profile.displays.iter().enumerate().filter(|(_, d)| shown(&d.when)) {
                        let Some(&value) = values.get(&display.source) else { continue };
                        let message = display.message(f64::from(value));
                        if connection.displayed.get(&at) != Some(&message) {
                            connection.send(&message);
                            connection.displayed.insert(at, message);
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
        let mut later = Vec::new();
        if let (Some(output), Some(profile)) = (output.as_mut(), controller_profiles::of(device)) {
            for (after_ms, message) in profile.init.iter().map(|m| m.when_and_what()) {
                if after_ms == 0 {
                    let _ = output.send(message);
                } else {
                    later.push((
                        Instant::now() + Duration::from_millis(u64::from(after_ms)),
                        message.to_vec(),
                    ));
                }
            }
        }
        let connection = Connection {
            device: device.to_owned(),
            port: port_name,
            _input: connection,
            output,
            kept_alive: Instant::now(),
            lit: Lit::default(),
            later,
            clipped: HashMap::new(),
            displayed: HashMap::new(),
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
        let layer = || {
            let reader = lock(&self.reader);
            (reader.mode_changes(device), reader.shifts(device))
        };
        let before = layer();
        let routed = self.route(device, event, at_s);
        // a key's light may show it held, latched or at the second of its two positions, and
        // the lights show the layer in use (a mode chosen by a switch, say)
        if event.control.kind == ControlKind::Note || layer() != before {
            self.show_lights(device);
        }
        match routed {
            Some(Routed::Learned(mapping)) => drop(app.emit("controller-learned", mapping)),
            Some(Routed::Actions(actions)) => self.act(app, actions, at_s, jog_steps_per_turn),
            None => {}
        }
    }

    /// Performs what a controller drove at `at_s` on its clock: the UI is told its actions, the
    /// direct ones are done here.
    fn act(&self, app: &AppHandle, actions: Vec<Action>, at_s: f64, jog_steps_per_turn: f64) {
        for action in actions {
            match action {
                Action::Ui(input) => drop(app.emit("controller-input", input)),
                Action::Direct(direct, value) => self.perform(app, direct, value, at_s, jog_steps_per_turn),
            }
        }
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

    /// Learns the control if a target waits for one, else finds what it drives; `at_s` when,
    /// on the controller's clock.
    fn route(&self, device: &str, event: Event, at_s: f64) -> Option<Routed> {
        if let Some(learnt) = self.complete_fine(device, event) {
            return Some(Routed::Learned(Box::new(learnt)));
        }
        let mut reader = lock(&self.reader);
        let shift = lock(&self.mappings)
            .find(device, Event { raw: 127, ..event }, &Shifts::default(), &Modes::new())
            .is_some_and(|m| m.target == SHIFT);
        // a release is the end of the previous press, not the control to learn; nor is shift
        let released = event.control.kind == ControlKind::Note && event.raw == 0;
        let learning = lock(&self.learning).take_if(|_| !released && !shift);
        if let Some(Learning { target, encoder }) = learning {
            let binding = ProfileControl {
                control: event.control,
                target,
                encoding: if encoder { Encoding::guess(event.raw) } else { Encoding::Absolute },
                shift: reader.shifted(device).into(),
                ..Default::default()
            };
            let mapping = Mapping::new(device, binding);
            self.save(|s| {
                let unmapped = Unmapped::On { device: device.to_owned(), target: mapping.target.clone() };
                s.unmapped.retain(|u| *u != unmapped);
                s.mappings.learn(mapping.clone(), Direct::takes_several(&mapping.target));
            });
            *lock(&self.just_learnt) = Some((mapping.clone(), Instant::now()));
            return Some(Routed::Learned(Box::new(mapping)));
        }
        let mappings = lock(&self.mappings);
        let (mapping, value) = reader.read(&mappings, device, event)?;
        if selected_mode(&mapping.target).is_some() {
            return match &mapping.hold {
                Some(hold) => self.hold(device, event, at_s, mapping, hold, &mut reader),
                None => driven(inputs(mapping, value)),
            };
        }
        if let Some(routed) = self.instead(device, event, mapping, &reader) {
            return routed;
        }
        if let Some(down) = &mapping.down {
            let steps = reader.whole_steps(device, mapping.control, value);
            return key_steps(&mapping.target, down, steps);
        }
        if let Some(hold) = &mapping.hold {
            return self.hold(device, event, at_s, mapping, hold, &mut reader);
        }
        driven(inputs(mapping, value))
    }

    /// What a key drives instead of its action as it is: another action pressed twice or
    /// with another key held, a fader put at a position, its action latched. `None` when it
    /// drives its action as it is.
    fn instead(
        &self,
        device: &str,
        event: Event,
        mapping: &Mapping,
        reader: &Reader,
    ) -> Option<Option<Routed>> {
        let key = (device.to_owned(), mapping.control);
        let pressed = event.raw > 0;
        let mut keys = lock(&self.keys);
        let press = |target: &str, value| Some(driven(vec![ControllerInputDto::of(target, value)]));
        if !pressed {
            if let Some(target) = keys.instead.remove(&key) {
                return press(&target, 0.0);
            }
        }
        if pressed {
            if let Some(chord) = mapping.chord.as_ref().filter(|c| reader.held(device).contains(&c.with)) {
                keys.instead.insert(key, chord.target.clone());
                return press(&chord.target, 1.0);
            }
            let before = keys.pressed_at.insert(key.clone(), Instant::now());
            let again =
                |ms: u32| before.is_some_and(|at| at.elapsed() < Duration::from_millis(u64::from(ms)));
            if let Some(double) = mapping.double.as_ref().filter(|d| again(d.ms)) {
                keys.instead.insert(key, double.target.clone());
                return press(&double.target, 1.0);
            }
        }
        if let Some(set) = mapping.set {
            if !pressed {
                return Some(None);
            }
            let position = match set {
                FaderSet::At(position) => position,
                FaderSet::InTurn([first, second]) if keys.second.remove(&key) => first,
                FaderSet::InTurn([_, second]) => {
                    keys.second.insert(key);
                    second
                }
            };
            return press(&mapping.target, position);
        }
        if mapping.latch {
            if !pressed {
                return Some(None);
            }
            let down = keys.latched.insert(key.clone()) || !keys.latched.remove(&key);
            return press(&mapping.target, if down { 1.0 } else { 0.0 });
        }
        None
    }

    /// A key whose action waits to know whether it is held: pressed, nothing yet; let go
    /// before its time, its own actions (those it drives along too) pressed and let go, or its
    /// mode chosen; after, the hold's let go.
    fn hold(
        &self,
        device: &str,
        event: Event,
        at_s: f64,
        mapping: &Mapping,
        hold: &Hold,
        reader: &mut Reader,
    ) -> Option<Routed> {
        let mut holdings = lock(&self.holdings);
        if event.raw > 0 {
            holdings.push(Holding {
                device: device.to_owned(),
                control: event.control,
                hold: hold.clone(),
                since: Instant::now(),
                at_s,
                fired: false,
            });
            return None;
        }
        let at = holdings.iter().position(|h| h.device == device && h.control == event.control)?;
        let held = holdings.remove(at);
        if held.fired && selected_mode(&held.hold.target).is_some() {
            return None;
        }
        if held.fired {
            return driven(vec![ControllerInputDto::of(&held.hold.target, 0.0)]);
        }
        reader.choose(device, mapping);
        driven([inputs(mapping, 1.0), inputs(mapping, 0.0)].concat())
    }

    /// The keys held long enough: their hold's action pressed, or its mode chosen.
    fn fire_holds(&self, app: &AppHandle) {
        for (device, pressed) in self.fired_holds() {
            match pressed {
                Some((action, at_s)) => {
                    let jog_steps_per_turn = controller_profiles::jog_steps_per_turn(&device);
                    self.act(app, vec![action], at_s, jog_steps_per_turn);
                }
                None => self.show_lights(&device),
            }
        }
    }

    /// The holds whose time came, by controller: the action each presses and when, on the
    /// controller's clock, or `None` for a mode it chose (here).
    fn fired_holds(&self) -> Vec<(String, Option<(Action, f64)>)> {
        let mut fired = Vec::new();
        for held in lock(&self.holdings).iter_mut() {
            if !held.fired && held.since.elapsed() >= Duration::from_millis(u64::from(held.hold.ms)) {
                held.fired = true;
                let at_s = held.at_s + f64::from(held.hold.ms) / 1000.0;
                fired.push((held.device.clone(), held.hold.target.clone(), at_s));
            }
        }
        // the reader once the holdings are let go: a key read takes them the other way round
        fired
            .into_iter()
            .map(|(device, target, at_s)| {
                let pressed = match selected_mode(&target) {
                    Some((group, mode)) => {
                        lock(&self.reader).set_mode(&device, group, mode);
                        None
                    }
                    None => Some((Action::of(ControllerInputDto::of(&target, 1.0)), at_s)),
                };
                (device, pressed)
            })
            .collect()
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
        let mut fine_mapping = mapping;
        fine_mapping.fine = Some(fine);
        Some(fine_mapping)
    }

    /// Drives the engine for a direct action; the UI hears of a new tempo.
    fn perform(&self, app: &AppHandle, action: Direct, value: f32, at_s: f64, jog_steps_per_turn: f64) {
        let state = app.state::<AppState>();
        let deck = match action {
            Direct::Jog(d) | Direct::JogTouch(d) | Direct::JogSearch(d) | Direct::Pitch(d) => d,
        };
        if state.has_deck(deck).is_err() {
            return;
        }
        // a wheel that only nudges: its top is no hand on the record
        if matches!(action, Direct::JogTouch(_)) && !lock(&state.decks)[deck].jog_scratch {
            return;
        }
        let range = state.tempo_range(deck);
        let tempo = Tempo { ratio: lock(&state.decks)[deck].pitch, range };
        let Some(command) = lock(&self.performer).command(action, value, at_s, jog_steps_per_turn, tempo)
        else {
            return;
        };
        if !matches!(action, Direct::Pitch(_)) {
            if let Some(auto_dj) = app.try_state::<AutoDj>() {
                auto_dj.take_over(Takeover::Transport(deck));
            }
        }
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
            let unmapped = |m: &Mapping| s.unmapped.iter().any(|u| u.covers(m));
            Mappings::layered(profiled, &s.mappings, unmapped, Direct::takes_several)
        });
        *lock(&self.mappings) = layered;
    }
}

/// What `mapping` driven to `value` gives the UI: its action (a mode key's is chosen as it is
/// read), and those it drives along.
fn inputs(mapping: &Mapping, value: f32) -> Vec<ControllerInputDto> {
    let steps = mapping.encoding.is_relative();
    let own = selected_mode(&mapping.target).is_none().then_some(&mapping.target);
    own.into_iter()
        .chain(&mapping.also)
        .map(|target| ControllerInputDto { target: target.clone(), value, steps, step: mapping.step })
        .collect()
}

/// What `inputs` drive, if there are any.
fn driven(inputs: Vec<ControllerInputDto>) -> Option<Routed> {
    (!inputs.is_empty()).then(|| Routed::Actions(inputs.into_iter().map(Action::of).collect()))
}

/// An encoder's `steps` driving keys: each a press and a release of `up`, or of `down` for
/// those to the left.
fn key_steps(up: &str, down: &str, steps: i32) -> Option<Routed> {
    let target = if steps > 0 { up } else { down };
    let presses = steps.unsigned_abs().min(MOST_KEY_STEPS.unsigned_abs());
    let inputs: Vec<ControllerInputDto> = (0..presses)
        .flat_map(|_| [ControllerInputDto::of(target, 1.0), ControllerInputDto::of(target, 0.0)])
        .collect();
    driven(inputs)
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
        .chain([
            ("main".to_owned(), status.main_peak),
            ("main.left".to_owned(), status.master_peaks[0]),
            ("main.right".to_owned(), status.master_peaks[1]),
        ])
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
    use prisme_control::{parse, Chord, Control, LightValues, Meter, Shift, When};

    const PAD: Control = Control { kind: ControlKind::Note, channel: 0, number: 36 };

    fn controllers() -> Controllers {
        Controllers::new(JsonFile::in_memory(ControllerSetup::default()))
    }

    fn input(target: &str, value: f32) -> Option<Routed> {
        let steps = target == "library.browse";
        Some(Routed::Actions(vec![Action::Ui(ControllerInputDto {
            steps,
            ..ControllerInputDto::of(target, value)
        })]))
    }

    fn mapping(control: Control, target: &str) -> Mapping {
        Mapping::new("DDJ", ProfileControl { control, target: target.into(), ..Default::default() })
    }

    /// `mapping` changed by `change`.
    fn with(mut mapping: Mapping, change: impl FnOnce(&mut ProfileControl)) -> Mapping {
        change(&mut mapping.binding);
        mapping
    }

    fn route(c: &Controllers, bytes: &[u8]) -> Option<Routed> {
        c.route("DDJ", parse(bytes).unwrap(), 0.0)
    }

    fn direct(action: Direct, value: f32) -> Option<Routed> {
        Some(Routed::Actions(vec![Action::Direct(action, value)]))
    }

    #[test]
    fn a_learnt_control_then_drives_its_target() {
        let c = controllers();
        c.learn("deck1.play".into(), false);
        // the release of a press made before Learn is not learnt
        assert_eq!(c.route("DDJ", parse(&[0x80, 40, 0]).unwrap(), 0.0), None);
        let learnt = c.route("DDJ", parse(&[0x90, 36, 127]).unwrap(), 0.0);
        assert_eq!(learnt, Some(Routed::Learned(Box::new(mapping(PAD, "deck1.play")))));
        assert_eq!(c.route("DDJ", parse(&[0x80, 36, 0]).unwrap(), 0.0), input("deck1.play", 0.0));
        assert_eq!(c.route("DDJ", parse(&[0x90, 36, 127]).unwrap(), 0.0), input("deck1.play", 1.0));
        assert_eq!(c.route("Other", parse(&[0x90, 36, 127]).unwrap(), 0.0), None);
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
        assert_eq!(route(&c, &[0xb0, 33, 61]), direct(Direct::Jog(0), -3.0));
    }

    #[test]
    fn a_fader_sending_its_low_half_is_learnt_14_bit() {
        let c = controllers();
        c.learn("deck1.pitch".into(), false);
        route(&c, &[0xb0, 0, 64]);
        let fine = route(&c, &[0xb0, 32, 0]);
        let tempo = Control { kind: ControlKind::ControlChange, channel: 0, number: 0 };
        assert_eq!(
            fine,
            Some(Routed::Learned(Box::new(with(mapping(tempo, "deck1.pitch"), |b| b.fine = Some(32)))))
        );
        assert_eq!(route(&c, &[0xb0, 0, 64]), None);
        assert_eq!(route(&c, &[0xb0, 32, 64]), direct(Direct::Pitch(0), 8256.0 / 16383.0));
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
            Some(Routed::Learned(Box::new(with(mapping(PAD, "deck1.hotcue1.delete"), |b| {
                b.shift = prisme_control::Shift::Any
            }))))
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
        let play = |c: &Controllers| c.route("DDJ-SB2", parse(&[0x90, 0x0b, 127]).unwrap(), 0.0);
        assert_eq!(play(&c), input("deck1.play", 1.0));
        // learnt over the profile, the control drives what was learnt
        c.learn("deck1.cue".into(), false);
        play(&c);
        assert_eq!(play(&c), input("deck1.cue", 1.0));
        // forgotten, an action leaves the profile's controls too
        c.forget("DDJ-SB2", "deck1.cue");
        c.forget("DDJ-SB2", "deck1.play");
        assert_eq!(play(&c), None);
        assert!(c
            .setup()
            .unmapped
            .contains(&Unmapped::On { device: "DDJ-SB2".into(), target: "deck1.play".into() }));
        // learnt again, it is mapped again
        c.learn("deck1.play".into(), false);
        play(&c);
        assert_eq!(play(&c), input("deck1.play", 1.0));
    }

    #[test]
    fn an_action_taken_off_a_controller_stays_on_the_others() {
        let c = sb2();
        c.save(|s| s.enabled.push("DDJ-SB2 MIDI 2".into()));
        let play = |c: &Controllers, device| c.route(device, parse(&[0x90, 0x0b, 127]).unwrap(), 0.0);
        c.forget("DDJ-SB2", "deck1.play");
        assert_eq!(play(&c, "DDJ-SB2"), None);
        assert_eq!(play(&c, "DDJ-SB2 MIDI 2"), input("deck1.play", 1.0));
    }

    #[test]
    fn an_action_taken_off_before_controllers_were_told_apart_stays_off_them_all() {
        let setup: ControllerSetup =
            serde_json::from_str(r#"{ "enabled": ["DDJ-SB2"], "unmapped": ["deck1.play"] }"#).unwrap();
        assert_eq!(setup.unmapped, [Unmapped::Everywhere("deck1.play".into())]);
        let c = controllers();
        c.save(|s| *s = setup);
        assert_eq!(c.route("DDJ-SB2", parse(&[0x90, 0x0b, 127]).unwrap(), 0.0), None);
        let saved = serde_json::to_string(&c.setup()).unwrap();
        c.forget("DDJ-SB2", "deck1.cue");
        assert!(saved.contains(r#""unmapped":["deck1.play"]"#));
        assert!(serde_json::to_string(&c.setup())
            .unwrap()
            .contains(r#"{"device":"DDJ-SB2","target":"deck1.cue"}"#));
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
        assert_eq!(c.route("DDJ-SB2", parse(&[0x90, 0x0b, 127]).unwrap(), 0.0), None);
        assert!(c.mappings().all().is_empty());
    }

    #[test]
    fn learning_can_be_cancelled() {
        let c = controllers();
        c.learn("deck1.play".into(), false);
        c.cancel_learning();
        assert_eq!(c.route("DDJ", parse(&[0x90, 36, 127]).unwrap(), 0.0), None);
        assert!(c.setup().mappings.all().is_empty());
    }

    /// Controllers whose `DDJ` has `mappings`.
    fn with_mappings(mappings: impl IntoIterator<Item = Mapping>) -> Controllers {
        let c = controllers();
        c.save(|s| mappings.into_iter().for_each(|m| s.mappings.learn(m, false)));
        c
    }

    fn inputs_of(pairs: &[(&str, f32)]) -> Option<Routed> {
        Some(Routed::Actions(pairs.iter().map(|&(t, v)| Action::Ui(ControllerInputDto::of(t, v))).collect()))
    }

    #[test]
    fn a_key_pressed_twice_drives_its_double() {
        let double = Hold { ms: 300, target: "deck1.slip".into() };
        let c = with_mappings([with(mapping(PAD, "deck1.sync"), |b| b.double = Some(double))]);
        assert_eq!(route(&c, &[0x90, 36, 127]), input("deck1.sync", 1.0));
        assert_eq!(route(&c, &[0x90, 36, 0]), input("deck1.sync", 0.0));
        assert_eq!(route(&c, &[0x90, 36, 127]), input("deck1.slip", 1.0));
        assert_eq!(route(&c, &[0x90, 36, 0]), input("deck1.slip", 0.0), "its release lets the double go");
    }

    #[test]
    fn a_key_pressed_with_another_drives_their_chord() {
        let other = Control { number: 37, ..PAD };
        let chord = Chord { with: other, target: "deck1.tempo.reset".into() };
        let c = with_mappings([
            mapping(other, "deck1.bend.up"),
            with(mapping(PAD, "deck1.bend.down"), |b| b.chord = Some(chord)),
        ]);
        assert_eq!(route(&c, &[0x90, 36, 127]), input("deck1.bend.down", 1.0), "alone, its own");
        route(&c, &[0x90, 36, 0]);
        route(&c, &[0x90, 37, 127]);
        assert_eq!(route(&c, &[0x90, 36, 127]), input("deck1.tempo.reset", 1.0));
        assert_eq!(route(&c, &[0x90, 36, 0]), input("deck1.tempo.reset", 0.0));
    }

    #[test]
    fn a_key_sets_a_fader_at_a_position_or_two_in_turn() {
        let c = with_mappings([with(mapping(PAD, "mixer.cueMix"), |b| {
            b.set = Some(FaderSet::InTurn([0.0, 1.0]))
        })]);
        assert_eq!(route(&c, &[0x90, 36, 127]), input("mixer.cueMix", 1.0));
        assert_eq!(route(&c, &[0x90, 36, 0]), None, "a release sets nothing");
        assert_eq!(route(&c, &[0x90, 36, 127]), input("mixer.cueMix", 0.0));
    }

    #[test]
    fn a_latched_key_holds_its_action_until_pressed_again() {
        let c = with_mappings([with(mapping(PAD, "deck1.eq.low.kill"), |b| b.latch = true)]);
        assert_eq!(route(&c, &[0x90, 36, 127]), input("deck1.eq.low.kill", 1.0));
        assert_eq!(route(&c, &[0x90, 36, 0]), None);
        assert_eq!(route(&c, &[0x90, 36, 127]), input("deck1.eq.low.kill", 0.0));
    }

    #[test]
    fn a_mode_key_drives_its_other_actions_too() {
        let key = Control { number: 9, ..PAD };
        let c = with_mappings([with(mapping(key, "mode.pads.roll"), |b| b.also = vec!["deck1.slip".into()])]);
        assert_eq!(route(&c, &[0x90, 9, 127]), input("deck1.slip", 1.0));
        assert_eq!(lock(&c.reader).modes("DDJ")["pads"], "roll");
        assert_eq!(route(&c, &[0x90, 9, 0]), input("deck1.slip", 0.0));
    }

    #[test]
    fn a_key_let_go_before_its_hold_drives_its_other_actions_too() {
        let load = |b: &mut ProfileControl| {
            b.hold = Some(Hold { ms: 60_000, target: "deck1.eject".into() });
            b.also = vec!["deck1.play".into()];
        };
        let c = with_mappings([with(mapping(PAD, "library.load1"), load)]);
        assert_eq!(route(&c, &[0x90, 36, 127]), None);
        let pressed = [("library.load1", 1.0), ("deck1.play", 1.0)];
        let let_go = [("library.load1", 0.0), ("deck1.play", 0.0)];
        assert_eq!(route(&c, &[0x90, 36, 0]), inputs_of(&[pressed[0], pressed[1], let_go[0], let_go[1]]));
    }

    #[test]
    fn a_mode_key_held_chooses_its_hold_and_let_go_sooner_its_own_mode() {
        let key = Control { number: 9, ..PAD };
        let mode_key = |ms| {
            let hold = Hold { ms, target: "mode.pads.slicer".into() };
            with(mapping(key, "mode.pads.roll"), |b| b.hold = Some(hold))
        };
        let c = with_mappings([mode_key(60_000)]);
        lock(&c.reader).set_modes("DDJ", Modes::from([("pads".into(), "cues".into())]));
        let mode = |c: &Controllers| lock(&c.reader).modes("DDJ")["pads"].clone();
        assert_eq!(route(&c, &[0x90, 9, 127]), None);
        assert_eq!(mode(&c), "cues", "the press waits to know whether it is held");
        assert_eq!(route(&c, &[0x90, 9, 0]), None);
        assert_eq!(mode(&c), "roll");
        lock(&c.mappings).learn(mode_key(0), false);
        route(&c, &[0x90, 9, 127]);
        assert_eq!(c.fired_holds(), [("DDJ".to_owned(), None)]);
        assert_eq!(mode(&c), "slicer");
        assert_eq!(route(&c, &[0x90, 9, 0]), None);
        assert_eq!(mode(&c), "slicer", "let go after its hold, its own mode is not chosen");
    }

    #[test]
    fn a_direct_action_is_done_here_whatever_drives_it() {
        let (other, third) = (Control { number: 37, ..PAD }, Control { number: 38, ..PAD });
        let c = with_mappings([
            with(mapping(PAD, "deck1.jog.touch"), |b| b.latch = true),
            with(mapping(other, "deck1.play"), |b| {
                b.double = Some(Hold { ms: 60_000, target: "deck1.jog.touch".into() })
            }),
            with(mapping(third, "deck1.cue"), |b| {
                b.hold = Some(Hold { ms: 0, target: "deck1.jog.touch".into() })
            }),
        ]);
        assert_eq!(c.route("DDJ", parse(&[0x90, 38, 127]).unwrap(), 2.5), None);
        let held = Some((Action::Direct(Direct::JogTouch(0), 1.0), 2.5));
        assert_eq!(
            c.fired_holds(),
            [("DDJ".to_owned(), held)],
            "held, at its time on the controller's clock"
        );
        assert_eq!(route(&c, &[0x90, 38, 0]), direct(Direct::JogTouch(0), 0.0));
        assert_eq!(route(&c, &[0x90, 36, 127]), direct(Direct::JogTouch(0), 1.0), "latched");
        assert_eq!(route(&c, &[0x90, 36, 0]), None);
        assert_eq!(route(&c, &[0x90, 36, 127]), direct(Direct::JogTouch(0), 0.0));
        route(&c, &[0x90, 37, 127]);
        route(&c, &[0x90, 37, 0]);
        assert_eq!(route(&c, &[0x90, 37, 127]), direct(Direct::JogTouch(0), 1.0), "pressed twice");
        assert_eq!(route(&c, &[0x90, 37, 0]), direct(Direct::JogTouch(0), 0.0));
    }

    #[test]
    fn what_a_key_drives_along_with_a_direct_action_is_driven_too() {
        let c = with_mappings([with(mapping(PAD, "deck1.jog.touch"), |b| {
            b.also = vec!["deck2.jog.touch".into()]
        })]);
        let touched =
            vec![Action::Direct(Direct::JogTouch(0), 1.0), Action::Direct(Direct::JogTouch(1), 1.0)];
        assert_eq!(route(&c, &[0x90, 36, 127]), Some(Routed::Actions(touched)));
    }

    /// A control's light of `value` in `state`.
    fn lit(state: &str, value: u8) -> impl FnOnce(&mut ProfileControl) + '_ {
        move |b| b.light = LightValues::from_iter([(state.to_owned(), LightValue::Steady(value))])
    }

    #[test]
    fn a_key_lit_while_held_goes_dark_when_let_go() {
        let c = with_mappings([with(mapping(PAD, "deck1.jump.back"), lit(PRESSED, 100))]);
        route(&c, &[0x90, 36, 127]);
        assert_eq!(c.lights("DDJ"), [(PAD, LightValue::Steady(100))]);
        route(&c, &[0x90, 36, 0]);
        assert_eq!(c.lights("DDJ"), [(PAD, LightValue::Steady(0))]);
    }

    fn light(target: &str, state: &str) -> LightDto {
        LightDto { target: target.into(), state: state.into(), color: None }
    }

    #[test]
    fn a_key_of_two_layers_shows_the_one_in_use() {
        let shift_key = Control { number: 63, ..PAD };
        let c = with_mappings([
            mapping(shift_key, SHIFT),
            with(mapping(PAD, "deck1.play"), lit(ON, 10)),
            with(with(mapping(PAD, "deck1.slip"), lit(ON, 20)), |b| b.shift = Shift::Any),
        ]);
        c.set_lights(&[light("deck1.play", ON), light("deck1.slip", OFF)]);
        assert_eq!(c.lights("DDJ"), [(PAD, LightValue::Steady(10))]);
        route(&c, &[0x90, 63, 127]);
        assert_eq!(c.lights("DDJ"), [(PAD, LightValue::Steady(0))], "the shift layer's");
        route(&c, &[0x90, 63, 0]);
        assert_eq!(c.lights("DDJ"), [(PAD, LightValue::Steady(10))]);
    }

    #[test]
    fn a_key_of_two_modes_shows_the_one_in_use() {
        let when = |mode: &str| Some(When { group: "pads".into(), mode: mode.into() });
        let c = with_mappings([
            with(with(mapping(PAD, "deck1.hotcue1"), lit(ON, 10)), |b| b.when = when("cues")),
            with(with(mapping(PAD, "deck1.roll1"), lit(ON, 20)), |b| b.when = when("rolls")),
        ]);
        lock(&c.reader).set_modes("DDJ", Modes::from([("pads".into(), "cues".into())]));
        c.set_lights(&[light("deck1.hotcue1", ON), light("deck1.roll1", ON)]);
        assert_eq!(c.lights("DDJ"), [(PAD, LightValue::Steady(10))]);
        lock(&c.reader).set_mode("DDJ", "pads", "rolls");
        assert_eq!(c.lights("DDJ"), [(PAD, LightValue::Steady(20))]);
    }

    /// The messages going to the controller for `meter` at `level`, what `lit` shows already
    /// left out.
    fn sent(lit: &mut Lit, meter: &Meter, level: f32, clipping: bool) -> Vec<Vec<u8>> {
        meter
            .show(level, clipping)
            .into_iter()
            .filter_map(|(control, value)| lit.light(control, value))
            .collect()
    }

    #[test]
    fn a_meter_sends_only_the_leds_that_change() {
        let meter: Meter = serde_json::from_str(
            r#"{ "source": "deck1", "leds": [
                { "control": { "kind": "note", "channel": 0, "number": 10 }, "from": 0.2, "on": [176, 80, 10], "off": [176, 81, 10] },
                { "control": { "kind": "note", "channel": 0, "number": 11 }, "from": 0.5, "on": [176, 80, 11], "off": [176, 81, 11] },
                { "control": { "kind": "controlChange", "channel": 0, "number": 89 }, "from": 1, "on": 1, "clip": 1 }
            ] }"#,
        )
        .unwrap();
        let lit = &mut Lit::default();
        assert_eq!(sent(lit, &meter, 0.0, false), [vec![176, 81, 10], vec![176, 81, 11], vec![0xb0, 89, 0]]);
        assert_eq!(sent(lit, &meter, 0.1, false), Vec::<Vec<u8>>::new());
        assert_eq!(sent(lit, &meter, 0.6, false), [vec![176, 80, 10], vec![176, 80, 11]]);
        assert_eq!(sent(lit, &meter, 0.3, true), [vec![176, 81, 11], vec![0xb0, 89, 1]], "the clip LED held");
        assert_eq!(sent(lit, &meter, 0.3, false), [vec![0xb0, 89, 0]]);
    }

    #[test]
    fn a_meter_on_one_control_sends_its_value_when_it_changes() {
        let meter: Meter = serde_json::from_str(
            r#"{ "source": "main", "control": { "kind": "controlChange", "channel": 0, "number": 2 }, "max": 117, "clip": 119 }"#,
        )
        .unwrap();
        let lit = &mut Lit::default();
        assert_eq!(sent(lit, &meter, 1.0, false), [vec![0xb0, 2, 117]]);
        assert_eq!(sent(lit, &meter, 1.0, false), Vec::<Vec<u8>>::new());
        assert_eq!(sent(lit, &meter, 1.0, true), [vec![0xb0, 2, 119]]);
    }

    #[test]
    fn an_encoder_presses_keys_a_step_each() {
        let knob = Control { kind: ControlKind::ControlChange, channel: 0, number: 20 };
        let loops = |b: &mut ProfileControl| {
            b.encoding = Encoding::RelativeOffset;
            b.down = Some("deck1.loop.halve".into());
        };
        let c = with_mappings([with(mapping(knob, "deck1.loop.double"), loops)]);
        let twice = [("deck1.loop.double", 1.0), ("deck1.loop.double", 0.0)];
        assert_eq!(route(&c, &[0xb0, 20, 66]), inputs_of(&[twice[0], twice[1], twice[0], twice[1]]));
        assert_eq!(
            route(&c, &[0xb0, 20, 63]),
            inputs_of(&[("deck1.loop.halve", 1.0), ("deck1.loop.halve", 0.0)])
        );
    }
}
