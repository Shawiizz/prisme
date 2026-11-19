import { formatRemaining, formatTime } from './time';

describe('formatTime', () => {
  it('reads minutes, seconds and tenths', () => {
    expect(formatTime(83.4)).toBe('1:23.4');
    expect(formatTime(5)).toBe('0:05.0');
    expect(formatTime(-1)).toBe('0:00.0');
  });

  it('rounds up to the next minute rather than to 60 seconds', () => {
    expect(formatTime(59.97)).toBe('1:00.0');
    expect(formatTime(119.96)).toBe('2:00.0');
    expect(formatRemaining(179.97)).toBe('−3:00.0');
  });
});
