import { barNumber } from './waveform-overlay';

describe('barNumber', () => {
  it('counts the bars from bar 1 on the grid’s first beat, and down before it', () => {
    expect([0, 3, 4, 7, 8].map((n) => barNumber(n, 4))).toEqual([1, 1, 2, 2, 3]);
    expect([-1, -4, -5, -8].map((n) => barNumber(n, 4))).toEqual([-1, -1, -2, -2]);
  });
});
