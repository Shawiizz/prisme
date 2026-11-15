import { Injectable, computed, inject, signal } from '@angular/core';
import { relaunch } from '@tauri-apps/plugin-process';
import { Update, check } from '@tauri-apps/plugin-updater';
import { EngineService } from './engine.service';
import { errorText } from '../errors';

/**
 * New versions published on GitHub, signed: offered in the title bar, installed only when
 * the DJ asks and no deck is playing (installing restarts Prisme).
 */
@Injectable({ providedIn: 'root' })
export class UpdateService {
  private readonly engine = inject(EngineService);

  readonly available = signal<Update | null>(null);
  /** How the last look went, until the next. */
  readonly looked = signal<'looking' | 'latest' | 'unreachable' | null>(null);
  readonly installing = signal(false);
  readonly decksPlaying = computed(
    () => this.engine.status()?.decks.some((d) => d.playing || Math.abs(d.speed) > 0.1) ?? false,
  );

  async look(): Promise<void> {
    this.looked.set('looking');
    try {
      const update = await check();
      this.available.set(update);
      this.looked.set(update ? null : 'latest');
    } catch {
      this.looked.set('unreachable');
    }
  }

  async install(): Promise<void> {
    const update = this.available();
    if (!update || this.decksPlaying()) return;
    this.installing.set(true);
    try {
      await update.downloadAndInstall();
      await relaunch();
    } catch (e) {
      this.engine.error.set(errorText(e));
      this.installing.set(false);
    }
  }
}
