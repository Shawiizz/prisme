//! HID controllers (`prisme_control::hid`): the devices plugged in, by the name Prisme gives
//! them, and a thread for each one open, which reads its reports as they come and writes its
//! output reports once their lights changed, never more often than [`WRITE_EVERY`]. The
//! system's devices come through hidapi on Windows, macOS and Linux; Android and iOS have none
//! here. A debug build can plug in simulated devices (`simulated`), fed reports and read back.

use crate::state::lock;
use prisme_control::hid::{self, Field, HidDevice, HidInfo, Outputs, Read, Reports};
use std::collections::BTreeMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tracing::{error, warn};

/// How long a read waits for a report before the output reports are seen to.
const READ_WAIT: Duration = Duration::from_millis(4);
/// An output report is written again this long after it last was, at the soonest.
const WRITE_EVERY: Duration = Duration::from_millis(10);
/// Room for the longest input report.
const LONGEST_REPORT: usize = 4096;

/// A HID device plugged in.
#[derive(Debug, Clone, PartialEq)]
pub struct Plugged {
    /// As Prisme names it ([`hid::device_name`]), `2- ` before it for the second of a model.
    pub name: String,
    /// Its product's name, as it gives it.
    pub product: String,
    pub info: HidInfo,
    path: Path,
}

#[derive(Debug, Clone, PartialEq)]
enum Path {
    #[cfg(desktop)]
    System(std::ffi::CString),
    #[cfg(debug_assertions)]
    Simulated(u32),
}

/// An interface or collection of a device, as the system lists it.
struct Entry {
    info: HidInfo,
    product: String,
    serial: String,
    path: Path,
}

/// The HID devices plugged in that may be controllers, each by the interface of its controls:
/// the one a profile fits best (`fit`), else the first of a vendor's own usage, else its first.
pub fn plugged(fit: impl Fn(&HidInfo) -> Option<usize>) -> Vec<Plugged> {
    let mut devices: BTreeMap<(u16, u16, String), Vec<Entry>> = BTreeMap::new();
    for entry in system().into_iter().chain(simulated::entries()) {
        let key = (entry.info.vendor_id, entry.info.product_id, entry.serial.clone());
        devices.entry(key).or_default().push(entry);
    }
    let mut plugged: Vec<Plugged> = Vec::new();
    for entries in devices.into_values() {
        let best = entries
            .iter()
            .max_by_key(|e| {
                (fit(&e.info), e.info.usage_page >= VENDOR_PAGES, std::cmp::Reverse(e.info.interface))
            })
            .expect("a device has an entry");
        let base = hid::device_name(&best.product, best.info.vendor_id, best.info.product_id);
        let same = plugged.iter().filter(|p| p.name.ends_with(&base)).count();
        let name = if same == 0 { base } else { format!("{}- {base}", same + 1) };
        plugged.push(Plugged {
            name,
            product: best.product.clone(),
            info: best.info.clone(),
            path: best.path.clone(),
        });
    }
    plugged
}

#[cfg(test)]
impl Plugged {
    /// A device of `product` and its ids plugged in.
    pub fn named(product: &str, vendor_id: u16, product_id: u16) -> Self {
        Plugged {
            name: hid::device_name(product, vendor_id, product_id),
            product: product.to_owned(),
            info: HidInfo { vendor_id, product_id, ..HidInfo::default() },
            path: Path::Simulated(0),
        }
    }
}

/// The usage pages of a vendor's own, where controllers put their controls.
const VENDOR_PAGES: u16 = 0xff00;

/// Whether a device is surely no controller: a keyboard, a mouse, a touch screen, a security
/// key, a part of the computer (Apple's); a DJ who enabled one would lose it.
#[cfg(desktop)]
fn no_controller(info: &HidInfo) -> bool {
    const GENERIC_DESKTOP: u16 = 0x01;
    const POINTER_MOUSE_KEYBOARD_KEYPAD: [u16; 4] = [0x01, 0x02, 0x06, 0x07];
    const SYSTEM_CONTROL: u16 = 0x80;
    const CONSUMER: u16 = 0x0c;
    const DIGITIZER: u16 = 0x0d;
    const FIDO: u16 = 0xf1d0;
    const APPLE: [u16; 2] = [0x05ac, 0x004c];
    let desktop = info.usage_page == GENERIC_DESKTOP
        && (POINTER_MOUSE_KEYBOARD_KEYPAD.contains(&info.usage) || info.usage == SYSTEM_CONTROL);
    desktop || [CONSUMER, DIGITIZER, FIDO].contains(&info.usage_page) || APPLE.contains(&info.vendor_id)
}

