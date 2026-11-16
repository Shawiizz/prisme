import { afterEach, describe, expect, it } from 'vitest';
import { storeChoice, storedChoice } from './stored';

describe('a choice kept from one start to the next', () => {
  const KEY = 'prisme.test.choice';
  afterEach(() => localStorage.removeItem(KEY));

  it('is read back as it was chosen', () => {
    storeChoice(KEY, false);
    expect(storedChoice(KEY)).toBe(false);
    storeChoice(KEY, true);
    expect(storedChoice(KEY)).toBe(true);
  });

  it('is unknown until chosen, or when the storage holds something else', () => {
    expect(storedChoice(KEY)).toBeNull();
    localStorage.setItem(KEY, 'maybe');
    expect(storedChoice(KEY)).toBeNull();
  });
});
