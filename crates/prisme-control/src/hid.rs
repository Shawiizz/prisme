//! HID controllers: the fields of their reports, which their controls and lights are, and the
//! reports themselves, read for what changed and written once a light changed.
//!
//! A report's bytes are counted as the system gives them: from its id, when the device numbers
//! its reports, else from its first byte of data; an output report always from its id, 0 for a
//! device that does not number them (the byte the system expects first).

use crate::message::{Control, Event};
use schemars::JsonSchema;
use serde::{de, Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// A value in a report: a key's bit, a fader's 12 bits, a counter's byte, a light's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Field {
    /// The id of its report, 0 for a device that does not number its reports.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub report: u8,
    /// Its first byte, counted from the report's first.
    pub byte: u16,
    /// Its lowest bit in that byte.
    #[serde(default, skip_serializing_if = "is_zero")]
    #[schemars(range(max = 7))]
    pub bit: u8,
    /// How many bits it has: 1 for a key or a light, up to 16; 24 for a light of three colours
    /// (its red, green and blue bytes).
    #[serde(default = "one", skip_serializing_if = "is_one", deserialize_with = "bits")]
    #[schemars(range(min = 1, max = 24))]
    pub bits: u8,
    /// Its bytes from the most significant, when it has several (little-endian otherwise, as
    /// most devices write them).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub big_endian: bool,
}

fn is_zero<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

fn one() -> u8 {
    1
}

fn is_one(bits: &u8) -> bool {
    *bits == 1
}

/// A field's width: 1 to 16 bits, or 24 for a light of three colours.
fn bits<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u8, D::Error> {
    let bits = u8::deserialize(deserializer)?;
    if (1..=16).contains(&bits) || bits == RGB_BITS {
        Ok(bits)
    } else {
        Err(de::Error::custom(format!(
            "a field has 1 to 16 bits, or 24 for a light of three colours, not {bits}"
        )))
    }
}

/// The width of a light of three colours.
pub const RGB_BITS: u8 = 24;

impl Field {
    /// The bytes it lies on, from its first.
    pub(crate) fn span(self) -> std::ops::Range<usize> {
        let start = usize::from(self.byte);
        start..start + (usize::from(self.bit) + usize::from(self.bits)).div_ceil(8)
    }

    fn mask(self) -> u32 {
        u32::MAX >> (32 - u32::from(self.bits))
    }

    /// Where byte `at` of its span weighs in its value, in bits.
    fn shift(self, at: usize) -> usize {
        let last = self.span().len() - 1;
        8 * if self.big_endian { last - at } else { at }
    }

    /// Its value in `report`, if the report holds it.
    pub fn read(self, report: &[u8]) -> Option<u32> {
        let bytes = report.get(self.span())?;
        let whole = bytes.iter().enumerate().fold(0u32, |v, (at, &b)| v | u32::from(b) << self.shift(at));
        Some(whole >> self.bit & self.mask())
    }

    /// Puts `value` (cut to its bits) in `report`; whether the report holds it.
    pub fn write(self, report: &mut [u8], value: u32) -> bool {
        let span = self.span();
        let Some(bytes) = report.get_mut(span) else { return false };
        let (mask, value) = (self.mask() << self.bit, (value & self.mask()) << self.bit);
        for (at, byte) in bytes.iter_mut().enumerate() {
            let shift = self.shift(at);
            let keep = !(mask >> shift) as u8;
            *byte = *byte & keep | (value >> shift) as u8;
        }
        true
    }

    /// Whether it is a field of `report` (a report of a device that numbers them if `numbered`).
    fn of(self, report: &[u8], numbered: bool) -> bool {
        !numbered || report.first() == Some(&self.report)
    }
}

/// How Prisme finds a HID controller among the devices plugged in, and what it writes to it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HidDevice {
    pub vendor_id: u16,
    pub product_id: u16,
    /// The USB interface of its controls, when it has others (its sound card's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub interface: Option<u8>,
    /// The usage page and usage of its controls' collection, when it has several.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_page: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<u16>,
    /// Its output reports, which its lights are fields of.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<OutputReport>,
    /// The light of each key that has one, a field of an output report.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub leds: Vec<Led>,
    /// A light's value at full: the colours of three-colour lights are scaled to it.
    #[serde(default = "full", skip_serializing_if = "is_full")]
    pub brightness: u8,
}