/// Runs `f` with hidapi's context, the devices listed again if `list`: one context is kept,
/// as hidapi allows one at a time.
#[cfg(desktop)]
fn with_api<T>(list: bool, f: impl FnOnce(&hidapi::HidApi) -> T) -> Result<T, String> {
    static API: Mutex<Option<hidapi::HidApi>> = Mutex::new(None);
    let mut api = lock(&API);
    match api.as_mut() {
        Some(api) if list => api.refresh_devices().map_err(|e| e.to_string())?,
        Some(_) => {}
        None => *api = Some(hidapi::HidApi::new().map_err(|e| e.to_string())?),
    }
    Ok(f(api.as_ref().expect("made above")))
}

/// The system's HID devices.
#[cfg(desktop)]
fn system() -> Vec<Entry> {
    let listed = with_api(true, |api| {
        api.device_list()
            .map(|d| Entry {
                info: HidInfo {
                    vendor_id: d.vendor_id(),
                    product_id: d.product_id(),
                    interface: u8::try_from(d.interface_number()).ok(),
                    usage_page: d.usage_page(),
                    usage: d.usage(),
                },
                product: d.product_string().unwrap_or_default().to_owned(),
                serial: d.serial_number().unwrap_or_default().to_owned(),
                path: Path::System(d.path().to_owned()),
            })
            .filter(|e| !no_controller(&e.info))
            .collect()
    });
    listed.unwrap_or_else(|e| {
        warn!("HID devices not listed: {e}");
        Vec::new()
    })
}

#[cfg(not(desktop))]
fn system() -> Vec<Entry> {
    Vec::new()
}

/// What a device open is read and written through.
trait Port: Send {
    /// A report, if one came within `wait`; its length in `buffer`.
    fn read(&mut self, buffer: &mut [u8], wait: Duration) -> Result<Option<usize>, String>;
    /// The input report whose id `buffer` begins with (0 for a device that does not number its
    /// reports), asked of the device: its length in `buffer`, that id first.
    fn ask(&mut self, buffer: &mut [u8]) -> Result<usize, String>;
    fn write(&mut self, report: &[u8]) -> Result<(), String>;
}

#[cfg(desktop)]
impl Port for hidapi::HidDevice {
    fn read(&mut self, buffer: &mut [u8], wait: Duration) -> Result<Option<usize>, String> {
        let ms = i32::try_from(wait.as_millis()).unwrap_or(i32::MAX);
        match self.read_timeout(buffer, ms) {
            Ok(0) => Ok(None),
            Ok(n) => Ok(Some(n)),
            Err(e) => Err(e.to_string()),
        }
    }

    fn ask(&mut self, buffer: &mut [u8]) -> Result<usize, String> {
        self.get_input_report(buffer).map_err(|e| e.to_string())
    }

    fn write(&mut self, report: &[u8]) -> Result<(), String> {
        hidapi::HidDevice::write(self, report).map(drop).map_err(|e| e.to_string())
    }
}

/// What the thread of a device writes, and what it is told.
struct Shared {
    stop: AtomicBool,
    out: Mutex<Out>,
}

struct Out {
    outputs: Outputs,
    /// Whole reports to write as they are, in order.
    queued: Vec<Vec<u8>>,
}

