import { Injectable, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { PHONE } from '../platform';
import { PreferencesService } from './preferences.service';
import type { Span } from '../models/backend';

/**
 * A screen under the main one, as on a laptop with two screens: Prisme can spread over
 * both, the library on the main screen, the decks on the one below, under the fingers.
 */
@Injectable({ providedIn: 'root' })
export class ScreensService {
  private readonly preferences = inject(PreferencesService);
  /** There is a screen under the one Prisme is on. */
  readonly below = signal(false);
  /** Spread over both: where the screen below starts in the window (CSS px), and the
   * scale of what it shows, so that it looks as the system scales that screen. */
  readonly span = signal<{ split: number; zoom: number } | null>(null);

  /** Looks for a screen below, and spreads over it again if the DJ chose so last time. */
  async init(): Promise<void> {
    if (PHONE) return;
    this.below.set(await invoke<boolean>('screen_below'));
    if (this.below() && this.preferences.ui().screenBelow) await this.spread(true);
  }

  /** Spreads over the screen below or no longer: the backend keeps the choice. */
  async spread(on: boolean): Promise<void> {
    this.span.set(await invoke<Span | null>('span_screens', { span: on }));
  }
}
