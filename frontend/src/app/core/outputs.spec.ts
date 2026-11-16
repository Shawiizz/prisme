import { describe, expect, it } from 'vitest';
import { boothPairs } from './outputs';

describe('boothPairs', () => {
  it('leaves out the master and the headphones', () => {
    expect(boothPairs(8, 2, null)).toEqual([
      { first: 4, taken: false },
      { first: 6, taken: false },
    ]);
    expect(boothPairs(4, 2, null)).toEqual([]);
    expect(boothPairs(4, null, null)).toEqual([{ first: 2, taken: false }]);
  });

  it('shows the pair chosen, taken when the headphones have it', () => {
    expect(boothPairs(4, 2, 2)).toEqual([{ first: 2, taken: true }]);
    expect(boothPairs(2, null, 4)).toEqual([{ first: 4, taken: false }]);
  });
});
