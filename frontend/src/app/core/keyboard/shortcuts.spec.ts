import { buildControls, ControlActions } from '../control/controls';
import { matches, SHORTCUTS } from './shortcuts';

const key = (code: string, mods: { ctrl?: boolean; shift?: boolean } = {}) => ({
  code,
  ctrlKey: !!mods.ctrl,
  metaKey: false,
  shiftKey: !!mods.shift,
  altKey: false,
});

describe('shortcuts', () => {
  it('never binds the same key twice', () => {
    const combos = SHORTCUTS.map(
      (s) => `${s.ctrl ? 'ctrl+' : ''}${s.shift ? 'shift+' : ''}${s.code}`,
    );
    expect(new Set(combos).size).toBe(combos.length);
  });

  it('only names controls that exist', () => {
    const ids = new Set(
      buildControls(new Proxy({}, { get: () => () => {} }) as ControlActions).map((c) => c.id),
    );
    expect(SHORTCUTS.filter((s) => !ids.has(s.control))).toEqual([]);
  });

  it('gives every deck the same actions', () => {
    const of = (deck: number) =>
      SHORTCUTS.filter((s) => s.control.startsWith(`deck${deck}.`)).map((s) => s.control.slice(6));
    expect(of(1).length).toBe(16);
    for (const deck of [2, 3, 4]) expect(of(deck)).toEqual(of(1));
  });

  it('plays decks C and D with the keys of A and B, Shift held', () => {
    const keyOf = (control: string) => SHORTCUTS.find((s) => s.control === control)!;
    for (const [upper, lower] of [
      [3, 1],
      [4, 2],
    ])
      for (const action of ['play', 'cue', 'hotcue1', 'loop.halve']) {
        const shifted = keyOf(`deck${upper}.${action}`);
        expect(shifted.code).toBe(keyOf(`deck${lower}.${action}`).code);
        expect(matches(shifted, key(shifted.code, { shift: true }))).toBe(true);
        expect(matches(shifted, key(shifted.code))).toBe(false);
      }
  });

  it('requires exactly the modifiers of the shortcut', () => {
    const search = SHORTCUTS.find((s) => s.control === 'library.search')!;
    const loop = SHORTCUTS.find((s) => s.control === 'deck1.loop')!;
    expect(matches(search, key('KeyF', { ctrl: true }))).toBe(true);
    expect(matches(loop, key('KeyF', { ctrl: true }))).toBe(false);
    expect(matches(loop, key('KeyF'))).toBe(true);
  });
});
