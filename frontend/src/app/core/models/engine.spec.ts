import { recordLeadIn } from './engine';

describe('recordLeadIn', () => {
  it("is the timecode's own at 33 rpm", () => {
    expect(recordLeadIn(28, '33')).toBe(28);
  });

  it('is shorter at 45 rpm, the grooves going by faster', () => {
    expect(recordLeadIn(28, '45')).toBeCloseTo(20.74, 2);
    expect(recordLeadIn(0, '45')).toBe(0);
  });
});
