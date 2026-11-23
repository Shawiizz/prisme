//! What the running Prisme is, as installed, and how a new version takes its place.
//!
//! Where the running one can go on from a file renamed (Windows, an AppImage), the new version
//! takes its place at once and the next launch is it. A Mac application's bundle is read while
//! it runs: the new one is unpacked beside it, and put in its place as Prisme quits, or as it
//! starts again if it did not quit normally.

#[cfg(any(windows, target_os = "macos"))]
use crate::package;
use crate::{Error, Identity};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Install {
    /// Windows: `prisme-desktop.exe` in its folder of the user's local data.
    #[cfg(windows)]
    Windows { exe: PathBuf },
    /// macOS: the application's bundle.
    #[cfg(target_os = "macos")]
    MacApp { bundle: PathBuf },
    /// Linux: the AppImage run.
    #[cfg(target_os = "linux")]
    AppImage { file: PathBuf },
    /// Installed by the system's packages (a .deb): they update it.
    #[cfg(target_os = "linux")]
    SystemPackage,
    /// Run where it was built, or copied anywhere by hand: not updated.
    Elsewhere,
}

impl Install {
    /// What this process is, as `identity` installs it.
    pub fn of_this_process(identity: &Identity) -> Self {
        let Ok(exe) = std::env::current_exe() else { return Self::Elsewhere };
        Self::of(&exe, identity)
    }

    #[cfg_attr(not(windows), allow(unused_variables))]
    fn of(exe: &Path, identity: &Identity) -> Self {
        #[cfg(windows)]
        {
            let same = |a: &Path, b: &Path| a.as_os_str().eq_ignore_ascii_case(b.as_os_str());
            let installed =
                identity.folder().is_some_and(|folder| exe.parent().is_some_and(|p| same(p, &folder)));
            let named = exe.file_name().is_some_and(|n| n.eq_ignore_ascii_case(crate::EXE));
            if installed && named {
                return Self::Windows { exe: exe.to_owned() };
            }
        }
        #[cfg(target_os = "macos")]
        {
            // the executable is <bundle>.app/Contents/MacOS/<name>
            let bundle = exe.parent().and_then(Path::parent).and_then(Path::parent);
            if let Some(bundle) = bundle.filter(|b| b.extension().is_some_and(|e| e == "app")) {
                return Self::MacApp { bundle: bundle.to_owned() };
            }
        }
        #[cfg(target_os = "linux")]
        {
            if let Some(file) = std::env::var_os("APPIMAGE").filter(|f| !f.is_empty()) {
                return Self::AppImage { file: file.into() };
            }
            if exe.starts_with("/usr") {
                return Self::SystemPackage;
            }
        }
        Self::Elsewhere
    }

