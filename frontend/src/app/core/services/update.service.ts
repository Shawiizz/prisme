import { Injectable, computed, inject, signal } from '@angular/core';
import { getVersion } from '@tauri-apps/api/app';
import { openUrl } from '@tauri-apps/plugin-opener';
import { relaunch } from '@tauri-apps/plugin-process';
import { Update, check } from '@tauri-apps/plugin-updater';
import { EngineService } from './engine.service';
import { errorText } from '../errors';
import { ANDROID } from '../platform';

/** The latest release, from GitHub's API. */
const LATEST = 'https://api.github.com/repos/Shawiizz/prisme/releases/latest';
/** A look shows that it looks at least this long (ms): an answer at once would go unseen. */
const LOOKING_MS = 1200;

/** A newer version: installed in place on a computer, its APK downloaded on a phone. */
export interface NewVersion {
  version: string;
  update: Update | null;
  apk: string | null;
}

/**
 * New versions published on GitHub. On a computer, signed and offered in the title bar,
 * installed only when the DJ asks and no deck is playing (installing restarts Prisme). On a
 * phone, the APK of the release, downloaded by the browser and installed from there.
 */
@Injectable({ providedIn: 'root' })
export class UpdateService {
  private readonly engine = inject(EngineService);

  readonly available = signal<NewVersion | null>(null);
  /** How the last look went, until the next. */
  readonly looked = signal<'looking' | 'latest' | 'unreachable' | null>(null);
  readonly installing = signal(false);
  readonly decksPlaying = computed(
    () => this.engine.status()?.decks.some((d) => d.playing || Math.abs(d.speed) > 0.1) ?? false,
  );
  /** Installed in place, restarting Prisme: not while a deck plays. */
  readonly waitsForDecks = computed(() => !!this.available()?.update && this.decksPlaying());

  async look(): Promise<void> {
    this.looked.set('looking');
    const shown = new Promise((done) => setTimeout(done, LOOKING_MS));
    try {
      const found = await (ANDROID ? newerApk() : newerUpdate());
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
    if (!found || this.waitsForDecks()) return;
    if (found.apk) {
      await openUrl(found.apk).catch((e) => this.engine.error.set(errorText(e)));
      return;
    }
    this.installing.set(true);
    try {
      await found.update?.downloadAndInstall();
      await relaunch();
    } catch (e) {
      this.engine.error.set(errorText(e));
      this.installing.set(false);
    }
  }
}

async function newerUpdate(): Promise<NewVersion | null> {
  const update = await check();
  return update && { version: update.version, update, apk: null };
}

async function newerApk(): Promise<NewVersion | null> {
  const response = await fetch(LATEST, { headers: { Accept: 'application/vnd.github+json' } });
  if (!response.ok) throw new Error(`GitHub answered ${response.status}`);
  const release = (await response.json()) as {
    tag_name: string;
    assets: { name: string; browser_download_url: string }[];
  };
  const apk = release.assets.find((a) => a.name.endsWith('.apk'));
  if (!apk || !isNewer(release.tag_name, await getVersion())) return null;
  return { version: release.tag_name, update: null, apk: apk.browser_download_url };
}

/** Whether version `a` (1.2.3) comes after `b`. */
export function isNewer(a: string, b: string): boolean {
  const [x, y] = [a, b].map((v) => v.split('.').map(Number));
  for (let i = 0; i < Math.max(x.length, y.length); i++) {
    if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) > (y[i] ?? 0);
  }
  return false;
}
