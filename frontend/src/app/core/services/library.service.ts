import { Injectable, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { AnalysisProgress, DirListing, TrackAnalyzed } from '../models/library';

/**
 * Browsing the music folders on disk, with the analysis results of the tracks. The
 * background analysis pushes each result (`track-analyzed`) and its progress
 * (`library-progress`): the listing is patched in place, no reload needed.
 */
@Injectable({ providedIn: 'root' })
export class LibraryService {
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
    await this.open(root);
  }

  async open(path: string): Promise<void> {
    try {
      this.listing.set(await invoke<DirListing>('list_dir', { path }));
      this.error.set(null);
    } catch (e) {
      this.error.set(String(e));
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
