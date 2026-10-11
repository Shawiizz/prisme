import { Injectable, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { DjPreferences, LibraryPreferences, Preferences } from '../models/preferences';

/** Preferences kept by the backend from one start to the next. */
@Injectable({ providedIn: 'root' })
export class PreferencesService {
  readonly dj = signal<DjPreferences>({ vinylSync: 'once', quantize: true, pitchRange: 0.08 });
  readonly library = signal<LibraryPreferences>({
    lastFolder: null,
    hiddenColumns: [],
    writeTags: true,
  });

  async load(): Promise<void> {
    const p = await invoke<Preferences>('get_preferences');
    this.dj.set(p.dj);
    this.library.set(p.library);
  }

  updateDj(changes: Partial<DjPreferences>): Promise<void> {
    const dj = { ...this.dj(), ...changes };
    this.dj.set(dj);
    return invoke('set_dj_preferences', { dj });
  }

  updateLibrary(changes: Partial<LibraryPreferences>): Promise<void> {
    const library = { ...this.library(), ...changes };
    this.library.set(library);
    return invoke('set_library_preferences', { library });
  }
}
