//! The controllers plugged in and opened, MIDI and HID: looked for as they come and go, opened,
//! closed, let go.

use super::{
    base_port_name, hid_plugged, light_of, midi_error, open_output, port_names, port_of, with_hid,
    Connection, Controllers, JogSteps, Lighting, Link, Lit, MidiWriter, PluggedIn, Shade, CLIENT,
    PLUGGED_EVENT, WATCH_EVERY, WATCH_WITHOUT_MIDI_EVERY,
};
use crate::actions::{Host, InTurn, Moved, Source};
use crate::controller_hid;
use crate::controller_profiles;
use crate::dto::OpenedDto;
use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState};
use midir::{Ignore, MidiInput, MidiOutputConnection};
use prisme_actions::input::Input;
use prisme_control::hid::{self};
use prisme_control::{Control, DeckAction, Event, Profile, ShownRoom, Target, OFF};
use std::sync::mpsc::{self};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tracing::{error, info, warn};

impl Controllers {
    /// The controllers plugged in: the MIDI ports, and the HID devices that may be controllers.
    pub fn plugged() -> Vec<String> {
        let midi = MidiInput::new(CLIENT).map(|input| port_names(&input)).unwrap_or_else(|e| {
            warn!("MIDI ports not listed: {e}");
            Vec::new()
        });
        with_hid(midi, &hid_plugged())
    }

