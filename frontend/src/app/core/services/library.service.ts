import {
  Injectable,
  computed,
  effect,
  inject,
  linkedSignal,
  signal,
  untracked,
} from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import {
  AnalysisProgress,
  DirEntry,
  DirListing,
  HistorySession,
  MusicFolder,
  SmartRules,
  StoragePlace,
  TrackAnalyzed,
  TrackDetails,
} from '../models/library';
import { camelotRank } from '../../shared/camelot';
import { Separation } from '../models/stems';
import { MixContext, MixReference, mixRank } from '../mix-rank';
import { CratesService } from './crates.service';
import { EngineService } from './engine.service';
import { ExportService, ListRef } from './export.service';
import { HistoryService } from './history.service';
import { PrepareService } from './prepare.service';
import { PreferencesService } from './preferences.service';
import { errorText } from '../errors';
import { folderName, folderOf } from '../folders';
import { t } from '../i18n/i18n';
import { ConfirmService } from '../../shared/confirm/confirm';

/** `order`: the order of the list itself (play order of a set). */
/** `mix`: how well the tracks follow the reference deck (see `mixRank`). */
export type SortColumn =
  | 'order'
  | 'title'
  | 'artist'
  | 'album'
  | 'genre'
  | 'year'
  | 'bpm'
  | 'key'
  | 'rating'
  | 'plays'
  | 'mix';

export interface TrackSort {
  column: SortColumn;
  direction: 1 | -1;
}

/** The list of tracks being browsed. */
export type TrackSource =
  | { kind: 'folder' }
  | { kind: 'prepare' }
  | { kind: 'duplicates' }
  | { kind: 'elsewhere' }
  | { kind: 'crate'; id: number }
  | { kind: 'history'; id: number };

/** Where the search looks: the tracks being browsed, or every analysed track. */
export type SearchScope = 'list' | 'library';

/** Wait after the last keystroke before searching the whole library. */
const LIBRARY_SEARCH_DELAY_MS = 120;

/** Title shown for a track: its tag, else the file name without extension. */
export function trackTitle(t: DirEntry): string {
  return t.details?.tags.title ?? t.name.replace(/\.[^.]+$/, '');
}

/** Whether every word of the search appears in the track's title, tags or file name. */
/** The list `steps` after `from` among `lists` (before it, negative), held at the ends; the
 * first if `from` is not among them. */
export function listAfter<T>(lists: T[], from: T, steps: number): T {
  const at = Math.max(lists.indexOf(from), 0);
  return lists[Math.min(Math.max(at + steps, 0), lists.length - 1)];
}

/** The first of `decks` that is not playing, if one is not. */
export function firstStoppedDeck(decks: number[], playing: (deck: number) => boolean) {
  return decks.find((deck) => !playing(deck));
}

export function matchesSearch(t: DirEntry, words: string[]): boolean {
  const tags = t.details?.tags;
  const text = [t.name, tags?.title, tags?.artist, tags?.album, tags?.genre]
    .join(' ')
    .toLowerCase();
  return words.every((w) => text.includes(w));
}

/**
 * Browsing the music folders on disk, the crates and the history, and searching the library, with
 * the analysis results and tags of the tracks, and the view on them (search, sort,
 * selection). The background analysis pushes each result (`track-analyzed`) and its
 * progress (`library-progress`): the lists are patched in place, no reload needed. The open
 * folder is listed again when files appear in it or leave it (`library-files-changed`); the
 * music folders, when one is added, removed, unplugged or plugged back
 * (`music-folders-changed`). The tracks loaded from outside them (dropped on a deck from the
 * file manager) are listed apart, "elsewhere", until they are removed or their folder added.
 */
@Injectable({ providedIn: 'root' })
export class LibraryService {
  private readonly preferences = inject(PreferencesService);
  private readonly engine = inject(EngineService);
  private readonly crates = inject(CratesService);
  private readonly history = inject(HistoryService);
  private readonly prepare = inject(PrepareService);
  private readonly exports = inject(ExportService);
  private readonly confirm = inject(ConfirmService);
  /** Analyses received since the last frame, by path. */
  private analyzed = new Map<string, TrackDetails>();

