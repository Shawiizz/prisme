//! The controller models Prisme knows, each by its profile (`controllers/*.json`): as shipped
//! with this version, as published since where Prisme is made (fetched into the data folder's
//! `controller-updates`), and the DJ's own (the data folder's `controllers`), each replacing
//! the one before it of the same file name. The DJ's own profile of a model Prisme knows takes
//! the place of Prisme's, the corrections published since included: the settings say so.
//!
//! The published profiles are listed in `controllers/index.json`: of each, its digest, its
//! revision (raised at each change) and the version of the format it was written in. A
//! profile fetched is taken over the shipped one only if it is a later revision, in a format
//! this Prisme reads.

use prisme_control::{Profile, JOG_STEPS_PER_TURN, PROFILE_FORMAT, SCHEMA_URL};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, PoisonError, RwLock};
use std::time::{Duration, SystemTime};
use tracing::{info, warn};

const SHIPPED: &[(&str, &str)] = &[
    ("akai-lpd8.json", include_str!("../controllers/akai-lpd8.json")),
    ("akai-mpd24.json", include_str!("../controllers/akai-mpd24.json")),
    ("allen-and-heath-xone-k2.json", include_str!("../controllers/allen-and-heath-xone-k2.json")),
    (
        "american-audio-radius-2000-ch1.json",
        include_str!("../controllers/american-audio-radius-2000-ch1.json"),
    ),
    (
        "american-audio-radius-2000-ch2.json",
        include_str!("../controllers/american-audio-radius-2000-ch2.json"),
    ),
    ("american-audio-vms2.json", include_str!("../controllers/american-audio-vms2.json")),
    ("american-audio-vms4.json", include_str!("../controllers/american-audio-vms4.json")),
    ("arturia-keylab.json", include_str!("../controllers/arturia-keylab.json")),
    ("behringer-bcd3000.json", include_str!("../controllers/behringer-bcd3000.json")),
    ("behringer-bcr2000.json", include_str!("../controllers/behringer-bcr2000.json")),
    ("behringer-cmd-dv-1.json", include_str!("../controllers/behringer-cmd-dv-1.json")),
    ("behringer-cmd-micro.json", include_str!("../controllers/behringer-cmd-micro.json")),
    ("behringer-cmd-mm-1.json", include_str!("../controllers/behringer-cmd-mm-1.json")),
    ("behringer-cmd-studio-4a.json", include_str!("../controllers/behringer-cmd-studio-4a.json")),
    ("behringer-ddm4000.json", include_str!("../controllers/behringer-ddm4000.json")),
    ("denon-dn-hs5500.json", include_str!("../controllers/denon-dn-hs5500.json")),
    ("denon-dn-sc2000.json", include_str!("../controllers/denon-dn-sc2000.json")),
    ("denon-mc3000.json", include_str!("../controllers/denon-mc3000.json")),
    ("denon-mc4000.json", include_str!("../controllers/denon-mc4000.json")),
    ("denon-mc6000mk2.json", include_str!("../controllers/denon-mc6000mk2.json")),
    ("denon-mc7000.json", include_str!("../controllers/denon-mc7000.json")),
    ("dj-tech-cdj-101.json", include_str!("../controllers/dj-tech-cdj-101.json")),
    ("dj-tech-djm-101.json", include_str!("../controllers/dj-tech-djm-101.json")),
    ("dj-tech-i-mix-reload.json", include_str!("../controllers/dj-tech-i-mix-reload.json")),
    ("dj-tech-kontrol-one.json", include_str!("../controllers/dj-tech-kontrol-one.json")),
    ("dj-tech-mix-101.json", include_str!("../controllers/dj-tech-mix-101.json")),
    ("dj-tech-mixer-one.json", include_str!("../controllers/dj-tech-mixer-one.json")),
    (
        "dj-techtools-midi-fighter-spectra.json",
        include_str!("../controllers/dj-techtools-midi-fighter-spectra.json"),
    ),
    (
        "dj-techtools-midi-fighter-twister.json",
        include_str!("../controllers/dj-techtools-midi-fighter-twister.json"),
    ),
    ("electrix-tweaker.json", include_str!("../controllers/electrix-tweaker.json")),
    ("evolution-x-session.json", include_str!("../controllers/evolution-x-session.json")),
    ("faderfox-dj2.json", include_str!("../controllers/faderfox-dj2.json")),
    ("gemini-cdmp-7000-l.json", include_str!("../controllers/gemini-cdmp-7000-l.json")),
    ("gemini-cdmp-7000-r.json", include_str!("../controllers/gemini-cdmp-7000-r.json")),
    ("gemini-firstmix.json", include_str!("../controllers/gemini-firstmix.json")),
    ("hercules-dj-console-4-mx.json", include_str!("../controllers/hercules-dj-console-4-mx.json")),
    (
        "hercules-dj-console-mac-edition.json",
        include_str!("../controllers/hercules-dj-console-mac-edition.json"),
    ),
    ("hercules-dj-console-mk2.json", include_str!("../controllers/hercules-dj-console-mk2.json")),
    ("hercules-dj-console-mk4.json", include_str!("../controllers/hercules-dj-console-mk4.json")),
    ("hercules-dj-console-rmx-2.json", include_str!("../controllers/hercules-dj-console-rmx-2.json")),
    ("hercules-dj-console-rmx.json", include_str!("../controllers/hercules-dj-console-rmx.json")),
    ("hercules-dj-control-air.json", include_str!("../controllers/hercules-dj-control-air.json")),
    ("hercules-dj-control-instinct.json", include_str!("../controllers/hercules-dj-control-instinct.json")),
    ("hercules-dj-control-mp3-e2.json", include_str!("../controllers/hercules-dj-control-mp3-e2.json")),
    ("hercules-dj-control-steel.json", include_str!("../controllers/hercules-dj-control-steel.json")),
    ("hercules-djcontrol-compact.json", include_str!("../controllers/hercules-djcontrol-compact.json")),
    (
        "hercules-djcontrol-inpulse-200.json",
        include_str!("../controllers/hercules-djcontrol-inpulse-200.json"),
    ),
    (
        "hercules-djcontrol-inpulse-300.json",
        include_str!("../controllers/hercules-djcontrol-inpulse-300.json"),
    ),
    (
        "hercules-djcontrol-inpulse-500.json",
        include_str!("../controllers/hercules-djcontrol-inpulse-500.json"),
    ),
    ("hercules-djcontrol-jogvision.json", include_str!("../controllers/hercules-djcontrol-jogvision.json")),
    ("hercules-djcontrol-mix.json", include_str!("../controllers/hercules-djcontrol-mix.json")),
    ("hercules-djcontrol-starlight.json", include_str!("../controllers/hercules-djcontrol-starlight.json")),
    ("hercules-p32-dj.json", include_str!("../controllers/hercules-p32-dj.json")),
    ("icon-icontrols.json", include_str!("../controllers/icon-icontrols.json")),
    ("icon-p1-nano.json", include_str!("../controllers/icon-p1-nano.json")),
    ("intech-tek2.json", include_str!("../controllers/intech-tek2.json")),
    ("ion-discover-dj-pro.json", include_str!("../controllers/ion-discover-dj-pro.json")),
    ("ion-discover-dj.json", include_str!("../controllers/ion-discover-dj.json")),
    ("keith-mcmillen-quneo.json", include_str!("../controllers/keith-mcmillen-quneo.json")),
    ("kontrol-dj-kdj500.json", include_str!("../controllers/kontrol-dj-kdj500.json")),
    ("korg-kaoss-dj.json", include_str!("../controllers/korg-kaoss-dj.json")),
    ("korg-nanokontrol.json", include_str!("../controllers/korg-nanokontrol.json")),
    ("korg-nanokontrol2.json", include_str!("../controllers/korg-nanokontrol2.json")),
    ("korg-nanopad2.json", include_str!("../controllers/korg-nanopad2.json")),
    ("m-audio-x-session-pro.json", include_str!("../controllers/m-audio-x-session-pro.json")),
    ("m-vave-smc-mixer.json", include_str!("../controllers/m-vave-smc-mixer.json")),
    ("m-vave-smk-25-ii.json", include_str!("../controllers/m-vave-smk-25-ii.json")),
    ("miditech-midicontrol.json", include_str!("../controllers/miditech-midicontrol.json")),
    ("mixman-dm2.json", include_str!("../controllers/mixman-dm2.json")),
    ("mixvibes-u-mix-control-2.json", include_str!("../controllers/mixvibes-u-mix-control-2.json")),
    ("mixvibes-u-mix-control-pro-2.json", include_str!("../controllers/mixvibes-u-mix-control-pro-2.json")),
    (
        "native-instruments-traktor-kontrol-x1.json",
        include_str!("../controllers/native-instruments-traktor-kontrol-x1.json"),
    ),
    ("novation-dicer.json", include_str!("../controllers/novation-dicer.json")),
    ("novation-launchpad-mini-mk3.json", include_str!("../controllers/novation-launchpad-mini-mk3.json")),
    ("novation-launchpad-mini.json", include_str!("../controllers/novation-launchpad-mini.json")),
    ("novation-launchpad-mk2.json", include_str!("../controllers/novation-launchpad-mk2.json")),
    ("novation-launchpad.json", include_str!("../controllers/novation-launchpad.json")),
    ("numark-dj2go.json", include_str!("../controllers/numark-dj2go.json")),
    ("numark-dj2go2-touch.json", include_str!("../controllers/numark-dj2go2-touch.json")),
    ("numark-idj-live-ii.json", include_str!("../controllers/numark-idj-live-ii.json")),
    ("numark-mixstream-pro.json", include_str!("../controllers/numark-mixstream-pro.json")),
    ("numark-mixtrack-2.json", include_str!("../controllers/numark-mixtrack-2.json")),
    ("numark-mixtrack-platinum-fx.json", include_str!("../controllers/numark-mixtrack-platinum-fx.json")),
    ("numark-mixtrack-platinum.json", include_str!("../controllers/numark-mixtrack-platinum.json")),
    ("numark-mixtrack-pro-3.json", include_str!("../controllers/numark-mixtrack-pro-3.json")),
    ("numark-mixtrack-pro-fx.json", include_str!("../controllers/numark-mixtrack-pro-fx.json")),
    ("numark-mixtrack-pro.json", include_str!("../controllers/numark-mixtrack-pro.json")),
    ("numark-mixtrack.json", include_str!("../controllers/numark-mixtrack.json")),
    ("numark-n4.json", include_str!("../controllers/numark-n4.json")),
    ("numark-ns4fx.json", include_str!("../controllers/numark-ns4fx.json")),
    ("numark-ns6ii.json", include_str!("../controllers/numark-ns6ii.json")),
    ("numark-ns7.json", include_str!("../controllers/numark-ns7.json")),
    ("numark-omni-control.json", include_str!("../controllers/numark-omni-control.json")),
    ("numark-party-mix.json", include_str!("../controllers/numark-party-mix.json")),
    ("numark-scratch.json", include_str!("../controllers/numark-scratch.json")),
    ("numark-total-control.json", include_str!("../controllers/numark-total-control.json")),
    ("numark-v7.json", include_str!("../controllers/numark-v7.json")),
    ("pioneer-cdj-2000.json", include_str!("../controllers/pioneer-cdj-2000.json")),
    ("pioneer-cdj-350-ch1.json", include_str!("../controllers/pioneer-cdj-350-ch1.json")),
    ("pioneer-cdj-350-ch2.json", include_str!("../controllers/pioneer-cdj-350-ch2.json")),
    ("pioneer-cdj-850.json", include_str!("../controllers/pioneer-cdj-850.json")),
    ("pioneer-ddj-200.json", include_str!("../controllers/pioneer-ddj-200.json")),
    ("pioneer-ddj-400.json", include_str!("../controllers/pioneer-ddj-400.json")),
    ("pioneer-ddj-flx2.json", include_str!("../controllers/pioneer-ddj-flx2.json")),
    ("pioneer-ddj-flx4.json", include_str!("../controllers/pioneer-ddj-flx4.json")),
    ("pioneer-ddj-sb.json", include_str!("../controllers/pioneer-ddj-sb.json")),
    ("pioneer-ddj-sb2.json", include_str!("../controllers/pioneer-ddj-sb2.json")),
    ("pioneer-ddj-sb3.json", include_str!("../controllers/pioneer-ddj-sb3.json")),
    ("pioneer-ddj-sx.json", include_str!("../controllers/pioneer-ddj-sx.json")),
    ("reloop-beatmix-2-4-mk2.json", include_str!("../controllers/reloop-beatmix-2-4-mk2.json")),
    ("reloop-beatpad.json", include_str!("../controllers/reloop-beatpad.json")),
    (
        "reloop-digital-jockey-2-controller-edition.json",
        include_str!("../controllers/reloop-digital-jockey-2-controller-edition.json"),
    ),
    (
        "reloop-digital-jockey-2-interface-edition.json",
        include_str!("../controllers/reloop-digital-jockey-2-interface-edition.json"),
    ),
    ("reloop-jockey-3-me.json", include_str!("../controllers/reloop-jockey-3-me.json")),
    ("reloop-mixage.json", include_str!("../controllers/reloop-mixage.json")),
    ("reloop-terminal-mix-2-4.json", include_str!("../controllers/reloop-terminal-mix-2-4.json")),
    ("roland-dj-505.json", include_str!("../controllers/roland-dj-505.json")),
    ("soundless-joymidi.json", include_str!("../controllers/soundless-joymidi.json")),
    ("stanton-djc-4.json", include_str!("../controllers/stanton-djc-4.json")),
    ("stanton-scs-1m.json", include_str!("../controllers/stanton-scs-1m.json")),
    ("stanton-scs-3m.json", include_str!("../controllers/stanton-scs-3m.json")),
    ("tascam-us-428.json", include_str!("../controllers/tascam-us-428.json")),
    ("vestax-spin.json", include_str!("../controllers/vestax-spin.json")),
    ("vestax-typhoon.json", include_str!("../controllers/vestax-typhoon.json")),
    ("vestax-vci-100.json", include_str!("../controllers/vestax-vci-100.json")),
    ("vestax-vci-100mkii.json", include_str!("../controllers/vestax-vci-100mkii.json")),
    ("vestax-vci-300.json", include_str!("../controllers/vestax-vci-300.json")),
    ("vestax-vci-400.json", include_str!("../controllers/vestax-vci-400.json")),
    ("yaeltex-minimixxx.json", include_str!("../controllers/yaeltex-minimixxx.json")),
];
/// The index of the shipped profiles.
const SHIPPED_INDEX: &str = include_str!("../controllers/index.json");
/// Where the profiles and their index are published.
const PUBLISHED: &str =
    "https://raw.githubusercontent.com/Shawiizz/prisme/main/apps/prisme-desktop/controllers/";
