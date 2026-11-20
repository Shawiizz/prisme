import { StemMix } from './models/stems';
import {
  StemPart,
  hasPart,
  partId,
  partIn,
  partLevel,
  partPlaying,
  togglePart,
} from './stem-parts';

const ALL: StemMix = {
  vocals: { level: 1, on: true },
  drums: { level: 1, on: true },
  bass: { level: 1, on: true },
  other: { level: 1, on: true },
};
const off = { level: 1, on: false };

describe('stem parts', () => {
  it('offers the parts a separation has', () => {
    expect(hasPart('none', 'vocals')).toBe(false);
    expect(hasPart('vocals', 'vocals')).toBe(true);
    expect(hasPart('vocals', 'instrumental')).toBe(true);
    expect(hasPart('vocals', 'drums')).toBe(false);
    expect(hasPart('full', 'bass')).toBe(true);
    expect(hasPart('full', 'instrumental')).toBe(true);
  });

  it('toggles one stem alone', () => {
    expect(togglePart(ALL, 'drums')).toEqual({ ...ALL, drums: off });
    expect(togglePart({ ...ALL, other: off }, 'other')).toEqual(ALL);
  });

  it('mutes the whole instrumental, then plays it all again', () => {
    const muted = togglePart(ALL, 'instrumental');
    expect(muted).toEqual({ vocals: ALL.vocals, drums: off, bass: off, other: off });
    expect(partPlaying(muted, 'instrumental')).toBe(false);
    expect(togglePart(muted, 'instrumental')).toEqual(ALL);
  });

  it('counts the instrumental as playing while any instrument does', () => {
    const drumsOnly: StemMix = { vocals: off, drums: ALL.drums, bass: off, other: off };
    expect(partPlaying(drumsOnly, 'instrumental')).toBe(true);
    expect(togglePart(drumsOnly, 'instrumental')).toEqual({ ...drumsOnly, drums: off });
  });

  it('keeps a level through a mute', () => {
    const low = { ...ALL, bass: { level: 0.4, on: true } };
    const muted = togglePart(low, 'bass');
    expect(muted.bass).toEqual({ level: 0.4, on: false });
    expect(togglePart(muted, 'bass').bass).toEqual({ level: 0.4, on: true });
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
