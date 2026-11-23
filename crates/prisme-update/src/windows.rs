//! Prisme installed on Windows as the user's own, without asking for the administrator: its
//! folder in the local data, its entry among the installed applications, its shortcuts, its
//! uninstallation. Also what the NSIS installer of the versions before left, cleared.

use crate::{Identity, PUBLISHER};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteKeyW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ,
};

const NO_WINDOW: u32 = 0x0800_0000;
/// Where the NSIS installer of the versions before 0.7 kept the folder it installed in, by the
/// part of Prisme's identifier naming its publisher.
const NSIS_PUBLISHER_KEY: &str = r"Software\shawiizz";
const HOME_PAGE: &str = "https://github.com/Shawiizz/prisme";

/// `new` takes the place of `exe`, which may be running: Windows lets a running program's file
/// be renamed, not replaced. The file it goes on from is named aside, and deleted once it is
/// gone ([`clear_aside`]).
pub fn replace_running(exe: &Path, new: &Path) -> std::io::Result<()> {
    if !exe.exists() {
        return std::fs::rename(new, exe);
    }
    let aside = (1..)
        .map(|n| exe.with_file_name(format!("{}.old-{n}.exe", stem(exe))))
        .find(|path| !path.exists())
        .expect("a free name");
    std::fs::rename(exe, &aside)?;
    if let Err(e) = std::fs::rename(new, exe) {
        let _ = std::fs::rename(&aside, exe);
        return Err(e);
    }
    // gone at once if it was not running
    let _ = std::fs::remove_file(&aside);
    Ok(())
}

/// Deletes the files of the versions `exe` replaced, those no longer running.
pub fn clear_aside(exe: &Path) {
    let (Some(folder), prefix) = (exe.parent(), format!("{}.old-", stem(exe))) else { return };
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(&prefix) && name.ends_with(".exe") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

fn stem(exe: &Path) -> String {
    exe.file_stem().map_or_else(|| "prisme-desktop".into(), |s| s.to_string_lossy().into_owned())
}

/// Waits for process `pid` to end, `limit` at most: whether it did (or was not there).
pub fn wait_for_exit(pid: u32, limit: Duration) -> bool {
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    // SAFETY: no pointer is given; the handle is closed once
    unsafe {
        let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if process.is_null() {
            return true;
        }
        let ended =
            WaitForSingleObject(process, limit.as_millis().min(u32::MAX as u128) as u32) == WAIT_OBJECT_0;
        CloseHandle(process);
        ended
    }
}

/// Whether a program of `folder` runs, but this process: an installed Prisme, or the file a
/// version replaced goes on from. Read from the system's list of processes, no program started
/// to ask.
pub fn runs_from(folder: &Path) -> bool {
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    // SAFETY: no pointer is given
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return false;
    }
    let Some(folder) = FileId::of(folder) else { return false };
    let mut entry = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
    let mut found = false;
    // SAFETY: the snapshot is open and the entry says its own size
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) } != 0;
    while more && !found {
        found = entry.th32ProcessID != std::process::id()
            && image(entry.th32ProcessID)
                .and_then(|path| path.parent().and_then(FileId::of))
                .is_some_and(|parent| parent == folder);
        // SAFETY: as above
        more = unsafe { Process32NextW(snapshot, &mut entry) } != 0;
    }
    // SAFETY: the snapshot is open, and closed once
    unsafe { CloseHandle(snapshot) };
    found
}

/// What a file or a folder is on its disk, whatever path reaches it: a folder redirected (an
/// application's own data, for a packaged program), a junction, a short name.
#[derive(Debug, PartialEq, Eq)]
struct FileId {
    volume: u32,
    index: u64,
}

