import { StemMix } from './models/stems';
import { hasPart, partPlaying, togglePart } from './stem-parts';

const ALL: StemMix = { vocals: true, drums: true, bass: true, other: true };

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
    expect(togglePart(ALL, 'drums')).toEqual({ ...ALL, drums: false });
    expect(togglePart({ ...ALL, other: false }, 'other')).toEqual(ALL);
  });

  it('mutes the whole instrumental, then plays it all again', () => {
    const muted = togglePart(ALL, 'instrumental');
    expect(muted).toEqual({ vocals: true, drums: false, bass: false, other: false });
    expect(partPlaying(muted, 'instrumental')).toBe(false);
    expect(togglePart(muted, 'instrumental')).toEqual(ALL);
  });

  it('counts the instrumental as playing while any instrument does', () => {
    const drumsOnly = { vocals: false, drums: true, bass: false, other: false };
    expect(partPlaying(drumsOnly, 'instrumental')).toBe(true);
    expect(togglePart(drumsOnly, 'instrumental')).toEqual({ ...drumsOnly, drums: false });
  });
});
