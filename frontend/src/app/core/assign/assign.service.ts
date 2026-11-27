import { Injectable, computed, effect, inject, signal, untracked } from '@angular/core';
import { emit, listen } from '@tauri-apps/api/event';
import { ControlService } from '../control/control.service';
import { ControllerService } from '../control/controller.service';
import { KeyboardService } from '../keyboard/keyboard.service';
import { PreferencesService } from '../services/preferences.service';
import { EngineService } from '../services/engine.service';
import { errorText } from '../errors';
import { describeMapping } from '../models/controllers';
import type { Key, TakenKey } from '../models/backend';

/** What gives an action: a key of the keyboard, or a control of a controller moved. */
export type AssignWay = 'keys' | 'controller';

/** The windows are in the mode together: one tells the others. */
const MODE_EVENT = 'assign-mode';
/** What the mode lets through: its bar and its bubble. */
const ITS_OWN = '[data-assign-ui]';
/** What a press would start, held back while assigning: nothing plays. */
const HELD_BACK = [
  'pointerdown',
  'pointerup',
  'mousedown',
  'mouseup',
  'click',
  'dblclick',
  'contextmenu',
  'wheel',
  'dragstart',
] as const;

/** The action chosen and where it is, for its bubble. */
export interface Chosen {
  control: string;
  element: HTMLElement;
}

/** Where the bubble of the action chosen is. */
export type Step =
  | { kind: 'listening' }
  /** The key pressed is another action's: given once the DJ says so. */
  | { kind: 'taken'; key: Key; from: TakenKey[] }
  /** The control moved on the controller drives it now. */
  | { kind: 'learnt' };

/**
 * The mode where the DJ gives an action its key, or its control on a controller, from the button
 * that does it on the screen: the buttons that can take one are outlined, their keys shown; one
 * pressed waits for a key, or for a control moved. Nothing plays while the mode is on: every
 * press but on the mode's own bar and bubble is held back. The windows are in it together.
 */
@Injectable({ providedIn: 'root' })
export class AssignService {
  private readonly keyboard = inject(KeyboardService);
  private readonly controls = inject(ControlService);
  private readonly controllers = inject(ControllerService);
  private readonly preferences = inject(PreferencesService);
  private readonly engine = inject(EngineService);

  readonly on = signal(false);
  readonly way = signal<AssignWay>('keys');
  readonly chosen = signal<Chosen | null>(null);
  readonly step = signal<Step | null>(null);
  /** A controller is open: its controls can be learnt. */
  readonly canLearn = this.controllers.canLearn;
  /** The action whose key the one pressed would take: shown. */
  readonly pointed = computed(() => {
    const step = this.step();
    return step?.kind === 'taken' ? (step.from[0]?.control ?? null) : null;
  });

  constructor() {
    // the control moved on the controller learnt: what it drives is shown, then the bubble goes
    let learning: string | null = null;
    effect(() => {
      if (!this.on()) return;
      const now = this.controllers.learning();
      untracked(() => {
        const chosen = this.chosen();
        const waited = this.way() === 'controller' && this.step()?.kind === 'listening';
        if (learning !== null && now === null && waited && chosen?.control === learning) {
          this.step.set({ kind: 'learnt' });
        }
        learning = now;
      });
    });
  }

  /** The other windows' word: the mode follows. */
  async start() {
    await listen<boolean>(MODE_EVENT, ({ payload }) => (payload ? this.enter() : this.leave()));
  }

  toggle() {
    if (this.on()) this.stop();
    else this.begin();
  }

  /** The mode on, in every window. */
  begin() {
    this.enter();
    void emit(MODE_EVENT, true);
  }

  /** The mode off, in every window. */
  stop() {
    this.leave();
    void emit(MODE_EVENT, false);
  }

  choose(way: AssignWay) {
    if (way === this.way()) return;
    this.drop();
    this.way.set(way);
  }

  /** `control` can take what the mode gives: a key only for a key's action. */
  allows(control: string): boolean {
    const found = this.controls.find(control);
    if (!found?.inUse) return false;
    return this.way() === 'controller' ? this.canLearn() : found.kind === 'button';
  }

  /** What names the action: "PLAY · Deck A". */
  name(control: string): { label: string; group: string } {
    const found = this.controls.find(control);
    return { label: found?.label ?? control, group: found?.group ?? '' };
  }

  /** The keys of `control`, as printed on the user's keyboard. */
  keysOf(control: string): string[] {
    return this.keyboard.keysOf(control);
  }

  /** The controls that drive `control` on the controllers in use: "Note 12 · ch 1". */
  controlsOf(control: string): { device: string; name: string }[] {
    return this.controllers
      .mappings()
      .filter((m) => m.target.id === control)
      .flatMap((m) =>
        m.devices.flatMap((d) =>
          d.mappings.map((mapping) => ({
            device: d.device,
            name: mapping.name ?? describeMapping(mapping),
          })),
        ),
      );
  }

