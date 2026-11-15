//! What a card of the second USB audio class says of itself: its streams (interface,
//! setting, channels, sample size, endpoints) and the clock that paces them.

/// `bInterfaceClass`, subclasses and protocol of the audio class, second version.
const AUDIO: u8 = 1;
const AUDIO_CONTROL: u8 = 1;
const AUDIO_STREAMING: u8 = 2;
const CLASS_2: u8 = 0x20;

/// Descriptor types.
const DEVICE: u8 = 1;
const CONFIGURATION: u8 = 2;
const INTERFACE: u8 = 4;
const ENDPOINT: u8 = 5;
const CLASS_INTERFACE: u8 = 0x24;

/// Audio control subtypes.
const INPUT_TERMINAL: u8 = 2;
const OUTPUT_TERMINAL: u8 = 3;
const CLOCK_SOURCE: u8 = 0x0a;
const CLOCK_SELECTOR: u8 = 0x0b;

/// Audio streaming subtypes.
const AS_GENERAL: u8 = 1;
const FORMAT_TYPE: u8 = 2;
const FORMAT_TYPE_I: u8 = 1;
const PCM: u32 = 1;

/// A stream of the card, in the setting chosen for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stream {
    pub interface: u8,
    pub alt: u8,
    pub channels: usize,
    /// Bytes a sample takes, and how many of its bits carry the sound.
    pub subslot: usize,
    pub bits: u8,
    pub endpoint: u8,
    /// Bytes a packet holds at most.
    pub max_packet: usize,
    /// `bInterval`: a packet every 2^(n-1) microframes at high speed, frames at full speed.
    pub interval: u8,
    /// The endpoint the card tells through how many frames it wants (an asynchronous output).
    pub feedback: Option<Feedback>,
    /// The clock source that paces it.
    pub clock: u8,
}

/// An output's feedback endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Feedback {
    pub endpoint: u8,
    /// Bytes its packet holds: 3 at full speed, 4 at high speed.
    pub max_packet: usize,
}

