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
import {
  AnalysisProgress,
  DirEntry,
  DirListing,
  HistorySession,
  SmartRules,
  TrackAnalyzed,
  TrackDetails,
} from '../models/library';
import { camelotRank } from '../../shared/camelot';
import { Separation } from '../models/stems';
import { MixContext, MixReference, mixRank } from '../mix-rank';
import { CratesService } from './crates.service';
import { EngineService } from './engine.service';
import { HistoryService } from './history.service';
import { PrepareService } from './prepare.service';
import { PreferencesService } from './preferences.service';

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
 * folder is listed again when files appear in it or leave it (`library-files-changed`).
 */
@Injectable({ providedIn: 'root' })
export class LibraryService {
  private readonly preferences = inject(PreferencesService);
  private readonly engine = inject(EngineService);
  private readonly crates = inject(CratesService);
  private readonly history = inject(HistoryService);
  private readonly prepare = inject(PrepareService);
  /** Analyses received since the last frame, by path. */
  private analyzed = new Map<string, TrackDetails>();

  readonly root = signal<string | null>(null);
  readonly listing = signal<DirListing | null>(null);
  readonly source = signal<TrackSource>({ kind: 'folder' });
  readonly error = signal<string | null>(null);
  readonly progress = signal<AnalysisProgress>({
    total: 0,
    done: 0,
    failed: 0,
    running: false,
    paused: false,
  });
  /** The library's database could not be opened: what changes is lost at exit. */
  readonly inMemory = signal(false);
  readonly filter = signal('');
  readonly scope = signal<SearchScope>('list');
  readonly sort = signal<TrackSort>({ column: 'title', direction: 1 });
  /** The track the keyboard and the controllers move through, and load. */
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

  /** Tracks of the crate or history session being browsed. */
  private readonly sourceTracks = signal<DirEntry[]>([]);
  private readonly libraryResults = signal<DirEntry[]>([]);
  private searchTimer: ReturnType<typeof setTimeout> | undefined;
  /** Answers to older searches arriving late are dropped. */
  private searchId = 0;

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

  /** Whether the deck the next track goes on keeps its key: the mix order depends on it,
   * not on the rest of that deck's settings. */
  private readonly nextKeyLock = computed(() => this.engine.decks()[this.reference() ^ 1].keyLock);

  private readonly mixContext = computed((): MixContext | null => {
    const reference = this.mixReference();
    if (!reference) return null;
    return {
      reference,
      pitchRange: this.preferences.dj().pitchRange,
      keyLock: this.nextKeyLock(),
      playedTonight: this.history.playedTonight(),
    };
  });

  constructor() {
    effect(() => {
      this.referenceTrack();
      if (this.sort().column === 'mix') untracked(() => this.captureMixReference());
    });
    effect(() => {
      if (this.history.changes() === 0) return;
      const id = untracked(this.currentSession);
      if (id !== null) untracked(() => this.loadSource(() => this.history.plays(id)));
    });
  }

  async init(): Promise<void> {
    await Promise.all([
      listen<TrackAnalyzed>('track-analyzed', (e) => this.patch(e.payload)),
      listen<AnalysisProgress>('library-progress', (e) => this.progress.set(e.payload)),
      listen<string[]>('library-files-changed', (e) => this.relist(e.payload)),
    ]);
    this.progress.set(await invoke<AnalysisProgress>('library_progress'));
    this.inMemory.set(await invoke<boolean>('library_in_memory'));
    await Promise.all([this.crates.refresh(), this.history.init(), this.prepare.init()]);
    const root = await invoke<string>('music_root');
    this.root.set(root);
    const last = this.preferences.library().lastFolder;
    if (!last || !(await this.open(last))) {
      await this.open(root);
    }
  }

  /** Lists a folder and remembers it; `false` if it cannot be read. */
  async open(path: string): Promise<boolean> {
    try {
      this.listing.set(await invoke<DirListing>('list_dir', { path }));
      this.browse({ kind: 'folder' });
      await this.preferences.updateLibrary({ lastFolder: path });
      return true;
    } catch (e) {
      this.error.set(String(e));
      return false;
    }
  }

  openPrepare() {
    this.browse({ kind: 'prepare' });
  }

  /** Tracks that look like the same song, grouped. */
  async openDuplicates(): Promise<void> {
    await this.attempt(async () => {
      await this.loadSource(() => invoke<DirEntry[]>('find_duplicates'));
      this.browse({ kind: 'duplicates' });
    });
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
    await this.attempt(async () => {
      await this.loadSource(() => this.crates.tracks(id));
      this.browse({ kind: 'crate', id });
    });
  }

  async openSession(id: number): Promise<void> {
    await this.attempt(async () => {
      await this.loadSource(() => this.history.plays(id));
      this.browse({ kind: 'history', id });
    });
  }

  exportSession(session: HistorySession): Promise<void> {
    return this.attempt(async () => void (await this.history.export(session)));
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
      if (this.currentCrate() === id) await this.loadSource(() => this.crates.tracks(id));
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
      if (this.currentCrate() === id) await this.loadSource(() => this.crates.tracks(id));
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
  moveSelection(delta: number) {
    this.clearSelection();
    const tracks = this.visibleTracks();
    if (!tracks.length) return;
    const index = tracks.findIndex((t) => t.path === this.selected());
    const next = index < 0 ? 0 : Math.min(Math.max(index + delta, 0), tracks.length - 1);
    this.selected.set(tracks[next].path);
  }

  /** Loads the selected track on `deck`, or on the first free deck (deck A if none). */
  loadSelected(deck: number | null) {
    const path = this.selected();
    if (path) this.load(path, deck);
  }

  load(path: string, deck: number | null) {
    this.selected.set(path);
    const free = this.engine.deckIndexes().find((d) => !this.engine.tracks()[d]);
    this.engine.load(deck ?? free ?? 0, path);
  }

  private async loadSource(tracks: () => Promise<DirEntry[]>): Promise<void> {
    this.sourceTracks.set(await tracks());
  }

  private browse(source: TrackSource) {
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
      this.error.set(String(e));
    }
  }

  /** Lists the open folder again if it is one of `folders`, keeping the view as it is. */
  private async relist(folders: string[]) {
    const path = this.listing()?.path;
    if (!path || !folders.includes(path)) return;
    await this.attempt(async () =>
      this.listing.set(await invoke<DirListing>('list_dir', { path })),
    );
  }

  private searchLibrary() {
    clearTimeout(this.searchTimer);
    const id = ++this.searchId;
    const query = this.filter().trim();
    if (!query) {
      this.libraryResults.set([]);
      return;
    }
    this.searchTimer = setTimeout(async () => {
      try {
        const results = await invoke<DirEntry[]>('search_library', { query });
        if (id === this.searchId) this.libraryResults.set(results);
      } catch (e) {
        this.error.set(String(e));
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
    for (const list of [this.sourceTracks, this.libraryResults, this.prepare.list]) {
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
    for (const list of [this.sourceTracks, this.libraryResults, this.prepare.list]) {
      const tracks = update(list());
      if (tracks !== list()) list.set(tracks);
    }
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
