import { octaveRatio } from './sync';

describe('octaveRatio', () => {
  it('relates tempos an octave apart', () => {
    expect(octaveRatio(70, 140)).toBe(0.5);
    expect(octaveRatio(174, 87)).toBe(2);
    expect(octaveRatio(125, 128)).toBe(1);
  });
});
