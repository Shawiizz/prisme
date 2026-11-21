import { Signal, computed, inject } from '@angular/core';
import { t } from './i18n/i18n';
import { EngineService } from './services/engine.service';
import { StemsService } from './services/stems.service';
import type { StemPart } from './models/backend';

/** Each part's knob label and name. */
const PARTS: Record<StemPart, { label: string; name: () => string }> = {
  vocals: { label: 'VOX', name: () => t('Vocals') },
  drums: { label: 'DRUM', name: () => t('Drums') },
  bass: { label: 'BASS', name: () => t('Bass') },
  melody: { label: 'MELO', name: () => t('Melody (the other instruments)') },
  instrumental: { label: 'INST', name: () => t('Instrumental') },
};

/**
 * The stems of a deck as its controls show them: the parts of its track with their level,
 * whether they play and whether they wait for the bar, and its separation. To be called
 * where `inject` works.
 */
export function deckStems(deck: Signal<number>) {
  const engine = inject(EngineService);
  const stems = inject(StemsService);
  const track = computed(() => engine.tracks()[deck()]);
  const separation = computed(() => track()?.separation ?? 'none');
  /** Vocals and instrumental, or the four stems of a fully separated track, as the backend
   * shows them: a knob each, its name muting it at its level, greyed where the deck plays a
   * part not separated yet. */
  const knobs = computed(() =>
    (engine.status()?.decks[deck()]?.stems ?? []).map((knob) => {
      const { label, name } = PARTS[knob.part];
      const named = name();
      const title = t('{part}: turn for the level, click the name to mute', { part: named });
      return { ...knob, label, name: named, title };
    }),
  );
  /** How far the track's separation is, while it is being separated, or the download of
   * the models it waits for. */
  const separating = computed(() => engine.status()?.decks[deck()]?.separating ?? null);
  /** Without a model, the separation offers to download them. */
  const canSeparate = computed(() => !!track() && separating() === null);
  /** A separation stopped short: its stems play where they are there, and it can be done
   * again. */
  const resumable = computed(() => !!track()?.partial && separating() === null);
  const separate = () => {
    const path = track()?.path;
    if (path) void stems.separate([path]);
  };
  return { separation, knobs, separating, canSeparate, resumable, separate };
}
