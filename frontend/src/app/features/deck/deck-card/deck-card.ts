import { Component, computed, inject, input, signal } from '@angular/core';
import { EngineService } from '../../../core/services/engine.service';
import { SignalState, VinylMode } from '../../../core/models/engine';
import {
  decimal,
  deckLetter,
  formatPitch,
  formatRemaining,
  formatTime,
} from '../../../shared/time';
import { camelotHue, camelotMajor, keyName } from '../../../shared/camelot';
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

/** The remaining time turns amber, then red and blinking, near the end of the track. */
const WARN_S = 30;
const ALERT_S = 10;

/**
 * Deck header: letter (lit while playing), cover and title, who drives the deck, and the
 * three readouts seen from afar: remaining time, BPM, key.
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
  protected readonly letter = computed(() => deckLetter(this.deck()));
  protected readonly track = computed(() => this.engine.tracks()[this.deck()]);
  protected readonly status = computed(() => this.engine.status()?.decks[this.deck()] ?? null);
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
  private readonly remainingS = computed(() => {
    const s = this.status();
    return s?.loaded ? Math.max(0, s.durationS - s.positionS) : null;
  });
  protected readonly remaining = computed(() => {
    const s = this.remainingS();
    return s === null ? '—' : formatRemaining(s);
  });
  protected readonly ending = computed(() => {
    const s = this.remainingS();
    if (s === null || !this.status()?.playing) return null;
    return s < ALERT_S ? 'alert' : s < WARN_S ? 'warn' : null;
  });

  private readonly rate = computed(() => this.engine.playbackRate(this.deck()));
  protected readonly bpm = computed(() => {
    const tempo = this.track()?.tempo;
    return tempo ? decimal(tempo.bpm * this.rate(), 2) : null;
  });
  /** The track's own tempo and the pitch applied to it. */
  protected readonly tempoDetail = computed(() => {
    const tempo = this.track()?.tempo;
    return tempo ? `${decimal(tempo.bpm, 2)} ${formatPitch(this.rate())}` : '';
  });
  /** The key heard: shifted by the pitch unless the key lock holds it. */
  protected readonly key = computed(() => this.engine.heardKey(this.deck()));
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