fn full() -> u8 {
    127
}

fn is_full(value: &u8) -> bool {
    *value == 127
}

/// An output report: `length` bytes, its id the first, holding `bytes` (a header, a light always
/// on) by their place beside what its lights show.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OutputReport {
    pub report: u8,
    #[schemars(range(min = 1))]
    pub length: u16,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bytes: BTreeMap<u16, u8>,
}

/// The light of `control`, a key: `led`, a field of an output report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Led {
    pub control: Control,
    pub led: Control,
}

/// What a HID device is, as the system lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HidInfo {
    pub vendor_id: u16,
    pub product_id: u16,
    pub interface: Option<u8>,
    pub usage_page: u16,
    pub usage: u16,
}

impl HidDevice {
    /// How closely `info` is this model: more for each of its interface, usage page and usage it
    /// names and `info` has; `None` if it is another.
    pub fn fits(&self, info: &HidInfo) -> Option<usize> {
        if (self.vendor_id, self.product_id) != (info.vendor_id, info.product_id) {
            return None;
        }
        let interface = match self.interface {
            Some(wanted) => (info.interface? == wanted).then_some(1)?,
            None => 0,
        };
        let usage = |wanted: Option<u16>, has: u16| match wanted {
            Some(wanted) => (wanted == has).then_some(1),
            None => Some(0),
        };
        Some(1 + interface + usage(self.usage_page, info.usage_page)? + usage(self.usage, info.usage)?)
    }

    /// The light of `control`, if it is a key with one.
    pub fn led(&self, control: Control) -> Option<Control> {
        self.leds.iter().find(|l| l.control == control).map(|l| l.led)
    }
}

/// The name Prisme gives a HID device: its product's, and its vendor and product ids, which
/// tell its model whatever the system calls it.
pub fn device_name(product: &str, vendor_id: u16, product_id: u16) -> String {
    let ids = format!("HID {vendor_id:04x}:{product_id:04x}");
    match product.trim() {
        "" => ids,
        product => format!("{product} ({ids})"),
    }
}

/// The vendor and product ids of a device named by [`device_name`] (case aside), if it is one.
pub fn ids_of(name: &str) -> Option<(u16, u16)> {
    let name = name.to_ascii_lowercase();
    let start = match name.rfind("(hid ") {
        Some(at) => at + 5,
        None => name.starts_with("hid ").then_some(4)?,
    };
    let (vendor, product) = name.get(start..start + 9)?.split_once(':')?;
    let hex = |s: &str| (s.len() == 4).then(|| u16::from_str_radix(s, 16).ok()).flatten();
    Some((hex(vendor)?, hex(product)?))
}

/// The colour `colour` (`"#rrggbb"`) as a three-colour light shows it at `brightness`.
pub fn rgb(colour: &str, brightness: u8) -> Option<[u8; 3]> {
    let n = u32::from_str_radix(colour.strip_prefix('#')?, 16).ok()?;
    let scaled = |c: u32| ((c & 0xff) * u32::from(brightness) / 255) as u8;
    Some([scaled(n >> 16), scaled(n >> 8), scaled(n)])
}

/// Whether a report descriptor numbers the device's reports (holds a Report ID item).
pub fn numbers_reports(descriptor: &[u8]) -> bool {
    let mut at = 0;
    while let Some(&prefix) = descriptor.get(at) {
        if prefix == 0xfe {
            // a long item: its size follows
            at += 3 + usize::from(descriptor.get(at + 1).copied().unwrap_or(0));
            continue;
        }
        if prefix & 0xfc == 0x84 {
            return true;
        }
        at += 1 + [0, 1, 2, 4][usize::from(prefix & 3)];
    }
    false
}

/// What was last read of a device's reports, to tell what changed.
#[derive(Debug, Default)]
pub struct Reports {
    numbered: bool,
    /// The last report of each id.
    last: HashMap<u8, Vec<u8>>,
}

/// A report read, and the one of its id before it.
pub struct Read<'r> {
    report: &'r [u8],
    before: Option<Vec<u8>>,
    numbered: bool,
}

/// What the action a control is learnt for takes: a key, a position, steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Learnt {
    Button,
    Fader,
    Encoder,
}

impl Reports {
    /// For a device that numbers its reports, or not.
    pub fn new(numbered: bool) -> Self {
        Self { numbered, last: HashMap::new() }
    }

