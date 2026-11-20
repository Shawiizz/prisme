import { Signal, computed, inject } from '@angular/core';
import { EngineService } from './services/engine.service';
import { SyncState, TempoFader } from './models/engine';
import { formatPitch } from '../shared/time';
import { same } from './same';

const STILL: TempoFader = {
  held: false,
  ratio: 1,
  position: 0.5,
  matching: null,
  matchingAt: null,
};

/**
 * The tempo of a deck as its controls share it, as the backend shows it: the rate it plays
 * at, its fader (held by a record or sync, where it stands in its range) and, on a record, the
 * rate that would match the other deck. To be called where `inject` works.
 */
export function deckTempo(deck: Signal<number>) {
  const engine = inject(EngineService);
  const status = computed(() => engine.status()?.decks[deck()] ?? null);
  const settings = computed(() => engine.decks()[deck()]);
  const vinyl = computed(() => settings().vinylEnabled);
  const syncState = computed<SyncState>(() => status()?.sync.state ?? 'off');
  const rate = computed(() => status()?.rate ?? 1);
  const fader = computed(() => status()?.tempo ?? STILL, { equal: same });
  /** The fader does not set the tempo: a record does, or sync. */
  const held = computed(() => fader().held);
  /** How far the fader goes either way: 0.08 for ±8 %. */
  const range = computed(() => settings().tempoRange);
  const rangePercent = computed(() => Math.round(range() * 100));
  /** Where the fader stands, 0 to 1. */
  const position = computed(() => fader().position);
  /** The fader's tempo as read: `+1.20 %`. */
  const pitch = computed(() => formatPitch(fader().ratio));
  /** On a record not locked by sync: the rate that would match the other deck. */
  const target = computed(() => fader().matching);
  /** Where that rate is along the fader, within its range. */
  const targetAt = computed(() => fader().matchingAt);
  /** The target as read, to bring the turntable's pitch to. */
  const toMatch = computed(() => {
    const at = target();
    return at === null ? null : formatPitch(at);
  });
  return {
    status,
    settings,
    vinyl,
    syncState,
    rate,
    held,
    range,
    rangePercent,
    position,
    pitch,
    target,
    targetAt,
    toMatch,
  };
}
