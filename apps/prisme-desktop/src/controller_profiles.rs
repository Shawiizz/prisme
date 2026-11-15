//! The controller models Prisme knows (`controllers/*.json`): the profile of one maps it as
//! soon as it is turned on, before anything is learnt on it.

use prisme_control::{Profile, JOG_STEPS_PER_TURN};
use std::sync::OnceLock;
use tracing::error;

const BUNDLED: &[(&str, &str)] = &[
    ("allen-and-heath-xone-k2.json", include_str!("../controllers/allen-and-heath-xone-k2.json")),
    ("behringer-cmd-micro.json", include_str!("../controllers/behringer-cmd-micro.json")),
    ("behringer-cmd-mm-1.json", include_str!("../controllers/behringer-cmd-mm-1.json")),
    ("behringer-cmd-studio-4a.json", include_str!("../controllers/behringer-cmd-studio-4a.json")),
    ("denon-dn-sc2000.json", include_str!("../controllers/denon-dn-sc2000.json")),
    ("denon-mc3000.json", include_str!("../controllers/denon-mc3000.json")),
    ("denon-mc4000.json", include_str!("../controllers/denon-mc4000.json")),
    ("denon-mc6000mk2.json", include_str!("../controllers/denon-mc6000mk2.json")),
    ("denon-mc7000.json", include_str!("../controllers/denon-mc7000.json")),
    (
        "dj-techtools-midi-fighter-twister.json",
        include_str!("../controllers/dj-techtools-midi-fighter-twister.json"),
    ),
    ("hercules-dj-console-mk4.json", include_str!("../controllers/hercules-dj-console-mk4.json")),
    ("hercules-dj-console-rmx-2.json", include_str!("../controllers/hercules-dj-console-rmx-2.json")),
    ("hercules-dj-control-air.json", include_str!("../controllers/hercules-dj-control-air.json")),
    ("hercules-dj-control-instinct.json", include_str!("../controllers/hercules-dj-control-instinct.json")),
    ("hercules-dj-control-mp3-e2.json", include_str!("../controllers/hercules-dj-control-mp3-e2.json")),
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
    ("korg-kaoss-dj.json", include_str!("../controllers/korg-kaoss-dj.json")),
    (
        "native-instruments-traktor-kontrol-x1.json",
        include_str!("../controllers/native-instruments-traktor-kontrol-x1.json"),
    ),
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
    ("numark-ns4fx.json", include_str!("../controllers/numark-ns4fx.json")),
    ("numark-ns6ii.json", include_str!("../controllers/numark-ns6ii.json")),
    ("numark-ns7.json", include_str!("../controllers/numark-ns7.json")),
    ("numark-party-mix.json", include_str!("../controllers/numark-party-mix.json")),
    ("numark-scratch.json", include_str!("../controllers/numark-scratch.json")),
    ("pioneer-ddj-200.json", include_str!("../controllers/pioneer-ddj-200.json")),
    ("pioneer-ddj-400.json", include_str!("../controllers/pioneer-ddj-400.json")),
    ("pioneer-ddj-flx4.json", include_str!("../controllers/pioneer-ddj-flx4.json")),
    ("pioneer-ddj-sb.json", include_str!("../controllers/pioneer-ddj-sb.json")),
    ("pioneer-ddj-sb2.json", include_str!("../controllers/pioneer-ddj-sb2.json")),
    ("pioneer-ddj-sb3.json", include_str!("../controllers/pioneer-ddj-sb3.json")),
    ("pioneer-ddj-sx.json", include_str!("../controllers/pioneer-ddj-sx.json")),
    ("reloop-beatmix-2-4-mk2.json", include_str!("../controllers/reloop-beatmix-2-4-mk2.json")),
    ("reloop-beatpad.json", include_str!("../controllers/reloop-beatpad.json")),
    (
        "reloop-digital-jockey-2-interface-edition.json",
        include_str!("../controllers/reloop-digital-jockey-2-interface-edition.json"),
    ),
    ("reloop-jockey-3-me.json", include_str!("../controllers/reloop-jockey-3-me.json")),
    ("reloop-mixage.json", include_str!("../controllers/reloop-mixage.json")),
    ("reloop-terminal-mix-2-4.json", include_str!("../controllers/reloop-terminal-mix-2-4.json")),
    ("roland-dj-505.json", include_str!("../controllers/roland-dj-505.json")),
    ("stanton-djc-4.json", include_str!("../controllers/stanton-djc-4.json")),
];

fn bundled() -> &'static [Profile] {
    static PROFILES: OnceLock<Vec<Profile>> = OnceLock::new();
    PROFILES.get_or_init(|| {
        BUNDLED
            .iter()
            .filter_map(|(file, json)| {
                serde_json::from_str(json)
                    .inspect_err(|e| error!("controller profile {file} unreadable: {e}"))
                    .ok()
            })
            .collect()
    })
}

/// The profile of the controller on the MIDI port `port`, if Prisme knows its model.
pub fn of(port: &str) -> Option<&'static Profile> {
    Profile::of_port(bundled(), port)
}

/// Steps in a turn of the jog wheels of the controller on `port`.
pub fn jog_steps_per_turn(port: &str) -> f64 {
    of(port).map_or(JOG_STEPS_PER_TURN, |p| p.jog_steps_per_turn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_profile_reads() {
        assert_eq!(bundled().len(), BUNDLED.len());
        for profile in bundled() {
            assert!(!profile.ports.is_empty(), "{} names no port", profile.name);
            assert!(!profile.controls.is_empty(), "{} maps nothing", profile.name);
        }
    }

    #[test]
    fn a_controller_finds_its_profile() {
        assert_eq!(of("DDJ-SB2").map(|p| p.name.as_str()), Some("Pioneer DDJ-SB2"));
        assert_eq!(
            of("PIONEER DDJ-SB2:PIONEER DDJ-SB2 MIDI 1 20:0").map(|p| p.name.as_str()),
            Some("Pioneer DDJ-SB2")
        );
        assert_eq!(of("DDJ-400").map(|p| p.name.as_str()), Some("Pioneer DDJ-400"));
        assert!(of("DDJ-1000").is_none());
    }

    #[test]
    fn each_model_is_found_by_its_own_ports() {
        for profile in bundled() {
            let brand = profile.name.split(' ').next().unwrap_or_default();
            for port in &profile.ports {
                for name in [port.clone(), format!("{brand} {port}"), format!("{brand} {port} MIDI 1")] {
                    assert_eq!(of(&name).map(|p| p.name.as_str()), Some(profile.name.as_str()), "{name}");
                }
            }
        }
    }
}
