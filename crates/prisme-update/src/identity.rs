//! What an installation is called on Windows: its folder, its entry among the installed
//! applications, its shortcuts. A development build installs under another name, to try an
//! installation and its updates beside the real one.

use std::path::PathBuf;

const NAME: &str = "Prisme";
/// Prisme's executable on Windows.
pub const EXE: &str = "prisme-desktop.exe";
pub const PUBLISHER: &str = "Shawiizz";
/// What Windows groups Prisme's windows and shortcuts under.
pub const APP_ID: &str = "com.shawiizz.prisme";
/// The name a development build installs under instead, if given.
#[cfg(debug_assertions)]
const INSTALL_AS: &str = "PRISME_INSTALL_AS";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub name: String,
}

impl Identity {
    /// Prisme, or in a development build the name `PRISME_INSTALL_AS` gives (letters and
    /// digits).
    pub fn current() -> Self {
        #[cfg(debug_assertions)]
        if let Some(name) = std::env::var(INSTALL_AS).ok().filter(|n| is_name(n)) {
            return Self { name };
        }
        Self { name: NAME.into() }
    }

    /// Where it is installed: its folder in the user's local data.
    pub fn folder(&self) -> Option<PathBuf> {
        let local = std::env::var_os("LOCALAPPDATA").filter(|d| !d.is_empty())?;
        Some(PathBuf::from(local).join(&self.name))
    }

    /// Prisme's for Prisme, its own for another.
    pub fn app_id(&self) -> String {
        if self.name == NAME {
            APP_ID.into()
        } else {
            format!("{APP_ID}.{}", self.name.to_lowercase())
        }
    }
}

#[cfg(any(debug_assertions, test))]
fn is_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 40 && name.chars().all(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn another_name_is_made_of_letters_and_digits_only() {
        assert!(is_name("PrismeEssai"));
        for name in ["", "..", "Prisme Essai", r"a\b", "a/b"] {
            assert!(!is_name(name), "{name}");
        }
        let other = Identity { name: "PrismeEssai".into() };
        assert_eq!(other.app_id(), "com.shawiizz.prisme.prismeessai");
        assert_eq!(Identity { name: NAME.into() }.app_id(), APP_ID);
    }
}
