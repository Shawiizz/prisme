//! The library as the interface browses it: each gesture a command, sent in turn with its other
//! actions (`order`) and done on the library's lane; the list shown comes back with the status
//! stream, the window of rows a scroll asks for at once (`library_viewport`).

use crate::actions::browse::{self, effective, Library};
use crate::actions::{Actions, Order};
use crate::commands::blocking;
use crate::dto::{
    ColumnDto, LibraryMenuDto, LibraryViewDto, LibraryWindowDto, NamedTrackDto, PickDto, ScopeDto,
    SeparateDto, SourceDto,
};
use crate::error::{AppError, AppResult};
use crate::library_service::LibraryService;
use crate::state::{lock, AppState};
use crate::stems_service::StemsService;
use prisme_actions::lane::Lane;
use prisme_library::browse::Column;
use prisme_library::{within, LibraryDb};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use tauri::{AppHandle, Manager, State};

/// Most tracks the command palette is given for a search.
const SEARCH_LIMIT: usize = 500;

/// Does `act` on the library, sent in turn with the interface's other actions (`order`) and done
/// in turn with the library's on their lane: a list opened may read a folder, which the actions
/// of the decks sent after it do not wait for, and a load does (see `prisme_actions::lane`).
pub(crate) async fn in_turn<R: Send + 'static>(
    app: AppHandle,
    order: Order,
    act: impl FnOnce(&AppState, &LibraryDb) -> AppResult<R> + Send + 'static,
) -> AppResult<R> {
    blocking(move || {
        let actions = app.state::<Actions>();
        let (answer, outcome) = mpsc::sync_channel(1);
        let doing = app.clone();
        let job = move || {
            let (state, library) = (doing.state::<AppState>(), doing.state::<Arc<LibraryService>>());
            drop(answer.send(act(&state, &library.db)));
        };
        actions.in_order(order, || actions.in_turn_on(Lane::Library, job));
        outcome.recv().unwrap_or_else(|e| Err(AppError::Other(e.to_string())))
    })
    .await
}

/// Looks for new or changed files in the music folder and analyses them, shown as a rescan
/// (see `FoldersStateDto::rescanning`). Off the main thread: the analysis running stops first.
#[tauri::command]
pub async fn rescan_library(app: AppHandle) -> AppResult<()> {
    blocking(move || {
        app.state::<Arc<LibraryService>>().rescan();
        Ok(())
    })
    .await
}

/// Shows `source`: the whole list, in its own order if it has one.
#[tauri::command]
pub async fn library_open(app: AppHandle, source: SourceDto, order: Order) -> AppResult<()> {
    in_turn(app, order, move |state, db| browse::open(state, db, source.into())).await
}

/// Shows the folder above the one shown, inside its music folder.
#[tauri::command]
pub async fn library_up(app: AppHandle, order: Order) -> AppResult<()> {
    in_turn(app, order, browse::up).await
}

/// Searches the list shown (or the whole library) for every word of `text`: the list as it is
/// then, at once.
#[tauri::command]
pub async fn library_filter(app: AppHandle, text: String, order: Order) -> AppResult<LibraryViewDto> {
    in_turn(app, order, move |state, _| {
        state.browse(|l| l.browser.set_filter(&text));
        Ok(browse::shown(state, Library::view))
    })
    .await
}

/// Searches the list browsed, or the whole library.
#[tauri::command]
pub async fn library_scope(app: AppHandle, scope: ScopeDto, order: Order) -> AppResult<()> {
    in_turn(app, order, move |state, _| {
        state.browse(|l| l.browser.set_scope(scope.into()));
        Ok(())
    })
    .await
}

/// Sorts the rows by `column`, the other way if they already are; by how well they follow the
/// deck they are compared with as it sounds now.
#[tauri::command]
pub async fn library_sort(app: AppHandle, column: ColumnDto, order: Order) -> AppResult<()> {
    let view = app.state::<Actions>().view(&app);
    in_turn(app, order, move |state, _| {
        let column = Column::from(column);
        let mix = (column == Column::Mix).then(|| {
            let deck = effective(lock(&state.library).browser.reference(), view.decks_in_use);
            let path = lock(&state.tracks)[deck].as_ref().map(|t| t.path.clone());
            browse::mix_reference(&view, deck, path.as_deref())
        });
        state.browse(|l| l.browser.sort_by(column, mix));
        Ok(())
    })
    .await
}

/// Picks the row of the track at `path` (played at `played_at_ms`, in a set) as `pick` says.
#[tauri::command]
pub async fn library_select(
    app: AppHandle,
    path: String,
    played_at_ms: Option<i64>,
    pick: PickDto,
    order: Order,
) -> AppResult<()> {
    in_turn(app, order, move |state, _| {
        browse::select(state, &path, played_at_ms, pick.into());
        Ok(())
    })
    .await
}

/// Compares the tracks with deck `deck`: the keys that mix with it, the pitch to its tempo.
#[tauri::command]
pub async fn library_compare_with(app: AppHandle, deck: usize, order: Order) -> AppResult<()> {
    in_turn(app, order, move |state, _| {
        state.browse(|l| l.browser.compare_with(deck));
        Ok(())
    })
    .await
}

