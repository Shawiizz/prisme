//! Controllers played from one end to the other with the interface asleep: a MIDI controller
//! and a HID one, each by its shipped profile, their messages taken where a real device's are
//! (`Controllers::receive`, and for HID the reader thread of a simulated device), the actions
//! performed on a bench whose engine is run by hand (in turn on their lanes, waited for), and
//! what the controllers are sent back, their lights and values, read where they would be
//! written.

use super::{ControllerSetup, Controllers, JogSteps, Link};
use crate::actions::bench::{track, Bench, TRACK_S};
use crate::actions::{Source, DONE_EVENT, FAILED_EVENT};
use crate::controller_hid::{self, simulated};
use crate::state::lock;
use prisme_actions::input::Input;
use prisme_control::hid::{self, HidInfo};
use prisme_session::JsonFile;
use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Controllers with `device` in use, as Prisme keeps them.
fn controllers(device: &str) -> Arc<Controllers> {
    let controllers = Controllers::new(JsonFile::in_memory(ControllerSetup::default()));
    controllers.save(|s| s.enabled.push(device.to_owned()));
    Arc::new(controllers)
}

/// What the interface was told since last asked: of a failure, its code; of another event, its
/// name (an action it would perform, say).
fn told(bench: &Bench) -> Vec<String> {
    let told = bench.told().into_iter();
    told.map(|(event, payload)| match payload["code"].as_str() {
        Some(code) if event == FAILED_EVENT => code.to_owned(),
        _ => event,
    })
    .collect()
}

fn playing(bench: &Bench) -> bool {
    bench.status(0).playing
}

fn position(bench: &Bench) -> f64 {
    bench.status(0).position_s
}

fn load_id(bench: &Bench) -> Option<u64> {
    lock(&bench.state.tracks)[0].as_ref().map(|t| t.load_id)
}

/// A Hercules DJControl Jogvision on its MIDI port.
struct Jogvision {
    controllers: Arc<Controllers>,
    bench: Arc<Bench>,
}

impl Jogvision {
    const PORT: &str = "DJControl Jogvision";

    fn plugged() -> Self {
        let controllers = controllers(Self::PORT);
        let bench = Arc::new(Bench::new());
        let jog = JogSteps::new(controllers.jog_steps_per_turn(Self::PORT));
        controllers.opened(&bench, Self::PORT, Self::PORT.into(), jog, Link::Sent(Vec::new()));
        Self { controllers, bench }
    }

    /// The controller sends `message`, as its port gives it; once it is performed, the engine
    /// runs a moment.
    fn send(&self, message: &[u8]) {
        let event = prisme_control::parse(message).unwrap();
        let steps = self.controllers.jog_steps_per_turn(Self::PORT);
        self.controllers.receive(&self.bench, Self::PORT, steps, event, 0.0);
        self.bench.actions.settle();
        self.bench.run(0.01);
    }

    /// The key of `note` (channel 1) pressed and let go.
    fn press(&self, note: u8) {
        self.send(&[0x90, note, 0x7f]);
        self.send(&[0x80, note, 0]);
    }

    /// What the controller was sent since last asked, a tick of the lights and values done.
    fn sent(&self) -> Vec<Vec<u8>> {
        self.controllers.tick(&self.bench.view(), None, 0.0);
        let mut connections = lock(&self.controllers.connections);
        let connection = connections.iter_mut().find(|c| c.device == Self::PORT).unwrap();
        match &mut connection.link {
            Link::Sent(sent) => std::mem::take(sent),
            _ => unreachable!("a controller of the tests"),
        }
    }

    /// The last value the controller was sent for the light or value of `control` (its two
    /// first bytes), if it was sent one since last asked.
    fn shown(&self, control: [u8; 2]) -> Option<u8> {
        self.sent().iter().rev().find(|m| m[..2] == control).map(|m| m[2])
    }
}

const PLAY: u8 = 50;
const CUE: u8 = 49;
const HOT_CUE_1: u8 = 16;
/// The light of the first hot cue's delete key, which shows it too.
const HOT_CUE_1_DELETE: u8 = 24;
const LOAD_A: u8 = 81;
const VOLUME: u8 = 1;
const SIZE: u8 = 68;
const POSITION: u8 = 97;

