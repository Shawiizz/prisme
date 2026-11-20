import { StemMix } from './models/stems';
import { StemPart, partId, partIn, partLevel, partPlaying } from './stem-parts';

const ALL: StemMix = {
  vocals: { level: 1, on: true },
  drums: { level: 1, on: true },
  bass: { level: 1, on: true },
  other: { level: 1, on: true },
};
const off = { level: 1, on: false };

describe('stem parts', () => {
  it('counts the instrumental as playing while any instrument does', () => {
    const drumsOnly: StemMix = { vocals: off, drums: ALL.drums, bass: off, other: off };
    expect(partPlaying(drumsOnly, 'instrumental')).toBe(true);
    expect(partPlaying({ ...drumsOnly, drums: off }, 'instrumental')).toBe(false);
  });

  it('reads the instrumental level as its loudest instrument', () => {
    const mix = { ...ALL, drums: { level: 0.25, on: true }, bass: { level: 0.5, on: true } };
    expect(partLevel(mix, 'instrumental')).toBe(1);
    expect(partLevel({ ...mix, other: { level: 0.1, on: true } }, 'instrumental')).toBe(0.5);
  });

  it('names the other instruments the melody in the actions', () => {
    expect(['vocals', 'instrumental', 'other'].map((p) => partId(p as StemPart))).toEqual([
      'vocals',
      'instrumental',
      'melody',
    ]);
  });
});

describe('partIn', () => {
  it('reads the engine bits, the instrumental by any of its instruments', () => {
    expect(partIn(0b0001, 'vocals')).toBe(true);
    expect(partIn(0b0001, 'drums')).toBe(false);
    expect(partIn(0b0100, 'bass')).toBe(true);
    expect(partIn(0b1000, 'instrumental')).toBe(true);
    expect(partIn(0b0001, 'instrumental')).toBe(false);
  });
});
