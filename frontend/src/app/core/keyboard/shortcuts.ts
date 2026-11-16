/** On a Mac the shortcuts take Command where the others take Control. */
export const MAC = /Mac/.test(navigator.platform);
/** The name of that key. */
export const CTRL = MAC ? '⌘' : 'Ctrl';

/** A key bound to a control (see `buildControls`). */
export interface Shortcut {
  /** Physical key (`KeyboardEvent.code`), so the layout does not matter. */
  code: string;
  /** With Control (Command on a Mac). */
  ctrl?: boolean;
  shift?: boolean;
  control: string;
  /** Works while typing and over the settings and the palette. */
  global?: boolean;
}

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

export const SHORTCUTS: Shortcut[] = [
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
  { code: 'F1', control: 'general.help', global: true },
  { code: 'KeyK', ctrl: true, control: 'general.palette', global: true },
  { code: 'KeyP', ctrl: true, shift: true, control: 'general.palette', global: true },
  { code: 'Comma', ctrl: true, control: 'general.settings', global: true },
];

/** `true` if the event is the shortcut's key with exactly its modifiers. */
export function matches(
  shortcut: Shortcut,
  event: Pick<KeyboardEvent, 'code' | 'ctrlKey' | 'metaKey' | 'shiftKey' | 'altKey'>,
): boolean {
  return (
    shortcut.code === event.code &&
    !!shortcut.ctrl === (MAC ? event.metaKey : event.ctrlKey) &&
    !!shortcut.shift === event.shiftKey &&
    !event.altKey
  );
}
