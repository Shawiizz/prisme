/** What the keyboard can do, independent of how it is done. */
export interface ShortcutActions {
  hotCue(deck: number, slot: number): void;
  cue(deck: number, pressed: boolean): void;
  togglePlay(deck: number): void;
  sync(deck: number): void;
  bend(deck: number, direction: -1 | 0 | 1): void;
  toggleLoop(deck: number): void;
  scaleLoop(deck: number, factor: number): void;
  moveSelection(delta: number): void;
  loadSelected(deck: number | null): void;
  focusSearch(): void;
  toggleHelp(): void;
}

export interface Shortcut {
  /** Physical key (`KeyboardEvent.code`), so the layout does not matter. */
  code: string;
  ctrl?: boolean;
  shift?: boolean;
  group: string;
  description: string;
  press(): void;
  /** For keys that act while held. */
  release?(): void;
}

/** Physical keys of each deck: the left hand plays deck 1, the right hand deck 2. */
const DECK_KEYS = [
  {
    hotCues: ['Digit1', 'Digit2', 'Digit3', 'Digit4', 'KeyQ', 'KeyW', 'KeyE', 'KeyR'],
    cue: 'KeyA',
    play: 'KeyS',
    sync: 'KeyD',
    loop: 'KeyF',
    halve: 'KeyC',
    double: 'KeyV',
    slower: 'KeyZ',
    faster: 'KeyX',
  },
  {
    hotCues: ['Digit7', 'Digit8', 'Digit9', 'Digit0', 'KeyU', 'KeyI', 'KeyO', 'KeyP'],
    cue: 'KeyJ',
    play: 'KeyK',
    sync: 'KeyL',
    loop: 'KeyH',
    halve: 'Comma',
    double: 'Period',
    slower: 'KeyN',
    faster: 'KeyM',
  },
];

export function buildShortcuts(a: ShortcutActions): Shortcut[] {
  const deckShortcuts = DECK_KEYS.flatMap((keys, deck) => {
    const group = `Deck ${deck + 1}`;
    return [
      ...keys.hotCues.map((code, slot) => ({
        code,
        group,
        description: `Hot cue ${slot + 1}`,
        press: () => a.hotCue(deck, slot),
      })),
      {
        code: keys.cue,
        group,
        description: 'Cue (hold to preview)',
        press: () => a.cue(deck, true),
        release: () => a.cue(deck, false),
      },
      { code: keys.play, group, description: 'Play / pause', press: () => a.togglePlay(deck) },
      { code: keys.sync, group, description: 'Sync', press: () => a.sync(deck) },
      {
        code: keys.loop,
        group,
        description: 'Loop 4 beats / exit',
        press: () => a.toggleLoop(deck),
      },
      {
        code: keys.halve,
        group,
        description: 'Halve the loop',
        press: () => a.scaleLoop(deck, 0.5),
      },
      {
        code: keys.double,
        group,
        description: 'Double the loop',
        press: () => a.scaleLoop(deck, 2),
      },
      {
        code: keys.slower,
        group,
        description: 'Slow down while held',
        press: () => a.bend(deck, -1),
        release: () => a.bend(deck, 0),
      },
      {
        code: keys.faster,
        group,
        description: 'Speed up while held',
        press: () => a.bend(deck, 1),
        release: () => a.bend(deck, 0),
      },
    ] satisfies Shortcut[];
  });
  const library = 'Library';
  return [
    ...deckShortcuts,
    {
      code: 'ArrowUp',
      group: library,
      description: 'Previous track',
      press: () => a.moveSelection(-1),
    },
    {
      code: 'ArrowDown',
      group: library,
      description: 'Next track',
      press: () => a.moveSelection(1),
    },
    {
      code: 'ArrowLeft',
      shift: true,
      group: library,
      description: 'Load on deck 1',
      press: () => a.loadSelected(0),
    },
    {
      code: 'ArrowRight',
      shift: true,
      group: library,
      description: 'Load on deck 2',
      press: () => a.loadSelected(1),
    },
    {
      code: 'Enter',
      group: library,
      description: 'Load on a free deck',
      press: () => a.loadSelected(null),
    },
    {
      code: 'KeyF',
      ctrl: true,
      group: library,
      description: 'Search',
      press: () => a.focusSearch(),
    },
    {
      code: 'F1',
      group: 'General',
      description: 'Keyboard shortcuts',
      press: () => a.toggleHelp(),
    },
  ];
}

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
