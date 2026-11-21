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

/** What an action asks of the library in sight: brought up, left once a track is loaded from
 * it, or brought up and left in turn (the key showing it). */
export type LibrarySight = 'show' | 'done' | 'toggle';

/**
 * What the action `id` done (by a controller, a key, the command palette) asks of the library
 * in sight: its browsing, its search, its lists, its sorts and its preview bring it up; a load
 * leaves it, the track chosen; the key showing it shows or leaves it. A double, the preview
 * stopped and the actions of the decks, the mixer or the effects leave the screen as it is
 * (`null`): the hands are on the controller, the DJ may be choosing a track.
 */
export function librarySight(id: string): LibrarySight | null {
  if (id === 'general.library') return 'toggle';
  if (!id.startsWith('library.')) return null;
  if (/^library\.load\d*(\.play)?$/.test(id)) return 'done';
  if (/^library\.double\d$/.test(id) || id === 'library.preview.stop') return null;
  return 'show';
}

/** What is open over the main screen: the settings, the command palette. */
@Injectable({ providedIn: 'root' })
export class NavigationService {
  private readonly engine = inject(EngineService);
  /** The library over the whole window, the decks out of sight (`general.library`). */
  readonly libraryOnly = computed(() => this.engine.told().libraryOnly);
  readonly settings = signal<SettingsPlace | null>(null);
  readonly palette = signal(false);
  /** What was asked of the library in sight last, each time anew: a phone shows its screen,
   * or the one it was brought over. */
  readonly libraryWanted = signal<{ sight: LibrarySight } | null>(null);
  /** The action a controller or a key did last, each time anew: a phone shows the setting it
   * moves where its screen does not. */
  readonly moved = signal<{ id: string } | null>(null);
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

  /** The library is wanted in sight (a crate opened from the palette, its search). */
  showLibrary() {
    this.libraryWanted.set({ sight: 'show' });
  }

  /** The action `id` was done (the backend tells those of the library, whoever asked, and the
   * settings a controller moves): the library brought into sight or left as it asks
   * (`librarySight`), else the setting it moves shown. */
  done(id: string) {
    const sight = librarySight(id);
    if (sight) this.libraryWanted.set({ sight });
    else this.moved.set({ id });
  }
}
