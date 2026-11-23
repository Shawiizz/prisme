import { Injectable, computed, inject, signal } from '@angular/core';
import { Channel, invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { EngineService } from './engine.service';
import { PreferencesService } from './preferences.service';
import { errorText, isBackendError } from '../errors';
import { ANDROID, PHONE } from '../platform';
import type { Apk, NewVersion, UpdateState } from '../models/backend';

/** What the backend tells when the state of the updates changes. */
const UPDATE_EVENT = 'update-state';
/** A look shows that it looks at least this long (ms): an answer at once would go unseen. */
const LOOKING_MS = 1200;

/** What the title bar offers of a new version. */
export type UpdateOffer =
  | { kind: 'restart'; version: string }
  | { kind: 'download'; version: string }
  | { kind: 'downloading'; version: string; progress: number };

/**
 * Prisme's new versions. On a computer the backend looks for them, downloads them in the
 * background (or when the DJ asks, automatic updates off) and puts them in place for the next
 * start (`state`): the DJ may restart into one at once, the decks stopped. On Android, the APK
 * of the release, downloaded by Prisme and handed to Android's installer, where the DJ
 * confirms; installing ends Prisme.
 */
@Injectable({ providedIn: 'root' })
export class UpdateService {
  private readonly engine = inject(EngineService);
  private readonly preferences = inject(PreferencesService);

  /** On a computer: where the new versions stand, as the backend tells it. */
  readonly state = signal<UpdateState>({ stage: 'unknown' });
  readonly decksPlaying = computed(() => this.engine.status()?.plays ?? false);
  /** On a computer: what the title bar offers, a download under way only when the DJ asked
   * for it. */
  readonly offer = computed<UpdateOffer | null>(() => {
    const state = this.state();
    switch (state.stage) {
      case 'ready':
        return { kind: 'restart', version: state.version };
      case 'available':
        return state.installsItself ? { kind: 'download', version: state.version } : null;
      case 'downloading':
        return this.preferences.updates().automatic
          ? null
          : { kind: 'downloading', version: state.version, progress: state.progress };
      default:
        return null;
    }
  });

  /** On Android: the newer version's APK, once looked for. */
  readonly apk = signal<NewVersion | null>(null);
  /** On Android: how the last look went, until the next. */
  readonly looked = signal<'looking' | 'latest' | 'unreachable' | null>(null);
  readonly installing = signal(false);
  /** How much of the APK came, from 0 to 1, while it downloads. */
  readonly progress = signal<number | null>(null);
  /** Android installs the new version once the DJ lets Prisme install applications. */
  readonly needsLeave = signal(false);
  readonly waitsForDecks = computed(() => !!this.apk() && this.decksPlaying());

  constructor() {
    // back from Android's settings, the leave given: on to its installer
    if (ANDROID) {
      document.addEventListener('visibilitychange', () => {
        if (document.visibilityState === 'visible' && this.needsLeave()) void this.install();
      });
    }
    if (!PHONE) {
      listen<UpdateState>(UPDATE_EVENT, ({ payload }) => this.state.set(payload))
        .then(() => invoke<UpdateState>('update_state'))
        .then((state) => this.state.set(state))
        .catch((e) => console.error(e));
    }
  }

  /** Looks for a new version now. */
  async look(): Promise<void> {
    if (!PHONE) {
      await invoke('look_for_update').catch((e) => this.engine.error.set(errorText(e)));
      return;
    }
    this.looked.set('looking');
    const shown = new Promise((done) => setTimeout(done, LOOKING_MS));
    try {
      const found = await invoke<NewVersion | null>('check_update');
      await shown;
      this.apk.set(found);
      this.looked.set(found ? null : 'latest');
    } catch {
      await shown;
      this.looked.set('unreachable');
    }
  }

  /** On a computer: downloads the new version found, automatic updates off. */
  async download(): Promise<void> {
    await invoke('download_update').catch((e) => this.engine.error.set(errorText(e)));
  }

  /** On a computer: quits and opens the new version in place, the decks stopped. */
  async restart(): Promise<void> {
    if (this.decksPlaying()) return;
    await invoke('restart_to_update').catch((e) => {
      if (!isBackendError(e) || e.code !== 'decksPlay') this.engine.error.set(errorText(e));
    });
  }

  /** On Android: downloads the APK and hands it to Android's installer, the decks stopped. */
  async install(): Promise<void> {
    const found = this.apk();
    if (!found || this.waitsForDecks() || this.installing()) return;
    this.installing.set(true);
    try {
      await this.handToAndroid(found.apk);
    } catch (e) {
      // a deck started meanwhile: the APK waits for the decks to stop
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

  private async handToAndroid(apk: Apk): Promise<void> {
    await invoke('download_apk', { url: apk.url, size: apk.size, progress: this.told() });
    this.progress.set(null);
    const handed = await invoke<'installer' | 'notAllowed'>('install_apk', { url: apk.url });
    this.needsLeave.set(handed === 'notAllowed');
  }

  /** Where the backend tells how much of the APK came. */
  private told(): Channel<number> {
    this.progress.set(0);
    const progress = new Channel<number>();
    progress.onmessage = (fraction) => this.progress.set(fraction);
    return progress;
  }
}