  /** The folders whose tracks make the library, the main one first. */
  readonly folders = signal<MusicFolder[]>([]);
  /** The main music folder. */
  readonly root = computed(() => this.folders().find((f) => f.main)?.path ?? null);
  readonly listing = signal<DirListing | null>(null);
  /** The music folder the open folder is in. */
  readonly folderRoot = computed(() => {
    const path = this.listing()?.path;
    return path ? folderOf(this.folders(), path) : null;
  });
  readonly source = signal<TrackSource>({ kind: 'folder' });
  readonly error = signal<string | null>(null);
  readonly progress = signal<AnalysisProgress>({
    total: 0,
    done: 0,
    failed: 0,
    running: false,
    paused: false,
  });
  /** How many scans of the folder have ended. */
  readonly scansEnded = signal(0);
  /** The library's database could not be opened: what changes is lost at exit. */
  readonly inMemory = signal(false);
  readonly filter = signal('');
  readonly scope = signal<SearchScope>('list');
  readonly sort = signal<TrackSort>({ column: 'title', direction: 1 });
  /** The track the keyboard and the controllers move through, and load (the backend is told of
   * it). */
  readonly selected = signal<string | null>(null);
  /** Tracks picked with Ctrl and Shift clicks (or Ctrl + A), the cursor's among them. */
  readonly picked = signal<ReadonlySet<string>>(new Set());
  /** Where a Shift click extends the selection from. */
  private anchor: string | null = null;
  /** The tracks the actions apply to: the picked ones, else the cursor's. */
  readonly selection = computed<ReadonlySet<string>>(() => {
    const picked = this.picked();
    const cursor = this.selected();
    return picked.size > 0 ? picked : new Set(cursor ? [cursor] : []);
  });
  /** The selection, in the order shown. */
  readonly selectedPaths = computed(() => {
    const selection = this.selection();
    return [...new Set(this.visibleTracks().map((t) => t.path))].filter((p) => selection.has(p));
  });
  /** The genres of the library, the most common first (loaded on demand). */
  readonly genres = signal<string[]>([]);
  /** The deck the tracks are compared with: compatible keys, pitch to match. */
  readonly reference = linkedSignal<number, number>({
    source: () => this.engine.deckIndexes().length,
    // a deck that is no longer shown gives way to A
    computation: (decks, previous) => (previous && previous.value < decks ? previous.value : 0),
  });
  /** Incremented to ask the view to focus the search field. */
  readonly searchRequests = signal(0);
  /** Where a controller's browse knob moves: the tracks, or the lists on the left. */
  readonly focus = signal<'tracks' | 'lists'>('tracks');
  /** The list on the left a controller points at: the prepare list, or a crate (its id). */
  readonly pointedList = signal<'prepare' | number>('prepare');
  private readonly lists = computed<('prepare' | number)[]>(() => [
    'prepare',
    ...this.crates.crates().map((c) => c.id),
  ]);

  /** The tracks of the library under none of the music folders. */
  readonly elsewhere = signal<DirEntry[]>([]);
  private readonly elsewherePaths = computed(() => new Set(this.elsewhere().map((t) => t.path)));
  /** Answers to older fetches of the tracks from elsewhere arriving late are dropped. */
  private elsewhereId = 0;

  /** Tracks of the crate or history session being browsed. */
  private readonly sourceTracks = signal<DirEntry[]>([]);
  private readonly libraryResults = signal<DirEntry[]>([]);
  private searchTimer: ReturnType<typeof setTimeout> | undefined;
  /** Answers to older searches arriving late are dropped. */
  private searchId = 0;
  /** The same for the lists: the one asked for last is shown. */
  private listId = 0;

