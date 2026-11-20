import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import {
  AnalysisProgress,
  DirListing,
  HistorySession,
  LibraryLists,
  LibraryMenu,
  LibraryRow,
  LibraryView,
  LibraryWindow,
  MusicFolder,
  Pick,
  SearchScope,
  SmartRules,
  SortColumn,
  StoragePlace,
  TrackSource,
} from '../models/library';
import { EngineService } from './engine.service';
import { ExportService, ListRef } from './export.service';
import { HistoryService } from './history.service';
import { errorText } from '../errors';
import { folderName } from '../folders';
import { t } from '../i18n/i18n';
import { ConfirmService } from '../../shared/confirm/confirm';
import { same } from '../same';

const NO_LISTS: LibraryLists = { crates: [], sessions: [], prepare: 0, next: null, elsewhere: 0 };
const NO_WINDOW: LibraryWindow = { seq: -1, first: 0, rows: [] };

/**
 * The library as the backend browses it: the list shown, its search, its sort and the rows
 * picked live in the backend, which tells them with the status (`controls.library`) and gives
 * the rows the table draws as it scrolls (`viewport`); each gesture here is a command sent in
 * turn with the app's actions. The lists of the side panel come the same way
 * (`controls.lists`), and the music folders with how far their analysis is (`controls.folders`).
 */
@Injectable({ providedIn: 'root' })
export class LibraryService {
  private readonly engine = inject(EngineService);
  private readonly history = inject(HistoryService);
  private readonly exports = inject(ExportService);
  private readonly confirm = inject(ConfirmService);

  private readonly told = computed(() => this.engine.controls()?.folders ?? null);
  /** The folders whose tracks make the library, the main one first. */
  readonly folders = computed(() => this.told()?.folders ?? []);
  /** The main music folder. */
  readonly root = computed(() => this.folders().find((f) => f.main)?.path ?? null);
  readonly error = signal<string | null>(null);
  readonly progress = computed<AnalysisProgress>(
    () => this.told()?.progress ?? { total: 0, done: 0, failed: 0, running: false, paused: false },
  );
  /** The run of the analysis `progress` is of. */
  readonly scan = computed(() => this.told()?.scan ?? 0);
  /** The library's database could not be opened: what changes is lost at exit. */
  readonly inMemory = computed(() => this.told()?.inMemory ?? false);
  /** The genres of the library, the most common first (loaded on demand). */
  readonly genres = signal<string[]>([]);
  /** Incremented to ask the view to focus the search field. */
  readonly searchRequests = signal(0);

  /** The list browsed, as the backend told it last. */
  readonly view = signal<LibraryView | null>(null);
  /** The rows the table draws, as the backend gave them last. */
  readonly window = signal<LibraryWindow>(NO_WINDOW);
  /** The crates, the sets, the prepare list and the tracks from elsewhere, as the side panel
   * shows them. */
  readonly lists = computed(() => this.engine.controls()?.lists ?? NO_LISTS, { equal: same });
  /** What is typed in the search while it has the focus: shown rather than the search applied,
   * which follows a keystroke behind. */
  private readonly typing = signal<string | null>(null);
  readonly filter = computed(() => this.typing() ?? this.view()?.filter ?? '');

  readonly source = computed(() => this.view()?.source ?? null, { equal: same });
  readonly scope = computed<SearchScope>(() => this.view()?.scope ?? 'list');
  readonly sort = computed(() => this.view()?.sort ?? { column: 'title', descending: false }, {
    equal: same,
  });
  readonly count = computed(() => this.view()?.count ?? 0);
  /** Whether the list has an order of its own (shown in a `#` column). */
  readonly ordered = computed(() => this.view()?.ordered ?? false);
  /** Whether the selection can be taken out of the list shown. */
  readonly removable = computed(() => this.view()?.removable ?? false);
  /** Whether the rows can be dragged to another place: the prepare list in its order. */
  readonly reorderable = computed(() => this.view()?.reorderable ?? false);
  /** The deck the tracks are compared with: compatible keys, pitch to match. */
  readonly reference = computed(() => this.view()?.reference ?? 0);
  readonly crates = computed(() => this.lists().crates);
  readonly sessions = computed(() => this.lists().sessions);
  /** The crate being browsed, if any. */
  readonly currentCrate = computed(() => {
    const source = this.source();
    return source?.kind === 'crate' ? source.id : null;
  });
  /** The history session being browsed, if any. */
  readonly currentSession = computed(() => {
    const source = this.source();
    return source?.kind === 'history' ? source.id : null;
  });
  /** The folder being browsed, if any. */
  readonly currentFolder = computed(() => this.view()?.folder ?? null, { equal: same });

