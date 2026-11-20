import { formatBeats } from './beats';

describe('formatBeats', () => {
  it('writes fractions of a beat as fractions', () => {
    expect(formatBeats(0.25)).toBe('1/4');
    expect(formatBeats(1 / 16)).toBe('1/16');
    expect(formatBeats(2)).toBe('2');
    expect(formatBeats(-0.5)).toBe('-1/2');
    expect(formatBeats(-16)).toBe('-16');
  });
});
