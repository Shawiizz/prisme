import { describe, expect, it } from 'vitest';
import { RecordSource } from './models/engine';
import { preferredSource } from './record-source';

const MASTER: RecordSource = { kind: 'master' };
const pair = (first: number): RecordSource => ({ kind: 'input', first });

describe('preferredSource', () => {
  it('takes the software mixer’s master when nothing was recorded', () => {
    expect(preferredSource([MASTER, pair(0)], null, [0, 1])).toEqual(MASTER);
  });

  it('takes again the inputs recorded last, while no turntable comes in there', () => {
    expect(preferredSource([MASTER, pair(0), pair(4)], pair(4), [4, 5])).toEqual(pair(4));
    expect(preferredSource([MASTER, pair(0)], pair(0), [])).toEqual(MASTER);
  });

  it('records nothing with an external mixer until its inputs are chosen', () => {
    expect(preferredSource([pair(0), pair(2)], null, [])).toBeNull();
    expect(preferredSource([pair(0), pair(2)], MASTER, [])).toBeNull();
    expect(preferredSource([pair(0)], pair(6), [6, 7])).toBeNull();
  });
});
