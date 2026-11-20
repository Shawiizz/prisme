import { computed, Injectable, inject } from '@angular/core';
import { EngineService } from '../services/engine.service';
import { LibraryService } from '../services/library.service';
import { NavigationService } from '../services/navigation.service';
import { Control, buildControls, measuredControl } from './controls';

/** Tracks a page of the library moves by. */
const PAGE = 12;

/** A key's action, done as it is pressed: a pad hit softly is hit all the same. */
const pressed =
  (run: () => void) =>
  (value: number): void => {
    if (value > 0) run();
  };

/**
 * Every action a key or a controller can do (see `buildControls`), and the one way to do it
 * (`act`): the backend performs it, whoever asks (a key, the command palette), as it does for
 * the controllers; the app does itself only what it keeps (`own`): the library's browsing, its
 * windows. A controller's control driving one of those the backend tells (`ui-action`).
 */
@Injectable({ providedIn: 'root' })
export class ControlService {
  private readonly engine = inject(EngineService);
  private readonly library = inject(LibraryService);
  private readonly navigation = inject(NavigationService);

  /** What the app does of the actions it keeps, given a key's value or an encoder's steps. */
  private readonly own: Record<string, (value: number) => void> = {
    'library.browse': (steps) => this.browse(steps),
    'library.previous': pressed(() => this.browse(-1)),
    'library.next': pressed(() => this.browse(1)),
    'library.page.previous': pressed(() => this.browse(-PAGE)),
    'library.page.next': pressed(() => this.browse(PAGE)),
    'library.remove': pressed(() => this.library.removeSelected()),
    'library.prepare': pressed(() => this.library.addSelectedToPrepare()),
    'library.selectAll': pressed(() => this.library.selectAll()),
    'library.clearSelection': pressed(() => this.library.clearSelection()),
    'library.search': pressed(() => this.library.searchRequests.update((n) => n + 1)),
    'library.focus': pressed(() =>
      this.library.focus.update((f) => (f === 'tracks' ? 'lists' : 'tracks')),
    ),
    'library.lists': (steps) => this.library.movePointedList(steps),
    'library.list.open': pressed(() => this.library.openPointedList()),
    'library.list.next': pressed(() => this.library.openNextList(1)),
    'library.list.previous': pressed(() => this.library.openNextList(-1)),
    'library.go': pressed(() =>
      this.library.focus() === 'lists'
        ? this.library.openPointedList()
        : this.library.loadSelectedOnStoppedDeck(),
    ),
    'library.left': pressed(() => this.library.focus.set('lists')),
    'library.right': pressed(() => this.library.focus.set('tracks')),
    'library.sort.title': pressed(() => this.library.sortBy('title')),
    'library.sort.artist': pressed(() => this.library.sortBy('artist')),
    'library.sort.bpm': pressed(() => this.library.sortBy('bpm')),
    'library.sort.key': pressed(() => this.library.sortBy('key')),
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
  /** `inUse` as a set: looked into at every message of a controller. */
  private readonly inUseSet = computed(() => new Set(this.inUse()));

  /** The control `id`, whatever its deck (to name it, to map it, to put its light out). */
  find(id: string): Control | undefined {
    return this.built().byId.get(id) ?? this.measured(id);
  }

  /** The control `id` if it acts now: a control of a deck not shown does nothing. */
  findInUse(id: string): Control | undefined {
    const control = this.built().byId.get(id);
    if (control) return this.inUseSet().has(control) ? control : undefined;
    const measured = this.measured(id);
    return measured && measured.deck! < this.engine.deckIndexes().length ? measured : undefined;
  }

  /** The action `id` given `value` (a key's 1 pressed and 0 let go, a fader's position, an
   * encoder's steps), if it acts now. */
  act(id: string, value: number) {
    const control = this.findInUse(id);
    if (!control || control.kind === 'light') return;
    const own = this.own[id];
    if (own) own(value);
    else void this.engine.perform(id, value, control.kind === 'encoder');
  }

  /** Whether the light of `id` is on, for one the app keeps what it shows (the library over
   * the window): `undefined` for any other, which the backend lights. */
  lit(id: string): boolean | undefined {
    return id === 'general.library' ? this.navigation.libraryOnly() : undefined;
  }

  private browse(steps: number) {
    if (this.library.focus() === 'lists') this.library.movePointedList(steps);
    else this.library.moveSelection(steps);
  }

  /** The controls named by their measure (`deck1.jump.-8`), made once each. */
  private readonly measuredById = new Map<string, Control | undefined>();

  private measured(id: string): Control | undefined {
    if (!this.measuredById.has(id)) this.measuredById.set(id, measuredControl(id));
    return this.measuredById.get(id);
  }
}
