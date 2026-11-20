import { computed, Injectable, inject } from '@angular/core';
import { EngineService } from '../services/engine.service';
import { LibraryService } from '../services/library.service';
import { NavigationService } from '../services/navigation.service';
import { Control, buildControls, measuredControl } from './controls';

/** A key's action, done as it is pressed: a pad hit softly is hit all the same. */
const pressed =
  (run: () => void) =>
  (value: number): void => {
    if (value > 0) run();
  };

/**
 * Every action a key or a controller can do (see `buildControls`), and the one way to do it
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

  /** What the app does of the actions it keeps, given a key's value. */
  private readonly own: Record<string, (value: number) => void> = {
    'library.search': pressed(() => this.library.searchRequests.update((n) => n + 1)),
    'general.library': pressed(() => this.navigation.libraryOnly.update((only) => !only)),
    'general.help': pressed(() => this.navigation.toggleSettings('shortcuts')),
    'general.palette': pressed(() => this.navigation.togglePalette()),
    'general.settings': pressed(() => this.navigation.openSettings()),
    // the backend reads a controller's shift as it reads the controller
    shift: () => undefined,
  };

  /** Built again when the language changes: their names are in it. */
  private readonly built = computed(() => {
    const controls = buildControls();
    return { controls, byId: new Map(controls.map((c) => [c.id, c])) };
  });

  /** Every control, named in the UI's language. */
  get controls(): Control[] {
    return this.built().controls;
  }

  /** The controls that act now: those of the decks shown, and those of no deck. */
  readonly inUse = computed(() => {
    const decks = this.engine.deckIndexes().length;
    return this.controls.filter((c) => c.deck === undefined || c.deck < decks);
  });

  /** The control `id`, whatever its deck (to name it, to map it, to put its light out). */
  find(id: string): Control | undefined {
    return this.built().byId.get(id) ?? this.measured(id);
  }

  /** The action `id` given `value` (a key's 1 pressed and 0 let go, a fader's position, an
   * encoder's steps): the app's own here, the others the backend's (which does nothing for one
   * of a deck not in use, or a light). */
  act(id: string, value: number) {
    const own = this.own[id];
    if (own) own(value);
    else void this.engine.perform(id, value);
  }

  /** Whether the light of `id` is on, for one the app keeps what it shows (the library over
   * the window): `undefined` for any other, which the backend lights. */
  lit(id: string): boolean | undefined {
    return id === 'general.library' ? this.navigation.libraryOnly() : undefined;
  }

  /** The controls named by their measure (`deck1.jump.-8`), made once each. */
  private readonly measuredById = new Map<string, Control | undefined>();

  private measured(id: string): Control | undefined {
    if (!this.measuredById.has(id)) this.measuredById.set(id, measuredControl(id));
    return this.measuredById.get(id);
  }
}
