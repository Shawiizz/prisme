import { Component, computed, effect, inject, signal, untracked } from '@angular/core';
import { StemsService } from '../../core/services/stems.service';
import { TPipe } from '../../core/i18n/t.pipe';
import { percent } from '../../shared/readout';
import { Dismiss, overlay } from '../../shared/overlay';

/**
 * Offered when a track is to be separated and no model is there: what the models do, the
 * two choices with their size and speed, and their download, after which the track is
 * separated. Closed meanwhile, the download goes on and the track still follows.
 */
@Component({
  selector: 'app-models-dialog',
  templateUrl: './models-dialog.html',
  styleUrl: './models-dialog.scss',
  imports: [TPipe, Dismiss],
})
export class ModelsDialog {
  protected readonly stems = inject(StemsService);
  protected readonly percent = percent;

  protected readonly fast = signal(false);
  /** The choice shown: what it has left to download, and why its download failed. */
  protected readonly chosen = computed(() => {
    const state = this.stems.state();
    return state && (this.fast() ? state.fast : state.quality);
  });

  constructor() {
    effect(() => {
      if (this.stems.offered()) {
        this.fast.set(untracked(() => this.stems.state()?.suggestFast ?? false));
      }
    });
    // the models in, the track separating: nothing left to show
    effect(() => {
      if (this.stems.offered() && this.stems.reach() !== 'none') this.close();
    });
    overlay(
      () => this.stems.offered(),
      () => this.close(),
    );
  }

  protected close() {
    this.stems.offered.set(false);
  }

  protected download() {
    void this.stems.downloadFor(this.fast());
  }

  /** `456 MB` left for a choice, `null` once it is installed. */
  protected megabytes(bytes: number | null): number | null {
    return bytes === null ? null : Math.round(bytes / 1e6);
  }
}
