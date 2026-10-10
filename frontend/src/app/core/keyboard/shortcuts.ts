/** A key bound to a control (see `buildControls`). */
export interface Shortcut {
  /** Physical key (`KeyboardEvent.code`), so the layout does not matter. */
  code: string;
  ctrl?: boolean;
  shift?: boolean;
  control: string;
}

/** Physical keys of each deck: the left hand plays deck 1, the right hand deck 2. */
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
  ...DECK_KEYS.flatMap(({ hotCues, ...keys }, deck) => [
    ...hotCues.map((code, slot) => ({ code, control: `deck${deck + 1}.hotcue${slot + 1}` })),
    ...Object.entries(keys).map(([name, code]) => ({ code, control: `deck${deck + 1}.${name}` })),
  ]),
  { code: 'ArrowUp', control: 'library.previous' },
  { code: 'ArrowDown', control: 'library.next' },
  { code: 'ArrowLeft', shift: true, control: 'library.load1' },
  { code: 'ArrowRight', shift: true, control: 'library.load2' },
  { code: 'Enter', control: 'library.load' },
  { code: 'Delete', control: 'library.remove' },
  { code: 'KeyP', ctrl: true, control: 'library.prepare' },
  { code: 'KeyF', ctrl: true, control: 'library.search' },
  { code: 'F1', control: 'general.help' },
];

/** `true` if the event is the shortcut's key with exactly its modifiers. */
export function matches(
  shortcut: Shortcut,
  event: Pick<KeyboardEvent, 'code' | 'ctrlKey' | 'shiftKey' | 'altKey'>,
): boolean {
  return (
    shortcut.code === event.code &&
    !!shortcut.ctrl === event.ctrlKey &&
    !!shortcut.shift === event.shiftKey &&
    !event.altKey
  );
}
