//! Controllers, MIDI and HID (`controller_hid`). A mapped control's messages, or the fields of
//! a HID report that changed, drive the application's actions (`actions`), but those the
//! interface keeps (the library's search, its windows), which it is told of (`ui-action`). The lights show what the actions
//! read of the application (`prisme_actions::light`); they, the values shown on the
//! controllers, the lights that blink, the levels and the messages some models want again and
//! again go out at each tick of the backend's clock (`lights`). What is in use and what drives
//! what is `setup`; the ports and devices opened, `connection`; a control's messages read,
//! `keys`; a control learnt, `learn`.
//! Mappings are learnt from the controller itself and kept in `controllers.json`, with the
//! controllers in use; a model Prisme knows comes mapped by its profile
//! (`controller_profiles`), what is learnt on it taking the profile's place.
//! A controller seen both as a MIDI port and as a HID device (a Hercules with its driver) is
//! used one way only: by HID if Prisme knows it so and not by MIDI, else by MIDI, as its
//! maker's driver then speaks to the device itself.

mod connection;
mod keys;
mod learn;
mod lights;
pub(crate) mod midi;
mod setup;

use crate::controller_hid;
use crate::controller_profiles;
use crate::dto::ControllersDto;
use crate::error::{AppError, AppResult};
use midir::{MidiIO, MidiInput, MidiOutput, MidiOutputPort};
use prisme_actions::hold::Pressed;
use prisme_actions::input::Input;
use prisme_actions::perform::ClipHold;
use prisme_audio_io::base_device_name;
use prisme_control::hid::{self, Field, Learnt};
use prisme_control::{
    nearest, Control, Event, Hold, LightValue, Mapping, Mappings, Owner, Profile, Reader, ShownRoom, Target,
    TargetId,
};
use prisme_session::JsonFile;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// The low half of a 14-bit control follows its high half within this.
const FINE_HALF_WITHIN: Duration = Duration::from_millis(100);

pub(crate) const CLIENT: &str = "Prisme";
/// How often the controllers in use are looked for, to open again one plugged back: less
/// than a controller takes to come back on the USB bus, so that none goes unseen.
const WATCH_EVERY: Duration = Duration::from_millis(500);
/// How often MIDI is tried again on a system whose MIDI does not answer (no sequencer loaded,
/// on Linux).
const WATCH_WITHOUT_MIDI_EVERY: Duration = Duration::from_secs(10);
/// How often the levels shown on the controllers are sent.
pub const METERS_EVERY: Duration = Duration::from_millis(40);
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
/// A controller's action the interface performs (a `UiActionDto`).
pub const UI_ACTION_EVENT: &str = "ui-action";

/// What the DJ is told of a controller plugged in while Prisme runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
#[cfg_attr(test, derive(ts_rs::TS))]
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
    Learnt(Box<Mapping>),
    /// What the controller drives, in order.
    Actions(Vec<Action>),
}

/// An action a controller drives, performed by the interface or here, and how it is driven.
#[derive(Debug, PartialEq)]
enum Action {
    Ui(TargetId, Input),
    Rust(Target, Input),
}

/// A key pressed whose action waits to know whether it is held: held for `hold.ms`, it drives
/// `hold.target` (it acted), let go sooner its own.
struct Holding {
    device: String,
    control: Control,
    hold: Hold,
    since: Instant,
    /// When it was pressed, on its controller's clock; when it was let go, once its release
    /// waits for its hold.
    at_s: f64,
    /// Timed from `since`.
    pressed: Pressed,
}

struct Connection {
    /// The controller as saved.
    device: String,
    /// The port it was opened on: the same name, or with a number Windows added.
    port: String,
    /// Its profile, as chosen when it opened or when the profiles were read again.
    profile: Option<Arc<Profile>>,
    /// The steps of a turn of its jog wheels, as its profile says, shared with its reader.
    jog_steps: JogSteps,
    link: Link,
    /// When its keep-alive message last went.
    kept_alive: Instant,
    lit: Lit,
    /// Where the mappings its lights show are worked out, kept from one time to the next.
    room: ShownRoom,
    /// The messages of its profile to send later, and when.
    later: Vec<(Instant, Vec<u8>)>,
    /// When each of its meters last reached full scale, by its place among the meters.
    clipped: Vec<ClipHold>,
    /// The number each of its screens was last sent, by its place among the displays.
    displayed: Vec<Option<i64>>,
}

/// The steps of a turn of a controller's jog wheels, shared with the thread reading it: its
/// profile read again changes them there too.
#[derive(Debug, Clone)]
struct JogSteps(Arc<AtomicU64>);

impl JogSteps {
    fn new(steps: f64) -> Self {
        Self(Arc::new(AtomicU64::new(steps.to_bits())))
    }

    fn get(&self) -> f64 {
        f64::from_bits(self.0.load(Ordering::Relaxed))
    }

    fn set(&self, steps: f64) {
        self.0.store(steps.to_bits(), Ordering::Relaxed);
    }
}

/// How a controller is reached.
enum Link {
    Midi {
        /// Read from its own thread, closed once dropped.
        _input: midi::Input,
        /// Lights; not every controller has them.
        output: Option<MidiWriter>,
    },
    /// Read and written from its own thread.
    Hid(controller_hid::Link),
    /// A MIDI controller of the tests (`end_to_end`): the messages sent to it are kept.
    #[cfg(test)]
    Sent(Vec<Vec<u8>>),
}