impl FileId {
    fn of(path: &Path) -> Option<Self> {
        use windows_sys::Win32::Storage::FileSystem::{
            CreateFileW, GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        };
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: the path ends with a nul; the handle is checked and closed once; the
        // information is written by the call
        unsafe {
            let file = CreateFileW(
                wide.as_ptr(),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS,
                std::ptr::null_mut(),
            );
            if file == INVALID_HANDLE_VALUE {
                return None;
            }
            let mut information: BY_HANDLE_FILE_INFORMATION = std::mem::zeroed();
            let read = GetFileInformationByHandle(file, &mut information);
            CloseHandle(file);
            (read != 0).then(|| Self {
                volume: information.dwVolumeSerialNumber,
                index: u64::from(information.nFileIndexHigh) << 32 | u64::from(information.nFileIndexLow),
            })
        }
    }
}

/// The file process `pid` runs, if the system tells it.
fn image(pid: u32) -> Option<PathBuf> {
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    let mut path = vec![0u16; 32_768];
    let mut length = path.len() as u32;
    // SAFETY: the handle is checked and closed once; the buffer holds `length` characters
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let read = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, path.as_mut_ptr(), &mut length);
        CloseHandle(process);
        (read != 0).then(|| PathBuf::from(String::from_utf16_lossy(&path[..length as usize])))
    }
}

/// Prisme's entry among the installed applications, written whole: `exe` of version
/// `version`, uninstalled by itself.
pub fn register(identity: &Identity, exe: &Path, version: &str) -> std::io::Result<()> {
    let path = uninstall_key(identity);
    delete_tree(&path);
    let key = Key::create(&path)?;
    let exe_text = exe.display().to_string();
    let folder = exe.parent().map(|f| f.display().to_string()).unwrap_or_default();
    let size_kb = std::fs::metadata(exe).map_or(0, |m| (m.len() / 1024).min(u32::MAX as u64) as u32);
    key.text("DisplayName", &identity.name)?;
    key.text("DisplayVersion", version)?;
    key.text("Publisher", PUBLISHER)?;
    key.text("DisplayIcon", &format!("\"{exe_text}\",0"))?;
    key.text("InstallLocation", &folder)?;
    key.text("UninstallString", &format!("\"{exe_text}\" --uninstall"))?;
    key.text("URLInfoAbout", HOME_PAGE)?;
    key.number("NoModify", 1)?;
    key.number("NoRepair", 1)?;
    key.number("EstimatedSize", size_kb)
}

