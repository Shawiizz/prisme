import { Injectable, computed, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { AnalysisProgress, DirEntry, DirListing, TrackAnalyzed } from '../models/library';
import { camelotRank } from '../../shared/camelot';
import { EngineService } from './engine.service';
import { PreferencesService } from './preferences.service';

export type SortColumn = 'name' | 'bpm' | 'key';

export interface TrackSort {
  column: SortColumn;
  direction: 1 | -1;
}

/**
 * Browsing the music folders on disk, with the analysis results of the tracks, and the
 * view on them (filter, sort, selection). The background analysis pushes each result
 * (`track-analyzed`) and its progress (`library-progress`): the listing is patched in
 * place, no reload needed.
 */
@Injectable({ providedIn: 'root' })
export class LibraryService {
  private readonly preferences = inject(PreferencesService);
  private readonly engine = inject(EngineService);

  readonly root = signal<string | null>(null);
  readonly listing = signal<DirListing | null>(null);
  readonly error = signal<string | null>(null);
  readonly progress = signal<AnalysisProgress>({ total: 0, done: 0, failed: 0, running: false });
  readonly filter = signal('');
  readonly sort = signal<TrackSort>({ column: 'name', direction: 1 });
  readonly selected = signal<string | null>(null);
  /** Incremented to ask the view to focus the search field. */
  readonly searchRequests = signal(0);

  /** Tracks of the folder as shown: filtered, then sorted. */
  readonly visibleTracks = computed(() => {
    const all = this.listing()?.tracks ?? [];
    const f = this.filter().trim().toLowerCase();
    const shown = f ? all.filter((t) => t.name.toLowerCase().includes(f)) : [...all];
    const { column, direction } = this.sort();
    const value = (t: DirEntry): number | string => {
      switch (column) {
        case 'bpm':
          return t.analysis?.bpm ?? Number.MAX_SAFE_INTEGER;
        case 'key':
          return camelotRank(t.analysis?.key?.camelot);
        default:
          return t.name.toLowerCase();
      }
    };
    return shown.sort((a, b) => {
      const va = value(a);
      const vb = value(b);
      return (va < vb ? -1 : va > vb ? 1 : 0) * direction;
    });
  });

  async init(): Promise<void> {
    await Promise.all([
      listen<TrackAnalyzed>('track-analyzed', (e) => this.patch(e.payload)),
      listen<AnalysisProgress>('library-progress', (e) => this.progress.set(e.payload)),
    ]);
    this.progress.set(await invoke<AnalysisProgress>('library_progress'));
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
      this.filter.set('');
      this.error.set(null);
      await this.preferences.updateLibrary({ lastFolder: path });
      return true;
    } catch (e) {
      this.error.set(String(e));
      return false;
    }
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

  private patch({ path, analysis }: TrackAnalyzed) {
    const listing = this.listing();
    if (!listing?.tracks.some((t) => t.path === path)) return;
    this.listing.set({ ...listing, tracks: listing.tracks.map((t) => (t.path === path ? { ...t, analysis } : t)) });
  }
}
