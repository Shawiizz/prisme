import { Component, OnDestroy, computed, inject, signal } from '@angular/core';
import { ControlService } from '../../core/control/control.service';
import { Control } from '../../core/control/controls';
import { KeyboardService } from '../../core/keyboard/keyboard.service';
import { CTRL } from '../../core/keyboard/shortcuts';
import { PreferencesService } from '../../core/services/preferences.service';
import { EngineService } from '../../core/services/engine.service';
import { errorText } from '../../core/errors';
import { invoke } from '@tauri-apps/api/core';
import { labelText, t } from '../../core/i18n/i18n';
import { TPipe } from '../../core/i18n/t.pipe';
import type { Key, TakenKey } from '../../core/models/backend';

interface Line {
  control: Control;
  /** Its keys, as printed on the user's keyboard. */
  keys: string[];
  /** The DJ changed them. */
  changed: boolean;
}

/** Keys given to a control that other controls have: they are taken only once the DJ says so. */
interface Taking {
  control: string;
  /** `null`: Prisme's. */
  keys: Key[] | null;
  /** What the keys are taken from: "Ctrl + K: Command palette". */
  from: string[];
}

/**
 * Settings page: the keyboard shortcuts, by group, with the keys as printed on the user's
 * keyboard. A key is changed by pressing it, taken off, or reset to Prisme's, one by one or all
 * at once; a key another action has is taken from it once the DJ says so.
 */
@Component({
  selector: 'app-shortcuts-settings',
  templateUrl: './shortcuts-settings.html',
  styleUrl: './shortcuts-settings.scss',
  imports: [TPipe],
})
export class ShortcutsSettings implements OnDestroy {
  private readonly keyboard = inject(KeyboardService);
  private readonly controls = inject(ControlService);
  private readonly preferences = inject(PreferencesService);
  private readonly engine = inject(EngineService);

  /** Words looked for among every action, to give one a key. */
  protected readonly query = signal('');
  /** The control waiting for its key. */
  protected readonly capturing = signal<string | null>(null);
  protected readonly taking = signal<Taking | null>(null);
  /** Reset all was pressed once: a second press resets. */
  protected readonly resetAsked = signal(false);

  protected readonly changedCount = computed(() => this.preferences.changed().length);
  protected readonly changedText = computed(() => {
    const n = this.changedCount();
    return n === 0
      ? t('Every action has the key Prisme gives it.')
      : n === 1
        ? t('1 action with a key of your own')
        : t('{n} actions with keys of your own', { n });
  });
  protected readonly CTRL = CTRL;

  protected readonly groups = computed(() => {
    const changed = new Set(this.preferences.changed());
    const shortcuts = this.keyboard.shortcuts();
    const words = this.query().toLowerCase().split(/\s+/).filter(Boolean);
    const groups = new Map<string, Line[]>();
    for (const control of this.controls.controls().filter((c) => c.kind === 'button')) {
      const keys = shortcuts
        .filter((s) => s.control === control.id)
        .map((s) => this.keyboard.label(s));
      const mine = changed.has(control.id);
      const text = `${control.label} ${control.group}`.toLowerCase();
      const shown =
        words.length > 0 ? words.every((w) => text.includes(w)) : keys.length > 0 || mine;
      if (!shown) continue;
      groups.set(control.group, [
        ...(groups.get(control.group) ?? []),
        { control, keys, changed: mine },
      ]);
    }
    return [...groups.entries()].map(([name, lines]) => ({ name, lines }));
  });

  /** Waits for the key of `control`; pressed again, gives up. */
  protected async change(control: string) {
    if (this.capturing() === control) {
      this.keyboard.cancelCapture();
      return;
    }
    this.taking.set(null);
    this.capturing.set(control);
    const key = await this.keyboard.capture();
    if (this.capturing() === control) this.capturing.set(null);
    if (key) this.give(control, [key]);
  }

  /** No longer waits for a key. */
  protected giveUp() {
    if (this.capturing()) this.keyboard.cancelCapture();
  }

  protected remove(control: string) {
    void this.give(control, []);
  }

  protected reset(control: string) {
    void this.give(control, null);
  }

  /** `keys` to `control` (`null`: Prisme's), unless another action has one of them: then the
   * DJ is asked. */
  private async give(control: string, keys: Key[] | null) {
    const taken = await this.setKeys(control, keys, false);
    if (taken?.length) this.taking.set({ control, keys, from: taken.map((s) => this.owner(s)) });
  }

  protected take() {
    const taking = this.taking();
    if (!taking) return;
    this.taking.set(null);
    void this.setKeys(taking.control, taking.keys, true);
  }

  protected resetAll() {
    this.resetAsked.set(false);
    this.taking.set(null);
    void this.engine.call('reset_keys', {});
  }

  /** "Ctrl + K: Command palette (General)" */
  private owner(taken: TakenKey): string {
    const { action } = taken;
    const what = action ? `${labelText(action.label)} (${labelText(action.group)})` : taken.control;
    return t('{key}: {action}', { key: this.keyboard.label(taken), action: what });
  }

  /** The keys other actions have that `keys` would take from them, unless taken. */
  private async setKeys(control: string, keys: Key[] | null, take: boolean) {
    try {
      return await invoke<TakenKey[]>('set_keys', { control, keys, take });
    } catch (e) {
      this.engine.error.set(errorText(e));
      return null;
    }
  }

  ngOnDestroy() {
    this.giveUp();
  }
}
