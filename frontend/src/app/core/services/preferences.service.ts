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

/** Where the interface kept the language, the phone's touch controls and the screen below
 * before the backend kept them: taken over once, then forgotten. */
const STORED = {
  language: 'prisme.language',
  phoneTouch: 'prisme.phone.touch',
  screenBelow: 'prisme.screenBelow',
} as const;

/** The preferences as the backend keeps them, read before the interface starts, the language
 * shown in theirs. */
export async function loadPreferences(): Promise<void> {
  tellPreferences(await invoke<Preferences>('get_preferences'));
  const ui = told()!.ui;
  const taken = [
    await takeOver(STORED.language, (saved) => !ui.language && choose('language', saved)),
    await takeOver(
      STORED.phoneTouch,
      (saved) => ui.phoneTouch === null && choose('phoneTouch', saved === 'true'),
    ),
    await takeOver(
      STORED.screenBelow,
      (saved) => saved === 'true' && !ui.screenBelow && choose('screenBelow', true),
    ),
  ];
  if (taken.includes(true)) tellPreferences(await invoke<Preferences>('get_preferences'));
  await showLanguage(told()!.ui.language);
}

/** What the page's storage kept under `key`, given to the backend by `give` unless it says
 * `false` (not wanted), then forgotten: whether it was given. Kept for the next start when the
 * backend could not take it, the interface starting all the same. */
export async function takeOver(
  key: string,
  give: (saved: string) => false | Promise<void>,
): Promise<boolean> {
  try {
    const saved = localStorage.getItem(key);
    if (saved === null) return false;
    const given = give(saved);
    if (given) await given;
    localStorage.removeItem(key);
    return given !== false;
  } catch (err) {
    console.error(err);
    return false;
  }
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