  /** The crate being browsed, if any. */
  readonly currentCrate = computed(() => {
    const source = this.source();
    return source.kind === 'crate' ? source.id : null;
  });

  /** The history session being browsed, if any. */
  readonly currentSession = computed(() => {
    const source = this.source();
    return source.kind === 'history' ? source.id : null;
  });

  /** Whether the list has an order of its own (shown in a `#` column). */
  readonly ordered = computed(() =>
    ['history', 'prepare', 'duplicates'].includes(this.source().kind),
  );

  /** Tracks of the folder, prepare list, crate or history session being browsed. */
  readonly listTracks = computed(() => {
    switch (this.source().kind) {
      case 'folder':
        return this.listing()?.tracks ?? [];
      case 'prepare':
        return this.prepare.list();
      case 'elsewhere':
        return this.elsewhere();
      default:
        return this.sourceTracks();
    }
  });

  /** Position of each track in its list, from 1. */
  readonly positions = computed(() => new Map(this.listTracks().map((t, i) => [t, i + 1])));

  /** Tracks as shown: those of the list matching the search, or the library's matches; sorted. */
  readonly visibleTracks = computed(() => {
    const words = this.filter().trim().toLowerCase().split(/\s+/).filter(Boolean);
    const shown =
      this.scope() === 'library'
        ? [...this.libraryResults()]
        : this.listTracks().filter((t) => matchesSearch(t, words));
    const { column, direction } = this.sort();
    if (column === 'mix') {
      const context = this.mixContext();
      if (!context) return shown;
      const ranks = new Map(shown.map((t) => [t, mixRank(t, context)]));
      return shown.sort((a, b) => compareRanks(ranks.get(a)!, ranks.get(b)!) * direction);
    }
    const positions = this.positions();
    const value = (t: DirEntry) =>
      column === 'order' ? (positions.get(t) ?? Number.MAX_SAFE_INTEGER) : sortValue(t, column);
    return shown.sort((a, b) => compare(value(a), value(b)) * direction);
  });

  /**
   * The reference deck as it sounded when sorting by `mix` was chosen, or when another
   * track came on it: its tempo and key move with the platter, the order should not.
   */
  private readonly mixReference = signal<MixReference | null>(null);

  /** The reference deck and its track: another load, or its analysis arriving after it. */
  private readonly referenceTrack = computed(() => {
    const deck = this.reference();
    const track = this.engine.tracks()[deck];
    return `${deck} ${track?.loadId} ${track?.tempo?.bpm} ${track?.key?.camelot}`;
  });

  /** Whether the deck the next track goes on keeps its key, and the range of its tempo fader:
   * the mix order depends on them, not on the rest of that deck's settings. */
  private readonly nextKeyLock = computed(() => this.engine.decks()[this.reference() ^ 1].keyLock);
  private readonly nextPitchRange = computed(() => this.engine.tempoRange(this.reference() ^ 1));

  private readonly mixContext = computed((): MixContext | null => {
    const reference = this.mixReference();
    if (!reference) return null;
    return {
      reference,
      pitchRange: this.nextPitchRange(),
      keyLock: this.nextKeyLock(),
      playedTonight: this.history.playedTonight(),
    };
  });

  constructor() {
    effect(() => {
      const path = this.selected();
      untracked(() => void this.tellSelection(path));
    });
    effect(() => {
      this.referenceTrack();
      if (this.sort().column === 'mix') untracked(() => this.captureMixReference());
    });
    effect(() => {
      if (this.history.changes() === 0) return;
      const id = untracked(this.currentSession);
      if (id !== null) untracked(() => this.reloadSource(() => this.history.plays(id)));
    });
  }

