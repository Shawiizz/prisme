import { Injectable, computed, signal } from '@angular/core';

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

/** What is open over the main screen: the settings, the command palette. */
@Injectable({ providedIn: 'root' })
export class NavigationService {
  readonly settings = signal<SettingsPlace | null>(null);
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
