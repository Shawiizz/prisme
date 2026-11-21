import { Injectable, computed, inject, signal } from '@angular/core';
import { NavigationService } from '../services/navigation.service';
import { ControlService } from '../control/control.service';
import { PreferencesService } from '../services/preferences.service';
import { CTRL, keyName, keyOf } from './shortcuts';
import { t } from '../i18n/i18n';
import type { Key, Shortcut } from '../models/backend';

interface KeyboardLayoutApi {
  keyboard?: { getLayoutMap(): Promise<Map<string, string>> };
}

/** Plays the keyboard shortcuts as the backend tells them, the DJ's own keys over the
 * defaults; held keys are released on key up or on focus loss. Whether a key is a shortcut is
 * decided as it is pressed (it must not reach the page then): it is looked up in a table by its
 * code and modifiers, made again from the backend's shortcuts as they change. */
@Injectable({ providedIn: 'root' })
export class KeyboardService {
  private readonly controls = inject(ControlService);
  private readonly navigation = inject(NavigationService);
  private readonly preferences = inject(PreferencesService);

  /** The shortcuts in use. */
  readonly shortcuts = computed(() => this.preferences.shortcuts());
  /** The shortcuts of the actions in use, by the key's name (`keyName`). */
  private readonly table = computed(
    () =>
      new Map<string, Shortcut>(
        this.shortcuts()
          .filter((s) => s.inUse)
          .map((s) => [keyName(s), s]),
      ),
  );
  /** Key code -> what the user's keyboard layout prints on that key. */
  private readonly layout = signal(new Map<string, string>());
  /** The action of each key held, by its code. */
  private readonly held = new Map<string, string>();
  /** Given the next key pressed, while one is waited for (`capture`). */
  private captured: ((key: Key | null) => void) | null = null;

  async start(): Promise<void> {
    // first of all: a key waited for goes nowhere else
    window.addEventListener('keydown', (e) => this.onCapture(e), true);
    window.addEventListener('keydown', (e) => this.onKeyDown(e));
    window.addEventListener('keyup', (e) => this.onKeyUp(e));
    window.addEventListener('blur', () => this.releaseAll());
    const api = (navigator as Navigator & KeyboardLayoutApi).keyboard;
    if (api) this.layout.set(await api.getLayoutMap());
  }

  /** The next key pressed, with its modifiers: `null` for Escape, or if given up. */
  capture(): Promise<Key | null> {
    this.cancelCapture();
    return new Promise((resolve) => (this.captured = resolve));
  }

  cancelCapture() {
    this.captured?.(null);
    this.captured = null;
  }

  /** The keys of a control, as printed on the user's keyboard. */
  keysOf(control: string): string[] {
    return this.shortcuts()
      .filter((s) => s.control === control)
      .map((s) => this.label(s));
  }

  /** The first key of a control, as printed on the user's keyboard. */
  keyOf(control: string): string | null {
    return this.keysOf(control)[0] ?? null;
  }

  /** "Ctrl + F", "Shift + →", "A" as printed on the user's keyboard. */
  label(key: Key): string {
    // a layout may print the space bar as a space: named all the same
    const name =
      this.layout().get(key.code)?.trim().toUpperCase() ||
      KEY_NAMES[key.code] ||
      key.code.replace(/^(Key|Digit)/, '');
    return [key.ctrl && CTRL, key.shift && 'Shift', name].filter(Boolean).join(' + ');
  }

  private onCapture(event: KeyboardEvent) {
    const done = this.captured;
    if (!done) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    const plainEscape = event.code === 'Escape' && !event.ctrlKey && !event.metaKey;
    const key = plainEscape ? null : keyOf(event);
    // a modifier pressed first: the key comes next
    if (key === null && !plainEscape) return;
    this.captured = null;
    done(key);
  }

  private onKeyDown(event: KeyboardEvent) {
    const key = keyOf(event);
    const shortcut = key && this.table().get(keyName(key));
    if (!shortcut) return;
    if (!shortcut.everywhere && (isTyping(event) || inDialog(event) || this.navigation.covered()))
      return;
    event.preventDefault();
    if (event.repeat || this.held.has(event.code)) return;
    this.controls.act(shortcut.control, 1);
    this.held.set(event.code, shortcut.control);
  }

  private onKeyUp(event: KeyboardEvent) {
    const control = this.held.get(event.code);
    this.held.delete(event.code);
    if (control) this.controls.act(control, 0);
  }

  private releaseAll() {
    for (const control of this.held.values()) this.controls.act(control, 0);
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
  Escape: 'Esc',
  get Space() {
    return t('Space');
  },
  get Delete() {
    return t('Del');
  },
};

function isTyping(event: KeyboardEvent): boolean {
  const target = event.target as HTMLElement | null;
  return (
    !!target &&
    (['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName) || target.isContentEditable)
  );
}

/** A key pressed in a dialog is the dialog's: Enter on its button answers it, not the page. */
function inDialog(event: KeyboardEvent): boolean {
  return event.target instanceof Element && event.target.closest('[aria-modal="true"]') !== null;
}