  async init(): Promise<void> {
    await Promise.all([
      listen<TrackAnalyzed>('track-analyzed', (e) => this.patch(e.payload)),
      listen<AnalysisProgress>('library-progress', (e) => {
        this.progress.set(e.payload);
        if (e.payload.running) return;
        this.scansEnded.update((n) => n + 1);
        void this.refreshElsewhere();
      }),
      listen<string[]>('library-files-changed', (e) => this.relist(e.payload)),
      listen<MusicFolder[]>('music-folders-changed', (e) => this.foldersChanged(e.payload)),
    ]);
    this.progress.set(await invoke<AnalysisProgress>('library_progress'));
    this.inMemory.set(await invoke<boolean>('library_in_memory'));
    await Promise.all([this.crates.refresh(), this.history.init(), this.prepare.init()]);
    this.folders.set(await invoke<MusicFolder[]>('music_folders'));
    await this.refreshElsewhere();
    const last = this.preferences.library().lastFolder;
    const root = this.root();
    if ((!last || !(await this.open(last))) && root) {
      await this.open(root);
    }
  }

  /** Makes `path` the main music folder (`null`: the system's), scanned at once, and opens it. */
  async setRoot(path: string | null): Promise<void> {
    const root = await invoke<string>('set_music_folder', { path });
    await this.preferences.load();
    this.folders.set(await invoke<MusicFolder[]>('music_folders'));
    await this.open(root);
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
    this.folders.set(await invoke<MusicFolder[]>('add_music_folder', { path }));
    await this.open(path);
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
    await this.attempt(async () =>
      this.foldersChanged(
        await invoke<MusicFolder[]>('remove_music_folder', { path: folder.path }),
      ),
    );
  }

  /** Shows a folder in the computer's file manager, or a file selected in its folder. */
  showInFileManager(path: string): Promise<void> {
    return this.attempt(() => invoke('show_in_file_manager', { path }));
  }

  /** Whether the track at `path` is under none of the music folders. */
  isElsewhere(path: string): boolean {
    return this.elsewherePaths().has(path);
  }

  /** The tracks from elsewhere, fetched again. */
  refreshElsewhere(): Promise<void> {
    const id = ++this.elsewhereId;
    return this.attempt(async () => {
      const tracks = await invoke<DirEntry[]>('elsewhere_tracks');
      if (id === this.elsewhereId) this.elsewhere.set(tracks);
    });
  }

  openElsewhere() {
    this.browse({ kind: 'elsewhere' });
  }

  /** Takes tracks from elsewhere out of the library, once the DJ says so. The files stay where
   * they are; their list, emptied, gives way to the main folder. */
  async removeFromPrisme(paths: string[]): Promise<void> {
    const tracks = this.elsewhere().filter((t) => paths.includes(t.path));
    if (!tracks.length) return;
    const one = tracks.length === 1;
    const yes = await this.confirm.ask({
      title: one
        ? t('Remove “{title}” from Prisme?', { title: trackTitle(tracks[0]) })
        : t('Remove these {n} tracks from Prisme?', { n: tracks.length }),
      detail: one
        ? t('It leaves the library. The file stays where it is.')
        : t('They leave the library. The files stay where they are.'),
      confirm: t('Remove'),
      cancel: t('Cancel'),
    });
    if (!yes) return;
    const removed = tracks.map((t) => t.path);
    const next = this.neighbour(removed[removed.length - 1]);
    await this.attempt(async () => {
      await invoke('remove_tracks', { paths: removed });
      this.libraryResults.update((results) => results.filter((t) => !removed.includes(t.path)));
      this.clearSelection();
      this.selected.set(next && !removed.includes(next) ? next : null);
      await this.refreshElsewhere();
      const root = this.root();
      if (!this.elsewhere().length && this.source().kind === 'elsewhere' && root) {
        await this.open(root);
      }
    });
  }

  /** The folders in `path`. */
  async subFolders(path: string): Promise<DirEntry[]> {
    return (await this.listDir(path)).folders;
  }

  /** The folders and tracks in `path`, the folder browsed left as it is. */
  listDir(path: string): Promise<DirListing> {
    return invoke<DirListing>('list_dir', { path });
  }

