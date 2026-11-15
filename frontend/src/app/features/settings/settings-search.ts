import { Injectable, computed, signal } from '@angular/core';
import { SettingsCategory } from '../../core/services/navigation.service';
import { SETTINGS, SETTINGS_PAGES, SETTING_BY_ID, matches, normalize } from './settings-catalog';

/** Pages that are panels rather than rows: shown whole when the search finds them. */
const PANEL_PAGES: readonly SettingsCategory[] = ['controllers', 'shortcuts'];

/** The search over the settings and the setting pointed out: the window's, read by its rows. */
@Injectable()
export class SettingsSearch {
  readonly query = signal('');
  /** The setting pointed out (from the command palette), lit once. */
  readonly pointed = signal<string | null>(null);

  /** What the search finds, `null` without a search: pages by what they are, settings by
   * theirs, and the pages holding a setting found. */
  readonly found = computed(() => {
    const words = normalize(this.query()).split(/\s+/).filter(Boolean);
    if (!words.length) return null;
    const pages = SETTINGS_PAGES.filter((p) =>
      matches(words, `${p.label()} ${p.description()} ${p.keywords}`),
    );
    const settings = SETTINGS.filter((s) => matches(words, `${s.label()} ${s.description()}`));
    return {
      pages: new Set(pages.map((p) => p.id)),
      settings: new Set(settings.map((s) => s.id)),
      holding: new Set(settings.map((s) => s.page)),
    };
  });
  /** The settings found, a panel found counting as one. */
  readonly count = computed(() => {
    const found = this.found();
    if (!found) return 0;
    return found.settings.size + PANEL_PAGES.filter((p) => found.pages.has(p)).length;
  });

  /** Whether a setting's row shows: found, or on a page found. */
  finds(id: string): boolean {
    const found = this.found();
    if (!found) return true;
    const page = SETTING_BY_ID.get(id)?.page;
    return found.settings.has(id) || (page !== undefined && found.pages.has(page));
  }
}
