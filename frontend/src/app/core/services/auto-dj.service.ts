import { Injectable, computed, inject } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { EngineService } from './engine.service';
import { MixerService } from './mixer.service';
import { t } from '../i18n/i18n';
import { deckLetter } from '../../shared/time';
import { attempt } from '../errors';

/**
 * The Auto DJ: plays the prepare list on decks A and B, each track mixed into the next with
 * the software mixer's crossfader. It runs in the backend, so that it keeps time with the
 * window hidden; a track leaves the list as its mix starts. The DJ's own gestures on those
 * decks or the crossfader pause it.
 */
@Injectable({ providedIn: 'root' })
export class AutoDjService {
  private readonly engine = inject(EngineService);
  private readonly mixer = inject(MixerService);

  readonly status = computed(() => this.engine.told().autoDj);
  /** On, paused or not. */
  readonly on = computed(() => this.status().state !== 'off');
  /** Playing the list: what a controller's light shows. */
  readonly running = computed(() => ['on', 'mixing'].includes(this.status().state));
  readonly paused = computed(() => this.status().state === 'paused');
  readonly mixing = computed(() => this.status().state === 'mixing');
  /** It fades with the software mixer's crossfader: an external mixer's is out of its reach. */
  readonly available = computed(() => this.mixer.internal());
  /** The first track of the prepare list: the one mixed in next. */
  readonly next = computed(() => this.engine.told().lists.next);
  /** Seconds before the next mix starts, while one is planned. */
  readonly mixIn = computed(() => this.engine.status()?.mixInS ?? null);
  /** Why it stopped, paused or left a track out, in words. */
  readonly reason = computed(() => {
    const { reason, reasonDeck } = this.status();
    const deck = deckLetter(reasonDeck ?? 0);
    switch (reason) {
      case null:
        return null;
      case 'externalMixer':
        return t('The Auto DJ fades with the software mixer: choose it in the settings');
      case 'records':
        return t('The Auto DJ plays decks A and B by themselves: no record, no passthrough');
      case 'listEmpty':
        return t('The prepare list is empty');
      case 'listEnded':
        return t('The prepare list has been played');
      case 'transport':
        return t('Paused: you took over deck {deck}', { deck });
      case 'crossfader':
        return t('Paused: you moved the crossfader');
      case 'load':
        return t('Paused: you loaded deck {deck}', { deck });
      case 'unloadable':
        return t('A track could not be read: it was left out');
    }
  });

  /** On, off, or on again after the DJ took over. */
  toggle() {
    return this.engine.perform('autodj.toggle', 1);
  }

  /** Mixes into the next track at once. */
  fadeNow() {
    return this.engine.perform('autodj.fade', 1);
  }

  /** Leaves the next track out of the list: the one after comes next. */
  skip() {
    return this.engine.perform('autodj.skip', 1);
  }

  /** How long a mix lasts between tracks with a beat grid. */
  setFadeBeats(beats: number) {
    return this.call('set_auto_dj_fade', { beats, seconds: null });
  }

  /** How long a mix lasts when a track has no beat grid. */
  setFadeSeconds(seconds: number) {
    return this.call('set_auto_dj_fade', { beats: null, seconds });
  }

  private call(command: string, args: Record<string, unknown> = {}) {
    return attempt(this.engine.error, () => invoke(command, args));
  }
}
