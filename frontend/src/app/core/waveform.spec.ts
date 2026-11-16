import { addWaveformPiece, decodeWaveform } from './models/engine';

/** A piece as the backend sends it, `values` in each band: from, slices, rate, then the bands. */
function piece(from: number, slices: number, values: number[]): ArrayBuffer {
  const header = new Uint8Array(new Uint32Array([from, slices, 150]).buffer);
  return new Uint8Array([...header, ...values, ...values, ...values, ...values]).buffer;
}

describe('decodeWaveform', () => {
  it('reads where the piece starts, the track length, the rate, then the four bands', () => {
    const header = new Uint8Array(new Uint32Array([2, 10, 150]).buffer);
    const w = decodeWaveform(new Uint8Array([...header, 1, 2, 3, 4, 5, 6, 7, 8]).buffer)!;
    expect([w.from, w.slices, w.slicesPerSecond]).toEqual([2, 10, 150]);
    expect([...w.peak, ...w.low, ...w.mid, ...w.high]).toEqual([1, 2, 3, 4, 5, 6, 7, 8]);
  });

  it('gives nothing for an empty answer', () => {
    expect(decodeWaveform(new ArrayBuffer(0))).toBeNull();
  });
});

describe('addWaveformPiece', () => {
  const add = (
    w: ReturnType<typeof addWaveformPiece> | null,
    from: number,
    slices: number,
    values: number[],
  ) => addWaveformPiece(w, decodeWaveform(piece(from, slices, values))!);

  it('fills the track from its start as the pieces come', () => {
    const first = add(null, 0, 6, [9, 8]);
    expect(first.known).toBe(2);
    expect([...first.peak]).toEqual([9, 8, 0, 0, 0, 0]);
    const second = add(first, 2, 6, [7, 6, 5]);
    expect(second).not.toBe(first);
    expect(second.peak).toBe(first.peak);
    expect(second.known).toBe(5);
    expect([...second.mid]).toEqual([9, 8, 7, 6, 5, 0]);
  });

  it('keeps what is known when the track turns out shorter or longer', () => {
    const w = add(add(null, 0, 6, [9, 8, 7]), 3, 5, [6, 5]);
    expect(w.peak.length).toBe(5);
    expect(w.known).toBe(5);
    expect([...w.high]).toEqual([9, 8, 7, 6, 5]);
  });

  it('leaves a piece past a gap out', () => {
    const w = add(add(null, 0, 6, [9]), 3, 6, [6]);
    expect(w.known).toBe(1);
  });
});