/// A controller's MIDI output, written by a thread of its own: the thread of a key's message or
/// of the lights' tick never waits for the port (Windows sends a SysEx message a little at a
/// time), nor sends while it holds the controllers.
struct MidiWriter {
    messages: Option<Sender<Vec<u8>>>,
    writer: Option<JoinHandle<()>>,
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
#[derive(Debug, Clone, Copy, PartialEq)]
enum Lighting<'m> {
    Value(Control, u8),
    Message(Control, &'m [u8]),
}

/// What a light is set to: one of its profile's values, or one made for it (the value of its
/// palette nearest a colour, the bytes of a HID light of three colours).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shade<'v> {
    Given(&'v LightValue),
    Steady(u8),
    Rgb([u8; 3]),
}

/// A light showing its two values in turn, since `since`.
struct Blink {
    values: [u8; 2],
    ms: u32,
    since: Instant,
}

/// What the keys of the controllers do beyond driving their action, by controller.
#[derive(Default)]
struct KeyWays(HashMap<String, DeviceKeys>);

/// What the keys of a controller do beyond driving their action, by key.
#[derive(Default)]
struct DeviceKeys {
    /// Keys latched down.
    latched: HashSet<Control>,
    /// Keys putting a fader at two positions in turn, at their second.
    second: HashSet<Control>,
    /// When each key that drives another action pressed twice was last pressed.
    pressed_at: HashMap<Control, Instant>,
    /// Keys whose press drove another action than their own (pressed twice, or with another
    /// key held): their release lets that one go.
    instead: HashMap<Control, TargetId>,
}

/// What the interface shows of the controllers (see [`Controllers::show`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shown {
    pub listed: ControllersDto,
    /// The controller models known.
    pub profiles: Models,
    /// What drives what: the profiles of the controllers in use, and what was learnt.
    pub mappings: Vec<Mapping>,
    /// Where the MIDI clock goes.
    pub clock_output: Option<String>,
    /// The action the next control moved will drive.
    pub learning: Option<String>,
}

/// The controller models known, the same as long as they are not read again (compared by
/// pointer: the clock compares them often).
#[derive(Debug, Clone, Default)]
pub struct Models(pub Arc<controller_profiles::Library>);

impl PartialEq for Models {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

pub struct Controllers {
    setup: JsonFile<ControllerSetup>,
    /// The controllers plugged in when last looked for (see [`Controllers::plugged`]).
    plugged: Mutex<Vec<String>>,
    /// The profiles of the controllers in use, with what was learnt over them.
    mappings: Mutex<Mappings>,
    learning: Mutex<Option<Learning>>,
    /// The mapping just learnt: it becomes a 14-bit one if its low half follows.
    just_learnt: Mutex<Option<(Mapping, Instant)>>,
    reader: Mutex<Reader>,
    connections: Mutex<Vec<Connection>>,
    holdings: Mutex<Vec<Holding>>,
    keys: Mutex<KeyWays>,
    /// The fields of each HID controller's reports its mappings read.
    hid_fields: Mutex<HashMap<String, Arc<BTreeSet<Field>>>>,
}

/// Where the light of `control` is: on the control itself for MIDI, the LED its HID profile
/// (`hid`) gives a key.
fn light_of(hid: Option<&hid::HidDevice>, control: Control) -> Option<Control> {
    match control {
        Control::Hid(_) => hid?.led(control),
        _ => Some(control),
    }
}

/// The light `light` at `value`, or at the `colour` of its action: the value of its `palette`
/// nearest it, else, on a HID light of three colours, that colour.
fn coloured<'v>(
    value: &'v LightValue,
    colour: Option<u32>,
    palette: &BTreeMap<String, u8>,
    light: Control,
    hid: Option<&hid::HidDevice>,
) -> Shade<'v> {
    match (colour, light, hid) {
        (Some(c), _, _) if !palette.is_empty() => {
            nearest(palette, c).map_or(Shade::Given(value), Shade::Steady)
        }
        (Some(c), Control::Hid(led), Some(hid)) if led.bits == hid::RGB_BITS => {
            Shade::Rgb(hid::rgb(c, hid.brightness))
        }
        _ => Shade::Given(value),
    }
}

/// The HID devices plugged in that may be controllers, each by the interface its profile reads.
pub(crate) fn hid_plugged() -> Vec<controller_hid::Plugged> {
    controller_hid::plugged(controller_profiles::hid_fit)
}

/// A MIDI input plugged in: its name, and the id its system gives it (midir's): on Windows its
/// device's interface (`\\?\usb#vid_06f8&pid_b105&mi_00#…`), shared by the device's ports.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MidiPort {
    pub name: String,
    pub id: String,
}

/// The MIDI inputs plugged in.
pub(crate) fn input_ports(input: &MidiInput) -> Vec<MidiPort> {
    input
        .ports()
        .iter()
        .filter_map(|p| Some(MidiPort { name: input.port_name(p).ok()?, id: p.id() }))
        .collect()
}

/// The USB vendor and product ids a port's id holds (`…#vid_06f8&pid_b105…`), on Windows.
fn usb_ids(id: &str) -> Option<(u16, u16)> {
    let id = id.to_lowercase();
    let hex = |key: &str| {
        let at = id.find(key)? + key.len();
        u16::from_str_radix(id.get(at..at + 4)?, 16).ok()
    };
    Some((hex("vid_")?, hex("pid_")?))
}

/// The controllers plugged in, the MIDI ports `midi` and the HID devices `hid`, a controller
/// that is both used one way only: by HID if Prisme knows it so and not by MIDI, else by MIDI.
/// A port is of a HID device when its id holds the device's USB ids; where the system gives a
/// port no such id, when its name begins with the device's product name.
fn with_hid(midi: Vec<MidiPort>, hid: &[controller_hid::Plugged]) -> Vec<String> {
    let known = |name: &str| controller_profiles::of(name).is_some();
    let mut midi: Vec<MidiPort> = midi;
    let mut plugged = Vec::with_capacity(midi.len() + hid.len());
    for device in hid {
        let (ids, product) = ((device.info.vendor_id, device.info.product_id), device.product.to_lowercase());
        let same = |port: &MidiPort| match usb_ids(&port.id) {
            Some(usb) => usb == ids,
            None => product.len() >= 4 && port.name.to_lowercase().starts_with(&product),
        };
        match midi.iter().position(same) {
            Some(at) if known(&device.name) && !known(&midi[at].name) => drop(midi.remove(at)),
            Some(_) => continue,
            None => {}
        }
        plugged.push(device.name.clone());
    }
    midi.into_iter().map(|port| port.name).chain(plugged).collect()
}

/// What `mapping` driven to `value` drives: its action and those it drives along, but a shift's
/// and a mode's, which the reader keeps.
fn inputs(mapping: &Mapping, value: f32) -> Vec<Action> {
    let steps = mapping.encoding.is_relative();
    let read = |target: &&TargetId| target.target().is_some_and(|t| t.owner() == Owner::Reader);
    std::iter::once(&mapping.target)
        .chain(&mapping.also)
        .filter(|target| !read(target))
        .map(|target| Action::of(target, value, steps, mapping.step))
        .collect()
}

