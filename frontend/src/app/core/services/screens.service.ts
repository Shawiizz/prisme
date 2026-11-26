import { Injectable, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Screen } from '../models/backend';

/**
 * The screens plugged in, and the decks in a window of their own on the one the DJ chose, full
 * screen there, the main window apart from them (`commands/screens.rs`): a laptop's second
 * screen, a touch screen beside it.
 */
@Injectable({ providedIn: 'root' })
export class ScreensService {
  /** As last looked at. */
  readonly screens = signal<Screen[]>([]);
  /** The decks are in a window of their own. */
  readonly decksApart = signal(false);

  /** Follows where the decks are: their screen unplugged, they come back. */
  async init(): Promise<void> {
    await listen<boolean>('decks-apart', ({ payload }) => this.decksApart.set(payload));
    this.decksApart.set(await invoke<boolean>('decks_apart'));
  }

  /** The screens plugged in now. */
  async look(): Promise<void> {
    this.screens.set(await invoke<Screen[]>('list_screens'));
  }

  /** The decks on `screen` (its id), or back in the main window (`null`). */
  async putDecksOn(screen: string | null): Promise<void> {
    await invoke('set_decks_screen', { screen });
    await this.look();
  }
}

/** The screens numbered as read: from the top, then from the left (the system's own numbers
 * are not those its settings show). */
export function numbered(screens: readonly Screen[]): { screen: Screen; number: number }[] {
  const order = [...screens].sort((a, b) => a.y - b.y || a.x - b.x);
  return screens.map((screen) => ({ screen, number: order.indexOf(screen) + 1 }));
}
