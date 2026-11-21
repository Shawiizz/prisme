//! The USB sound cards Prisme drives itself, speaking the second USB audio class to them
//! through the system's access to the device: where the system's own audio reaches only some
//! of their channels (Android opens the inputs of a UMC404HD as eight mixed-up ones). The
//! system opens the card and hands its file over ([`UsbHost`]); Prisme streams it.

#[cfg_attr(not(any(target_os = "linux", target_os = "android")), allow(dead_code))]
pub mod asks;
#[cfg_attr(not(any(target_os = "linux", target_os = "android")), allow(dead_code))]
mod descriptors;
#[cfg(any(target_os = "linux", target_os = "android"))]
mod driver;
#[cfg_attr(not(any(target_os = "linux", target_os = "android")), allow(dead_code))]
mod format;
#[cfg(any(target_os = "linux", target_os = "android"))]
mod usbfs;

use crate::devices::{Backend, DeviceInfo, DeviceKind};
use std::sync::{Mutex, OnceLock};

/// A card the system opened for Prisme: its file, and its descriptors (the device's, then its
/// configuration's).
pub struct OpenedCard {
    pub fd: i32,
    pub descriptors: Vec<u8>,
}

/// A card plugged in: the system's name for it, its own until it is unplugged (given as it is
/// plugged in), and its product's, shown to the DJ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbCard {
    pub id: String,
    pub name: String,
}

/// What the system does for the cards, each known by its [`UsbCard::id`]: says which are
/// plugged in, whether Prisme may open one, asks the user to let it (the answer coming later
/// through [`asks::answered`]), opens one it may, gives it back.
pub trait UsbHost: Send + Sync {
    fn cards(&self) -> Vec<UsbCard>;
    fn allowed(&self, _id: &str) -> bool {
        true
    }
    fn ask(&self, _id: &str) {}
    fn open(&self, id: &str) -> Option<OpenedCard>;
    fn close(&self, id: &str);
}

static HOST: OnceLock<Box<dyn UsbHost>> = OnceLock::new();

/// Gives the audio the system's way to its USB cards: [`Backend::Usb`] is offered from then on.
pub fn set_usb_host(host: Box<dyn UsbHost>) {
    let _ = HOST.set(host);
}

pub(crate) fn host() -> Option<&'static dyn UsbHost> {
    HOST.get().map(|h| h.as_ref())
}

/// The rate the cards run at: the one of the engine's other devices.
pub(crate) const SAMPLE_RATE: u32 = 48_000;

/// The channels of the cards once opened, outputs and inputs: listed before, a card says
/// nothing of them.
static CHANNELS: Mutex<Vec<(String, u16, u16)>> = Mutex::new(Vec::new());

/// The cards plugged in, told apart ([`told_apart`]).
fn cards() -> Vec<UsbCard> {
    host().map_or_else(Vec::new, |host| told_apart(host.cards()))
}

/// `cards` in the order the system named them, a second card of a product already there named
/// after it with a number ("UMC404HD 192k 2"). Plugged in again, two such cards may trade
/// their numbers: the system names them anew.
fn told_apart(mut cards: Vec<UsbCard>) -> Vec<UsbCard> {
    cards.sort_by(|a, b| a.id.cmp(&b.id));
    let products: Vec<String> = cards.iter().map(|c| c.name.clone()).collect();
    for (i, card) in cards.iter_mut().enumerate() {
        let before = products[..i].iter().filter(|&p| *p == card.name).count();
        if before > 0 {
            card.name = format!("{} {}", card.name, before + 1);
        }
    }
    cards
}

/// The card plugged in that `wanted` names ([`pick`]).
pub(crate) fn find(wanted: Option<&str>) -> Option<UsbCard> {
    pick(cards(), wanted)
}

