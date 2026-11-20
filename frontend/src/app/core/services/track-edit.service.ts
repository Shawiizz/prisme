import { Injectable, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { GridChange, MusicalKey } from '../models/engine';
import { EngineService } from './engine.service';

/** Corrections of a loaded track's beat grid and key (kept by the library): the decks playing
 * the track show them as the backend tells it. */
@Injectable({ providedIn: 'root' })
export class TrackEditService {
  private readonly engine = inject(EngineService);

  /** The 24 keys in Camelot order, once loaded. */
  readonly keys = signal<MusicalKey[]>([]);

  async loadKeys(): Promise<void> {
    if (!this.keys().length) this.keys.set(await invoke<MusicalKey[]>('key_choices'));
  }

  editGrid(deck: number, change: GridChange) {
    return this.engine.call('edit_grid', { deck, change });
  }

  /** `null` goes back to the detected key. */
  setKey(deck: number, camelot: string | null) {
    return this.engine.call('set_key', { deck, camelot });
  }
}
