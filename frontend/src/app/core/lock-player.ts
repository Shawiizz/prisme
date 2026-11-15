import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';
import { convertFileSrc } from '@tauri-apps/api/core';
import { CrossfaderCurve } from './models/engine';
import { EngineService } from './services/engine.service';
import { MixerService } from './services/mixer.service';
import { PreferencesService } from './services/preferences.service';

/** A deck as Android's player shows it (`Player.kt`). */
export interface PlayerDeck {
  deck: number;
  title: string;
  artist: string | null;
  playing: boolean;
}

/** What Android's player is told: the decks loaded, the one shown and where it is. */
export interface PlayerState {
  decks: PlayerDeck[];
  shown: number | null;
  /** The file of the deck shown, which its cover goes with. */
  path: string | null;
  positionS: number;
  durationS: number;
  /** How fast its position moves: 0 while it stands. */
  speed: number;
  controls: boolean;
}

/** What `MainActivity.kt` offers the page on Android. */
interface AndroidPlayer {
  update(state: string): void;
  cover(path: string, base64: string): void;
}

/** As `mixer.rs`: how far the sharp curve cuts. */
const SHARP_CUT = 0.04;
/** A deck turning slower than this stands (a record held, a jog let go). */
const MOVING_SPEED = 0.25;
/** Android moves the position on by itself: told again past this much of a drift. */
const DRIFT_S = 1;

/** The crossfader's gain on `deck`, as `mixer.rs` has it: A and C on its left, B and D on its
 * right. */
export function crossfaderGain(deck: number, position: number, curve: CrossfaderCurve): number {
  const x = (Math.min(Math.max(position, -1), 1) + 1) / 2;
  const away = deck % 2 === 0 ? x : 1 - x;
  return curve === 'smooth' ? Math.cos((away * Math.PI) / 2) : Math.min((1 - away) / SHARP_CUT, 1);
}

/** Of the decks `moving`, the one heard: the loudest through the software mixer (`level`), the
 * last started when they are as loud or when a mixer of its own mixes them (`level` null). */
export function heardDeck(
  moving: number[],
  startedAt: readonly number[],
  level: ((deck: number) => number) | null,
): number | null {
  const later = (a: number, b: number) => (startedAt[b] > startedAt[a] ? b : a);
  const louder = (a: number, b: number) => {
    if (!level) return later(a, b);
    const [la, lb] = [level(a), level(b)];
    return Math.abs(la - lb) < 1e-3 ? later(a, b) : lb > la ? b : a;
  };
  return moving.length ? moving.reduce(louder) : null;
}

/**
 * The player of Android's lock screen and notifications: the track heard, its cover and where
 * it is, a button for each deck. Android keeps the sound going while a deck plays, and a while
 * after (`PlaybackService.kt`).
 */
@Injectable({ providedIn: 'root' })
export class LockPlayer {
  private readonly engine = inject(EngineService);
  private readonly mixer = inject(MixerService);
  private readonly preferences = inject(PreferencesService);
  private readonly android = (window as { prismeDecks?: AndroidPlayer }).prismeDecks;

  /** When each deck last started to move. */
  private readonly startedAt = signal<number[]>([]);
  private readonly moving = computed(() => {
    const decks = this.engine.status()?.decks ?? [];
    const tracks = this.engine.tracks();
    return this.engine
      .deckIndexes()
      .filter(
        (d) => tracks[d] && (decks[d]?.playing || Math.abs(decks[d]?.speed ?? 0) > MOVING_SPEED),
      );
  });
  private readonly heard = computed(() => {
    const mixer = this.mixer;
    const level = mixer.internal()
      ? (d: number) =>
          this.engine.decks()[d].volume *
          crossfaderGain(d, mixer.crossfader(), mixer.crossfaderCurve())
      : null;
    return heardDeck(this.moving(), this.startedAt(), level);
  });
  /** The deck heard last, shown while all stand. */
  private readonly lastHeard = signal<number | null>(null);
  private readonly shown = computed(() => {
    const tracks = this.engine.tracks();
    const last = this.lastHeard();
    return (
      this.heard() ??
      (last !== null && tracks[last] ? last : null) ??
      this.engine.deckIndexes().find((d) => tracks[d]) ??
      null
    );
  });

  private sent: PlayerState | null = null;
  private sentAt = 0;
  private coverPath: string | null = null;

  /** Tells Android, as long as Prisme runs, what its player shows. */
  start() {
    const android = this.android;
    if (!android) return;
    (window as { prismePlayer?: unknown }).prismePlayer = {
      toggle: (deck: number) => this.engine.togglePlay(deck),
    };
    let wasMoving = new Set<number>();
    effect(() => {
      const moving = new Set(this.moving());
      const started = [...moving].filter((d) => !wasMoving.has(d));
      wasMoving = moving;
      if (started.length) {
        const now = performance.now();
        untracked(() =>
          this.startedAt.update((at) => {
            const next = [...at];
            started.forEach((d) => (next[d] = now));
            return next;
          }),
        );
      }
    });
    effect(() => {
      const heard = this.heard();
      if (heard !== null) untracked(() => this.lastHeard.set(heard));
    });
    effect(() => this.tell(android, this.state()));
    effect(() => {
      const shown = this.shown();
      const path = shown === null ? null : (this.engine.tracks()[shown]?.path ?? null);
      if (path !== this.coverPath) void this.sendCover(android, path);
    });
  }

  private state(): PlayerState {
    const tracks = this.engine.tracks();
    const status = this.engine.status()?.decks ?? [];
    const moving = this.moving();
    const decks = this.engine
      .deckIndexes()
      .filter((d) => tracks[d])
      .map((d) => ({
        deck: d,
        title: tracks[d]!.title,
        artist: tracks[d]!.artist,
        playing: moving.includes(d),
      }));
    const shown = this.shown();
    const deck = shown === null ? null : status[shown];
    return {
      decks,
      shown,
      path: shown === null ? null : tracks[shown]!.path,
      positionS: deck?.positionS ?? 0,
      durationS: deck?.durationS ?? 0,
      speed: shown !== null && moving.includes(shown) ? (deck?.speed ?? 0) : 0,
      controls: this.preferences.dj().lockControls,
    };
  }

  /** Tells Android when what it shows changed, or when the position it moves on by itself
   * drifted from the deck's (a jump, a loop, a scratch). */
  private tell(android: AndroidPlayer, state: PlayerState) {
    const now = performance.now();
    const sent = this.sent;
    const same =
      sent !== null &&
      JSON.stringify({ ...sent, positionS: 0, speed: 0 }) ===
        JSON.stringify({ ...state, positionS: 0, speed: 0 }) &&
      Math.abs(sent.speed - state.speed) < 0.005 &&
      Math.abs(sent.positionS + (sent.speed * (now - this.sentAt)) / 1000 - state.positionS) <
        DRIFT_S;
    if (same) return;
    this.sent = state;
    this.sentAt = now;
    android.update(JSON.stringify(state));
  }

  private async sendCover(android: AndroidPlayer, path: string | null) {
    this.coverPath = path;
    if (path === null) return;
    let base64 = '';
    try {
      const response = await fetch(convertFileSrc(path, 'cover'));
      if (response.ok) base64 = await asBase64(await response.blob());
    } catch {
      // no cover: the logo of Prisme instead
    }
    if (this.coverPath === path) android.cover(path, base64);
  }
}

function asBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).replace(/^data:[^,]*,/, ''));
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(blob);
  });
}