  /** Lists a folder and remembers it; `false` if it cannot be read. */
  async open(path: string): Promise<boolean> {
    const id = ++this.listId;
    try {
      const listing = await invoke<DirListing>('list_dir', { path });
      if (id !== this.listId) return true;
      this.listing.set(listing);
      this.browse({ kind: 'folder' });
      await this.preferences.updateLibrary({ lastFolder: path });
      return true;
    } catch (e) {
      this.error.set(errorText(e));
      return false;
    }
  }

  openPrepare() {
    this.browse({ kind: 'prepare' });
  }

  /** Tracks that look like the same song, grouped. */
  async openDuplicates(): Promise<void> {
    await this.attempt(() =>
      this.showSource({ kind: 'duplicates' }, () => invoke<DirEntry[]>('find_duplicates')),
    );
  }

  addToPrepare(paths: string[]): Promise<void> {
    return this.attempt(() => this.prepare.add(paths));
  }

  /** Puts tracks before `before` in the prepare list (`null`: at its end), adding them if needed. */
  placeInPrepare(paths: string[], before: string | null): Promise<void> {
    if (before !== null && paths.includes(before)) return Promise.resolve();
    return this.attempt(async () => {
      const missing = paths.filter((p) => !this.prepare.list().some((t) => t.path === p));
      if (missing.length) await this.prepare.add(missing);
      // each one in turn just before `before`: they keep their order
      for (const path of paths) {
        const others = this.prepare.list().filter((t) => t.path !== path);
        const index = before === null ? others.length : others.findIndex((t) => t.path === before);
        await this.prepare.move(path, index);
      }
    });
  }

  addSelectedToPrepare(): Promise<void> {
    const paths = this.selectedPaths();
    return paths.length ? this.addToPrepare(paths) : Promise.resolve();
  }

  async openCrate(id: number): Promise<void> {
    await this.attempt(() => this.showSource({ kind: 'crate', id }, () => this.crates.tracks(id)));
  }

  async openSession(id: number): Promise<void> {
    await this.attempt(() =>
      this.showSource({ kind: 'history', id }, () => this.history.plays(id)),
    );
  }

  exportSession(session: HistorySession): Promise<void> {
    return this.attempt(async () => void (await this.history.export(session)));
  }

  /** Asks where to save the list as an M3U playlist, and writes it. */
  exportList(list: ListRef): Promise<void> {
    return this.attempt(async () => void (await this.exports.exportM3u(list)));
  }

  async deleteSession(id: number): Promise<void> {
    await this.attempt(async () => {
      await this.history.delete(id);
      if (this.currentSession() === id) this.browse({ kind: 'folder' });
    });
  }

  async createCrate(name: string): Promise<void> {
    await this.attempt(async () => this.openCrate(await this.crates.create(name)));
  }

  async createSmartCrate(name: string, rules: SmartRules): Promise<void> {
    await this.attempt(async () => this.openCrate(await this.crates.createSmart(name, rules)));
  }

  /** Renames a smart crate and changes its rules. */
  async updateSmartCrate(id: number, name: string, rules: SmartRules): Promise<void> {
    await this.attempt(async () => {
      await this.crates.rename(id, name);
      await this.crates.setRules(id, rules);
      if (this.currentCrate() === id) await this.reloadSource(() => this.crates.tracks(id));
    });
  }

  renameCrate(id: number, name: string): Promise<void> {
    return this.attempt(() => this.crates.rename(id, name));
  }

  async deleteCrate(id: number): Promise<void> {
    await this.attempt(async () => {
      await this.crates.delete(id);
      if (this.currentCrate() === id) this.browse({ kind: 'folder' });
    });
  }

  async addToCrate(id: number, paths: string[]): Promise<void> {
    await this.attempt(async () => {
      await this.crates.add(id, paths);
      if (this.currentCrate() === id) await this.reloadSource(() => this.crates.tracks(id));
    });
  }