  /** What shows on the action's button: its first key, or that a controller drives it. */
  cap(control: string): string | null {
    if (this.way() === 'keys') return this.keyboard.firstKey(control);
    return this.controlsOf(control)[0]?.name ?? null;
  }

  /** The bubble on `control`: a key or a control moved waited for. */
  pick(control: string, element: HTMLElement) {
    this.drop();
    this.chosen.set({ control, element });
    this.step.set({ kind: 'listening' });
    if (this.way() === 'controller') void this.controllers.learn(control);
    else void this.listen(control);
  }

  /** The bubble closed, what it waited for given up. */
  drop() {
    const chosen = this.chosen();
    this.chosen.set(null);
    this.step.set(null);
    if (!chosen) return;
    this.keyboard.cancelCapture();
    if (this.controllers.learning() === chosen.control) void this.controllers.cancelLearning();
  }

  /** The key the action chosen was to take, taken from the action that has it. */
  take() {
    const chosen = this.chosen();
    const step = this.step();
    if (!chosen || step?.kind !== 'taken') return;
    void this.give(chosen.control, [step.key], true);
  }

  /** Another key waited for, the one taken left where it is. */
  again() {
    const chosen = this.chosen();
    if (!chosen) return;
    this.step.set({ kind: 'listening' });
    if (this.way() === 'controller') void this.controllers.learn(chosen.control);
    else void this.listen(chosen.control);
  }

  /** The key `index` of the action chosen taken off it. */
  removeKey(index: number) {
    const chosen = this.chosen();
    if (!chosen) return;
    const keys = this.preferences.shortcuts().filter((s) => s.control === chosen.control);
    const left: Key[] = keys
      .filter((_, i) => i !== index)
      .map(({ code, ctrl, shift }) => ({ code, ctrl, shift }));
    void this.give(chosen.control, left, false, false);
  }

  /** The action chosen given back the key Prisme gives it. */
  resetKeys() {
    const chosen = this.chosen();
    if (chosen) void this.give(chosen.control, null, false, false);
  }

  /** The control `name` of `device` no longer drives the action chosen. */
  forget(device: string) {
    const chosen = this.chosen();
    if (chosen) void this.controllers.forget(device, chosen.control);
  }

  private async listen(control: string) {
    const key = await this.keyboard.capture();
    if (this.chosen()?.control !== control || this.step()?.kind !== 'listening') return;
    if (key === null) this.drop();
    else await this.give(control, [key], false);
  }

  /** `keys` to `control` (`null`: Prisme's): if another action has one, the DJ is asked, else
   * the bubble closes (`close`), or stays to show the keys left. */
  private async give(control: string, keys: Key[] | null, take: boolean, close = true) {
    try {
      const taken = await this.preferences.setKeys(control, keys, take);
      if (this.chosen()?.control !== control) return;
      if (taken.length && keys?.[0]) this.step.set({ kind: 'taken', key: keys[0], from: taken });
      else if (close) this.drop();
    } catch (e) {
      this.engine.error.set(errorText(e));
    }
  }

  private enter() {
    if (this.on()) return;
    this.on.set(true);
    this.keyboard.asleep.set(true);
    document.documentElement.classList.add('pr-assigning');
    for (const type of HELD_BACK) window.addEventListener(type, this.hold, HOLD_OPTIONS);
    window.addEventListener('keydown', this.escape);
  }

  private leave() {
    if (!this.on()) return;
    this.drop();
    this.on.set(false);
    this.keyboard.asleep.set(false);
    document.documentElement.classList.remove('pr-assigning');
    for (const type of HELD_BACK) window.removeEventListener(type, this.hold, HOLD_OPTIONS);
    window.removeEventListener('keydown', this.escape);
  }

  /** A press held back, but on the mode's own bar and bubble: on a button that can take what the
   * mode gives, the button picked; elsewhere, the bubble closed. */
  private readonly hold = (event: Event) => {
    const target = event.target instanceof Element ? event.target : null;
    if (target?.closest(ITS_OWN)) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    if (event.type !== 'pointerdown' || (event as PointerEvent).button !== 0) return;
    const element = target?.closest<HTMLElement>('[data-assign]');
    const control = element?.dataset['assign'];
    if (element && control && this.allows(control)) this.pick(control, element);
    else this.drop();
  };

  /** Escape closes the bubble, or else ends the mode (a key waited for takes it first). */
  private readonly escape = (event: KeyboardEvent) => {
    if (event.code !== 'Escape') return;
    event.preventDefault();
    if (this.chosen()) this.drop();
    else this.stop();
  };
}

/** Before the page's own listeners, and able to hold back a wheel. */
const HOLD_OPTIONS: AddEventListenerOptions = { capture: true, passive: false };