    /// Takes `report` as the last of its id.
    pub fn read<'r>(&mut self, report: &'r [u8]) -> Read<'r> {
        let id = if self.numbered { report.first().copied().unwrap_or(0) } else { 0 };
        let before = self.last.insert(id, report.to_vec());
        Read { report, before, numbered: self.numbered }
    }
}

impl Read<'_> {
    /// The events of the report: those of `fields` whose value changed since the report of its
    /// id before; of the first, all but the keys that are not pressed.
    pub fn events(&self, fields: &BTreeSet<Field>) -> Vec<Event> {
        fields
            .iter()
            .filter(|f| f.of(self.report, self.numbered))
            .filter_map(|&field| {
                let now = field.read(self.report)?;
                let was = self.before.as_deref().and_then(|b| field.read(b));
                let control = Control::Hid(field);
                let first_let_go = was.is_none() && control.is_key() && now == 0;
                (was != Some(now) && !first_let_go).then_some(Event { control, raw: now as u16 })
            })
            .collect()
    }

    /// The control the report moved, learnt for an action taking `learnt`: the field of `known`
    /// that changed, else the bits that changed (a key pressed, a counter's nibble or byte, a
    /// byte or two of a fader), with its value.
    pub fn learnt(&self, known: &BTreeSet<Field>, learnt: Learnt) -> Option<Event> {
        let (report, before) = (self.report, self.before.as_deref()?);
        let changed: Vec<(usize, u8)> = (usize::from(self.numbered)..report.len().min(before.len()))
            .map(|at| (at, report[at] ^ before[at]))
            .filter(|&(_, x)| x != 0)
            .collect();
        let &(first, x) = changed.first()?;
        let holds = |f: &Field| f.span().contains(&first) && f.read(before) != f.read(report);
        let field = match known.iter().find(|f| f.of(report, self.numbered) && holds(f)) {
            Some(&field) => field,
            None => {
                let at = |bit: u8, bits: u8| Field {
                    report: if self.numbered { report[0] } else { 0 },
                    byte: first as u16,
                    bit,
                    bits,
                    big_endian: false,
                };
                let next_changed = changed.get(1).is_some_and(|&(at, _)| at == first + 1);
                let one_nibble = x & 0xf0 == 0 || x & 0x0f == 0;
                match learnt {
                    Learnt::Button if changed.len() == 1 && x.count_ones() == 1 => {
                        at(x.trailing_zeros() as u8, 1)
                    }
                    Learnt::Encoder if one_nibble => at(x.trailing_zeros() as u8 & 4, 4),
                    _ if next_changed => at(0, 16),
                    _ => at(0, 8),
                }
            }
        };
        Some(Event { control: Control::Hid(field), raw: field.read(report)? as u16 })
    }
}

/// The output reports of a device as its lights set them: a report goes again only once one of
/// its bytes changed.
#[derive(Debug, Default)]
pub struct Outputs {
    reports: Vec<Output>,
}

#[derive(Debug)]
struct Output {
    bytes: Vec<u8>,
    /// As last written.
    written: Option<Vec<u8>>,
}

impl Outputs {
    /// The reports of `device`, holding their bytes of always.
    pub fn new(device: &HidDevice) -> Self {
        let reports = device
            .outputs
            .iter()
            .map(|r| {
                let mut bytes = vec![0; usize::from(r.length.max(1))];
                bytes[0] = r.report;
                for (&at, &value) in &r.bytes {
                    if let Some(byte) = bytes.get_mut(usize::from(at)).filter(|_| at > 0) {
                        *byte = value;
                    }
                }
                Output { bytes, written: None }
            })
            .collect();
        Self { reports }
    }

    fn report(&mut self, id: u8) -> Option<&mut Vec<u8>> {
        self.reports.iter_mut().find(|r| r.bytes[0] == id).map(|r| &mut r.bytes)
    }

    /// Sets the light `led` to `value`: a field of one bit lit by any value above 0, a light of
    /// three colours white at that level. Whether it is a field of one of the reports.
    pub fn set(&mut self, led: Field, value: u8) -> bool {
        let value = match led.bits {
            1 => u32::from(value > 0),
            RGB_BITS => u32::from(value) * 0x01_01_01,
            _ => u32::from(value),
        };
        self.report(led.report).is_some_and(|report| led.write(report, value))
    }

