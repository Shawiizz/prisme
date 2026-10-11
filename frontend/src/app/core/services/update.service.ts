import { Injectable, computed, inject, signal } from '@angular/core';
import { relaunch } from '@tauri-apps/plugin-process';
import { Update, check } from '@tauri-apps/plugin-updater';
import { EngineService } from './engine.service';

/**
 * New versions published on GitHub, signed: offered in the title bar, installed only when
 * the DJ asks and no deck is playing (installing restarts Prisme).
 */
@Injectable({ providedIn: 'root' })
export class UpdateService {
  private readonly engine = inject(EngineService);

  readonly available = signal<Update | null>(null);
  readonly installing = signal(false);
  readonly decksPlaying = computed(
    () => this.engine.status()?.decks.some((d) => d.playing || Math.abs(d.speed) > 0.1) ?? false,
  );

  async look(): Promise<void> {
    try {
      this.available.set(await check());
    } catch {
      // offline, or nothing published yet: nothing to offer
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
      this.engine.error.set(String(e));
      this.installing.set(false);
    }
  }
}
