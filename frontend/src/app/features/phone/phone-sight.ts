import { DestroyRef, Injectable, Signal, computed, inject, signal } from '@angular/core';
import { EFFECT_NAMES } from '../../core/effects';
import type { PadMode } from '../deck/deck-pads/deck-pads';
import type { Band, StemPart } from '../../core/models/backend';

/** What a knob of an effect slot sets: its level, its main and second knobs, its period, the
 * effect it plays, or whether it plays. */
export type FxSetting = 'mix' | 'amount' | 'param2' | 'beats' | 'kind' | 'on';

/** What the mixer sets beside the channels; `samplers`, the level of every deck's sampler. */
export type MixerSetting =
  | 'crossfader'
  | 'cueMix'
  | 'headphones'
  | 'main'
  | 'balance'
  | 'booth'
  | 'mic'
  | 'aux'
  | 'samplers';

/** What a deck sets beside its EQ, its stems and its effect slots. */
export type DeckSetting =
  | 'trim'
  | 'filter'
  | 'volume'
  | 'tempo'
  | 'key'
  | 'keyLock'
  | 'cue'
  | 'mute'
  | 'samplerGain'
  | 'fxChain';

/** A setting an action moves: of a deck, or of the mixer (`deck` null). The effects' `slot`
 * is null for the knob that turns them all. */
export type Setting =
  | { deck: number; of: 'eq'; band: Band }
  | { deck: number; of: 'stem'; part: StemPart }
  | { deck: number; of: 'fx'; slot: number | null; knob: FxSetting }
  | { deck: number; of: DeckSetting }
  | { deck: null; of: MixerSetting };

/** What a part of the phone's screen shows: a deck's readouts, its channel, its panel (with
 * the channel, or not), its pitch, its stems or its pads floating; the crossfader, the mixer's
 * sheet; the tracks of the library. */
export type View =
  | { kind: 'deck'; deck: number; compact: boolean }
  | { kind: 'channel'; deck: number }
  | { kind: 'panel'; deck: number; mixer: boolean }
  | { kind: 'pitch'; deck: number }
  | { kind: 'stems'; deck: number }
  | { kind: 'pads'; deck: number; mode: PadMode; fxSlot: number }
  | { kind: 'crossfader' }
  | { kind: 'mixer' }
  | { kind: 'tracks' };

const MIXER: Record<string, MixerSetting> = {
  'mixer.crossfader': 'crossfader',
  'mixer.cueMix': 'cueMix',
  'mixer.headphones': 'headphones',
  'mixer.main': 'main',
  'mixer.balance': 'balance',
  'mixer.booth': 'booth',
  'mixer.mic': 'mic',
  'mixer.mic.on': 'mic',
  'mixer.aux': 'aux',
  'mixer.aux.on': 'aux',
  'sampler.gain': 'samplers',
};
/** A deck's settings by the action, its switches turned over or on or off alike. */
const DECK: Record<string, DeckSetting> = {
  pitch: 'tempo',
  tempo: 'tempo',
  'tempo.up': 'tempo',
  'tempo.down': 'tempo',
  'tempo.reset': 'tempo',
  trim: 'trim',
  filter: 'filter',
  'filter.toggle': 'filter',
  volume: 'volume',
  key: 'key',
  'key.up': 'key',
  'key.down': 'key',
  'key.reset': 'key',
  'key.sync': 'key',
  keylock: 'keyLock',
  headphones: 'cue',
  'headphones.solo': 'cue',
  mute: 'mute',
  'sampler.gain': 'samplerGain',
  'fx.chain': 'fxChain',
};
const PARTS: readonly string[] = ['vocals', 'instrumental', 'drums', 'bass', 'melody'];
const EFFECTS = Object.keys(EFFECT_NAMES);

/** What an action of an effect slot sets: none for its focus. */
function fxSetting(action: string | undefined): FxSetting | null {
  if (action === undefined || ['on', 'off', 'held'].includes(action)) return 'on';
  if (action === 'mix' || action === 'amount' || action === 'param2') return action;
  if (action === 'period' || action === 'beats') return 'beats';
  if (['kind', 'next', 'previous', ...EFFECTS].includes(action)) return 'kind';
  return null;
}

