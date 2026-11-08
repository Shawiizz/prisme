import { Injectable, computed, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { ModelStatus, StemsProgress, StemsStatus } from '../models/stems';
import { LibraryService } from './library.service';

const IDLE: StemsProgress = {
  running: false,
  queued: 0,
  done: 0,
  failed: 0,
  current: null,
  fraction: 0,
  device: null,
};

/**
 * Vocals separation, done in the background by the backend: the model (downloaded once),
 * the queue and its progress. A separated track gets `details.vocals` in the lists.
 */
@Injectable({ providedIn: 'root' })
export class StemsService {
  private readonly library = inject(LibraryService);

  readonly model = signal<ModelStatus>({
    installed: false,
    downloading: null,
    error: null,
    bytes: 0,
  });
  readonly progress = signal<StemsProgress>(IDLE);
  /** Tracks sent to the queue and not separated yet. */
  readonly pending = signal<ReadonlySet<string>>(new Set());
  readonly error = signal<string | null>(null);

  /** The whole queue, as a share of the tracks it held (0 to 1). */
  readonly overall = computed(() => {
    const p = this.progress();
    const total = p.done + p.failed + p.queued;
    return total > 0 ? (p.done + p.failed + (p.current ? p.fraction : 0)) / total : 0;
  });

  async init() {
    await Promise.all([
      listen<StemsProgress>('stems-progress', (e) => this.onProgress(e.payload)),
      listen<ModelStatus>('stems-model', (e) => this.model.set(e.payload)),
      listen<string>('stems-separated', (e) => this.onSeparated(e.payload)),
    ]);
    const status = await invoke<StemsStatus>('stems_status');
    this.model.set(status.model);
    this.progress.set(status.progress);
  }

  /** Whether `path` is being separated, and how far. */
  separating(path: string): number | null {
    const p = this.progress();
    return p.current === path ? p.fraction : null;
  }

  downloadModel() {
    this.model.update((m) => ({ ...m, downloading: m.downloading ?? 0, error: null }));
    return this.call('download_stems_model', {});
  }

  async separate(paths: string[]) {
    this.pending.update((s) => new Set([...s, ...paths]));
    if (!(await this.call('separate_tracks', { paths }))) this.forget(paths);
  }

  separateLibrary() {
    return this.call('separate_library', {});
  }

  cancel() {
    this.pending.set(new Set());
    return this.call('cancel_separation', {});
  }

  private onProgress(progress: StemsProgress) {
    this.progress.set(progress);
    if (!progress.running) this.pending.set(new Set());
  }

  private onSeparated(path: string) {
    this.forget([path]);
    this.library.markSeparated(path);
  }

  private forget(paths: string[]) {
    this.pending.update((s) => new Set([...s].filter((p) => !paths.includes(p))));
  }

  private async call(command: string, args: Record<string, unknown>): Promise<boolean> {
    try {
      this.error.set(null);
      await invoke(command, args);
      return true;
    } catch (e) {
      this.error.set(String(e));
      return false;
    }
  }
}
