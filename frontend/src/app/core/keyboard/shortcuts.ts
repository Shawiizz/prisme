/** On a Mac the shortcuts take Command where the others take Control. */
export const MAC = /Mac/.test(navigator.platform);
/** The name of that key. */
export const CTRL = MAC ? '⌘' : 'Ctrl';

/** A key of the keyboard, with the modifiers held. */
export interface Key {
  /** Physical key (`KeyboardEvent.code`), so the layout does not matter. */
  code: string;
  /** With Control (Command on a Mac). */
  ctrl?: boolean;
  shift?: boolean;
}

/** A key bound to an action, as the backend tells the shortcuts in use: the keys the DJ gave
 * and the defaults of the other actions. */
export interface Shortcut extends Key {
  control: string;
  /** It acts while typing and over the settings and the palette. */
  everywhere: boolean;
  /** Its action is of a deck in use, or of none. */
  inUse: boolean;
}

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
