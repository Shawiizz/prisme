import { Injectable, computed, signal } from '@angular/core';
import { Channel, invoke } from '@tauri-apps/api/core';
import { DECK_COUNT, DeckSettings, EngineStatus, MixerState, TrackInfo, VinylMode } from '../models/engine';

const DEFAULT_DECK: DeckSettings = {
  pitch: 1,
  volume: 1,
  vinylEnabled: false,
  vinylMode: 'absolute',
  leadInS: 0,
  cueS: 0,
};

/**
 * Bridge to the Rust engine: commands go through `invoke`, the engine state arrives
 * ~60 times per second through a Tauri channel.
 */
@Injectable({ providedIn: 'root' })
export class EngineService {
  /** Latest engine state (drives the templates). */
  readonly status = signal<EngineStatus | null>(null);
  readonly tracks = signal<(TrackInfo | null)[]>(Array(DECK_COUNT).fill(null));
  readonly loading = signal<boolean[]>(Array(DECK_COUNT).fill(false));
  readonly decks = signal<DeckSettings[]>(Array.from({ length: DECK_COUNT }, () => ({ ...DEFAULT_DECK })));
  readonly crossfader = signal(0);
  readonly error = signal<string | null>(null);

  readonly audioRunning = computed(() => this.status()?.audioRunning ?? false);

  private latest: EngineStatus | null = null;
  private latestAt = 0;

  async connect(): Promise<void> {
    const channel = new Channel<EngineStatus>();
    channel.onmessage = (s) => {
      this.latest = s;
      this.latestAt = performance.now();
      this.status.set(s);
    };
    await invoke('subscribe_status', { channel });
    const [tracks, mixer] = await Promise.all([
      invoke<(TrackInfo | null)[]>('loaded_tracks'),
      invoke<MixerState>('mixer_state'),
    ]);
    this.tracks.set(tracks);
    this.decks.set(mixer.decks.map((d) => ({ ...DEFAULT_DECK, ...d })));
    this.crossfader.set(mixer.crossfader);
  }

  /**
   * Position of a deck extrapolated to this instant, for smooth 60 fps drawing between
   * two status updates.
   */
  livePosition(deck: number): number | null {
    const d = this.latest?.decks[deck];
    if (!d?.loaded) return null;
    const elapsed = Math.min((performance.now() - this.latestAt) / 1000, 0.05);
    return d.positionS + d.speed * elapsed;
  }

  async load(deck: number, path: string): Promise<void> {
    this.patch(this.loading, deck, true);
    try {
      const info = await invoke<TrackInfo>('load_track', { deck, path });
      this.patch(this.tracks, deck, info);
      this.updateDeck(deck, { cueS: 0 });
      this.error.set(null);
    } catch (e) {
      this.error.set(String(e));
    } finally {
      this.patch(this.loading, deck, false);
    }
  }

  setPlaying(deck: number, playing: boolean) {
    return this.call('set_playing', { deck, playing });
  }

  seek(deck: number, seconds: number) {
    return this.call('seek', { deck, seconds: Math.max(0, seconds) });
  }

  setPitch(deck: number, ratio: number) {
    this.updateDeck(deck, { pitch: ratio });
    return this.call('set_pitch', { deck, ratio });
  }

  setVolume(deck: number, gain: number) {
    this.updateDeck(deck, { volume: gain });
    return this.call('set_volume', { deck, gain });
  }

  setVinyl(deck: number, changes: { enabled?: boolean; mode?: VinylMode; leadInS?: number }) {
    const d = this.updateDeck(deck, {
      vinylEnabled: changes.enabled ?? this.decks()[deck].vinylEnabled,
      vinylMode: changes.mode ?? this.decks()[deck].vinylMode,
      leadInS: changes.leadInS ?? this.decks()[deck].leadInS,
    });
    return this.call('set_vinyl', { deck, enabled: d.vinylEnabled, mode: d.vinylMode, leadInS: d.leadInS });
  }

  setCrossfader(position: number) {
    this.crossfader.set(position);
    return this.call('set_crossfader', { position });
  }

  /** CDJ-style cue: while playing, jump back to the cue point and stop; when stopped, set it. */
  cue(deck: number) {
    const d = this.status()?.decks[deck];
    if (!d?.loaded) return;
    if (d.playing) {
      this.setPlaying(deck, false);
      this.seek(deck, this.decks()[deck].cueS);
    } else {
      this.updateDeck(deck, { cueS: d.positionS });
    }
  }

  private updateDeck(deck: number, changes: Partial<DeckSettings>): DeckSettings {
    const next = { ...this.decks()[deck], ...changes };
    this.patch(this.decks, deck, next);
    return next;
  }

  private patch<T>(s: { (): T[]; set(v: T[]): void }, index: number, value: T) {
    const copy = [...s()];
    copy[index] = value;
    s.set(copy);
  }

  private async call(command: string, args: Record<string, unknown>): Promise<void> {
    try {
      await invoke(command, args);
    } catch (e) {
      this.error.set(String(e));
    }
  }
}