/// A HID device open: its thread reads its reports and writes its lights. Dropped, the thread
/// writes what was left to write, then the device is closed.
pub struct Link {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Link {
    /// Opens `plugged`, of the model `device` if Prisme knows it, numbering its reports or not;
    /// each report read goes to `received`, with the seconds since it was opened, the reports
    /// its model asks for first. A report that makes `received` panic is logged and dropped,
    /// the device read on.
    pub fn open(
        plugged: &Plugged,
        device: Option<&HidDevice>,
        numbered: Option<bool>,
        mut received: impl FnMut(Read, f64) + Send + 'static,
    ) -> Result<Self, String> {
        let (mut port, descriptor_numbers) = open_port(&plugged.path)?;
        let numbered = numbered.or(descriptor_numbers).unwrap_or(true);
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            out: Mutex::new(Out {
                outputs: device.map(Outputs::new).unwrap_or_default(),
                queued: Vec::new(),
            }),
        });
        let name = plugged.name.clone();
        let mut received = {
            let name = name.clone();
            move |read: Read, at_s: f64| {
                if catch_unwind(AssertUnwindSafe(|| received(read, at_s))).is_err() {
                    error!("HID controller {name}: a report failed");
                }
            }
        };
        let asked = device.map(|d| d.inputs.clone()).unwrap_or_default();
        let rewrite = device.is_some_and(|d| d.rewrite);
        let thread = std::thread::Builder::new()
            .name("prisme-hid".into())
            .spawn({
                let shared = shared.clone();
                move || {
                    let mut reports = Reports::new(numbered);
                    for report in ask(&name, port.as_mut(), &asked, numbered) {
                        received(reports.read(&report), 0.0);
                    }
                    run(&name, port, &shared, reports, rewrite, received);
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self { shared, thread: Some(thread) })
    }

    /// Sets the light `led` to `value` (see [`Outputs::set`]).
    pub fn set(&self, led: Field, value: u8) {
        lock(&self.shared.out).outputs.set(led, value);
    }

    /// Sets the bytes of the light `led` (see [`Outputs::set_bytes`]).
    pub fn set_bytes(&self, led: Field, bytes: &[u8]) {
        lock(&self.shared.out).outputs.set_bytes(led, bytes);
    }

    /// Writes `report` as it is, after the lights changed before it.
    pub fn send(&self, report: &[u8]) {
        let mut out = lock(&self.shared.out);
        let changed = out.outputs.changed();
        out.queued.extend(changed);
        out.queued.push(report.to_vec());
        out.outputs.sent(report);
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The port of `path` open, and whether its report descriptor numbers its reports.
fn open_port(path: &Path) -> Result<(Box<dyn Port>, Option<bool>), String> {
    match *path {
        #[cfg(desktop)]
        Path::System(ref path) => {
            let device = with_api(false, |api| api.open_path(path))?.map_err(|e| e.to_string())?;
            let mut descriptor = [0; hidapi::MAX_REPORT_DESCRIPTOR_SIZE];
            let numbered = device
                .get_report_descriptor(&mut descriptor)
                .ok()
                .map(|n| hid::numbers_reports(&descriptor[..n]));
            Ok((Box::new(device), numbered))
        }
        #[cfg(debug_assertions)]
        Path::Simulated(id) => simulated::open(id),
    }
}

/// The input reports `asked` of the device on `port` (by id), as it would send them.
fn ask(name: &str, port: &mut dyn Port, asked: &[u8], numbered: bool) -> Vec<Vec<u8>> {
    let mut buffer = vec![0; LONGEST_REPORT];
    let mut reports = Vec::new();
    for &id in asked {
        buffer[0] = id;
        match port.ask(&mut buffer) {
            // a device that does not number its reports sends them without that 0
            Ok(length) => reports.push(buffer[usize::from(!numbered)..length].to_vec()),
            Err(e) => warn!("HID controller {name} did not give its report {id}: {e}"),
        }
    }
    reports
}

/// Reads the device on `port` until told to stop or it is gone, writing its output reports
/// meanwhile (all of them again after each report read if `rewrite`); once told, writes what
/// was left to write.
fn run(
    name: &str,
    mut port: Box<dyn Port>,
    shared: &Shared,
    mut reports: Reports,
    rewrite: bool,
    mut received: impl FnMut(Read, f64),
) {
    let opened = Instant::now();
    let mut buffer = vec![0; LONGEST_REPORT];
    let mut written: Option<Instant> = None;
    let mut failed = false;
    loop {
        let stopping = shared.stop.load(Ordering::Acquire);
        if !stopping {
            match port.read(&mut buffer, READ_WAIT) {
                Ok(Some(length)) => {
                    received(reports.read(&buffer[..length]), opened.elapsed().as_secs_f64());
                    if rewrite {
                        lock(&shared.out).outputs.rewrite();
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    warn!("HID controller {name} not read: {e}");
                    // gone: the watch closes it
                    while !shared.stop.load(Ordering::Acquire) {
                        std::thread::sleep(WRITE_EVERY);
                    }
                    return;
                }
            }
        }
        let due = stopping || written.is_none_or(|at| at.elapsed() >= WRITE_EVERY);
        let reports: Vec<Vec<u8>> = {
            let mut out = lock(&shared.out);
            let mut reports = std::mem::take(&mut out.queued);
            if due {
                reports.extend(out.outputs.changed());
            }
            reports
        };
        if due {
            written = Some(Instant::now());
        }
        for report in reports {
            if let Err(e) = port.write(&report) {
                if !failed {
                    warn!("HID controller {name} not written to: {e}");
                }
                failed = true;
            }
        }
        if stopping {
            return;
        }
    }
}

/// Simulated HID devices, in debug builds: plugged in and fed reports from the commands
/// (`simulate_hid_*`), their output reports kept to be read back. A test or the demo plays a
/// controller with them, without one.
#[cfg(debug_assertions)]
pub mod simulated {
    use super::{Entry, Path, Port};
    use crate::state::lock;
    use prisme_control::hid::{Field, HidInfo};
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::{Condvar, Mutex};
    use std::time::Duration;

    struct Device {
        id: u32,
        product: String,
        info: HidInfo,
        numbered: bool,
        /// The length of each of its input reports, by id.
        lengths: BTreeMap<u8, usize>,
        /// The last report of each id it was fed.
        last: BTreeMap<u8, Vec<u8>>,
        coming: VecDeque<Vec<u8>>,
        written: Vec<Vec<u8>>,
    }

    impl Device {
        /// Its report of id `key` as last fed, zeros before.
        fn last(&self, key: u8) -> Vec<u8> {
            self.last.get(&key).cloned().unwrap_or_else(|| {
                let mut zeros = vec![0; self.lengths.get(&key).copied().unwrap_or(64)];
                zeros[0] = key;
                zeros
            })
        }
    }

    static DEVICES: Mutex<Vec<Device>> = Mutex::new(Vec::new());
    static FED: Condvar = Condvar::new();

    /// Plugs in a device of `info` named `product`, numbering its reports or not, its input
    /// reports of `lengths` (by id; 0 when it does not number them); its id.
    pub fn plug(product: &str, info: HidInfo, numbered: bool, lengths: BTreeMap<u8, usize>) -> u32 {
        let mut devices = lock(&DEVICES);
        let id = devices.iter().map(|d| d.id + 1).max().unwrap_or(1);
        devices.push(Device {
            id,
            product: product.to_owned(),
            info,
            numbered,
            lengths,
            last: BTreeMap::new(),
            coming: VecDeque::new(),
            written: Vec::new(),
        });
        id
    }

    pub fn unplug(id: u32) {
        lock(&DEVICES).retain(|d| d.id != id);
        FED.notify_all();
    }

    /// The name device `id` goes by among `plugged`.
    pub fn name_of(plugged: &[super::Plugged], id: u32) -> Option<String> {
        plugged.iter().find(|p| p.path == Path::Simulated(id)).map(|p| p.name.clone())
    }

    /// Feeds `report` to device `id`, as if it sent it.
    pub fn feed(id: u32, report: Vec<u8>) -> Result<(), String> {
        let mut devices = lock(&DEVICES);
        let device = devices.iter_mut().find(|d| d.id == id).ok_or("no such simulated device")?;
        let key = if device.numbered { report.first().copied().unwrap_or(0) } else { 0 };
        device.last.insert(key, report.clone());
        device.coming.push_back(report);
        FED.notify_all();
        Ok(())
    }

    /// Feeds device `id` the reports holding `values`: of each report, the last fed changed by
    /// them (zeros at first).
    pub fn set(id: u32, values: &[(Field, u32)]) -> Result<(), String> {
        let reports = {
            let devices = lock(&DEVICES);
            let device = devices.iter().find(|d| d.id == id).ok_or("no such simulated device")?;
            let mut reports: BTreeMap<u8, Vec<u8>> = BTreeMap::new();
            for &(field, value) in values {
                let key = if device.numbered { field.report } else { 0 };
                let report = reports.entry(key).or_insert_with(|| device.last(key));
                if !field.write(report, value) {
                    return Err(format!("report {} holds no byte {}", field.report, field.byte));
                }
            }
            reports
        };
        reports.into_values().try_for_each(|report| feed(id, report))
    }

    /// Device `id` as the watch finds it plugged in.
    #[cfg(test)]
    pub fn plugged(id: u32) -> Option<super::Plugged> {
        let entry = entries().into_iter().find(|e| e.path == Path::Simulated(id))?;
        let name =
            prisme_control::hid::device_name(&entry.product, entry.info.vendor_id, entry.info.product_id);
        Some(super::Plugged { name, product: entry.product, info: entry.info, path: entry.path })
    }

    /// The reports device `id` was written since this was last asked.
    pub fn written(id: u32) -> Vec<Vec<u8>> {
        lock(&DEVICES)
            .iter_mut()
            .find(|d| d.id == id)
            .map(|d| std::mem::take(&mut d.written))
            .unwrap_or_default()
    }

    pub(super) fn entries() -> Vec<Entry> {
        lock(&DEVICES)
            .iter()
            .map(|d| Entry {
                info: d.info.clone(),
                product: d.product.clone(),
                serial: format!("simulated {}", d.id),
                path: Path::Simulated(d.id),
            })
            .collect()
    }

    pub(super) fn open(id: u32) -> Result<(Box<dyn Port>, Option<bool>), String> {
        let devices = lock(&DEVICES);
        let device = devices.iter().find(|d| d.id == id).ok_or("unplugged")?;
        Ok((Box::new(Simulated(id)), Some(device.numbered)))
    }

    struct Simulated(u32);

    impl Port for Simulated {
        fn read(&mut self, buffer: &mut [u8], wait: Duration) -> Result<Option<usize>, String> {
            let devices = lock(&DEVICES);
            let (mut devices, _) = FED
                .wait_timeout_while(devices, wait, |devices| {
                    devices.iter().find(|d| d.id == self.0).is_some_and(|d| d.coming.is_empty())
                })
                .unwrap_or_else(|e| e.into_inner());
            let device = devices.iter_mut().find(|d| d.id == self.0).ok_or("unplugged")?;
            Ok(device.coming.pop_front().map(|report| {
                let length = report.len().min(buffer.len());
                buffer[..length].copy_from_slice(&report[..length]);
                length
            }))
        }

        fn ask(&mut self, buffer: &mut [u8]) -> Result<usize, String> {
            let devices = lock(&DEVICES);
            let device = devices.iter().find(|d| d.id == self.0).ok_or("unplugged")?;
            let report = device.last(buffer[0]);
            // the system puts a 0 before the report of a device that does not number them
            let at = usize::from(!device.numbered);
            let length = (at + report.len()).min(buffer.len());
            buffer[at..length].copy_from_slice(&report[..length - at]);
            Ok(length)
        }

        fn write(&mut self, report: &[u8]) -> Result<(), String> {
            let mut devices = lock(&DEVICES);
            let device = devices.iter_mut().find(|d| d.id == self.0).ok_or("unplugged")?;
            device.written.push(report.to_vec());
            Ok(())
        }
    }
}

#[cfg(not(debug_assertions))]
mod simulated {
    pub(super) fn entries() -> Vec<super::Entry> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(usage_page: u16, usage: u16) -> HidInfo {
        HidInfo { vendor_id: 0x17cc, product_id: 0x1310, interface: Some(4), usage_page, usage }
    }

    #[test]
    fn keyboards_mice_and_the_computers_own_are_no_controllers() {
        assert!(no_controller(&info(0x01, 0x06)));
        assert!(no_controller(&info(0x01, 0x02)));
        assert!(no_controller(&info(0x0c, 0x01)));
        assert!(no_controller(&HidInfo { vendor_id: 0x05ac, ..info(0xff00, 1) }));
        assert!(!no_controller(&info(0xff01, 0x01)));
        assert!(!no_controller(&info(0x01, 0x05)), "a game pad may be");
    }

    /// Waits for what `got` gives that is not `None`, a second at most.
    fn soon<T>(mut got: impl FnMut() -> Option<T>) -> T {
        let until = Instant::now() + Duration::from_secs(1);
        loop {
            if let Some(it) = got() {
                return it;
            }
            assert!(Instant::now() < until, "nothing came");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// A model asking for its report `id` at open and rewriting its report 128 after each
    /// report, plugged in with `fader` (a field of that report) at 42, and opened: its id, the
    /// link, and the values of the fader read.
    fn opened(id: u8, fader: Field) -> (u32, Link, std::sync::mpsc::Receiver<Vec<u16>>) {
        let device: HidDevice = serde_json::from_str(&format!(
            r#"{{ "vendorId": 6092, "productId": 4384, "inputs": [{id}], "rewrite": true,
                 "outputs": [{{ "report": 128, "length": 3 }}] }}"#
        ))
        .unwrap();
        let info = HidInfo { vendor_id: 6092, product_id: 4384, ..HidInfo::default() };
        let numbered = id != 0;
        let simulated = simulated::plug("F1", info, numbered, BTreeMap::from([(id, 4)]));
        simulated::set(simulated, &[(fader, 42)]).unwrap();
        let plugged = plugged(|_| None);
        let name = simulated::name_of(&plugged, simulated).unwrap();
        let plugged = plugged.iter().find(|p| p.name == name).unwrap();
        let (sent, values) = std::sync::mpsc::channel();
        let fields = std::collections::BTreeSet::from([fader]);
        let link = Link::open(plugged, Some(&device), Some(numbered), move |read, _| {
            drop(sent.send(read.events(&fields).iter().map(|e| e.raw).collect()));
        })
        .unwrap();
        (simulated, link, values)
    }

    #[test]
    fn the_reports_a_model_asks_for_are_read_first() {
        for (id, byte) in [(1, 1), (0, 0)] {
            let fader = Field { report: id, byte, bit: 0, bits: 8, big_endian: false };
            let (simulated, link, values) = opened(id, fader);
            assert_eq!(values.recv_timeout(Duration::from_secs(1)), Ok(vec![42]), "asked, report {id}");
            assert_eq!(values.recv_timeout(Duration::from_secs(1)), Ok(vec![]), "then sent the same");
            drop(link);
            simulated::unplug(simulated);
        }
    }

    #[test]
    fn a_report_that_fails_is_dropped_and_the_device_read_on() {
        let fader = Field { report: 1, byte: 1, bit: 0, bits: 8, big_endian: false };
        let info = HidInfo { vendor_id: 6092, product_id: 4385, ..HidInfo::default() };
        let simulated = simulated::plug("F1", info, true, BTreeMap::from([(1, 4)]));
        let plugged = plugged(|_| None);
        let name = simulated::name_of(&plugged, simulated).unwrap();
        let plugged = plugged.iter().find(|p| p.name == name).unwrap();
        let (sent, values) = std::sync::mpsc::channel();
        let fields = std::collections::BTreeSet::from([fader]);
        let link = Link::open(plugged, None, Some(true), move |read, _| {
            let raw: Vec<u16> = read.events(&fields).iter().map(|e| e.raw).collect();
            assert_ne!(raw, [13], "a bug");
            drop(sent.send(raw));
        })
        .unwrap();
        simulated::set(simulated, &[(fader, 13)]).unwrap();
        simulated::set(simulated, &[(fader, 14)]).unwrap();
        assert_eq!(soon(|| values.try_recv().ok()), [14]);
        drop(link);
        simulated::unplug(simulated);
    }

    #[test]
    fn a_model_rewriting_its_lights_writes_them_after_each_report() {
        let fader = Field { report: 1, byte: 1, bit: 0, bits: 8, big_endian: false };
        let (simulated, link, values) = opened(1, fader);
        soon(|| values.try_recv().ok().filter(Vec::is_empty));
        soon(|| (!simulated::written(simulated).is_empty()).then_some(()));
        std::thread::sleep(WRITE_EVERY * 2);
        simulated::written(simulated);
        simulated::set(simulated, &[(fader, 43)]).unwrap();
        assert_eq!(soon(|| Some(simulated::written(simulated)).filter(|w| !w.is_empty())), [[128, 0, 0]]);
        drop(link);
        simulated::unplug(simulated);
    }
}
