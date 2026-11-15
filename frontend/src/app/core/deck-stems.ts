import { Signal, computed, inject } from '@angular/core';
import { t } from './i18n/i18n';
import { EngineService } from './services/engine.service';
import { StemsService } from './services/stems.service';
import { StemPart, partIn, partLevel, partPlaying } from './stem-parts';

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
  /** Vocals and instrumental, or the four stems of a fully separated track: a knob each,
   * its name muting it at its level, greyed where the deck plays a part not separated yet. */
  const knobs = computed(() => {
    const mix = track()?.stemMix;
    const status = engine.status()?.decks[deck()];
    const waiting = status?.stemsWaiting ?? 0;
    const here = status?.stemsHere ?? 0;
    const knob = (part: StemPart, label: string, name: string) => ({
      part,
      label,
      name,
      title: t('{part}: turn for the level, click the name to mute', { part: name }),
      level: mix ? partLevel(mix, part) : 1,
      on: !mix || partPlaying(mix, part),
      waiting: partIn(waiting, part),
      ready: partIn(here, part),
    });
    return separation() === 'full'
      ? [
          knob('vocals', 'VOX', t('Vocals')),
          knob('drums', 'DRUM', t('Drums')),
          knob('bass', 'BASS', t('Bass')),
          knob('other', 'MELO', t('Melody (the other instruments)')),
        ]
      : [knob('vocals', 'VOX', t('Vocals')), knob('instrumental', 'INST', t('Instrumental'))];
  });
  /** How far the track's separation is, while it is being separated. */
  const separating = computed(() => {
    const progress = stems.progress();
    const path = track()?.path;
    return path && progress.current === path ? progress.fraction : null;
  });
  const canSeparate = computed(() => !!track() && stems.canAdd('none') && separating() === null);
  const separate = () => {
    const path = track()?.path;
    if (path) void stems.separate([path]);
  };
  return { separation, knobs, separating, canSeparate, separate };
}