    /// Opens the controllers in use that are plugged in, and those of a model Prisme knows,
    /// unless the DJ turned them off.
    pub fn open_at_start(&self, app: &AppHandle) {
        for device in self.setup.read(|s| s.enabled.clone()) {
            if let Err(e) = self.open(app, &device) {
                warn!("controller {device} not opened: {e}");
            }
        }
        let plugged = Self::plugged();
        for port in &plugged {
            if self.new_to_the_dj(port) && controller_profiles::of(port).is_some() {
                self.take_up(app, port);
            }
        }
        *lock(&self.plugged) = plugged;
        self.show(&app.state::<AppState>());
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
                            let model = controllers.model_name(&device);
                            app.tell("controller-opened", OpenedDto { device, model });
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
                *lock(&controllers.plugged) = plugged.clone();
                controllers.show(&app.state::<AppState>());
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
    pub(super) fn plug_in(&self, app: &AppHandle, port: &str) {
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
        app.tell(PLUGGED_EVENT, news);
    }

    /// Whether the controller on `port` is neither in use nor one the DJ turned off or chose
    /// not to use. The port Linux loops MIDI back through is no controller.
    pub(super) fn new_to_the_dj(&self, port: &str) -> bool {
        let base = base_port_name(port);
        let same = |d: &String| base_port_name(d) == base;
        !base.starts_with("Midi Through")
            && self.setup.read(|s| !s.enabled.iter().any(same) && !s.declined.iter().any(same))
    }

    /// Puts the controller on `port` in use; whether it could be opened. Held by another
    /// program, it is in use all the same: opened as soon as that program lets go (see
    /// [`Self::watch`]).
    pub(super) fn take_up(&self, app: &AppHandle, port: &str) -> bool {
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

    /// The ports the controllers are open on.
    pub(super) fn ports_open(&self) -> Vec<String> {
        lock(&self.connections).iter().map(|c| c.port.clone()).collect()
    }

    /// Takes out the connections `which` picks, closed once the lock is let go: closing one
    /// waits for its callback, which may be waiting for the lock (see [`Self::receive`]).
    pub(super) fn take_connections(&self, which: impl Fn(&Connection) -> bool) -> Vec<Connection> {
        lock(&self.connections).extract_if(.., |c| which(c)).collect()
    }

    /// Closes `device`: its lights go off, its meters show silence, and its profile's last
    /// messages go out.
    pub(super) fn close(&self, device: &str) {
        let connection = {
            let mut connections = lock(&self.connections);
            let at = connections.iter().position(|c| c.device == device);
            at.map(|at| connections.remove(at))
        };
        let Some(mut connection) = connection else { return };
        let profile = connection.profile.clone();
        let mappings = lock(&self.mappings);
        let hid = profile.as_ref().and_then(|p| p.hid.as_ref());
        for mapping in mappings.of(device) {
            if let Some(light) = light_of(hid, mapping.control) {
                connection.light(light, Shade::Given(mapping.light.value(OFF)));
            }
        }
        if let Some(profile) = profile {
            for extra in &profile.lights {
                connection.light(extra.control, Shade::Given(extra.values.value(OFF)));
            }
            for meter in &profile.meters {
                meter.show(0.0, false, |control, value| connection.light(control, Shade::Given(value)));
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

    pub(super) fn open(&self, app: &AppHandle, device: &str) -> AppResult<()> {
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
        let (receiving, name) = (app.clone(), device.to_owned());
        let jog = JogSteps::new(self.jog_steps_per_turn(device));
        let read = jog.clone();
        let connection = input
            .connect(
                port,
                CLIENT,
                move |stamp_us, bytes, _| {
                    if let Some(event) = prisme_control::parse(bytes) {
                        let at_s = stamp_us as f64 / 1e6;
                        let controllers = receiving.state::<Controllers>();
                        controllers.receive(&receiving, &name, read.get(), event, at_s);
                    }
                },
                (),
            )
            .map_err(midi_error)?;
        let output = open_output(&port_name).unwrap_or_else(|e| {
            warn!("controller {device} opened without its lights: {e}");
            None
        });
        self.opened(app, device, port_name, jog, Link::Midi { input: Some(connection), output });
        Ok(())
    }

    /// Opens the HID controller `device`: its reports are read from its own thread.
    pub(super) fn open_hid(&self, app: &AppHandle, device: &str) -> AppResult<()> {
        let plugged = hid_plugged();
        let names: Vec<String> = plugged.iter().map(|p| p.name.clone()).collect();
        let at = port_of(&names, device, &self.ports_open())
            .ok_or_else(|| AppError::ControllerUnplugged(device.to_owned()))?;
        let profile = self.profile(device);
        // a profile's fields tell whether the device numbers its reports, else its descriptor does
        let numbered = profile.as_ref().and_then(|p| p.hid_inputs().first().map(|f| f.report != 0));
        let (receiving, name) = (app.clone(), device.to_owned());
        let jog = JogSteps::new(self.jog_steps_per_turn(device));
        let steps = jog.clone();
        let link = controller_hid::Link::open(
            &plugged[at],
            profile.as_ref().and_then(|p| p.hid.as_ref()),
            numbered,
            move |read, at_s| {
                let controllers = receiving.state::<Controllers>();
                controllers.receive_hid(&receiving, &name, &read, steps.get(), at_s);
            },
        )
        .map_err(AppError::Hid)?;
        self.opened(app, device, names[at].clone(), jog, Link::Hid(link));
        Ok(())
    }

    /// `device` opened on `port`, its jogs read by `jog`: it is sent what its profile sends
    /// first (some of it later), its groups are in their first modes, and its lights show.
    pub(super) fn opened(&self, host: &impl Host, device: &str, port: String, jog: JogSteps, link: Link) {
        let profile = self.profile(device);
        let mut connection = Connection::new(device, port, profile.clone(), jog, link);
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
        self.show_lights(host, device);
    }

    /// A controller gone lets go of what it held: its keys (shift among them) are released,
    /// and so are the jog wheels it touched, however they tell it.
    pub(super) fn let_go(&self, host: &impl Host, device: &str) {
        let jog_steps_per_turn = self.jog_steps_per_turn(device);
        let held = lock(&self.reader).held(device);
        for control in held {
            self.receive(host, device, jog_steps_per_turn, Event { control, raw: 0 }, 0.0);
        }
        let touched: Vec<usize> = lock(&self.mappings)
            .of(device)
            .filter_map(|m| match m.target.target() {
                Some(&Target::Deck(deck, DeckAction::JogTouch)) => Some(deck),
                _ => None,
            })
            .collect();
        let moved = Moved { device: device.into(), at_s: 0.0, jog_steps_per_turn };
        for deck in touched {
            let touch = Target::Deck(deck, DeckAction::JogTouch);
            host.perform(
                &touch,
                Input::Value(0.0),
                Source::Controller(moved.clone()),
                &mut InTurn::default(),
            );
        }
    }
}

/// A controller let go is closed, not only dropped: on Android, a port dropped keeps the device
/// open, and with it a sound card the USB driver would play on.
impl Drop for Connection {
    fn drop(&mut self) {
        if let Link::Midi { input, output } = &mut self.link {
            if let Some(input) = input.take() {
                input.close();
            }
            // its last messages sent, then closed
            drop(output.take());
        }
    }
}

impl MidiWriter {
    pub(super) fn new(mut output: MidiOutputConnection) -> std::io::Result<Self> {
        let (messages, waiting) = mpsc::channel::<Vec<u8>>();
        let writer = std::thread::Builder::new().name("prisme-midi-write".into()).spawn(move || {
            let mut failed = false;
            for message in waiting {
                // told once: a controller unplugged fails every message until it is closed
                if let Err(e) = output.send(&message) {
                    if !std::mem::replace(&mut failed, true) {
                        warn!("a controller's lights not sent: {e}");
                    }
                }
            }
            output.close();
        })?;
        Ok(Self { messages: Some(messages), writer: Some(writer) })
    }

    pub(super) fn send(&self, message: &[u8]) {
        if let Some(messages) = &self.messages {
            let _ = messages.send(message.to_vec());
        }
    }
}

/// Sends what is left to send, then closes the port.
impl Drop for MidiWriter {
    fn drop(&mut self) {
        drop(self.messages.take());
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

impl Link {
    pub(super) fn apply(&mut self, lighting: Lighting) {
        match (self, lighting) {
            (Link::Hid(link), Lighting::Value(Control::Hid(led), value)) => link.set(led, value),
            (Link::Hid(link), Lighting::Message(Control::Hid(led), bytes)) => link.set_bytes(led, bytes),
            (Link::Hid(_), _) => {}
            (link, Lighting::Value(control, value)) => {
                if let Some(message) = prisme_control::light(control, value) {
                    link.send(&message);
                }
            }
            (link, Lighting::Message(_, message)) => link.send(message),
        }
    }

    /// Sends a message of its own: a MIDI message, a whole HID report.
    pub(super) fn send(&mut self, message: &[u8]) {
        match self {
            Link::Midi { output: Some(output), .. } => output.send(message),
            Link::Midi { output: None, .. } => {}
            Link::Hid(link) => link.send(message),
            #[cfg(test)]
            Link::Sent(sent) => sent.push(message.to_vec()),
        }
    }
}

impl Connection {
    /// `device` opened on `port`, driven by `profile`, nothing shown nor sent yet.
    pub(super) fn new(
        device: &str,
        port: String,
        profile: Option<Arc<Profile>>,
        jog_steps: JogSteps,
        link: Link,
    ) -> Self {
        Connection {
            device: device.to_owned(),
            port,
            profile,
            jog_steps,
            link,
            kept_alive: Instant::now(),
            lit: Lit::default(),
            room: ShownRoom::default(),
            later: Vec::new(),
            clipped: Vec::new(),
            displayed: Vec::new(),
        }
    }

    /// Sets the light of `control` to `shade`, blinking or not.
    pub(super) fn light(&mut self, control: Control, shade: Shade) {
        if let Some(lighting) = self.lit.light(control, shade) {
            self.link.apply(lighting);
        }
    }

    /// Each blinking light at the value of the moment.
    pub(super) fn blink(&mut self) {
        let link = &mut self.link;
        self.lit.blink(|lighting| link.apply(lighting));
    }

    /// Sends a message of its own: a MIDI message, a whole HID report.
    pub(super) fn send(&mut self, message: &[u8]) {
        self.link.send(message);
    }

    /// Sends the messages of its profile whose time came.
    pub(super) fn send_due(&mut self) {
        let now = Instant::now();
        let Connection { later, link, .. } = self;
        later.retain(|(at, message)| {
            let due = *at <= now;
            if due {
                link.send(message);
            }
            !due
        });
    }
}
