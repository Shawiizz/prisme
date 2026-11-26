import { keyName, keyOf } from './shortcuts';

const key = (code: string, mods: { ctrl?: boolean; shift?: boolean; key?: string } = {}) => ({
  code,
  key: mods.key ?? code.replace(/^Key/, '').toLowerCase(),
  ctrlKey: !!mods.ctrl,
  metaKey: false,
  shiftKey: !!mods.shift,
  altKey: false,
});

describe('shortcuts', () => {
  it('makes a key of what is pressed, not of a modifier alone nor of Alt', () => {
    expect(keyOf(key('KeyG', { ctrl: true }))).toEqual({ code: 'KeyG', ctrl: true });
    expect(keyOf(key('KeyG', { shift: true }))).toEqual({ code: 'KeyG', shift: true });
    expect(keyOf(key('ShiftLeft', { shift: true }))).toBeNull();
    expect(keyOf(key('ControlRight', { ctrl: true }))).toBeNull();
    expect(keyOf({ ...key('KeyA'), altKey: true })).toBeNull();
  });

  it('takes the letter printed with Control, the place of the key without', () => {
    // the A of an AZERTY keyboard is where a QWERTY's Q is
    expect(keyOf(key('KeyQ', { ctrl: true, key: 'a' }))).toEqual({ code: 'KeyA', ctrl: true });
    expect(keyOf(key('KeyQ', { ctrl: true, shift: true, key: 'A' }))?.code).toBe('KeyA');
    expect(keyOf(key('KeyQ', { key: 'a' }))).toEqual({ code: 'KeyQ' });
    expect(keyOf(key('Comma', { ctrl: true, key: ',' }))).toEqual({ code: 'Comma', ctrl: true });
    expect(keyOf(key('KeyA', { ctrl: true, key: 'ф' }))?.code).toBe('KeyA');
  });

  it('names a key pressed as the backend names its shortcut, its modifiers exactly', () => {
    const shortcut = { code: 'KeyF', ctrl: true, control: 'library.search' };
    expect(keyName(keyOf(key('KeyF', { ctrl: true }))!)).toBe(keyName(shortcut));
    expect(keyName(keyOf(key('KeyF'))!)).not.toBe(keyName(shortcut));
    expect(keyName(keyOf(key('KeyF', { ctrl: true, shift: true }))!)).not.toBe(keyName(shortcut));
  });
});
