//! Controllers, MIDI and HID (see `controllers`): what the interface shows of them comes with
//! its section (`ControllersStateDto`), shown again after each command that changes it. Those
//! that may open a MIDI port run off the window's thread: Android hands a port over on that
//! thread, which would wait on itself.

use crate::controller_profiles;
use crate::controllers::Controllers;
use crate::error::{AppError, AppResult};
use crate::midi_clock::MidiClock;
use crate::picked;
use crate::state::AppState;
use prisme_control::Encoding;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_fs::FilePath;

/// Runs `change` on the controllers, then shows them.
fn changing<R>(app: &AppHandle, change: impl FnOnce(&Controllers) -> R) -> R {
    let controllers = app.state::<Controllers>();
    let changed = change(&controllers);
    controllers.show(&app.state::<AppState>());
    changed
}

/// Looks for the controllers plugged in now (they are looked for twice a second besides).
#[tauri::command(async)]
pub fn look_for_controllers(state: State<'_, AppState>, controllers: State<'_, Controllers>) {
    controllers.look(&state);
}

#[tauri::command(async)]
pub fn set_controller_enabled(app: AppHandle, name: String, enabled: bool) -> AppResult<()> {
    changing(&app, |c| c.set_enabled(&app, &name, enabled))
}

/// Drives the controller `name` by `file`, one of the profiles offered for it (a variant of its
/// model's), from now on.
#[tauri::command(async)]
pub fn choose_controller_profile(app: AppHandle, name: String, file: String) -> AppResult<()> {
    changing(&app, |c| c.choose_profile(&app, &name, &file))
}

/// The models of the controllers in use that another program holds.
#[tauri::command]
pub fn waiting_controllers(controllers: State<'_, Controllers>) -> Vec<String> {
    controllers.waiting()
}

/// The DJ chose not to use the controller just plugged in: it is not offered again.
#[tauri::command]
pub fn decline_controller(app: AppHandle, name: String) {
    changing(&app, |c| c.decline(&name));
}

/// The next control moved on a controller in use will drive `target`, read as a key, a fader
/// or an encoder as its action takes.
#[tauri::command]
pub fn learn_control(app: AppHandle, target: String) {
    changing(&app, |c| c.learn(target));
}

/// How the encoder of `target` on `device` sends its steps (a jog that turns the wrong way
/// or by leaps is read with another encoding).
#[tauri::command]
pub fn set_control_encoding(app: AppHandle, device: String, target: String, encoding: Encoding) {
    changing(&app, |c| c.set_encoding(&device, &target, encoding));
}

#[tauri::command]
pub fn cancel_learning(app: AppHandle) {
    changing(&app, Controllers::cancel_learning);
}

/// Takes `target` off the controller `device`.
#[tauri::command]
pub fn forget_control(app: AppHandle, device: String, target: String) {
    changing(&app, |c| c.forget(&device, &target));
}

/// The MIDI outputs the clock can go to.
#[tauri::command]
pub fn midi_outputs() -> AppResult<Vec<String>> {
    MidiClock::outputs()
}

/// Sends the MIDI clock to `name` (`None`: nowhere); kept for the next start.
#[tauri::command(async)]
pub fn set_midi_clock_output(app: AppHandle, name: Option<String>) -> AppResult<()> {
    app.state::<MidiClock>().set_output(name.as_deref())?;
    changing(&app, |c| c.remember_clock_output(name));
    Ok(())
}

/// Reads the profiles again: the DJ's own changed.
#[tauri::command]
pub fn reload_controller_profiles(app: AppHandle) {
    controller_profiles::reload();
    changing(&app, Controllers::profiles_changed);
}

/// Fetches the profiles published since: how many came.
#[tauri::command]
pub async fn update_controller_profiles(app: AppHandle) -> AppResult<usize> {
    let came = controller_profiles::update().await.map_err(AppError::Other)?;
    changing(&app, Controllers::profiles_changed);
    Ok(came)
}

/// Writes what `device` drives now as a profile, to the file the DJ picked, and says what the
/// DJ knows that file by: what the controller drives does not change.
#[tauri::command(async)]
pub fn export_controller_profile(
    app: AppHandle,
    controllers: State<'_, Controllers>,
    device: String,
    path: FilePath,
) -> AppResult<String> {
    let profile = controllers.as_profile(&device);
    controller_profiles::export(&profile, picked::create(&app, path.clone())?).map_err(AppError::Other)?;
    Ok(picked::name(&path))
}

/// Shows the folder of the DJ's own profiles in the system's file manager.
#[tauri::command]
pub fn open_controller_profiles_folder() -> AppResult<()> {
    let folder = controller_profiles::own_folder().ok_or_else(|| AppError::Other("no data folder".into()))?;
    std::fs::create_dir_all(&folder)?;
    Ok(crate::folders::show(&folder)?)
}
