import { signal } from '@angular/core';
import { levelMeter, levelSegments } from './level';

describe('levelSegments', () => {
  it('lights nothing in silence', () => {
    expect(levelSegments(0, 8).every((s) => s === '')).toBe(true);
  });

  it('turns amber from -6 dB and red at full scale', () => {
    const loud = levelSegments(0.6, 25); // -4.4 dB: segments every 2 dB from -48
    expect(loud.filter((s) => s === 'is-hot').length).toBe(1);
    expect(loud.at(-1)).toBe('');
    expect(levelSegments(1, 25).at(-1)).toBe('is-clip');
  });
});

describe('levelMeter', () => {
  it('rises at once, then falls back slowly under its held peak', () => {
    let now = 0;
    vi.spyOn(performance, 'now').mockImplementation(() => now);
    const peak = signal(1);
    const meter = levelMeter(() => peak(), 25);
    expect(meter().at(-1)).toBe('is-clip');

    now = 50;
    peak.set(0);
    // 1 dB lower after 50 ms, and the full scale still held
    expect(meter().filter((s) => s !== '').length).toBe(25);

    now = 3000;
    peak.set(0.0001);
    expect(meter().every((s) => s === '')).toBe(true);
  });
});
