import { Injectable, computed, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { EngineService } from './engine.service';
import { attempt, errorText } from '../errors';
import type { ModelId, Separated, StemsState } from '../models/backend';

/**
 * Stem separation, done in the background by the backend, as it tells it: the models (each
 * downloaded once), the queue and its progress, the tracks waiting for the models. The rows of
 * the library say how far a track is separated.
 */
@Injectable({ providedIn: 'root' })
export class StemsService {
  private readonly engine = inject(EngineService);

  /** As the backend last told it; nothing until the first status. */
  readonly state = computed<StemsState>(() => this.engine.told().stems);
  readonly progress = computed(() => this.state().progress);
  /** How far the models chosen and installed separate a track. */
  readonly reach = computed(() => this.state().reach);
  readonly error = signal<string | null>(null);
  /** The models were offered for tracks to separate: the dialog shows. */
  readonly offered = signal(false);

  downloadModel(id: ModelId) {
    return this.call('download_stems_model', { id });
  }

  /** Queues the tracks at `paths` (else those selected); without a model, offers the models,
   * the tracks following once they are in. */
  async separate(paths: string[] | null) {
    try {
      this.error.set(null);
      const outcome = await this.engine.ordered<Separated>('separate_tracks', {
        paths,
      });
      if (outcome === 'needsModels') this.offered.set(true);
    } catch (e) {
      this.error.set(errorText(e));
    }
  }

  /** Separates with the models of a choice from now on, downloads those missing, then
   * separates the tracks offered. */
  downloadFor(fast: boolean) {
    return this.call('download_stems_models', { fast });
  }

  separateLibrary() {
    return this.call('separate_library', {});
  }

  cancel() {
    return this.call('cancel_separation', {});
  }

  private call(command: string, args: Record<string, unknown>): Promise<boolean> {
    return attempt(this.error, () => invoke(command, args));
  }
}
