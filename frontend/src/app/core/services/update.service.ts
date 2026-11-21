import { Injectable, computed, inject, signal } from '@angular/core';
import { Channel, invoke } from '@tauri-apps/api/core';
import { relaunch } from '@tauri-apps/plugin-process';
import { EngineService } from './engine.service';
import { errorText, isBackendError } from '../errors';
import { ANDROID } from '../platform';
import type { Apk, NewVersion } from '../models/backend';

/** A look shows that it looks at least this long (ms): an answer at once would go unseen. */
const LOOKING_MS = 1200;

/**
 * New versions published on GitHub, installed only when the DJ asks and no deck is playing
 * (the backend refuses meanwhile, the version downloaded kept).
 * On a computer, signed and offered in the title bar; installing restarts Prisme. On Android,
 * the APK of the release, downloaded by Prisme and handed to Android's installer, where the
 * DJ confirms; installing ends Prisme.
 */
@Injectable({ providedIn: 'root' })
export class UpdateService {
  private readonly engine = inject(EngineService);

  readonly available = signal<NewVersion | null>(null);
  /** How the last look went, until the next. */
  readonly looked = signal<'looking' | 'latest' | 'unreachable' | null>(null);
  readonly installing = signal(false);
  /** How much of the new version came, from 0 to 1, while it downloads. */
  readonly progress = signal<number | null>(null);
  /** Android installs the new version once the DJ lets Prisme install applications. */
  readonly needsLeave = signal(false);
  readonly decksPlaying = computed(() => this.engine.status()?.plays ?? false);
  readonly waitsForDecks = computed(() => !!this.available() && this.decksPlaying());
  /** The version downloaded: a deck that started meanwhile leaves it to install once stopped. */
  private downloaded: string | null = null;

  constructor() {
    // back from Android's settings, the leave given: on to its installer
    if (ANDROID) {
      document.addEventListener('visibilitychange', () => {
        if (document.visibilityState === 'visible' && this.needsLeave()) void this.install();
      });
    }
  }

  async look(): Promise<void> {
    this.looked.set('looking');
    const shown = new Promise((done) => setTimeout(done, LOOKING_MS));
    try {
      const found = await invoke<NewVersion | null>('check_update');
      await shown;
      this.available.set(found);
      this.looked.set(found ? null : 'latest');
    } catch {
      await shown;
      this.looked.set('unreachable');
    }
  }

  async install(): Promise<void> {
    const found = this.available();
    if (!found || this.waitsForDecks() || this.installing()) return;
    this.installing.set(true);
    try {
      if (found.apk) await this.handToAndroid(found.apk);
      else await this.installInPlace(found.version);
    } catch (e) {
      // a deck started meanwhile: what came waits for the decks to stop
      if (!isBackendError(e) || e.code !== 'decksPlay') this.engine.error.set(errorText(e));
    } finally {
      this.installing.set(false);
      this.progress.set(null);
    }
  }

  /** Opens Android's setting letting Prisme install applications, for the DJ to allow it. */
  async allow(): Promise<void> {
    await invoke('allow_installs').catch((e) => this.engine.error.set(errorText(e)));
  }

  private async installInPlace(version: string): Promise<void> {
    if (this.downloaded !== version) {
      await invoke('download_update', { progress: this.told() });
      this.downloaded = version;
    }
    this.progress.set(null);
    // by Prisme, which closes first: on Windows the installer ends it at once
    await invoke('install_update');
    await relaunch();
  }

  private async handToAndroid(apk: Apk): Promise<void> {
    await invoke('download_apk', { url: apk.url, size: apk.size, progress: this.told() });
    this.progress.set(null);
    const handed = await invoke<'installer' | 'notAllowed'>('install_apk', { url: apk.url });
    this.needsLeave.set(handed === 'notAllowed');
  }

  /** Where the backend tells how much of the new version came. */
  private told(): Channel<number> {
    this.progress.set(0);
    const progress = new Channel<number>();
    progress.onmessage = (fraction) => this.progress.set(fraction);
    return progress;
  }
}
