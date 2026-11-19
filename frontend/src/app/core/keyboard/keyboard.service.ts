import { Injectable, computed, inject, signal } from '@angular/core';
import { Control } from '../control/controls';
import { NavigationService } from '../services/navigation.service';
import { ControlService } from '../control/control.service';
import { PreferencesService } from '../services/preferences.service';
import { CTRL, everywhere, Key, keyOf, matches, shortcutsWith } from './shortcuts';
import { t } from '../i18n/i18n';

interface KeyboardLayoutApi {
  keyboard?: { getLayoutMap(): Promise<Map<string, string>> };
}

/** Plays the keyboard shortcuts, the DJ's own keys over the defaults; held keys are released
 * on key up or on focus loss. */
@Injectable({ providedIn: 'root' })
export class KeyboardService {
  private readonly controls = inject(ControlService);
  private readonly navigation = inject(NavigationService);
  private readonly preferences = inject(PreferencesService);

  /** The shortcuts in use. */
  readonly shortcuts = computed(() => shortcutsWith(this.preferences.keys()));
  /** Key code -> what the user's keyboard layout prints on that key. */
  private readonly layout = signal(new Map<string, string>());
  private readonly held = new Map<string, Control>();
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
    const shortcut = this.shortcuts().find((s) => matches(s, event));
    const control = shortcut && this.controls.findInUse(shortcut.control);
    if (!control) return;
    if (!everywhere(shortcut) && (isTyping(event) || inDialog(event) || this.navigation.covered()))
      return;
    event.preventDefault();
    if (event.repeat || this.held.has(event.code)) return;
    control.apply(1);
    this.held.set(event.code, control);
  }

  private onKeyUp(event: KeyboardEvent) {
    this.held.get(event.code)?.apply(0);
    this.held.delete(event.code);
  }

  private releaseAll() {
    for (const control of this.held.values()) control.apply(0);
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