#[test]
fn a_midi_controller_plays_its_deck_and_lights_it_with_the_interface_asleep() {
    let jogvision = Jogvision::plugged();
    let bench = &jogvision.bench;
    assert_eq!(told(bench), Vec::<String>::new());
    let first = jogvision.sent();
    assert_eq!(first[..4], [vec![176, 127, 0], vec![177, 127, 0], vec![144, 70, 127], vec![176, 127, 127]]);
    assert!(first.contains(&vec![0x90, PLAY, 0]), "play dark");

    // the track the library selects, loaded on deck A
    let track = track("midi");
    bench.select(&track.0);
    jogvision.press(LOAD_A);
    let loaded = load_id(bench).expect("loaded on deck A");
    assert_eq!(told(bench), [DONE_EVENT], "the library left, the track chosen");

    // CUE held on the cue point plays from it, PLAY pressed meanwhile keeps it playing
    jogvision.send(&[0x90, CUE, 0x7f]);
    assert!(playing(bench));
    jogvision.press(PLAY);
    jogvision.send(&[0x80, CUE, 0]);
    assert!(playing(bench), "as on a CDJ");
    assert_eq!(jogvision.shown([0x90, PLAY]), Some(127));

    // a hot cue set where the track plays, then jumped to
    bench.run(0.5);
    let set_at = position(bench);
    jogvision.press(HOT_CUE_1);
    let lit = jogvision.sent();
    assert!(lit.contains(&vec![0x90, HOT_CUE_1, 126]), "{lit:?}");
    assert!(lit.contains(&vec![0x90, HOT_CUE_1_DELETE, 126]), "the light of its delete key too");
    bench.run(0.5);
    // the place in the track, on a ring of 127 steps
    let place = |seconds: f64| (seconds / TRACK_S * 127.0).round() as u8;
    assert_eq!(jogvision.shown([0xb0, POSITION]), Some(place(position(bench))));
    jogvision.press(HOT_CUE_1);
    assert!((position(bench) - set_at).abs() < 0.05, "back on the hot cue: {}", position(bench));
    assert_eq!(jogvision.shown([0xb0, POSITION]), Some(place(set_at)));

    // the deck plays: a load on it waits for the DJ to say yes, or is refused if the DJ chose so
    jogvision.press(LOAD_A);
    assert_eq!(told(bench), ["confirmReplace"]);
    bench.state.update_preferences(|p| p.dj.protect_playing = true);
    jogvision.press(LOAD_A);
    assert_eq!(told(bench), ["deckPlays"]);
    assert_eq!(load_id(bench), Some(loaded), "the track playing stays");

    // CUE while it plays: back to the cue point, stopped
    jogvision.press(CUE);
    assert!(!playing(bench));
    assert!(position(bench) < 0.02);
    assert_eq!(jogvision.shown([0x90, PLAY]), Some(0));

    // the screen left the volume at 0.2: the fader takes it once it reaches it
    let volume = || lock(&bench.state.decks)[0].volume;
    bench.perform("deck1.volume", Input::Value(0.2), Source::Interface { confirmed: false }).unwrap();
    jogvision.send(&[0xb0, VOLUME, 127]);
    jogvision.send(&[0xb0, VOLUME, 30]);
    assert_eq!(volume(), 0.2, "not picked up");
    jogvision.send(&[0xb0, VOLUME, 24]);
    assert_eq!(volume(), 24.0 / 127.0, "picked up as it crossed the setting");

    // the size encoder, a step to the right: the size doubles
    jogvision.send(&[0xb0, SIZE, 1]);
    assert_eq!(lock(&bench.state.decks)[0].size_beats, 8.0);
    let mut told = told(bench);
    told.dedup();
    assert_eq!(told, [DONE_EVENT], "the volume moved, for the interface to show");
}

