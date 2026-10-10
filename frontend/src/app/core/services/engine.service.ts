import { Injectable, computed, inject, signal } from '@angular/core';
import { Channel, invoke } from '@tauri-apps/api/core';
import { Cue, DECK_COUNT, DeckSettings, EngineStatus, MixerState, TrackInfo, VinylMode } from '../models/engine';
import { PreferencesService } from './preferences.service';

/** A deck this close to its cue point counts as sitting on it. */
const CUE_TOLERANCE_S = 0.02;
const MAIN_CUE_COLOR = 0xffb02e;
/** Speed change while a bend key is held. */
const PITCH_BEND = 0.03;

const DEFAULT_DECK: DeckSettings = {
  pitch: 1,
  volume: 1,
  vinylEnabled: false,
  vinylMode: 'absolute',
  leadInS: 0,
};

/**
 * Bridge to the Rust engine: commands go through `invoke`, the engine state arrives
 * ~60 times per second through a Tauri channel.
 */
@Injectable({ providedIn: 'root' })
export class EngineService {
  private readonly preferences = inject(PreferencesService);

  /** Latest engine state (drives the templates). */
  readonly status = signal<EngineStatus | null>(null);
  readonly tracks = signal<(TrackInfo | null)[]>(Array(DECK_COUNT).fill(null));
  readonly loading = signal<boolean[]>(Array(DECK_COUNT).fill(false));
  readonly decks = signal<DeckSettings[]>(Array.from({ length: DECK_COUNT }, () => ({ ...DEFAULT_DECK })));
  readonly crossfader = signal(0);
  readonly error = signal<string | null>(null);

  readonly audioRunning = computed(() => this.status()?.audioRunning ?? false);

  private readonly previewing = new Set<number>();
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

  /** Playback rate of the tempo the engine reports (sync and platter included), if the track has a beat grid. */
  tempoRate(deck: number): number | null {
    const bpm = this.status()?.decks[deck]?.bpm;
    const trackBpm = this.tracks()[deck]?.tempo?.bpm;
    return bpm && trackBpm ? bpm / trackBpm : null;
  }

  async load(deck: number, path: string): Promise<void> {
    this.patch(this.loading, deck, true);
    try {
      const info = await invoke<TrackInfo>('load_track', { deck, path });
      this.patch(this.tracks, deck, info);
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

  /**
   * SYNC button: unlocks a locked deck; otherwise locks it, or on a relative record in
   * one-shot mode matches it once (pressing again re-matches after a drift).
   */
  toggleSync(deck: number) {
    const status = this.status()?.decks[deck];
    if (!status) return;
    if (status.sync.locked) {
      return this.call('set_sync_locked', { deck, locked: false });
    }
    const oneShot = this.decks()[deck].vinylEnabled && this.preferences.dj().vinylSync === 'once';
    return oneShot ? this.call('sync_once', { deck }) : this.call('set_sync_locked', { deck, locked: true });
  }

  /**
   * CDJ cue, pressed: while playing, back to the cue point and stop; stopped on the cue
   * point, play while held; stopped elsewhere, set the cue point there.
   */
  async cueDown(deck: number) {
    const d = this.status()?.decks[deck];
    if (!d?.loaded) return;
    const cue = this.mainCue(deck)?.positionS ?? 0;
    if (d.playing) {
      this.returnToCue(deck);
    } else if (Math.abs(d.positionS - cue) < CUE_TOLERANCE_S) {
      this.previewing.add(deck);
      this.setPlaying(deck, true);
    } else {
      await this.setMainCue(deck, d.positionS);
    }
  }

  /** CDJ cue, released: a preview goes back to the cue point. */
  cueUp(deck: number) {
    if (this.previewing.delete(deck)) this.returnToCue(deck);
  }

  /** -1, 0 or 1: slower, released, faster. */
  pitchBend(deck: number, direction: -1 | 0 | 1) {
    return this.call('set_pitch_bend', { deck, amount: direction * PITCH_BEND });
  }

  private returnToCue(deck: number) {
    this.setPlaying(deck, false);
    this.seek(deck, this.mainCue(deck)?.positionS ?? 0);
  }

  /** Stores the cue point (the backend may move it onto the beat) and goes there. */
  private async setMainCue(deck: number, seconds: number) {
    try {
      const positionS = await invoke<number>('set_main_cue', { deck, seconds });
      const main: Cue = { hot: null, positionS, loopLengthS: null, color: MAIN_CUE_COLOR, label: '' };
      this.setCues(deck, [...(this.tracks()[deck]?.cues ?? []).filter((c) => c.hot !== null), main]);
      this.seek(deck, positionS);
    } catch (e) {
      this.error.set(String(e));
    }
  }

  mainCue(deck: number): Cue | null {
    return this.tracks()[deck]?.cues.find((c) => c.hot === null) ?? null;
  }

  /** Replaces the cues of the track on `deck` (after the backend changed them). */
  setCues(deck: number, cues: Cue[]) {
    const track = this.tracks()[deck];
    if (track) this.patch(this.tracks, deck, { ...track, cues });
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

  /** Runs a backend command; a failure is shown rather than thrown. */
  async call(command: string, args: Record<string, unknown>): Promise<void> {
    try {
      await invoke(command, args);
    } catch (e) {
      this.error.set(String(e));
    }
  }
}