/// The rows the interface shows, `count` from the `first`: at once, and again with the status
/// stream as they change. Off the main thread: a track shown first is looked for its stems.
#[tauri::command(async)]
pub fn library_viewport(state: State<'_, AppState>, first: usize, count: usize) -> LibraryWindowDto {
    lock(&state.library).browser.set_window(first, count);
    browse::shown(&state, Library::window)
}

/// What the menu of the tracks at `paths` (else of those selected) can do with them.
#[tauri::command]
pub async fn library_menu(
    app: AppHandle,
    paths: Option<Vec<String>>,
    order: Order,
) -> AppResult<LibraryMenuDto> {
    let stems = app.clone();
    in_turn(app, order, move |state, _| Ok(menu(state, &stems.state::<StemsService>(), paths))).await
}

fn menu(state: &AppState, stems: &StemsService, paths: Option<Vec<String>>) -> LibraryMenuDto {
    let paths = browse::tracks(state, paths);
    let library = lock(&state.library);
    let browser = &library.browser;
    let catalogue = browser.catalogue();
    let tracks: Vec<_> = paths.iter().filter_map(|p| catalogue.find(p)).map(|id| catalogue.get(id)).collect();
    let elsewhere = browser.elsewhere_among(&paths).len();
    let folders: Vec<&Path> = paths.iter().filter_map(|p| Path::new(p).parent()).collect();
    let shared = folders.first().filter(|first| folders.iter().all(|f| f == *first)).copied();
    let folder = shared.filter(|_| elsewhere == paths.len() && elsewhere > 0);
    let holds = folder.and_then(|folder| browser.roots().iter().find(|root| within(root, folder)));
    let genres: Vec<&str> = tracks
        .iter()
        .map(|t| t.record.as_ref().and_then(|r| r.tags.genre.as_deref()).unwrap_or(""))
        .collect();
    let genre = match genres.first() {
        Some(genre) if genres.iter().all(|g| g == genre) && !genre.is_empty() => genre.to_string(),
        _ => shared
            .and_then(Path::file_name)
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
    };
    let readable: Vec<PathBuf> =
        tracks.iter().filter(|t| t.analysed()).map(|t| PathBuf::from(&t.path)).collect();
    LibraryMenuDto {
        count: paths.len(),
        first: tracks.first().map(|t| NamedTrackDto::of(t)),
        removable: browser.removable().then(|| SourceDto::from(browser.source())),
        elsewhere,
        folder: folder.map(|f| f.display().to_string()),
        holds: holds.map(|root| root.display().to_string()),
        genre,
        separable: stems.separable(readable).len(),
    }
}

/// Separates those of the tracks at `paths` (else of those selected) the installed models have
/// stems to add to, analysed and not queued already; without a model, offers the models for
/// those analysed.
#[tauri::command]
pub async fn separate_tracks(
    app: AppHandle,
    paths: Option<Vec<String>>,
    order: Order,
) -> AppResult<SeparateDto> {
    let stems = app.clone();
    in_turn(app, order, move |state, _| {
        let paths = browse::tracks(state, paths);
        let readable = {
            let library = lock(&state.library);
            let catalogue = library.browser.catalogue();
            let analysed =
                |path: &&String| catalogue.find(path).is_some_and(|id| catalogue.get(id).analysed());
            paths.iter().filter(analysed).map(PathBuf::from).collect()
        };
        let stems = stems.state::<StemsService>();
        stems.separate(stems.separable(readable))
    })
    .await
}

/// Takes the tracks at `paths` (else those selected) out of the list shown, the prepare list or
/// a crate filled by hand.
#[tauri::command]
pub async fn library_remove(app: AppHandle, paths: Option<Vec<String>>, order: Order) -> AppResult<()> {
    in_turn(app, order, move |state, db| browse::remove_from_list(state, db, paths)).await
}

/// Takes the tracks from elsewhere among `paths` (else among those selected) out of the
/// library, their files left as they are: their cues, corrections and crate entries are kept
/// for when the same audio comes back.
#[tauri::command]
pub async fn remove_tracks(app: AppHandle, paths: Option<Vec<String>>, order: Order) -> AppResult<()> {
    in_turn(app, order, move |state, db| browse::remove_from_library(state, db, paths)).await
}

/// Tracks of the whole library a search for `query` finds, by the rule of the lists' searches:
/// the command palette's.
#[tauri::command(async)]
pub fn search_library(state: State<'_, AppState>, query: String) -> Vec<NamedTrackDto> {
    lock(&state.library).browser.search(&query, SEARCH_LIMIT).map(NamedTrackDto::of).collect()
}

/// The genres of the library, the most common first (suggestions when tagging).
#[tauri::command(async)]
pub fn library_genres(library: State<'_, Arc<LibraryService>>) -> AppResult<Vec<String>> {
    Ok(library.db.genres()?)
}
