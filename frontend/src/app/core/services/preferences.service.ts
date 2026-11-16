import { Injectable, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { PHONE } from '../platform';
import { DjPreferences, LibraryPreferences, Preferences } from '../models/preferences';
import { KeyChanges } from '../keyboard/shortcuts';

/** Preferences kept by the backend from one start to the next. */
@Injectable({ providedIn: 'root' })
export class PreferencesService {
  readonly dj = signal<DjPreferences>({
    vinylSync: 'once',
    quantize: true,
    pitchRange: 0.08,
    autoGain: true,
    deckCount: 2,
    knobMagnet: true,
    lockControls: true,
    cueOnLoad: false,
    protectPlaying: false,
  });
  readonly library = signal<LibraryPreferences>({
    lastFolder: null,
    hiddenColumns: [],
    writeTags: true,
    replaceTags: false,
    pauseWhilePlaying: true,
    stemsOnLoad: null,
    stemsGpu: true,
    stemsFast: PHONE,
  });
  /** The keys the DJ changed, by control: the others keep their defaults. */
  readonly keys = signal<KeyChanges>({});

  async load(): Promise<void> {
    const p = await invoke<Preferences>('get_preferences');
    this.dj.set(p.dj);
    this.library.set(p.library);
    this.keys.set(p.keys);
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

  updateKeys(keys: KeyChanges): Promise<void> {
    this.keys.set(keys);
    return invoke('set_key_preferences', { keys });
  }
}
