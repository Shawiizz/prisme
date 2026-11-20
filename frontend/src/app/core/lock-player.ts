import { Injectable, Injector, computed, effect, inject } from '@angular/core';
import { convertFileSrc } from '@tauri-apps/api/core';
import { EngineService } from './services/engine.service';
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

/** What the player shows but where the deck shown is, which Android moves on by itself. */
export type PlayerShowing = Omit<PlayerState, 'positionS' | 'speed'>;

/** What Android was last told, and when (`performance.now()`). */
export interface PlayerTold {
  showing: PlayerShowing;
  positionS: number;
  speed: number;
  at: number;
}

/** What `MainActivity.kt` offers the page on Android. */
interface AndroidPlayer {
  update(state: string): void;
  cover(path: string, base64: string): void;
}

/** Android moves the position on by itself: told again past this much of a drift. */
const DRIFT_S = 1;
/** A change of speed smaller than this is not told. */
const SPEED_STEP = 0.005;

/** Whether the player shows the same in both, where the deck shown is aside. */
export function sameShowing(a: PlayerShowing, b: PlayerShowing): boolean {
  return (
    a.shown === b.shown &&
    a.path === b.path &&
    a.durationS === b.durationS &&
    a.controls === b.controls &&
    a.decks.length === b.decks.length &&
    a.decks.every(
      (d, i) =>
        d.deck === b.decks[i].deck &&
        d.title === b.decks[i].title &&
        d.artist === b.decks[i].artist &&
        d.playing === b.decks[i].playing,
    )
  );
}

/** Whether Android, told `told`, still shows the deck at `positionS` and `speed` at `now` (ms):
 * the same speed, and the position it moved on by itself close to the deck's. */
export function stillTrue(
  told: PlayerTold,
  positionS: number,
  speed: number,
  now: number,
): boolean {
  const moved = told.positionS + (told.speed * (now - told.at)) / 1000;
  return Math.abs(told.speed - speed) < SPEED_STEP && Math.abs(moved - positionS) < DRIFT_S;
}

/**
 * The player of Android's lock screen and notifications: the track of the deck the backend says
 * is heard, its cover and where it is, a button for each deck. Android keeps the sound going while a deck plays, and a while
 * after (`PlaybackService.kt`).
 */
@Injectable({ providedIn: 'root' })
export class LockPlayer {
  private readonly engine = inject(EngineService);
  private readonly preferences = inject(PreferencesService);
  private readonly injector = inject(Injector);
  private readonly android = (window as { prismeDecks?: AndroidPlayer }).prismeDecks;

  private readonly shown = computed(() => this.engine.status()?.player ?? null);
  private readonly showing = computed<PlayerShowing>(
    () => {
      const tracks = this.engine.tracks();
      const status = this.engine.status();
      const shown = this.shown();
      const deck = shown === null ? null : status?.decks[shown];
      return {
        decks: this.engine
          .deckIndexes()
          .filter((d) => tracks[d])
          .map((d) => ({
            deck: d,
            title: tracks[d]!.title,
            artist: tracks[d]!.artist,
            playing: status?.decks[d]?.heard ?? false,
          })),
        shown,
        path: shown === null ? null : tracks[shown]!.path,
        durationS: deck?.durationS ?? 0,
        controls: this.preferences.dj().lockControls,
      };
    },
    { equal: sameShowing },
  );

  private told: PlayerTold | null = null;
  private coverPath: string | null = null;
  private started = false;

  /** Tells Android, as long as Prisme runs, what its player shows. */
  start() {
    const android = this.android;
    if (!android || this.started) return;
    this.started = true;
    (window as { prismePlayer?: unknown }).prismePlayer = {
      toggle: (deck: number) => this.engine.togglePlay(deck),
    };
    const injector = this.injector;
    effect(
      () => {
        const showing = this.showing();
        const shown = showing.shown;
        const deck = shown === null ? null : this.engine.status()?.decks[shown];
        const speed = deck?.heard ? deck.speed : 0;
        this.tell(android, showing, deck?.positionS ?? 0, speed);
      },
      { injector },
    );
    effect(
      () => {
        const path = this.showing().path;
        if (path !== this.coverPath) void this.sendCover(android, path);
      },
      { injector },
    );
  }

  /** Tells Android when what it shows changed, or when the position it moves on by itself
   * drifted from the deck's (a jump, a loop, a scratch). */
  private tell(android: AndroidPlayer, showing: PlayerShowing, positionS: number, speed: number) {
    const now = performance.now();
    const told = this.told;
    if (told?.showing === showing && stillTrue(told, positionS, speed, now)) return;
    this.told = { showing, positionS, speed, at: now };
    const state: PlayerState = { ...showing, positionS, speed };
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
