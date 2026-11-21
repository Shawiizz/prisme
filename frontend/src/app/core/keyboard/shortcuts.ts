import type { Key } from '../models/backend';

/** On a Mac the shortcuts take Command where the others take Control. */
export const MAC = /Mac/.test(navigator.platform);
/** The name of that key. */
export const CTRL = MAC ? '⌘' : 'Ctrl';

type KeyEvent = Pick<KeyboardEvent, 'code' | 'ctrlKey' | 'metaKey' | 'shiftKey' | 'altKey'>;

/** The key an event presses, if a shortcut can be made of it: not a modifier alone, not with
 * Alt (the palette's). */
export function keyOf(event: KeyEvent): Key | null {
  if (event.altKey || /^(Shift|Control|Alt|Meta|OS)(Left|Right)?$/.test(event.code)) return null;
  const key: Key = { code: event.code };
  if (MAC ? event.metaKey : event.ctrlKey) key.ctrl = true;
  if (event.shiftKey) key.shift = true;
  return key;
}

/** What names `key` in the keyboard's table: its code and its modifiers. */
export function keyName(key: Key): string {
  return `${key.ctrl ? 'Ctrl+' : ''}${key.shift ? 'Shift+' : ''}${key.code}`;
}