    /// Its package's name in the releases' manifest, if it updates itself.
    pub fn platform(&self) -> Option<&'static str> {
        match self {
            #[cfg(windows)]
            Self::Windows { .. } => {
                Some(if cfg!(target_arch = "aarch64") { "windows-aarch64-app" } else { "windows-x86_64-app" })
            }
            #[cfg(target_os = "macos")]
            Self::MacApp { .. } => {
                Some(if cfg!(target_arch = "aarch64") { "darwin-aarch64-app" } else { "darwin-x86_64-app" })
            }
            #[cfg(target_os = "linux")]
            Self::AppImage { .. } => Some("linux-x86_64-appimage"),
            #[cfg(target_os = "linux")]
            Self::SystemPackage => None,
            Self::Elsewhere => None,
        }
    }

    /// Puts version `version`, its package `bytes` checked already, in place of the running
    /// one: at once where the running one goes on from its file renamed, else beside it, put
    /// in place by [`Install::apply`].
    pub fn stage(&self, version: &str, bytes: &[u8]) -> Result<(), Error> {
        match self {
            #[cfg(windows)]
            Self::Windows { exe } => {
                let unpacked = exe.with_file_name("update");
                let _ = std::fs::remove_dir_all(&unpacked);
                package::unpack(bytes, &unpacked)?;
                let new = unpacked.join(crate::EXE);
                if !new.is_file() {
                    return Err(Error::Missing(crate::EXE.into()));
                }
                crate::windows::replace_running(exe, &new)?;
                let _ = std::fs::remove_dir_all(&unpacked);
                let _ = version;
                Ok(())
            }
            #[cfg(target_os = "macos")]
            Self::MacApp { bundle } => {
                let staged = staged_beside(bundle);
                let _ = std::fs::remove_dir_all(&staged);
                package::unpack(bytes, &staged)?;
                bundle_in(&staged)?;
                std::fs::write(staged.join(VERSION_FILE), version)?;
                Ok(())
            }
            #[cfg(target_os = "linux")]
            Self::AppImage { file } => {
                use std::os::unix::fs::PermissionsExt;
                let _ = version;
                let new = staged_beside(file);
                std::fs::write(&new, bytes)?;
                std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755))?;
                // the running one goes on from its own, which stays as long as it is open
                std::fs::rename(&new, file)?;
                Ok(())
            }
            #[cfg(target_os = "linux")]
            Self::SystemPackage => Err(not_updated()),
            Self::Elsewhere => {
                let _ = (version, bytes);
                Err(not_updated())
            }
        }
    }

    /// Puts in place what [`Install::stage`] set beside the running version `running`, if a
    /// newer one waits: the version put, if one was.
    pub fn apply(&self, running: &str) -> Result<Option<String>, Error> {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacApp { bundle } => {
                let staged = staged_beside(bundle);
                let Ok(version) = std::fs::read_to_string(staged.join(VERSION_FILE)) else {
                    return Ok(None);
                };
                if !crate::newer(&version, running) {
                    let _ = std::fs::remove_dir_all(&staged);
                    return Ok(None);
                }
                let aside = with_name(bundle, |name| format!(".{name}.old"));
                let _ = std::fs::remove_dir_all(&aside);
                std::fs::rename(bundle, &aside)?;
                if let Err(e) = std::fs::rename(bundle_in(&staged)?, bundle) {
                    let _ = std::fs::rename(&aside, bundle);
                    return Err(e.into());
                }
                let _ = std::fs::remove_dir_all(&aside);
                let _ = std::fs::remove_dir_all(&staged);
                Ok(Some(version))
            }
            _ => {
                let _ = running;
                Ok(None)
            }
        }
    }

    /// Clears what earlier updates left: on Windows the files the versions before went on
    /// from, once they are gone.
    pub fn tidy(&self) {
        match self {
            #[cfg(windows)]
            Self::Windows { exe } => {
                crate::windows::clear_aside(exe);
                let _ = std::fs::remove_dir_all(exe.with_file_name("update"));
            }
            #[cfg(target_os = "linux")]
            Self::AppImage { file } => {
                let _ = std::fs::remove_file(staged_beside(file));
            }
            _ => {}
        }
    }

    /// Starts the version in place, once process `pid` (this one, quitting) is gone: two would
    /// share the library.
    pub fn launch_after(&self, pid: u32) -> std::io::Result<()> {
        let after = ["--after".to_owned(), pid.to_string()];
        match self {
            #[cfg(windows)]
            Self::Windows { exe } => {
                Command::new(exe).args(after).current_dir(exe.parent().unwrap_or(exe)).spawn()?;
            }
            #[cfg(target_os = "macos")]
            Self::MacApp { bundle } => {
                Command::new("/usr/bin/open").arg("-n").arg(bundle).arg("--args").args(after).spawn()?;
            }
            #[cfg(target_os = "linux")]
            Self::AppImage { file } => {
                // the new one sets them for itself, from where it is mounted
                let mut command = Command::new(file);
                for name in ["APPDIR", "APPIMAGE", "ARGV0", "OWD"] {
                    command.env_remove(name);
                }
                command.args(after).spawn()?;
            }
            #[cfg(target_os = "linux")]
            Self::SystemPackage => return Err(not_updated().into_io()),
            Self::Elsewhere => {
                let _ = after;
                return Err(not_updated().into_io());
            }
        }
        Ok(())
    }
}

/// Waits for process `pid` to end, `limit` at most: whether it did.
pub fn wait_for_exit(pid: u32, limit: Duration) -> bool {
    #[cfg(windows)]
    {
        crate::windows::wait_for_exit(pid, limit)
    }
    #[cfg(unix)]
    {
        let started = std::time::Instant::now();
        // SAFETY: signal 0 only asks whether the process is there
        while unsafe { libc::kill(pid as libc::pid_t, 0) } == 0 {
            if started.elapsed() >= limit {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        true
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = (pid, limit);
        true
    }
}

fn not_updated() -> Error {
    Error::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "this copy of Prisme does not update itself",
    ))
}

impl Error {
    fn into_io(self) -> std::io::Error {
        match self {
            Error::Io(e) => e,
            other => std::io::Error::other(other.to_string()),
        }
    }
}

