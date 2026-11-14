import { Injectable, computed, signal } from '@angular/core';
import { REOPEN_KEY } from '../i18n/i18n';

/** The pages of the settings. */
export type SettingsCategory =
  | 'audio'
  | 'turntables'
  | 'mixing'
  | 'dj'
  | 'library'
  | 'controllers'
  | 'shortcuts'
  | 'interface'
  | 'about';

/** Where the settings are open: a page, and the setting to point out on it. */
export interface SettingsPlace {
  category: SettingsCategory;
  setting: string | null;
}

/** The page of the settings the UI was on when it started again in another language. */
function reopened(): SettingsPlace | null {
  try {
    const category = sessionStorage.getItem(REOPEN_KEY) as SettingsCategory | null;
    sessionStorage.removeItem(REOPEN_KEY);
    return category ? { category, setting: null } : null;
  } catch {
    return null;
  }
}

/** What is open over the main screen: the settings, the command palette. */
@Injectable({ providedIn: 'root' })
export class NavigationService {
  readonly settings = signal<SettingsPlace | null>(reopened());
  readonly palette = signal(false);
  /** The main screen is covered: its keyboard shortcuts wait. */
  readonly covered = computed(() => this.settings() !== null || this.palette());

  openSettings(category: SettingsCategory = 'audio', setting: string | null = null) {
    this.palette.set(false);
    this.settings.set({ category, setting });
  }

  closeSettings() {
    this.settings.set(null);
  }

  /** The page is open: close it; otherwise open it. */
  toggleSettings(category: SettingsCategory) {
    if (this.settings()?.category === category) this.closeSettings();
    else this.openSettings(category);
  }

  togglePalette() {
    this.palette.update((open) => !open);
  }
}
