import { Signal, computed, inject } from '@angular/core';
import { EngineService } from './services/engine.service';
import { same } from './same';
import { Text, t } from './i18n/i18n';
import type { DeckMode, SignalState } from './models/backend';

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

export const DECK_MODES: readonly { id: DeckMode; label: string; title: Text }[] = [
  {
    id: 'int',
    label: 'INT',
    title: () => t('The deck plays by itself'),
  },
  {
    id: 'relative',
    label: 'REL',
    title: () => t('The record drives the speed'),
  },
  {
    id: 'absolute',
    label: 'ABS',
    title: () => t('The record drives the speed and the position'),
  },
  {
    id: 'thru',
    label: 'THRU',
    title: () => t('The deck plays its input as it comes: a real record, a CD player'),
  },
];

/**
 * Who drives a deck and how its record is heard, as its card and its strip on a phone share
 * them. To be called where `inject` works.
 */
export function deckVinyl(deck: Signal<number>) {
  const engine = inject(EngineService);
  const vinyl = computed(() => engine.deckStatus(deck())?.vinyl ?? null, { equal: same });
  const settings = computed(() => engine.decks()[deck()]);
  /** After the end of the record the deck plays on by itself until handed back. */
  const mode = computed<DeckMode>(() => engine.deckStatus(deck())?.mode ?? 'int');
  /** Records can be chosen to drive it. */
  const offered = computed(() => settings().records);
  /** The timecode as its light tells it; `null` while the deck plays by itself. */
  const signal = computed<SignalState | null>(() => vinyl()?.light ?? null);
  const signalTitle = computed(() => {
    if (settings().passthrough) return t('The deck plays its input as it comes');
    if (!settings().vinylEnabled) return t('The deck plays by itself');
    if (vinyl()?.runOut) {
      return t(
        'The needle reached the end of the record: the deck plays on by itself. Choose REL or ABS to hand it back to the record.',
      );
    }
    return vinyl()?.crossed
      ? t('Left and right cables crossed? See Settings, Turntables')
      : SIGNAL_TITLE[vinyl()?.signal ?? 'none'];
  });
  const setMode = (to: DeckMode) => engine.call('set_deck_mode', { deck: deck(), mode: to });
  return { mode, offered, signal, signalTitle, setMode };
}
