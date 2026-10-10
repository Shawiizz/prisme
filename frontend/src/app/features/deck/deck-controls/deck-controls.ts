import { Component, computed, inject, input } from '@angular/core';
import { EngineService } from '../../../core/services/engine.service';
import { phaseOffset, pitchToMatch } from '../../../core/sync';
import { deckColor, formatPitch } from '../../../shared/time';

const PITCH_RANGE = 0.08;
/** Beat offset under which two decks count as aligned on the phase meter. */
const ALIGNED_BEATS = 0.02;

type SyncState = 'off' | 'matched' | 'locked';

/** Transport, sync, tempo fader and phase meter of a deck. */
@Component({
  selector: 'app-deck-controls',
  templateUrl: './deck-controls.html',
  styleUrl: './deck-controls.scss',
})
export class DeckControls {
  readonly deck = input.required<number>();

  protected readonly engine = inject(EngineService);
  protected readonly range = PITCH_RANGE;
  protected readonly color = computed(() => deckColor(this.deck()));
  protected readonly status = computed(() => this.engine.status()?.decks[this.deck()] ?? null);
  protected readonly settings = computed(() => this.engine.decks()[this.deck()]);
  protected readonly vinyl = computed(() => this.settings().vinylEnabled);
  private readonly absolute = computed(() => this.vinyl() && this.settings().vinylMode === 'absolute');

  /** The deck this one is compared with: the leader, else another deck with a tempo. */
  private readonly reference = computed(() => {
    const others = (this.engine.status()?.decks ?? []).filter((_, i) => i !== this.deck());
    return others.find((d) => d.sync.leader) ?? others.find((d) => d.bpm !== null) ?? null;
  });

  protected readonly syncState = computed<SyncState>(() => {
    const sync = this.status()?.sync;
    if (sync?.locked) return 'locked';
    return sync && sync.rateOffset !== 1 ? 'matched' : 'off';
  });
  protected readonly canSync = computed(() => {
    const s = this.status();
    return !!s?.loaded && s.bpm !== null && !this.absolute();
  });
  protected readonly syncTitle = computed(() => {
    if (this.absolute()) return 'Sync needs relative mode or internal play';
    if (this.status()?.bpm == null) return 'No beat grid';
    return this.vinyl() && this.engine.vinylSync() === 'once' ? 'Match the tempo and beats once' : 'Lock to the leader';
  });
  protected readonly tempoFaderLocked = computed(() => this.vinyl() || this.syncState() === 'locked');

  /** Playback rate as the engine reports it, the fader position when it cannot. */
  private readonly rate = computed(() => this.engine.tempoRate(this.deck()) ?? this.settings().pitch);
  protected readonly pitchText = computed(() => formatPitch(this.rate()));
  /** A fader the deck does not obey shows the rate it plays at. */
  protected readonly faderValue = computed(() => (this.tempoFaderLocked() ? this.rate() : this.settings().pitch) - 1);

  protected readonly phase = computed(() => {
    const own = this.status();
    const reference = this.reference();
    return own && reference ? phaseOffset(own, reference) : null;
  });
  protected readonly aligned = computed(() => Math.abs(this.phase() ?? 1) < ALIGNED_BEATS);

  /** On a record not locked by sync: the pitch change that would match the other deck. */
  protected readonly pitchHint = computed(() => {
    const own = this.status();
    const reference = this.reference();
    if (!this.vinyl() || this.syncState() === 'locked' || !own || !reference) return null;
    const change = pitchToMatch(own, reference);
    return change === null ? null : formatPitch(1 + change);
  });

  togglePlay() {
    const s = this.status();
    if (s?.loaded) this.engine.setPlaying(this.deck(), !s.playing);
  }

  cue() {
    this.engine.cue(this.deck());
  }

  sync() {
    this.engine.toggleSync(this.deck());
  }

  onPitch(event: Event) {
    const v = Number((event.target as HTMLInputElement).value);
    this.engine.setPitch(this.deck(), 1 + v);
  }

  resetPitch() {
    this.engine.setPitch(this.deck(), 1);
  }
}