fn uninstall_key(identity: &Identity) -> String {
    format!(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{}", identity.name)
}

/// Clears what the NSIS installer of the versions before 0.7 left in `folder` and in the
/// registry: its uninstaller and the key where it kept the folder. Its entry among the installed
/// applications is written again by [`register`], its shortcuts are files of the same names.
/// Whether it found its uninstaller. Kept until no installation from before 0.7 is left.
pub fn forget_nsis(identity: &Identity, folder: &Path) -> bool {
    let uninstaller = folder.join("uninstall.exe");
    let found = uninstaller.is_file();
    if found {
        if let Err(e) = std::fs::remove_file(&uninstaller) {
            tracing::warn!("the NSIS uninstaller stays: {e}");
        }
    }
    delete_tree(&format!(r"{NSIS_PUBLISHER_KEY}\{}", identity.name));
    delete_if_empty(NSIS_PUBLISHER_KEY);
    found
}

/// Deletes, a minute after `delay` (the installer it left may still run), what Tauri's updater
/// of the versions before 0.7 left in the temporary folder: a folder
/// `<name>-<version>-updater-<random>` for each update. Kept with [`forget_nsis`].
pub fn clear_updater_leftovers(identity: &Identity, delay: Duration) {
    let name = identity.name.clone();
    let spawned = std::thread::Builder::new().name("prisme-leftovers".into()).spawn(move || {
        std::thread::sleep(delay);
        let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else { return };
        for entry in entries.flatten() {
            let leftover =
                entry.file_name().to_str().is_some_and(|folder| is_updater_leftover(folder, &name));
            if leftover && entry.file_type().is_ok_and(|t| t.is_dir()) {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    });
    if let Err(e) = spawned {
        tracing::warn!("the updates' leftovers stay: {e}");
    }
}

/// Whether `folder` is one Tauri's updater made for application `app` (`make_temp_dir` in
/// tauri-plugin-updater).
fn is_updater_leftover(folder: &str, app: &str) -> bool {
    let Some((version, random)) = folder
        .strip_prefix(app)
        .and_then(|rest| rest.strip_prefix('-'))
        .and_then(|rest| rest.rsplit_once("-updater-"))
    else {
        return false;
    };
    version.starts_with(|c: char| c.is_ascii_digit())
        && version.chars().all(|c| c.is_ascii_alphanumeric() || ".+-".contains(c))
        && random.len() == 6
        && random.chars().all(|c| c.is_ascii_alphanumeric())
}

/// The shortcuts opening `exe`: in the Start menu, and on the desktop if `desktop`. The ones
/// there of the same name are written again.
pub fn shortcuts(identity: &Identity, exe: &Path, desktop: bool) -> windows::core::Result<()> {
    let _com = Com::start();
    shortcut(&known_folder(Folder::StartMenu)?.join(link_name(identity)), identity, exe)?;
    if desktop {
        shortcut(&known_folder(Folder::Desktop)?.join(link_name(identity)), identity, exe)?;
    }
    Ok(())
}

fn link_name(identity: &Identity) -> String {
    format!("{}.lnk", identity.name)
}

/// Deletes the shortcuts [`shortcuts`] made.
fn remove_shortcuts(identity: &Identity) {
    for folder in [Folder::StartMenu, Folder::Desktop] {
        if let Ok(folder) = known_folder(folder) {
            let _ = std::fs::remove_file(folder.join(link_name(identity)));
        }
    }
}

fn shortcut(link: &Path, identity: &Identity, exe: &Path) -> windows::core::Result<()> {
    use windows::core::{Interface, HSTRING};
    use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
    use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
    use windows::Win32::System::Com::{CoCreateInstance, IPersistFile, CLSCTX_INPROC_SERVER};
    use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    let folder = exe.parent().unwrap_or(exe);
    // SAFETY: COM is started on this thread for the call; the strings outlive their use
    unsafe {
        let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        shell_link.SetPath(&HSTRING::from(exe.as_os_str()))?;
        shell_link.SetWorkingDirectory(&HSTRING::from(folder.as_os_str()))?;
        shell_link.SetIconLocation(&HSTRING::from(exe.as_os_str()), 0)?;
        shell_link.SetDescription(&HSTRING::from(identity.name.as_str()))?;
        let store: IPropertyStore = shell_link.cast()?;
        store.SetValue(&PKEY_AppUserModel_ID, &PROPVARIANT::from(identity.app_id().as_str()))?;
        store.Commit()?;
        shell_link.cast::<IPersistFile>()?.Save(&HSTRING::from(link.as_os_str()), true)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Folder {
    StartMenu,
    Desktop,
}

/// The user's folder `folder`, wherever it was moved (a desktop kept in OneDrive).
fn known_folder(folder: Folder) -> windows::core::Result<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{
        FOLDERID_Desktop, FOLDERID_Programs, SHGetKnownFolderPath, KF_FLAG_DEFAULT,
    };
    let id = match folder {
        Folder::StartMenu => FOLDERID_Programs,
        Folder::Desktop => FOLDERID_Desktop,
    };
    // SAFETY: the path the system gives is read, then freed once
    unsafe {
        let path = SHGetKnownFolderPath(&id, KF_FLAG_DEFAULT, None)?;
        let read = path.to_string();
        CoTaskMemFree(Some(path.0.cast()));
        Ok(PathBuf::from(read.map_err(|_| windows::core::Error::empty())?))
    }
}

/// COM started on the current thread while it lives.
struct Com(bool);

impl Com {
    fn start() -> Self {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
        // SAFETY: undone on drop when it succeeded
        Self(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: started by `start` on this thread
            unsafe { windows::Win32::System::Com::CoUninitialize() };
        }
    }
}

/// Prisme's own uninstallation, as the installed applications' list starts it
/// (`--uninstall`), asked first; the library, the settings and the recordings are kept. A
/// program cannot delete its own file while it runs: a copy of it in the temporary folder ends
/// the job once this one quit (`--uninstall-finish`). The exit code to end with.
pub fn uninstall(identity: &Identity) -> i32 {
    let texts = Texts::of_the_system();
    // only the installation it runs from: a copy anywhere else uninstalls nothing
    let Some(folder) = installed_folder(identity) else {
        message(texts.not_installed);
        return 1;
    };
    if runs_from(&folder) {
        message(texts.close_first);
        return 1;
    }
    if !message_box(&texts.question.replace("{name}", &identity.name), Message::Question) {
        return 1;
    }
    remove_shortcuts(identity);
    delete_tree(&uninstall_key(identity));
    forget_nsis(identity, &folder);
    let finisher =
        std::env::temp_dir().join(format!("{}-uninstall-{}.exe", identity.name, std::process::id()));
    let copied = std::env::current_exe().and_then(|exe| std::fs::copy(exe, &finisher));
    let started = copied.and_then(|_| {
        Command::new(&finisher)
            .args([FINISH, &std::process::id().to_string()])
            .current_dir(std::env::temp_dir())
            .spawn()
    });
    match started {
        Ok(_) => 0,
        Err(e) => {
            message(&format!("{}\n\n{e}", texts.failed));
            1
        }
    }
}

/// The folder of the installation this process runs from, if it runs from one.
fn installed_folder(identity: &Identity) -> Option<PathBuf> {
    match crate::Install::of_this_process(identity) {
        crate::Install::Windows { exe } => exe.parent().map(Path::to_owned),
        _ => None,
    }
}

/// The argument starting the end of an uninstallation, followed by the process it waits for.
const FINISH: &str = "--uninstall-finish";

/// The end of an uninstallation, from a copy of Prisme in the temporary folder: once process
/// `pid` quit, Prisme's folder is deleted, then this copy itself.
fn finish_uninstall(identity: &Identity, pid: u32) -> i32 {
    let texts = Texts::of_the_system();
    wait_for_exit(pid, Duration::from_secs(30));
    if let Some(folder) = identity.folder() {
        for _ in 0..20 {
            if !folder.exists() || std::fs::remove_dir_all(&folder).is_ok() {
                break;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
    message_box(&texts.done.replace("{name}", &identity.name), Message::Information);
    if let Ok(me) = std::env::current_exe() {
        delete_soon(&me);
    }
    0
}

/// Deletes `file` two seconds from now, by a program of the system's: the program running from
/// it is gone by then.
fn delete_soon(file: &Path) {
    // given to cmd as it is: Rust's quoting of arguments is not cmd's
    let delete = format!("/C ping -n 3 127.0.0.1 >NUL & del /F /Q \"{}\"", file.display());
    let _ = Command::new("cmd.exe").raw_arg(delete).creation_flags(NO_WINDOW).spawn();
}

/// What the arguments ask of Prisme before anything starts: its uninstallation, the exit code
/// to end with.
pub(crate) fn on_arguments(identity: &Identity, arguments: &[String]) -> Option<i32> {
    match arguments {
        [flag] if flag == "--uninstall" => Some(uninstall(identity)),
        [flag, pid] if flag == FINISH => Some(pid.parse().map_or(1, |pid| finish_uninstall(identity, pid))),
        _ => None,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Message {
    Question,
    Warning,
    Information,
}

/// Shows `text` in a message box of Prisme's, as a warning.
pub fn message(text: &str) {
    message_box(text, Message::Warning);
}

/// Shows `text` in a message box of Prisme's: whether the DJ answered yes, to a question.
fn message_box(text: &str, kind: Message) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDYES, MB_ICONINFORMATION, MB_ICONQUESTION, MB_ICONWARNING, MB_OK, MB_SETFOREGROUND,
        MB_YESNO,
    };
    let style = match kind {
        Message::Question => MB_YESNO | MB_ICONQUESTION,
        Message::Warning => MB_OK | MB_ICONWARNING,
        Message::Information => MB_OK | MB_ICONINFORMATION,
    };
    let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (text, title) = (wide(text), wide("Prisme"));
    // SAFETY: both strings end with a nul and outlive the call
    let answer =
        unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), style | MB_SETFOREGROUND) };
    answer == IDYES
}

/// What the uninstallation says, in French on a French system, else in English.
struct Texts {
    question: &'static str,
    not_installed: &'static str,
    close_first: &'static str,
    failed: &'static str,
    done: &'static str,
}

impl Texts {
    fn of_the_system() -> Self {
        if french() {
            Self {
                question: "Désinstaller {name} ?\n\nVotre bibliothèque, vos cues, vos réglages et vos enregistrements sont gardés.",
                not_installed: "Cette copie de Prisme n’est pas installée : il n’y a rien à désinstaller.",
                close_first: "Prisme est ouvert : fermez-le pour le désinstaller.",
                failed: "La désinstallation n’a pas pu se terminer.",
                done: "{name} est désinstallé.",
            }
        } else {
            Self {
                question: "Uninstall {name}?\n\nYour library, cues, settings and recordings are kept.",
                not_installed: "This copy of Prisme is not installed: there is nothing to uninstall.",
                close_first: "Prisme is open: close it to uninstall it.",
                failed: "The uninstallation could not finish.",
                done: "{name} is uninstalled.",
            }
        }
    }
}

/// Whether the system speaks French.
pub fn french() -> bool {
    const FRENCH: u16 = 0x0c;
    // SAFETY: no argument, no state
    let language = unsafe { windows_sys::Win32::Globalization::GetUserDefaultUILanguage() };
    language & 0x3ff == FRENCH
}

/// A key of the current user's registry, open for writing.
struct Key(HKEY);

impl Key {
    fn create(path: &str) -> std::io::Result<Self> {
        let path = wide(path);
        let mut key = std::ptr::null_mut();
        // SAFETY: the path ends with a nul, the key is written by the call
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                path.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            )
        };
        checked(status).map(|()| Self(key))
    }

    fn text(&self, name: &str, value: &str) -> std::io::Result<()> {
        let (name, value) = (wide(name), wide(value));
        // SAFETY: the key is open, the value holds its bytes with its nul
        checked(unsafe {
            RegSetValueExW(self.0, name.as_ptr(), 0, REG_SZ, value.as_ptr().cast(), (value.len() * 2) as u32)
        })
    }

    fn number(&self, name: &str, value: u32) -> std::io::Result<()> {
        let name = wide(name);
        // SAFETY: the key is open, the value is 4 bytes read during the call
        checked(unsafe { RegSetValueExW(self.0, name.as_ptr(), 0, REG_DWORD, (&raw const value).cast(), 4) })
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: opened by `create`, closed once
        unsafe { RegCloseKey(self.0) };
    }
}

/// Deletes the current user's key at `path` with all it holds, if it is there.
fn delete_tree(path: &str) {
    let path = wide(path);
    // SAFETY: the path ends with a nul
    unsafe {
        RegDeleteTreeW(HKEY_CURRENT_USER, path.as_ptr());
        RegDeleteKeyW(HKEY_CURRENT_USER, path.as_ptr());
    }
}

/// Deletes the current user's key at `path` if no key is left under it.
fn delete_if_empty(path: &str) {
    let path = wide(path);
    // SAFETY: the path ends with a nul; the system refuses a key with keys under it
    unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, path.as_ptr()) };
}