/// What `actions` drive, if there are any.
fn driven(actions: Vec<Action>) -> Option<Routed> {
    (!actions.is_empty()).then_some(Routed::Actions(actions))
}

/// An encoder's `steps` driving keys: each a press and a release of `up`, or of `down` for
/// those to the left.
fn key_steps(up: &TargetId, down: &TargetId, steps: i32) -> Option<Routed> {
    let target = if steps > 0 { up } else { down };
    let presses = steps.unsigned_abs().min(MOST_KEY_STEPS.unsigned_abs());
    driven((0..presses).flat_map(|_| [Action::press(target, 1.0), Action::press(target, 0.0)]).collect())
}

/// The levels the meters show, at a moment.
#[derive(Debug, Clone, Copy)]
pub struct Levels {
    /// The decks the engine runs, each.
    decks: [f32; prisme_control::MAX_DECKS],
    deck_count: usize,
    main: f32,
    /// The master's left and right.
    sides: [f32; 2],
}

/// The output of the controller whose input is the port `name` of id `id`, for its lights: an
/// output of the same device (the same id: Windows gives every port of a device its device's,
/// ALSA a port its address both ways), of the same name if it has several; else, where an
/// output has an id of its own (Core MIDI, Android), the output of the same name. None on a
/// controller without one.
fn open_output(name: &str, id: &str) -> AppResult<Option<MidiWriter>> {
    let output = MidiOutput::new(CLIENT).map_err(midi_error)?;
    let ports = output.ports();
    let named = |port: &MidiOutputPort| output.port_name(port).ok();
    let device: Vec<&MidiOutputPort> = ports.iter().filter(|p| p.id() == id).collect();
    let port = device
        .iter()
        .copied()
        .find(|p| named(p).as_deref() == Some(name))
        .or_else(|| device.first().copied())
        .or_else(|| ports.iter().find(|p| named(p).as_deref() == Some(name)));
    let Some(port) = port.cloned() else { return Ok(None) };
    Ok(Some(MidiWriter::new(midi::Output::open(output, &port, CLIENT)?)?))
}

/// The names of the ports of a MIDI input or output.
pub(crate) fn port_names(io: &impl MidiIO) -> Vec<String> {
    io.ports().iter().filter_map(|p| io.port_name(p).ok()).collect()
}