/// The card of `cards` that `wanted` names, case aside: the one of that name, else the first
/// whose name holds it (a name saved before a second card was there), else the first whose
/// name it holds (the system's name of the card, "USB-Audio - UMC404HD 192k"), else the first
/// of its product (a second card saved, alone now); the first of all without a name.
fn pick(mut cards: Vec<UsbCard>, wanted: Option<&str>) -> Option<UsbCard> {
    let Some(wanted) = wanted.map(str::to_lowercase) else { return cards.into_iter().next() };
    let names: Vec<String> = cards.iter().map(|c| c.name.to_lowercase()).collect();
    let product = wanted.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end();
    let at = names
        .iter()
        .position(|n| *n == wanted)
        .or_else(|| names.iter().position(|n| n.contains(&wanted)))
        .or_else(|| names.iter().position(|n| wanted.contains(n.as_str())))
        .or_else(|| names.iter().position(|n| !product.is_empty() && n.starts_with(product)))?;
    Some(cards.swap_remove(at))
}

/// The cards plugged in, each as an output and an input.
pub(crate) fn list() -> Vec<DeviceInfo> {
    let known = CHANNELS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let mut list = Vec::new();
    for (n, UsbCard { name, .. }) in cards().into_iter().enumerate() {
        let (outputs, inputs) = known.iter().find(|(k, ..)| *k == name).map_or((0, 0), |&(_, o, i)| (o, i));
        for (kind, channels) in [(DeviceKind::Output, outputs), (DeviceKind::Input, inputs)] {
            list.push(DeviceInfo {
                name: name.clone(),
                kind,
                backend: Backend::Usb,
                channels,
                sample_rate: SAMPLE_RATE,
                buffer_range: None,
                is_default: n == 0,
            });
        }
    }
    list
}

#[cfg_attr(not(any(target_os = "linux", target_os = "android")), allow(dead_code))]
fn remember(name: &str, card: &descriptors::Card) {
    let channels = |s: Option<descriptors::Stream>| s.map_or(0, |s| s.channels as u16);
    let mut known = CHANNELS.lock().unwrap_or_else(|e| e.into_inner());
    known.retain(|(k, ..)| k != name);
    known.push((name.to_string(), channels(card.playback), channels(card.capture)));
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) use running::{start, Running};

#[cfg(any(target_os = "linux", target_os = "android"))]
mod running {
    use super::driver::{Driver, Options, MAX_URBS};
    use super::{asks, cards, descriptors, host, pick, remember, SAMPLE_RATE};
    use crate::stream::{Renderer, RunningStream, StreamFormat, StreamSettings, StreamStats, Streams};
    use crate::{AudioCallback, AudioError};
    use std::sync::Arc;

    /// Transfers of a millisecond kept in flight each way when no period is asked for: a phone
    /// separating stems renders in under one, the third leaves room for its scheduling.
    const DEFAULT_URBS: usize = 3;

    /// A card streaming; dropping it stops it and gives the card back to the system.
    pub(crate) struct Running {
        driver: Option<Driver>,
        /// The card's [`super::UsbCard::id`].
        id: String,
    }

    impl Drop for Running {
        fn drop(&mut self) {
            self.driver = None;
            if let Some(host) = host() {
                host.close(&self.id);
            }
        }
    }

