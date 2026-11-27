import { Injectable, inject, signal } from '@angular/core';
import { emit, listen } from '@tauri-apps/api/event';
import { PerformanceService } from './performance.service';

/** What a deck's pads do on the screen: their tab. */
export type PadMode = 'cues' | 'loops' | 'roll' | 'jump' | 'slice' | 'cut' | 'fx' | 'sampler';

/** The tabs whose pads come in pages of eight. */
const PAGED: readonly PadMode[] = ['cues', 'sampler'];

/** A deck's pads as shown: their tab, and their page of eight. */
interface Shown {
  mode: PadMode;
  page: number;
}

/** The windows show a deck's pads alike: one tells the others. */
const SHOWN_EVENT = 'pads-shown';

/**
 * The tab and the page of each deck's pads on the screen, the same wherever the deck is shown
 * and in every window: set from its tabs, or from a key or a controller (`deckN.pads.<tab>`,
 * `deckN.pads.page`).
 */
@Injectable({ providedIn: 'root' })
export class PadsService {
  private readonly performance = inject(PerformanceService);
  private readonly shown = signal<Shown[]>([]);

  /** The other windows' word: the pads follow. */
  async start() {
    await listen<Shown & { deck: number }>(SHOWN_EVENT, ({ payload: { deck, ...shown } }) =>
      this.set(deck, shown),
    );
  }

  mode(deck: number): PadMode {
    return this.shown()[deck]?.mode ?? 'cues';
  }

  page(deck: number): number {
    return this.shown()[deck]?.page ?? 0;
  }

  /** The pads of `deck` on the tab `mode`: the slicer is put on the bar the deck plays in. */
  choose(deck: number, mode: PadMode) {
    if (mode === 'slice') void this.performance.anchorSlicer(deck);
    this.tell(deck, { mode, page: this.page(deck) });
  }

  /** The other page of eight of `deck`'s pads, on a tab that has pages. */
  turnPage(deck: number) {
    const mode = this.mode(deck);
    if (PAGED.includes(mode)) this.tell(deck, { mode, page: 1 - this.page(deck) });
  }

  /** Whether `mode` names a tab of the pads. */
  static isMode(mode: string): mode is PadMode {
    return ['cues', 'loops', 'roll', 'jump', 'slice', 'cut', 'fx', 'sampler'].includes(mode);
  }

  private tell(deck: number, shown: Shown) {
    this.set(deck, shown);
    void emit(SHOWN_EVENT, { deck, ...shown });
  }

  private set(deck: number, shown: Shown) {
    this.shown.update((all) => {
      const next = [...all];
      next[deck] = shown;
      return next;
    });
  }
}
