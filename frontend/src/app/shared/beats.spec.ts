import { formatBeats, loopBeats } from './beats';

describe('formatBeats', () => {
  it('writes fractions of a beat as fractions', () => {
    expect(formatBeats(0.25)).toBe('1/4');
    expect(formatBeats(1 / 16)).toBe('1/16');
    expect(formatBeats(2)).toBe('2');
    expect(formatBeats(-0.5)).toBe('-1/2');
    expect(formatBeats(-16)).toBe('-16');
  });
});

describe('loopBeats', () => {
  it('recognises loops of a power of two beats', () => {
    expect(loopBeats(2, 0.5)).toBe(4);
    expect(loopBeats(0.125, 0.5)).toBe(0.25);
    expect(loopBeats(1.502, 0.5)).toBeNull();
  });
});