fn checked(status: u32) -> std::io::Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(std::io::Error::from_raw_os_error(status as i32))
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EXE;

    #[test]
    fn only_the_folders_of_tauris_updater_are_its_leftovers() {
        assert!(is_updater_leftover("Prisme-0.3.9-updater-KVjD3P", "Prisme"));
        assert!(is_updater_leftover("Prisme-0.4.0-beta.1-updater-4xtegx", "Prisme"));
        for folder in [
            "Ramure-0.1.6-updater-VkD7ZE",
            "Prismes-0.3.9-updater-KVjD3P",
            "Prisme-updater-KVjD3P",
            "Prisme--updater-KVjD3P",
            "Prisme-0.3.9-updater-",
            "Prisme-0.3.9-updater-KVjD3P7",
            "Prisme-0.3.9-updater-KVj.3P",
            "Prisme-0.3.9-installer.exe",
            "Prisme",
        ] {
            assert!(!is_updater_leftover(folder, "Prisme"), "{folder}");
        }
    }

    #[test]
    fn a_program_running_from_a_folder_is_found_but_this_one() {
        let exe = std::env::current_exe().unwrap();
        assert!(!runs_from(exe.parent().unwrap()), "this process is left out");
        let folder = tempfile::tempdir().unwrap();
        let copy = folder.path().join(EXE);
        std::fs::copy(std::env::var("COMSPEC").unwrap(), &copy).unwrap();
        assert!(!runs_from(folder.path()));
        let mut other = Command::new(&copy).args(["/C", "ping -n 30 127.0.0.1 >NUL"]).spawn().unwrap();
        let upper = folder.path().to_string_lossy().to_uppercase();
        assert!(runs_from(Path::new(&upper)));
        assert!(runs_from(&folder.path().join(".")), "whatever path reaches it");
        other.kill().unwrap();
        other.wait().unwrap();
        assert!(!runs_from(folder.path()));
    }

    /// Registered, then uninstalled by hand, under a name of the test's own.
    #[test]
    fn the_entry_and_the_nsis_leftovers_are_written_and_cleared_in_the_registry() {
        let identity = Identity { name: format!("PrismeTest{}", std::process::id()) };
        let folder = tempfile::tempdir().unwrap();
        let exe = folder.path().join(EXE);
        std::fs::write(&exe, vec![0; 4096]).unwrap();
        register(&identity, &exe, "9.9.9").unwrap();
        assert_eq!(read(&uninstall_key(&identity), "DisplayVersion").as_deref(), Some("9.9.9"));
        let uninstall = read(&uninstall_key(&identity), "UninstallString").unwrap();
        assert!(uninstall.ends_with("\" --uninstall"), "{uninstall}");

        let nsis = format!(r"{NSIS_PUBLISHER_KEY}\{}", identity.name);
        Key::create(&nsis).unwrap().text("", "C:\\somewhere").unwrap();
        std::fs::write(folder.path().join("uninstall.exe"), b"").unwrap();
        assert!(forget_nsis(&identity, folder.path()));
        assert!(!folder.path().join("uninstall.exe").exists());
        assert_eq!(read(&nsis, ""), None);
        assert!(!forget_nsis(&identity, folder.path()));

        delete_tree(&uninstall_key(&identity));
        assert_eq!(read(&uninstall_key(&identity), "DisplayVersion"), None);
    }

    fn read(key: &str, name: &str) -> Option<String> {
        use windows_sys::Win32::System::Registry::{RegGetValueW, RRF_RT_REG_SZ};
        let (key, name) = (wide(key), wide(name));
        let mut buffer = vec![0u16; 1024];
        let mut bytes = (buffer.len() * 2) as u32;
        // SAFETY: the strings end with a nul, the buffer holds `bytes` bytes
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        (status == 0).then(|| String::from_utf16_lossy(&buffer[..(bytes as usize / 2).saturating_sub(1)]))
    }

    #[test]
    fn a_file_is_deleted_soon_after() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("Prisme Test-uninstall.exe");
        std::fs::write(&file, b"").unwrap();
        delete_soon(&file);
        assert!(file.exists());
        std::thread::sleep(Duration::from_secs(5));
        assert!(!file.exists());
    }

    #[test]
    fn a_shortcut_opens_prisme_under_its_own_id() {
        let _com = Com::start();
        let folder = tempfile::tempdir().unwrap();
        let link = folder.path().join("Prisme Test.lnk");
        let identity = Identity { name: "PrismeTest".into() };
        let exe = std::env::current_exe().unwrap();
        shortcut(&link, &identity, &exe).unwrap();
        assert!(std::fs::metadata(&link).unwrap().len() > 0);
        assert!(known_folder(Folder::StartMenu).unwrap().is_dir());
    }
}
