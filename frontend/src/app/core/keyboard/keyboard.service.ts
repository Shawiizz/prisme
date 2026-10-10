import { Injectable, inject, signal } from '@angular/core';
import { EngineService } from '../services/engine.service';
import { LibraryService } from '../services/library.service';
import { PerformanceService } from '../services/performance.service';
import { buildShortcuts, matches, Shortcut } from './shortcuts';

interface KeyboardLayoutApi {
  keyboard?: { getLayoutMap(): Promise<Map<string, string>> };
}

const LOOP_BEATS = 4;

/** Plays the keyboard shortcuts; keys held for an action are released on key up or on focus loss. */
@Injectable({ providedIn: 'root' })
export class KeyboardService {
  private readonly engine = inject(EngineService);
  private readonly performance = inject(PerformanceService);
  private readonly library = inject(LibraryService);

  readonly helpOpen = signal(false);
  /** Key code -> what the user's keyboard layout prints on that key. */
  private readonly layout = signal(new Map<string, string>());
  private readonly held = new Map<string, Shortcut>();

  readonly shortcuts: Shortcut[] = buildShortcuts({
    hotCue: (deck, slot) => this.performance.pressHotCue(deck, slot),
    cue: (deck, pressed) => (pressed ? this.engine.cueDown(deck) : this.engine.cueUp(deck)),
    togglePlay: (deck) => {
      const status = this.engine.status()?.decks[deck];
      if (status?.loaded) this.engine.setPlaying(deck, !status.playing);
    },
    sync: (deck) => this.engine.toggleSync(deck),
    bend: (deck, direction) => this.engine.pitchBend(deck, direction),
    toggleLoop: (deck) =>
      this.engine.status()?.decks[deck]?.looping.active
        ? this.performance.exitLoop(deck)
        : this.performance.beatLoop(deck, LOOP_BEATS),
    scaleLoop: (deck, factor) => this.performance.scaleLoop(deck, factor),
    moveSelection: (delta) => this.library.moveSelection(delta),
    loadSelected: (deck) => this.library.loadSelected(deck),
    focusSearch: () => this.library.searchRequests.update((n) => n + 1),
    toggleHelp: () => this.helpOpen.update((open) => !open),
  });

  async start(): Promise<void> {
    window.addEventListener('keydown', (e) => this.onKeyDown(e));
    window.addEventListener('keyup', (e) => this.onKeyUp(e));
    window.addEventListener('blur', () => this.releaseAll());
    const api = (navigator as Navigator & KeyboardLayoutApi).keyboard;
    if (api) this.layout.set(await api.getLayoutMap());
  }

  /** "Ctrl + F", "Shift + →", "A" as printed on the user's keyboard. */
  label(shortcut: Shortcut): string {
    const key = this.layout().get(shortcut.code)?.toUpperCase() ?? KEY_NAMES[shortcut.code] ?? shortcut.code.replace(/^(Key|Digit)/, '');
    return [shortcut.ctrl && 'Ctrl', shortcut.shift && 'Shift', key].filter(Boolean).join(' + ');
  }

  private onKeyDown(event: KeyboardEvent) {
    const shortcut = this.shortcuts.find((s) => matches(s, event));
    if (!shortcut || (isTyping(event) && shortcut.code !== 'F1')) return;
    event.preventDefault();
    if (event.repeat || this.held.has(event.code)) return;
    shortcut.press();
    if (shortcut.release) this.held.set(event.code, shortcut);
  }

  private onKeyUp(event: KeyboardEvent) {
    const shortcut = this.held.get(event.code);
    if (!shortcut) return;
    this.held.delete(event.code);
    shortcut.release?.();
  }

  private releaseAll() {
    for (const shortcut of this.held.values()) shortcut.release?.();
    this.held.clear();
  }
}

const KEY_NAMES: Record<string, string> = {
  ArrowUp: '↑',
  ArrowDown: '↓',
  ArrowLeft: '←',
  ArrowRight: '→',
  Comma: ',',
  Period: '.',
};

function isTyping(event: KeyboardEvent): boolean {
  const target = event.target as HTMLElement | null;
  return !!target && (['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName) || target.isContentEditable);
}
