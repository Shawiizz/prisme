//! The controller models Prisme knows (`controllers/*.json`): the profile of one maps it as
//! soon as it is turned on, before anything is learnt on it.

use prisme_control::Profile;
use std::sync::OnceLock;
use tracing::error;

const BUNDLED: &[(&str, &str)] =
    &[("pioneer-ddj-sb2.json", include_str!("../controllers/pioneer-ddj-sb2.json"))];

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
    bundled().iter().find(|p| p.fits(port))
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
        assert!(of("DDJ-400").is_none());
    }
}
