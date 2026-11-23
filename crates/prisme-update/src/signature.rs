//! A package is taken only as the release signed it: with the key whose public half Prisme
//! carries, for the version the manifest announces (an older package signed then cannot pass
//! for a newer one).

use crate::Error;
use base64::Engine;
use minisign_verify::{PublicKey, Signature};

/// The releases' public key (minisign's, in base64, as `tauri signer` gives it).
const PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEExNDg0MERDNzQ2RTRENDgKUldSSVRXNTAzRUJJb1JQR2QweVJ0WmhpeE9hN1BFRC9hRkJVQWJkZ0l3Y2dUdm41WmdyQXI1bzUK";
/// The key a development build checks the packages against instead, if given: to try updates
/// signed by a key of the test's own.
#[cfg(debug_assertions)]
const KEY_INSTEAD: &str = "PRISME_UPDATES_KEY";

/// Whether `bytes` are the package the release of `version` signed with `signature` (base64,
/// as the manifest gives it).
pub fn verify(bytes: &[u8], signature: &str, version: &str) -> Result<(), Error> {
    #[cfg(debug_assertions)]
    if let Ok(key) = std::env::var(KEY_INSTEAD) {
        return verify_with(bytes, signature, version, &key);
    }
    verify_with(bytes, signature, version, PUBLIC_KEY)
}

fn verify_with(bytes: &[u8], signature: &str, version: &str, key: &str) -> Result<(), Error> {
    let text = |b64: &str| {
        let bytes =
            base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|_| Error::Signature)?;
        String::from_utf8(bytes).map_err(|_| Error::Signature)
    };
    let key = PublicKey::decode(&text(key)?).map_err(|_| Error::Signature)?;
    let signature = Signature::decode(&text(signature)?).map_err(|_| Error::Signature)?;
    key.verify(bytes, &signature, true).map_err(|_| Error::Signature)?;
    // read only once verified: the global signature covers it
    match signed_version(signature.trusted_comment()) {
        Some(signed) if signed.trim_start_matches('v') != version.trim_start_matches('v') => {
            Err(Error::SignedVersion { signed: signed.into(), announced: version.into() })
        }
        _ => Ok(()),
    }
}

/// The `version` the signature's trusted comment carries (`timestamp:…\tfile:…\tversion:1.2.3`),
/// if it does: older signatures carry none.
fn signed_version(comment: &str) -> Option<&str> {
    comment.split('\t').find_map(|pair| pair.strip_prefix("version:"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Cursor;

    /// A key pair of the tests' own, its public half as the manifest's keys are written, and a
    /// signer of `bytes` with `trusted` as comment, its signature as the manifest gives it.
    pub(crate) fn signer() -> (String, impl Fn(&[u8], &str) -> String) {
        let pair = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let b64 = |text: String| base64::engine::general_purpose::STANDARD.encode(text);
        let public = b64(pair.pk.to_box().unwrap().to_string());
        let sign = move |bytes: &[u8], trusted: &str| {
            let signature = minisign::sign(Some(&pair.pk), &pair.sk, Cursor::new(bytes), Some(trusted), None);
            b64(signature.unwrap().to_string())
        };
        (public, sign)
    }

    #[test]
    fn a_package_passes_only_as_signed_for_its_version() {
        let (key, sign) = signer();
        let package = b"Prisme 0.7.0";
        let signed = sign(package, "timestamp:1\tfile:Prisme.tar.gz\tversion:0.7.0");
        assert!(verify_with(package, &signed, "0.7.0", &key).is_ok());
        assert!(matches!(verify_with(b"Prisme 0.7.1", &signed, "0.7.0", &key), Err(Error::Signature)));
        assert!(matches!(verify_with(package, &signed, "0.8.0", &key), Err(Error::SignedVersion { .. })));
        let unversioned = sign(package, "timestamp:1\tfile:Prisme.tar.gz");
        assert!(verify_with(package, &unversioned, "0.7.0", &key).is_ok());
        let (other_key, _) = signer();
        assert!(matches!(verify_with(package, &signed, "0.7.0", &other_key), Err(Error::Signature)));
        assert!(matches!(verify_with(package, "not base64", "0.7.0", &key), Err(Error::Signature)));
    }

    #[test]
    fn the_releases_key_is_readable() {
        let text = base64::engine::general_purpose::STANDARD.decode(PUBLIC_KEY).unwrap();
        assert!(PublicKey::decode(std::str::from_utf8(&text).unwrap()).is_ok());
    }
}
