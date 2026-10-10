import { Injectable, inject, signal } from '@angular/core';
import { Control } from '../control/controls';
import { ControlService } from '../control/control.service';
import { matches, Shortcut, SHORTCUTS } from './shortcuts';

interface KeyboardLayoutApi {
  keyboard?: { getLayoutMap(): Promise<Map<string, string>> };
}

/** Plays the keyboard shortcuts; held keys are released on key up or on focus loss. */
@Injectable({ providedIn: 'root' })
export class KeyboardService {
  private readonly controls = inject(ControlService);

  readonly shortcuts = SHORTCUTS;
  /** Key code -> what the user's keyboard layout prints on that key. */
  private readonly layout = signal(new Map<string, string>());
  private readonly held = new Map<string, Control>();

  async start(): Promise<void> {
    window.addEventListener('keydown', (e) => this.onKeyDown(e));
    window.addEventListener('keyup', (e) => this.onKeyUp(e));
    window.addEventListener('blur', () => this.releaseAll());
    const api = (navigator as Navigator & KeyboardLayoutApi).keyboard;
    if (api) this.layout.set(await api.getLayoutMap());
  }

  /** "Ctrl + F", "Shift + →", "A" as printed on the user's keyboard. */
  label(shortcut: Shortcut): string {
    const key =
      this.layout().get(shortcut.code)?.toUpperCase() ??
      KEY_NAMES[shortcut.code] ??
      shortcut.code.replace(/^(Key|Digit)/, '');
    return [shortcut.ctrl && 'Ctrl', shortcut.shift && 'Shift', key].filter(Boolean).join(' + ');
  }

  private onKeyDown(event: KeyboardEvent) {
    const shortcut = this.shortcuts.find((s) => matches(s, event));
    const control = shortcut && this.controls.find(shortcut.control);
    if (!control || (isTyping(event) && shortcut.code !== 'F1')) return;
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
  Delete: 'Del',
};

function isTyping(event: KeyboardEvent): boolean {
  const target = event.target as HTMLElement | null;
  return (
    !!target &&
    (['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName) || target.isContentEditable)
  );
}
