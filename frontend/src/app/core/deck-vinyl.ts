import { Signal, computed, inject } from '@angular/core';
import { DeckMode, SignalState } from './models/engine';
import { EngineService } from './services/engine.service';
import { SettingsService } from './services/settings.service';
import { Text, t } from './i18n/i18n';

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
  const audio = inject(SettingsService);
  const vinyl = computed(() => engine.status()?.decks[deck()]?.vinyl ?? null);
  const settings = computed(() => engine.decks()[deck()]);
  /** After the end of the record the deck plays on by itself until handed back. */
  const mode = computed<DeckMode>(() => engine.status()?.decks[deck()]?.mode ?? 'int');
  /** Records can be chosen to drive it: turntables are plugged, or it is still handed to
   * one. */
  const offered = computed(() => audio.turntables() || mode() !== 'int');
  const crossed = computed(() => vinyl()?.crossed ?? false);
  /** The timecode as its light tells it; `null` while the deck plays by itself. */
  const signal = computed<SignalState | null>(() => {
    if (!settings().vinylEnabled || settings().passthrough) return null;
    return crossed() ? 'none' : (vinyl()?.signal ?? 'none');
  });
  const signalTitle = computed(() => {
    if (settings().passthrough) return t('The deck plays its input as it comes');
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
  /** THRU leaves the record as it was, for when the deck plays its track again. */
  const setMode = async (to: DeckMode) => {
    if ((to === 'thru') !== settings().passthrough)
      await engine.setPassthrough(deck(), to === 'thru');
    if (to === 'thru') return;
    if (to !== 'int') await engine.setVinyl(deck(), { setting: 'mode', value: to });
    await engine.setVinylEnabled(deck(), to !== 'int');
  };
  return { mode, offered, signal, signalTitle, setMode };
}