  constructor() {
    effect(() => {
      const view = this.engine.controls()?.library;
      if (view) untracked(() => this.take(view));
    });
  }

  /** The crate `id`, if there is one. */
  crate(id: number) {
    return this.crates().find((c) => c.id === id);
  }

  /** Shows `source`; `false` if it cannot be read (a folder gone). */
  open(source: TrackSource): Promise<boolean> {
    return this.attempt(() => this.engine.ordered('library_open', { source }));
  }

  openFolder(path: string): Promise<boolean> {
    return this.open({ kind: 'folder', path });
  }

  /** The folder above the one shown, inside its music folder. */
  up(): Promise<boolean> {
    return this.attempt(() => this.engine.ordered('library_up'));
  }

  /** Searches for what is typed, at each key: the list comes back searched at once. */
  async setFilter(text: string) {
    this.typing.set(text);
    try {
      this.take(await this.engine.ordered<LibraryView>('library_filter', { text }));
    } catch (e) {
      this.error.set(errorText(e));
    }
  }

  /** The search lost the focus: it shows the search applied. */
  endTyping() {
    this.typing.set(null);
  }

  setScope(scope: SearchScope) {
    return this.attempt(() => this.engine.ordered('library_scope', { scope }));
  }

  /** Click on a column title: sort by it, again to reverse. */
  sortBy(column: SortColumn) {
    return this.attempt(() => this.engine.ordered('library_sort', { column }));
  }

  /** Picks `row` as `pick` says: alone, added or taken out (Ctrl), up to it (Shift), or alone
   * unless it is in the selection (a right click, a drag). */
  select(row: LibraryRow, pick: Pick) {
    const { path, playedAtMs } = row;
    return this.attempt(() => this.engine.ordered('library_select', { path, playedAtMs, pick }));
  }

  clearSelection() {
    return this.engine.perform('library.clearSelection', 1);
  }

  compareWith(deck: number) {
    return this.attempt(() => this.engine.ordered('library_compare_with', { deck }));
  }

  /** The table shows `count` rows from the `first`: they come at once, and again as they change. */
  async viewport(first: number, count: number) {
    this.takeWindow(await invoke<LibraryWindow>('library_viewport', { first, count }));
  }

  /** Loads `path` on `deck`, or on the first deck without a track (`null`). Whether it was. */
  load(path: string, deck: number | null): Promise<boolean> {
    return this.engine.loadTrack(path, deck);
  }

  /** What the menu of the tracks at `paths` (`null`: those selected) can do with them. */
  menu(paths: string[] | null): Promise<LibraryMenu> {
    return this.engine.ordered<LibraryMenu>('library_menu', { paths });
  }

  /** Puts the tracks at `paths` (`null`: those selected) at the end of the prepare list. */
  addToPrepare(paths: string[] | null) {
    return this.attempt(() => this.engine.ordered('add_to_prepare', { paths }));
  }

  /** Puts the tracks at `paths` (`null`: those selected) before `before` in the prepare list
   * (`null`: at its end), adding them if needed. */
  placeInPrepare(paths: string[] | null, before: string | null) {
    return this.attempt(() => this.engine.ordered('place_in_prepare', { paths, before }));
  }

  addToCrate(id: number, paths: string[] | null) {
    return this.attempt(() => this.engine.ordered('add_to_crate', { id, paths }));
  }

  /** Takes the tracks at `paths` (`null`: those selected) out of the crate or prepare list shown. */
  remove(paths: string[] | null) {
    return this.attempt(() => this.engine.ordered('library_remove', { paths }));
  }

  /** Takes the tracks from elsewhere among `paths` (`null`: those selected) out of the library,
   * once the DJ says so; `menu` says how many and which. The files stay where they are. */
  async removeFromPrisme(paths: string[] | null, menu: LibraryMenu): Promise<void> {
    if (!menu.elsewhere) return;
    const one = menu.elsewhere === 1;
    const yes = await this.confirm.ask({
      title: one
        ? t('Remove “{title}” from Prisme?', { title: menu.first?.title ?? '' })
        : t('Remove these {n} tracks from Prisme?', { n: menu.elsewhere }),
      detail: one
        ? t('It leaves the library. The file stays where it is.')
        : t('They leave the library. The files stay where they are.'),
      confirm: t('Remove'),
      cancel: t('Cancel'),
    });
    if (yes) await this.attempt(() => this.engine.ordered('remove_tracks', { paths }));
  }

