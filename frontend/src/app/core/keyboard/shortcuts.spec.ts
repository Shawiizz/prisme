import { buildControls, ControlActions } from '../control/controls';
import { matches, SHORTCUTS } from './shortcuts';

const key = (code: string, mods: { ctrl?: boolean; shift?: boolean } = {}) => ({
  code,
  ctrlKey: !!mods.ctrl,
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

  it('gives both decks the same keys', () => {
    const of = (deck: number) =>
      SHORTCUTS.filter((s) => s.control.startsWith(`deck${deck}.`)).map((s) => s.control.slice(6));
    expect(of(1)).toEqual(of(2));
    expect(of(1).length).toBe(16);
  });

  it('requires exactly the modifiers of the shortcut', () => {
    const search = SHORTCUTS.find((s) => s.control === 'library.search')!;
    const loop = SHORTCUTS.find((s) => s.control === 'deck1.loop')!;
    expect(matches(search, key('KeyF', { ctrl: true }))).toBe(true);
    expect(matches(loop, key('KeyF', { ctrl: true }))).toBe(false);
    expect(matches(loop, key('KeyF'))).toBe(true);
  });
});
