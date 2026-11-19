import { Component, computed, effect, inject, signal } from '@angular/core';
import { PreferencesService } from '../../core/services/preferences.service';
import { StemsService } from '../../core/services/stems.service';
import { PHONE } from '../../core/platform';
import { TPipe } from '../../core/i18n/t.pipe';
import { readouts } from '../../shared/mixer-scales';
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
  private readonly preferences = inject(PreferencesService);
  protected readonly percent = readouts.percent;

  /** The small models are the better choice on a phone or without a graphics chip worth it. */
  protected readonly fastSuits = computed(() => PHONE || this.stems.gpu() === false);
  protected readonly fast = signal(false);
  /** Megabytes each choice has left to download; `null` once it is installed. */
  protected readonly sizes = computed(() => ({
    quality: this.megabytes(false),
    fast: this.megabytes(true),
  }));
  protected readonly chosenInstalled = computed(() =>
    this.stems.modelsFor(this.fast()).every((m) => m.installed),
  );
  /** Why the download of the choice failed, if it did. */
  protected readonly failed = computed(
    () => this.stems.modelsFor(this.fast()).find((m) => m.error)?.error ?? null,
  );

  constructor() {
    effect(() => {
      if (this.stems.offered()) {
        this.fast.set(this.preferences.library().stemsFast || this.fastSuits());
      }
    });
    // the models in, the track separating: nothing left to show
    effect(() => {
      if (this.stems.offered() && this.stems.reach() !== 'none') this.close();
    });
    overlay(
      () => this.stems.offered() !== null,
      () => this.close(),
    );
  }

  protected close() {
    this.stems.offered.set(null);
  }

  protected download() {
    void this.stems.downloadFor(this.fast());
  }

  private megabytes(fast: boolean): number | null {
    const missing = this.stems.modelsFor(fast).filter((m) => !m.installed);
    if (!missing.length) return null;
    return Math.round(missing.reduce((sum, m) => sum + m.bytes, 0) / 1e6);
  }
}
