import { Injectable, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { GridChange, MusicalKey, TrackAnalysis } from '../models/engine';
import { EngineService } from './engine.service';

/** Corrections of a loaded track's beat grid and key (kept by the library). */
@Injectable({ providedIn: 'root' })
export class TrackEditService {
  private readonly engine = inject(EngineService);

  /** The 24 keys in Camelot order, once loaded. */
  readonly keys = signal<MusicalKey[]>([]);

  async loadKeys(): Promise<void> {
    if (!this.keys().length) this.keys.set(await invoke<MusicalKey[]>('key_choices'));
  }

  editGrid(deck: number, change: GridChange): Promise<void> {
    return this.apply(deck, 'edit_grid', { deck, change });
  }

  /** `null` goes back to the detected key. */
  setKey(deck: number, camelot: string | null): Promise<void> {
    return this.apply(deck, 'set_key', { deck, camelot });
  }

  private async apply(deck: number, command: string, args: Record<string, unknown>) {
    try {
      this.engine.setAnalysis(deck, await invoke<TrackAnalysis>(command, args));
      this.engine.error.set(null);
    } catch (e) {
      this.engine.error.set(String(e));
    }
  }
}
