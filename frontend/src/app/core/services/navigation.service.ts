import { Injectable, computed, inject, signal } from '@angular/core';
import { EngineService } from './engine.service';

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
  private readonly engine = inject(EngineService);
  /** The library over the whole window, the decks out of sight (`general.library`). */
  readonly libraryOnly = computed(() => this.engine.controls()?.libraryOnly ?? false);
  readonly settings = signal<SettingsPlace | null>(null);
  readonly palette = signal(false);
  /** Times the library was asked for from elsewhere: a phone shows it then. */
  readonly libraryAsked = signal(0);
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

  /** The library is wanted in sight (a crate opened from the palette). */
  showLibrary() {
    this.libraryAsked.update((n) => n + 1);
  }
}