  /** Whether the selection can be taken out of the list being browsed. */
  readonly removable = computed(() => {
    const source = this.source();
    if (this.scope() !== 'list') return false;
    if (source.kind === 'prepare') return true;
    return source.kind === 'crate' && !this.crates.crates().find((c) => c.id === source.id)?.rules;
  });

  /** Takes the selected tracks out of the crate or prepare list being browsed. */
  async removeSelected(): Promise<void> {
    const source = this.source();
    const paths = this.selectedPaths();
    if (!paths.length || !this.removable()) return;
    const next = this.neighbour(paths[paths.length - 1]);
    await this.attempt(async () => {
      if (source.kind === 'crate') {
        await this.crates.remove(source.id, paths);
        this.sourceTracks.update((tracks) => tracks.filter((t) => !paths.includes(t.path)));
      } else if (source.kind === 'prepare') {
        await this.prepare.remove(paths);
      }
      this.clearSelection();
      this.selected.set(next && !paths.includes(next) ? next : null);
    });
  }

  /** Writes `genre` into the files' tags (`null`: takes it off). */
  setGenre(paths: string[], genre: string | null): Promise<void> {
    return this.attempt(() => invoke('set_track_genre', { paths, genre }));
  }

  async loadGenres() {
    this.genres.set(await invoke<string[]>('library_genres'));
  }

  setFilter(text: string) {
    this.filter.set(text);
    if (this.scope() === 'library') this.searchLibrary();
  }

  setScope(scope: SearchScope) {
    this.scope.set(scope);
    if (scope === 'library') this.searchLibrary();
  }

  /** Colours tracks; `null` takes the colour off. The lists update with `track-analyzed`. */
  setColor(paths: string[], color: number | null): Promise<void> {
    return this.attempt(() => invoke('set_track_color', { paths, color }));
  }

  /** Rates tracks from 0 (not rated) to 5. */
  setRating(paths: string[], rating: number): Promise<void> {
    return this.attempt(() => invoke('set_track_rating', { paths, rating }));
  }

  /** Looks for new or changed files and analyses them. */
  rescan(): Promise<void> {
    return invoke('rescan_library');
  }

  /** Click on a column title: sort by it, again to reverse. */
  sortBy(column: SortColumn) {
    const s = this.sort();
    this.sort.set({ column, direction: s.column === column ? (-s.direction as 1 | -1) : 1 });
  }

  private captureMixReference() {
    const deck = this.reference();
    this.mixReference.set({
      bpm: this.engine.status()?.decks[deck]?.bpm ?? this.engine.tracks()[deck]?.tempo?.bpm ?? null,
      camelot: this.engine.heardKey(deck)?.camelot ?? null,
      path: this.engine.tracks()[deck]?.path ?? null,
    });
  }

  /**
   * A click on a row: `only` selects it alone, `toggle` (Ctrl) adds or removes it, `range`
   * (Shift) selects from the last clicked one.
   */
  select(path: string, mode: 'only' | 'toggle' | 'range') {
    if (mode === 'only') {
      this.picked.set(new Set());
    } else if (mode === 'toggle') {
      const picked = new Set(this.selection());
      if (!picked.delete(path)) picked.add(path);
      this.picked.set(picked);
    } else {
      const paths = this.visibleTracks().map((t) => t.path);
      const from = paths.indexOf(this.anchor ?? this.selected() ?? path);
      const to = paths.indexOf(path);
      const [a, b] = from < 0 ? [to, to] : [Math.min(from, to), Math.max(from, to)];
      this.picked.set(new Set(paths.slice(a, b + 1)));
    }
    if (mode !== 'range') this.anchor = path;
    this.selected.set(path);
  }

  selectAll() {
    this.picked.set(new Set(this.visibleTracks().map((t) => t.path)));
  }

  clearSelection() {
    this.picked.set(new Set());
    this.anchor = null;
  }

  /** Moves the selection through the visible tracks. */
  /** The pointer on the lists of the left moved by `steps`, the knob on them. */
  movePointedList(steps: number) {
    this.pointedList.set(listAfter(this.lists(), this.pointedList(), steps));
    this.focus.set('lists');
  }

