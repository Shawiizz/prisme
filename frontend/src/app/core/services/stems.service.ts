import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';
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
import { errorText } from '../errors';

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
  /** Tracks asked to be separated while no model was there: the models offered for them. */
  readonly offered = signal<string[] | null>(null);
  /** Tracks to separate once the models on their way are in. */
  readonly awaiting = signal<readonly string[]>([]);

  /**
   * How far the models the DJ chose separate a track, as far as they are installed: the
   * small one for every stem, or the large ones (the others never stand in for them).
   */
  readonly reach = computed<Separation>(() => {
    if (this.preferences.library().stemsFast) {
      return this.fourFastModel().installed ? 'full' : 'none';
    }
    if (!this.vocalsModel().installed) return 'none';
    return this.drumsBassModel().installed ? 'full' : 'vocals';
  });

  /** The whole queue, as a share of the tracks it held (0 to 1). */
  readonly overall = computed(() => {
    const p = this.progress();
    const total = p.done + p.failed + p.queued;
    return total > 0 ? (p.done + p.failed + (p.current ? p.fraction : 0)) / total : 0;
  });

  /** How far the downloads under way are, by their bytes (0 to 1); `null` without any. */
  readonly downloading = computed(() => {
    const models = this.models().filter((m) => m.downloading !== null);
    if (!models.length) return null;
    const bytes = models.reduce((sum, m) => sum + m.bytes, 0);
    return models.reduce((sum, m) => sum + m.bytes * m.downloading!, 0) / Math.max(bytes, 1);
  });

  constructor() {
    // the tracks waiting for the models go once the downloads end with some in
    effect(() => {
      const paths = this.awaiting();
      if (!paths.length || this.downloading() !== null || this.reach() === 'none') return;
      this.awaiting.set([]);
      untracked(() => void this.separate([...paths]));
    });
  }

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

  /** Queues the tracks; without a model, offers to download them first. */
  async separate(paths: string[]) {
    if (this.reach() === 'none') {
      // the models on their way already: these follow too
      if (this.downloading() !== null) this.wait(paths);
      this.offered.set(paths);
      return;
    }
    this.pending.update((s) => new Set([...s, ...paths]));
    if (!(await this.call('separate_tracks', { paths }))) this.forget(paths);
  }

  /** The models of a choice: the small one when `fast`, else the large ones. */
  modelsFor(fast: boolean): ModelStatus[] {
    return fast ? [this.fourFastModel()] : [this.vocalsModel(), this.drumsBassModel()];
  }

  /** Separates with the models of a choice from now on, downloads those missing, then
   * separates the tracks offered. */
  async downloadFor(fast: boolean) {
    this.wait(this.offered() ?? []);
    await this.preferences.updateLibrary({ stemsFast: fast });
    for (const model of this.modelsFor(fast).filter((m) => !m.installed)) {
      await this.downloadModel(model.id);
    }
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

  /** Separates the tracks once the models on their way are in. */
  private wait(paths: string[]) {
    this.awaiting.update((a) => [...new Set([...a, ...paths])]);
  }

  private models(): ModelStatus[] {
    return [this.vocalsModel(), this.drumsBassModel(), this.fourFastModel()];
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
      this.error.set(errorText(e));
      return false;
    }
  }
}
