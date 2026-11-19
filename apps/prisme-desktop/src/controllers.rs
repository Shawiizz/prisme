//! Controllers, MIDI and HID (`controller_hid`). A mapped control's messages, or the fields of
//! a HID report that changed, become `controller-input` events for the UI, which performs the
//! actions and tells the state of each (its lights show it), except for the jog wheels and
//! tempo faders, which drive the engine from here (`controller_direct`). The lights that
//! blink, the levels a controller shows and the messages some models want again and again go
//! out from a thread of their own.
//! Mappings are learnt from the controller itself and kept in `controllers.json`, with the
//! controllers in use; a model Prisme knows comes mapped by its profile
//! (`controller_profiles`), what is learnt on it taking the profile's place.
//! A controller seen both as a MIDI port and as a HID device (a Hercules with its driver) is
//! used one way only: by HID if Prisme knows it so and not by MIDI, else by MIDI, as its
//! maker's driver then speaks to the device itself.

use crate::auto_dj::{AutoDj, Takeover};
use crate::controller_direct::{Direct, Performer, Tempo};
use crate::controller_hid;
use crate::controller_profiles;
use crate::dto::{ControllerInputDto, DeckPitchDto, LightDto, OutputDto};
use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState};
use midir::{Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection};
use prisme_audio_io::base_device_name;
use prisme_control::hid::{self, Field, Learnt, Read};
use prisme_control::{
    nearest, selected_mode, Control, Encoding, Event, FaderSet, Hold, LightValue, Mapping, Mappings, Modes,
    Profile, ProfileControl, Reader, Shifts, JOG_STEPS_PER_TURN, OFF, ON, SHIFT,
};
use prisme_engine::Command;
use prisme_session::JsonFile;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tracing::{error, info, warn};

/// The low half of a 14-bit control follows its high half within this.
const FINE_HALF_WITHIN: Duration = Duration::from_millis(100);

const CLIENT: &str = "Prisme";
/// How often the controllers in use are looked for, to open again one plugged back: less
/// than a controller takes to come back on the USB bus, so that none goes unseen.
const WATCH_EVERY: Duration = Duration::from_millis(500);
/// How often MIDI is tried again on a system whose MIDI does not answer (no sequencer loaded,
/// on Linux).
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
    /// Controllers in use, by their MIDI port or the name Prisme gives a HID device
    /// (`hid::device_name`): Windows lets a single program open a port, so a controller is
    /// taken by itself only when plugged in, and only of a model Prisme knows.
    pub enabled: Vec<String>,
    /// Controllers the DJ turned off, or chose not to use when plugged in: never opened, nor
    /// offered, by themselves.
    pub declined: Vec<String>,
    pub mappings: Mappings,
    /// Where the MIDI clock goes (see `midi_clock`).
    pub clock_output: Option<String>,
    /// Actions the DJ took off a controller: its profile's controls no longer drive them.
    pub unmapped: Vec<Unmapped>,
    /// The profile the DJ chose for a controller among those offered for it (a variant of its
    /// model's), by file.
    pub profiles: BTreeMap<String, String>,
}

pub const PLUGGED_EVENT: &str = "controller-plugged";

/// What the DJ is told of a controller plugged in while Prisme runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PluggedIn {
    /// Of a model Prisme knows: opened, ready to play.
    Opened { device: String, model: String },
    /// Of a model Prisme does not know: used if the DJ wants it.
    Offered { device: String, model: String },
    /// Of a model Prisme knows, held by another program: taken as soon as it lets go.
    Waiting { device: String, model: String },
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

/// The next control moved will drive `target`, an action taking `learnt`; an encoder's steps
/// are read as it sends its first one.
struct Learning {
    target: String,
    learnt: Learnt,
}

/// What a HID report tells.
#[derive(Debug, PartialEq)]
enum Reported {
    Learnt(Box<Mapping>),
    Events(Vec<Event>),
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
    link: Link,
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

/// How a controller is reached.
enum Link {
    Midi {
        /// Taken only to be closed (see [`Drop`]).
        input: Option<MidiInputConnection<()>>,
        /// Lights; not every controller has them.
        output: Option<MidiOutputConnection>,
    },
    /// Read and written from its own thread.
    Hid(controller_hid::Link),
}

/// A controller let go is closed, not only dropped: on Android, a port dropped keeps the device
/// open, and with it a sound card the USB driver would play on.
impl Drop for Connection {
    fn drop(&mut self) {
        if let Link::Midi { input, output } = &mut self.link {
            if let Some(input) = input.take() {
                input.close();
            }
            if let Some(output) = output.take() {
                output.close();
            }
        }
    }
}

/// What a controller's lights and meters show, so that a light changes only for a change.
#[derive(Default)]
struct Lit {
    /// The value each one shows.
    shown: HashMap<Control, u8>,
    /// Those blinking.
    blinking: HashMap<Control, Blink>,
    /// Those shown by a message of their own.
    messages: HashMap<Control, Vec<u8>>,
}

/// A light to change: its control to a value, or to a message of its own (a MIDI message, the
/// bytes of a HID light).
#[derive(Debug, Clone, PartialEq)]
enum Lighting {
    Value(Control, u8),
    Message(Control, Vec<u8>),
}

impl Lighting {
    /// The MIDI message it is, if it is one.
    fn midi(self) -> Option<Vec<u8>> {
        match self {
            Lighting::Value(control, value) => prisme_control::light(control, value).map(Vec::from),
            Lighting::Message(_, message) => Some(message),
        }
    }
}

/// A light showing its two values in turn, since `since`.
struct Blink {
    values: [u8; 2],
    ms: u32,
    since: Instant,
}

impl Lit {
    /// `control` at `value`, unless it shows it already.
    fn show(&mut self, control: Control, value: u8) -> Option<Lighting> {
        (self.shown.insert(control, value) != Some(value)).then_some(Lighting::Value(control, value))
    }

