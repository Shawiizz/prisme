import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { Preferences, Settings } from '../models/preferences';
import { showLanguage } from '../i18n/i18n';
import { EngineService } from './engine.service';

const told = signal<Preferences | null>(null);

/** The preferences as the backend told them (`loadPreferences`, or a test). */
export function tellPreferences(preferences: Preferences) {
  told.set(preferences);
}

/** Where the interface kept the language and the phone's touch controls before the backend
 * kept them: taken over once, then forgotten. */
const STORED = { language: 'prisme.language', phoneTouch: 'prisme.phone.touch' } as const;

/** What the page's storage kept under `key`, forgotten. */
export function takeStored(key: string): string | null {
  try {
    const saved = localStorage.getItem(key);
    localStorage.removeItem(key);
    return saved;
  } catch {
    return null;
  }
}

/** The preferences as the backend keeps them, read before the interface starts, the language
 * shown in theirs. */
export async function loadPreferences(): Promise<void> {
  tellPreferences(await invoke<Preferences>('get_preferences'));
  const language = takeStored(STORED.language);
  if (language && !told()!.ui.language) await choose('language', language);
  const touch = takeStored(STORED.phoneTouch);
  if (touch && told()!.ui.phoneTouch === null) await choose('phoneTouch', touch === 'true');
  await showLanguage(told()!.ui.language ?? language);
}

function choose<K extends keyof Settings>(setting: K, value: Settings[K]): Promise<void> {
  return invoke('set_preference', { setting: { setting, value } });
}

/** Preferences kept by the backend from one start to the next, as it tells them: each change
 * is one setting it puts in them. */
@Injectable({ providedIn: 'root' })
export class PreferencesService {
  private readonly engine = inject(EngineService);

  // read before the interface starts (`loadPreferences`)
  readonly dj = computed(() => told()!.dj);
  readonly library = computed(() => told()!.library);
  readonly ui = computed(() => told()!.ui);
  /** The keyboard's shortcuts in use. */
  readonly shortcuts = computed(() => told()!.shortcuts);
  /** The actions whose keys the DJ changed. */
  readonly changed = computed(() => told()!.changed);
  /** What the settings offer where there are a few values. */
  readonly choices = computed(() => told()!.choices);

  constructor() {
    effect(() => {
      const preferences = this.engine.controls()?.preferences;
      if (preferences) untracked(() => told.set(preferences));
    });
    effect(() => {
      const language = told()?.ui.language;
      if (language !== undefined) untracked(() => void showLanguage(language));
    });
  }

  /** `setting` changed to `value`: shown once the backend tells it. */
  set<K extends keyof Settings>(setting: K, value: Settings[K]): Promise<void> {
    return choose(setting, value);
  }
}
