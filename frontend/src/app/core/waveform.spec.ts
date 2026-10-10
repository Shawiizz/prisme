import { decodeWaveform } from './models/engine';

describe('decodeWaveform', () => {
  it('reads the rate then the four bands', () => {
    const bytes = new Uint8Array([150, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8]);
    const w = decodeWaveform(bytes.buffer)!;
    expect(w.slicesPerSecond).toBe(150);
    expect([...w.peak, ...w.low, ...w.mid, ...w.high]).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
  });

  it('gives nothing for an empty answer', () => {
    expect(decodeWaveform(new ArrayBuffer(0))).toBeNull();
  });
});