    /// The change setting the light of `control` to `value`, blinking or not, unless it shows
    /// it already.
    fn light(&mut self, control: Control, value: LightValue) -> Option<Lighting> {
        if !matches!(value, LightValue::Message(_)) {
            self.messages.remove(&control);
        }
        match value {
            LightValue::Steady(v) => {
                self.blinking.remove(&control);
                self.show(control, v)
            }
            LightValue::Message(message) => {
                self.blinking.remove(&control);
                self.shown.remove(&control);
                let before = self.messages.insert(control, message.clone());
                (before.as_ref() != Some(&message)).then_some(Lighting::Message(control, message))
            }
            LightValue::Blink { blink, ms } => {
                let same = self.blinking.get(&control).is_some_and(|b| b.values == blink && b.ms == ms);
                if same {
                    return None;
                }
                self.blinking.insert(control, Blink { values: blink, ms, since: Instant::now() });
                self.show(control, blink[0])
            }
        }
    }

    /// The changes of the blinking lights whose value of the moment is not shown yet.
    fn blink(&mut self) -> Vec<Lighting> {
        let now: Vec<(Control, u8)> = self
            .blinking
            .iter()
            .map(|(&control, b)| {
                let half = (b.since.elapsed().as_millis() / u128::from(b.ms.max(1))) % 2;
                (control, b.values[half as usize])
            })
            .collect();
        now.into_iter().filter_map(|(control, value)| self.show(control, value)).collect()
    }
}

impl Connection {
    /// Sets the light of `control` to `value`, blinking or not.
    fn light(&mut self, control: Control, value: LightValue) {
        if let Some(lighting) = self.lit.light(control, value) {
            self.apply(lighting);
        }
    }

    /// Each blinking light at the value of the moment.
    fn blink(&mut self) {
        for lighting in self.lit.blink() {
            self.apply(lighting);
        }
    }

    fn apply(&mut self, lighting: Lighting) {
        match (&self.link, lighting) {
            (Link::Hid(link), Lighting::Value(Control::Hid(led), value)) => link.set(led, value),
            (Link::Hid(link), Lighting::Message(Control::Hid(led), bytes)) => link.set_bytes(led, &bytes),
            (Link::Hid(_), _) => {}
            (Link::Midi { .. }, lighting) => {
                if let Some(message) = lighting.midi() {
                    self.send(&message);
                }
            }
        }
    }

