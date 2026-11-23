//! The releases' manifest (`latest.json`, attached to each release): the latest version, and
//! for each system its package and the package's signature.

use crate::Error;
use serde::Deserialize;
use std::collections::HashMap;

const URL: &str = "https://github.com/Shawiizz/prisme/releases/latest/download/latest.json";
/// Where a development build reads the manifest instead, if given.
#[cfg(debug_assertions)]
const URL_INSTEAD: &str = "PRISME_UPDATES_URL";

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub version: String,
    #[serde(default)]
    pub platforms: HashMap<String, Package>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Package {
    pub url: String,
    pub signature: String,
}

impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        Ok(serde_json::from_slice(bytes)?)
    }

    pub fn package(&self, platform: &str) -> Option<&Package> {
        self.platforms.get(platform)
    }
}

/// Where the manifest of the latest release is: in a development build, the one
/// `PRISME_UPDATES_URL` gives if it does.
pub fn manifest_url() -> String {
    #[cfg(debug_assertions)]
    if let Ok(url) = std::env::var(URL_INSTEAD) {
        return url;
    }
    URL.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_gives_the_package_of_each_system() {
        let manifest = Manifest::parse(
            br#"{
                "version": "0.7.0",
                "notes": "",
                "pub_date": "2026-11-24T10:00:00.000Z",
                "platforms": {
                    "windows-x86_64-app": { "signature": "c2ln", "url": "https://x/Prisme-0.7.0-windows-x64-update.tar.gz" }
                }
            }"#,
        )
        .unwrap();
        assert_eq!(manifest.version, "0.7.0");
        let package = manifest.package("windows-x86_64-app").unwrap();
        assert_eq!(package.signature, "c2ln");
        assert!(manifest.package("darwin-aarch64-app").is_none());
        assert!(Manifest::parse(b"<html>").is_err());
    }
}