  /** Colours the tracks at `paths` (`null`: those selected); `null` takes the colour off. */
  setColor(paths: string[] | null, color: number | null) {
    return this.attempt(() => this.engine.ordered('set_track_color', { paths, color }));
  }

  /** Rates `path` `stars`; the stars it has already take its rating off. */
  rate(path: string, stars: number) {
    return this.attempt(() => this.engine.ordered('rate_track', { path, stars }));
  }

  /** Writes `genre` into the files' tags (`null`: takes it off). */
  setGenre(paths: string[] | null, genre: string | null) {
    return this.attempt(() => this.engine.ordered('set_track_genre', { paths, genre }));
  }

  async loadGenres() {
    this.genres.set(await invoke<string[]>('library_genres'));
  }

  createCrate(name: string) {
    return this.attempt(() => this.engine.ordered('create_crate', { name }));
  }

  /** Creates a smart crate, or changes one (`id`): its rules refused as the backend says. */
  saveSmartCrate(id: number | null, name: string, rules: SmartRules): Promise<void> {
    return id === null
      ? this.engine.ordered('create_smart_crate', { name, rules })
      : this.engine.ordered('update_smart_crate', { id, name, rules });
  }

  renameCrate(id: number, name: string) {
    return this.attempt(() => this.engine.ordered('rename_crate', { id, name }));
  }

  deleteCrate(id: number) {
    return this.attempt(() => this.engine.ordered('delete_crate', { id }));
  }

  deleteSession(id: number) {
    return this.attempt(() => this.engine.ordered('delete_history_session', { id }));
  }

  exportSession(session: HistorySession) {
    return this.attempt(() => this.history.export(session));
  }

  /** Asks where to save the list as an M3U playlist, and writes it. */
  exportList(list: ListRef) {
    return this.attempt(() => this.exports.exportM3u(list));
  }

  /** Makes `path` the main music folder (`null`: the system's), scanned at once, and opens it. */
  async setRoot(path: string | null): Promise<void> {
    const root = await invoke<string>('set_music_folder', { path });
    await this.openFolder(root);
  }

  /** Asks for a folder and adds it to the music folders. */
  async chooseFolder(): Promise<void> {
    const path = await open({ directory: true, title: t('Add a folder') });
    if (typeof path === 'string') await this.addFolder(path);
  }

  /** Adds a folder to the music folders, its tracks analysed, and opens it. */
  async addFolder(path: string): Promise<void> {
    await this.attempt(() => this.joinFolder(path));
  }

  /** Adds a folder to the music folders and opens it; throws what kept it out. */
  async joinFolder(path: string): Promise<void> {
    await invoke('add_music_folder', { path });
    await this.engine.ordered('library_open', { source: { kind: 'folder', path } });
  }

  /** Where a folder to add is looked for first: the phone's storage, its card, its USB drive. */
  storagePlaces(): Promise<StoragePlace[]> {
    return invoke<StoragePlace[]>('storage_places');
  }

  /** Takes a folder out of the music folders, its tracks out of the library, once the DJ
   * says so. The files stay where they are. */
  async removeFolder(folder: MusicFolder): Promise<void> {
    const yes = await this.confirm.ask({
      title: t('Remove {folder} from Prisme?', { folder: folderName(folder) }),
      detail: t('Its tracks leave the library. The files stay where they are.'),
      confirm: t('Remove'),
      cancel: t('Cancel'),
    });
    if (!yes) return;
    await this.attempt(() => invoke('remove_music_folder', { path: folder.path }));
  }

  /** Shows a folder in the computer's file manager, or a file selected in its folder. */
  showInFileManager(path: string) {
    return this.attempt(() => invoke('show_in_file_manager', { path }));
  }

  /** The folders in `path`, and how many tracks. */
  listDir(path: string): Promise<DirListing> {
    return invoke<DirListing>('list_dir', { path });
  }

  /** Looks for new or changed files and analyses them: the run started (see `scan`). */
  rescan(): Promise<number> {
    return invoke<number>('rescan_library');
  }

  /** The list and the rows sent last, whichever came first: a view told before another is not
   * shown after it. */
  private take(view: LibraryView) {
    if (view.seq >= (this.view()?.seq ?? -1)) this.view.set(view);
    this.takeWindow(view.window);
  }

  private takeWindow(window: LibraryWindow) {
    if (window.seq >= this.window().seq) this.window.set(window);
  }

  /** Runs `action`, its failure shown: whether it went through. */
  private async attempt(action: () => Promise<unknown>): Promise<boolean> {
    try {
      await action();
      this.error.set(null);
      return true;
    } catch (e) {
      this.error.set(errorText(e));
      return false;
    }
  }
}
