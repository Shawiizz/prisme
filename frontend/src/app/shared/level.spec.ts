import { levelSegments } from './level';

describe('levelSegments', () => {
  it('lights nothing in silence', () => {
    expect(levelSegments(0, 8).every((s) => s === '')).toBe(true);
  });

  it('turns amber from -6 dB and red at full scale', () => {
    const loud = levelSegments(0.6, 25); // -4.4 dB: segments every 2 dB from -48
    expect(loud.filter((s) => s === 'is-hot').length).toBe(1);
    expect(loud.at(-1)).toBe('');
    expect(levelSegments(1, 25).at(-1)).toBe('is-clip');
  });
});
