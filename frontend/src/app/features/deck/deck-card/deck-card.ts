import { Component, computed, inject, input, signal } from '@angular/core';
import { EngineService } from '../../../core/services/engine.service';
import { MixerService } from '../../../core/services/mixer.service';
import { SignalState, VinylMode } from '../../../core/models/engine';
import { deckReadouts } from '../../../core/deck-readouts';
import { decimal, deckLetter, formatTime } from '../../../shared/time';
import { camelotHue, camelotMajor, keyName } from '../../../shared/camelot';
import { levelSegments } from '../../../shared/level';
import { Cover } from '../../../shared/cover/cover';
import { GridEditor } from '../grid-editor/grid-editor';
import { t } from '../../../core/i18n/i18n';
import { TPipe } from '../../../core/i18n/t.pipe';

const SIGNAL_TITLE: Record<SignalState, string> = {
  none: t('No timecode signal'),
  bounce: t('Timecode signal unsteady'),
  ok: t('Timecode signal locked'),
};

/** Who drives the deck: itself (INT), or the record in relative or absolute mode. */
type DeckMode = 'int' | VinylMode;

/** Segments of the meter of what goes to the external mixer. */
const SEND_SEGMENTS = 10;

/**
 * Deck header: letter (lit while playing) with the level sent to an external mixer, cover
 * and title, who drives the deck, and the three readouts seen from afar: remaining time,
 * BPM, key.
 */
@Component({
  selector: 'app-deck-card',
  templateUrl: './deck-card.html',
  styleUrl: './deck-card.scss',
  imports: [Cover, GridEditor, TPipe],
})
export class DeckCard {
  readonly deck = input.required<number>();

  protected readonly engine = inject(EngineService);
  private readonly mixer = inject(MixerService);
  protected readonly letter = computed(() => deckLetter(this.deck()));
  /** The level sent to the deck's outputs of an external mixer (the software mixer has
   * its own meters). */
  protected readonly send = computed(() =>
    this.mixer.internal()
      ? null
      : levelSegments(this.engine.status()?.decks[this.deck()]?.peak ?? 0, SEND_SEGMENTS),
  );
  protected readonly outputs = computed(() => this.mixer.outputsOf(this.deck()));
  private readonly readouts = deckReadouts(this.deck);
  protected readonly track = this.readouts.track;
  protected readonly status = this.readouts.status;
  protected readonly settings = computed(() => this.engine.decks()[this.deck()]);
  protected readonly loading = computed(() => this.engine.loading()[this.deck()]);
  /** The grid and key editor is open. */
  protected readonly editing = signal(false);

  protected readonly modes: { id: DeckMode; label: string; title: string }[] = [
    { id: 'int', label: 'INT', title: t('The deck plays by itself') },
    { id: 'relative', label: 'REL', title: t('The record drives the speed') },
    { id: 'absolute', label: 'ABS', title: t('The record drives the speed and the position') },
  ];
  /** After the end of the record the deck plays on by itself until handed back. */
  protected readonly mode = computed<DeckMode>(() => {
    const settings = this.settings();
    return settings.vinylEnabled && !this.status()?.vinyl.runOut ? settings.vinylMode : 'int';
  });

  protected readonly elapsed = computed(() => formatTime(this.status()?.positionS ?? 0));
  protected readonly length = computed(() => formatTime(this.status()?.durationS ?? 0));
  protected readonly remaining = this.readouts.remaining;
  protected readonly ending = this.readouts.ending;
  protected readonly bpm = this.readouts.bpm;
  /** The track's own tempo and the pitch applied to it. */
  protected readonly tempoDetail = computed(() => {
    const tempo = this.track()?.tempo;
    return tempo ? `${decimal(tempo.bpm, 2)} ${this.readouts.pitch()}` : '';
  });
  protected readonly key = this.readouts.key;
  protected readonly keyShifted = computed(
    () => this.key()?.camelot !== this.track()?.key?.camelot,
  );
  protected readonly hue = camelotHue;
  protected readonly major = camelotMajor;
  protected readonly keyName = keyName;

  protected readonly signal = computed(() => this.status()?.vinyl.signal ?? 'none');
  protected readonly crossed = computed(() => this.status()?.vinyl.crossed ?? false);
  protected readonly signalTitle = computed(() => {
    if (!this.settings().vinylEnabled) return t('The deck plays by itself');
    if (this.status()?.vinyl.runOut) {
      return t(
        'The needle reached the end of the record: the deck plays on by itself. Choose REL or ABS to hand it back to the record.',
      );
    }
    return this.crossed()
      ? t('Left and right cables crossed? See Settings, Turntables')
      : SIGNAL_TITLE[this.signal()];
  });

  setMode(mode: DeckMode) {
    if (mode === 'int') {
      this.engine.setVinyl(this.deck(), { enabled: false });
    } else {
      this.engine.setVinyl(this.deck(), { enabled: true, mode });
    }
  }
}
