import { Injectable, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { AnalysisProgress, DirListing, TrackAnalyzed } from '../models/library';
import { PreferencesService } from './preferences.service';

/**
 * Browsing the music folders on disk, with the analysis results of the tracks. The
 * background analysis pushes each result (`track-analyzed`) and its progress
 * (`library-progress`): the listing is patched in place, no reload needed.
 */
@Injectable({ providedIn: 'root' })
export class LibraryService {
  private readonly preferences = inject(PreferencesService);

  readonly root = signal<string | null>(null);
  readonly listing = signal<DirListing | null>(null);
  readonly error = signal<string | null>(null);
  readonly progress = signal<AnalysisProgress>({ total: 0, done: 0, failed: 0, running: false });

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

  private patch({ path, analysis }: TrackAnalyzed) {
    const listing = this.listing();
    if (!listing?.tracks.some((t) => t.path === path)) return;
    this.listing.set({ ...listing, tracks: listing.tracks.map((t) => (t.path === path ? { ...t, analysis } : t)) });
  }
}
