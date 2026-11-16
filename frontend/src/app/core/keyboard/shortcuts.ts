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

/** A key bound to a control (see `buildControls`). */
export interface Shortcut extends Key {
  control: string;
}

/** The keys the DJ gave the controls whose keys they changed, by control (none: taken off). */
export type KeyChanges = Record<string, Key[]>;

/** Controls whose keys work while typing and over the settings and the palette, if they hold
 * Control or are function keys: a letter alone types. */
const EVERYWHERE = new Set(['general.help', 'general.palette', 'general.settings']);

/** Physical keys of each deck: the left hand plays deck A, the right hand deck B; Shift held,
 * the same keys play decks C and D, as the second layer of a four-deck controller. */
const DECK_KEYS = [
  {
    hotCues: ['Digit1', 'Digit2', 'Digit3', 'Digit4', 'KeyQ', 'KeyW', 'KeyE', 'KeyR'],
    cue: 'KeyA',
    play: 'KeyS',
    sync: 'KeyD',
    loop: 'KeyF',
    'loop.halve': 'KeyC',
    'loop.double': 'KeyV',
    'bend.down': 'KeyZ',
    'bend.up': 'KeyX',
  },
  {
    hotCues: ['Digit7', 'Digit8', 'Digit9', 'Digit0', 'KeyU', 'KeyI', 'KeyO', 'KeyP'],
    cue: 'KeyJ',
    play: 'KeyK',
    sync: 'KeyL',
    loop: 'KeyH',
    'loop.halve': 'Comma',
    'loop.double': 'Period',
    'bend.down': 'KeyN',
    'bend.up': 'KeyM',
  },
];

/** The keys Prisme gives the controls, until the DJ changes them. */
export const DEFAULT_SHORTCUTS: Shortcut[] = [
  ...[0, 1, 2, 3].flatMap((deck) => {
    const { hotCues, ...keys } = DECK_KEYS[deck % 2];
    const shift = deck >= 2 || undefined;
    return [
      ...hotCues.map((code, slot) => ({
        code,
        shift,
        control: `deck${deck + 1}.hotcue${slot + 1}`,
      })),
      ...Object.entries(keys).map(([name, code]) => ({
        code,
        shift,
        control: `deck${deck + 1}.${name}`,
      })),
    ];
  }),
  { code: 'ArrowUp', control: 'library.previous' },
  { code: 'ArrowDown', control: 'library.next' },
  { code: 'ArrowLeft', shift: true, control: 'library.load1' },
  { code: 'ArrowRight', shift: true, control: 'library.load2' },
  { code: 'ArrowLeft', ctrl: true, shift: true, control: 'library.double1' },
  { code: 'ArrowRight', ctrl: true, shift: true, control: 'library.double2' },
  { code: 'Enter', control: 'library.load' },
  { code: 'Space', control: 'library.preview' },
  { code: 'Delete', control: 'library.remove' },
  { code: 'KeyP', ctrl: true, control: 'library.prepare' },
  { code: 'KeyA', ctrl: true, control: 'library.selectAll' },
  { code: 'Escape', control: 'library.clearSelection' },
  { code: 'KeyF', ctrl: true, control: 'library.search' },
  { code: 'F1', control: 'general.help' },
  { code: 'KeyK', ctrl: true, control: 'general.palette' },
  { code: 'KeyP', ctrl: true, shift: true, control: 'general.palette' },
  { code: 'Comma', ctrl: true, control: 'general.settings' },
];

type KeyEvent = Pick<KeyboardEvent, 'code' | 'ctrlKey' | 'metaKey' | 'shiftKey' | 'altKey'>;

/** `true` if the event is the key with exactly its modifiers. */
export function matches(key: Key, event: KeyEvent): boolean {
  return (
    key.code === event.code &&
    !!key.ctrl === (MAC ? event.metaKey : event.ctrlKey) &&
    !!key.shift === event.shiftKey &&
    !event.altKey
  );
}

/** The key an event presses, if a shortcut can be made of it: not a modifier alone, not with
 * Alt (the palette's). */
export function keyOf(event: KeyEvent): Key | null {
  if (event.altKey || /^(Shift|Control|Alt|Meta|OS)(Left|Right)?$/.test(event.code)) return null;
  const key: Key = { code: event.code };
  if (MAC ? event.metaKey : event.ctrlKey) key.ctrl = true;
  if (event.shiftKey) key.shift = true;
  return key;
}

export function sameKey(a: Key, b: Key): boolean {
  return a.code === b.code && !!a.ctrl === !!b.ctrl && !!a.shift === !!b.shift;
}

/** Whether `shortcut` acts while typing and over the settings and the palette. */
export function everywhere(shortcut: Shortcut): boolean {
  return EVERYWHERE.has(shortcut.control) && (!!shortcut.ctrl || /^F\d+$/.test(shortcut.code));
}

/** The keys of `control` among `shortcuts`. */
export function keysOf(shortcuts: Shortcut[], control: string): Key[] {
  return shortcuts
    .filter((s) => s.control === control)
    .map(({ code, ctrl, shift }) => ({ code, ctrl, shift }));
}

/** The keys `control` has until the DJ changes them. */
export function defaultKeys(control: string): Key[] {
  return keysOf(DEFAULT_SHORTCUTS, control);
}

/** The shortcuts in use: those the DJ gave, and the defaults of the controls they left alone,
 * but for a key they gave another (a default added since). */
export function shortcutsWith(changes: KeyChanges): Shortcut[] {
  const given = Object.entries(changes).flatMap(([control, keys]) =>
    keys.map((key) => ({ ...key, control })),
  );
  const kept = DEFAULT_SHORTCUTS.filter(
    (s) => !(s.control in changes) && !given.some((key) => sameKey(key, s)),
  );
  return [...kept, ...given];
}

/** The shortcuts of other controls than `control` that `keys` would take from them. */
export function conflicts(shortcuts: Shortcut[], control: string, keys: Key[]): Shortcut[] {
  return shortcuts.filter((s) => s.control !== control && keys.some((key) => sameKey(key, s)));
}

/** `changes` once `control` has `keys`, taken from the controls that had them; a control back
 * to its default keys is no longer a change. */
export function assign(changes: KeyChanges, control: string, keys: Key[]): KeyChanges {
  const shortcuts = shortcutsWith(changes);
  const next: KeyChanges = { ...changes };
  const set = (c: string, given: Key[]) => {
    const defaults = defaultKeys(c);
    const same =
      given.length === defaults.length && given.every((k) => defaults.some((d) => sameKey(k, d)));
    if (same) delete next[c];
    else next[c] = given;
  };
  for (const taken of new Set(conflicts(shortcuts, control, keys).map((s) => s.control)))
    set(
      taken,
      keysOf(shortcuts, taken).filter((key) => !keys.some((k) => sameKey(k, key))),
    );
  set(control, keys);
  return next;
}
