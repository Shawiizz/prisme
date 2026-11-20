//! The controllers in use and what drives what: the profiles, what was learnt over them, the
//! choices of the DJ kept in `controllers.json`; what the interface is shown of them.

use super::{base_port_name, ControllerSetup, Controllers, KeyWays, Shown, Unmapped};
use crate::controller_profiles;
use crate::dto::{ControllerDto, ControllersDto, OfferedProfileDto};
use crate::error::{AppError, AppResult};
use crate::state::{lock, AppState};
use prisme_control::hid::{self, Field};
use prisme_control::{
    Control, Encoding, Mapping, Mappings, Profile, ProfileControl, Reader, JOG_STEPS_PER_TURN,
};
use prisme_session::JsonFile;
use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tracing::warn;

impl Controllers {
    pub fn new(setup: JsonFile<ControllerSetup>) -> Self {
        let controllers = Self {
            setup,
            plugged: Mutex::new(Vec::new()),
            mappings: Mutex::new(Mappings::default()),
            learning: Mutex::new(None),
            just_learnt: Mutex::new(None),
            reader: Mutex::new(Reader::default()),
            connections: Mutex::new(Vec::new()),
            holdings: Mutex::new(Vec::new()),
            keys: Mutex::new(KeyWays::default()),
            hid_fields: Mutex::new(HashMap::new()),
        };
        controllers.layer();
        controllers
    }

    /// The profiles were read again: what they map, and the lights they give (shown at the
    /// next tick of [`Self::tick`]).
    pub fn profiles_changed(&self) {
        self.layer();
        for connection in lock(&self.connections).iter_mut() {
            connection.profile = self.profile(&connection.device);
        }
    }

    /// What `device` drives now, as the profile of its model: its own profile's, what was
    /// learnt over it in place of what it maps.
    pub fn as_profile(&self, device: &str) -> Profile {
        let controls: Vec<ProfileControl> = lock(&self.mappings)
            .all()
            .iter()
            .filter(|m| m.device == device && m.target.mode().is_none())
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
    pub(super) fn profile(&self, device: &str) -> Option<Arc<Profile>> {
        let chosen = self.setup.read(|s| s.profiles.get(device).cloned());
        controller_profiles::current().chosen(device, chosen.as_deref()).map(|k| k.profile.clone())
    }

    /// Steps in a turn of the jog wheels of `device`.
    pub(super) fn jog_steps_per_turn(&self, device: &str) -> f64 {
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
                Ok(_) => {
                    let controllers = app.state::<Controllers>();
                    controllers.profiles_changed();
                    controllers.show(&app.state::<AppState>());
                }
                Err(e) => warn!("controller profiles not looked for: {e}"),
            }
        });
        if let Err(e) = spawned {
            warn!("controller profiles not looked for: {e}");
        }
    }

    pub fn setup(&self) -> ControllerSetup {
        self.setup.get()
    }

    /// Looks for the controllers plugged in now, and shows them.
    pub fn look(&self, state: &AppState) {
        *lock(&self.plugged) = Self::plugged();
        self.show(state);
    }

    /// Shows the interface the controllers, what they drive, the action being learnt: its
    /// section moves if any of it changed.
    pub fn show(&self, state: &AppState) {
        let shown = Shown {
            listed: self.listed(),
            mappings: lock(&self.mappings).all().to_vec(),
            clock_output: self.setup.read(|s| s.clock_output.clone()),
            learning: lock(&self.learning).as_ref().map(|l| l.target.clone()),
        };
        state.controllers.set(shown);
    }

    /// The controllers plugged in when last looked for, and those in use that are not.
    pub(super) fn listed(&self) -> ControllersDto {
        let plugged = lock(&self.plugged).clone();
        let setup = self.setup();
        let missing: Vec<String> =
            setup.enabled.iter().filter(|name| !plugged.contains(name)).cloned().collect();
        let library = controller_profiles::current();
        let dto = |name: String, plugged: bool| {
            let known = library.chosen(&name, setup.profiles.get(&name).map(String::as_str));
            let models = known.map(|k| &k.file) == library.of(&name).map(|k| &k.file);
            let (enabled, hid) = (setup.enabled.contains(&name), hid::ids_of(&name).is_some());
            ControllerDto {
                listed: !hid || known.is_some() || enabled,
                can_export: enabled && plugged,
                enabled,
                profile: known.map(|k| k.profile.name.clone()),
                profile_file: known.map(|k| k.file.clone()),
                offered: library
                    .offered(&name)
                    .into_iter()
                    .map(|k| OfferedProfileDto { file: k.file.clone(), name: k.profile.name.clone() })
                    .collect(),
                verified: known.is_some_and(|k| k.profile.verified),
                profile_source: known.map(|k| k.source),
                replaces: library.replaced_on(&name).filter(|_| models).map(|k| k.profile.name.clone()),
                export_file: controller_profiles::file_name(&self.model_name(&name)),
                name,
                plugged,
            }
        };
        let controllers: Vec<ControllerDto> = plugged
            .into_iter()
            .map(|n| dto(n, true))
            .chain(missing.into_iter().map(|n| dto(n, false)))
            .collect();
        ControllersDto { can_learn: controllers.iter().any(|c| c.can_export), controllers }
    }

    /// What drives what: the profiles of the controllers in use, and what was learnt.
    #[cfg(test)]
    pub(super) fn mappings(&self) -> Mappings {
        lock(&self.mappings).clone()
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
        self.show_lights(app, device);
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

    /// Changes how the control of `target` on `device` is read (an encoder's steps). A
    /// control of a profile becomes the DJ's own, read that way.
    pub fn set_encoding(&self, device: &str, target: &str, encoding: Encoding) {
        let of_target = |m: &&Mapping| m.device == device && m.target == target;
        let profiled: Vec<Mapping> = lock(&self.mappings).all().iter().filter(of_target).cloned().collect();
        self.save(|s| {
            if !s.mappings.all().iter().any(|m| of_target(&m)) {
                // each of the profile's controls of the action, as many as it has
                for mapping in profiled {
                    s.mappings.learn_another(mapping);
                }
            }
            s.mappings.set_encoding(device, target, encoding);
        });
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

    pub(super) fn save(&self, change: impl FnOnce(&mut ControllerSetup)) {
        self.setup.update(change);
        self.layer();
    }

    /// Lays what was learnt over the profiles of the controllers in use.
    pub(super) fn layer(&self) {
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
            Mappings::layered(profiled, &s.mappings, unmapped)
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

impl Unmapped {
    /// Whether `mapping` is taken off by this.
    pub(super) fn covers(&self, mapping: &Mapping) -> bool {
        match self {
            Unmapped::On { device, target } => mapping.device == *device && mapping.target == *target,
            Unmapped::Everywhere(target) => mapping.target == *target,
        }
    }
}