    /// Opens the card of `settings` (its output device, else the first plugged in), once the
    /// system lets Prisme ([`asks`]), with its inputs if an input is asked for, and streams it
    /// through the callback.
    pub(crate) fn start<C: AudioCallback>(
        settings: &StreamSettings,
        make_callback: impl FnOnce(StreamFormat) -> C,
    ) -> Result<RunningStream, AudioError> {
        let host = host().ok_or(AudioError::BackendUnavailable(crate::Backend::Usb))?;
        let wanted = settings.output_device.as_deref();
        let plugged = cards();
        let card = pick(plugged.clone(), wanted)
            .ok_or_else(|| AudioError::DeviceNotFound(wanted.unwrap_or_default().into()))?;
        asks::may_open(host, &card, &plugged)?;
        let name = card.name;
        let opened =
            host.open(&card.id).ok_or_else(|| AudioError::Usb(format!("{name} could not be opened")))?;
        let mut running = Running { driver: None, id: card.id };
        let card = descriptors::parse(&opened.descriptors).map_err(|e| AudioError::Usb(e.to_string()))?;
        remember(&name, &card);
        let playback = card.playback.ok_or_else(|| AudioError::Usb(format!("{name} has no output")))?;
        let capture = card.capture.filter(|_| settings.input_device.is_some());

        let frames_per_ms = SAMPLE_RATE as usize / 1000;
        let urbs = settings
            .buffer_frames
            .map_or(DEFAULT_URBS, |f| (f as usize).div_ceil(frames_per_ms).clamp(2, MAX_URBS));
        let format = StreamFormat {
            sample_rate: SAMPLE_RATE,
            input_channels: capture.map_or(0, |c| c.channels),
            output_channels: playback.channels,
            buffer_frames: Some((urbs * frames_per_ms) as u32),
        };
        let stats = Arc::new(StreamStats::default());
        let (producer, ring) = match capture {
            Some(c) => {
                let (producer, ring) = rtrb::RingBuffer::<f32>::new(SAMPLE_RATE as usize * c.channels / 2);
                (Some(producer), Some(ring))
            }
            None => (None, None),
        };
        // the input comes a transfer at a time, out of step with the output's: one ahead, and as
        // many as may be in flight let wait (on the card's clock both ways, none is too many)
        let renderer = Renderer::new(make_callback(format), ring, format, stats.clone())
            .late_told_by_the_device()
            .input_cushion(frames_per_ms, MAX_URBS * frames_per_ms);
        let options = Options { sample_rate: SAMPLE_RATE, urbs };
        running.driver = Some(
            Driver::start(opened.fd, &card, &options, renderer, producer, stats.clone())
                .map_err(|e| AudioError::Usb(format!("{name}: {e}")))?,
        );
        Ok(RunningStream::new(Streams::Usb { _card: running }, format, stats, name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(id: &str, name: &str) -> UsbCard {
        UsbCard { id: id.into(), name: name.into() }
    }

    #[test]
    fn two_cards_of_a_product_are_told_apart_in_the_order_they_were_named() {
        let cards = told_apart(vec![
            card("/dev/bus/usb/001/007", "UMC404HD 192k"),
            card("/dev/bus/usb/001/003", "UMC404HD 192k"),
            card("/dev/bus/usb/001/005", "DJControl"),
        ]);
        let names: Vec<_> = cards.iter().map(|c| (c.id.as_str(), c.name.as_str())).collect();
        assert_eq!(
            names,
            [
                ("/dev/bus/usb/001/003", "UMC404HD 192k"),
                ("/dev/bus/usb/001/005", "DJControl"),
                ("/dev/bus/usb/001/007", "UMC404HD 192k 2"),
            ]
        );
    }

    #[test]
    fn a_card_is_found_by_its_name_or_a_part_of_it() {
        let cards = || told_apart(vec![card("a", "UMC404HD 192k"), card("b", "UMC404HD 192k")]);
        let found = |wanted| pick(cards(), wanted).map(|c| c.id);
        assert_eq!(found(Some("umc404hd 192k 2")).as_deref(), Some("b"));
        assert_eq!(found(Some("UMC404HD 192k")).as_deref(), Some("a"));
        assert_eq!(found(Some("UMC404")).as_deref(), Some("a"));
        assert_eq!(found(None).as_deref(), Some("a"));
        assert_eq!(found(Some("SL3")), None);
    }

    #[test]
    fn a_card_is_found_by_the_name_android_gives_its_audio() {
        let cards = vec![card("a", "DJControl"), card("b", "UMC404HD 192k")];
        assert_eq!(pick(cards, Some("USB-Audio - UMC404HD 192k")).map(|c| c.id).as_deref(), Some("b"));
    }

    #[test]
    fn a_second_card_saved_is_found_alone_by_its_product() {
        let alone = vec![card("b", "UMC404HD 192k")];
        assert_eq!(pick(alone, Some("UMC404HD 192k 2")).map(|c| c.id).as_deref(), Some("b"));
    }
}
