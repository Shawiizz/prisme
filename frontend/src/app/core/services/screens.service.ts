import { Injectable, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { PHONE } from '../platform';

const STORAGE_KEY = 'prisme.screenBelow';

/**
 * A screen under the main one, as on a laptop with two screens: Prisme can spread over
 * both, the library on the main screen, the decks on the one below, under the fingers.
 */
@Injectable({ providedIn: 'root' })
export class ScreensService {
  /** There is a screen under the one Prisme is on. */
  readonly below = signal(false);
  /** Spread over both: where the screen below starts in the window (CSS px), and the
   * scale of what it shows, so that it looks as the system scales that screen. */
  readonly span = signal<{ split: number; zoom: number } | null>(null);

  /** Looks for a screen below, and spreads over it again if the DJ chose so last time. */
  async init(): Promise<void> {
    if (PHONE) return;
    this.below.set(await invoke<boolean>('screen_below'));
    if (this.below() && this.chosen()) await this.spread(true);
  }

  async spread(on: boolean): Promise<void> {
    try {
      localStorage.setItem(STORAGE_KEY, String(on));
    } catch {
      // kept for this run only
    }
    this.span.set(
      await invoke<{ split: number; zoom: number } | null>('span_screens', { span: on }),
    );
  }

  private chosen(): boolean {
    try {
      return localStorage.getItem(STORAGE_KEY) === 'true';
    } catch {
      return false;
    }
  }
}