    /// Sends a message of its own: a MIDI message, a whole HID report.
    fn send(&mut self, message: &[u8]) {
        match &mut self.link {
            Link::Midi { output: Some(output), .. } => drop(output.send(message)),
            Link::Midi { output: None, .. } => {}
            Link::Hid(link) => link.send(message),
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
    /// The fields of each HID controller's reports its mappings read.
    hid_fields: Mutex<HashMap<String, Arc<BTreeSet<Field>>>>,
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
            hid_fields: Mutex::new(HashMap::new()),
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
        let profile = self.profile(device).map(|p| (*p).clone()).unwrap_or_else(|| Profile {
            schema: None,
            name: name.clone(),
            ports: vec![name],
            hid: None,
            jog_steps_per_turn: JOG_STEPS_PER_TURN,
            credits: String::new(),
            verified: false,
            variant: false,
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
        Profile { controls, verified: false, variant: false, ..profile }
    }

    /// The model of `device`, as its profile names it, else as its port does.
    pub fn model_name(&self, device: &str) -> String {
        self.profile(device).map_or_else(|| base_port_name(device), |p| p.name.clone())
    }

    /// The profile of `device`: the one the DJ chose for it, else its model's.
    fn profile(&self, device: &str) -> Option<Arc<Profile>> {
        let chosen = self.setup.read(|s| s.profiles.get(device).cloned());
        controller_profiles::current().chosen(device, chosen.as_deref()).map(|k| k.profile.clone())
    }

    /// Steps in a turn of the jog wheels of `device`.
    fn jog_steps_per_turn(&self, device: &str) -> f64 {
        self.profile(device).map_or(JOG_STEPS_PER_TURN, |p| p.jog_steps_per_turn)
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

    /// The controllers plugged in: the MIDI ports, and the HID devices that may be controllers.
    pub fn plugged() -> Vec<String> {
        let midi = MidiInput::new(CLIENT).map(|input| port_names(&input)).unwrap_or_else(|e| {
            warn!("MIDI ports not listed: {e}");
            Vec::new()
        });
        with_hid(midi, &hid_plugged())
    }

    pub fn setup(&self) -> ControllerSetup {
        self.setup.get()
    }

    /// What drives what: the profiles of the controllers in use, and what was learnt.
    pub fn mappings(&self) -> Mappings {
        lock(&self.mappings).clone()
    }

    /// Opens the controllers in use that are plugged in, and those of a model Prisme knows,
    /// unless the DJ turned them off.
    pub fn open_at_start(&self, app: &AppHandle) {
        for device in self.setup.read(|s| s.enabled.clone()) {
            if let Err(e) = self.open(app, &device) {
                warn!("controller {device} not opened: {e}");
            }
        }
        for port in Self::plugged() {
            if self.new_to_the_dj(&port) && controller_profiles::of(&port).is_some() {
                self.take_up(app, &port);
            }
        }
    }

    /// Keeps the controllers in use open: one unplugged (a USB cable pulled, a hub that
    /// resets) is closed, and opened again as soon as it is back. One plugged in for the first
    /// time is opened if Prisme knows its model, else offered (see [`Self::plug_in`]).
    pub fn watch(app: AppHandle) {
        let spawned = std::thread::Builder::new().name("prisme-midi-watch".into()).spawn(move || {
            let mut missing: Vec<String> = Vec::new();
            // the ports of the last look; those there at start were seen to then
            let mut seen: Option<Vec<String>> = None;
            // one client for looking, kept: a new one each time would come and go in every
            // MIDI patchbay of the system
            let mut looking: Option<MidiInput> = None;
            let mut midi_tried: Option<Instant> = None;
            loop {
                std::thread::sleep(WATCH_EVERY);
                if looking.is_none() && midi_tried.is_none_or(|at| at.elapsed() >= WATCH_WITHOUT_MIDI_EVERY) {
                    looking = MidiInput::new(CLIENT).ok();
                    midi_tried = Some(Instant::now());
                }
                let midi = looking.as_ref().map(port_names).unwrap_or_default();
                let plugged = with_hid(midi, &hid_plugged());
                let controllers = app.state::<Controllers>();
                let unplugged: Vec<String> = controllers
                    .take_connections(|c| !plugged.contains(&c.port))
                    .into_iter()
                    .map(|c| c.device.clone())
                    .collect();
                for device in &unplugged {
                    controllers.let_go(&app, device);
                }
                for device in controllers.setup.read(|s| s.enabled.clone()) {
                    let open = lock(&controllers.connections).iter().any(|c| c.device == device);
                    if open || port_of(&plugged, &device, &controllers.ports_open()).is_none() {
                        continue;
                    }
                    match controllers.open(&app, &device) {
                        // turned off meanwhile
                        Ok(()) if !controllers.setup.read(|s| s.enabled.contains(&device)) => {
                            drop(controllers.take_connections(|c| c.device == device));
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
                if let Some(seen) = &seen {
                    for port in plugged.iter().filter(|p| !seen.contains(p)) {
                        controllers.plug_in(&app, port);
                    }
                }
                seen = Some(plugged);
            }
        });
        if let Err(e) = spawned {
            error!("controllers not watched: {e}");
        }
    }

    /// A controller plugged in while Prisme runs: opened if Prisme knows its model, else
    /// offered, unless it is in use or the DJ turned it off or chose not to use it. The
    /// interface is told ([`PLUGGED_EVENT`]).
    fn plug_in(&self, app: &AppHandle, port: &str) {
        if !self.new_to_the_dj(port) {
            return;
        }
        let (device, model) = (port.to_owned(), self.model_name(port));
        let news = if controller_profiles::of(port).is_none() {
            // on Android the MIDI of a sound card, opened, holds the card the USB driver
            // plays on: a model Prisme does not know is turned on in the settings only; most
            // HID devices are no controllers, those Prisme does not know are only listed there
            if cfg!(target_os = "android") || hid::ids_of(port).is_some() {
                return;
            }
            PluggedIn::Offered { device, model }
        } else if self.take_up(app, port) {
            PluggedIn::Opened { device, model }
        } else {
            PluggedIn::Waiting { device, model }
        };
        let _ = app.emit(PLUGGED_EVENT, news);
    }

    /// Whether the controller on `port` is neither in use nor one the DJ turned off or chose
    /// not to use. The port Linux loops MIDI back through is no controller.
    fn new_to_the_dj(&self, port: &str) -> bool {
        let base = base_port_name(port);
        let same = |d: &String| base_port_name(d) == base;
        !base.starts_with("Midi Through")
            && self.setup.read(|s| !s.enabled.iter().any(same) && !s.declined.iter().any(same))
    }

    /// Puts the controller on `port` in use; whether it could be opened. Held by another
    /// program, it is in use all the same: opened as soon as that program lets go (see
    /// [`Self::watch`]).
    fn take_up(&self, app: &AppHandle, port: &str) -> bool {
        match self.set_enabled(app, port, true) {
            Ok(()) => true,
            Err(e) => {
                info!("controller {port} held elsewhere ({e}), taken once free");
                self.save(|s| {
                    s.declined.retain(|d| d != port);
                    if !s.enabled.iter().any(|d| d == port) {
                        s.enabled.push(port.to_owned());
                    }
                });
                false
            }
        }
    }

    /// The models of the controllers in use, plugged in, that another program holds: Prisme
    /// takes each as soon as it lets go.
    pub fn waiting(&self) -> Vec<String> {
        let plugged = Self::plugged();
        let open: Vec<String> = lock(&self.connections).iter().map(|c| c.device.clone()).collect();
        let taken = self.ports_open();
        self.setup
            .read(|s| s.enabled.clone())
            .into_iter()
            .filter(|device| port_of(&plugged, device, &taken).is_some() && !open.contains(device))
            .map(|device| self.model_name(&device))
            .collect()
    }

    /// The DJ chose not to use the controller on `device`: it is not offered again.
    pub fn decline(&self, device: &str) {
        self.save(|s| {
            if !s.declined.iter().any(|d| d == device) {
                s.declined.push(device.to_owned());
            }
        });
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
            s.declined.retain(|d| d != device);
            // turned off, it is not taken again by itself when plugged in
            let list = if enabled { &mut s.enabled } else { &mut s.declined };
            list.push(device.to_owned());
        });
        // its profile's lights, now that it is among the controllers in use
        self.show_lights(device);
        Ok(())
    }

    /// Drives `device` by `file`, one of the profiles offered for it, and keeps the choice for it.
    /// Open, it is opened again: the new profile's messages go and its lights show.
    pub fn choose_profile(&self, app: &AppHandle, device: &str, file: &str) -> AppResult<()> {
        let library = controller_profiles::current();
        if !library.offered(device).iter().any(|k| k.file == file) {
            return Err(AppError::Other(format!("{file} is no profile for {device}")));
        }
        let model = library.of(device).is_some_and(|k| k.file == file);
        let open = lock(&self.connections).iter().any(|c| c.device == device);
        if open {
            self.close(device);
            self.let_go(app, device);
        }
        self.save(|s| {
            if model {
                s.profiles.remove(device);
            } else {
                s.profiles.insert(device.to_owned(), file.to_owned());
            }
        });
        if open {
            self.open(app, device)?;
        }
        Ok(())
    }

    pub fn learn(&self, target: String, learnt: Learnt) {
        *lock(&self.learning) = Some(Learning { target, learnt });
    }

    /// Changes how the control of `target` on `device` is read (an encoder's steps). A
    /// control of a profile becomes the DJ's own, read that way.
    pub fn set_encoding(&self, device: &str, target: &str, encoding: Encoding) {
        let of_target = |m: &&Mapping| m.device == device && m.target == target;
        let profiled: Vec<Mapping> = lock(&self.mappings).all().iter().filter(of_target).cloned().collect();
        self.save(|s| {
            if !s.mappings.all().iter().any(|m| of_target(&m)) {
                // each of the profile's controls of the action, as many as it has
                for mapping in profiled {
                    s.mappings.learn(mapping, true);
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
            .filter_map(|device| self.profile(device))
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
            .filter_map(|device| self.profile(device))
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
        let profile = self.profile(device);
        let (modes, shifts, held) = {
            let reader = lock(&self.reader);
            (reader.modes(device), reader.shifts(device), reader.held(device))
        };
        let hid = profile.as_ref().and_then(|p| p.hid.as_ref());
        // in the order a key read takes them
        let mappings = lock(&self.mappings);
        let states = lock(&self.states);
        let keys = lock(&self.keys);
        let state_of = |target: &str| match selected_mode(target) {
            Some((group, mode)) => Some(if modes.get(group).is_some_and(|m| m == mode) { ON } else { OFF }),
            None => states.get(target).map(|s| s.state.as_str()),
        };
        // the colour of its action, by the nearest of its palette, else on a light of three colours
        let coloured = |target: &str, palette: &BTreeMap<String, u8>, light: Control, value: LightValue| {
            let colour = states
                .get(target)
                .and_then(|s| s.color.as_deref())
                .filter(|_| state_of(target).is_some_and(|s| s != OFF));
            let rgb = |colour| match (light, hid) {
                (Control::Hid(led), Some(hid)) if led.bits == hid::RGB_BITS => {
                    hid::rgb(colour, hid.brightness)
                }
                _ => None,
            };
            match colour {
                Some(c) if !palette.is_empty() => nearest(palette, c).map_or(value, LightValue::Steady),
                Some(c) => rgb(c).map_or(value, |rgb| LightValue::Message(rgb.to_vec())),
                None => value,
            }
        };
        let mut lights: Vec<(Control, LightValue)> = Vec::new();
        for mapping in mappings.shown(device, &shifts, &modes) {
            let Some(light) = light_of(hid, mapping.control) else { continue };
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
                let value = coloured(&mapping.target, &mapping.palette, light, mapping.light.value(state));
                lights.push((light, value));
            }
        }
        for extra in profile.iter().flat_map(|p| &p.lights) {
            if let Some(value) = state_of(&extra.target).and_then(|s| extra.value(s, &modes)) {
                lights.push((extra.control, coloured(&extra.target, &extra.palette, extra.control, value)));
            }
        }
        lights
    }

    /// The ports the controllers are open on.
    fn ports_open(&self) -> Vec<String> {
        lock(&self.connections).iter().map(|c| c.port.clone()).collect()
    }

    /// Takes out the connections `which` picks, closed once the lock is let go: closing one
    /// waits for its callback, which may be waiting for the lock (see [`Self::receive`]).
    fn take_connections(&self, which: impl Fn(&Connection) -> bool) -> Vec<Connection> {
        lock(&self.connections).extract_if(.., |c| which(c)).collect()
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
        let profile = self.profile(device);
        let mappings = lock(&self.mappings);
        let hid = profile.as_ref().and_then(|p| p.hid.as_ref());
        for mapping in mappings.all().iter().filter(|m| m.device == device) {
            if let Some(light) = light_of(hid, mapping.control) {
                connection.light(light, mapping.light.value(OFF));
            }
        }
        if let Some(profile) = profile {
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

    /// Closes every controller, as Prisme quits, and waits for what was learnt to be saved.
    pub fn close_all(&self) {
        let devices: Vec<String> = lock(&self.connections).iter().map(|c| c.device.clone()).collect();
        for device in devices {
            self.close(&device);
        }
        self.setup.flush();
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
                    let Some(profile) = controllers.profile(&connection.device) else { continue };
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
        if hid::ids_of(device).is_some() {
            return self.open_hid(app, device);
        }
        let mut input = MidiInput::new(CLIENT).map_err(midi_error)?;
        input.ignore(Ignore::All);
        let ports = input.ports();
        let names: Vec<String> = ports.iter().map(|p| input.port_name(p).unwrap_or_default()).collect();
        let at = port_of(&names, device, &self.ports_open())
            .ok_or_else(|| AppError::ControllerUnplugged(device.to_owned()))?;
        let (port, port_name) = (&ports[at], names[at].clone());
        let (app, name) = (app.clone(), device.to_owned());
        let jog_steps_per_turn = self.jog_steps_per_turn(device);
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
        let output = open_output(&port_name);
        self.opened(device, port_name, Link::Midi { input: Some(connection), output });
        Ok(())
    }

    /// Opens the HID controller `device`: its reports are read from its own thread.
    fn open_hid(&self, app: &AppHandle, device: &str) -> AppResult<()> {
        let plugged = hid_plugged();
        let names: Vec<String> = plugged.iter().map(|p| p.name.clone()).collect();
        let at = port_of(&names, device, &self.ports_open())
            .ok_or_else(|| AppError::ControllerUnplugged(device.to_owned()))?;
        let profile = self.profile(device);
        // a profile's fields tell whether the device numbers its reports, else its descriptor does
        let numbered = profile.as_ref().and_then(|p| p.hid_inputs().first().map(|f| f.report != 0));
        let (app, name) = (app.clone(), device.to_owned());
        let jog_steps_per_turn = self.jog_steps_per_turn(device);
        let link = controller_hid::Link::open(
            &plugged[at],
            profile.as_ref().and_then(|p| p.hid.as_ref()),
            numbered,
            move |read, at_s| {
                app.state::<Controllers>().receive_hid(&app, &name, &read, jog_steps_per_turn, at_s);
            },
        )
        .map_err(AppError::Hid)?;
        self.opened(device, names[at].clone(), Link::Hid(link));
        Ok(())
    }

    /// `device` opened on `port`: it is sent what its profile sends first (some of it later),
    /// its groups are in their first modes, and its lights show.
    fn opened(&self, device: &str, port: String, link: Link) {
        let mut connection = Connection {
            device: device.to_owned(),
            port,
            link,
            kept_alive: Instant::now(),
            lit: Lit::default(),
            later: Vec::new(),
            clipped: HashMap::new(),
            displayed: HashMap::new(),
        };
        let profile = self.profile(device);
        for (after_ms, message) in profile.iter().flat_map(|p| p.init.iter().map(|m| m.when_and_what())) {
            if after_ms == 0 {
                connection.send(message);
            } else {
                let at = Instant::now() + Duration::from_millis(u64::from(after_ms));
                connection.later.push((at, message.to_vec()));
            }
        }
        lock(&self.reader).set_modes(device, profile.map(|p| p.first_modes()).unwrap_or_default());
        lock(&self.connections).push(connection);
        self.show_lights(device);
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
        if event.control.is_key() || layer() != before {
            self.show_lights(device);
        }
        match routed {
            Some(Routed::Learned(mapping)) => drop(app.emit("controller-learned", mapping)),
            Some(Routed::Actions(actions)) => self.act(app, actions, at_s, jog_steps_per_turn),
            None => {}
        }
    }

    /// A report of the HID controller `device`, at `at_s` on its clock (see [`Self::read_hid`]).
    fn receive_hid(&self, app: &AppHandle, device: &str, read: &Read, jog_steps_per_turn: f64, at_s: f64) {
        match self.read_hid(device, read) {
            Reported::Learnt(mapping) => drop(app.emit("controller-learned", mapping)),
            Reported::Events(events) => {
                for event in events {
                    self.receive(app, device, jog_steps_per_turn, event, at_s);
                }
            }
        }
    }

    /// What a report of the HID controller `device` tells: the control learnt, if one is waited
    /// for and the report moved one, else an event for each field read that changed.
    fn read_hid(&self, device: &str, read: &Read) -> Reported {
        let fields = lock(&self.hid_fields).get(device).cloned().unwrap_or_default();
        let learning = lock(&self.learning).as_ref().map(|l| l.learnt);
        if let Some(learnt) = learning {
            let mut known = self.profile(device).map(|p| p.hid_inputs()).unwrap_or_default();
            known.extend(fields.iter());
            if let Some(mapping) =
                read.learnt(&known, learnt).and_then(|event| self.learn_from(device, event))
            {
                return Reported::Learnt(Box::new(mapping));
            }
        }
        Reported::Events(read.events(&fields))
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
        let jog_steps_per_turn = self.jog_steps_per_turn(device);
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
        if let Some(learnt) = self.complete_fine(device, event).or_else(|| self.learn_from(device, event)) {
            return Some(Routed::Learned(Box::new(learnt)));
        }
        let mut reader = lock(&self.reader);
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

    /// Learns the control of `event` of `device` for the action waiting for one, if one is: but
    /// for a key let go, the end of a press made before, and shift.
    fn learn_from(&self, device: &str, event: Event) -> Option<Mapping> {
        let reader = lock(&self.reader);
        let shift = lock(&self.mappings)
            .find(device, Event { raw: 127, ..event }, &Shifts::default(), &Modes::new())
            .is_some_and(|m| m.target == SHIFT);
        let released = event.control.is_key() && event.raw == 0;
        let Learning { target, learnt } = lock(&self.learning).take_if(|_| !released && !shift)?;
        let binding = ProfileControl {
            control: event.control,
            target,
            encoding: if learnt == Learnt::Encoder { Encoding::guess(event) } else { Encoding::Absolute },
            shift: reader.shifted(device).into(),
            ..Default::default()
        };
        drop(reader);
        let mapping = Mapping::new(device, binding);
        self.save(|s| {
            let unmapped = Unmapped::On { device: device.to_owned(), target: mapping.target.clone() };
            s.unmapped.retain(|u| *u != unmapped);
            s.mappings.learn(mapping.clone(), Direct::takes_several(&mapping.target));
        });
        *lock(&self.just_learnt) = Some((mapping.clone(), Instant::now()));
        Some(mapping)
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
                    let jog_steps_per_turn = self.jog_steps_per_turn(&device);
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
        let fine = match mapping.control {
            Control::ControlChange(coarse) if coarse.number < 32 => Some(coarse.number + 32),
            _ => None,
        };
        let follows = fine.is_some_and(|fine| mapping.control.with_number(fine) == Some(event.control))
            && mapping.encoding == Encoding::Absolute;
        let Some(fine) = fine.filter(|_| follows) else {
            *just_learnt = Some((mapping, at));
            return None;
        };
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
        self.setup.update(change);
        self.layer();
    }

    /// Lays what was learnt over the profiles of the controllers in use.
    fn layer(&self) {
        let library = controller_profiles::current();
        let layered = self.setup.read(|s| {
            let profiled: Vec<Mapping> = s
                .enabled
                .iter()
                .filter_map(|device| {
                    let chosen = s.profiles.get(device).map(String::as_str);
                    Some(library.chosen(device, chosen)?.profile.mappings(device).collect::<Vec<_>>())
                })
                .flatten()
                .collect();
            let unmapped = |m: &Mapping| s.unmapped.iter().any(|u| u.covers(m));
            Mappings::layered(profiled, &s.mappings, unmapped, Direct::takes_several)
        });
        let mut fields: HashMap<String, BTreeSet<Field>> = HashMap::new();
        for m in layered.all() {
            for control in std::iter::once(m.control).chain(m.chord.as_ref().map(|c| c.with)) {
                if let Control::Hid(field) = control {
                    fields.entry(m.device.clone()).or_default().insert(field);
                }
            }
        }
        *lock(&self.mappings) = layered;
        *lock(&self.hid_fields) =
            fields.into_iter().map(|(device, fields)| (device, Arc::new(fields))).collect();
    }
}

/// Where the light of `control` is: on the control itself for MIDI, the LED its HID profile
/// (`hid`) gives a key.
fn light_of(hid: Option<&hid::HidDevice>, control: Control) -> Option<Control> {
    match control {
        Control::Hid(_) => hid?.led(control),
        _ => Some(control),
    }
}

/// The HID devices plugged in that may be controllers, each by the interface its profile reads.
pub(crate) fn hid_plugged() -> Vec<controller_hid::Plugged> {
    controller_hid::plugged(controller_profiles::hid_fit)
}

/// The controllers plugged in, the MIDI ports `midi` and the HID devices `hid`, a controller
/// that is both used one way only: by HID if Prisme knows it so and not by MIDI, else by MIDI.
fn with_hid(mut midi: Vec<String>, hid: &[controller_hid::Plugged]) -> Vec<String> {
    let known = |name: &str| controller_profiles::of(name).is_some();
    let mut plugged = Vec::with_capacity(midi.len() + hid.len());
    for device in hid {
        let product = device.product.to_lowercase();
        let same = |port: &String| product.len() >= 4 && port.to_lowercase().contains(&product);
        match midi.iter().position(same) {
            Some(at) if known(&device.name) && !known(&midi[at]) => drop(midi.remove(at)),
            Some(_) => continue,
            None => {}
        }
        plugged.push(device.name.clone());
    }
    midi.extend(plugged);
    midi
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
    let port = &ports[port_of(&names, device, &[])?];
    output.connect(port, CLIENT).ok()
}

fn port_names(input: &MidiInput) -> Vec<String> {
    input.ports().iter().filter_map(|p| input.port_name(p).ok()).collect()
}

/// Of the ports `names` but those `taken` (another controller of the same model's), the one of
/// the controller saved as `device`: its name, else the same wherever it is plugged (see
/// [`base_port_name`]).
pub(crate) fn port_of(names: &[String], device: &str, taken: &[String]) -> Option<usize> {
    let free = |n: &String| !taken.contains(n);
    names.iter().position(|n| n == device && free(n)).or_else(|| {
        let base = base_port_name(device);
        names.iter().position(|n| base_port_name(n) == base && free(n))
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
        assert_eq!(port_of(&plugged, "DDJ-400:DDJ-400 MIDI 1 24:0", &[]), Some(1));
        assert_eq!(port_of(&plugged, "DDJ-400:DDJ-400 MIDI 2 24:0", &[]), None);
    }

    #[test]
    fn a_controller_is_found_on_another_usb_port() {
        let names = ["2- DJControl Inpulse 200".to_owned(), "Other".to_owned()];
        assert_eq!(port_of(&names, "DJControl Inpulse 200", &[]), Some(0));
        let both = ["DJControl Inpulse 200".to_owned(), "2- DJControl Inpulse 200".to_owned()];
        assert_eq!(port_of(&both, "2- DJControl Inpulse 200", &[]), Some(1), "the same name first");
        assert_eq!(port_of(&names, "FLX4", &[]), None);
    }

    #[test]
    fn two_controllers_of_one_model_are_found_on_two_ports() {
        let plugged = ["Traktor Kontrol X1".to_owned(), "3- Traktor Kontrol X1".to_owned()];
        let taken = [plugged[0].clone()];
        assert_eq!(port_of(&plugged, "2- Traktor Kontrol X1", &taken), Some(1));
        assert_eq!(port_of(&plugged, "Traktor Kontrol X1", &plugged), None, "both in use");
        let alsa = ["X1:X1 MIDI 1 32:0".to_owned(), "X1:X1 MIDI 1 36:0".to_owned()];
        assert_eq!(port_of(&alsa, "X1:X1 MIDI 1 28:0", &alsa[..1]), Some(1));
    }
    use prisme_control::{parse, Chord, Control, LightValues, Meter, Shift, When};

    const PAD: Control = Control::note(0, 36);

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
        c.learn("deck1.play".into(), Learnt::Button);
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
        c.learn("library.browse".into(), Learnt::Encoder);
        route(&c, &[0xb0, 64, 1]);
        assert_eq!(route(&c, &[0xb0, 64, 127]), input("library.browse", -1.0));
        // a jog sending steps around 64
        c.learn("deck1.jog".into(), Learnt::Encoder);
        route(&c, &[0xb0, 33, 65]);
        assert_eq!(route(&c, &[0xb0, 33, 61]), direct(Direct::Jog(0), -3.0));
    }

    #[test]
    fn a_fader_sending_its_low_half_is_learnt_14_bit() {
        let c = controllers();
        c.learn("deck1.pitch".into(), Learnt::Fader);
        route(&c, &[0xb0, 0, 64]);
        let fine = route(&c, &[0xb0, 32, 0]);
        let tempo = Control::control_change(0, 0);
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
        c.learn(SHIFT.into(), Learnt::Button);
        route(&c, &[0x90, 63, 127]);
        route(&c, &[0x90, 63, 0]);
        c.learn("deck1.cue".into(), Learnt::Button);
        route(&c, &[0x90, 36, 127]);
        c.learn("deck1.hotcue1.delete".into(), Learnt::Button);
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
        c.learn("deck1.cue".into(), Learnt::Button);
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
        c.learn("deck1.play".into(), Learnt::Button);
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
    fn an_action_on_several_controls_of_a_profile_keeps_them_all_read_another_way() {
        let c = sb2();
        let amount = |m: &&Mapping| m.device == "DDJ-SB2" && m.target == "deck1.fx.amount";
        let profiled: Vec<Mapping> = c.mappings().all().iter().filter(amount).cloned().collect();
        assert!(profiled.len() > 1);
        c.set_encoding("DDJ-SB2", "deck1.fx.amount", Encoding::RelativeOffset);
        let read: Vec<Mapping> = c.mappings().all().iter().filter(amount).cloned().collect();
        let expected: Vec<Mapping> =
            profiled.into_iter().map(|m| with(m, |b| b.encoding = Encoding::RelativeOffset)).collect();
        assert_eq!(read, expected);
    }

    #[test]
    fn a_controller_plays_by_the_variant_chosen_for_it() {
        let c = controllers();
        c.save(|s| s.enabled.push("VCI-100".into()));
        let touch = |c: &Controllers, note| c.route("VCI-100", parse(&[0x90, note, 127]).unwrap(), 0.0);
        assert_eq!(c.model_name("VCI-100"), "Vestax VCI-100");
        assert_eq!(touch(&c, 48), direct(Direct::JogTouch(0), 1.0));
        c.save(|s| {
            s.profiles.insert("VCI-100".into(), "vestax-vci-100-hile.json".into());
        });
        assert_eq!(c.model_name("VCI-100"), "Vestax VCI-100 (Hile Mod)");
        assert_eq!(c.jog_steps_per_turn("VCI-100"), 256.0);
        assert_eq!(touch(&c, 46), direct(Direct::JogTouch(0), 1.0));
    }

    #[test]
    fn a_profile_chosen_that_is_not_offered_for_a_controller_leaves_it_its_models() {
        let c = sb2();
        c.save(|s| {
            s.profiles.insert("DDJ-SB2".into(), "pioneer-ddj-400.json".into());
        });
        assert_eq!(c.model_name("DDJ-SB2"), "Pioneer DDJ-SB2");
        assert_eq!(c.route("DDJ-SB2", parse(&[0x90, 0x0b, 127]).unwrap(), 0.0), input("deck1.play", 1.0));
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
        c.learn("deck1.play".into(), Learnt::Button);
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
        let other = Control::note(0, 37);
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
        let key = Control::note(0, 9);
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
        let key = Control::note(0, 9);
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
        let (other, third) = (Control::note(0, 37), Control::note(0, 38));
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
        let shift_key = Control::note(0, 63);
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
            .filter_map(|(control, value)| lit.light(control, value)?.midi())
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
        let knob = Control::control_change(0, 20);
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

    /// A Traktor Kontrol S4 MK2 in use, its short report (keys and jogs) at rest, its long one
    /// (faders and encoders) too.
    fn s4() -> (Controllers, String, prisme_control::hid::Reports) {
        let device = hid::device_name("Traktor Kontrol S4 MK2", 0x17cc, 0x1310);
        let c = controllers();
        c.save(|s| s.enabled.push(device.clone()));
        let modes = controller_profiles::of(&device).map(|p| p.first_modes()).unwrap_or_default();
        lock(&c.reader).set_modes(&device, modes);
        let mut reports = prisme_control::hid::Reports::new(true);
        for report in [short(&[]), long(&[])] {
            driven(&c, &device, &mut reports, &report);
        }
        (c, device, reports)
    }

    /// The short report with `bits` (byte, mask) set.
    fn short(bits: &[(usize, u8)]) -> Vec<u8> {
        let mut report = vec![0; 21];
        report[0] = 1;
        for &(byte, mask) in bits {
            report[byte] |= mask;
        }
        report
    }

    /// The long report with `bytes` (byte, value) set.
    fn long(bytes: &[(usize, u8)]) -> Vec<u8> {
        let mut report = vec![0; 79];
        report[0] = 2;
        for &(byte, value) in bytes {
            report[byte] = value;
        }
        report
    }

    /// What `report` drives on the S4.
    fn driven(
        c: &Controllers,
        device: &str,
        reports: &mut prisme_control::hid::Reports,
        report: &[u8],
    ) -> Vec<Routed> {
        match c.read_hid(device, &reports.read(report)) {
            Reported::Events(events) => events.into_iter().filter_map(|e| c.route(device, e, 0.0)).collect(),
            Reported::Learnt(mapping) => vec![Routed::Learned(mapping)],
        }
    }

    fn ui(pairs: &[(&str, f32)]) -> Vec<Routed> {
        pairs.iter().map(|&(t, v)| Routed::Actions(vec![Action::Ui(ControllerInputDto::of(t, v))])).collect()
    }

    #[test]
    fn a_hid_controller_plays_its_profile_from_its_reports() {
        let (c, device, mut reports) = s4();
        let mut drive = |report: Vec<u8>| driven(&c, &device, &mut reports, &report);
        // play, on the left
        assert_eq!(drive(short(&[(0x0d, 0x01)])), ui(&[("deck1.play", 1.0)]));
        assert_eq!(drive(short(&[])), ui(&[("deck1.play", 0.0)]));
        // the deck switch: the left side is deck C, its play deck 3's
        drive(short(&[(0x0f, 0x20)]));
        drive(short(&[]));
        assert_eq!(drive(short(&[(0x0d, 0x01)])), ui(&[("deck3.play", 1.0)]));
        drive(short(&[]));
        // the left shift: play is the key lock, of the left side only
        drive(short(&[(0x0d, 0x08)]));
        assert_eq!(drive(short(&[(0x0d, 0x09)])), ui(&[("deck3.keylock", 1.0)]));
        // the right side's play read first: its byte comes first
        assert_eq!(
            drive(short(&[(0x0d, 0x08), (0x0c, 0x01)])),
            ui(&[("deck2.play", 1.0), ("deck3.keylock", 0.0)])
        );
    }

    #[test]
    fn a_hid_jog_counter_turns_its_deck_and_faders_move_by_their_12_bits() {
        let (c, device, mut reports) = s4();
        let mut drive = |report: Vec<u8>| driven(&c, &device, &mut reports, &report);
        assert_eq!(drive(short(&[(0x01, 6)])), [direct(Direct::Jog(0), 6.0).unwrap()]);
        assert_eq!(drive(short(&[(0x01, 250)])), [direct(Direct::Jog(0), -12.0).unwrap()], "wrapped at 256");
        assert_eq!(drive(short(&[(0x01, 250), (0x11, 0x01)])), [direct(Direct::JogTouch(0), 1.0).unwrap()]);
        // the crossfader at 4095, the left tempo fader at 2048, its 0 the fastest
        let routed = drive(long(&[(0x07, 0xff), (0x08, 0x0f), (0x09, 0x00), (0x0a, 0x08)]));
        let mut expected = ui(&[("mixer.crossfader", 1.0)]);
        expected.extend(direct(Direct::Pitch(0), 1.0 - 2048.0 / 4095.0));
        assert_eq!(routed, expected);
        // the browse knob, a counter of four bits: a step back from 0 is -1
        assert_eq!(
            drive(long(&[(0x07, 0xff), (0x08, 0x0f), (0x0a, 0x08), (0x02, 0x0f)])),
            [Routed::Actions(vec![Action::Ui(ControllerInputDto {
                steps: true,
                ..ControllerInputDto::of("library.browse", -1.0)
            })])]
        );
    }

    #[test]
    fn a_hid_controllers_lights_are_its_keys_leds_in_its_output_reports() {
        let (c, device, _) = s4();
        let led = |report, byte, bits| Control::Hid(Field { report, byte, bit: 0, bits, big_endian: false });
        c.set_lights(&[
            light("deck1.play", ON),
            LightDto { target: "deck1.hotcue1".into(), state: ON.into(), color: Some("#ff0000".into()) },
            light("deck1.hotcue2", OFF),
        ]);
        let lights = c.lights(&device);
        assert!(lights.contains(&(led(0x81, 0x20, 8), LightValue::Steady(127))), "play");
        assert!(lights.contains(&(led(0x81, 0x01, 24), LightValue::Message(vec![127, 0, 0]))), "its colour");
        assert!(lights.contains(&(led(0x81, 0x04, 24), LightValue::Message(vec![8, 1, 1]))), "dim red");
        // the left side on deck C: the same LEDs show deck 3
        lock(&c.reader).set_mode(&device, "left", "C");
        c.set_lights(&[light("deck3.play", OFF)]);
        assert!(c.lights(&device).contains(&(led(0x81, 0x20, 8), LightValue::Steady(9))));
        assert!(c.lights(&device).contains(&(led(0x81, 0x31, 8), LightValue::Steady(127))), "deck C lit");
    }

    #[test]
    fn a_control_of_a_hid_controller_is_learnt_by_the_field_that_changed() {
        let (c, device, mut reports) = s4();
        c.learn("deck1.censor".into(), Learnt::Button);
        let learnt = driven(&c, &device, &mut reports, &short(&[(0x0e, 0x02)]));
        let slip = Control::Hid(Field { report: 1, byte: 0x0e, bit: 1, bits: 1, big_endian: false });
        let expected = Mapping::new(
            &device,
            ProfileControl { control: slip, target: "deck1.censor".into(), ..Default::default() },
        );
        assert_eq!(learnt, [Routed::Learned(Box::new(expected))]);
        driven(&c, &device, &mut reports, &short(&[]));
        assert_eq!(driven(&c, &device, &mut reports, &short(&[(0x0e, 0x02)])), ui(&[("deck1.censor", 1.0)]));
        // an encoder of no profile: the counter's nibble that turned
        c.learn("deck1.size".into(), Learnt::Encoder);
        let learnt = driven(&c, &device, &mut reports, &long(&[(0x10, 0x30)]));
        let Some(Routed::Learned(mapping)) = learnt.first() else { panic!("{learnt:?}") };
        assert_eq!(
            mapping.control,
            Control::Hid(Field { report: 2, byte: 0x10, bit: 4, bits: 4, big_endian: false })
        );
        assert_eq!(mapping.encoding, Encoding::AbsoluteWrap);
    }

    #[test]
    fn a_controller_both_midi_and_hid_is_used_one_way() {
        let hid = |product: &str, vendor_id, product_id| {
            controller_hid::Plugged::named(product, vendor_id, product_id)
        };
        let s4 = hid("Traktor Kontrol S4 MK2", 0x17cc, 0x1310);
        let other = hid("DJ Console Mk2", 0x06f8, 0xb100);
        let plugged = with_hid(
            vec!["Traktor Kontrol S4 MK2 MIDI".into(), "DJ Console Mk2 MIDI".into(), "DDJ-400".into()],
            &[s4.clone(), other],
        );
        // the S4 by HID, its profile's; the Hercules by MIDI, which Prisme knows it by
        assert_eq!(plugged, ["DJ Console Mk2 MIDI", "DDJ-400", s4.name.as_str()]);
    }
}
