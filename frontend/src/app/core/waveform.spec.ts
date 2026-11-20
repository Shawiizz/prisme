import { decodeWaveform } from './models/engine';

describe('decodeWaveform', () => {
  it('reads the track length, the rate, then the height and colour of each slice known', () => {
    const header = new Uint8Array(new Uint32Array([10, 150]).buffer);
    const w = decodeWaveform(new Uint8Array([...header, 1, 2, 3, 4, 5, 6, 7, 8]).buffer)!;
    expect([w.slices, w.slicesPerSecond]).toEqual([10, 150]);
    expect([...w.height, ...w.red, ...w.green, ...w.blue]).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
  });

  it('gives nothing for an empty answer', () => {
    expect(decodeWaveform(new ArrayBuffer(0))).toBeNull();
  });
});
