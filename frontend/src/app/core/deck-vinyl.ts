import { Signal, computed, inject } from '@angular/core';
import { SignalState, VinylMode } from './models/engine';
import { EngineService } from './services/engine.service';
import { t } from './i18n/i18n';

const SIGNAL_TITLE: Record<SignalState, string> = {
  get none() {
    return t('No timecode signal');
  },
  get bounce() {
    return t('Timecode signal unsteady');
  },
  get ok() {
    return t('Timecode signal locked');
  },
};

/** Who drives the deck: itself (INT), or the record in relative or absolute mode. */
export type DeckMode = 'int' | VinylMode;

export const DECK_MODES: readonly { id: DeckMode; label: string; title: string }[] = [
  {
    id: 'int',
    label: 'INT',
    get title() {
      return t('The deck plays by itself');
    },
  },
  {
    id: 'relative',
    label: 'REL',
    get title() {
      return t('The record drives the speed');
    },
  },
  {
    id: 'absolute',
    label: 'ABS',
    get title() {
      return t('The record drives the speed and the position');
    },
  },
];

/**
 * Who drives a deck and how its record is heard, as its card and its strip on a phone share
 * them. To be called where `inject` works.
 */
export function deckVinyl(deck: Signal<number>) {
  const engine = inject(EngineService);
  const vinyl = computed(() => engine.status()?.decks[deck()]?.vinyl ?? null);
  const settings = computed(() => engine.decks()[deck()]);
  /** After the end of the record the deck plays on by itself until handed back. */
  const mode = computed<DeckMode>(() => {
    const { vinylEnabled, vinylMode } = settings();
    return vinylEnabled && !vinyl()?.runOut ? vinylMode : 'int';
  });
  const crossed = computed(() => vinyl()?.crossed ?? false);
  /** The timecode as its light tells it; `null` while the deck plays by itself. */
  const signal = computed<SignalState | null>(() => {
    if (!settings().vinylEnabled) return null;
    return crossed() ? 'none' : (vinyl()?.signal ?? 'none');
  });
  const signalTitle = computed(() => {
    if (!settings().vinylEnabled) return t('The deck plays by itself');
    if (vinyl()?.runOut) {
      return t(
        'The needle reached the end of the record: the deck plays on by itself. Choose REL or ABS to hand it back to the record.',
      );
    }
    return crossed()
      ? t('Left and right cables crossed? See Settings, Turntables')
      : SIGNAL_TITLE[vinyl()?.signal ?? 'none'];
  });
  const setMode = (to: DeckMode) =>
    engine.setVinyl(deck(), to === 'int' ? { enabled: false } : { enabled: true, mode: to });
  return { mode, signal, signalTitle, setMode };
}