  /** Opens the list pointed at; the knob back on its tracks. */
  openPointedList() {
    const list = this.pointedList();
    if (list === 'prepare') this.openPrepare();
    else void this.openCrate(list);
    this.focus.set('tracks');
  }

  /** Opens the list `steps` after the one open on the left (else the one pointed at). */
  openNextList(steps: number) {
    const source = this.source();
    if (source.kind === 'prepare') this.pointedList.set('prepare');
    else if (source.kind === 'crate') this.pointedList.set(source.id);
    this.movePointedList(steps);
    this.openPointedList();
  }

  /** Loads the selected track on the first deck that is not playing, if one is not. */
  loadSelectedOnStoppedDeck() {
    const decks = this.engine.status()?.decks ?? [];
    const stopped = firstStoppedDeck(this.engine.deckIndexes(), (d) => !!decks[d]?.playing);
    if (stopped !== undefined) void this.engine.perform(`library.load${stopped + 1}`, 1);
  }

  moveSelection(delta: number) {
    this.clearSelection();
    const tracks = this.visibleTracks();
    if (!tracks.length) return;
    const index = tracks.findIndex((t) => t.path === this.selected());
    const next = index < 0 ? 0 : Math.min(Math.max(index + delta, 0), tracks.length - 1);
    this.selected.set(tracks[next].path);
  }

  /** Loads `path` on `deck`, or on the first deck without a track (deck A if none), selected
   * first: the backend loads the selected track. Whether it was. */
  async load(path: string, deck: number | null): Promise<boolean> {
    this.selected.set(path);
    await this.tellSelection(path);
    return this.engine.perform(deck === null ? 'library.load' : `library.load${deck + 1}`, 1);
  }

  /** Tells the backend the track selected. */
  private tellSelection(path: string | null): Promise<void> {
    return invoke<void>('set_library_selection', { path }).catch((e) =>
      this.error.set(errorText(e)),
    );
  }

  /** Browses `source`, its tracks those `tracks` gives, unless another list was asked for
   * meanwhile. */
  private async showSource(source: TrackSource, tracks: () => Promise<DirEntry[]>) {
    const id = ++this.listId;
    const list = await tracks();
    if (id !== this.listId) return;
    this.sourceTracks.set(list);
    this.browse(source);
  }

  /** The tracks of the crate or session browsed again, unless another list was asked for
   * meanwhile. */
  private async reloadSource(tracks: () => Promise<DirEntry[]>) {
    const id = this.listId;
    const list = await tracks();
    if (id === this.listId) this.sourceTracks.set(list);
  }

  private browse(source: TrackSource) {
    this.listId++;
    this.clearSelection();
    this.source.set(source);
    this.scope.set('list');
    this.setFilter('');
    this.error.set(null);
    if (this.ordered()) this.sort.set({ column: 'order', direction: 1 });
    else if (this.sort().column === 'order') this.sort.set({ column: 'title', direction: 1 });
  }

  /** The track shown after `path`, else before it: where the selection goes when it leaves. */
  private neighbour(path: string): string | null {
    const tracks = this.visibleTracks();
    const index = tracks.findIndex((t) => t.path === path);
    return (tracks[index + 1] ?? tracks[index - 1])?.path ?? null;
  }

  private async attempt(action: () => Promise<void>): Promise<void> {
    try {
      await action();
      this.error.set(null);
    } catch (e) {
      this.error.set(errorText(e));
    }
  }

  /** The folder browsed left with its music folder: the main one opens instead. */
  private async foldersChanged(folders: MusicFolder[]) {
    this.folders.set(folders);
    void this.refreshElsewhere();
    const root = this.root();
    if (this.source().kind === 'folder' && this.listing() && !this.folderRoot() && root) {
      await this.open(root);
    }
  }

