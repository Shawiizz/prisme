import { Waveform } from '../../../core/models/engine';
import { LOUDNESS, bandShapes, columnHeights } from './waveform-shapes';

function waveform(low: number[], mid: number[], high: number[], slicesPerSecond = 10): Waveform {
  return {
    slicesPerSecond,
    peak: new Uint8Array(low.length),
    low: Uint8Array.from(low),
    mid: Uint8Array.from(mid),
    high: Uint8Array.from(high),
  };
}

describe('bandShapes', () => {
  it('nests the bands, each taking its share of the height of the whole sound', () => {
    const shapes = bandShapes(waveform([200, 0, 0], [0, 200, 100], [0, 100, 100]));
    expect(shapes.low[0]).toBeGreaterThan(0.9);
    expect(shapes.mid[0]).toBe(0);
    expect(shapes.high[0]).toBe(0);
    expect(shapes.low[1]).toBe(1);
    expect(shapes.mid[1]).toBe(1);
    expect(shapes.high[1]).toBeCloseTo(1 / 3);
    expect(shapes.mid[2]).toBeCloseTo(shapes.low[2]);
    expect(shapes.high[2]).toBeCloseTo(shapes.low[2] / 2);
  });

  it('keeps a quiet passage in sight, below what is loud', () => {
    const shapes = bandShapes(waveform([250, 25, 0], [0, 0, 0], [0, 0, 0]));
    expect(shapes.low[1]).toBeCloseTo(Math.pow(0.1, LOUDNESS));
    expect(shapes.low[1]).toBeGreaterThan(0.15);
    expect(shapes.low[2]).toBe(0);
  });

  it('smooths the low band into one shape', () => {
    const comb = Array.from({ length: 40 }, (_, i) => (i % 2 ? 100 : 200));
    const silent = comb.map(() => 0);
    const shapes = bandShapes(waveform(comb, silent, silent, 150));
    const inside = Array.from(shapes.low.slice(5, 35));
    expect(Math.max(...inside) - Math.min(...inside)).toBeLessThan(0.1);
  });
});

describe('columnHeights', () => {
  const band = Float32Array.from([0, 1, 0.5, 0.25]);

  it('draws the line through the slices when a column is narrower than a slice', () => {
    const into = new Float32Array(4);
    columnHeights(band, 1, 0.5, into);
    expect(Array.from(into)).toEqual([0.75, 0.875, 0.625, 0.4375]);
  });

  it('keeps the highest slice under a wide column', () => {
    const into = new Float32Array(2);
    columnHeights(band, 0, 2, into);
    expect(Array.from(into)).toEqual([1, 0.5]);
  });

  it('draws nothing outside the track', () => {
    const into = new Float32Array(3);
    columnHeights(band, -2, 1, into);
    expect(into[0]).toBe(0);
    columnHeights(band, 4, 0.5, into);
    expect(Array.from(into)).toEqual([0, 0, 0]);
  });
});
