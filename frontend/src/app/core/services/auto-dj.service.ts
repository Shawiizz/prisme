import { Injectable, computed, inject, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { EngineService } from './engine.service';
import { MixerService } from './mixer.service';
import { PrepareService } from './prepare.service';
import { t } from '../i18n/i18n';
import { deckLetter } from '../../shared/time';
import { errorText } from '../errors';

export type AutoDjState = 'off' | 'on' | 'mixing' | 'paused';
export type AutoDjReason =
  | 'externalMixer'
  | 'records'
  | 'listEmpty'
  | 'listEnded'
  | 'transport'
  | 'crossfader'
  | 'load'
  | 'unloadable';

/** The Auto DJ as the backend runs it (see apps/prisme-desktop/src/auto_dj). */
export interface AutoDjStatus {
  state: AutoDjState;
  /** Why it stopped, paused, or left a track out. */
  reason: AutoDjReason | null;
  /** The deck the DJ played or loaded, for those reasons. */
  reasonDeck: number | null;
  /** The deck heard. */
  current: number | null;
  /** The deck being mixed in. */
  incoming: number | null;
  /** Where on the deck heard the next mix starts. */
  mixAtS: number | null;
  /** Length of a mix between tracks with a beat grid. */
  fadeBeats: number;
  /** Length of a mix when one has none. */
  fadeS: number;
}

export const FADE_BEATS = [8, 16, 32, 64] as const;
export const FADE_SECONDS = [5, 10, 15, 30] as const;

const OFF: AutoDjStatus = {
  state: 'off',
  reason: null,
  reasonDeck: null,
  current: null,
  incoming: null,
  mixAtS: null,
  fadeBeats: 16,
  fadeS: 10,
};

/** Seconds before a deck at `positionS`, going at `speed`, reaches `atS`. */
export function secondsUntil(atS: number, positionS: number, speed: number): number {
  return Math.max(0, (atS - positionS) / Math.max(Math.abs(speed), 0.05));
}

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
  private readonly prepare = inject(PrepareService);

  readonly status = signal<AutoDjStatus>(OFF);
  /** On, paused or not. */
  readonly on = computed(() => this.status().state !== 'off');
  /** Playing the list: what a controller's light shows. */
  readonly running = computed(() => ['on', 'mixing'].includes(this.status().state));
  readonly paused = computed(() => this.status().state === 'paused');
  readonly mixing = computed(() => this.status().state === 'mixing');
  /** It fades with the software mixer's crossfader: an external mixer's is out of its reach. */
  readonly available = computed(() => this.mixer.internal());
  /** The first track of the prepare list: the one mixed in next. */
  readonly next = computed(() => this.prepare.list()[0] ?? null);
  /** Seconds before the next mix starts, while one is planned. */
  readonly mixIn = computed(() => {
    const { state, current, mixAtS } = this.status();
    if (state !== 'on' || current === null || mixAtS === null) return null;
    const deck = this.engine.status()?.decks[current];
    return deck?.playing ? secondsUntil(mixAtS, deck.positionS, deck.speed) : null;
  });
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

  async init(): Promise<void> {
    await listen<AutoDjStatus>('auto-dj', ({ payload }) => this.status.set(payload));
    this.status.set(await invoke<AutoDjStatus>('auto_dj_status'));
  }

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

  setFade(beats: number, seconds: number) {
    return this.call('set_auto_dj_fade', { beats, seconds });
  }

  private async call(command: string, args: Record<string, unknown> = {}) {
    try {
      this.status.set(await invoke<AutoDjStatus>(command, args));
    } catch (e) {
      this.engine.error.set(errorText(e));
    }
  }
}
