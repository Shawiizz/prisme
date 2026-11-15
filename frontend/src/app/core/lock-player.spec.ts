import { crossfaderGain, heardDeck } from './lock-player';

describe('crossfaderGain', () => {
  it('lets a deck through on its side and cuts it on the other', () => {
    expect(crossfaderGain(0, -1, 'smooth')).toBeCloseTo(1);
    expect(crossfaderGain(0, 1, 'smooth')).toBeCloseTo(0);
    expect(crossfaderGain(1, -1, 'sharp')).toBeCloseTo(0);
    expect(crossfaderGain(3, 1, 'sharp')).toBeCloseTo(1);
  });

  it('keeps both sides at the middle', () => {
    expect(crossfaderGain(0, 0, 'smooth')).toBeCloseTo(Math.SQRT1_2);
    expect(crossfaderGain(1, 0, 'sharp')).toBe(1);
  });
});

describe('heardDeck', () => {
  const started = [10, 20, 0, 0];

  it('is none while no deck moves', () => {
    expect(heardDeck([], started, null)).toBeNull();
  });

  it('is the last started when a mixer of its own mixes them', () => {
    expect(heardDeck([0, 1], started, null)).toBe(1);
    expect(heardDeck([0, 1], [30, 20], null)).toBe(0);
  });

  it('is the loudest through the software mixer', () => {
    const levels = [0.9, 0.2];
    expect(heardDeck([0, 1], started, (d) => levels[d])).toBe(0);
  });

  it('is the last started of two as loud', () => {
    expect(heardDeck([0, 1], started, () => 0.7)).toBe(1);
  });
});
