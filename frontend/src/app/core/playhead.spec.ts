import { DeckStatus } from './models/engine';
import { followPlayhead } from './playhead';

function deck(positionS: number, speed: number, loaded = true): DeckStatus {
  return { loaded, positionS, speed } as DeckStatus;
}

describe('followPlayhead', () => {
  it('closes a small gap a share at a time', () => {
    const before = { at: 1000, positionS: 10, speed: 1 };
    // 16 ms later the status is 4 ms behind where the playhead got to
    const next = followPlayhead(before, deck(10.012, 1), 1016)!;
    expect(next.positionS).toBeCloseTo(10.016 - 0.0004, 6);
    expect(next.at).toBe(1016);
  });

  it('takes a jump, a speed change or a new track as reported', () => {
    const before = { at: 1000, positionS: 10, speed: 1 };
    expect(followPlayhead(before, deck(42, 1), 1016)!.positionS).toBe(42);
    expect(followPlayhead(before, deck(10.016, 0.5), 1016)!.positionS).toBe(10.016);
    expect(followPlayhead(null, deck(3, 1), 1016)!.positionS).toBe(3);
  });

  it('draws nothing without a track', () => {
    expect(followPlayhead({ at: 0, positionS: 1, speed: 1 }, deck(0, 0, false))).toBeNull();
  });
});
