import { buildControls } from '../control/controls';
import {
  assign,
  DEFAULT_SHORTCUTS as SHORTCUTS,
  defaultKeys,
  everywhere,
  keyOf,
  keysOf,
  matches,
  shortcutsWith,
} from './shortcuts';

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
    const ids = new Set(buildControls().map((c) => c.id));
    expect(SHORTCUTS.filter((s) => !ids.has(s.control))).toEqual([]);
  });

  it('gives every deck the same actions', () => {
    const of = (deck: number) =>
      SHORTCUTS.filter((s) => s.control.startsWith(`deck${deck}.`)).map((s) => s.control.slice(6));
    expect(of(1).length).toBe(16);
    for (const deck of [2, 3, 4]) expect(of(deck)).toEqual(of(1));
  });

  it('plays decks C and D with the keys of A and B, Shift held', () => {
    const first = (control: string) => SHORTCUTS.find((s) => s.control === control)!;
    for (const [upper, lower] of [
      [3, 1],
      [4, 2],
    ])
      for (const action of ['play', 'cue', 'hotcue1', 'loop.halve']) {
        const shifted = first(`deck${upper}.${action}`);
        expect(shifted.code).toBe(first(`deck${lower}.${action}`).code);
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

  it('makes a key of what is pressed, not of a modifier alone nor of Alt', () => {
    expect(keyOf(key('KeyG', { ctrl: true }))).toEqual({ code: 'KeyG', ctrl: true });
    expect(keyOf(key('KeyG', { shift: true }))).toEqual({ code: 'KeyG', shift: true });
    expect(keyOf(key('ShiftLeft', { shift: true }))).toBeNull();
    expect(keyOf(key('ControlRight', { ctrl: true }))).toBeNull();
    expect(keyOf({ ...key('KeyA'), altKey: true })).toBeNull();
  });

  it('works while typing only with Control or a function key', () => {
    const shortcuts = shortcutsWith({ 'general.palette': [{ code: 'KeyG' }] });
    const palette = shortcuts.find((s) => s.control === 'general.palette')!;
    expect(everywhere(palette)).toBe(false);
    expect(everywhere(SHORTCUTS.find((s) => s.control === 'general.help')!)).toBe(true);
    expect(everywhere(SHORTCUTS.find((s) => s.control === 'library.search')!)).toBe(false);
  });

  it('keeps the keys given, and the defaults of the other actions', () => {
    const changes = assign({}, 'deck1.play', [{ code: 'KeyG' }]);
    expect(changes).toEqual({ 'deck1.play': [{ code: 'KeyG' }] });
    const shortcuts = shortcutsWith(changes);
    expect(keysOf(shortcuts, 'deck1.play')).toEqual([{ code: 'KeyG' }]);
    expect(keysOf(shortcuts, 'deck1.cue')).toEqual(defaultKeys('deck1.cue'));
    // S is free now
    expect(shortcuts.some((s) => s.code === 'KeyS' && !s.shift && !s.ctrl)).toBe(false);
  });

  it('takes a key from the action that had it', () => {
    const changes = assign({}, 'deck1.play', [{ code: 'KeyA' }]);
    expect(changes['deck1.cue']).toEqual([]);
    const shortcuts = shortcutsWith(changes);
    expect(shortcuts.filter((s) => s.code === 'KeyA' && !s.shift && !s.ctrl)).toEqual([
      { code: 'KeyA', control: 'deck1.play' },
    ]);
    // one of two keys taken, the other stays
    const palette = shortcutsWith(assign({}, 'deck1.play', [{ code: 'KeyK', ctrl: true }]));
    expect(keysOf(palette, 'general.palette')).toEqual([{ code: 'KeyP', ctrl: true, shift: true }]);
  });

  it('forgets a change once the action has its default keys again', () => {
    let changes = assign({}, 'deck1.play', [{ code: 'KeyG' }]);
    changes = assign(changes, 'deck1.play', defaultKeys('deck1.play'));
    expect(changes).toEqual({});
    changes = assign({}, 'library.search', []);
    expect(changes).toEqual({ 'library.search': [] });
    expect(keysOf(shortcutsWith(changes), 'library.search')).toEqual([]);
  });

  it('leaves a key of the DJ to their action alone, though a default has it too', () => {
    // as a default added in a later version would
    const shortcuts = shortcutsWith({ 'deck1.cue': [{ code: 'KeyS' }] });
    expect(shortcuts.filter((s) => s.code === 'KeyS' && !s.shift && !s.ctrl)).toEqual([
      { code: 'KeyS', control: 'deck1.cue' },
    ]);
  });
});