    /// Sets the bytes of the light `led` (the red, green and blue of a light of three colours),
    /// in order.
    pub fn set_bytes(&mut self, led: Field, bytes: &[u8]) -> bool {
        let Some(report) = self.report(led.report) else { return false };
        let span = led.span();
        let Some(place) = report.get_mut(span.start..span.end.min(span.start + bytes.len())) else {
            return false;
        };
        place.copy_from_slice(&bytes[..place.len()]);
        true
    }

    /// A whole report sent as it is (an init, a display): one of these takes its bytes, as
    /// written.
    pub fn sent(&mut self, report: &[u8]) {
        let Some(&id) = report.first() else { return };
        if let Some(output) =
            self.reports.iter_mut().find(|r| r.bytes[0] == id && r.bytes.len() == report.len())
        {
            output.bytes.copy_from_slice(report);
            output.written = Some(report.to_vec());
        }
    }

    /// The reports changed since they were last written, now taken as written.
    pub fn changed(&mut self) -> Vec<Vec<u8>> {
        self.reports
            .iter_mut()
            .filter(|r| r.written.as_ref() != Some(&r.bytes))
            .map(|r| {
                r.written = Some(r.bytes.clone());
                r.bytes.clone()
            })
            .collect()
    }

    /// The reports as they are, every light dark but those always lit.
    pub fn dark(device: &HidDevice) -> Vec<Vec<u8>> {
        Outputs::new(device).reports.into_iter().map(|r| r.bytes).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(report: u8, byte: u16, bit: u8, bits: u8) -> Field {
        Field { report, byte, bit, bits, big_endian: false }
    }

    #[test]
    fn fields_read_bits_nibbles_and_words() {
        let report = [0x01, 0b0000_0110, 0x34, 0x12, 0xf5];
        assert_eq!(field(1, 1, 1, 1).read(&report), Some(1));
        assert_eq!(field(1, 1, 0, 1).read(&report), Some(0));
        assert_eq!(field(1, 4, 4, 4).read(&report), Some(0xf));
        assert_eq!(field(1, 4, 0, 4).read(&report), Some(0x5));
        assert_eq!(field(1, 2, 0, 16).read(&report), Some(0x1234), "little-endian");
        assert_eq!(field(1, 2, 0, 12).read(&report), Some(0x234));
        assert_eq!(Field { big_endian: true, ..field(1, 2, 0, 16) }.read(&report), Some(0x3412));
        assert_eq!(field(1, 4, 0, 16).read(&report), None, "past its end");
        // a bit of a 32-bit word: the third byte's first
        assert_eq!(field(1, 2, 4, 9).read(&report), Some(0x123));
    }

    #[test]
    fn fields_write_their_bits_only() {
        let mut report = [0x80, 0xff, 0, 0, 0];
        assert!(field(0x80, 1, 2, 1).write(&mut report, 0));
        assert_eq!(report[1], 0b1111_1011);
        assert!(field(0x80, 2, 0, 12).write(&mut report, 0xabc));
        assert_eq!(report[2..4], [0xbc, 0x0a]);
        assert!(field(0x80, 3, 4, 4).write(&mut report, 0x1f), "cut to its bits");
        assert_eq!(report[3], 0xfa);
        assert!(!field(0x80, 4, 0, 16).write(&mut report, 1));
    }

    #[test]
    fn a_field_has_1_to_16_bits_or_is_a_light_of_three_colours() {
        let read = |bits| serde_json::from_str::<Field>(&format!(r#"{{ "byte": 3, "bits": {bits} }}"#));
        assert!(read(16).is_ok() && read(24).is_ok());
        assert!(read(0).is_err() && read(17).is_err() && read(32).is_err());
        let key: Field = serde_json::from_str(r#"{ "report": 1, "byte": 13, "bit": 4 }"#).unwrap();
        assert_eq!(key, field(1, 13, 4, 1));
        assert_eq!(serde_json::to_string(&key).unwrap(), r#"{"report":1,"byte":13,"bit":4}"#);
    }

    fn fields(all: &[Field]) -> BTreeSet<Field> {
        all.iter().copied().collect()
    }

    #[test]
    fn a_report_gives_the_fields_that_changed() {
        let (play, fader) = (field(1, 1, 0, 1), field(1, 2, 0, 12));
        let watched = fields(&[play, fader, field(2, 1, 0, 8)]);
        let mut reports = Reports::new(true);
        let mut read = |report: &[u8]| reports.read(report).events(&watched);
        assert_eq!(
            read(&[1, 0, 0x10, 0]),
            [Event { control: Control::Hid(fader), raw: 16 }],
            "keys up untold"
        );
        assert_eq!(read(&[1, 1, 0x10, 0]), [Event { control: Control::Hid(play), raw: 1 }]);
        assert_eq!(read(&[1, 1, 0x10, 0xf0]), [], "bits beyond the fader's");
        assert_eq!(
            read(&[1, 0, 0x11, 0]),
            [Event { control: Control::Hid(play), raw: 0 }, Event { control: Control::Hid(fader), raw: 17 }]
        );
        // another report is read apart
        assert_eq!(read(&[2, 7]).len(), 1);
        assert_eq!(read(&[1, 0, 0x11, 0]), []);
    }

    #[test]
    fn a_device_that_does_not_number_its_reports_is_read_from_its_first_byte() {
        let key = field(0, 0, 3, 1);
        let mut reports = Reports::new(false);
        reports.read(&[0, 0]);
        assert_eq!(
            reports.read(&[8, 0]).events(&fields(&[key])),
            [Event { control: Control::Hid(key), raw: 1 }]
        );
    }

    #[test]
    fn a_learnt_control_is_a_known_field_or_the_bits_that_changed() {
        let fader = field(2, 9, 0, 12);
        let known = fields(&[fader]);
        let learn = |before: &[u8], after: &[u8], learnt| {
            let mut reports = Reports::new(true);
            reports.read(before);
            reports.read(after).learnt(&known, learnt)
        };
        let event = |field, raw| Some(Event { control: Control::Hid(field), raw });
        let (key, slider, knob) = (Learnt::Button, Learnt::Fader, Learnt::Encoder);
        assert_eq!(learn(&[1, 0, 0], &[1, 0, 0b100], key), event(field(1, 2, 2, 1), 1), "a key");
        assert_eq!(learn(&[1, 0, 0], &[1, 0, 0b100], slider), event(field(1, 2, 0, 8), 4), "no bit");
        assert_eq!(learn(&[1, 0, 0x03], &[1, 0, 0x04], knob), event(field(1, 2, 0, 4), 4), "a nibble");
        assert_eq!(learn(&[1, 0, 0x30], &[1, 0, 0x50], knob), event(field(1, 2, 4, 4), 5));
        assert_eq!(learn(&[1, 0x11, 0], &[1, 0x22, 0], slider), event(field(1, 1, 0, 8), 0x22), "a byte");
        assert_eq!(learn(&[1, 0xff, 0], &[1, 0x00, 1], slider), event(field(1, 1, 0, 16), 0x100), "two");
        let fader_moved = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0x0f];
        let mut before = fader_moved;
        before[9] = 0xfe;
        assert_eq!(learn(&before, &fader_moved, key), event(fader, 0xfff), "a field of its profile");
        let mut reports = Reports::new(true);
        assert_eq!(reports.read(&[1, 1]).learnt(&known, key), None, "nothing to compare with");
        assert_eq!(reports.read(&[1, 1]).learnt(&known, key), None, "nothing changed");
    }

    fn s4() -> HidDevice {
        serde_json::from_str(
            r#"{
                "vendorId": 6092, "productId": 4880, "interface": 4, "usagePage": 65281,
                "outputs": [
                    { "report": 128, "length": 6, "bytes": { "5": 127 } },
                    { "report": 129, "length": 4 }
                ],
                "leds": [{ "control": { "kind": "hid", "report": 1, "byte": 13 }, "led": { "kind": "hid", "report": 128, "byte": 2 } }]
            }"#,
        )
        .unwrap()
    }

    #[test]
    fn a_report_is_written_once_a_byte_of_it_changed() {
        let mut outputs = Outputs::new(&s4());
        assert_eq!(outputs.changed(), [vec![128, 0, 0, 0, 0, 127], vec![129, 0, 0, 0]], "all at first");
        assert!(outputs.set(field(128, 2, 0, 8), 9));
        assert!(outputs.set(field(128, 3, 1, 1), 127));
        assert!(outputs.set(field(129, 1, 0, 8), 0), "set as it is");
        assert_eq!(outputs.changed(), [vec![128, 0, 9, 2, 0, 127]], "one report for both");
        assert!(outputs.set(field(128, 2, 0, 8), 9));
        assert!(outputs.changed().is_empty());
        assert!(!outputs.set(field(130, 1, 0, 8), 9), "no such report");
        assert!(!outputs.set(field(129, 3, 0, 16), 9), "beyond its end");
    }

    #[test]
    fn a_light_of_three_colours_takes_a_level_or_its_colours() {
        let mut outputs = Outputs::new(&s4());
        let pad = field(129, 1, 0, RGB_BITS);
        outputs.set(pad, 9);
        assert_eq!(outputs.changed()[1], [129, 9, 9, 9]);
        outputs.set_bytes(pad, &[127, 4, 4]);
        assert_eq!(outputs.changed()[0], [129, 127, 4, 4]);
        assert_eq!(rgb("#ff8000", 127), Some([127, 63, 0]));
        assert_eq!(rgb("red", 127), None);
    }

    #[test]
    fn a_whole_report_sent_is_what_the_lights_change_from() {
        let mut outputs = Outputs::new(&s4());
        outputs.changed();
        outputs.sent(&[128, 1, 1, 1, 1, 1]);
        assert!(outputs.changed().is_empty(), "written already");
        outputs.set(field(128, 1, 0, 8), 5);
        assert_eq!(outputs.changed(), [vec![128, 5, 1, 1, 1, 1]]);
        assert_eq!(Outputs::dark(&s4())[0], [128, 0, 0, 0, 0, 127]);
    }

    #[test]
    fn a_model_is_its_ids_and_the_interface_and_usage_it_names() {
        let info =
            HidInfo { vendor_id: 6092, product_id: 4880, interface: Some(4), usage_page: 65281, usage: 1 };
        let s4 = s4();
        assert_eq!(s4.fits(&info), Some(3));
        assert_eq!(s4.fits(&HidInfo { interface: Some(0), ..info.clone() }), None);
        assert_eq!(s4.fits(&HidInfo { interface: None, ..info.clone() }), None);
        assert_eq!(s4.fits(&HidInfo { product_id: 1, ..info.clone() }), None);
        let any = HidDevice { interface: None, usage_page: None, ..s4.clone() };
        assert_eq!(any.fits(&HidInfo { interface: None, ..info }), Some(1));
        let key = Control::Hid(field(1, 13, 0, 1));
        assert_eq!(s4.led(key), Some(Control::Hid(field(128, 2, 0, 1))));
        assert_eq!(s4.led(Control::Hid(field(1, 14, 0, 1))), None);
    }

    #[test]
    fn a_hid_device_is_named_with_its_ids() {
        let name = device_name("Traktor Kontrol S4 MK2", 0x17cc, 0x1310);
        assert_eq!(name, "Traktor Kontrol S4 MK2 (HID 17cc:1310)");
        assert_eq!(ids_of(&name), Some((0x17cc, 0x1310)));
        assert_eq!(ids_of(&format!("{name} 2")), Some((0x17cc, 0x1310)), "a second one");
        assert_eq!(ids_of(&device_name(" ", 1, 2)), Some((1, 2)));
        assert_eq!(ids_of("DDJ-400"), None);
        assert_eq!(ids_of("My HID 12:34"), None);
        assert_eq!(ids_of("Hid 1234:abcd mixer"), Some((0x1234, 0xabcd)));
        assert_eq!(ids_of("Mixer HID 1234:abcd"), None, "a MIDI port, not one of Prisme's names");
        assert_eq!(ids_of(&device_name("HID VHF Driver", 0x046d, 0x407f)), Some((0x046d, 0x407f)));
    }

    #[test]
    fn a_descriptor_tells_whether_reports_are_numbered() {
        // usage page (vendor), usage, collection, report id 1, report size 8, end collection
        let numbered = [0x06, 0x01, 0xff, 0x09, 0x01, 0xa1, 0x01, 0x85, 0x01, 0x75, 0x08, 0xc0];
        assert!(numbers_reports(&numbered));
        let plain = [0x06, 0x01, 0xff, 0x09, 0x01, 0xa1, 0x01, 0x75, 0x08, 0x95, 0x40, 0xc0];
        assert!(!numbers_reports(&plain));
        // 0x85 as the data of another item is no report id
        assert!(!numbers_reports(&[0x95, 0x85, 0xc0]));
    }
}