const INDEX: &str = "index.json";
/// The DJ's own profiles, and those fetched, in the data folder.
const OWN: &str = "controllers";
const UPDATES: &str = "controller-updates";
/// How long a fetch of the published profiles holds: they are looked for again after.
const UPDATE_EVERY: Duration = Duration::from_secs(24 * 3600);
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

/// Where a profile comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Shipped,
    Updated,
    Own,
}

/// A model Prisme knows.
#[derive(Debug, Clone)]
pub struct Known {
    pub file: String,
    pub source: Source,
    pub profile: Arc<Profile>,
}

/// The profiles published, by file name.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Index {
    pub profiles: BTreeMap<String, Published>,
}

/// A profile published: its digest (of its text, lines ended by `\n`), its revision and the
/// format it was written in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Published {
    pub sha256: String,
    pub revision: u32,
    pub format: u32,
}

/// The models Prisme knows, by file name, and the DJ's files it could not read.
#[derive(Debug, Default)]
pub struct Library {
    pub known: Vec<Known>,
    /// Prisme's profiles that the DJ's own of the same file name took the place of.
    pub replaced: Vec<Known>,
    /// `file: why`.
    pub unreadable: Vec<String>,
}

impl Library {
    /// Of `profiles`, the one whose name `port` holds that is the longest, the DJ's own of
    /// those as long, else the first.
    fn fitting<'k>(profiles: impl IntoIterator<Item = &'k Known>, port: &str) -> Option<&'k Known> {
        let mine = |k: &Known| k.source == Source::Own;
        let mut best: Option<(usize, &Known)> = None;
        for known in profiles {
            let Some(fit) = known.profile.fits(port) else { continue };
            if best.is_none_or(|(longest, chosen)| {
                fit > longest || (fit == longest && mine(known) && !mine(chosen))
            }) {
                best = Some((fit, known));
            }
        }
        best.map(|(_, known)| known)
    }

    /// The profile of the controller on the MIDI port `port`, if Prisme knows its model.
    pub fn of(&self, port: &str) -> Option<&Known> {
        Self::fitting(&self.known, port)
    }

    /// Prisme's profile that the DJ's own takes the place of on `port`, if it does: the
    /// corrections published for it do not reach that controller.
    pub fn replaced_on(&self, port: &str) -> Option<&Known> {
        if self.of(port)?.source != Source::Own {
            return None;
        }
        Self::fitting(self.known.iter().chain(&self.replaced).filter(|k| k.source != Source::Own), port)
    }
}

