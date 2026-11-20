import { Signal, computed, inject } from '@angular/core';
import { EngineService } from './services/engine.service';
import { pitchToMatch } from './sync';
import { formatPitch } from '../shared/time';

export type SyncState = 'off' | 'matched' | 'locked';

/**
 * The tempo of a deck as its controls share it: the rate it plays at, what holds it (a
 * record, sync), where its fader stands in its range and, on a record, the rate that would
 * match the other deck. To be called where `inject` works.
 */
export function deckTempo(deck: Signal<number>) {
  const engine = inject(EngineService);
  const status = computed(() => engine.status()?.decks[deck()] ?? null);
  const settings = computed(() => engine.decks()[deck()]);
  const vinyl = computed(() => settings().vinylEnabled);
  const syncState = computed<SyncState>(() => {
    const sync = status()?.sync;
    if (sync?.locked) return 'locked';
    return sync && sync.rateOffset !== 1 ? 'matched' : 'off';
  });
  /** The deck this one is compared with: the leader, else another deck with a tempo. */
  const reference = computed(() => {
    const others = (engine.status()?.decks ?? []).filter((_, i) => i !== deck());
    return others.find((d) => d.sync.leader) ?? others.find((d) => d.bpm !== null) ?? null;
  });
  /** Playback rate as the engine reports it, the fader position when it cannot. */
  const rate = computed(() => engine.tempoRate(deck()) ?? settings().pitch);
  /** The fader does not set the tempo: a record does, or sync. */
  const held = computed(() => vinyl() || syncState() === 'locked');
  /** How far the fader goes either way: 0.08 for ±8 %. */
  const range = computed(() => engine.tempoRange(deck()));
  const rangePercent = computed(() => Math.round(range() * 100));
  /** Where the fader stands from the centre: one the deck does not obey shows the rate it
   * plays at. */
  const offset = computed(() => (held() ? rate() : settings().pitch) - 1);
  /** The fader's position as read: `+1.20 %`. */
  const pitch = computed(() => formatPitch(1 + offset()));
  /** On a record not locked by sync: the rate that would match the other deck. */
  const target = computed(() => {
    const own = status();
    const other = reference();
    if (!vinyl() || syncState() === 'locked' || !own || !other) return null;
    const change = pitchToMatch(own, other);
    return change === null ? null : rate() * (1 + change);
  });
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
    offset,
    pitch,
    target,
    toMatch,
  };
}
