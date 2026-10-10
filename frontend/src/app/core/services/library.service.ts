import { Injectable, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { DirListing } from '../models/library';

/** Browsing the music folders on disk. */
@Injectable({ providedIn: 'root' })
export class LibraryService {
  readonly root = signal<string | null>(null);
  readonly listing = signal<DirListing | null>(null);
  readonly error = signal<string | null>(null);

  async init(): Promise<void> {
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
}