impl Stream {
    /// Bytes a frame takes.
    pub fn frame_bytes(&self) -> usize {
        self.channels * self.subslot
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Card {
    /// The audio control interface, through which the clock is set.
    pub control: u8,
    pub playback: Option<Stream>,
    pub capture: Option<Stream>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DescriptorError {
    #[error("the descriptors end in the middle of one")]
    Truncated,
    #[error("no audio control interface of the second USB audio class")]
    NotClass2,
    #[error("no stream of plain samples")]
    NoStream,
}

/// The card of `raw`: the device descriptor, then its configurations (the first is read; the
/// system may give it twice).
pub(crate) fn parse(raw: &[u8]) -> Result<Card, DescriptorError> {
    let mut parsed = Parsed::default();
    let mut current: Option<(u8, u8, u8, u8, u8)> = None;
    let mut stream: Option<Partial> = None;
    let mut seen_configuration = false;
    for d in descriptors(raw)? {
        match d[1] {
            DEVICE => {}
            CONFIGURATION if seen_configuration => break,
            CONFIGURATION => seen_configuration = true,
            INTERFACE if d.len() >= 9 => {
                parsed.close(stream.take());
                current = Some((d[2], d[3], d[5], d[6], d[7]));
                if let Some((number, alt, AUDIO, AUDIO_STREAMING, CLASS_2)) = current {
                    stream = (alt > 0).then(|| Partial { interface: number, alt, ..Partial::default() });
                }
                if let Some((number, _, AUDIO, AUDIO_CONTROL, CLASS_2)) = current {
                    parsed.control.get_or_insert(number);
                }
            }
            CLASS_INTERFACE => match current {
                Some((_, _, AUDIO, AUDIO_CONTROL, CLASS_2)) => parsed.entity(d),
                Some((_, _, AUDIO, AUDIO_STREAMING, CLASS_2)) => {
                    if let Some(s) = stream.as_mut() {
                        s.general(d);
                    }
                }
                _ => {}
            },
            ENDPOINT if d.len() >= 7 => {
                if let Some(s) = stream.as_mut() {
                    s.endpoint(d);
                }
            }
            _ => {}
        }
    }
    parsed.close(stream);
    parsed.card()
}

/// The descriptors of `raw`, each with its length and type.
fn descriptors(raw: &[u8]) -> Result<Vec<&[u8]>, DescriptorError> {
    let mut list = Vec::new();
    let mut rest = raw;
    while !rest.is_empty() {
        let length = rest[0] as usize;
        if length < 2 || length > rest.len() {
            return Err(DescriptorError::Truncated);
        }
        list.push(&rest[..length]);
        rest = &rest[length..];
    }
    Ok(list)
}

#[derive(Default)]
struct Parsed {
    control: Option<u8>,
    /// Terminal → its clock entity; selector → its first input; the clock sources.
    terminal_clocks: Vec<(u8, u8)>,
    selectors: Vec<(u8, u8)>,
    sources: Vec<u8>,
    streams: Vec<Partial>,
}

impl Parsed {
    fn entity(&mut self, d: &[u8]) {
        match d[2] {
            INPUT_TERMINAL if d.len() >= 8 => self.terminal_clocks.push((d[3], d[7])),
            OUTPUT_TERMINAL if d.len() >= 9 => self.terminal_clocks.push((d[3], d[8])),
            CLOCK_SOURCE if d.len() >= 4 => self.sources.push(d[3]),
            CLOCK_SELECTOR if d.len() >= 6 && d[4] > 0 => self.selectors.push((d[3], d[5])),
            _ => {}
        }
    }

    fn close(&mut self, stream: Option<Partial>) {
        if let Some(s) = stream.filter(Partial::playable) {
            self.streams.push(s);
        }
    }

    /// The clock source behind entity `id`, through the selectors (their first input).
    fn source(&self, mut id: u8) -> Option<u8> {
        for _ in 0..8 {
            if self.sources.contains(&id) {
                return Some(id);
            }
            id = self.selectors.iter().find(|(s, _)| *s == id)?.1;
        }
        None
    }

    fn card(self) -> Result<Card, DescriptorError> {
        let control = self.control.ok_or(DescriptorError::NotClass2)?;
        let best = |input: bool| {
            self.streams
                .iter()
                .filter(|s| s.endpoint.is_some_and(|e| (e & 0x80 != 0) == input))
                .max_by_key(|s| (s.bits, s.channels, std::cmp::Reverse(s.alt)))
                .and_then(|s| {
                    let clock = self.terminal_clocks.iter().find(|(t, _)| *t == s.terminal)?.1;
                    Some(Stream {
                        interface: s.interface,
                        alt: s.alt,
                        channels: s.channels,
                        subslot: s.subslot,
                        bits: s.bits,
                        endpoint: s.endpoint?,
                        max_packet: s.max_packet,
                        interval: s.interval,
                        feedback: s.feedback,
                        clock: self.source(clock)?,
                    })
                })
        };
        let (playback, capture) = (best(false), best(true));
        if playback.is_none() && capture.is_none() {
            return Err(DescriptorError::NoStream);
        }
        Ok(Card { control, playback, capture })
    }
}

/// A setting of a streaming interface, as its descriptors come.
#[derive(Default)]
struct Partial {
    interface: u8,
    alt: u8,
    terminal: u8,
    pcm: bool,
    channels: usize,
    subslot: usize,
    bits: u8,
    endpoint: Option<u8>,
    max_packet: usize,
    interval: u8,
    feedback: Option<Feedback>,
}

impl Partial {
    fn general(&mut self, d: &[u8]) {
        match d[2] {
            AS_GENERAL if d.len() >= 11 => {
                self.terminal = d[3];
                self.pcm = d[5] == FORMAT_TYPE_I && u32::from_le_bytes([d[6], d[7], d[8], d[9]]) & PCM != 0;
                self.channels = d[10] as usize;
            }
            FORMAT_TYPE if d.len() >= 6 && d[3] == FORMAT_TYPE_I => {
                self.subslot = d[4] as usize;
                self.bits = d[5];
            }
            _ => {}
        }
    }

    fn endpoint(&mut self, d: &[u8]) {
        let (address, attributes) = (d[2], d[3]);
        let isochronous = attributes & 0b11 == 1;
        match (attributes >> 4) & 0b11 {
            0 if isochronous => {
                self.endpoint = Some(address);
                // bits 0..10: the bytes of a transaction; 11..12: more transactions per microframe
                let size = u16::from_le_bytes([d[4], d[5]]) as usize;
                self.max_packet = (size & 0x7ff) * (1 + ((size >> 11) & 0b11));
                self.interval = d[6];
            }
            1 if isochronous => {
                let max_packet = (u16::from_le_bytes([d[4], d[5]]) & 0x7ff) as usize;
                self.feedback = Some(Feedback { endpoint: address, max_packet });
            }
            _ => {}
        }
    }

    fn playable(&self) -> bool {
        self.pcm && self.channels > 0 && matches!(self.subslot, 2..=4) && self.endpoint.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A Behringer UMC404HD 192k, as Android gave it: its configuration twice.
    const UMC404HD: &[u8] = include_bytes!("umc404hd.bin");

    #[test]
    fn a_umc404hd_has_four_channels_each_way_in_24_bits() {
        let card = parse(UMC404HD).unwrap();
        assert_eq!(card.control, 0);
        let playback = card.playback.unwrap();
        assert_eq!(
            playback,
            Stream {
                interface: 1,
                alt: 1,
                channels: 4,
                subslot: 4,
                bits: 24,
                endpoint: 0x01,
                max_packet: 400,
                interval: 1,
                feedback: Some(Feedback { endpoint: 0x81, max_packet: 4 }),
                clock: 0x29,
            }
        );
        let capture = card.capture.unwrap();
        assert_eq!((capture.interface, capture.alt, capture.channels, capture.bits), (2, 1, 4, 24));
        assert_eq!(
            (capture.endpoint, capture.max_packet, capture.feedback, capture.clock),
            (0x82, 400, None, 0x29)
        );
    }

    #[test]
    fn descriptors_cut_short_are_refused() {
        assert_eq!(parse(&UMC404HD[..200]).err(), Some(DescriptorError::Truncated));
    }

    #[test]
    fn a_device_without_audio_is_refused() {
        // a device descriptor and an empty configuration
        let raw = [
            [18, DEVICE, 0, 2, 0, 0, 0, 64, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1].as_slice(),
            &[9, CONFIGURATION, 9, 0, 0, 1, 0, 0x80, 50],
        ]
        .concat();
        assert_eq!(parse(&raw).err(), Some(DescriptorError::NotClass2));
    }
}