#[cfg(target_os = "macos")]
const VERSION_FILE: &str = "version";

/// `path` named as `name` says from its own name, in the same folder.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn with_name(path: &Path, name: impl Fn(&str) -> String) -> PathBuf {
    let own = path.file_name().and_then(|n| n.to_str()).unwrap_or("Prisme");
    path.with_file_name(name(own))
}

/// Where the new version waits beside `installed`: in its folder, on the same disk, where it
/// moves in place at once.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn staged_beside(installed: &Path) -> PathBuf {
    with_name(installed, |name| format!(".{name}.update"))
}

/// The application's bundle a package unpacked in `folder` holds.
#[cfg(target_os = "macos")]
fn bundle_in(folder: &Path) -> Result<PathBuf, Error> {
    std::fs::read_dir(folder)?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|e| e == "app") && path.is_dir())
        .ok_or_else(|| Error::Missing("application".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_run_where_it_was_built_does_not_update_itself() {
        let identity = Identity { name: "PrismeTest".into() };
        let install = Install::of(Path::new("/work/target/debug/prisme-desktop"), &identity);
        assert!(install.platform().is_none());
        assert!(install.stage("9.9.9", b"").is_err());
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_the_exe_in_its_folder_updates_itself() {
        let identity = Identity { name: "PrismeTest".into() };
        let folder = identity.folder().unwrap();
        let install = Install::of(&folder.join("PRISME-DESKTOP.exe"), &identity);
        assert!(matches!(install, Install::Windows { .. }));
        assert_eq!(install.platform(), Some("windows-x86_64-app"));
        assert_eq!(Install::of(&folder.join("other.exe"), &identity), Install::Elsewhere);
    }

    #[cfg(windows)]
    #[test]
    fn on_windows_a_new_version_takes_the_place_of_the_one_running() {
        let folder = tempfile::tempdir().unwrap();
        let exe = folder.path().join(crate::EXE);
        // a program running from the file replaced
        std::fs::copy(std::env::var("COMSPEC").unwrap(), &exe).unwrap();
        let mut running = Command::new(&exe).args(["/C", "ping -n 30 127.0.0.1 >NUL"]).spawn().unwrap();
        let install = Install::Windows { exe: exe.clone() };
        let package = crate::package::tests::pack(&[(crate::EXE, b"new version")]);
        install.stage("9.9.9", &package).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new version");
        assert!(!folder.path().join("update").exists());
        let names = || std::fs::read_dir(folder.path()).unwrap().count();
        assert_eq!(names(), 2, "the running one's file named aside");
        install.tidy();
        assert_eq!(names(), 2, "kept while it runs");
        running.kill().unwrap();
        running.wait().unwrap();
        install.tidy();
        assert_eq!(names(), 1, "cleared once it is gone");
        let empty = crate::package::tests::pack(&[("other.exe", b"")]);
        assert!(matches!(install.stage("9.9.9", &empty), Err(Error::Missing(_))));
        assert_eq!(std::fs::read(&exe).unwrap(), b"new version");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn on_macos_the_new_bundle_waits_beside_until_applied() {
        let folder = tempfile::tempdir().unwrap();
        let bundle = folder.path().join("Prisme.app");
        std::fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        std::fs::write(bundle.join("Contents/MacOS/prisme-desktop"), b"old").unwrap();
        let install = Install::MacApp { bundle: bundle.clone() };
        let package = crate::package::tests::pack(&[("Prisme.app/Contents/MacOS/prisme-desktop", b"new")]);
        install.stage("9.9.9", &package).unwrap();
        let read = || std::fs::read(bundle.join("Contents/MacOS/prisme-desktop")).unwrap();
        assert_eq!(read(), b"old");
        assert_eq!(install.apply("9.9.9").unwrap(), None, "not newer than the one running");
        install.stage("9.9.9", &package).unwrap();
        assert_eq!(install.apply("1.0.0").unwrap().as_deref(), Some("9.9.9"));
        assert_eq!(read(), b"new");
        assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 1);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn on_linux_the_appimage_is_replaced_at_once() {
        use std::os::unix::fs::PermissionsExt;
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("Prisme.AppImage");
        std::fs::write(&file, b"old").unwrap();
        let install = Install::AppImage { file: file.clone() };
        install.stage("9.9.9", b"new").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"new");
        assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o755);
    }
}
