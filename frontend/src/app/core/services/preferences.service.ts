import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { showLanguage } from '../i18n/i18n';
import { EngineService } from './engine.service';
import type { Key, Preferences, Setting, TakenKey } from '../models/backend';

/** Each setting the settings change, and its value. */
export type Settings = { [S in Setting as S['setting']]: S['value'] };

const told = signal<Preferences | null>(null);

/** The preferences as the backend told them (`loadPreferences`, or a test). */
export function tellPreferences(preferences: Preferences) {
  told.set(preferences);
}

/** Where the interface kept the language and the phone's touch controls before the backend kept
 * them: taken over once, then forgotten. The screen below it kept is gone with the way the decks
 * went on it: forgotten too. */
const STORED = {
  language: 'prisme.language',
  phoneTouch: 'prisme.phone.touch',
} as const;
const FORGOTTEN = 'prisme.screenBelow';

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
  ];
  try {
    localStorage.removeItem(FORGOTTEN);
  } catch {
    // no storage: nothing kept
  }
  if (taken.includes(true)) tellPreferences(await invoke<Preferences>('get_preferences'));
  await showLanguage(told()!.ui.language);
}

/** What the page's storage kept under `key`, given to the backend by `give` unless it says
 * `false` (not wanted), then forgotten: whether it was given. Kept for the next start when the
 * backend could not take it, the interface starting all the same. */
async function takeOver(
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

  /** As the status stream tells them, else as read before the interface started
   * (`loadPreferences`): one or the other is there once the backend told what it mirrors. */
  private readonly current = computed(() => this.engine.controls()?.preferences ?? told()!);
  readonly dj = computed(() => this.current().dj);
  readonly library = computed(() => this.current().library);
  readonly ui = computed(() => this.current().ui);
  readonly updates = computed(() => this.current().updates);
  /** The keyboard's shortcuts in use. */
  readonly shortcuts = computed(() => this.current().shortcuts);
  /** The actions whose keys the DJ changed. */
  readonly changed = computed(() => this.current().changed);
  /** What the settings offer where there are a few values. */
  readonly choices = computed(() => this.current().choices);

  constructor() {
    effect(() => {
      const language = (this.engine.controls()?.preferences ?? told())?.ui.language;
      if (language !== undefined) untracked(() => void showLanguage(language));
    });
  }

  /** `keys` given to `control` (`null`: Prisme's), taken from the actions that have them if
   * `take`: until then, those they would take are told back, nothing changed. */
  setKeys(control: string, keys: Key[] | null, take: boolean): Promise<TakenKey[]> {
    return invoke<TakenKey[]>('set_keys', { control, keys, take });
  }

  /** `setting` changed to `value`: shown once the backend tells it. */
  set<K extends keyof Settings>(setting: K, value: Settings[K]): Promise<void> {
    return choose(setting, value);
  }
}