/// A Traktor Kontrol S4 MK2, simulated: its reports read by the thread a real one is read by.
struct S4 {
    id: u32,
    controllers: Arc<Controllers>,
    bench: Arc<Bench>,
    /// A report read and done with, for each.
    read: Receiver<()>,
    /// The last output report of each id written to it.
    written: BTreeMap<u8, Vec<u8>>,
    /// The two input reports as last fed: keys, then faders and encoders.
    keys: Vec<u8>,
    faders: Vec<u8>,
}

impl S4 {
    const VENDOR: u16 = 0x17cc;
    const PRODUCT: u16 = 0x1310;

    fn plugged() -> Self {
        let info = HidInfo {
            vendor_id: Self::VENDOR,
            product_id: Self::PRODUCT,
            interface: Some(4),
            usage_page: 0xff01,
            usage: 1,
        };
        let id = simulated::plug("Traktor Kontrol S4 MK2", info, true, BTreeMap::from([(1, 21), (2, 79)]));
        let plugged = simulated::plugged(id).unwrap();
        let device = plugged.name.clone();
        assert_eq!(device, hid::device_name("Traktor Kontrol S4 MK2", Self::VENDOR, Self::PRODUCT));
        let controllers = controllers(&device);
        let bench = Arc::new(Bench::new());
        let profile = controllers.profile(&device).unwrap();
        let (done, read) = mpsc::channel();
        let jog = JogSteps::new(controllers.jog_steps_per_turn(&device));
        let (receiving, on, name, steps) = (controllers.clone(), bench.clone(), device.clone(), jog.clone());
        let link =
            controller_hid::Link::open(&plugged, profile.hid.as_ref(), Some(true), move |read, at_s| {
                receiving.receive_hid(&on, &name, &read, steps.get(), at_s);
                let _ = done.send(());
            })
            .unwrap();
        controllers.opened(&bench, &device, device.clone(), jog, Link::Hid(link));
        let mut s4 = Self {
            id,
            controllers,
            bench,
            read,
            written: BTreeMap::new(),
            keys: vec![0; 21],
            faders: vec![0; 79],
        };
        s4.keys[0] = 1;
        s4.faders[0] = 2;
        s4.feed(s4.keys.clone());
        s4.feed(s4.faders.clone());
        s4
    }

    /// The device sends `report`; once read and performed, the engine runs a moment.
    fn feed(&self, report: Vec<u8>) {
        simulated::feed(self.id, report).unwrap();
        self.read.recv_timeout(Duration::from_secs(5)).expect("the report read");
        self.bench.actions.settle();
        self.bench.run(0.01);
    }

    /// The keys report with bit `mask` of `byte` down or up.
    fn key(&mut self, byte: usize, mask: u8, down: bool) {
        if down {
            self.keys[byte] |= mask;
        } else {
            self.keys[byte] &= !mask;
        }
        self.feed(self.keys.clone());
    }

    fn press(&mut self, byte: usize, mask: u8) {
        self.key(byte, mask, true);
        self.key(byte, mask, false);
    }

    /// The faders report with the 12 bits from `byte` at `value`.
    fn fader(&mut self, byte: usize, value: u16) {
        self.faders[byte] = value as u8;
        self.faders[byte + 1] = (self.faders[byte + 1] & 0xf0) | (value >> 8) as u8;
        self.feed(self.faders.clone());
    }

