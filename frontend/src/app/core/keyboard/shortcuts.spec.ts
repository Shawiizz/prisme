import { buildShortcuts, matches, ShortcutActions } from './shortcuts';

function recorder() {
  const calls: string[] = [];
  const record =
    (name: string) =>
    (...args: unknown[]) =>
      calls.push(`${name}(${args.join(',')})`);
  const actions: ShortcutActions = {
    hotCue: record('hotCue'),
    cue: record('cue'),
    togglePlay: record('togglePlay'),
    sync: record('sync'),
    bend: record('bend'),
    toggleLoop: record('toggleLoop'),
    scaleLoop: record('scaleLoop'),
    moveSelection: record('moveSelection'),
    loadSelected: record('loadSelected'),
    removeSelected: record('removeSelected'),
    focusSearch: record('focusSearch'),
    toggleHelp: record('toggleHelp'),
  };
  return { calls, shortcuts: buildShortcuts(actions) };
}

const key = (code: string, mods: { ctrl?: boolean; shift?: boolean } = {}) => ({
  code,
  ctrlKey: !!mods.ctrl,
  shiftKey: !!mods.shift,
  altKey: false,
});

describe('shortcuts', () => {
  it('never binds the same key twice', () => {
    const { shortcuts } = recorder();
    const combos = shortcuts.map(
      (s) => `${s.ctrl ? 'ctrl+' : ''}${s.shift ? 'shift+' : ''}${s.code}`,
    );
    expect(new Set(combos).size).toBe(combos.length);
  });

  it('gives both decks the same set of actions', () => {
    const { shortcuts } = recorder();
    const of = (group: string) =>
      shortcuts.filter((s) => s.group === group).map((s) => s.description);
    expect(of('Deck 1')).toEqual(of('Deck 2'));
    expect(of('Deck 1').length).toBe(16);
  });

  it('runs the action of the key pressed, and its release', () => {
    const { calls, shortcuts } = recorder();
    const find = (code: string, mods = {}) => shortcuts.find((s) => matches(s, key(code, mods)))!;
    find('KeyA').press();
    find('KeyA').release!();
    find('Digit9').press();
    find('ArrowRight', { shift: true }).press();
    expect(calls).toEqual(['cue(0,true)', 'cue(0,false)', 'hotCue(1,2)', 'loadSelected(1)']);
  });

  it('requires exactly the modifiers of the shortcut', () => {
    const { shortcuts } = recorder();
    const search = shortcuts.find((s) => s.description === 'Search')!;
    const loop = shortcuts.find((s) => s.description === 'Loop 4 beats / exit')!;
    expect(matches(search, key('KeyF', { ctrl: true }))).toBe(true);
    expect(matches(loop, key('KeyF', { ctrl: true }))).toBe(false);
    expect(matches(loop, key('KeyF'))).toBe(true);
  });
});
