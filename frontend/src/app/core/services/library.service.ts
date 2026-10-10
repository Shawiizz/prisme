import { Injectable, computed, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { AnalysisProgress, DirEntry, DirListing, TrackAnalyzed } from '../models/library';
import { camelotRank } from '../../shared/camelot';
import { CratesService } from './crates.service';
import { EngineService } from './engine.service';
import { PreferencesService } from './preferences.service';

export type SortColumn = 'title' | 'artist' | 'album' | 'genre' | 'year' | 'bpm' | 'key';

export interface TrackSort {
  column: SortColumn;
  direction: 1 | -1;
}

/** The list of tracks being browsed. */
export type TrackSource = { kind: 'folder' } | { kind: 'crate'; id: number };

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
 * Browsing the music folders on disk and the crates, and searching the library, with
 * the analysis results and tags of the tracks, and the view on them (search, sort,
 * selection). The background analysis pushes each result (`track-analyzed`) and its
 * progress (`library-progress`): the lists are patched in place, no reload needed.
 */
@Injectable({ providedIn: 'root' })
export class LibraryService {
  private readonly preferences = inject(PreferencesService);
  private readonly engine = inject(EngineService);
  private readonly crates = inject(CratesService);

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

  private readonly crateTracks = signal<DirEntry[]>([]);
  private readonly libraryResults = signal<DirEntry[]>([]);
  private searchTimer: ReturnType<typeof setTimeout> | undefined;
  /** Answers to older searches arriving late are dropped. */
  private searchId = 0;

  /** The crate being browsed, if any. */
  readonly currentCrate = computed(() => {
    const source = this.source();
    return source.kind === 'crate' ? source.id : null;
  });

  /** Tracks of the folder or crate being browsed. */
  readonly listTracks = computed(() =>
    this.source().kind === 'crate' ? this.crateTracks() : (this.listing()?.tracks ?? []),
  );

  /** Tracks as shown: those of the list matching the search, or the library's matches; sorted. */
  readonly visibleTracks = computed(() => {
    const words = this.filter().trim().toLowerCase().split(/\s+/).filter(Boolean);
    const shown =
      this.scope() === 'library'
        ? [...this.libraryResults()]
        : this.listTracks().filter((t) => matchesSearch(t, words));
    const { column, direction } = this.sort();
    return shown.sort((a, b) => compare(sortValue(a, column), sortValue(b, column)) * direction);
  });

  async init(): Promise<void> {
    await Promise.all([
      listen<TrackAnalyzed>('track-analyzed', (e) => this.patch(e.payload)),
      listen<AnalysisProgress>('library-progress', (e) => this.progress.set(e.payload)),
    ]);
    this.progress.set(await invoke<AnalysisProgress>('library_progress'));
    await this.crates.refresh();
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

  async openCrate(id: number): Promise<void> {
    await this.attempt(async () => {
      this.crateTracks.set(await this.crates.tracks(id));
      this.browse({ kind: 'crate', id });
    });
  }

  async createCrate(name: string): Promise<void> {
    await this.attempt(async () => this.openCrate(await this.crates.create(name)));
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
      if (this.currentCrate() === id) this.crateTracks.set(await this.crates.tracks(id));
    });
  }

  /** Takes the selected track out of the crate being browsed. */
  async removeSelectedFromCrate(): Promise<void> {
    const id = this.currentCrate();
    const path = this.selected();
    if (id === null || !path || this.scope() !== 'list') return;
    const next = this.neighbour(path);
    await this.attempt(async () => {
      await this.crates.remove(id, [path]);
      this.crateTracks.update((tracks) => tracks.filter((t) => t.path !== path));
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

  private browse(source: TrackSource) {
    this.source.set(source);
    this.scope.set('list');
    this.setFilter('');
    this.error.set(null);
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
    for (const list of [this.crateTracks, this.libraryResults]) {
      if (list().some((t) => t.path === path)) list.update(update);
    }
  }
}

/** Value a column sorts on; tracks without one go last. */
function sortValue(t: DirEntry, column: SortColumn): number | string {
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
  }
}

function compare(a: number | string, b: number | string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}