  /** Lists the open folder again if it is one of `folders`, keeping the view as it is. */
  private async relist(folders: string[]) {
    const path = this.listing()?.path;
    if (!path || !folders.includes(path)) return;
    const id = this.listId;
    await this.attempt(async () => {
      const listing = await invoke<DirListing>('list_dir', { path });
      if (id === this.listId) this.listing.set(listing);
    });
  }

  private searchLibrary() {
    clearTimeout(this.searchTimer);
    const id = ++this.searchId;
    const query = this.filter().trim();
    this.searchTimer = setTimeout(async () => {
      try {
        const results = await invoke<DirEntry[]>('search_library', { query });
        if (id === this.searchId) this.libraryResults.set(results);
      } catch (e) {
        this.error.set(errorText(e));
      }
    }, LIBRARY_SEARCH_DELAY_MS);
  }

  /** Shows `path` as separated this far in the lists. */
  markSeparated(path: string, separation: Separation) {
    const update = (tracks: DirEntry[]) =>
      tracks.map((t) =>
        t.path === path && t.details ? { ...t, details: { ...t.details, separation } } : t,
      );
    const listing = this.listing();
    if (listing?.tracks.some((t) => t.path === path)) {
      this.listing.set({ ...listing, tracks: update(listing.tracks) });
    }
    for (const list of [
      this.sourceTracks,
      this.libraryResults,
      this.prepare.list,
      this.elsewhere,
    ]) {
      if (list().some((t) => t.path === path)) list.update(update);
    }
  }

  /** Takes the analysis of a track; a scan sends them faster than frames are drawn, so they
   * reach the lists together, once a frame. */
  private patch({ path, details }: TrackAnalyzed) {
    if (this.analyzed.size === 0) requestAnimationFrame(() => this.applyAnalyzed());
    this.analyzed.set(path, details);
  }

  private applyAnalyzed() {
    const analyzed = this.analyzed;
    this.analyzed = new Map();
    const update = (tracks: DirEntry[]) =>
      tracks.some((t) => analyzed.has(t.path))
        ? tracks.map((t) => {
            const details = analyzed.get(t.path);
            return details ? { ...t, details } : t;
          })
        : tracks;
    const listing = this.listing();
    if (listing) {
      const tracks = update(listing.tracks);
      if (tracks !== listing.tracks) this.listing.set({ ...listing, tracks });
    }
    for (const list of [
      this.sourceTracks,
      this.libraryResults,
      this.prepare.list,
      this.elsewhere,
    ]) {
      const tracks = update(list());
      if (tracks !== list()) list.set(tracks);
    }
    // a track just loaded from outside the music folders joins those from elsewhere
    const folders = this.folders();
    const arrived = [...analyzed.keys()].some(
      (path) => !this.isElsewhere(path) && !folderOf(folders, path),
    );
    if (arrived) void this.refreshElsewhere();
  }
}

/** Value a tag or analysis column sorts on; tracks without one go last. */
function sortValue(t: DirEntry, column: Exclude<SortColumn, 'order' | 'mix'>): number | string {
  const d = t.details;
  const text = (s: string | null | undefined) => s?.toLowerCase() ?? '\uffff';
  switch (column) {
    case 'title':
      return trackTitle(t).toLowerCase();
    case 'artist':
      return text(d?.tags.artist);
    case 'album':
      return text(d?.tags.album);
    case 'genre':
      return text(d?.tags.genre);
    case 'year':
      return d?.tags.year ?? Number.MAX_SAFE_INTEGER;
    case 'bpm':
      return d?.bpm ?? Number.MAX_SAFE_INTEGER;
    case 'key':
      return camelotRank(d?.key?.camelot);
    // the best rated and most played first
    case 'rating':
      return -(d?.rating ?? 0);
    case 'plays':
      return -(d?.playCount ?? 0);
  }
}

function compare(a: number | string, b: number | string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

function compareRanks(a: number[], b: number[]): number {
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== b[i]) return a[i] - b[i];
  }
  return 0;
}
