import { computed, effect, Injectable, inject, signal, untracked } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { emitTo } from '@tauri-apps/api/event';
import { EngineService } from '../services/engine.service';
import { LibraryService } from '../services/library.service';
import { NavigationService } from '../services/navigation.service';
import { labelText } from '../i18n/i18n';
import { OverlayService } from '../../shared/overlay';
import { WindowService } from '../services/window.service';
import { DECKS_WINDOW } from '../platform';
import { Control } from './controls';
import type { Target } from '../models/backend';

/** A key's action, done as it is pressed: a pad hit softly is hit all the same. */
const pressed =
  (run: () => void) =>
  (value: number): void => {
    if (value > 0) run();
  };

/** `target` named in the UI's language: read in a `computed`, it follows the language. */
export function named(target: Target): Control {
  const { id, kind, inUse } = target;
  return { id, kind, inUse, group: labelText(target.group), label: labelText(target.label) };
}

/**
 * Every action a key or a controller can do, as the backend lists them, and the one way to do it
 * (`act`): the backend performs it, whoever asks (a key, the command palette), as it does for
 * the controllers, the library's browsing included; the app does itself only what it keeps
 * (`own`): its windows, the focus of its search. A controller's control driving one of those
 * the backend tells (`ui-action`).
 */
@Injectable({ providedIn: 'root' })
export class ControlService {
  private readonly engine = inject(EngineService);
  private readonly library = inject(LibraryService);
  private readonly navigation = inject(NavigationService);
  private readonly overlays = inject(OverlayService);
  private readonly window = inject(WindowService);

  /** What the app does of the actions it keeps, given a key's value. */
  private readonly own: Record<string, (value: number) => void> = {
    'library.search': pressed(() => {
      this.navigation.showLibrary();
      this.library.searchWanted.set(true);
    }),
    'general.help': pressed(() => this.navigation.toggleSettings('shortcuts')),
    'general.palette': pressed(() => this.navigation.togglePalette()),
    'general.settings': pressed(() => this.navigation.openSettings()),
    'general.fullscreen': pressed(() => void this.window.toggleFullscreen()),
    'general.close': pressed(() => this.overlays.closeTop()),
  };

  private readonly targets = signal<Target[]>([]);
  /** The lists asked for: a slower answer to an older one is dropped. */
  private asked = 0;

  /** Every action, named in the UI's language. */
  readonly controls = computed(() => this.targets().map(named));

  /** The actions that act now: those of the decks shown, and those of no deck. */
  readonly inUse = computed(() => this.controls().filter((c) => c.inUse));

  private readonly byId = computed(() => new Map(this.controls().map((c) => [c.id, c])));

  constructor() {
    // the decks in use tell which actions act
    effect(() => {
      this.engine.deckIndexes();
      untracked(() => void this.load());
    });
  }

  private async load() {
    const asked = ++this.asked;
    const targets = await invoke<Target[]>('list_targets');
    if (asked === this.asked) this.targets.set(targets);
  }

  /** The action `id`, if listed. */
  find(id: string): Control | undefined {
    return this.byId().get(id);
  }

  /** The action `id` given `value` (a key's 1 pressed and 0 let go, a fader's position, an
   * encoder's steps): the app's own here, the others the backend's (which does nothing for one
   * of a deck not in use, or a light). */
  act(id: string, value: number) {
    const own = id in this.own ? this.own[id] : undefined;
    // the decks' own window has no library, dialogs or title bar: the main one does those, but
    // closes what is open over the decks
    if (own === undefined) {
      void this.engine.perform(id, value);
    } else if (DECKS_WINDOW && id !== 'general.close') {
      void emitTo('main', 'ui-action', { target: id, value });
    } else own(value);
  }
}
