import { Component, computed, inject, input, signal } from '@angular/core';
import { EngineService } from '../../../core/services/engine.service';
import { SignalState } from '../../../core/models/engine';
import { deckColor, formatPitch, formatTime } from '../../../shared/time';
import { Cover } from '../../../shared/cover/cover';
import { GridEditor } from '../grid-editor/grid-editor';
import { t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';

const SIGNAL_TITLE: Record<SignalState, string> = {
  none: t('No timecode signal'),
  bounce: t('Timecode signal unsteady'),
  ok: t('Timecode signal locked'),
};

/**
 * Deck header: number tile in the deck colour, cover, title and time, labelled stats and
 * the vinyl (DVS) controls.
 */
@Component({
  selector: 'app-deck-card',
  templateUrl: './deck-card.html',
  styleUrl: './deck-card.scss',
  imports: [Cover, GridEditor, TPipe],
  host: { '[style.--accent]': 'color()' },
})
export class DeckCard {
  readonly deck = input.required<number>();

  protected readonly engine = inject(EngineService);
  protected readonly color = computed(() => deckColor(this.deck()));
  protected readonly track = computed(() => this.engine.tracks()[this.deck()]);
  protected readonly status = computed(() => this.engine.status()?.decks[this.deck()] ?? null);
  protected readonly settings = computed(() => this.engine.decks()[this.deck()]);
  protected readonly loading = computed(() => this.engine.loading()[this.deck()]);
  /** The grid and key editor is open. */
  protected readonly editing = signal(false);

  protected readonly elapsed = computed(() => formatTime(this.status()?.positionS ?? 0));
  protected readonly remaining = computed(() => {
    const s = this.status();
    return s ? '-' + formatTime(s.durationS - s.positionS) : '';
  });
  private readonly rate = computed(() => this.engine.playbackRate(this.deck()));
  protected readonly pitch = computed(() => formatPitch(this.rate()));
  protected readonly bpm = computed(() => {
    const tempo = this.track()?.tempo;
    return tempo ? (tempo.bpm * this.rate()).toFixed(1) : null;
  });
  /** The key heard: shifted by the pitch unless the key lock holds it. */
  protected readonly key = computed(() => this.engine.heardKey(this.deck()));
  protected readonly keyShifted = computed(
    () => this.key()?.camelot !== this.track()?.key?.camelot,
  );
  protected readonly signal = computed(() => this.status()?.vinyl.signal ?? 'none');
  protected readonly crossed = computed(() => this.status()?.vinyl.crossed ?? false);
  protected readonly signalTitle = computed(() =>
    this.crossed() ? t('Left and right cables crossed? See DVS') : SIGNAL_TITLE[this.signal()],
  );

  /** After the end of the record: the record drives the deck again. */
  takeBack() {
    this.engine.setVinyl(this.deck(), {});
  }

  toggleVinyl() {
    this.engine.setVinyl(this.deck(), { enabled: !this.settings().vinylEnabled });
  }

  toggleMode() {
    this.engine.setVinyl(this.deck(), {
      mode: this.settings().vinylMode === 'absolute' ? 'relative' : 'absolute',
    });
  }
}
