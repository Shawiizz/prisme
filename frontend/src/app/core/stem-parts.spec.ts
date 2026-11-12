import { FULL_MIX, StemMix } from './models/stems';
import { hasPart, partIn, partLevel, partPlaying, setPartLevel, togglePart } from './stem-parts';

const ALL = FULL_MIX;
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
    const low = setPartLevel(ALL, 'bass', 0.4);
    const muted = togglePart(low, 'bass');
    expect(muted.bass).toEqual({ level: 0.4, on: false });
    expect(togglePart(muted, 'bass').bass).toEqual({ level: 0.4, on: true });
  });

  it('sets the instrumental level on all its instruments, within 0 and 1', () => {
    const mix = setPartLevel(ALL, 'instrumental', 1.5);
    expect(partLevel(mix, 'instrumental')).toBe(1);
    const low = setPartLevel(ALL, 'instrumental', 0.25);
    expect([low.drums.level, low.bass.level, low.other.level, low.vocals.level]).toEqual([
      0.25, 0.25, 0.25, 1,
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