/// The port of `io` of the controller saved as `device`, but those `taken` (see [`port_of`]),
/// and its name.
pub(crate) fn find_port<IO: MidiIO>(io: &IO, device: &str, taken: &[String]) -> Option<(IO::Port, String)> {
    let ports = io.ports();
    let names: Vec<String> = ports.iter().map(|p| io.port_name(p).unwrap_or_default()).collect();
    let at = port_of(&names, device, taken)?;
    Some((ports[at].clone(), names[at].clone()))
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

/// The controllers `saved`, those on a port of their very name among `plugged` first: given the
/// ports in this order (see [`port_of`]), one numbered anew by Windows takes no other's port.
pub(crate) fn in_order(saved: &[String], plugged: &[String]) -> Vec<String> {
    let (own, others): (Vec<String>, Vec<String>) = saved.iter().cloned().partition(|s| plugged.contains(s));
    own.into_iter().chain(others).collect()
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

pub(crate) fn midi_error(e: impl std::fmt::Display) -> AppError {
    AppError::Midi(e.to_string())
}

#[cfg(test)]
mod end_to_end;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{lock, AppState};
    use prisme_actions::position::output;
    use prisme_actions::view::View;
    use prisme_control::{Encoding, FaderSet, Modes, ProfileControl, ON, SHIFT};

    #[test]
    fn a_controller_is_found_under_another_alsa_address() {
        let plugged =
            ["Midi Through:Midi Through Port-0 14:0".to_owned(), "DDJ-400:DDJ-400 MIDI 1 28:0".to_owned()];
        assert_eq!(port_of(&plugged, "DDJ-400:DDJ-400 MIDI 1 24:0", &[]), Some(1));
        assert_eq!(port_of(&plugged, "DDJ-400:DDJ-400 MIDI 2 24:0", &[]), None);
    }

    #[test]
    fn a_controller_numbered_anew_by_windows_is_listed_once_as_it_was_saved() {
        let c = controllers();
        c.save(|s| s.enabled = vec!["2- DDJ-SB2".into(), "DDJ-SB2".into()]);
        *lock(&c.plugged) = vec!["DDJ-SB2".into(), "Other".into()];
        let listed: Vec<_> = c.listed().controllers.into_iter().map(|c| (c.name, c.plugged)).collect();
        assert_eq!(
            listed,
            [("DDJ-SB2".into(), true), ("Other".into(), true), ("2- DDJ-SB2".into(), false)],
            "the one of its very name on it, the other missing"
        );
        c.save(|s| s.enabled = vec!["2- DDJ-SB2".into()]);
        let listed: Vec<_> = c.listed().controllers.into_iter().map(|c| (c.name, c.plugged)).collect();
        assert_eq!(listed, [("2- DDJ-SB2".into(), true), ("Other".into(), true)], "alone, it is that one");
    }

    #[test]
    fn the_port_linux_loops_midi_back_through_is_no_controller() {
        let c = controllers();
        assert!(!c.new_to_the_dj("Midi Through:Midi Through Port-0 14:0"));
        assert!(c.new_to_the_dj("DDJ-400:DDJ-400 MIDI 1 28:0"));
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

    /// `target` driven to `value`, where it is performed.
    fn input(target: &str, value: f32) -> Option<Routed> {
        let steps = target == "library.browse";
        Some(Routed::Actions(vec![Action::of(&target.into(), value, steps, None)]))
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

    /// `target` performed here, driven by `input`.
    fn rust(target: &str, input: Input) -> Action {
        Action::Rust(target.parse().unwrap(), input)
    }

    fn direct(target: &str, input: Input) -> Option<Routed> {
        Some(Routed::Actions(vec![rust(target, input)]))
    }

    fn steps(steps: f32) -> Input {
        Input::Steps { steps, step: None }
    }

    /// The application with nothing loaded, two decks in use and the audio off.
    fn view() -> View {
        let state = AppState::new(JsonFile::in_memory(Default::default()));
        crate::actions::view_of(&state, false, &Mutex::default(), 0.0)
    }

    /// `view` with a track on `deck` whose first hot cue is set, of `colour`.
    fn with_hot_cue(mut view: View, deck: usize, colour: u32) -> View {
        let mut hot_cues = [None; prisme_control::HOT_CUES];
        hot_cues[0] = Some(colour);
        view.decks[deck].track = Some(prisme_actions::view::TrackView {
            bpm: None,
            key: None,
            key_shift: 0,
            separation: prisme_actions::stems::Separation::None,
            stem_mix: Default::default(),
            main_cue_s: None,
            hot_cues,
        });
        view
    }

    #[test]
    fn a_learnt_control_then_drives_its_target() {
        let c = controllers();
        c.learn("deck1.play".into());
        // the release of a press made before Learn is not learnt
        assert_eq!(c.route("DDJ", parse(&[0x80, 40, 0]).unwrap(), 0.0), None);
        let learnt = c.route("DDJ", parse(&[0x90, 36, 127]).unwrap(), 0.0);
        assert_eq!(learnt, Some(Routed::Learnt(Box::new(mapping(PAD, "deck1.play")))));
        assert_eq!(c.route("DDJ", parse(&[0x80, 36, 0]).unwrap(), 0.0), input("deck1.play", 0.0));
        assert_eq!(c.route("DDJ", parse(&[0x90, 36, 127]).unwrap(), 0.0), input("deck1.play", 1.0));
        assert_eq!(c.route("Other", parse(&[0x90, 36, 127]).unwrap(), 0.0), None);
    }

    #[test]
    fn the_controllers_are_shown_again_once_their_models_are_read_again() {
        let (c, state) = (controllers(), AppState::new(JsonFile::in_memory(Default::default())));
        c.show(&state);
        let before = state.revision();
        c.show(&state);
        assert_eq!(state.revision(), before, "nothing changed");
        controller_profiles::reload();
        c.show(&state);
        assert_ne!(state.revision(), before, "the models read again");
    }

    #[test]
    fn encoders_are_learnt_with_their_kind_of_steps() {
        let c = controllers();
        c.learn("library.browse".into());
        route(&c, &[0xb0, 64, 1]);
        assert_eq!(route(&c, &[0xb0, 64, 127]), input("library.browse", -1.0));
        // a jog sending steps around 64
        c.learn("deck1.jog".into());
        route(&c, &[0xb0, 33, 65]);
        assert_eq!(route(&c, &[0xb0, 33, 61]), direct("deck1.jog", steps(-3.0)));
    }

    #[test]
    fn a_fader_sending_its_low_half_is_learnt_14_bit() {
        let c = controllers();
        c.learn("deck1.pitch".into());
        route(&c, &[0xb0, 0, 64]);
        let fine = route(&c, &[0xb0, 32, 0]);
        let tempo = Control::control_change(0, 0);
        assert_eq!(
            fine,
            Some(Routed::Learnt(Box::new(with(mapping(tempo, "deck1.pitch"), |b| b.fine = Some(32)))))
        );
        assert_eq!(route(&c, &[0xb0, 0, 64]), None);
        assert_eq!(route(&c, &[0xb0, 32, 64]), direct("deck1.pitch", Input::Value(8256.0 / 16383.0)));
    }

    #[test]
    fn shift_held_while_learning_learns_the_second_layer() {
        let c = controllers();
        c.learn(SHIFT.into());
        route(&c, &[0x90, 63, 127]);
        route(&c, &[0x90, 63, 0]);
        c.learn("deck1.cue".into());
        route(&c, &[0x90, 36, 127]);
        c.learn("deck1.hotcue1.delete".into());
        // shift is pressed first: it is not what is learnt
        assert_eq!(route(&c, &[0x90, 63, 127]), None);
        let learnt = route(&c, &[0x90, 36, 127]);
        assert_eq!(
            learnt,
            Some(Routed::Learnt(Box::new(with(mapping(PAD, "deck1.hotcue1.delete"), |b| {
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
        c.learn("deck1.cue".into());
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
        c.learn("deck1.play".into());
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

    /// A `controllers.json` as 0.4.0 wrote it, with what the DJ learnt (a key, a hold driving
    /// another key too, a chord in the shift layer, an encoder read another way), and a HID
    /// controller's control as Prisme writes one since.
    const SAVED_0_4_0: &str = r#"{
        "enabled": ["DDJ-SB2", "Traktor Kontrol S4 MK2 (HID 17cc:baff)"],
        "declined": ["Launchpad Mini"],
        "mappings": [
            { "device": "DDJ-SB2", "control": { "kind": "note", "channel": 0, "number": 11 },
              "target": "deck1.cue" },
            { "device": "DDJ-SB2", "control": { "kind": "note", "channel": 0, "number": 12 },
              "target": "deck1.sync", "also": ["deck2.sync"],
              "hold": { "ms": 450, "target": "deck1.sync.once" } },
            { "device": "DDJ-SB2", "control": { "kind": "note", "channel": 0, "number": 13 },
              "target": "deck1.hotcue1", "shift": true,
              "chord": { "with": { "kind": "note", "channel": 0, "number": 64 },
                         "target": "deck1.hotcue1.delete" } },
            { "device": "DDJ-SB2", "control": { "kind": "controlChange", "channel": 0, "number": 33 },
              "target": "library.browse", "encoding": "relativeOffset" },
            { "device": "Traktor Kontrol S4 MK2 (HID 17cc:baff)",
              "control": { "kind": "hid", "report": 1, "byte": 13, "bit": 2 },
              "target": "deck2.play" }
        ],
        "clockOutput": null,
        "unmapped": [{ "device": "DDJ-SB2", "target": "deck2.cue" }]
    }"#;

    #[test]
    fn a_controllers_file_of_0_4_0_drives_what_was_learnt_and_is_written_back_as_it_was() {
        let setup: ControllerSetup = serde_json::from_str(SAVED_0_4_0).unwrap();
        assert_eq!(setup.mappings.all().len(), 5);
        let c = controllers();
        c.save(|s| *s = setup.clone());
        let pressed = |number| c.route("DDJ-SB2", parse(&[0x90, number, 127]).unwrap(), 0.0);
        assert_eq!(pressed(11), input("deck1.cue", 1.0), "learnt over the profile");
        assert_eq!(pressed(12), None, "waits to know whether it is held");
        let mut written = serde_json::to_value(&setup).unwrap();
        assert_eq!(written["profiles"], serde_json::json!({}), "the choice of profiles, new");
        written.as_object_mut().unwrap().remove("profiles");
        assert_eq!(written, serde_json::from_str::<serde_json::Value>(SAVED_0_4_0).unwrap());
    }

    #[test]
    fn an_action_taken_off_before_controllers_were_told_apart_stays_off_them_all() {
        let setup: ControllerSetup =
            serde_json::from_str(r#"{ "enabled": ["DDJ-SB2"], "unmapped": ["deck1.play"] }"#).unwrap();
        assert_eq!(setup.unmapped, [Unmapped::Everywhere("deck1.play".into())]);
        let c = controllers();
        c.save(|s| *s = setup);
        assert_eq!(c.route("DDJ-SB2", parse(&[0x90, 0x0b, 127]).unwrap(), 0.0), None);
        let unmapped = |c: &Controllers| serde_json::to_value(c.setup()).unwrap()["unmapped"].clone();
        assert_eq!(unmapped(&c), serde_json::json!(["deck1.play"]));
        c.forget("DDJ-SB2", "deck1.cue");
        let forgotten = serde_json::json!(["deck1.play", { "device": "DDJ-SB2", "target": "deck1.cue" }]);
        assert_eq!(unmapped(&c), forgotten);
    }

    #[test]
    fn a_profiled_encoder_read_another_way_becomes_the_djs() {
        let c = sb2();
        let jogs = |mappings: &[Mapping]| -> Vec<Encoding> {
            let jog = |m: &&Mapping| m.device == "DDJ-SB2" && m.target == "deck1.jog";
            mappings.iter().filter(jog).map(|m| m.encoding).collect()
        };
        assert!(c.setup().mappings.all().is_empty());
        let profiled = jogs(c.mappings().all()).len();
        assert!(profiled > 0);
        c.set_encoding("DDJ-SB2", "deck1.jog", Encoding::Relative);
        assert_eq!(jogs(c.setup().mappings.all()), vec![Encoding::Relative; profiled], "learnt");
        assert_eq!(jogs(c.mappings().all()), vec![Encoding::Relative; profiled], "in use");
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
        assert_eq!(touch(&c, 48), direct("deck1.jog.touch", Input::Value(1.0)));
        c.save(|s| {
            s.profiles.insert("VCI-100".into(), "vestax-vci-100-hile.json".into());
        });
        assert_eq!(c.model_name("VCI-100"), "Vestax VCI-100 (Hile Mod)");
        assert_eq!(c.jog_steps_per_turn("VCI-100"), 256.0);
        assert_eq!(touch(&c, 46), direct("deck1.jog.touch", Input::Value(1.0)));
    }

    #[test]
    fn an_open_controller_s_jogs_turn_by_its_profile_read_again() {
        let c = controllers();
        let jog = JogSteps::new(c.jog_steps_per_turn("VCI-100"));
        let link = Link::Sent(Vec::new());
        let connection = Connection::new("VCI-100", "VCI-100".into(), None, jog.clone(), link);
        lock(&c.connections).push(connection);
        c.save(|s| {
            s.profiles.insert("VCI-100".into(), "vestax-vci-100-hile.json".into());
        });
        c.profiles_changed();
        assert_eq!(jog.get(), 256.0);
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
        c.learn("deck1.play".into());
        c.cancel_learning();
        assert_eq!(c.route("DDJ", parse(&[0x90, 36, 127]).unwrap(), 0.0), None);
        assert!(c.setup().mappings.all().is_empty());
    }

    /// Controllers whose `DDJ` has `mappings`.
    fn with_mappings(mappings: impl IntoIterator<Item = Mapping>) -> Controllers {
        let c = controllers();
        c.save(|s| mappings.into_iter().for_each(|m| s.mappings.learn(m)));
        c
    }

    #[test]
    fn the_interface_is_shown_the_controllers_again_only_when_something_of_them_changed() {
        let state = AppState::new(JsonFile::in_memory(Default::default()));
        let c = with_mappings([mapping(PAD, "deck1.loop.2")]);
        *lock(&c.plugged) = vec!["DDJ".into()];
        let before = state.revision();
        c.show(&state);
        assert_eq!(state.moved_since(before), [crate::state::Section::Controllers]);
        let before = state.revision();
        c.show(&state);
        assert_eq!(state.moved_since(before), [], "the same");
        c.learn("deck1.play".into());
        c.show(&state);
        let shown = lock(&state.controllers).clone();
        assert_eq!(shown.learning.as_deref(), Some("deck1.play"));
        assert_eq!(shown.listed.controllers.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["DDJ"]);
        assert_eq!(shown.mappings.len(), 1);
        assert_eq!(state.moved_since(before), [crate::state::Section::Controllers]);
    }

    fn inputs_of(pairs: &[(&str, f32)]) -> Option<Routed> {
        Some(Routed::Actions(pairs.iter().map(|&(t, v)| Action::press(&t.into(), v)).collect()))
    }

    #[test]
    fn a_shift_let_go_alone_presses_its_action_alone() {
        let shift = Control::note(0, 37);
        let c = with_mappings([
            with(mapping(shift, "shift.erase"), |b| b.alone = Some("general.close".into())),
            with(mapping(PAD, "deck1.hotcue1.delete"), |b| b.shift = Shift::Named("erase".into())),
        ]);
        assert_eq!(route(&c, &[0x90, 37, 127]), None);
        assert_eq!(route(&c, &[0x90, 37, 0]), inputs_of(&[("general.close", 1.0), ("general.close", 0.0)]));
        // a pad pressed under it: its second use alone
        route(&c, &[0x90, 37, 127]);
        assert_eq!(route(&c, &[0x90, 36, 127]), input("deck1.hotcue1.delete", 1.0));
        assert_eq!(route(&c, &[0x90, 36, 0]), input("deck1.hotcue1.delete", 0.0));
        assert_eq!(route(&c, &[0x90, 37, 0]), None);
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
        let set = |position| Some(Routed::Actions(vec![Action::set(&"mixer.cueMix".into(), position)]));
        assert_eq!(route(&c, &[0x90, 36, 127]), set(1.0));
        assert_eq!(route(&c, &[0x90, 36, 0]), None, "a release sets nothing");
        assert_eq!(route(&c, &[0x90, 36, 127]), set(0.0));
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
        assert_eq!(lock(&c.reader).seen("DDJ").modes["pads"], "roll");
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
        let mode = |c: &Controllers| lock(&c.reader).seen("DDJ").modes["pads"].clone();
        assert_eq!(route(&c, &[0x90, 9, 127]), None);
        assert_eq!(mode(&c), "cues", "the press waits to know whether it is held");
        assert_eq!(route(&c, &[0x90, 9, 0]), None);
        assert_eq!(mode(&c), "roll");
        lock(&c.mappings).learn(mode_key(0));
        route(&c, &[0x90, 9, 127]);
        assert_eq!(c.fired_holds(), [("DDJ".to_owned(), key, None)]);
        assert_eq!(mode(&c), "slicer");
        assert_eq!(route(&c, &[0x90, 9, 0]), None);
        assert_eq!(c.hold_fired("DDJ", key), None, "a mode chosen: nothing to let go");
        assert_eq!(mode(&c), "slicer", "let go after its hold, its own mode is not chosen");
        lock(&c.reader).set_modes("DDJ", Modes::from([("pads".into(), "cues".into())]));
        route(&c, &[0x90, 9, 127]);
        assert_eq!(route(&c, &[0x90, 9, 0]), None);
        assert_eq!(mode(&c), "slicer", "held its time, its hold not fired yet: chosen at the release");
    }

    #[test]
    fn a_key_let_go_after_its_hold_was_due_drives_the_hold_the_tick_late() {
        let eject = |b: &mut ProfileControl| b.hold = Some(Hold { ms: 0, target: "deck1.eject".into() });
        let c = with_mappings([with(mapping(PAD, "library.load1"), eject)]);
        assert_eq!(route(&c, &[0x90, 36, 127]), None);
        assert_eq!(route(&c, &[0x90, 36, 0]), inputs_of(&[("deck1.eject", 1.0), ("deck1.eject", 0.0)]));
    }

    #[test]
    fn a_key_let_go_while_the_clock_performs_its_hold_lets_it_go_after() {
        let eject = |b: &mut ProfileControl| b.hold = Some(Hold { ms: 0, target: "deck1.eject".into() });
        let c = with_mappings([with(mapping(PAD, "library.load1"), eject)]);
        let action = |value| Action::press(&"deck1.eject".into(), value);
        assert_eq!(c.route("DDJ", parse(&[0x90, 36, 127]).unwrap(), 1.0), None);
        assert_eq!(c.fired_holds(), [("DDJ".to_owned(), PAD, Some((action(1.0), 1.0)))]);
        assert_eq!(c.route("DDJ", parse(&[0x90, 36, 0]).unwrap(), 1.5), None, "its hold not performed yet");
        assert_eq!(c.route("DDJ", parse(&[0x90, 36, 127]).unwrap(), 1.6), None, "pressed again meanwhile");
        assert_eq!(c.hold_fired("DDJ", PAD), Some((action(0.0), 1.5)), "performed: let go now, as it was");
        assert_eq!(lock(&c.holdings).len(), 1, "the second press waits");
        assert_eq!(c.hold_fired("DDJ", PAD), None);
        c.fired_holds();
        assert_eq!(c.hold_fired("DDJ", PAD), None, "held on");
        assert_eq!(route(&c, &[0x90, 36, 0]), inputs_of(&[("deck1.eject", 0.0)]));
    }

    #[test]
    fn the_interface_is_told_only_the_actions_it_keeps() {
        let listed = Target::all().iter().map(Target::to_string).collect::<Vec<_>>();
        let measured =
            ["deck1.jump.-8", "deck2.loop.0.5", "deck3.roll.1", "deck4.key.+2", "deck1.sample3.velocity"];
        let ids = listed.iter().map(String::as_str).chain(measured);
        let told: Vec<&str> =
            ids.filter(|&id| matches!(Action::of(&id.into(), 1.0, false, None), Action::Ui(..))).collect();
        assert_eq!(
            told,
            [
                "shift",
                "library.search",
                "general.help",
                "general.palette",
                "general.settings",
                "general.fullscreen",
                "general.decks",
                "general.assign",
                "general.close"
            ]
        );
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
        let held = Some((rust("deck1.jog.touch", Input::Value(1.0)), 2.5));
        assert_eq!(
            c.fired_holds(),
            [("DDJ".to_owned(), third, held)],
            "held, at its time on the controller's clock"
        );
        assert_eq!(c.hold_fired("DDJ", third), None);
        assert_eq!(route(&c, &[0x90, 38, 0]), direct("deck1.jog.touch", Input::Value(0.0)));
        assert_eq!(route(&c, &[0x90, 36, 127]), direct("deck1.jog.touch", Input::Value(1.0)), "latched");
        assert_eq!(route(&c, &[0x90, 36, 0]), None);
        assert_eq!(route(&c, &[0x90, 36, 127]), direct("deck1.jog.touch", Input::Value(0.0)));
        route(&c, &[0x90, 37, 127]);
        route(&c, &[0x90, 37, 0]);
        assert_eq!(
            route(&c, &[0x90, 37, 127]),
            direct("deck1.jog.touch", Input::Value(1.0)),
            "pressed twice"
        );
        assert_eq!(route(&c, &[0x90, 37, 0]), direct("deck1.jog.touch", Input::Value(0.0)));
    }

    #[test]
    fn what_a_key_drives_along_with_a_direct_action_is_driven_too() {
        let c = with_mappings([with(mapping(PAD, "deck1.jog.touch"), |b| {
            b.also = vec!["deck2.jog.touch".into()]
        })]);
        let touched =
            vec![rust("deck1.jog.touch", Input::Value(1.0)), rust("deck2.jog.touch", Input::Value(1.0))];
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
        assert_eq!(c.lights_of("DDJ", &view()), [(PAD, LightValue::Steady(100))]);
        route(&c, &[0x90, 36, 0]);
        assert_eq!(c.lights_of("DDJ", &view()), [(PAD, LightValue::Steady(0))]);
    }

    #[test]
    fn a_key_lit_while_held_stays_dark_let_go_whatever_its_action_shows() {
        let c = with_mappings([with(mapping(PAD, "library.load1"), lit(PRESSED, 1))]);
        let mut loaded = view();
        loaded.status.decks[0].loaded = true;
        assert_eq!(c.lights_of("DDJ", &loaded), [(PAD, LightValue::Steady(0))], "a track on the deck");
        route(&c, &[0x90, 36, 127]);
        assert_eq!(c.lights_of("DDJ", &loaded), [(PAD, LightValue::Steady(1))]);
    }

    #[test]
    fn a_mapping_to_play_lights_while_its_deck_plays() {
        let c = with_mappings([with(mapping(PAD, "deck1.play"), lit(ON, 10))]);
        let mut playing = view();
        assert_eq!(c.lights_of("DDJ", &playing), [(PAD, LightValue::Steady(0))]);
        playing.status.decks[0].playing = true;
        assert_eq!(c.lights_of("DDJ", &playing), [(PAD, LightValue::Steady(10))]);
    }

    #[test]
    fn a_light_of_a_deck_no_longer_in_use_goes_out() {
        let c = with_mappings([with(mapping(PAD, "deck3.play"), lit(ON, 10))]);
        let mut four = view();
        four.decks_in_use = 4;
        four.status.decks[2].playing = true;
        assert_eq!(c.lights_of("DDJ", &four), [(PAD, LightValue::Steady(10))]);
        let two = View { decks_in_use: 2, ..four };
        assert_eq!(c.lights_of("DDJ", &two), [(PAD, LightValue::Steady(0))]);
    }

    #[test]
    fn the_key_of_the_library_over_the_window_shows_whether_it_is() {
        let c = with_mappings([with(mapping(PAD, "general.library"), lit(ON, 10))]);
        let pad = |view: &View| c.lights_of("DDJ", view).into_iter().find(|(control, _)| *control == PAD);
        let mut shown = view();
        assert_eq!(pad(&shown), Some((PAD, LightValue::Steady(0))));
        shown.perform.library_only = true;
        assert_eq!(pad(&shown), Some((PAD, LightValue::Steady(10))));
    }

    #[test]
    fn the_values_shown_are_read_from_the_view() {
        let mut shown = view();
        shown.status.decks[0].loaded = true;
        (shown.status.decks[0].position_s, shown.status.decks[0].bpm) = (36.0, Some(128.0));
        let value = |id: &str| output(&id.parse().unwrap(), &shown);
        // 36 s at 33 1/3 turns a minute: 20 turns
        assert!(value("deck1.turn").is_some_and(|turn| turn.abs() < 1e-9));
        assert_eq!(value("deck1.bpm"), Some(128.0));
        assert_eq!(value("deck2.bpm"), None, "no track");
    }

    #[test]
    fn a_key_of_two_layers_shows_the_one_in_use() {
        let shift_key = Control::note(0, 63);
        let c = with_mappings([
            mapping(shift_key, SHIFT),
            with(mapping(PAD, "deck1.play"), lit(ON, 10)),
            with(with(mapping(PAD, "deck1.slip"), lit(ON, 20)), |b| b.shift = Shift::Any),
        ]);
        let mut playing = view();
        playing.status.decks[0].playing = true;
        assert_eq!(c.lights_of("DDJ", &playing), [(PAD, LightValue::Steady(10))]);
        route(&c, &[0x90, 63, 127]);
        assert_eq!(c.lights_of("DDJ", &playing), [(PAD, LightValue::Steady(0))], "the shift layer's");
        route(&c, &[0x90, 63, 0]);
        assert_eq!(c.lights_of("DDJ", &playing), [(PAD, LightValue::Steady(10))]);
    }

    #[test]
    fn a_key_of_two_modes_shows_the_one_in_use() {
        let when = |mode: &str| Some(When { group: "pads".into(), mode: mode.into() });
        let c = with_mappings([
            with(with(mapping(PAD, "deck1.hotcue1"), lit(ON, 10)), |b| b.when = when("cues")),
            with(with(mapping(PAD, "deck1.play"), lit(ON, 20)), |b| b.when = when("transport")),
        ]);
        lock(&c.reader).set_modes("DDJ", Modes::from([("pads".into(), "cues".into())]));
        let mut cued = with_hot_cue(view(), 0, 0xff0000);
        cued.status.decks[0].playing = true;
        assert_eq!(c.lights_of("DDJ", &cued), [(PAD, LightValue::Steady(10))]);
        lock(&c.reader).set_mode("DDJ", "pads", "transport");
        assert_eq!(c.lights_of("DDJ", &cued), [(PAD, LightValue::Steady(20))]);
    }

    /// The messages going to the controller for `meter` at `level`, what `lit` shows already
    /// left out.
    fn sent(lit: &mut Lit, meter: &Meter, level: f32, clipping: bool) -> Vec<Vec<u8>> {
        let mut sent = Vec::new();
        meter.show(level, clipping, |control, value| {
            let message = match lit.light(control, Shade::Given(value)) {
                Some(Lighting::Value(control, value)) => prisme_control::light(control, value).map(Vec::from),
                Some(Lighting::Message(_, message)) => Some(message.to_vec()),
                None => None,
            };
            sent.extend(message);
        });
        sent
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
    fn a_meter_reads_the_level_of_its_source_among_the_decks_the_engine_runs() {
        let levels = Levels { decks: [0.1, 0.2, 0.3, 0.4], deck_count: 2, main: 0.5, sides: [0.6, 0.7] };
        assert_eq!(levels.of_source("deck2"), Some(0.2));
        assert_eq!(levels.of_source("deck3"), None, "not running");
        assert_eq!(levels.of_source("deck0"), None);
        assert_eq!(levels.of_source("main.right"), Some(0.7));
    }

    #[test]
    fn a_connection_shows_its_lights_as_they_change_in_the_room_it_keeps() {
        let c = with_mappings([with(mapping(PAD, "deck1.play"), lit(ON, 10))]);
        let link = Link::Sent(Vec::new());
        let mut connection = Connection::new("DDJ", "DDJ".into(), None, JogSteps::new(720.0), link);
        let mut playing = view();
        c.show_on(&mut connection, &playing);
        assert_eq!(connection.lit.shown[&PAD], 0);
        playing.status.decks[0].playing = true;
        c.show_on(&mut connection, &playing);
        assert_eq!(connection.lit.shown[&PAD], 10);
    }

    #[test]
    fn a_light_shown_by_a_message_is_sent_once_and_written_over() {
        let lit = &mut Lit::default();
        let led = Control::note(0, 40);
        let red = Shade::Rgb([127, 0, 0]);
        assert_eq!(lit.light(led, red), Some(Lighting::Message(led, &[127, 0, 0])));
        assert_eq!(lit.light(led, red), None);
        let kept = lit.messages[&led].as_ptr();
        assert_eq!(lit.light(led, Shade::Rgb([0, 0, 127])), Some(Lighting::Message(led, &[0, 0, 127])));
        assert_eq!(lit.messages[&led].as_ptr(), kept, "not made again");
        assert_eq!(lit.light(led, Shade::Steady(5)), Some(Lighting::Value(led, 5)));
        assert!(!lit.messages.contains_key(&led));
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
            Reported::Learnt(mapping) => vec![Routed::Learnt(mapping)],
        }
    }

    fn ui(pairs: &[(&str, f32)]) -> Vec<Routed> {
        pairs.iter().map(|&(t, v)| Routed::Actions(vec![Action::press(&t.into(), v)])).collect()
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
        assert_eq!(drive(short(&[(0x01, 6)])), [direct("deck1.jog", steps(6.0)).unwrap()]);
        assert_eq!(
            drive(short(&[(0x01, 250)])),
            [direct("deck1.jog", steps(-12.0)).unwrap()],
            "wrapped at 256"
        );
        assert_eq!(
            drive(short(&[(0x01, 250), (0x11, 0x01)])),
            [direct("deck1.jog.touch", Input::Value(1.0)).unwrap()]
        );
        // the crossfader at 4095, the left tempo fader at 2048, its 0 the fastest
        let routed = drive(long(&[(0x07, 0xff), (0x08, 0x0f), (0x09, 0x00), (0x0a, 0x08)]));
        let mut expected = ui(&[("mixer.crossfader", 1.0)]);
        expected.extend(direct("deck1.pitch", Input::Value(1.0 - 2048.0 / 4095.0)));
        assert_eq!(routed, expected);
        // the browse knob, a counter of four bits: a step back from 0 is -1
        assert_eq!(
            drive(long(&[(0x07, 0xff), (0x08, 0x0f), (0x0a, 0x08), (0x02, 0x0f)])),
            [Routed::Actions(vec![Action::of(&"library.browse".into(), -1.0, true, None)])]
        );
    }

    #[test]
    fn a_hid_controllers_lights_are_its_keys_leds_in_its_output_reports() {
        let (c, device, _) = s4();
        let led = |report, byte, bits| Control::Hid(Field { report, byte, bit: 0, bits, big_endian: false });
        let mut shown = with_hot_cue(view(), 0, 0xff0000);
        shown.status.decks[0].playing = true;
        let lights = c.lights_of(&device, &shown);
        assert!(lights.contains(&(led(0x81, 0x20, 8), LightValue::Steady(127))), "play");
        assert!(lights.contains(&(led(0x81, 0x01, 24), LightValue::Message(vec![127, 0, 0]))), "its colour");
        assert!(lights.contains(&(led(0x81, 0x04, 24), LightValue::Message(vec![8, 1, 1]))), "dim red");
        // the left side on deck C: the same LEDs show deck 3
        lock(&c.reader).set_mode(&device, "left", "C");
        assert!(c.lights_of(&device, &shown).contains(&(led(0x81, 0x20, 8), LightValue::Steady(9))));
        assert!(
            c.lights_of(&device, &shown).contains(&(led(0x81, 0x31, 8), LightValue::Steady(127))),
            "deck C lit"
        );
    }

    #[test]
    fn a_control_of_a_hid_controller_is_learnt_by_the_field_that_changed() {
        let (c, device, mut reports) = s4();
        c.learn("deck1.censor".into());
        let learnt = driven(&c, &device, &mut reports, &short(&[(0x0e, 0x02)]));
        let slip = Control::Hid(Field { report: 1, byte: 0x0e, bit: 1, bits: 1, big_endian: false });
        let expected = Mapping::new(
            &device,
            ProfileControl { control: slip, target: "deck1.censor".into(), ..Default::default() },
        );
        assert_eq!(learnt, [Routed::Learnt(Box::new(expected))]);
        driven(&c, &device, &mut reports, &short(&[]));
        assert_eq!(driven(&c, &device, &mut reports, &short(&[(0x0e, 0x02)])), ui(&[("deck1.censor", 1.0)]));
        // an encoder of no profile: the counter's nibble that turned
        c.learn("deck1.size".into());
        let learnt = driven(&c, &device, &mut reports, &long(&[(0x10, 0x30)]));
        let Some(Routed::Learnt(mapping)) = learnt.first() else { panic!("{learnt:?}") };
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
        let port = |name: &str, id: &str| MidiPort { name: name.into(), id: id.into() };
        let plugged = with_hid(
            vec![
                // Windows: the USB ids of the port's device
                port("NI S4 MK2 MIDI", r"\\?\usb#vid_17cc&pid_1310&mi_00#7&1a&0&0000#{6994ad04}"),
                // Core MIDI, ALSA: the product's name
                port("DJ Console Mk2 MIDI", "1234"),
                port("DDJ-400", r"\\?\usb#vid_2b73&pid_0001&mi_00#7&2b&0&0000#{6994ad04}"),
            ],
            &[s4.clone(), other],
        );
        // the S4 by HID, its profile's; the Hercules by MIDI, which Prisme knows it by
        assert_eq!(plugged, ["DJ Console Mk2 MIDI", "DDJ-400", s4.name.as_str()]);
    }

    #[test]
    fn a_port_tells_the_usb_ids_of_its_device_on_windows() {
        assert_eq!(usb_ids(r"\\?\USB#VID_08E4&PID_01A0&MI_00#7&2&0&0000#{…}"), Some((0x08e4, 0x01a0)));
        assert_eq!(usb_ids("24:0"), None);
    }
}
