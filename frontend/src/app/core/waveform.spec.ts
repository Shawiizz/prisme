import { Waveform, decodeWaveform } from './models/engine';

describe('decodeWaveform', () => {
  const sent = (from: number, ...bands: number[]) =>
    new Uint8Array([...new Uint8Array(new Uint32Array([10, 150, from]).buffer), ...bands]).buffer;
  const all = (w: Waveform) => [...w.height, ...w.red, ...w.green, ...w.blue];

  it('reads the track length, the rate, then the height and colour of each slice known', () => {
    const w = decodeWaveform(sent(0, 1, 2, 3, 4, 5, 6, 7, 8))!;
    expect([w.slices, w.slicesPerSecond, w.changedFrom]).toEqual([10, 150, 0]);
    expect(all(w)).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
  });

  it('grows a waveform from the slice sent, in the buffers it shares', () => {
    const first = decodeWaveform(sent(0, 1, 2, 3, 4, 5, 6, 7, 8))!;
    const grown = decodeWaveform(sent(1, 9, 9, 7, 7, 6, 6, 5, 5), first)!;
    expect(all(grown)).toEqual([1, 9, 9, 3, 7, 7, 5, 6, 6, 7, 5, 5]);
    expect(grown.changedFrom).toBe(1);
    expect(grown.height.buffer).toBe(first.height.buffer);
  });

  it('gives nothing for an empty answer, nor for a piece of a waveform it does not have', () => {
    expect(decodeWaveform(new ArrayBuffer(0))).toBeNull();
    expect(decodeWaveform(sent(4, 1, 1, 1, 1))).toBeNull();
  });
});