/// The folders of the profiles beside those shipped.
#[derive(Debug, Clone)]
struct Folders {
    own: PathBuf,
    updates: PathBuf,
}

static FOLDERS: OnceLock<Folders> = OnceLock::new();

fn library() -> &'static RwLock<Arc<Library>> {
    static LIBRARY: OnceLock<RwLock<Arc<Library>>> = OnceLock::new();
    LIBRARY.get_or_init(|| RwLock::new(Arc::new(read_library(FOLDERS.get()))))
}

/// Reads the profiles of the data folder `data` too, from now on.
pub fn install(data: &Path) {
    let folders = Folders { own: data.join(OWN), updates: data.join(UPDATES) };
    if FOLDERS.set(folders).is_ok() {
        reload();
    }
}

/// Reads the profiles again (the DJ's own changed, or updates came).
pub fn reload() -> Arc<Library> {
    let read = Arc::new(read_library(FOLDERS.get()));
    *library().write().unwrap_or_else(PoisonError::into_inner) = read.clone();
    read
}

/// The models Prisme knows now.
pub fn current() -> Arc<Library> {
    library().read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// The folder of the DJ's own profiles, if Prisme has a data folder.
pub fn own_folder() -> Option<PathBuf> {
    FOLDERS.get().map(|f| f.own.clone())
}

/// The profile of the controller on the MIDI port `port`, if Prisme knows its model (see
/// [`Library::of`]).
pub fn of(port: &str) -> Option<Arc<Profile>> {
    current().of(port).map(|known| known.profile.clone())
}

/// Steps in a turn of the jog wheels of the controller on `port`.
pub fn jog_steps_per_turn(port: &str) -> f64 {
    of(port).map_or(JOG_STEPS_PER_TURN, |p| p.jog_steps_per_turn)
}

/// The profiles shipped, then those fetched that are later, then the DJ's own.
fn read_library(folders: Option<&Folders>) -> Library {
    let mut files: BTreeMap<String, Known> = BTreeMap::new();
    let mut replaced = Vec::new();
    let mut take = |file: &str, source, profile| {
        let known = Known { file: file.to_owned(), source, profile: Arc::new(profile) };
        match files.insert(file.to_owned(), known) {
            Some(prismes) if source == Source::Own => replaced.push(prismes),
            _ => {}
        }
    };
    for (file, text) in SHIPPED {
        match serde_json::from_str(text) {
            Ok(profile) => take(file, Source::Shipped, profile),
            Err(e) => warn!("controller profile {file} unreadable: {e}"),
        }
    }
    let mut unreadable = Vec::new();
    if let Some(folders) = folders {
        let shipped = shipped_index();
        for (file, published) in read_index(&folders.updates).profiles {
            if !later(&published, shipped.profiles.get(&file)) {
                continue;
            }
            let text = std::fs::read_to_string(folders.updates.join(&file)).unwrap_or_default();
            match serde_json::from_str(&text) {
                Ok(profile) if digest(&text) == published.sha256 => take(&file, Source::Updated, profile),
                _ => warn!("controller profile {file} fetched is damaged: the shipped one stays"),
            }
        }
        for (file, read) in own_files(&folders.own) {
            match read.and_then(|text| serde_json::from_str(&text).map_err(|e| e.to_string())) {
                Ok(profile) => take(&file, Source::Own, profile),
                Err(e) => unreadable.push(format!("{file}: {e}")),
            }
        }
    }
    Library { known: files.into_values().collect(), replaced, unreadable }
}

/// The JSON files of `folder`, read.
fn own_files(folder: &Path) -> Vec<(String, Result<String, String>)> {
    let Ok(entries) = std::fs::read_dir(folder) else { return Vec::new() };
    let mut files: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")))
        .filter_map(|p| {
            let name = p.file_name()?.to_string_lossy().into_owned();
            Some((name, std::fs::read_to_string(&p).map_err(|e| e.to_string())))
        })
        .collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

fn shipped_index() -> Index {
    serde_json::from_str(SHIPPED_INDEX).unwrap_or_default()
}

fn read_index(folder: &Path) -> Index {
    std::fs::read(folder.join(INDEX))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Whether `published` is to be taken over what this version ships of it.
fn later(published: &Published, shipped: Option<&Published>) -> bool {
    published.format <= PROFILE_FORMAT && shipped.is_none_or(|s| published.revision > s.revision)
}

/// The digest of a profile's text, whatever ends its lines.
fn digest(text: &str) -> String {
    let hash = Sha256::digest(text.replace("\r\n", "\n").as_bytes());
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether the published profiles were last looked for long enough ago.
pub fn update_due() -> bool {
    let Some(folders) = FOLDERS.get() else { return false };
    let fetched = std::fs::metadata(folders.updates.join(INDEX)).and_then(|m| m.modified());
    fetched.map_or(true, |at| SystemTime::now().duration_since(at).unwrap_or_default() >= UPDATE_EVERY)
}

/// Fetches the profiles published since this version, or since they were last fetched, and
/// reads the profiles again: how many came.
pub async fn update() -> Result<usize, String> {
    let folders = FOLDERS.get().ok_or("Prisme has no data folder")?;
    let client = crate::http::client(FETCH_TIMEOUT)?;
    let index = crate::http::fetch(&client, &format!("{PUBLISHED}{INDEX}")).await?;
    let index: Index = serde_json::from_slice(&index).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&folders.updates).map_err(|e| e.to_string())?;
    let had = read_index(&folders.updates);
    // all fetched before any is written: a fetch that fails leaves the folder as it was
    let mut texts = Vec::new();
    for file in wanted(&index, &had, &folders.updates) {
        let bytes = crate::http::fetch(&client, &format!("{PUBLISHED}{file}")).await?;
        texts.push((file, String::from_utf8(bytes).map_err(|e| e.to_string())?));
    }
    let came = keep(&folders.updates, &index, &had, texts)?;
    if came > 0 {
        info!("{came} controller profiles updated");
    }
    reload();
    Ok(came)
}

/// The profiles of `index` to fetch into `folder`, which holds those of `had`: later than those
/// shipped, and not there yet.
fn wanted<'i>(index: &'i Index, had: &Index, folder: &Path) -> Vec<&'i str> {
    let shipped = shipped_index();
    index
        .profiles
        .iter()
        .filter(|&(file, published)| {
            let there = had.profiles.get(file) == Some(published) && folder.join(file).exists();
            plain_file_name(file) && later(published, shipped.profiles.get(file)) && !there
        })
        .map(|(file, _)| file.as_str())
        .collect()
}

/// Writes into `folder`, which holds the profiles of `had`, those fetched (`texts`) that are the
/// ones `index` tells, then the index of what it holds: of a profile not written, the one there
/// before. How many came.
fn keep(folder: &Path, index: &Index, had: &Index, texts: Vec<(&str, String)>) -> Result<usize, String> {
    let mut held = had.clone();
    held.profiles.retain(|file, _| index.profiles.contains_key(file));
    let (mut came, mut written) = (0, Ok(()));
    for (file, text) in texts {
        let Some(published) = index.profiles.get(file) else { continue };
        if digest(&text) != published.sha256 || serde_json::from_str::<Profile>(&text).is_err() {
            warn!("controller profile {file} published is not the one its index tells");
            continue;
        }
        written = write_whole(&folder.join(file), text.as_bytes());
        if written.is_err() {
            break;
        }
        held.profiles.insert(file.to_owned(), published.clone());
        came += 1;
    }
    let held = serde_json::to_vec_pretty(&held).map_err(|e| e.to_string())?;
    write_whole(&folder.join(INDEX), &held)?;
    written.map(|()| came)
}

/// A name of a file in the folder itself (an index names no other place).
fn plain_file_name(name: &str) -> bool {
    name.ends_with(".json") && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Writes `bytes` to `path` whole, or leaves what was there.
fn write_whole(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let partial = path.with_extension("part");
    std::fs::write(&partial, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(&partial, path).map_err(|e| e.to_string())
}

/// Writes `profile` to `path`, where the DJ chose: a file to share or to keep, which Prisme does
/// not read unless the DJ puts it among their own.
pub fn export(profile: &Profile, path: &Path) -> Result<(), String> {
    let profile = Profile { schema: Some(SCHEMA_URL.to_owned()), ..profile.clone() };
    let text = serde_json::to_string_pretty(&profile).map_err(|e| e.to_string())?;
    std::fs::write(path, format!("{text}\n")).map_err(|e| e.to_string())
}

/// The file the profile of `model` goes by: `Pioneer DDJ-400` → `pioneer-ddj-400.json`.
pub fn file_name(model: &str) -> String {
    format!("{}.json", file_stem(model))
}

/// `Pioneer DDJ-400` → `pioneer-ddj-400`.
fn file_stem(name: &str) -> String {
    let stem: String =
        name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let stem = stem.split('-').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("-");
    if stem.is_empty() {
        "controller".into()
    } else {
        stem
    }
}

/// The index of `files` (name, text), the revisions of `before` kept where a file did not
/// change and raised where it did.
#[cfg(test)]
fn index_of(files: &[(&str, &str)], before: &Index) -> Index {
    let profiles = files
        .iter()
        .map(|(file, text)| {
            let sha256 = digest(text);
            let published = match before.profiles.get(*file) {
                Some(p) if p.sha256 == sha256 => p.clone(),
                Some(p) => Published { sha256, revision: p.revision + 1, format: PROFILE_FORMAT },
                None => Published { sha256, revision: 1, format: PROFILE_FORMAT },
            };
            ((*file).to_owned(), published)
        })
        .collect();
    Index { profiles }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prisme_control::{profile_schema, Target};

    const FOLDER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/controllers");

    /// A folder of its own for a test.
    fn scratch(name: &str) -> Folders {
        let root = std::env::temp_dir().join(format!("prisme-profiles-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        Folders { own: root.join(OWN), updates: root.join(UPDATES) }
    }

    fn known<'l>(library: &'l Library, file: &str) -> &'l Known {
        library.known.iter().find(|k| k.file == file).expect("the profile")
    }

    /// The DDJ-400's profile renamed, as text.
    fn ddj_400_named(name: &str) -> String {
        let text = SHIPPED.iter().find(|(f, _)| *f == "pioneer-ddj-400.json").expect("shipped").1;
        let mut profile: Profile = serde_json::from_str(text).unwrap();
        profile.name = name.into();
        serde_json::to_string(&profile).unwrap()
    }

    /// `text` fetched as `file`, published at `revision` in `format`.
    fn fetched(folders: &Folders, file: &str, text: &str, revision: u32, format: u32) {
        std::fs::create_dir_all(&folders.updates).unwrap();
        std::fs::write(folders.updates.join(file), text).unwrap();
        let published = Published { sha256: digest(text), revision, format };
        let index = Index { profiles: BTreeMap::from([(file.to_owned(), published)]) };
        std::fs::write(folders.updates.join(INDEX), serde_json::to_vec(&index).unwrap()).unwrap();
    }

    #[test]
    fn every_shipped_profile_reads() {
        let library = read_library(None);
        assert_eq!(library.known.len(), SHIPPED.len());
        for Known { profile, .. } in &library.known {
            assert!(!profile.ports.is_empty(), "{} names no port", profile.name);
            assert!(!profile.controls.is_empty(), "{} maps nothing", profile.name);
        }
    }

    #[test]
    fn a_controller_finds_its_profile() {
        assert_eq!(of("DDJ-SB2").map(|p| p.name.clone()).as_deref(), Some("Pioneer DDJ-SB2"));
        let port = "PIONEER DDJ-SB2:PIONEER DDJ-SB2 MIDI 1 20:0";
        assert_eq!(of(port).map(|p| p.name.clone()).as_deref(), Some("Pioneer DDJ-SB2"));
        assert_eq!(of("DDJ-400").map(|p| p.name.clone()).as_deref(), Some("Pioneer DDJ-400"));
        assert!(of("DDJ-1000").is_none());
    }

    #[test]
    fn each_model_is_found_by_its_own_ports() {
        for Known { profile, .. } in &current().known {
            let brand = profile.name.split(' ').next().unwrap_or_default();
            for port in &profile.ports {
                for name in [port.clone(), format!("{brand} {port}"), format!("{brand} {port} MIDI 1")] {
                    assert_eq!(
                        of(&name).map(|p| p.name.clone()).as_deref(),
                        Some(profile.name.as_str()),
                        "{name}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_later_profile_fetched_replaces_the_shipped_one_and_the_djs_own_both() {
        let folders = scratch("layers");
        let shipped = shipped_index().profiles["pioneer-ddj-400.json"].revision;
        fetched(&folders, "pioneer-ddj-400.json", &ddj_400_named("Updated"), shipped + 1, PROFILE_FORMAT);
        let library = read_library(Some(&folders));
        assert_eq!(known(&library, "pioneer-ddj-400.json").source, Source::Updated);
        assert_eq!(known(&library, "pioneer-ddj-400.json").profile.name, "Updated");

        std::fs::create_dir_all(&folders.own).unwrap();
        std::fs::write(folders.own.join("pioneer-ddj-400.json"), ddj_400_named("Mine")).unwrap();
        std::fs::write(folders.own.join("my-own.json"), ddj_400_named("New model")).unwrap();
        std::fs::write(folders.own.join("broken.json"), "{ \"name\": ").unwrap();
        let library = read_library(Some(&folders));
        assert_eq!(known(&library, "pioneer-ddj-400.json").profile.name, "Mine");
        assert_eq!(known(&library, "my-own.json").source, Source::Own);
        assert_eq!(library.known.len(), SHIPPED.len() + 1);
        assert!(library.unreadable.len() == 1 && library.unreadable[0].starts_with("broken.json: "));
    }

    #[test]
    fn a_profile_of_the_djs_that_takes_the_place_of_prismes_is_told() {
        let folders = scratch("replaced");
        std::fs::create_dir_all(&folders.own).unwrap();
        let mut pads: Profile = serde_json::from_str(&ddj_400_named("My pads")).unwrap();
        pads.ports = vec!["MY-PADS".into()];
        std::fs::write(folders.own.join("my-pads.json"), serde_json::to_string(&pads).unwrap()).unwrap();
        let library = read_library(Some(&folders));
        assert_eq!(library.of("MY-PADS").map(|k| k.source), Some(Source::Own));
        assert!(library.replaced_on("MY-PADS").is_none(), "a model Prisme does not know replaces none");
        assert!(library.replaced_on("DDJ-400").is_none());

        // the same file name as Prisme's
        std::fs::write(folders.own.join("pioneer-ddj-400.json"), ddj_400_named("Mine")).unwrap();
        let library = read_library(Some(&folders));
        assert_eq!(library.of("DDJ-400").map(|k| k.profile.name.as_str()), Some("Mine"));
        assert_eq!(library.replaced_on("DDJ-400").map(|k| k.profile.name.as_str()), Some("Pioneer DDJ-400"));
        assert_eq!(
            library.replaced.iter().map(|k| k.file.as_str()).collect::<Vec<_>>(),
            ["pioneer-ddj-400.json"]
        );

        // another name, the same ports: the DJ's wins all the same, and says so
        std::fs::remove_file(folders.own.join("pioneer-ddj-400.json")).unwrap();
        std::fs::write(folders.own.join("zz-mine.json"), ddj_400_named("Mine")).unwrap();
        let library = read_library(Some(&folders));
        assert!(library.replaced.is_empty());
        assert_eq!(library.of("DDJ-400").map(|k| k.profile.name.as_str()), Some("Mine"));
        assert_eq!(library.replaced_on("DDJ-400").map(|k| k.profile.name.as_str()), Some("Pioneer DDJ-400"));
    }

    #[test]
    fn an_export_goes_where_the_dj_chose_and_leaves_their_profiles_alone() {
        let folders = scratch("export");
        std::fs::create_dir_all(&folders.updates).unwrap();
        let path = folders.updates.join("shared.json");
        let profile: Profile = serde_json::from_str(&ddj_400_named("Pioneer DDJ-400")).unwrap();
        export(&profile, &path).unwrap();
        let written: Profile = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written.schema.as_deref(), Some(SCHEMA_URL));
        assert_eq!(written.controls, profile.controls);
        assert!(!folders.own.exists());
        assert_eq!(file_name(&profile.name), "pioneer-ddj-400.json");
    }

    #[test]
    fn a_profile_fetched_is_left_when_older_damaged_or_of_a_later_format() {
        let shipped = shipped_index().profiles["pioneer-ddj-400.json"].revision;
        let text = ddj_400_named("Fetched");
        for (case, revision, format, damaged) in [
            ("older", shipped, PROFILE_FORMAT, false),
            ("later format", shipped + 1, PROFILE_FORMAT + 1, false),
            ("damaged", shipped + 1, PROFILE_FORMAT, true),
        ] {
            let folders = scratch(&case.replace(' ', "-"));
            fetched(&folders, "pioneer-ddj-400.json", &text, revision, format);
            if damaged {
                std::fs::write(folders.updates.join("pioneer-ddj-400.json"), ddj_400_named("Changed"))
                    .unwrap();
            }
            let library = read_library(Some(&folders));
            assert_eq!(known(&library, "pioneer-ddj-400.json").source, Source::Shipped, "{case}");
        }
    }

    /// The index publishing `files` (name, text) at `revision`.
    fn publishing(files: &[(&str, &str)], revision: u32) -> Index {
        let published = |text| Published { sha256: digest(text), revision, format: PROFILE_FORMAT };
        Index { profiles: files.iter().map(|(file, text)| ((*file).to_owned(), published(text))).collect() }
    }

    #[test]
    fn a_profile_published_unlike_its_index_leaves_the_one_fetched_before() {
        let folders = scratch("unlike");
        let file = "pioneer-ddj-400.json";
        let revision = shipped_index().profiles[file].revision + 1;
        let before = ddj_400_named("Before");
        fetched(&folders, file, &before, revision, PROFILE_FORMAT);
        let had = read_index(&folders.updates);
        let index = publishing(&[(file, &ddj_400_named("After"))], revision + 1);
        assert_eq!(wanted(&index, &had, &folders.updates), [file]);
        // a cache gives the text before with the index after
        assert_eq!(keep(&folders.updates, &index, &had, vec![(file, before)]), Ok(0));
        assert_eq!(known(&read_library(Some(&folders)), file).profile.name, "Before");
        assert_eq!(wanted(&index, &read_index(&folders.updates), &folders.updates), [file], "fetched again");
    }

    #[test]
    fn profiles_not_all_written_leave_an_index_telling_those_there() {
        let folders = scratch("not-all-written");
        let files = ["pioneer-ddj-400.json", "pioneer-ddj-flx4.json"];
        let revision = files.iter().map(|f| shipped_index().profiles[*f].revision).max().unwrap() + 1;
        let before = ddj_400_named("Before");
        std::fs::create_dir_all(&folders.updates).unwrap();
        for file in files {
            std::fs::write(folders.updates.join(file), &before).unwrap();
        }
        let had = publishing(&[(files[0], &before), (files[1], &before)], revision);
        std::fs::write(folders.updates.join(INDEX), serde_json::to_vec(&had).unwrap()).unwrap();
        let after = ddj_400_named("After");
        let index = publishing(&[(files[0], &after), (files[1], &after)], revision + 1);
        // the second cannot be written
        std::fs::create_dir(folders.updates.join("pioneer-ddj-flx4.part")).unwrap();
        let texts = files.iter().map(|&f| (f, after.clone())).collect();
        assert!(keep(&folders.updates, &index, &had, texts).is_err());
        let library = read_library(Some(&folders));
        assert_eq!(known(&library, files[0]).profile.name, "After");
        assert_eq!(known(&library, files[1]).profile.name, "Before");
    }

    #[test]
    fn a_changed_profile_is_published_as_a_later_revision() {
        let before = index_of(&[("a.json", "{}"), ("b.json", "[]")], &Index::default());
        assert_eq!(before.profiles["a.json"].revision, 1);
        let after = index_of(&[("a.json", "{}"), ("b.json", "[1]"), ("c.json", "")], &before);
        assert_eq!(after.profiles["a.json"], before.profiles["a.json"]);
        assert_eq!(after.profiles["b.json"].revision, 2);
        assert_eq!(after.profiles["c.json"].revision, 1);
        assert_eq!(digest("{\r\n}"), digest("{\n}"));
    }

    #[test]
    fn names_of_files_stay_in_their_folder() {
        assert!(plain_file_name("pioneer-ddj-400.json"));
        assert!(
            !plain_file_name("../preferences.json")
                && !plain_file_name("a/b.json")
                && !plain_file_name("x.exe")
        );
        assert_eq!(file_stem("Allen & Heath Xone:K2"), "allen-heath-xone-k2");
        assert_eq!(file_stem("***"), "controller");
    }

    /// The index and the schema published beside the profiles follow them and the format;
    /// `PRISME_WRITE_PROFILES=1` writes them (`targets.json` comes from the page's tests).
    #[test]
    fn the_published_index_and_schema_are_up_to_date() {
        let index = index_of(SHIPPED, &shipped_index());
        let index = format!("{}\n", serde_json::to_string_pretty(&index).unwrap());
        let targets: Vec<Target> =
            serde_json::from_str(&std::fs::read_to_string(format!("{FOLDER}/targets.json")).unwrap())
                .unwrap();
        let schema = format!("{}\n", serde_json::to_string_pretty(&profile_schema(&targets)).unwrap());
        for (file, text) in [(INDEX, index), ("profile.schema.json", schema)] {
            let path = format!("{FOLDER}/{file}");
            if std::env::var_os("PRISME_WRITE_PROFILES").is_some() {
                std::fs::write(&path, &text).unwrap();
            }
            let written = std::fs::read_to_string(&path).unwrap_or_default().replace("\r\n", "\n");
            assert!(written == text, "{file} is behind: run the test again with PRISME_WRITE_PROFILES=1");
        }
    }
}
