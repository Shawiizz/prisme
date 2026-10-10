import { TapTempo } from './tap-tempo';

describe('TapTempo', () => {
  it('gives the tempo after four taps', () => {
    const t = new TapTempo();
    expect([0, 500, 1000].map((ms) => t.tap(ms))).toEqual([null, null, null]);
    expect(t.tap(1500)).toBeCloseTo(120);
  });

  it('starts again after a pause', () => {
    const t = new TapTempo();
    [0, 500, 1000, 1500].forEach((ms) => t.tap(ms));
    expect(t.tap(10_000)).toBeNull();
  });

  it('follows the latest taps', () => {
    const t = new TapTempo();
    for (let i = 0; i < 16; i++) t.tap(i * 500);
    let bpm = 0;
    for (let i = 0; i < 16; i++) bpm = t.tap(7500 + (i + 1) * 470) ?? 0;
    expect(bpm).toBeCloseTo(60_000 / 470, 1);
  });
});
