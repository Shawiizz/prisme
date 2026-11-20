import { withChanges } from './told';

interface Told {
  decks: { at: number }[];
  level: number;
  name: string;
}

describe('withChanges', () => {
  const before: Told = { decks: [{ at: 1 }, { at: 2 }], level: 0.5, name: 'A' };

  it('takes the fields sent and the decks that changed, the others kept as they were', () => {
    const next = withChanges(before, { decks: [null as never, { at: 3 }], level: 0.75 });
    expect(next).toEqual({ decks: [{ at: 1 }, { at: 3 }], level: 0.75, name: 'A' });
    expect(next.decks[0]).toBe(before.decks[0]);
  });

  it('takes another number of decks whole, and everything the first time', () => {
    expect(withChanges(before, { decks: [{ at: 9 }] }).decks).toEqual([{ at: 9 }]);
    expect(withChanges(null, before)).toBe(before);
  });
});
