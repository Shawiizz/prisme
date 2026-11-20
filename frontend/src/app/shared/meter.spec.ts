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
