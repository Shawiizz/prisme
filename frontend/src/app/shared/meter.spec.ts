import { sameSegments, segments } from './meter';

describe('sameSegments', () => {
  it('holds a meter whose level moved within a segment', () => {
    const level = (level: number) => segments({ level, held: 0, clip: false }, 8);
    expect(sameSegments(level(0.5), level(0.51))).toBe(true);
    expect(sameSegments(level(0.5), level(0.9))).toBe(false);
    expect(sameSegments(null, null)).toBe(true);
    expect(sameSegments(level(0.5), null)).toBe(false);
  });
});

describe('segments', () => {
  const lit = (meter: Parameters<typeof segments>[0]) => segments(meter, 9);

  it('lights up to the level, the top eighth hot, and nothing for no meter', () => {
    expect(lit({ level: 0.5, held: 0, clip: false })).toEqual([
      'is-lit',
      'is-lit',
      'is-lit',
      'is-lit',
      'is-lit',
      '',
      '',
      '',
      '',
    ]);
    expect(lit({ level: 1, held: 0, clip: false }).slice(6)).toEqual(['is-lit', 'is-hot', '']);
    expect(lit(undefined)).toEqual(Array(9).fill(''));
  });

  it('keeps the highest level of late lit, and the last segment for a clip', () => {
    const held = lit({ level: 0, held: 0.5, clip: false });
    expect(held.map((s, i) => (s ? i : -1)).filter((i) => i >= 0)).toEqual([4]);
    expect(lit({ level: 0, held: 0, clip: true })[8]).toBe('is-clip');
  });
});