/** The setting the action `id` moves, as a controller or a key moves it: none for what plays
 * the track, browses or lays the screen out. */
export function settingOf(id: string): Setting | null {
  const mixer = MIXER[id];
  if (mixer) return { deck: null, of: mixer };
  const named = /^deck([1-4])\.(.+)$/.exec(id);
  if (!named) return null;
  const deck = Number(named[1]) - 1;
  const action = named[2].replace(/\.(on|off)$/, '');
  const of = DECK[action];
  if (of) return { deck, of };
  if (/^key\.[+-]?\d+$/.test(action)) return { deck, of: 'key' };
  if (action === 'fx.super') return { deck, of: 'fx', slot: null, knob: 'amount' };
  const band = /^eq\.(low|mid|high)(\.kill(\.toggle)?)?$/.exec(action);
  if (band) return { deck, of: 'eq', band: band[1] as Band };
  const part = /^(\w+)(\.level)?$/.exec(action);
  if (part && PARTS.includes(part[1])) return { deck, of: 'stem', part: part[1] as StemPart };
  const fx = /^fx([23])?(?:\.(\w+))?$/.exec(named[2]);
  const knob = fx && fxSetting(fx[2]);
  if (!fx || !knob) return null;
  return { deck, of: 'fx', slot: fx[1] ? Number(fx[1]) - 1 : 0, knob };
}

/** What a deck's channel shows of its settings. */
const CHANNEL = new Set<Setting['of']>(['eq', 'filter', 'volume', 'cue']);

/**
 * Whether `view` shows `setting`: the readouts of a deck its tempo and its key heard (its
 * mute but compact); its channel on the table its EQ, filter, fader and headphones, not its
 * trim; its panel its pitch, key lock and mute, and the channel where the table has none; the
 * pitch floating its tempo and key lock; the stems floating each part; the pads their mode's
 * knobs: the effect slot chosen and the switches of all, or the sampler's level; the sheet
 * of the mixer all it sets beside the crossfader and the samplers.
 */
export function shows(view: View, setting: Setting): boolean {
  if (setting.deck === null) {
    if (setting.of === 'crossfader') return view.kind === 'crossfader';
    if (setting.of === 'samplers') return view.kind === 'pads' && view.mode === 'sampler';
    return view.kind === 'mixer';
  }
  if (!('deck' in view) || view.deck !== setting.deck) return false;
  const of = setting.of;
  switch (view.kind) {
    case 'deck':
      return of === 'tempo' || of === 'key' || (of === 'mute' && !view.compact);
    case 'channel':
      return CHANNEL.has(of);
    case 'panel':
      return of === 'tempo' || of === 'keyLock' || of === 'mute' || (view.mixer && CHANNEL.has(of));
    case 'pitch':
      return of === 'tempo' || of === 'keyLock';
    case 'stems':
      return of === 'stem';
    case 'pads':
      if (view.mode === 'sampler') return of === 'samplerGain';
      if (view.mode !== 'fx') return false;
      return (
        of === 'fxChain' ||
        (setting.of === 'fx' && (setting.knob === 'on' || setting.slot === view.fxSlot))
      );
  }
}

/** What the phone's screen shows now, each part of it telling it while it is there
 * (`inSight`): a setting a controller moves is shown over it where it shows none. */
@Injectable()
export class PhoneSight {
  private readonly views = signal<readonly Signal<View | null>[]>([]);
  readonly shown = computed(() => this.views().flatMap((view) => view() ?? []));

  /** Whether the screen shows the tracks of the library. */
  readonly tracks = computed(() => this.shown().some((view) => view.kind === 'tracks'));

  /** Whether the screen shows `setting`. */
  showing(setting: Setting): boolean {
    return this.shown().some((view) => shows(view, setting));
  }

  /** `view` shown until the function returned is called. */
  show(view: Signal<View | null>): () => void {
    this.views.update((views) => [...views, view]);
    return () => this.views.update((views) => views.filter((v) => v !== view));
  }
}

/** Tells the phone what the component calling it shows, `view` while it is there (none: nothing
 * of it); off a phone, nothing. To be called where `inject` works. */
export function inSight(view: () => View | null) {
  const sight = inject(PhoneSight, { optional: true });
  if (!sight) return;
  inject(DestroyRef).onDestroy(sight.show(computed(view)));
}
