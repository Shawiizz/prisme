import { Injectable, computed, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  ModelId,
  ModelStatus,
  Separated,
  Separation,
  StemsProgress,
  StemsStatus,
} from '../models/stems';
import { LibraryService } from './library.service';
import { PreferencesService } from './preferences.service';

const IDLE: StemsProgress = {
  running: false,
  queued: 0,
  done: 0,
  failed: 0,
  current: null,
  fraction: 0,
  device: null,
};

const MISSING = (id: ModelId): ModelStatus => ({
  id,
  installed: false,
  downloading: null,
  error: null,
  bytes: 0,
});

/**
 * Stem separation, done in the background by the backend: the models (each downloaded
 * once), the queue and its progress. A separated track gets `details.separation` in the lists.
 */
@Injectable({ providedIn: 'root' })
export class StemsService {
  private readonly library = inject(LibraryService);
  private readonly preferences = inject(PreferencesService);

  readonly vocalsModel = signal<ModelStatus>(MISSING('vocals'));
  readonly drumsBassModel = signal<ModelStatus>(MISSING('drums-bass'));
  readonly fourFastModel = signal<ModelStatus>(MISSING('four-fast'));
  readonly progress = signal<StemsProgress>(IDLE);
  /** Whether a graphics card can separate; `null` until known. */
  readonly gpu = signal<boolean | null>(null);
  /** Tracks sent to the queue and not separated yet. */
  readonly pending = signal<ReadonlySet<string>>(new Set());
  readonly error = signal<string | null>(null);

  /**
   * How far the installed models separate a track: the small one for every stem when the DJ
   * prefers it or no large one is there, else the large ones.
   */
  readonly reach = computed<Separation>(() => {
    const vocals = this.vocalsModel().installed;
    const drumsBass = this.drumsBassModel().installed;
    const fast = this.preferences.library().stemsFast;
    if (this.fourFastModel().installed && (fast || !(vocals || drumsBass))) return 'full';
    if (!vocals) return 'none';
    return drumsBass ? 'full' : 'vocals';
  });

  /** The whole queue, as a share of the tracks it held (0 to 1). */
  readonly overall = computed(() => {
    const p = this.progress();
    const total = p.done + p.failed + p.queued;
    return total > 0 ? (p.done + p.failed + (p.current ? p.fraction : 0)) / total : 0;
  });

  async init() {
    await Promise.all([
      listen<StemsProgress>('stems-progress', (e) => this.onProgress(e.payload)),
      listen<ModelStatus>('stems-model', (e) => this.setModel(e.payload)),
      listen<Separated>('stems-separated', (e) => this.onSeparated(e.payload)),
    ]);
    // looking for the graphics card takes a moment: not waited for
    void invoke<boolean>('stems_gpu').then((gpu) => this.gpu.set(gpu));
    const status = await invoke<StemsStatus>('stems_status');
    status.models.forEach((m) => this.setModel(m));
    this.progress.set(status.progress);
  }

  /** Whether the installed models would add stems to a track separated this far. */
  canAdd(separation: Separation): boolean {
    const order: Separation[] = ['none', 'vocals', 'full'];
    return order.indexOf(this.reach()) > order.indexOf(separation);
  }

  downloadModel(id: ModelId) {
    const model = this.model(id);
    model.update((m) => ({ ...m, downloading: m.downloading ?? 0, error: null }));
    return this.call('download_stems_model', { id });
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

  private setModel(status: ModelStatus) {
    this.model(status.id).set(status);
  }

  private model(id: ModelId) {
    return {
      vocals: this.vocalsModel,
      'drums-bass': this.drumsBassModel,
      'four-fast': this.fourFastModel,
    }[id];
  }

  private onProgress(progress: StemsProgress) {
    this.progress.set(progress);
    if (!progress.running) this.pending.set(new Set());
  }

  private onSeparated({ path, separation }: Separated) {
    this.forget([path]);
    this.library.markSeparated(path, separation);
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