    /// Whether the bytes from `byte` of output report `report` come to be `expected` as the
    /// device is written what the lights show now (its thread writes them within a few
    /// milliseconds), a second at most; else what they were.
    fn shows(&mut self, report: u8, byte: usize, expected: &[u8]) -> Result<(), Option<Vec<u8>>> {
        self.controllers.tick(&self.bench.view(), None, 0.0);
        let until = Instant::now() + Duration::from_secs(1);
        loop {
            for written in simulated::written(self.id) {
                self.written.insert(written[0], written);
            }
            let shown = self.written.get(&report).map(|w| &w[byte..byte + expected.len()]);
            if shown == Some(expected) {
                return Ok(());
            }
            if Instant::now() >= until {
                return Err(shown.map(<[u8]>::to_vec));
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

impl Drop for S4 {
    fn drop(&mut self) {
        lock(&self.controllers.connections).clear();
        simulated::unplug(self.id);
    }
}

/// Where the S4's keys of the left deck are, by report byte and bit.
const KEYS: usize = 0x0d;
const S4_PLAY: u8 = 0x01;
const S4_CUE: u8 = 0x02;
const S4_SHIFT: u8 = 0x08;
const S4_HOT_CUE_1: u8 = 0x80;
const BROWSE_KEYS: usize = 0x0f;
const S4_LOAD_A: u8 = 0x10;
/// The left deck's volume fader, 12 bits.
const S4_VOLUME: usize = 55;
/// Its encoder, a nibble counting turns: under shift, the tempo a step a turn.
const S4_ENCODER: usize = 1;
/// The output report of the deck's lights, and the bytes of the play key's and the first hot
/// cue's (red, green, blue).
const LIGHTS: u8 = 0x81;
const PLAY_LED: usize = 0x20;
const HOT_CUE_1_LED: usize = 0x01;

#[test]
fn a_hid_controller_plays_its_deck_from_its_reader_thread_and_lights_it_in_its_reports() {
    let mut s4 = S4::plugged();
    let bench = s4.bench.clone();
    assert_eq!(s4.shows(LIGHTS, PLAY_LED, &[9]), Ok(()), "play dim");

    let track = track("hid");
    bench.select(&track.0);
    s4.press(BROWSE_KEYS, S4_LOAD_A);
    let loaded = load_id(&bench).expect("loaded on deck A");
    assert_eq!(told(&bench), [DONE_EVENT], "the library left, the track chosen");

    s4.press(KEYS, S4_PLAY);
    assert!(playing(&bench));
    assert_eq!(s4.shows(LIGHTS, PLAY_LED, &[127]), Ok(()), "play lit");

    // a hot cue set, then jumped to; its pad lit in the hot cue's colour
    bench.run(0.5);
    let set_at = position(&bench);
    assert_eq!(s4.shows(LIGHTS, HOT_CUE_1_LED, &[8, 1, 1]), Ok(()), "an empty pad dim");
    s4.press(KEYS, S4_HOT_CUE_1);
    assert_eq!(s4.shows(LIGHTS, HOT_CUE_1_LED, &[127, 41, 32]), Ok(()), "a set pad lit in its colour");
    bench.run(0.5);
    s4.press(KEYS, S4_HOT_CUE_1);
    assert!((position(&bench) - set_at).abs() < 0.05, "back on the hot cue: {}", position(&bench));

    // a load on the deck that plays waits for the DJ to say yes
    s4.press(BROWSE_KEYS, S4_LOAD_A);
    assert_eq!(told(&bench), ["confirmReplace"]);
    assert_eq!(load_id(&bench), Some(loaded));

    s4.press(KEYS, S4_CUE);
    assert!(!playing(&bench));
    assert_eq!(s4.shows(LIGHTS, PLAY_LED, &[9]), Ok(()), "play dim again");

    // the volume fader, at its bottom, far from the setting the screen gave: it takes it once it
    // crosses it
    let volume = || lock(&bench.state.decks)[0].volume;
    bench.perform("deck1.volume", Input::Value(0.5), Source::Interface { confirmed: false }).unwrap();
    s4.fader(S4_VOLUME, 1200);
    s4.fader(S4_VOLUME, 1900);
    assert_eq!(volume(), 0.5, "not picked up");
    s4.fader(S4_VOLUME, 2300);
    assert_eq!(volume(), 2300.0 / 4095.0, "picked up");

    // shift held, the encoder a turn to the right: the tempo up a step
    s4.key(KEYS, S4_SHIFT, true);
    s4.faders[S4_ENCODER] = 0x10;
    s4.feed(s4.faders.clone());
    s4.key(KEYS, S4_SHIFT, false);
    assert!((lock(&bench.state.decks)[0].pitch - 1.001).abs() < 1e-9);
    let mut told = told(&bench);
    told.dedup();
    assert_eq!(told, [DONE_EVENT], "the volume and the tempo moved, for the interface to show");
}
