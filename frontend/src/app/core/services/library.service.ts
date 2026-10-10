import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  AnalysisProgress,
  DirEntry,
  DirListing,
  HistorySession,
  SmartRules,
  TrackAnalyzed,
} from '../models/library';
import { camelotRank } from '../../shared/camelot';
import { CratesService } from './crates.service';
import { EngineService } from './engine.service';
import { HistoryService } from './history.service';
import { PrepareService } from './prepare.service';
import { PreferencesService } from './preferences.service';

/** `order`: the order of the list itself (play order of a set). */
export type SortColumn =
  'order' | 'title' | 'artist' | 'album' | 'genre' | 'year' | 'bpm' | 'key' | 'rating' | 'plays';

export interface TrackSort {
  column: SortColumn;
  direction: 1 | -1;
}

/** The list of tracks being browsed. */
export type TrackSource =
  | { kind: 'folder' }
  | { kind: 'prepare' }
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

  readonly root = signal<string | null>(null);
  readonly listing = signal<DirListing | null>(null);
  readonly source = signal<TrackSource>({ kind: 'folder' });
  readonly error = signal<string | null>(null);
  readonly progress = signal<AnalysisProgress>({ total: 0, done: 0, failed: 0, running: false });
  readonly filter = signal('');
  readonly scope = signal<SearchScope>('list');
  readonly sort = signal<TrackSort>({ column: 'title', direction: 1 });
  readonly selected = signal<string | null>(null);
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
  readonly ordered = computed(() => ['history', 'prepare'].includes(this.source().kind));

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
    const positions = this.positions();
    const value = (t: DirEntry) =>
      column === 'order' ? (positions.get(t) ?? Number.MAX_SAFE_INTEGER) : sortValue(t, column);
    return shown.sort((a, b) => compare(value(a), value(b)) * direction);
  });

  constructor() {
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

  addToPrepare(paths: string[]): Promise<void> {
    return this.attempt(() => this.prepare.add(paths));
  }

  /** Puts a track at `index` of the prepare list, adding it if needed. */
  placeInPrepare(path: string, index: number): Promise<void> {
    return this.attempt(async () => {
      if (!this.prepare.list().some((t) => t.path === path)) await this.prepare.add([path]);
      await this.prepare.move(path, index);
    });
  }

  addSelectedToPrepare(): Promise<void> {
    const path = this.selected();
    return path ? this.addToPrepare([path]) : Promise.resolve();
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

  /** Takes the selected track out of the crate or prepare list being browsed. */
  async removeSelected(): Promise<void> {
    const source = this.source();
    const path = this.selected();
    if (!path || this.scope() !== 'list') return;
    const next = this.neighbour(path);
    await this.attempt(async () => {
      if (source.kind === 'crate') {
        if (this.crates.crates().find((c) => c.id === source.id)?.rules) return;
        await this.crates.remove(source.id, [path]);
        this.sourceTracks.update((tracks) => tracks.filter((t) => t.path !== path));
      } else if (source.kind === 'prepare') {
        await this.prepare.remove([path]);
      } else {
        return;
      }
      this.selected.set(next);
    });
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

  /** Moves the selection through the visible tracks. */
  moveSelection(delta: number) {
    const tracks = this.visibleTracks();
    if (!tracks.length) return;
    const index = tracks.findIndex((t) => t.path === this.selected());
    const next = index < 0 ? 0 : Math.min(Math.max(index + delta, 0), tracks.length - 1);
    this.selected.set(tracks[next].path);
  }

  /** Loads the selected track on `deck`, or on the first free deck (deck 1 if none). */
  loadSelected(deck: number | null) {
    const path = this.selected();
    if (path) this.load(path, deck);
  }

  load(path: string, deck: number | null) {
    this.selected.set(path);
    const free = this.engine.tracks().findIndex((t) => !t);
    this.engine.load(deck ?? (free >= 0 ? free : 0), path);
  }

  private async loadSource(tracks: () => Promise<DirEntry[]>): Promise<void> {
    this.sourceTracks.set(await tracks());
  }

  private browse(source: TrackSource) {
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

  private patch({ path, details }: TrackAnalyzed) {
    const update = (tracks: DirEntry[]) =>
      tracks.map((t) => (t.path === path ? { ...t, details } : t));
    const listing = this.listing();
    if (listing?.tracks.some((t) => t.path === path)) {
      this.listing.set({ ...listing, tracks: update(listing.tracks) });
    }
    for (const list of [this.sourceTracks, this.libraryResults, this.prepare.list]) {
      if (list().some((t) => t.path === path)) list.update(update);
    }
  }
}

/** Value a tag or analysis column sorts on; tracks without one go last. */
function sortValue(t: DirEntry, column: Exclude<SortColumn, 'order'>): number | string {
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
